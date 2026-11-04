// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Note processors through the normal live-note path: a channel container's graph transforms `Engine::note_on` notes
//! before the instrument member sees them. The assertions measure captured output, not just the data model: pitch from
//! zero crossings, velocity from amplitude, and delay from the first sounding frame. A graph changed while audio runs is
//! picked up on the next tick, with every external note arriving exactly once.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use os_engine::chain::{Chain, Member};
use os_engine::{Engine, Instrument, PlayMode, SampleLibrary, SharedInstrument};
use os_model::container::{Graph, NoteKind, NoteProcessor};
use os_model::{Channel, ChannelKind, MixerTrack, Pattern, Project};

const RATE: u32 = 48_000;

/// A monophonic oscillator that receives notes on the instrument path. Events keep their frame so a delayed note is
/// audible from exactly the frame the chain handed it to the instrument.
struct Osc {
    at: u64,
    key: Option<u8>,
    velocity: f32,
    phase: f64,
    events: VecDeque<(u64, bool, u8, f32)>,
    ons: Vec<u64>,
    offs: Vec<u64>,
}

impl Osc {
    fn new() -> Arc<Mutex<Osc>> {
        Arc::new(Mutex::new(Osc {
            at: 0,
            key: None,
            velocity: 0.0,
            phase: 0.0,
            events: VecDeque::new(),
            ons: Vec::new(),
            offs: Vec::new(),
        }))
    }
}

impl Instrument for Osc {
    fn note_on(&mut self, offset: u32, key: u8, velocity: f32) {
        let at = self.at + u64::from(offset);
        self.ons.push(at);
        self.events.push_back((at, true, key, velocity));
    }

    fn note_off(&mut self, offset: u32, key: u8) {
        let at = self.at + u64::from(offset);
        self.offs.push(at);
        self.events.push_back((at, false, key, 0.0));
    }

    fn process(&mut self, rate: u32, left: &mut [f32], right: &mut [f32]) {
        left.fill(0.0);
        right.fill(0.0);
        for i in 0..left.len() {
            let frame = self.at + i as u64;
            while self.events.front().is_some_and(|e| e.0 <= frame) {
                let (_, on, key, velocity) = self.events.pop_front().unwrap();
                if on {
                    self.key = Some(key);
                    self.velocity = velocity;
                    self.phase = 0.0;
                } else if self.key == Some(key) {
                    self.key = None;
                }
            }
            if let Some(key) = self.key {
                let hz = 440.0 * 2.0_f64.powf((f64::from(key) - 69.0) / 12.0);
                let sample = (self.phase.sin() * f64::from(self.velocity) * 0.25) as f32;
                left[i] = sample;
                right[i] = sample;
                self.phase += std::f64::consts::TAU * hz / f64::from(rate);
            }
        }
        self.at += left.len() as u64;
    }

    fn active(&self) -> bool {
        self.key.is_some() || !self.events.is_empty()
    }

    fn voices(&self) -> usize {
        usize::from(self.key.is_some())
    }
}

fn channel() -> Channel {
    Channel {
        id: os_model::Id::NONE,
        index: 0,
        kind: ChannelKind::Plugin,
        name: Some("Notes".into()),
        plugin: Some("Container".into()),
        plugin_state: None,
        color: None,
        enabled: true,
        volume: 0.78125,
        pan: 0.0,
        pitch_cents: 0,
        mixer_track: 0,
        sample_path: None,
        layer_children: vec![],
        envelopes: Default::default(),
        rack: Default::default(),
        misc: Default::default(),
        sound: Default::default(),
    }
}

fn project() -> Project {
    Project {
        ppq: 96,
        tempo: Some(130.0),
        channels: vec![channel()],
        patterns: vec![Pattern { number: 1, ..Pattern::default() }],
        mixer: (0..4).map(MixerTrack::new).collect(),
        ..Project::default()
    }
}

fn graph(notes: &[NoteKind]) -> Graph {
    let mut g = Graph::serial(&[1]);
    g.notes = notes.iter().copied().map(NoteProcessor::builtin).collect();
    g
}

fn engine(notes: &[NoteKind]) -> (Engine, Arc<Mutex<Osc>>) {
    let p = project();
    let osc = Osc::new();
    let chain = Chain::with_graph(vec![Member::instrument(1, osc.clone() as SharedInstrument, 1.0, 0.0)], graph(notes))
        .expect("the graph is playable");
    let mut lib = SampleLibrary::default();
    let mut e = Engine::new(&p, &mut lib, RATE, PlayMode::Pattern(1));
    e.clear_schedule();
    e.set_instrument(0, Arc::new(Mutex::new(chain)));
    e.set_track_capture(true);
    (e, osc)
}

