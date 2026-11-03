// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! FILE > Export: the export's choices (kept in settings.txt), and what is done with the rendered mix: the tail, the
//! sample rate, the channels, dither, and the WAV, FLAC, MP3 and Ogg Vorbis files with their markers. The render
//! itself runs on the audio thread (`transport`); the form is `export_form`.
// spec: audio/export.md

use std::path::{Path, PathBuf};

use crate::settings_file as sf;

/// The formats, in the form's order.
pub const FORMATS: [&str; 4] = ["WAV", "FLAC", "MP3", "OGG"];
pub const EXTENSIONS: [&str; 4] = ["wav", "flac", "mp3", "ogg"];
pub const WAV: usize = 0;
pub const FLAC: usize = 1;
pub const MP3: usize = 2;
pub const OGG: usize = 3;

/// The WAV depths: 16 and 24-bit integers, 32-bit float.
pub const WAV_DEPTHS: [&str; 3] = ["16-bit", "24-bit", "32-bit float"];
pub const FLAC_DEPTHS: [&str; 2] = ["16-bit", "24-bit"];
/// The MP3 and OGG bitrates, kbps.
pub const BITRATES: [u32; 8] = [64, 96, 128, 160, 192, 224, 256, 320];
/// The sample rates besides the device's (index 0).
pub const RATES: [u32; 5] = [44100, 48000, 88200, 96000, 192000];
pub const CHANNELS: [&str; 4] = ["Stereo", "Mono (merged)", "Mono (left)", "Mono (right)"];
/// The resampling choices: the window's width in points (2: linear); 27 is what playback uses.
pub const POINTS: [u32; 8] = [2, 8, 16, 27, 64, 128, 256, 512];
pub const TAILS: [&str; 4] = ["Cut", "Ring (legacy)", "Wrap", "10 s tail"];
/// Explicit, bounded default: quiet gaps do not truncate future delayed audio within this interval.
pub const FIXED_TAIL_SECONDS: u64 = 10;

/// What happens to the sound after the song's end (spec §3 "Tail").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tail {
    /// The file ends at the song's end.
    Cut,
    /// Until the first quiet tick (the tail rule).
    Ring,
    /// What rang past the end is added to the file's start; the file is the song's length.
    Wrap,
    /// Always render ten seconds past the selected material, including silent gaps before delayed effects.
    Fixed,
}

/// The export's choices, as kept in settings.txt.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// Formats ticked, by `FORMATS`.
    pub formats: [bool; 4],
    /// Index in `WAV_DEPTHS`, `FLAC_DEPTHS`.
    pub wav_depth: usize,
    pub flac_depth: usize,
    /// FLAC compression level, 0..=8.
    pub flac_level: u8,
    /// MP3 and OGG bitrates, kbps (`BITRATES`).
    pub mp3_kbps: u32,
    pub ogg_kbps: u32,
    /// The file's sample rate; 0: the device's.
    pub rate: u32,
    /// Index in `CHANNELS`.
    pub channels: usize,
    /// The resampling window in points (`POINTS`).
    pub points: u32,
    pub tail: Tail,
    pub dither: bool,
    /// Hosted plugins render in their offline mode.
    pub hq: bool,
    pub split: bool,
    pub split_master: bool,
    pub insert_fx: bool,
    pub master_fx: bool,
    pub markers: bool,
    pub loops: bool,
    pub note_markers: bool,
    pub tempo: bool,
    /// The file manager opens on the first file when done.
    pub reveal: bool,
    /// MP3 files start with a LAME / Info frame giving the encoder's delay and padding, so gapless players trim them.
    /// FL and Lemon Studio play every frame, the Info frame as 1152 frames of silence (audio/sampler.md), so off is
    /// a file that sits earlier in a DAW and on is one that plays seamlessly elsewhere.
    pub mp3_gapless: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            formats: [true, false, false, false],
            wav_depth: 2,
            flac_depth: 0,
            flac_level: 5,
            mp3_kbps: 192,
            ogg_kbps: 192,
            rate: 0,
            channels: 0,
            points: 27,
            tail: Tail::Fixed,
            dither: false,
            hq: true,
            split: false,
            split_master: true,
            insert_fx: true,
            master_fx: true,
            markers: true,
            loops: true,
            note_markers: false,
            tempo: false,
            reveal: false,
            mp3_gapless: true,
        }
    }
}

/// The settings.txt keys of the switches, in `Options::flags` order.
const FLAG_KEYS: [&str; 12] = [
    "export_dither",
    "export_hq",
    "export_split",
    "export_split_master",
    "export_insert_fx",
    "export_master_fx",
    "export_markers",
    "export_loops",
    "export_note_markers",
    "export_tempo",
    "export_reveal",
    "export_mp3_gapless",
];

impl Options {
    /// The switches, in `FLAG_KEYS` order.
    fn flags(&mut self) -> [&mut bool; 12] {
        [
            &mut self.dither,
            &mut self.hq,
            &mut self.split,
            &mut self.split_master,
            &mut self.insert_fx,
            &mut self.master_fx,
            &mut self.markers,
            &mut self.loops,
            &mut self.note_markers,
            &mut self.tempo,
            &mut self.reveal,
            &mut self.mp3_gapless,
        ]
    }

    /// The last export's choices (the defaults where none is kept).
    pub fn load() -> Options {
        let mut o = Options::default();
        let num = |k: &str| sf::get(k).and_then(|v| v.trim().parse::<u32>().ok());
        if let Some(m) = num("export_formats").filter(|m| m & 15 != 0) {
            o.formats = std::array::from_fn(|i| m & (1 << i) != 0);
        }
        o.wav_depth = num("export_wav_depth").map_or(o.wav_depth, |v| (v as usize).min(WAV_DEPTHS.len() - 1));
        o.flac_depth = num("export_flac_depth").map_or(o.flac_depth, |v| (v as usize).min(FLAC_DEPTHS.len() - 1));
        o.flac_level = num("export_flac_level").map_or(o.flac_level, |v| v.min(8) as u8);
        o.mp3_kbps = num("export_mp3").filter(|v| BITRATES.contains(v)).unwrap_or(o.mp3_kbps);
        o.ogg_kbps = num("export_ogg").filter(|v| BITRATES.contains(v)).unwrap_or(o.ogg_kbps);
        o.rate = num("export_rate").filter(|v| *v == 0 || RATES.contains(v)).unwrap_or(o.rate);
        o.channels = num("export_channels").map_or(o.channels, |v| (v as usize).min(CHANNELS.len() - 1));
        o.points = num("export_points").filter(|v| POINTS.contains(v)).unwrap_or(o.points);
        o.tail = match num("export_tail") {
            Some(0) => Tail::Cut,
            Some(1) => Tail::Ring,
            Some(2) => Tail::Wrap,
            _ => Tail::Fixed,
        };
        for (k, f) in FLAG_KEYS.iter().zip(o.flags()) {
            *f = sf::flag(k, *f);
        }
        o
    }

    /// Keeps these choices for the next export.
    pub fn save(&self) {
        let mask: u32 = self.formats.iter().enumerate().filter(|(_, on)| **on).map(|(i, _)| 1 << i).sum();
        sf::write("export_formats", &mask.to_string());
        sf::write("export_wav_depth", &self.wav_depth.to_string());
        sf::write("export_flac_depth", &self.flac_depth.to_string());
        sf::write("export_flac_level", &self.flac_level.to_string());
        sf::write("export_mp3", &self.mp3_kbps.to_string());
        sf::write("export_ogg", &self.ogg_kbps.to_string());
        sf::write("export_rate", &self.rate.to_string());
        sf::write("export_channels", &self.channels.to_string());
        sf::write("export_points", &self.points.to_string());
        sf::write("export_tail", &(self.tail as u8).to_string());
        let mut o = self.clone();
        for (k, f) in FLAG_KEYS.iter().zip(o.flags()) {
            sf::set_flag(k, *f);
        }
    }

