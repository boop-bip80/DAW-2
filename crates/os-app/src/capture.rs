// Copyright (C) 2026 ItsAlphaNeon
// Copyright (C) 2026 Samrat Dutta
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Audio input capture (One-click audio recording): the audio device's inputs into a WAV file.
//!
//! An ASIO driver's inputs come in its own callback, with its outputs (`device::asio`); any other device's inputs come
//! from a system input stream opened while recording (`InputStream`). Either way the callback pushes stereo frames into
//! one lock-free ring (`capture`), only while a recording is on, and a writer thread (`Recorder`) drains it to disk. At
//! the end the writer turns what it wrote into a WAV, leaving out the frames before the take's start (`Recorder::finish`).
// spec: ui/one-click-recording.md

use std::cell::UnsafeCell;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

/// One producer, one consumer ring of samples: the device's callback pushes, the writer thread pops; neither locks.
pub struct Ring {
    buf: Box<[UnsafeCell<f32>]>,
    /// Samples written and read so far (the slot is the count modulo the length).
    head: AtomicUsize,
    tail: AtomicUsize,
}

// Safety: a slot is written only by the producer before `head` moves past it, and read only by the consumer before
// `tail` moves past it; the counts are published with release / acquire.
unsafe impl Sync for Ring {}

impl Ring {
    pub fn new(len: usize) -> Ring {
        Ring { buf: (0..len.max(1)).map(|_| UnsafeCell::new(0.0)).collect(), head: AtomicUsize::new(0), tail: AtomicUsize::new(0) }
    }

    /// Pushes `n` samples, the `k`-th being `at(k)`, as far as there is room; returns how many went in. Producer only.
    pub fn push_with(&self, n: usize, mut at: impl FnMut(usize) -> f32) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let free = self.buf.len() - (head - self.tail.load(Ordering::Acquire));
        let n = n.min(free);
        for k in 0..n {
            // Safety: the slot is free (the consumer has read past it) and only this producer writes.
            unsafe { *self.buf[(head + k) % self.buf.len()].get() = at(k) };
        }
        self.head.store(head + n, Ordering::Release);
        n
    }

    /// Appends every sample waiting to `out`. Consumer only.
    pub fn pop_into(&self, out: &mut Vec<f32>) {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        // Safety: slots between tail and head were written and published by the producer.
        out.extend((tail..head).map(|i| unsafe { *self.buf[i % self.buf.len()].get() }));
        self.tail.store(head, Ordering::Release);
    }

    /// Empties it. Consumer only.
    pub fn clear(&self) {
        self.tail.store(self.head.load(Ordering::Acquire), Ordering::Release);
    }
}

/// The capture the device's callbacks feed: whether a recording is on, which inputs it takes, and what came in.
pub struct Capture {
    ring: Ring,
    on: AtomicBool,
    /// The first input (0-based) and whether its next one makes a stereo pair (`set_source`).
    first: AtomicU32,
    stereo: AtomicBool,
    /// Stereo frames that came in since `start` (the ones the ring had no room for included), and those lost.
    frames: AtomicU64,
    dropped: AtomicU64,
}

/// About 20 s of stereo at 48 kHz: the writer drains it every few milliseconds.
const RING: usize = 1 << 21;

/// The one capture.
pub fn capture() -> &'static Capture {
    static C: OnceLock<Capture> = OnceLock::new();
    C.get_or_init(|| Capture {
        ring: Ring::new(RING),
        on: AtomicBool::new(false),
        first: AtomicU32::new(0),
        stereo: AtomicBool::new(true),
        frames: AtomicU64::new(0),
        dropped: AtomicU64::new(0),
    })
}

impl Capture {
    /// Starts taking inputs `first` (and `first + 1` when `stereo`) from the next block.
    pub fn start(&self, first: u16, stereo: bool) {
        self.on.store(false, Ordering::Release);
        self.ring.clear();
        self.first.store(u32::from(first), Ordering::Relaxed);
        self.stereo.store(stereo, Ordering::Relaxed);
        self.frames.store(0, Ordering::Relaxed);
        self.dropped.store(0, Ordering::Relaxed);
        self.on.store(true, Ordering::Release);
    }