fn capture(e: &mut Engine, frames: usize) -> Vec<f32> {
    let mut block = vec![0.0; frames * 2];
    e.process(&mut block);
    e.take_track_audio().into_iter().next().unwrap_or_default()
}

fn mono(audio: &[f32]) -> Vec<f32> {
    audio.chunks_exact(2).map(|f| f[0]).collect()
}

fn onset(samples: &[f32]) -> Option<usize> {
    samples.iter().position(|x| x.abs() > 1e-4)
}

fn peak(samples: &[f32], at: usize) -> f32 {
    samples[at..(at + 1024).min(samples.len())].iter().fold(0.0, |a, x| a.max(x.abs()))
}

/// Frequency from zero crossings over 2048 samples after the attack.
fn frequency(samples: &[f32], at: usize) -> f64 {
    let start = at + 16;
    let end = (start + 2048).min(samples.len());
    let mut crossings = 0u32;
    let mut previous = 0.0f32;
    let mut counted = 0u32;
    for &x in &samples[start..end] {
        if x.abs() < 1e-4 {
            continue;
        }
        if counted > 0 && previous.signum() != x.signum() {
            crossings += 1;
        }
        previous = x;
        counted += 1;
    }
    f64::from(crossings) * f64::from(RATE) / (2.0 * f64::from(counted))
}

fn expected(key: u8) -> f64 {
    440.0 * 2.0_f64.powf((f64::from(key) - 69.0) / 12.0)
}

fn assert_hz(got: f64, key: u8, what: &str) {
    let want = expected(key);
    assert!((got - want).abs() / want < 0.05, "{what}: {got:.1} Hz, expected {want:.1} Hz");
}

/// The three kinds change real output: transpose moves the pitch, velocity scales the amplitude, and delay moves the
/// first sounding frame by one tick (231 frames at 48 kHz, 96 PPQ, 130 BPM). A plain graph is the reference.
#[test]
fn each_note_processor_changes_captured_audio() {
    let (mut plain, plain_notes) = engine(&[]);
    plain.note_on(0, 60, 100);
    let plain = mono(&capture(&mut plain, 4096));
    let plain_onset = onset(&plain).expect("the plain note sounds");
    let plain_peak = peak(&plain, plain_onset);
    assert_eq!(plain_notes.lock().unwrap().ons.len(), 1, "one external note reaches the instrument once");
    assert_hz(frequency(&plain, plain_onset), 60, "the plain note");

    let (mut transposed, transposed_notes) = engine(&[NoteKind::Transpose { semitones: 12 }]);
    transposed.note_on(0, 60, 100);
    let transposed = mono(&capture(&mut transposed, 4096));
    let transposed_onset = onset(&transposed).expect("the transposed note sounds");
    assert_eq!(transposed_notes.lock().unwrap().ons.len(), 1);
    assert_hz(frequency(&transposed, transposed_onset), 72, "Transpose +12");
    assert!(peak(&transposed, transposed_onset) > 0.001, "the transposed note is audible");

    let (mut quiet, quiet_notes) = engine(&[NoteKind::Velocity { percent: 50 }]);
    quiet.note_on(0, 60, 100);
    let quiet = mono(&capture(&mut quiet, 4096));
    let quiet_onset = onset(&quiet).expect("the quieter note sounds");
    assert_eq!(quiet_notes.lock().unwrap().ons.len(), 1);
    let ratio = peak(&quiet, quiet_onset) / plain_peak;
    assert!((ratio - 0.5).abs() < 0.05, "Velocity 50% scales the captured amplitude: {ratio:.3}");
    assert_hz(frequency(&quiet, quiet_onset), 60, "Velocity keeps the pitch");

    let (mut delayed, delayed_notes) = engine(&[NoteKind::Delay { ticks: 1 }]);
    delayed.note_on(0, 60, 100);
    let delayed = mono(&capture(&mut delayed, 4096));
    let delayed_onset = onset(&delayed).expect("the delayed note sounds");
    assert_eq!(delayed_notes.lock().unwrap().ons.len(), 1, "a delayed note is not duplicated");
    assert!((190..=260).contains(&delayed_onset), "Delay 1 tick starts one tick (231 frames) late, not at {delayed_onset}");
    assert_hz(frequency(&delayed, delayed_onset), 60, "Delay keeps the pitch");
}