    /// The formats ticked, by index.
    pub fn ticked(&self) -> impl Iterator<Item = usize> + '_ {
        (0..FORMATS.len()).filter(|&i| self.formats[i])
    }

    /// Whether format `f` is written as integers (dither applies).
    pub fn integer(&self, f: usize) -> bool {
        match f {
            WAV => self.wav_depth < 2,
            FLAC => true,
            _ => false,
        }
    }

    /// The file's channel count.
    pub fn channel_count(&self) -> u16 {
        if self.channels == 0 { 2 } else { 1 }
    }

    /// Bytes a second of format `f` at `rate`, for the size estimate.
    pub fn bytes_per_second(&self, f: usize, rate: u32) -> f64 {
        let ch = f64::from(self.channel_count());
        match f {
            WAV => f64::from(rate) * ch * [2.0, 3.0, 4.0][self.wav_depth],
            // About 60 % of the PCM size (a typical mix; a Lemon Studio estimate).
            FLAC => f64::from(rate) * ch * [2.0, 3.0][self.flac_depth] * 0.6,
            MP3 => f64::from(self.mp3_kbps) * 125.0,
            _ => f64::from(self.ogg_kbps) * 125.0,
        }
    }
}

/// A marker in the file: its frame (at the render's rate, from the file's start) and name.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    pub frame: u64,
    pub name: String,
}

/// One export, as the audio thread needs it: the choices, where the files go, and what the project says about
/// markers, the loop, the tempo and the mixer tracks' names.
#[derive(Debug, Clone)]
pub struct Job {
    pub options: Options,
    /// The files' folder and name (without extension).
    pub folder: PathBuf,
    pub name: String,
    /// Render only ticks from..to (Range Selection).
    pub span: Option<(u64, u64)>,
    pub markers: Vec<Marker>,
    /// The loop (start, end frames), for the WAV `smpl` chunk; None: the whole file.
    pub loop_frames: Option<(u64, u64)>,
    /// Tempo (beats a minute) and the beats rendered, for the WAV `acid` chunk.
    pub tempo: (f64, f64),
    /// Mixer track names by track number (master first).
    pub track_names: Vec<String>,
    /// The mixer track menu's File ▸ Render to wave file...: these tracks' outputs are the files, without the mix
    /// (empty: an export of the mix).
    pub tracks: Vec<u16>,
}

impl Job {
    /// A 32-bit float WAV of the mix at `path` and nothing else (tests).
    #[cfg(test)]
    pub fn wav(path: &Path) -> Job {
        let options = Options { markers: false, loops: false, ..Options::default() };
        Job {
            options,
            folder: path.parent().map(Path::to_path_buf).unwrap_or_default(),
            name: path.file_stem().map_or_else(|| "Untitled".into(), |n| n.to_string_lossy().into_owned()),
            span: None,
            markers: Vec::new(),
            loop_frames: None,
            tempo: (120.0, 0.0),
            track_names: Vec::new(),
            tracks: Vec::new(),
        }
    }

    /// The mix's file of format `f`.
    pub fn file(&self, f: usize) -> PathBuf {
        self.folder.join(format!("{}.{}", self.name, EXTENSIONS[f]))
    }

    /// Mixer track `t`'s name as a file name takes it (the characters one cannot hold as `_`).
    fn track_name(&self, t: usize) -> String {
        let name = self.track_names.get(t).cloned().unwrap_or_else(|| if t == 0 { "Master".into() } else { format!("Bus {t}") });
        name.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect()
    }

    /// The folder the split mixer tracks go to.
    pub fn stems_folder(&self) -> PathBuf {
        self.folder.join(format!("{} stems", self.name))
    }
}

/// Below this (-96 dB) a split track is silent and not written.
const SILENT: f32 = 1.584_893_1e-5;

/// Buffered codecs/resampling remain explicitly bounded until their incremental encoders are qualified. The
/// default native-rate float WAV path below does not keep the song or all stems in memory.
pub const BUFFERED_SAMPLE_LIMIT: usize = 16 * 1024 * 1024;

pub fn can_stream(job: &Job, rate: u32) -> bool {
    let o = &job.options;
    o.formats == [true,false,false,false] && o.wav_depth == 2 && (o.rate == 0 || o.rate == rate) && o.tail != Tail::Wrap
}

struct WaveSpool {
    source: Option<usize>,
    path: PathBuf,
    raw: PathBuf,
    writer: Option<std::io::BufWriter<std::fs::File>>,
    frames: u64,
    loud: bool,
    omit_silent: bool,
    meta: Meta,
}

impl Drop for WaveSpool {
    fn drop(&mut self) {
        self.writer.take();
        let _ = std::fs::remove_file(&self.raw);
    }
}

impl WaveSpool {
    fn new(source: Option<usize>, path: PathBuf, omit_silent: bool, meta: Meta) -> Result<Self,String> {
        use std::sync::atomic::{AtomicU64,Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        let (raw,file) = loop {
            let n = NEXT.fetch_add(1,Ordering::Relaxed);
            let raw = parent.join(format!(".lemon-render-{}-{n}.raw",std::process::id()));
            match std::fs::File::create_new(&raw) {
                Ok(file) => break (raw,file),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("{}: {e}",raw.display())),
            }
        };
        Ok(Self { source,path,raw,writer:Some(std::io::BufWriter::with_capacity(64*1024,file)),frames:0,loud:false,omit_silent,meta })
    }

    fn push(&mut self, samples: &[f32], channels: usize, limit: Option<u64>) -> Result<(),String> {
        use std::io::Write;
        let allowed = limit.map_or(samples.len()/2, |n| (n.saturating_sub(self.frames) as usize).min(samples.len()/2));
        let writer = self.writer.as_mut().ok_or("render spool already closed")?;
        for f in samples.chunks_exact(2).take(allowed) {
            self.loud |= f.iter().any(|s| s.abs() >= SILENT);
            if channels == 0 {
                writer.write_all(&f[0].to_le_bytes()).and_then(|_|writer.write_all(&f[1].to_le_bytes())).map_err(|e|e.to_string())?;
            } else {
                let mono = [(f[0]+f[1])*0.5,f[0],f[1]][channels-1];
                writer.write_all(&mono.to_le_bytes()).map_err(|e|e.to_string())?;
            }
        }
        self.frames += allowed as u64;
        Ok(())
    }