    pub fn stop(&self) {
        self.on.store(false, Ordering::Release);
    }

    pub fn on(&self) -> bool {
        self.on.load(Ordering::Acquire)
    }

    /// The inputs to take: the first, and whether it is a pair.
    pub fn source(&self) -> (usize, bool) {
        (self.first.load(Ordering::Relaxed) as usize, self.stereo.load(Ordering::Relaxed))
    }

    /// Stereo frames that came in since `start`.
    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Acquire)
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// A callback's block of `frames` frames: `at(channel, frame)` is an input's sample. Pushes the chosen inputs as
    /// stereo (one input on both sides). The audio callback calls it; it neither locks nor allocates.
    pub fn push(&self, frames: usize, at: impl Fn(usize, usize) -> f32) {
        if !self.on() {
            return;
        }
        let (first, stereo) = self.source();
        let second = if stereo { first + 1 } else { first };
        let went = self.ring.push_with(2 * frames, |k| at(if k % 2 == 0 { first } else { second }, k / 2));
        self.dropped.fetch_add(((2 * frames - went) / 2) as u64, Ordering::Relaxed);
        self.frames.fetch_add(frames as u64, Ordering::AcqRel);
    }

    fn drain(&self, out: &mut Vec<f32>) {
        self.ring.pop_into(out);
    }
}

/// An input to record from: a device (the ASIO driver, or a system input device) and its first channel, alone or as a
/// pair with the next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Input {
    pub device: String,
    pub asio: bool,
    pub first: u16,
    pub stereo: bool,
}

impl Input {
    /// "In 1 - In 2", "In 3" (spec §2 The button and the menu).
    pub fn label(&self) -> String {
        let n = self.first + 1;
        if self.stereo { format!("In {n} - In {}", n + 1) } else { format!("In {n}") }
    }

    /// As kept in `settings.txt` (`rec_input`).
    pub fn key(&self) -> String {
        format!("{}|{}|{}|{}", if self.asio { "asio" } else { "system" }, self.first, if self.stereo { "stereo" } else { "mono" }, self.device)
    }

    pub fn parse(key: &str) -> Option<Input> {
        let mut it = key.splitn(4, '|');
        let asio = it.next()? == "asio";
        let first = it.next()?.parse().ok()?;
        let stereo = it.next()? == "stereo";
        Some(Input { device: it.next()?.to_string(), asio, first, stereo })
    }

    /// The inputs of a device with `channels` inputs: its pairs, then each channel.
    pub fn of(device: &str, asio: bool, channels: u16) -> Vec<Input> {
        let d = |first, stereo| Input { device: device.to_string(), asio, first, stereo };
        let pairs = (0..channels / 2).map(|p| d(2 * p, true));
        pairs.chain((0..channels).map(|c| d(c, false))).collect()
    }
}

/// The input picked last (`rec_input`).
pub fn last_input() -> Option<Input> {
    crate::settings_file::get("rec_input").as_deref().and_then(Input::parse)
}

pub fn set_last_input(i: &Input) {
    crate::settings_file::write("rec_input", &i.key());
}

/// The devices to record from, with their input count: the ASIO driver when the output is one (its inputs once it has
/// opened; until then two are assumed and checked at the start), else the system's input devices.
pub fn devices() -> Vec<(String, bool, u16)> {
    let cfg = crate::audio_cfg::AudioCfg::current();
    if let Some(name) = cfg.asio_driver() {
        let opened = crate::device::last().filter(|d| d.host == crate::device::ASIO && d.name == name).map(|d| d.inputs);
        return vec![(name.to_string(), true, opened.unwrap_or(2))];
    }
    system_inputs()
}