/// Live graph edits: add, edit, reorder and remove processors between processed blocks. Every external note reaches the
/// instrument once, and each captured note is transformed by the graph in force when it was played.
#[test]
fn note_processors_follow_live_graph_edits() {
    let p = project();
    let osc = Osc::new();
    let chain = Arc::new(Mutex::new(
        Chain::with_graph(vec![Member::instrument(1, osc.clone() as SharedInstrument, 1.0, 0.0)], graph(&[])).unwrap(),
    ));
    let mut lib = SampleLibrary::default();
    let mut e = Engine::new(&p, &mut lib, RATE, PlayMode::Pattern(1));
    e.clear_schedule();
    e.set_instrument(0, chain.clone());
    e.set_track_capture(true);

    // Each case uses a different key so an earlier held note cannot contribute to this capture. The oscillator is
    // monophonic, replacing its key on each note on.
    let cases: Vec<(u8, Vec<NoteKind>, u8, bool)> = vec![
        (60, vec![], 60, false),
        (62, vec![NoteKind::Transpose { semitones: 12 }], 74, false),
        (64, vec![NoteKind::Transpose { semitones: 12 }, NoteKind::Velocity { percent: 50 }], 76, false),
        (65, vec![NoteKind::Velocity { percent: 50 }, NoteKind::Transpose { semitones: 12 }], 77, false),
        (67, vec![NoteKind::Velocity { percent: 50 }], 67, false),
        (69, vec![NoteKind::Velocity { percent: 50 }, NoteKind::Delay { ticks: 1 }], 69, true),
    ];

    let mut plain_peak = 0.0;
    for (i, (key, notes, expected_key, delayed)) in cases.iter().enumerate() {
        if i > 0 {
            chain.lock().unwrap().set_graph(graph(notes)).expect("the live graph is playable");
            assert_eq!(
                chain.lock().unwrap().graph().notes,
                graph(notes).notes,
                "the live chain took the edited processor list"
            );
        }
        e.note_on(0, u16::from(*key), 100);
        let audio = mono(&capture(&mut e, 4096));
        let at = onset(&audio).expect("the live note sounds");
        assert_hz(frequency(&audio, at), *expected_key, "the live note's pitch");
        let amp = peak(&audio, at);
        if *expected_key == *key && notes.is_empty() {
            plain_peak = amp;
        } else if notes.iter().any(|n| matches!(n, NoteKind::Velocity { .. })) {
            let ratio = amp / plain_peak;
            assert!((ratio - 0.5).abs() < 0.08, "the live velocity transform is audible: {ratio:.3}");
        }
        if *delayed {
            assert!((190..=260).contains(&at), "the live delay starts at the delayed frame: {at}");
        } else {
            assert!(at <= 2, "an undelayed live note starts on the captured tick: {at}");
        }
        assert_eq!(osc.lock().unwrap().ons.len(), i + 1, "each live note reached the instrument once");
        if i + 1 < cases.len() {
            // Release the held note and let a delayed release arrive before the next capture, so the next onset is the
            // next note rather than this one's tail.
            e.note_off(0, u16::from(*key));
            let _ = capture(&mut e, 1024);
        }
    }
}


/// The frame after the last audible sample. A release is the first silent frame after the sustained part.
fn release(samples: &[f32]) -> Option<usize> {
    samples.iter().rposition(|x| x.abs() > 1e-4).map(|i| i + 1)
}

fn tail_is_silent(samples: &[f32], from: usize) -> bool {
    samples[from..].iter().all(|x| x.abs() <= 1e-4)
}

/// Play a note, let it ring for one captured block, release it, and capture the release. Separate engines keep the
/// plain and delayed reference notes independent.
fn note_with_release(notes: &[NoteKind]) -> (Vec<f32>, Arc<Mutex<Osc>>) {
    let (mut e, osc) = engine(notes);
    e.note_on(0, 60, 100);
    let mut audio = mono(&capture(&mut e, 4096));
    e.note_off(0, 60);
    audio.extend(mono(&capture(&mut e, 8192)));
    (audio, osc)
}