    fn prepare(mut self, options: &Options, rate: u32) -> Result<Option<PreparedWave>,String> {
        if self.omit_silent && !self.loud { return Ok(None); }
        self.writer.take().ok_or("render spool already closed")?.into_inner().map_err(|e|e.to_string())?.sync_all().map_err(|e|e.to_string())?;
        let channels = options.channel_count();
        let bytes = self.frames.checked_mul(u64::from(channels)*4).ok_or("WAV length overflow")?;
        if !options.markers && !options.note_markers { self.meta.markers.clear(); }
        if !options.loops { self.meta.loop_frames = None; }
        else if self.meta.loop_frames.is_none() { self.meta.loop_frames=Some((0,self.frames)); }
        if !options.tempo { self.meta.tempo=(0.0,0.0); }
        let mut extra=Vec::new();
        append_wav_metadata(&mut extra,self.frames,rate,&self.meta);
        let riff = 36u64.checked_add(bytes).and_then(|n|n.checked_add(extra.len() as u64)).filter(|&n|n<=u64::from(u32::MAX)).ok_or("too long for RIFF WAV; split the export into shorter ranges")?;
        let mut header=wav_bytes(&[],channels,rate,32,false,&Meta::default())?;
        header[4..8].copy_from_slice(&(riff as u32).to_le_bytes());
        header[40..44].copy_from_slice(&(bytes as u32).to_le_bytes());
        if std::fs::metadata(&self.raw).map_err(|e|e.to_string())?.len()!=bytes {
            return Err(format!("{}: render spool length changed",self.raw.display()));
        }
        Ok(Some(PreparedWave { spool:self,header,extra,bytes }))
    }
}

/// A finished, flushed spool, with a validated RIFF header. Nothing is published until *all* requested outputs
/// have passed this preparation step and the previous files have been snapshotted for rollback.
struct PreparedWave { spool:WaveSpool, header:Vec<u8>, extra:Vec<u8>, bytes:u64 }
impl PreparedWave {
    fn publish(&self) -> Result<(),String> {
        self.publish_with_cancel(&||false)
    }
    fn publish_with_cancel(&self, cancelled:&impl Fn()->bool) -> Result<(),String> {
        use std::io::{Read,Write};
        crate::durable_file::replace_with(&self.spool.path,|out| {
            out.write_all(&self.header)?;
            let mut input=std::fs::File::open(&self.spool.raw)?;
            let mut copied=0u64;
            let mut block=[0u8;64*1024];
            loop {
                if cancelled() {return Err(std::io::Error::new(std::io::ErrorKind::Interrupted,"Export cancelled"));}
                let n=input.read(&mut block)?;
                if n==0 {break;}
                out.write_all(&block[..n])?;
                copied+=n as u64;
            }
            if copied!=self.bytes {return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof,"render spool length changed"));}
            out.write_all(&self.extra)
        }).map_err(|e|format!("{}: {e}",self.spool.path.display()))
    }
}

/// A pre-publish snapshot of an output's previous generation. It is created before the *first* stem is replaced
/// so any normal I/O error partway through publication can restore every attempted destination. A process crash
/// during the renames is not a transactional multi-file commit; that still needs a generation manifest.
struct PreviousWave { target:PathBuf, copy:Option<PathBuf>, permissions:Option<std::fs::Permissions> }
impl PreviousWave {
    fn capture(target: &Path) -> Result<Self,String> {
        use std::io::{ErrorKind,Write};
        use std::sync::atomic::{AtomicU64,Ordering};
        static NEXT:AtomicU64=AtomicU64::new(0);
        let metadata=match std::fs::symlink_metadata(target) {
            Ok(m) => Some(m),
            Err(e) if e.kind()==ErrorKind::NotFound => None,
            Err(e) => return Err(format!("{}: {e}",target.display())),
        };
        if metadata.as_ref().is_some_and(|m|!m.file_type().is_file()) {
            return Err(format!("{}: export target is not a regular file",target.display()));
        }
        let permissions=metadata.map(|m|m.permissions());
        let mut copy=None;
        if permissions.is_some() {
            let parent=target.parent().unwrap_or_else(||Path::new("."));
            let (path,mut out)=loop {
                let n=NEXT.fetch_add(1,Ordering::Relaxed);
                let path=parent.join(format!(".lemon-render-original-{}-{n}.backup",std::process::id()));
                match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                    Ok(f) => break (path,f),
                    Err(e) if e.kind()==ErrorKind::AlreadyExists => continue,
                    Err(e) => return Err(format!("{}: {e}",path.display())),
                }
            };
            let snapshot=(||->std::io::Result<()> {
                let mut old=std::fs::File::open(target)?;
                std::io::copy(&mut old,&mut out)?;
                out.flush()?;
                out.sync_all()
            })();
            if let Err(e)=snapshot {
                drop(out);
                let _=std::fs::remove_file(&path);
                return Err(format!("{}: cannot snapshot before export: {e}",target.display()));
            }
            drop(out);
            copy=Some(path);
        }
        Ok(Self {target:target.to_path_buf(),copy,permissions})
    }

    fn restore(&self) -> Result<(),String> {
        match &self.copy {
            Some(old) => {
                crate::durable_file::replace_with(&self.target,|out| {
                    let mut saved=std::fs::File::open(old)?;
                    std::io::copy(&mut saved,out).map(|_|())
                }).map_err(|e|format!("{}: restore failed: {e}",self.target.display()))?;
                if let Some(permissions)=&self.permissions {
                    std::fs::set_permissions(&self.target,permissions.clone())
                        .map_err(|e|format!("{}: restore permissions failed: {e}",self.target.display()))?;
                }
                Ok(())
            }
            None => match std::fs::remove_file(&self.target) {
                Ok(()) => Ok(()),
                Err(e) if e.kind()==std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(format!("{}: removal of newly published output failed: {e}",self.target.display())),
            },
        }
    }
}
impl Drop for PreviousWave {
    fn drop(&mut self) {if let Some(copy)=&self.copy {let _=std::fs::remove_file(copy);}}
}

fn publish_prepared(
    outputs:Vec<PreparedWave>,mut publish:impl FnMut(usize,&PreparedWave)->Result<(),String>
) -> Result<Vec<PathBuf>,String> {
    use std::collections::HashSet;
    // Resolve parent aliases before checking for duplicate output names. Conservative lowercasing rejects paths
    // that collide on common case-insensitive volumes even when this test host happens to use a case-sensitive one.
    let mut names=HashSet::new();
    for output in &outputs {
        let path=&output.spool.path;
        let parent=std::fs::canonicalize(path.parent().unwrap_or_else(||Path::new(".")))
            .map_err(|e|format!("{}: {e}",path.display()))?;
        let name=path.file_name().ok_or_else(||format!("{}: output filename is missing",path.display()))?
            .to_string_lossy().to_lowercase();
        if !names.insert((parent,name)) {return Err(format!("{}: duplicate or case-colliding export destination",path.display()));}
    }
    // Preflight and reserve rollback copies *all at once*. None of the previous outputs can be modified before
    // every requested destination has been checked and snapshotted.
    let mut originals=outputs.iter().map(|w|PreviousWave::capture(&w.spool.path)).collect::<Result<Vec<_>,_>>()?;
    let mut files=Vec::with_capacity(outputs.len());
    for (i,output) in outputs.iter().enumerate() {
        if let Err(e)=publish(i,output) {
            // `replace_with` can fail after rename if syncing the parent directory fails; include the attempted
            // target as well as earlier successes in rollback, even if it is normally still unmodified.
            // Preserve the only previous-generation bytes if restoring that output fails.
            let failures=originals[..=i].iter_mut().rev().filter_map(|old| {
                let error=old.restore().err()?;
                Some(match old.copy.take() {
                    Some(backup)=>format!("{error}; previous output retained at {}",backup.display()),
                    None=>error,
                })
            }).collect::<Vec<_>>();
            if failures.is_empty() {return Err(format!("{e}; all attempted export files rolled back"));}
            return Err(format!("{e}; rollback incomplete: {}",failures.join("; ")));
        }
        files.push(output.spool.path.clone());
    }
    Ok(files)
}