/// The system's input devices and their channels (on Linux the sound server's, or ALSA's in exclusive mode, as the
/// output): the one Settings > Audio records from (else the system's default) first, asked for on its own so that it
/// is there even when the list leaves it out, then the others. A device that reports no format is listed with none.
fn system_inputs() -> Vec<(String, bool, u16)> {
    use cpal::traits::HostTrait;
    use crate::device::system_input::input_config;
    let cfg = crate::audio_cfg::AudioCfg::current();
    let channels = |d: &cpal::Device| input_config(d).inspect_err(|e| eprintln!("audio input {e}")).map_or(0, |c| c.channels());
    let mut v: Vec<(String, bool, u16)> =
        crate::audio_cfg::input_device(&cfg).map(|d| (crate::audio_cfg::device_name(&d), false, channels(&d))).into_iter().collect();
    let host = crate::audio_cfg::host(&cfg);
    for d in host.input_devices().into_iter().flatten().filter(crate::audio_cfg::listed) {
        let name = crate::audio_cfg::device_name(&d);
        if !v.iter().any(|(n, ..)| *n == name) {
            v.push((name, false, channels(&d)));
        }
    }
    v
}

/// A system input device's stream, recording while it is held.
pub struct InputStream {
    /// Taken when dropped: an ALSA device is let go of before the sound server is given it back.
    stream: Option<cpal::Stream>,
    alsa: bool,
    pub rate: u32,
}

impl Drop for InputStream {
    fn drop(&mut self) {
        drop(self.stream.take());
        if self.alsa {
            crate::device::heal_sound_server();
        }
    }
}

/// Opens system input device `name` in shared mode at its own rate, pushing into `capture`.
pub fn open_input(name: &str) -> Result<InputStream, String> {
    use cpal::traits::{HostTrait, StreamTrait};
    use cpal::SampleFormat;
    let host = crate::audio_cfg::host(&crate::audio_cfg::AudioCfg::current());
    let device = crate::audio_cfg::find(host.input_devices().map_err(|e| e.to_string())?, name).ok_or_else(|| format!("{name} is not there"))?;
    let supported = crate::device::system_input::input_config(&device)?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, &config),
        SampleFormat::F64 => build::<f64>(&device, &config),
        SampleFormat::I16 => build::<i16>(&device, &config),
        SampleFormat::I24 => build::<cpal::I24>(&device, &config),
        SampleFormat::I32 => build::<i32>(&device, &config),
        SampleFormat::U16 => build::<u16>(&device, &config),
        other => Err(format!("unsupported input sample format {other:?}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    let alsa = host.id().name() == "ALSA";
    Ok(InputStream { stream: Some(stream), alsa, rate: config.sample_rate })
}

fn build<T>(device: &cpal::Device, config: &cpal::StreamConfig) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    use cpal::traits::DeviceTrait;
    let channels = usize::from(config.channels).max(1);
    device
        .build_input_stream::<T, _, _>(
            config.clone(),
            move |data: &[T], _| {
                // An input past the device's channels is its last one.
                capture().push(data.len() / channels, |c, f| data[f * channels + c.min(channels - 1)].to_sample::<f32>());
            },
            |e| eprintln!("audio input stream: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

/// The take's file name: "<project>_<YYYY-MM-DD HH-MM-SS>_REC.wav" (spec §2 The take).
pub fn file_name(project: &str, now: chrono::NaiveDateTime) -> String {
    format!("{project}_{}_REC.wav", now.format("%Y-%m-%d %H-%M-%S"))
}

/// The writer: drains the capture into `<file>.part` until finished, then makes the WAV.
pub struct Recorder {
    stop: Arc<AtomicBool>,
    /// The first frame of the take (frames before it are left out), set before `finish`.
    head: Arc<AtomicI64>,
    /// The WAV's rate, set before `finish` when it was not known at the start (an ASIO driver's).
    rate: Arc<AtomicU32>,
    /// Kept here too, so even a panicking writer reports where its recoverable data is.
    part: PathBuf,
    thread: Option<std::thread::JoinHandle<Result<(PathBuf, u64), String>>>,
}

/// Each line is a complete recovery checkpoint. Appending preserves the previous checkpoint if an update is torn.
/// The raw file is interleaved little-endian f32 stereo; its complete frames are its byte length divided by eight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Recovery {
    version: u8,
    channels: u8,
    rate: u32,
    head: i64,
}

fn recovery_path(part: &Path) -> PathBuf {
    part.with_extension("part.jsonl")
}

fn kept_error(part: &Path, error: impl std::fmt::Display) -> String {
    format!("{error}. Raw audio retained: {}; recovery metadata: {}. Retry with --recover-recording and the raw-audio path.", part.display(), recovery_path(part).display())
}

fn checkpoint(file: &mut std::fs::File, value: Recovery) -> io::Result<()> {
    serde_json::to_writer(&mut *file, &value)?;
    file.write_all(b"\n")?;
    file.sync_all()
}

/// Reserve the actual on-disk identity, after callers have sanitized their names. Neither a previous take nor an
/// abandoned raw take or recovery journal is reused. create_new also makes simultaneous recorders safe.
fn reserve_take(requested: &Path, rate: u32) -> io::Result<(PathBuf, PathBuf, std::fs::File, std::fs::File)> {
    let stem = requested.file_stem().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "take needs a file name"))?;
    for number in 0u32..100_000 {
        let file = if number == 0 {
            requested.to_path_buf()
        } else {
            let mut name = stem.to_os_string();
            name.push(format!(" ({}).wav", number + 1));
            requested.with_file_name(name)
        };
        let part = file.with_extension("wav.part");
        let meta = recovery_path(&part);
        if file.try_exists()? || meta.try_exists()? {
            continue;
        }
        let raw = match std::fs::File::create_new(&part) {
            Ok(raw) => raw,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        };
        let mut journal = match std::fs::File::create_new(&meta) {
            Ok(journal) => journal,
            Err(e) => {
                drop(raw);
                let _ = std::fs::remove_file(&part);
                if e.kind() == io::ErrorKind::AlreadyExists {
                    continue;
                }
                return Err(e);
            }
        };
        if let Err(e) = checkpoint(&mut journal, Recovery { version: 1, channels: 2, rate, head: 0 }) {
            drop((raw, journal));
            let _ = std::fs::remove_file(&part);
            let _ = std::fs::remove_file(&meta);
            return Err(e);
        }
        return Ok((file, part, raw, journal));
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "too many recordings with this name"))
}