/// A delayed note must keep the same sounding duration as an unprocessed note: both its onset and its release move by
/// the same delay. The captured release is checked, not just the internal event list.
#[test]
fn a_delayed_note_releases_once_at_its_shifted_end() {
    let (plain, plain_osc) = note_with_release(&[]);
    let (delayed, delayed_osc) = note_with_release(&[NoteKind::Delay { ticks: 1 }]);

    let plain_onset = onset(&plain).expect("the plain note sounds");
    let plain_release = release(&plain).expect("the plain note is released");
    let delayed_onset = onset(&delayed).expect("the delayed note sounds");
    let delayed_release = release(&delayed).expect("the delayed note is released");

    assert_eq!(plain_osc.lock().unwrap().ons.len(), 1, "the plain note reaches the instrument once");
    assert_eq!(plain_osc.lock().unwrap().offs.len(), 1, "the plain note is released once");
    assert_eq!(delayed_osc.lock().unwrap().ons.len(), 1, "the delayed note reaches the instrument once");
    assert_eq!(delayed_osc.lock().unwrap().offs.len(), 1, "the delayed note is released once");

    let plain_duration = plain_release as i64 - plain_onset as i64;
    let delayed_duration = delayed_release as i64 - delayed_onset as i64;
    assert!(plain_duration > 3900, "the plain note is sustained before release: {plain_duration}");
    assert!((plain_duration - delayed_duration).abs() <= 2, "delay keeps the sounding duration: plain {plain_duration}, delayed {delayed_duration}");
    assert!((delayed_onset as i64 - plain_onset as i64 - 231).abs() <= 5, "the delayed onset moves by one tick: {} vs {}", delayed_onset, plain_onset);
    assert!((delayed_release as i64 - plain_release as i64 - 231).abs() <= 5, "the delayed release moves by the same tick: {} vs {}", delayed_release, plain_release);
    assert!(delayed_release < delayed.len() - 100, "the delayed release was captured: {delayed_release} of {}", delayed.len());
    assert!(tail_is_silent(&delayed, delayed_release), "the delayed note leaves silence after its release");
}

/// Play a note, change its processor graph while it is still held, then release it. The held note keeps the transform it
/// started with, so removing, changing, reordering or adding processors cannot cut it short, stick it on, or duplicate
/// its release.
fn hold_then_edit(start: &[NoteKind], end: &[NoteKind]) -> (Vec<f32>, Arc<Mutex<Osc>>) {
    let p = project();
    let osc = Osc::new();
    let chain = Arc::new(Mutex::new(
        Chain::with_graph(vec![Member::instrument(1, osc.clone() as SharedInstrument, 1.0, 0.0)], graph(start)).unwrap(),
    ));
    let mut lib = SampleLibrary::default();
    let mut e = Engine::new(&p, &mut lib, RATE, PlayMode::Pattern(1));
    e.clear_schedule();
    e.set_instrument(0, chain.clone());
    e.set_track_capture(true);

    e.note_on(0, 60, 100);
    let mut audio = mono(&capture(&mut e, 4096));
    chain.lock().unwrap().set_graph(graph(end)).expect("the edited graph is playable");
    assert_eq!(chain.lock().unwrap().graph().notes, graph(end).notes, "the live chain took the edit");
    e.note_off(0, 60);
    audio.extend(mono(&capture(&mut e, 8192)));
    (audio, osc)
}

#[test]
fn a_held_note_survives_live_processor_edits() {
    let cases: Vec<(&str, Vec<NoteKind>, Vec<NoteKind>)> = vec![
        ("remove delay", vec![NoteKind::Delay { ticks: 1 }], vec![]),
        ("change delay ticks", vec![NoteKind::Delay { ticks: 1 }], vec![NoteKind::Delay { ticks: 2 }]),
        (
            "reorder processors",
            vec![NoteKind::Delay { ticks: 1 }, NoteKind::Velocity { percent: 50 }],
            vec![NoteKind::Velocity { percent: 50 }, NoteKind::Delay { ticks: 1 }],
        ),
        ("add a processor", vec![], vec![NoteKind::Transpose { semitones: 12 }]),
    ];

    // A note that is held for the same time with no edit is the timing reference.
    let (base_audio, _) = hold_then_edit(&[], &[]);
    let base_on = onset(&base_audio).expect("the reference note sounds");
    let base_off = release(&base_audio).expect("the reference note is released");
    let base_duration = base_off as i64 - base_on as i64;

    for (name, start, end) in cases {
        let delayed_start = start.iter().any(|n| matches!(n, NoteKind::Delay { .. }));
        let (audio, osc) = hold_then_edit(&start, &end);
        let on = onset(&audio).expect("the held note sounds");
        let off = release(&audio).expect("the held note is released");
        let events = osc.lock().unwrap();
        assert_eq!(events.ons.len(), 1, "{name}: one note-on");
        assert_eq!(events.offs.len(), 1, "{name}: one note-off");
        assert!(on <= off, "{name}: onset before release");
        if delayed_start {
            assert!((190..=260).contains(&on), "{name}: the original delay still shifts the onset: {on}");
        } else {
            assert!(on <= 2, "{name}: the onset stays on its captured tick: {on}");
        }
        let duration = off as i64 - on as i64;
        assert!((duration - base_duration).abs() <= 2, "{name}: the held note keeps its original sounding duration: {duration} vs {base_duration}");
        assert!(off < audio.len() - 100, "{name}: the release is inside the captured audio: {off} of {}", audio.len());
        assert!(tail_is_silent(&audio, off), "{name}: audio returns to silence, with no stuck note");
    }
}