/// Incremental native-rate float WAV export. Every render tick is drained to disk; memory does not grow with song
/// length. Only requested stems get spool files, and split tracks that remained silent are omitted at completion.
pub struct WaveStream { job: Job, rate: u32, outputs: Vec<WaveSpool>, initialized: bool, song: u64 }
impl WaveStream {
    pub fn new(job: &Job, rate: u32, song: u64) -> Result<Self,String> {
        let mut outputs=Vec::new();
        if job.tracks.is_empty() {
            outputs.push(WaveSpool::new(None,job.file(WAV),false,Meta { markers:job.markers.clone(),loop_frames:job.loop_frames,tempo:job.tempo })?);
        }
        Ok(Self { job:job.clone(),rate,outputs,initialized:false,song })
    }
    pub fn push(&mut self, mix: &[f32], tracks: &[Vec<f32>]) -> Result<(),String> {
        if !self.initialized {
            self.initialized=true;
            if !self.job.tracks.is_empty() {
                for &t in &self.job.tracks {
                    let name=if self.job.tracks.len()==1 {self.job.name.clone()} else {format!("{} - {t:02} {}",self.job.name,self.job.track_name(usize::from(t)))};
                    self.outputs.push(WaveSpool::new(Some(usize::from(t)),self.job.folder.join(format!("{name}.wav")),false,Meta::default())?);
                }
            } else if self.job.options.split {
                for t in 0..tracks.len() {
                    if t==0 && !self.job.options.split_master { continue; }
                    let path=self.job.stems_folder().join(format!("{t:02} {}.wav",self.job.track_name(t)));
                    self.outputs.push(WaveSpool::new(Some(t),path,true,Meta::default())?);
                }
            }
        }
        let limit=(self.job.options.tail==Tail::Cut).then_some(self.song);
        for output in &mut self.outputs {
            let samples=output.source.map_or(mix,|t|tracks.get(t).map_or(&[],Vec::as_slice));
            output.push(samples,self.job.options.channels,limit)?;
        }
        Ok(())
    }
    pub fn finish(self) -> Result<Vec<PathBuf>,String> {self.finish_with_cancel(||false)}
    pub fn finish_with_cancel(self, cancelled:impl Fn()->bool) -> Result<Vec<PathBuf>,String> {
        let mut prepared=Vec::new();
        for output in self.outputs {
            if cancelled() {return Err("Export cancelled; existing outputs preserved".into());}
            if let Some(wave)=output.prepare(&self.job.options,self.rate)? {prepared.push(wave);}
        }
        if cancelled() {return Err("Export cancelled; existing outputs preserved".into());}
        publish_prepared(prepared,|_,wave| {
            if cancelled() {return Err("Export cancelled".into());}
            wave.publish_with_cancel(&cancelled)
        })
    }
}

/// Writes the files of `job` from the render at `rate`: `mix` and the mixer tracks' outputs (interleaved stereo, by
/// track number; empty unless Split mixer tracks), `song` frames long before the tail. Returns the files written.
pub fn write(job: &Job, rate: u32, mix: Vec<f32>, tracks: Vec<Vec<f32>>, song: u64) -> Result<Vec<PathBuf>, String> {
    let o = &job.options;
    let out_rate = if o.rate == 0 { rate } else { o.rate };
    let mut written = Vec::new();
    if !job.tracks.is_empty() {
        return write_tracks(job, rate, tracks, song);
    }
    let mix = shape(o, mix, song, rate, out_rate);
    let scale = f64::from(out_rate) / f64::from(rate);
    let meta = Meta {
        markers: job.markers.iter().map(|m| Marker { frame: (m.frame as f64 * scale).round() as u64, name: m.name.clone() }).collect(),
        loop_frames: job.loop_frames.map(|(a, b)| ((a as f64 * scale).round() as u64, (b as f64 * scale).round() as u64)),
        tempo: job.tempo,
    };
    for f in o.ticked() {
        let path = job.file(f);
        encode(o, f, &path, &mix, out_rate, &meta)?;
        written.push(path);
    }
    if o.split {
        let dir = job.stems_folder();
        let mut made = false;
        for (t, audio) in tracks.into_iter().enumerate() {
            if (t == 0 && !o.split_master) || audio.iter().all(|s| s.abs() < SILENT) {
                continue;
            }
            if !made {
                std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                made = true;
            }
            let audio = shape(o, audio, song, rate, out_rate);
            let name = job.track_name(t);
            // As FL, only WAV and FLAC (spec §3 "Split mixer tracks").
            for f in o.ticked().filter(|&f| f == WAV || f == FLAC) {
                let path = dir.join(format!("{t:02} {name}.{}", EXTENSIONS[f]));
                encode(o, f, &path, &audio, out_rate, &Meta::default())?;
                written.push(path);
            }
        }
    }
    Ok(written)
}

/// Render to wave file... (the mixer track menu's File ▸): the outputs of `job.tracks` (from `tracks`, by track number),
/// each in every format ticked, silent ones too. One track's file is named as the form's name; several are
/// "<name> - <NN> <track name>". Lemon Studio's choice: the original's own naming and form were not looked at.
fn write_tracks(job: &Job, rate: u32, mut tracks: Vec<Vec<f32>>, song: u64) -> Result<Vec<PathBuf>, String> {
    let o = &job.options;
    let out_rate = if o.rate == 0 { rate } else { o.rate };
    let mut written = Vec::new();
    for &t in &job.tracks {
        let audio = tracks.get_mut(usize::from(t)).map(std::mem::take).unwrap_or_default();
        let audio = shape(o, audio, song, rate, out_rate);
        let stem = match job.tracks.len() {
            1 => job.name.clone(),
            _ => format!("{} - {t:02} {}", job.name, job.track_name(usize::from(t))),
        };
        for f in o.ticked() {
            let path = job.folder.join(format!("{stem}.{}", EXTENSIONS[f]));
            encode(o, f, &path, &audio, out_rate, &Meta::default())?;
            written.push(path);
        }
    }
    Ok(written)
}

/// The tail, the sample rate and the channels applied to interleaved stereo `audio`: interleaved, in the file's
/// channel count.
pub fn shape(o: &Options, audio: Vec<f32>, song: u64, rate: u32, out_rate: u32) -> Vec<f32> {
    let audio = tail(audio, song as usize, o.tail);
    let audio = if out_rate == rate { audio } else { resample(&audio, 2, rate, out_rate, o.points) };
    match o.channels {
        0 => audio,
        c => audio.chunks_exact(2).map(|f| [(f[0] + f[1]) * 0.5, f[0], f[1]][c - 1]).collect(),
    }
}

/// Interleaved stereo `audio` cut to `song` frames (Cut), as it is (Ring), or with what is past `song` added to the
/// start (Wrap: a tail longer than the song wraps again).
pub fn tail(mut audio: Vec<f32>, song: usize, t: Tail) -> Vec<f32> {
    let n = audio.len() / 2;
    match t {
        Tail::Ring | Tail::Fixed => audio,
        _ if song == 0 => audio,
        _ if n <= song => {
            audio.resize(song * 2, 0.0);
            audio
        }
        Tail::Cut => {
            audio.truncate(song * 2);
            audio
        }
        Tail::Wrap => {
            let (head, rest) = audio.split_at_mut(song * 2);
            for (i, f) in rest.chunks_exact(2).enumerate() {
                let j = (i % song) * 2;
                head[j] += f[0];
                head[j + 1] += f[1];
            }
            audio.truncate(song * 2);
            audio
        }
    }
}