impl Recorder {
    /// Starts writing what the capture takes in into `file` (a WAV at `rate` once finished).
    pub fn start(file: PathBuf, rate: u32) -> Result<Recorder, String> {
        Recorder::start_from(file, rate, |out| capture().drain(out))
    }

    /// Starts writing what `drain` hands over (stereo frames, appended to its argument) into `file`.
    pub fn start_from(file: PathBuf, rate: u32, mut drain: impl FnMut(&mut Vec<f32>) + Send + 'static) -> Result<Recorder, String> {
        if let Some(d) = file.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        // Reject invalid rates before reserving files or starting a writer.
        os_engine::wav::write_f32_stereo_header(&mut io::sink(), rate, 0).map_err(|e| e.to_string())?;
        let (file, part, raw, mut journal) = reserve_take(&file, rate).map_err(|e| format!("{}: {e}", file.display()))?;
        let stop = Arc::new(AtomicBool::new(false));
        let head = Arc::new(AtomicI64::new(0));
        let mut saved = Recovery { version: 1, channels: 2, rate, head: 0 };
        let rate = Arc::new(AtomicU32::new(rate));
        let (s, h, r) = (stop.clone(), head.clone(), rate.clone());
        let thread_part = part.clone();
        let thread = std::thread::Builder::new()
            .name("audio recorder".into())
            .spawn(move || {
                let part = thread_part;
                let mut w = BufWriter::new(raw);
                let mut buf = Vec::with_capacity(1 << 16);
                let written = (|| -> io::Result<()> {
                    loop {
                        let last = s.load(Ordering::Acquire);
                        let current = Recovery { rate: r.load(Ordering::Acquire), head: h.load(Ordering::Acquire), ..saved };
                        if current != saved || last {
                            checkpoint(&mut journal, current)?;
                            saved = current;
                        }
                        buf.clear();
                        drain(&mut buf);
                        if buf.len() % 2 != 0 {
                            return Err(io::Error::new(io::ErrorKind::InvalidData, "capture supplied an incomplete stereo frame"));
                        }
                        for x in &buf {
                            w.write_all(&x.to_le_bytes())?;
                        }
                        if last {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    w.flush()?;
                    w.get_ref().sync_all()
                })();
                if let Err(e) = written {
                    // Preserve everything the OS will still accept, including a buffered tail on a recoverable error.
                    let _ = w.flush();
                    let _ = w.get_ref().sync_all();
                    return Err(kept_error(&part, e));
                }
                drop((w, journal));
                finalize_take(&part, &file, saved).map_err(|e| kept_error(&part, e))
            })
            .map_err(|e| kept_error(&part, e))?;
        Ok(Recorder { stop, head, rate, part, thread: Some(thread) })
    }

    /// The take starts at frame `head` of what came in (negative: that much silence first).
    pub fn set_head(&self, head: i64) {
        self.head.store(head, Ordering::Release);
    }

    pub fn set_rate(&self, rate: u32) {
        self.rate.store(rate, Ordering::Release);
    }

    /// Stops taking in; the WAV is made on the writer thread (`done`, then `join`).
    pub fn finish(&self) {
        self.stop.store(true, Ordering::Release);
    }

    pub fn done(&self) -> bool {
        self.thread.as_ref().is_none_or(|t| t.is_finished())
    }

    /// The WAV and its frames.
    pub fn join(mut self) -> Result<(PathBuf, u64), String> {
        self.thread.take().ok_or("already joined")?.join().map_err(|_| kept_error(&self.part, "the recorder stopped unexpectedly"))?
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        // Do not leave an abandoned recording thread running, and never block the UI while it drains and finalizes.
        self.stop.store(true, Ordering::Release);
    }
}

fn finalize_take(part: &Path, file: &Path, recovery: Recovery) -> io::Result<(PathBuf, u64)> {
    let bytes = std::fs::metadata(part)?.len();
    if bytes % 8 != 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "raw audio ends in an incomplete stereo frame"));
    }
    let frames = to_wav(part, file, recovery.rate, recovery.head, bytes / 8)?;
    // Only a complete, flushed and synced WAV lets us remove the original. A cleanup failure is harmless: it leaves
    // a redundant recovery copy, never a missing take.
    if std::fs::remove_file(part).is_ok() {
        let _ = std::fs::remove_file(recovery_path(part));
    }
    Ok((file.to_path_buf(), frames))
}