/// A polyphonic oscillator: two same-key notes can sound at once. The first active voice goes left and the second
/// right, so captured stereo separates the two note lives; a note-off releases the oldest voice of its key, as a
/// polyphonic instrument does.
struct PolyVoice {
    key: u8,
    velocity: f32,
    phase: f64,
    pan: f32,
}

struct Poly {
    at: u64,
    voices: Vec<PolyVoice>,
    ons: Vec<u64>,
    offs: Vec<u64>,
}

impl Poly {
    fn new() -> Arc<Mutex<Poly>> {
        Arc::new(Mutex::new(Poly { at: 0, voices: Vec::new(), ons: Vec::new(), offs: Vec::new() }))
    }
}

impl Instrument for Poly {
    fn note_on(&mut self, offset: u32, key: u8, velocity: f32) {
        let at = self.at + u64::from(offset);
        self.ons.push(at);
        let pan = if self.voices.is_empty() { -1.0 } else { 1.0 };
        self.voices.push(PolyVoice { key, velocity, phase: 0.0, pan });
    }

    fn note_off(&mut self, offset: u32, key: u8) {
        let at = self.at + u64::from(offset);
        self.offs.push(at);
        if let Some(i) = self.voices.iter().position(|v| v.key == key) {
            self.voices.remove(i);
        }
    }

    fn process(&mut self, rate: u32, left: &mut [f32], right: &mut [f32]) {
        left.fill(0.0);
        right.fill(0.0);
        for i in 0..left.len() {
            for v in &mut self.voices {
                let hz = 440.0 * 2.0_f64.powf((f64::from(v.key) - 69.0) / 12.0);
                let sample = (v.phase.sin() * f64::from(v.velocity) * 0.25) as f32;
                if v.pan < 0.0 {
                    left[i] += sample;
                } else {
                    right[i] += sample;
                }
                v.phase += std::f64::consts::TAU * hz / f64::from(rate);
            }
        }
        self.at += left.len() as u64;
    }

    fn active(&self) -> bool {
        !self.voices.is_empty()
    }

    fn voices(&self) -> usize {
        self.voices.len()
    }
}

fn poly_engine(notes: &[NoteKind]) -> (Engine, Arc<Mutex<Chain>>, Arc<Mutex<Poly>>) {
    let p = project();
    let poly = Poly::new();
    let chain = Arc::new(Mutex::new(
        Chain::with_graph(vec![Member::instrument(1, poly.clone() as SharedInstrument, 1.0, 0.0)], graph(notes)).unwrap(),
    ));
    let mut lib = SampleLibrary::default();
    let mut e = Engine::new(&p, &mut lib, RATE, PlayMode::Pattern(1));
    e.clear_schedule();
    e.set_instrument(0, chain.clone());
    e.set_track_capture(true);
    (e, chain, poly)
}

fn sides(audio: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let mut left = Vec::with_capacity(audio.len() / 2);
    let mut right = Vec::with_capacity(audio.len() / 2);
    for f in audio.chunks_exact(2) {
        left.push(f[0]);
        right.push(f[1]);
    }
    (left, right)
}

fn delay_ticks(notes: &[NoteKind]) -> u32 {
    notes.iter().filter_map(|n| match n {
        NoteKind::Delay { ticks } => Some(*ticks),
        _ => None,
    }).sum()
}

/// Two same-key notes overlapping in a Delay shift: the first note's release is pending when the second starts. The
/// first processor edit happens while the first note is held; the second happens after both are sounding. A delayed
/// off must release its own (oldest) note, leaving the later note audible for its full duration.
fn overlap_case(start: &[NoteKind], mid: &[NoteKind], after: &[NoteKind]) -> (Vec<f32>, Vec<f32>, Arc<Mutex<Poly>>) {
    let (mut e, chain, poly) = poly_engine(start);
    e.note_on(0, 60, 100);
    let mut audio = capture(&mut e, 1024);

    // The first note is still held while its graph changes; the second note then starts under the new graph.
    chain.lock().unwrap().set_graph(graph(mid)).expect("the first edit is playable");
    e.note_on(0, 60, 100);
    audio.extend(capture(&mut e, 1024));

    // Both notes are sounding now. Edit again mid-overlap; each held note keeps the transform it started with.
    chain.lock().unwrap().set_graph(graph(after)).expect("the mid-overlap edit is playable");
    audio.extend(capture(&mut e, 3072));

    e.note_off(0, 60);
    audio.extend(capture(&mut e, 1024));
    e.note_off(0, 60);
    audio.extend(capture(&mut e, 8192));

    let (left, right) = sides(&audio);
    (left, right, poly)
}