/// `audio` (interleaved, `ch` channels) from `from` Hz to `to` Hz through a Blackman-windowed sinc `points` wide (2:
/// linear), its cutoff lowered to the new Nyquist when going down.
pub fn resample(audio: &[f32], ch: usize, from: u32, to: u32, points: u32) -> Vec<f32> {
    let n = audio.len() / ch;
    if n == 0 || from == to {
        return audio.to_vec();
    }
    let step = f64::from(from) / f64::from(to);
    let out_n = ((n as f64) / step).round() as usize;
    let mut out = vec![0.0f32; out_n * ch];
    if points <= 2 {
        for j in 0..out_n {
            let t = j as f64 * step;
            let i = (t as usize).min(n - 1);
            let (k, x) = ((i + 1).min(n - 1), (t - i as f64) as f32);
            for c in 0..ch {
                out[j * ch + c] = audio[i * ch + c] + (audio[k * ch + c] - audio[i * ch + c]) * x;
            }
        }
        return out;
    }
    let cut = (1.0 / step).min(1.0);
    let half = f64::from(points) / 2.0;
    // The window's reach in source samples: wider when the cutoff is lower.
    let reach = half / cut;
    let mut acc = vec![0.0f64; ch];
    for j in 0..out_n {
        let t = j as f64 * step;
        let i0 = (t - reach).ceil().max(0.0) as usize;
        let i1 = ((t + reach).floor() as usize).min(n - 1);
        acc.iter_mut().for_each(|a| *a = 0.0);
        for i in i0..=i1 {
            let x = (i as f64 - t) * cut;
            let w = cut * sinc(x) * blackman(x, half);
            for (c, a) in acc.iter_mut().enumerate() {
                *a += f64::from(audio[i * ch + c]) * w;
            }
        }
        for (c, a) in acc.iter().enumerate() {
            out[j * ch + c] = *a as f32;
        }
    }
    out
}

fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        1.0
    } else {
        let px = std::f64::consts::PI * x;
        px.sin() / px
    }
}

fn blackman(t: f64, w: f64) -> f64 {
    if t.abs() >= w {
        return 0.0;
    }
    let x = std::f64::consts::PI * t / w;
    0.42 + 0.5 * x.cos() + 0.08 * (2.0 * x).cos()
}

/// Samples as `bits`-bit integers, with triangular dither of 1 LSB when `dither`.
pub fn quantize(audio: &[f32], bits: u32, dither: bool) -> Vec<i32> {
    let full = (1i64 << (bits - 1)) as f64;
    let (lo, hi) = (-full, full - 1.0);
    let mut seed: u32 = 0x9E37_79B9;
    let mut rnd = move || {
        // xorshift32: the noise needs no more.
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        f64::from(seed) / f64::from(u32::MAX)
    };
    audio
        .iter()
        .map(|&s| {
            let d = if dither { rnd() - rnd() } else { 0.0 };
            (f64::from(s) * full + d).round().clamp(lo, hi) as i32
        })
        .collect()
}

/// What a WAV file carries besides its samples.
#[derive(Debug, Clone, Default)]
struct Meta {
    markers: Vec<Marker>,
    loop_frames: Option<(u64, u64)>,
    tempo: (f64, f64),
}

/// Writes `audio` (interleaved, in the options' channel count) at `rate` as format `f` to `path`.
fn encode(o: &Options, f: usize, path: &Path, audio: &[f32], rate: u32, meta: &Meta) -> Result<(), String> {
    let ch = o.channel_count();
    let err = |e: String| format!("{}: {e}", path.display());
    let bytes = match f {
        WAV => {
            let bits = [16, 24, 32][o.wav_depth];
            let mut meta = meta.clone();
            if !o.markers && !o.note_markers {
                meta.markers.clear();
            }
            if !o.loops {
                meta.loop_frames = None;
            } else if meta.loop_frames.is_none() {
                meta.loop_frames = Some((0, (audio.len() / usize::from(ch)) as u64));
            }
            if !o.tempo {
                meta.tempo = (0.0, 0.0);
            }
            wav_bytes(audio, ch, rate, bits, o.dither, &meta).map_err(err)?
        }
        FLAC => flac_bytes(audio, ch, rate, [16, 24][o.flac_depth], o.flac_level, o.dither).map_err(err)?,
        MP3 => {
            // MP3 goes up to 48 kHz: higher rates are brought down to 48 kHz for it.
            if rate > 48000 {
                let down = resample(audio, usize::from(ch), rate, 48000, o.points);
                mp3_bytes(&down, ch, 48000, o.mp3_kbps, o.mp3_gapless).map_err(err)?
            } else {
                mp3_bytes(audio, ch, rate, o.mp3_kbps, o.mp3_gapless).map_err(err)?
            }
        }
        _ => ogg_bytes(audio, ch, rate, o.ogg_kbps).map_err(err)?,
    };
    crate::durable_file::replace(path, &bytes).map_err(|e| err(e.to_string()))
}