/// Retry a retained take after correcting a full disk or other finalization failure. A torn final checkpoint is
/// ignored in favour of the previous complete line. An existing destination is never replaced.
pub fn recover_take(part: &Path) -> Result<(PathBuf, u64), String> {
    let read = || -> io::Result<(PathBuf, u64)> {
        if part.extension().is_none_or(|e| e != "part") {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "expected a .wav.part recording"));
        }
        let file = std::fs::File::open(recovery_path(part))?;
        let mut recovery = None;
        for line in BufReader::new(file).lines() {
            let line = line?;
            if let Ok(value) = serde_json::from_str::<Recovery>(&line) {
                if value.version == 1 && value.channels == 2 {
                    recovery = Some(value);
                }
            }
        }
        let value = recovery.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no complete recovery checkpoint"))?;
        let original = part.with_extension("");
        let stem = original.file_stem().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "recording has no file name"))?;
        for number in 0u64.. {
            let output = if number == 0 {
                original.clone()
            } else {
                let mut name = stem.to_os_string();
                name.push(format!(" (recovered {number}).wav"));
                original.with_file_name(name)
            };
            match finalize_take(part, &output, value) {
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                result => return result,
            }
        }
        unreachable!("recording generation space exhausted")
    };
    read().map_err(|e| kept_error(part, e))
}