/// A same-key retrigger with Delay overlap keeps both note lives: each has one onset and one release at its own
/// delayed position, the later note survives the earlier note's pending release, and the final tail is silent.
/// The edit cases also prove that held-note matching is ordered rather than newest-first.
#[test]
fn overlapping_same_key_notes_release_independently() {
    let cases: Vec<(&str, Vec<NoteKind>, Vec<NoteKind>, Vec<NoteKind>)> = vec![
        ("same delay", vec![NoteKind::Delay { ticks: 1 }], vec![NoteKind::Delay { ticks: 1 }], vec![NoteKind::Delay { ticks: 1 }]),
        ("remove delay", vec![NoteKind::Delay { ticks: 1 }], vec![], vec![]),
        ("change delay", vec![NoteKind::Delay { ticks: 1 }], vec![NoteKind::Delay { ticks: 2 }], vec![NoteKind::Delay { ticks: 1 }]),
        (
            "reorder processors",
            vec![NoteKind::Delay { ticks: 1 }, NoteKind::Transpose { semitones: 12 }],
            vec![NoteKind::Transpose { semitones: 12 }, NoteKind::Delay { ticks: 1 }],
            vec![NoteKind::Delay { ticks: 1 }, NoteKind::Transpose { semitones: 12 }],
        ),
    ];

    // The no-processor reference uses the same input calls and tick positions as the cases. Its captured durations
    // are the unprocessed note lengths; delay must move both ends without changing them.
    let (base_left, base_right, _) = overlap_case(&[], &[], &[]);
    let base_left_on = onset(&base_left).expect("the reference first note sounds");
    let base_left_off = release(&base_left).expect("the reference first note is released");
    let base_right_on = onset(&base_right).expect("the reference retrigger sounds");
    let base_right_off = release(&base_right).expect("the reference retrigger is released");
    let base_left_duration = base_left_off as i64 - base_left_on as i64;
    let base_right_duration = base_right_off as i64 - base_right_on as i64;

    for (name, start, mid, after) in cases {
        let (left, right, poly) = overlap_case(&start, &mid, &after);
        let left_on = onset(&left).expect("the first note sounds");
        let left_off = release(&left).expect("the first note is released");
        let right_on = onset(&right).expect("the retriggered note sounds");
        let right_off = release(&right).expect("the retriggered note is released");

        let events = poly.lock().unwrap();
        assert_eq!(events.ons.len(), 2, "{name}: exactly two note-ons");
        assert_eq!(events.offs.len(), 2, "{name}: exactly two note-offs");
        drop(events);

        let start_frames = u64::from(delay_ticks(&start)) * 231;
        let mid_frames = u64::from(delay_ticks(&mid)) * 231;
        assert!((left_on as i64 - start_frames as i64).abs() <= 260, "{name}: first onset at its delay: {left_on}");
        assert!((right_on as i64 - 1024 - mid_frames as i64).abs() <= 260, "{name}: retrigger onset at its delay: {right_on}");
        assert!(right_on < left_off, "{name}: the retrigger overlaps the pending first release");
        let left_duration = left_off as i64 - left_on as i64;
        let right_duration = right_off as i64 - right_on as i64;
        assert!((left_duration - base_left_duration).abs() <= 4, "{name}: first note's full duration: {left_duration} vs unprocessed {base_left_duration}");
        assert!((right_duration - base_right_duration).abs() <= 4, "{name}: retriggered note's full duration: {right_duration} vs unprocessed {base_right_duration}");
        assert!((left_off as i64 - base_left_off as i64 - start_frames as i64).abs() <= 5, "{name}: first release at its shifted position: {left_off} vs {base_left_off} + {start_frames}");
        assert!((right_off as i64 - base_right_off as i64 - mid_frames as i64).abs() <= 5, "{name}: retrigger release at its shifted position: {right_off} vs {base_right_off} + {mid_frames}");
        assert!(right_off > left_off, "{name}: the later note survives the earlier release");

        let last = left_off.max(right_off);
        assert!(last < left.len().min(right.len()) - 100, "{name}: releases are inside the captured audio");
        assert!(tail_is_silent(&left, last), "{name}: left returns to silence");
        assert!(tail_is_silent(&right, last), "{name}: right returns to silence");
    }
}


/// Three frequencies far enough apart to be read independently from captured audio. Each overlapping note gets one
/// frequency in `Trio`, so a stereo capture can still measure every note's own life instead of just their sum. The
/// frequencies are exact bins of a 32-frame window (1, 2 and 3 cycles), keeping onset edges within a few frames.
const TRIO_HZ: [f64; 3] = [1500.0, 3000.0, 4500.0];
const TRIO_WINDOW: usize = 32;

struct TrioVoice {
    key: u8,
    velocity: f32,
    phase: f64,
    hz: f64,
}