/// A WAV file: PCM at 16 or 24 bits (dithered when `dither`), IEEE float at 32; with `cue `, `LIST`/`adtl`, `smpl` and
/// `acid` chunks for the markers, the loop and the tempo (`Meta`).
fn wav_bytes(audio: &[f32], ch: u16, rate: u32, bits: u16, dither: bool, meta: &Meta) -> Result<Vec<u8>, String> {
    let bps = u32::from(bits / 8);
    let data_len = audio.len() as u64 * u64::from(bps);
    if data_len > u64::from(u32::MAX) - 4096 {
        return Err("too long for a WAV file".into());
    }
    let mut chunks: Vec<u8> = Vec::new();
    let mut put = |id: &[u8; 4], body: &[u8]| {
        chunks.extend_from_slice(id);
        chunks.extend_from_slice(&(body.len() as u32).to_le_bytes());
        chunks.extend_from_slice(body);
        if body.len() % 2 == 1 {
            chunks.push(0);
        }
    };
    let mut fmt = Vec::new();
    fmt.extend_from_slice(&(if bits == 32 { 3u16 } else { 1u16 }).to_le_bytes());
    fmt.extend_from_slice(&ch.to_le_bytes());
    fmt.extend_from_slice(&rate.to_le_bytes());
    fmt.extend_from_slice(&(rate * u32::from(ch) * bps).to_le_bytes());
    fmt.extend_from_slice(&(ch * bits / 8).to_le_bytes());
    fmt.extend_from_slice(&bits.to_le_bytes());
    put(b"fmt ", &fmt);
    let mut data = Vec::with_capacity(data_len as usize);
    if bits == 32 {
        audio.iter().for_each(|s| data.extend_from_slice(&s.to_le_bytes()));
    } else {
        for v in quantize(audio, u32::from(bits), dither) {
            data.extend_from_slice(&v.to_le_bytes()[..bps as usize]);
        }
    }
    put(b"data", &data);
    drop(data);
    let frames = audio.len() as u64 / u64::from(ch);
    append_wav_metadata(&mut chunks, frames, rate, meta);
    let mut out = Vec::with_capacity(chunks.len() + 12);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(4 + chunks.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(&chunks);
    Ok(out)
}

fn append_wav_metadata(chunks: &mut Vec<u8>, frames: u64, rate: u32, meta: &Meta) {
    let mut put = |id: &[u8;4], body: &[u8]| {
        chunks.extend_from_slice(id);
        chunks.extend_from_slice(&(body.len() as u32).to_le_bytes());
        chunks.extend_from_slice(body);
        if body.len() % 2 == 1 { chunks.push(0); }
    };
    let markers: Vec<&Marker> = meta.markers.iter().filter(|m| m.frame <= frames).collect();
    if !markers.is_empty() {
        let mut cue = (markers.len() as u32).to_le_bytes().to_vec();
        for (i, m) in markers.iter().enumerate() {
            let id = i as u32 + 1;
            let pos = m.frame as u32;
            for v in [id, pos] {
                cue.extend_from_slice(&v.to_le_bytes());
            }
            cue.extend_from_slice(b"data");
            for v in [0u32, 0, pos] {
                cue.extend_from_slice(&v.to_le_bytes());
            }
        }
        put(b"cue ", &cue);
        let mut list = b"adtl".to_vec();
        for (i, m) in markers.iter().enumerate().filter(|(_, m)| !m.name.is_empty()) {
            let mut body = (i as u32 + 1).to_le_bytes().to_vec();
            body.extend_from_slice(m.name.as_bytes());
            body.push(0);
            list.extend_from_slice(b"labl");
            list.extend_from_slice(&(body.len() as u32).to_le_bytes());
            list.extend_from_slice(&body);
            if body.len() % 2 == 1 {
                list.push(0);
            }
        }
        put(b"LIST", &list);
    }
    if let Some((a, b)) = meta.loop_frames.filter(|(a, b)| b > a) {
        let mut smpl = Vec::new();
        let period = (1_000_000_000u64 / u64::from(rate.max(1))) as u32;
        for v in [0u32, 0, period, 60, 0, 0, 0, 1, 0] {
            smpl.extend_from_slice(&v.to_le_bytes());
        }
        let end = b.min(frames).saturating_sub(1) as u32;
        for v in [0u32, 0, a as u32, end, 0, 0] {
            smpl.extend_from_slice(&v.to_le_bytes());
        }
        put(b"smpl", &smpl);
    }
    if meta.tempo.0 > 0.0 {
        let mut acid = Vec::new();
        // Flags 0 (a loop, not a one-shot), root note 60, two unknown words as other writers put them.
        acid.extend_from_slice(&0u32.to_le_bytes());
        acid.extend_from_slice(&60u16.to_le_bytes());
        acid.extend_from_slice(&0x8000u16.to_le_bytes());
        acid.extend_from_slice(&0f32.to_le_bytes());
        acid.extend_from_slice(&(meta.tempo.1.round().max(0.0) as u32).to_le_bytes());
        acid.extend_from_slice(&4u16.to_le_bytes());
        acid.extend_from_slice(&4u16.to_le_bytes());
        acid.extend_from_slice(&(meta.tempo.0 as f32).to_le_bytes());
        put(b"acid", &acid);
    }
}

/// A FLAC file at `bits`, compression `level` 0..=8 (block size, prediction order and stereo coding go up with it, as
/// libFLAC's levels do).
fn flac_bytes(audio: &[f32], ch: u16, rate: u32, bits: u32, level: u8, dither: bool) -> Result<Vec<u8>, String> {
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;
    let samples = quantize(audio, bits, dither);
    let mut config = flacenc::config::Encoder::default();
    config.block_size = if level <= 2 { 1152 } else { 4096 };
    config.subframe_coding.use_lpc = level >= 3;
    config.subframe_coding.qlpc.lpc_order = match level {
        0..=2 => 0,
        3..=5 => 8,
        _ => 12,
    }
    .max(1);
    config.stereo_coding.use_midside = level >= 1;
    config.stereo_coding.use_leftside = level >= 1;
    config.stereo_coding.use_rightside = level >= 1;
    let config = config.into_verified().map_err(|e| format!("FLAC settings: {e:?}"))?;
    let source = flacenc::source::MemSource::from_samples(&samples, usize::from(ch), bits as usize, rate as usize);
    let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size).map_err(|e| format!("FLAC: {e:?}"))?;
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream.write(&mut sink).map_err(|e| format!("FLAC: {e:?}"))?;
    let mut bytes = sink.as_slice().to_vec();
    // flacenc counts the short last block in STREAMINFO's minimum block size, which marks the stream as variable
    // block size to readers such as symphonia; a fixed-size stream's minimum leaves the last block out (as libFLAC
    // and ffmpeg write it). STREAMINFO is always first: "fLaC", a 4-byte block header, then min and max (16 bits).
    if bytes.len() >= 12 {
        let (max0, max1) = (bytes[10], bytes[11]);
        bytes[8] = max0;
        bytes[9] = max1;
    }
    Ok(bytes)
}

/// An MP3 file at a constant `kbps`, LAME's best quality; `gapless`: with LAME's Info frame first (the encoder's delay
/// and padding, for players that trim them).
fn mp3_bytes(audio: &[f32], ch: u16, rate: u32, kbps: u32, gapless: bool) -> Result<Vec<u8>, String> {
    use mp3lame_encoder::{Bitrate, Builder, FlushGap, InterleavedPcm, MonoPcm, Quality};
    let rate_of = |k| match k {
        64 => Bitrate::Kbps64,
        96 => Bitrate::Kbps96,
        128 => Bitrate::Kbps128,
        160 => Bitrate::Kbps160,
        224 => Bitrate::Kbps224,
        256 => Bitrate::Kbps256,
        320 => Bitrate::Kbps320,
        _ => Bitrate::Kbps192,
    };
    let mut b = Builder::new().ok_or("MP3: LAME did not start")?;
    b.set_num_channels(ch as u8).map_err(|e| format!("MP3: {e:?}"))?;
    b.set_sample_rate(rate).map_err(|e| format!("MP3: {e:?}"))?;
    b.set_brate(rate_of(kbps)).map_err(|e| format!("MP3: {e:?}"))?;
    b.set_quality(Quality::Best).map_err(|e| format!("MP3: {e:?}"))?;
    b.set_to_write_vbr_tag(gapless).map_err(|e| format!("MP3: {e:?}"))?;
    let mut enc = b.build().map_err(|e| format!("MP3: {e:?}"))?;
    let mut out = Vec::new();
    let frames = audio.len() / usize::from(ch);
    out.reserve(mp3lame_encoder::max_required_buffer_size(frames));
    if ch == 1 {
        enc.encode_to_vec(MonoPcm(audio), &mut out).map_err(|e| format!("MP3: {e:?}"))?;
    } else {
        enc.encode_to_vec(InterleavedPcm(audio), &mut out).map_err(|e| format!("MP3: {e:?}"))?;
    }
    // A full flush: the no-gap one (for joining streams) leaves the last samples unencoded.
    enc.flush_to_vec::<FlushGap>(&mut out).map_err(|e| format!("MP3: {e:?}"))?;
    if gapless {
        // LAME leaves a frame of the tag's size empty at the start; the finished tag goes there.
        let mut tag = Vec::with_capacity(enc.lame_tag_size());
        if enc.lame_tag_encode_to_vec(&mut tag).is_some() && tag.len() <= out.len() {
            out[..tag.len()].copy_from_slice(&tag);
        }
    }
    Ok(out)
}