/// Makes WAV `wav` (32-bit float stereo at `rate`) of the raw frames in `part` from frame `head` on; returns its frames.
fn to_wav(part: &Path, wav: &Path, rate: u32, head: i64, frames: u64) -> std::io::Result<u64> {
    let skip = head.max(0) as u64;
    let pad = if head < 0 { head.unsigned_abs() } else { 0 };
    let n = frames.saturating_sub(skip).checked_add(pad).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "recording length overflow"))?;
    let mut header = Vec::with_capacity(44);
    os_engine::wav::write_f32_stereo_header(&mut header, rate, n)?;
    let bytes = frames.checked_mul(8).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "raw recording length overflow"))?;
    let mut r = std::fs::File::open(part)?;
    if r.metadata()?.len() != bytes {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "raw recording length changed or is incomplete"));
    }
    r.seek(SeekFrom::Start(skip.min(frames) * 8))?;
    // Convert into a sibling and publish exclusively only when the complete WAV is flushed and synced.
    // A crash during conversion leaves the original raw audio and journal available for recovery.
    crate::durable_file::create_with(wav, |output| {
        let mut out = BufWriter::new(output);
        out.write_all(&header)?;
        let silence = [0u8; 8192];
        let mut left = pad * 8; // The checked RIFF header above bounds this multiplication.
        while left > 0 {
            let count = left.min(silence.len() as u64) as usize;
            out.write_all(&silence[..count])?;
            left -= count as u64;
        }
        let expected = frames.saturating_sub(skip) * 8;
        if io::copy(&mut r.take(expected), &mut out)? != expected {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "raw recording ended during conversion"));
        }
        out.flush()
    })?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_keeps_order_and_drops_what_has_no_room() {
        let r = Ring::new(4);
        assert_eq!(r.push_with(3, |k| k as f32), 3);
        let mut out = Vec::new();
        r.pop_into(&mut out);
        assert_eq!(out, [0.0, 1.0, 2.0]);
        assert_eq!(r.push_with(6, |k| 10.0 + k as f32), 4, "four slots");
        out.clear();
        r.pop_into(&mut out);
        assert_eq!(out, [10.0, 11.0, 12.0, 13.0], "wrapped around");
    }

    #[test]
    fn inputs_their_labels_and_keys() {
        let v = Input::of("Driver", true, 3);
        let labels: Vec<String> = v.iter().map(Input::label).collect();
        assert_eq!(labels, ["In 1 - In 2", "In 1", "In 2", "In 3"]);
        assert_eq!(Input::parse(&v[0].key()), Some(v[0].clone()));
        assert_eq!(Input::parse(&v[3].key()), Some(v[3].clone()));
        let t = chrono::NaiveDate::from_ymd_opt(2026, 10, 19).unwrap().and_hms_opt(21, 48, 43).unwrap();
        assert_eq!(file_name("untitled", t), "untitled_2026-10-19 21-48-43_REC.wav");
    }

    /// The WAV leaves out the frames before the head, or starts with silence for a negative one.
    #[test]
    fn a_wav_from_the_head_on() {
        let dir = std::env::temp_dir().join(format!("lemon_capture_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let part = dir.join("t.wav.part");
        let raw: Vec<u8> = (0..8).flat_map(|i| (i as f32).to_le_bytes()).collect();
        std::fs::write(&part, raw).unwrap();
        let wav = dir.join("t.wav");
        assert_eq!(to_wav(&part, &wav, 48000, 1, 4).unwrap(), 3);
        let b = std::fs::read(&wav).unwrap();
        let data: Vec<f32> = b[44..].chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
        assert_eq!(data, [2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
        let wav = dir.join("padded.wav");
        assert_eq!(to_wav(&part, &wav, 48000, -1, 4).unwrap(), 5);
        let b = std::fs::read(&wav).unwrap();
        assert_eq!(b.len(), 44 + 5 * 8);
        assert_eq!(&b[44..52], &[0u8; 8]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn take_dir(name: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("lemon_take_{name}_{}_{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn writer(file: PathBuf, value: f32) -> Recorder {
        let mut samples = vec![value; 32];
        Recorder::start_from(file, 48000, move |out| out.append(&mut samples)).unwrap()
    }

    #[test]
    fn simultaneous_names_and_existing_takes_never_overwrite_audio() {
        let dir = take_dir("collision");
        let requested = dir.join("session.v1_same.wav");
        std::fs::write(&requested, b"previous take").unwrap();
        let a = writer(requested.clone(), 0.25);
        let b = writer(requested.clone(), 0.75);
        a.finish();
        b.finish();
        let (a, an) = a.join().unwrap();
        let (b, bn) = b.join().unwrap();
        assert_ne!(a, b);
        assert_ne!(a, requested);
        assert!(a.file_name().unwrap().to_string_lossy().starts_with("session.v1_same ("));
        assert_eq!((an, bn), (16, 16));
        assert_eq!(std::fs::read(requested).unwrap(), b"previous take");
        assert_eq!(&std::fs::read(a).unwrap()[44..48], &0.25f32.to_le_bytes());
        assert_eq!(&std::fs::read(b).unwrap()[44..48], &0.75f32.to_le_bytes());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_finalization_keeps_audio_and_metadata_and_can_be_retried() {
        let dir = take_dir("recovery");
        let wav = dir.join("take.wav");
        let w = writer(wav.clone(), 0.5);
        let part = w.part.clone();
        // A competing writer wins the destination after this recorder reserved its raw file.
        std::fs::write(&wav, b"unrelated recording").unwrap();
        w.set_head(3);
        w.set_rate(44100);
        w.finish();
        let error = w.join().unwrap_err();
        assert!(error.contains(&part.display().to_string()), "{error}");
        assert_eq!(std::fs::metadata(&part).unwrap().len(), 32 * 4);
        assert_eq!(std::fs::read(&wav).unwrap(), b"unrelated recording");
        let journal = recovery_path(&part);
        // Simulate a power loss in the middle of the next journal checkpoint.
        std::fs::OpenOptions::new().append(true).open(&journal).unwrap().write_all(b"{\"rate\":").unwrap();
        let recovered = dir.join("take (recovered 1).wav");
        assert_eq!(recover_take(&part).unwrap(), (recovered.clone(), 13));
        assert_eq!(std::fs::read(wav).unwrap(), b"unrelated recording");
        let b = std::fs::read(recovered).unwrap();
        assert_eq!(u32::from_le_bytes(b[24..28].try_into().unwrap()), 44100);
        assert_eq!(b.len(), 44 + 13 * 8);
        assert!(!part.exists() && !journal.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_conversion_and_riff_limit_leave_original_bytes_untouched() {
        let dir = take_dir("invalid");
        let part = dir.join("take.wav.part");
        let wav = dir.join("take.wav");
        let original = 0.125f32.to_le_bytes().repeat(4);
        std::fs::write(&part, &original).unwrap();
        for (head, frames) in [(0, 3), (i64::MIN, 2), (0, u64::MAX)] {
            assert!(to_wav(&part, &wav, 48000, head, frames).is_err());
            assert!(!wav.exists());
            assert_eq!(std::fs::read(&part).unwrap(), original);
        }
        let w = writer(wav.clone(), 0.5);
        let raw = w.part.clone();
        w.set_head(-((u32::MAX as i64 / 8) + 1));
        w.finish();
        let error = w.join().unwrap_err();
        assert!(error.contains("RIFF WAV limit"), "{error}");
        assert!(raw.exists() && recovery_path(&raw).exists());
        assert!(!raw.with_extension("").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A real input device takes in (needs one: `cargo test -- --ignored`).
    #[test]
    #[ignore]
    fn a_system_input_takes_in() {
        let (name, ..) = system_inputs().into_iter().next().expect("an input device");
        capture().start(0, true);
        let stream = open_input(&name).expect("opens");
        let rate = stream.rate;
        std::thread::sleep(std::time::Duration::from_millis(500));
        drop(stream);
        capture().stop();
        let frames = capture().frames();
        assert!(frames > 0, "{name}: nothing came in");
        eprintln!("{name}: {frames} frames in 0.5 s at {rate} Hz");
    }

    /// Stereo takes a pair; one input goes to both sides.
    #[test]
    fn a_block_goes_in_as_stereo() {
        let c = Capture { ring: Ring::new(64), on: AtomicBool::new(false), first: AtomicU32::new(0), stereo: AtomicBool::new(true), frames: AtomicU64::new(0), dropped: AtomicU64::new(0) };
        c.push(2, |ch, f| (ch * 10 + f) as f32);
        assert_eq!(c.frames(), 0, "off");
        c.start(1, true);
        c.push(2, |ch, f| (ch * 10 + f) as f32);
        c.start(2, false);
        c.push(1, |ch, f| (ch * 10 + f) as f32);
        let mut out = Vec::new();
        c.drain(&mut out);
        assert_eq!(out, [20.0, 20.0], "start empties what came before");
        assert_eq!(c.frames(), 1);
    }
}