/// A three-voice test instrument. Voices keep the frequency they started with, and a note-off releases the oldest
/// voice of its key, so the three same-key lifetimes can be told apart in the output.
struct Trio {
    at: u64,
    voices: Vec<TrioVoice>,
    ons: Vec<u64>,
    offs: Vec<u64>,
}

impl Trio {
    fn new() -> Arc<Mutex<Trio>> {
        Arc::new(Mutex::new(Trio { at: 0, voices: Vec::new(), ons: Vec::new(), offs: Vec::new() }))
    }
}

impl Instrument for Trio {
    fn note_on(&mut self, offset: u32, key: u8, velocity: f32) {
        let at = self.at + u64::from(offset);
        self.ons.push(at);
        let hz = TRIO_HZ[self.voices.len().min(TRIO_HZ.len() - 1)];
        self.voices.push(TrioVoice { key, velocity, phase: 0.0, hz });
    }

    fn note_off(&mut self, offset: u32, key: u8) {
        let at = self.at + u64::from(offset);
        self.offs.push(at);
        if let Some(i) = self.voices.iter().position(|v| v.key == key) {
            self.voices.remove(i);
        }
    }

    fn process(&mut self, rate: u32, left: &mut [f32], right: &mut [f32]) {
        left.fill(0.0);
        right.fill(0.0);
        for i in 0..left.len() {
            for v in &mut self.voices {
                let sample = (v.phase.sin() * f64::from(v.velocity) * 0.16) as f32;
                left[i] += sample;
                right[i] += sample;
                v.phase += std::f64::consts::TAU * v.hz / f64::from(rate);
            }
        }
        self.at += left.len() as u64;
    }

    fn active(&self) -> bool {
        !self.voices.is_empty()
    }

    fn voices(&self) -> usize {
        self.voices.len()
    }
}

fn trio_engine(notes: &[NoteKind]) -> (Engine, Arc<Mutex<Chain>>, Arc<Mutex<Trio>>) {
    let p = project();
    let trio = Trio::new();
    let chain = Arc::new(Mutex::new(
        Chain::with_graph(vec![Member::instrument(1, trio.clone() as SharedInstrument, 1.0, 0.0)], graph(notes)).unwrap(),
    ));
    let mut lib = SampleLibrary::default();
    let mut e = Engine::new(&p, &mut lib, RATE, PlayMode::Pattern(1));
    e.clear_schedule();
    e.set_instrument(0, chain.clone());
    e.set_track_capture(true);
    (e, chain, trio)
}

/// Three same-key note-ons overlap because each starts under a different Delay. The graph changes again while all three
/// are held, so every release must use the delay its own note captured. The capture lasts through all three releases.
fn triple_overlap_case(start: &[NoteKind], mid: &[NoteKind], late: &[NoteKind], after: &[NoteKind]) -> (Vec<f32>, Arc<Mutex<Trio>>) {
    let (mut e, chain, trio) = trio_engine(start);
    e.note_on(0, 60, 100);
    let mut audio = capture(&mut e, 1024);
    audio.extend(capture(&mut e, 1024));

    chain.lock().unwrap().set_graph(graph(mid)).expect("the second note's graph is playable");
    e.note_on(0, 60, 100);
    audio.extend(capture(&mut e, 1024));

    chain.lock().unwrap().set_graph(graph(late)).expect("the third note's graph is playable");
    e.note_on(0, 60, 100);
    audio.extend(capture(&mut e, 1024));

    // All three are held and sounding; this edit must not disturb their captured delays.
    chain.lock().unwrap().set_graph(graph(after)).expect("the mid-overlap graph is playable");
    audio.extend(capture(&mut e, 2048));

    e.note_off(0, 60);
    audio.extend(capture(&mut e, 1024));
    e.note_off(0, 60);
    audio.extend(capture(&mut e, 1024));
    e.note_off(0, 60);
    audio.extend(capture(&mut e, 8192));

    (mono(&audio), trio)
}

/// The level at `hz` in a 32-frame window starting at `start`. The three TRIO_HZ values are exact DFT bins, so each
/// voice's steady level lands in its own bin and the other voices add nothing to it away from transitions.
fn tone_level(samples: &[f32], start: usize, hz: f64) -> f64 {
    let mut re = 0.0;
    let mut im = 0.0;
    for i in 0..TRIO_WINDOW {
        let phase = std::f64::consts::TAU * hz * i as f64 / f64::from(RATE);
        let x = f64::from(samples[start + i]);
        re += x * phase.cos();
        im -= x * phase.sin();
    }
    (re * re + im * im).sqrt() / TRIO_WINDOW as f64
}