/// An Ogg Vorbis file at an average `kbps`.
fn ogg_bytes(audio: &[f32], ch: u16, rate: u32, kbps: u32) -> Result<Vec<u8>, String> {
    use std::num::{NonZeroU32, NonZeroU8};
    use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisEncoderBuilder};
    let e = |e: vorbis_rs::VorbisError| format!("OGG: {e}");
    let (Some(r), Some(c), Some(br)) = (NonZeroU32::new(rate), NonZeroU8::new(ch as u8), NonZeroU32::new(kbps * 1000)) else {
        return Err("OGG: no rate".into());
    };
    let mut b = VorbisEncoderBuilder::new_with_serial(r, c, Vec::new(), 0x4C45_4D4E);
    b.bitrate_management_strategy(VorbisBitrateManagementStrategy::Abr { average_bitrate: br });
    let mut enc = b.build().map_err(e)?;
    let chans = usize::from(ch);
    for block in audio.chunks(4096 * chans) {
        let planar: Vec<Vec<f32>> = (0..chans).map(|c| block.iter().skip(c).step_by(chans).copied().collect()).collect();
        enc.encode_audio_block(&planar).map_err(e)?;
    }
    enc.finish().map_err(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_multiple_stems_rolls_back_an_earlier_published_wav_if_a_later_publish_fails() {
        use std::sync::atomic::{AtomicU64,Ordering};
        static NEXT:AtomicU64=AtomicU64::new(0);
        let folder=std::env::temp_dir().join(format!("lemon_wave_batch_{}_{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
        std::fs::create_dir(&folder).unwrap();
        let mut job=Job::wav(&folder.join("take.wav"));
        job.options.split=true;
        job.options.split_master=true;
        job.options.tail=Tail::Cut;
        job.track_names=vec!["Master".into(),"Bus".into()];
        let mix=vec![0.2f32,0.1,0.3,0.2];
        let tracks=vec![vec![0.3f32,0.2,0.1,0.4],vec![0.4f32,0.4,0.2,0.1]];
        let mut stream=WaveStream::new(&job,48000,2).unwrap();
        stream.push(&mix,&tracks).unwrap();
        let prepared=stream.outputs.into_iter().map(|w|w.prepare(&job.options,48000).unwrap().unwrap()).collect::<Vec<_>>();
        assert_eq!(prepared.len(),3);
        let old=prepared.iter().map(|w|{
            let value=format!("old output: {}",w.spool.path.display()).into_bytes();
            std::fs::write(&w.spool.path,&value).unwrap();
            (w.spool.path.clone(),value)
        }).collect::<Vec<_>>();
        let e=publish_prepared(prepared,|index,wave| {
            if index==1 {Err("injected second WAV publication failure".into())} else {wave.publish()}
        }).unwrap_err();
        assert!(e.contains("rolled back"),"{e}");
        for (path,bytes) in &old {assert_eq!(&std::fs::read(path).unwrap(),bytes);}
        for dir in [&folder,&job.stems_folder()] {
            assert!(std::fs::read_dir(dir).unwrap().all(|entry|{
                let name=entry.unwrap().file_name();
                !name.to_string_lossy().contains("lemon-render")
            }),"failed batch left temporary render/backup files in {}",dir.display());
        }
        std::fs::remove_dir_all(folder).unwrap();
    }


    #[test]
    fn incomplete_rollback_retains_original_snapshot() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let folder = std::env::temp_dir().join(format!(
            "lemon_wave_failed_restore_{}_{}",
            std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&folder).unwrap();
        let mut job = Job::wav(&folder.join("take.wav"));
        job.options.split = true;
        job.options.split_master = true;
        job.options.tail = Tail::Cut;
        job.track_names = vec!["Master".into(), "Bus".into()];
        let mix = vec![0.2f32, 0.1, 0.3, 0.2];
        let tracks = vec![mix.clone(), mix.clone()];
        let mut stream = WaveStream::new(&job, 48000, 2).unwrap();
        stream.push(&mix, &tracks).unwrap();
        let prepared = stream.outputs.into_iter()
            .map(|w| w.prepare(&job.options, 48000).unwrap().unwrap())
            .collect::<Vec<_>>();
        assert!(prepared.len() >= 2);
        let first = prepared[0].spool.path.clone();
        let second = prepared[1].spool.path.clone();
        let original = b"previous irreplaceable audio";
        std::fs::write(&first, original).unwrap();
        std::fs::write(&second, b"other original audio").unwrap();
        let error = publish_prepared(prepared, |index, wave| {
            if index == 1 {return Err("injected second publish failure".into());}
            wave.publish()?;
            // Force restore failure portably without relying on filesystem privileges.
            std::fs::remove_file(&first).unwrap();
            std::fs::create_dir(&first).unwrap();
            Ok(())
        }).unwrap_err();
        assert!(error.contains("rollback incomplete"), "{error}");
        assert!(error.contains("previous output retained at"), "{error}");
        assert_eq!(std::fs::read(&second).unwrap().as_slice(), b"other original audio");
        let backups = std::fs::read_dir(first.parent().unwrap()).unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|p| p.file_name().unwrap().to_string_lossy().contains(".lemon-render-original-"))
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1, "{backups:?}");
        assert_eq!(std::fs::read(&backups[0]).unwrap().as_slice(), original);
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn streamed_wav_and_stems_match_buffered_samples_and_metadata() {
        let folder=std::env::temp_dir().join(format!("lemon_streamed_export_{}",std::process::id()));
        let _=std::fs::remove_dir_all(&folder);std::fs::create_dir_all(&folder).unwrap();
        let mix:Vec<f32>=(0..34).map(|i|i as f32/100.0-0.17).collect();
        let tracks=vec![mix.clone(),vec![0.0;34],mix.iter().map(|v|v*0.3).collect()];
        for channels in [0,1,2,3] {
            let mut job=Job::wav(&folder.join(format!("channels_{channels}.wav")));
            job.options.channels=channels;job.options.split=true;job.options.tail=Tail::Cut;
            job.options.markers=true;job.options.loops=true;job.options.tempo=true;
            job.markers=vec![Marker{frame:3,name:"marker".into()}];job.loop_frames=Some((2,9));job.tempo=(120.0,4.0);
            let expected=write(&job,48000,mix.clone(),tracks.clone(),10).unwrap().into_iter()
                .map(|p|{let b=std::fs::read(&p).unwrap();(p,b)}).collect::<Vec<_>>();
            let mut stream=WaveStream::new(&job,48000,10).unwrap();
            for (a,b) in [(0,6),(6,16),(16,34)] {
                let block=tracks.iter().map(|t|t[a..b].to_vec()).collect::<Vec<_>>();
                stream.push(&mix[a..b],&block).unwrap();
            }
            let files=stream.finish().unwrap();
            assert_eq!(files.len(),expected.len());
            for (path,bytes) in expected {assert_eq!(std::fs::read(path).unwrap(),bytes);}
        }
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn streaming_scope_is_explicit_and_native_wav_is_default() {
        let mut job=Job::wav(Path::new("test.wav"));
        assert_eq!(job.options.tail,Tail::Fixed);assert!(can_stream(&job,48000));
        job.options.rate=44100;assert!(!can_stream(&job,48000));
        job.options.rate=0;job.options.formats[FLAC]=true;assert!(!can_stream(&job,48000));
        job.options.formats[FLAC]=false;job.options.tail=Tail::Wrap;assert!(!can_stream(&job,48000));
        assert_eq!(BUFFERED_SAMPLE_LIMIT*std::mem::size_of::<f32>(),64*1024*1024);
    }


    fn tone(frames: usize, rate: u32, hz: f32) -> Vec<f32> {
        (0..frames).flat_map(|i| {
            let v = (i as f32 * hz * std::f32::consts::TAU / rate as f32).sin() * 0.5;
            [v, v]
        })
        .collect()
    }

    #[test]
    fn tails_cut_ring_and_wrap() {
        let audio: Vec<f32> = (0..10).flat_map(|i| [i as f32, -(i as f32)]).collect();
        assert_eq!(tail(audio.clone(), 10, Tail::Ring).len(), 20);
        assert_eq!(tail(audio.clone(), 4, Tail::Cut), vec![0.0, 0.0, 1.0, -1.0, 2.0, -2.0, 3.0, -3.0]);
        // Frames 4..10 fold onto 0..4: frame 0 gets 4 and 8, frame 1 gets 5 and 9, ...
        let w = tail(audio, 4, Tail::Wrap);
        assert_eq!(w.len(), 8);
        assert_eq!(w[0], 0.0 + 4.0 + 8.0);
        assert_eq!(w[2], 1.0 + 5.0 + 9.0);
        assert_eq!(w[7], -(3.0 + 7.0));
    }

    #[test]
    fn resampling_keeps_a_tone() {
        let a = tone(4800, 48000, 1000.0);
        for points in [2, 27, 64] {
            let b = resample(&a, 2, 48000, 44100, points);
            assert_eq!(b.len() / 2, 4410);
            // The level of a 1 kHz tone stays (away from the edges).
            let peak = b[400..8000].iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!((peak - 0.5).abs() < 0.02, "{points} points: peak {peak}");
        }
        let up = resample(&a, 2, 48000, 96000, 27);
        assert_eq!(up.len() / 2, 9600);
    }

    #[test]
    fn mono_choices() {
        let o = |c| Options { channels: c, ..Options::default() };
        let a = vec![1.0, 0.0, 0.5, 0.5];
        assert_eq!(shape(&o(0), a.clone(), 2, 48000, 48000), a);
        assert_eq!(shape(&o(1), a.clone(), 2, 48000, 48000), vec![0.5, 0.5]);
        assert_eq!(shape(&o(2), a.clone(), 2, 48000, 48000), vec![1.0, 0.5]);
        assert_eq!(shape(&o(3), a, 2, 48000, 48000), vec![0.0, 0.5]);
    }

    #[test]
    fn dither_is_one_lsb_of_noise() {
        let silent = vec![0.0f32; 1000];
        assert!(quantize(&silent, 16, false).iter().all(|&v| v == 0));
        let d = quantize(&silent, 16, true);
        assert!(d.iter().all(|&v| v.abs() <= 1) && d.iter().any(|&v| v != 0));
        assert_eq!(quantize(&[1.0, -1.0], 16, false), vec![32767, -32768]);
        assert_eq!(quantize(&[0.5], 24, false), vec![1 << 22]);
    }

    /// Every format is written and read back by the decoder playback uses (symphonia), at the length written.
    /// Gapless on: the first frame is LAME's Info frame, with the encoder's delay and padding filled in (the frame
    /// LAME left empty is overwritten, the stream keeps its length). Off: plain audio frames.
    #[test]
    fn gapless_mp3_has_an_info_frame() {
        let audio: Vec<f32> = (0..44100 * 2).map(|i| ((i / 2) as f32 * 0.05).sin() * 0.5).collect();
        let on = mp3_bytes(&audio, 2, 44100, 192, true).unwrap();
        let off = mp3_bytes(&audio, 2, 44100, 192, false).unwrap();
        let find = |b: &[u8], what: &[u8]| b[..1200].windows(what.len()).position(|w| w == what);
        let info = find(&on, b"Info").or_else(|| find(&on, b"Xing")).expect("an Info frame");
        let lame = find(&on, b"LAME").expect("LAME's extension");
        assert!(lame > info);
        // The delay and padding: 12 bits each, 21 bytes after "LAME".
        let d = &on[lame + 21..lame + 24];
        let (delay, padding) = ((u32::from(d[0]) << 4) | (u32::from(d[1]) >> 4), ((u32::from(d[1]) & 15) << 8) | u32::from(d[2]));
        assert!((500..=2000).contains(&delay) && padding < 2304, "delay {delay}, padding {padding}");
        assert!(find(&off, b"Info").is_none() && find(&off, b"Xing").is_none());
        assert!(on.len() > off.len(), "one frame more");
        // Nothing is cut off the end: without the tag, every frame decodes, the delay in front of the audio.
        let path = std::env::temp_dir().join(format!("os-export-gapless-{}.mp3", std::process::id()));
        std::fs::write(&path, &off).unwrap();
        let frames = os_engine::sample::load(&path).unwrap().frames();
        let _ = std::fs::remove_file(&path);
        assert!(frames >= 44100 + delay as usize, "{frames} frames");
    }

    #[test]
    fn writes_every_format() {
        let dir = std::env::temp_dir().join(format!("lemon-export-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut job = Job::wav(&dir.join("mix.wav"));
        job.options.formats = [true, true, true, true];
        job.options.wav_depth = 1;
        job.options.markers = true;
        job.options.loops = true;
        job.options.tempo = true;
        job.options.split = true;
        job.markers = vec![Marker { frame: 100, name: "Verse".into() }];
        job.tempo = (120.0, 4.0);
        job.track_names = vec!["Master".into(), "Drums".into()];
        let mix = tone(44100, 44100, 440.0);
        let tracks = vec![mix.clone(), mix.clone(), vec![0.0; mix.len()]];
        let files = write(&job, 44100, mix, tracks, 44100).expect("export");
        // The mix in four formats, the master and Drums stems in WAV and FLAC; the silent track is left out.
        assert_eq!(files.len(), 8, "{files:?}");
        assert!(dir.join("mix stems").join("01 Drums.flac").exists());
        assert!(!dir.join("mix stems").join("02 Bus 2.wav").exists());
        let wav = std::fs::read(dir.join("mix.wav")).unwrap();
        for id in [&b"cue "[..], b"labl", b"Verse", b"smpl", b"acid"] {
            assert!(wav.windows(id.len()).any(|w| w == id), "{}", String::from_utf8_lossy(id));
        }
        for f in EXTENSIONS {
            let sample = os_engine::sample::load(&dir.join(format!("mix.{f}"))).unwrap_or_else(|e| panic!("{f}: {e}"));
            let frames = sample.frames();
            // Lossy coders add a little padding.
            assert!((44100..44100 + 4096).contains(&frames), "{f}: {frames} frames");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Render to wave file... (the mixer track menu): only the tracks asked for, silent ones too, without the mix; one
    /// track's file takes the form's name, several are "<name> - <NN> <track name>".
    #[test]
    fn render_to_wave_file_writes_the_tracks() {
        let dir = std::env::temp_dir().join(format!("lemon-export-tracks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut job = Job::wav(&dir.join("Drums.wav"));
        job.options.split = true;
        job.track_names = vec!["Master".into(), "Drums".into(), "Bass/Sub".into()];
        job.tracks = vec![1];
        job.options.tail = Tail::Cut;
        let mix = tone(4410, 44100, 440.0);
        let tracks = vec![mix.clone(), mix.clone(), vec![0.0; mix.len()]];
        let files = write(&job, 44100, mix.clone(), tracks.clone(), 4410).expect("render");
        assert_eq!(files, [dir.join("Drums.wav")]);
        assert_eq!(os_engine::sample::load(&files[0]).unwrap().frames(), 4410);
        job.name = "Song".into();
        job.tracks = vec![1, 2];
        let files = write(&job, 44100, mix, tracks, 4410).expect("render");
        assert_eq!(files, [dir.join("Song - 01 Drums.wav"), dir.join("Song - 02 Bass_Sub.wav")], "the silent track too");
        assert!(!dir.join("Song.wav").exists() && !dir.join("Song stems").exists(), "no mix, no stems folder");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