/// The first sounding frame and the frame after the last sounding frame for one frequency, found from the captured
/// buffer with a 32-frame sliding DFT. The half-level crossing is half a window before/after the edge, so adding half
/// a window back gives the edge without needing the instrument's event list.
fn tone_span(samples: &[f32], hz: f64) -> (usize, usize) {
    assert!(samples.len() > TRIO_WINDOW, "the capture is long enough for a tone window");
    let levels: Vec<f64> = (0..=samples.len() - TRIO_WINDOW)
        .map(|at| tone_level(samples, at, hz))
        .collect();
    let peak = levels.iter().copied().fold(0.0, f64::max);
    assert!(peak > 1e-5, "the {hz} Hz note is audible (peak {peak})");
    let threshold = peak * 0.5;
    let mut runs = 0;
    let mut above = false;
    for &level in &levels {
        if level >= threshold && !above {
            runs += 1;
            above = true;
        } else if level < threshold {
            above = false;
        }
    }
    assert_eq!(runs, 1, "the {hz} Hz note has one continuous sounding run");
    let first = levels.iter().position(|&level| level >= threshold).expect("the tone has an onset");
    let last = levels.iter().rposition(|&level| level >= threshold).expect("the tone has a release");
    (first + TRIO_WINDOW / 2, last + TRIO_WINDOW / 2)
}

/// Deeper same-key proof: three overlapping note-ons on one key, each under the Delay active at its start, plus a graph
/// edit while all three are held. Captured audio gives each frequency exactly one onset and one release, at its own shifted
/// position and with its unprocessed duration; the three releases are distinct and the final tail is silent.
#[test]
fn three_overlapping_same_key_notes_release_independently() {
    let (base, _) = triple_overlap_case(&[], &[], &[], &[]);
    let mut base_spans: Vec<(usize, usize)> = TRIO_HZ.iter().map(|&hz| tone_span(&base, hz)).collect();
    // The first note starts on the capture's first frames, before a 32-frame analysis window can show its rise. At that
    // point it is the only voice, so the plain first audible sample is its exact onset.
    base_spans[0].0 = onset(&base).expect("the reference first note is audible");
    let base_durations: Vec<i64> = base_spans.iter().map(|&(on, off)| off as i64 - on as i64).collect();

    let start = [NoteKind::Delay { ticks: 1 }];
    let mid = [NoteKind::Delay { ticks: 2 }];
    let late = [NoteKind::Delay { ticks: 1 }];
    let after = [];
    let (audio, trio) = triple_overlap_case(&start, &mid, &late, &after);
    let mut spans: Vec<(usize, usize)> = TRIO_HZ.iter().map(|&hz| tone_span(&audio, hz)).collect();
    // The first delayed note is still the only voice at its onset, so use the exact first audible sample as above.
    spans[0].0 = onset(&audio).expect("the first delayed note is audible");
    let shifts = [1_u64 * 231, 2_u64 * 231, 1_u64 * 231];

    let events = trio.lock().unwrap();
    assert_eq!(events.ons.len(), 3, "exactly three note-ons reach the instrument");
    assert_eq!(events.offs.len(), 3, "exactly three note-offs reach the instrument");
    drop(events);

    for i in 0..TRIO_HZ.len() {
        let (base_on, base_off) = base_spans[i];
        let (on, off) = spans[i];
        let duration = off as i64 - on as i64;
        assert!(on < off, "note {i}: onset before release");
        assert!((on as i64 - base_on as i64 - shifts[i] as i64).abs() <= 5, "note {i}: onset at its own shifted position: {on} vs {base_on} + {}", shifts[i]);
        assert!((off as i64 - base_off as i64 - shifts[i] as i64).abs() <= 5, "note {i}: release at its own shifted position: {off} vs {base_off} + {}", shifts[i]);
        assert!((duration - base_durations[i]).abs() <= 16, "note {i}: full unprocessed duration: {duration} vs {}", base_durations[i]);
        if i > 0 {
            assert!(spans[i - 1].1 < off, "note {i}: its release is later than note {}'s, so it is not cut short", i - 1);
            assert!(spans[i - 1].0 < on, "note {i}: its onset is later than note {}'s", i - 1);
        }
    }

    // Every note was sounding before the first release: the three lifetimes really overlap.
    assert!(spans[1].0 < spans[0].1, "the second note overlaps the first");
    assert!(spans[2].0 < spans[0].1, "the third note overlaps the first");
    assert!(spans[2].1 > spans[1].1, "the third release is after the second");

    let last = release(&audio).expect("the overlap capture has a final release");
    assert!(spans[2].1 <= last && last - spans[2].1 <= TRIO_WINDOW, "the captured end is the third note's release: {last} vs {}", spans[2].1);
    assert!(last < audio.len() - 100, "the last release is inside the captured audio");
    assert!(tail_is_silent(&audio, last), "audio is silent only after the last release");
}
