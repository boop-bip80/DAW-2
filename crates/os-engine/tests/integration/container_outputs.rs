// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! A channel container's extra graph outputs reach their own mixer tracks through the real playback path: notes come in
//! through `Engine::note_on`, the chain routes each instrument branch to a named output, and the mixer's track capture
//! sees the sound after that destination's fader, pan and mute. A graph changed while audio runs is read on the next
//! tick, and no note is lost or sent twice.

use std::sync::{Arc, Mutex};

use os_engine::chain::{Chain, Member};
use os_engine::{Engine, HostedEffect, Instrument, PlayMode, SampleLibrary, SharedEffect, SharedInstrument};
use os_model::container::{Edge, Endpoint, Graph};
use os_model::{Channel, ChannelKind, MixerTrack, Pattern, Project};

/// An instrument that puts a constant signed burst out when it gets a note. The sign says which member made the sound;
/// the onset says when the note reached it.
struct Tone {
    value: f32,
    at: u64,
    start: Option<u32>,
    left: u32,
    notes: Vec<u64>,
}

impl Tone {
    fn new(value: f32) -> Arc<Mutex<Tone>> {
        Arc::new(Mutex::new(Tone { value, at: 0, start: None, left: 192, notes: Vec::new() }))
    }
}

impl Instrument for Tone {
    fn note_on(&mut self, offset: u32, _key: u8, _velocity: f32) {
        self.notes.push(self.at + u64::from(offset));
        self.start = Some(offset);
        self.left = 192;
    }

    fn note_off(&mut self, _offset: u32, _key: u8) {}

    fn process(&mut self, _rate: u32, left: &mut [f32], right: &mut [f32]) {
        left.fill(0.0);
        right.fill(0.0);
        if let Some(start) = self.start.take() {
            let start = start as usize;
            let end = (start + self.left as usize).min(left.len());
            left[start..end].fill(self.value);
            right[start..end].fill(self.value);
            self.left = self.left.saturating_sub((end - start) as u32);
        }
        self.at += left.len() as u64;
    }

    fn active(&self) -> bool {
        self.left > 0
    }
}

fn channel() -> Channel {
    Channel {
        id: os_model::Id::NONE,
        index: 0,
        kind: ChannelKind::Plugin,
        name: Some("Container".into()),
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
    let mut p = Project {
        ppq: 96,
        tempo: Some(130.0),
        channels: vec![channel()],
        patterns: vec![Pattern { number: 1, ..Pattern::default() }],
        mixer: (0..4).map(MixerTrack::new).collect(),
        ..Project::default()
    };
    // The destination tracks make the graph's gain and pan measurable.
    p.mixer[1].volume = Some(0.5);
    p.mixer[1].pan = Some(-1.0);
    p.mixer[2].volume = Some(0.25);
    p.mixer[2].pan = Some(1.0);
    p.mixer[3].volume = Some(0.5);
    p.mixer[3].pan = Some(0.0);
    p
}

fn graph(outputs: &[&str], destinations: &[Option<u16>], edges: Vec<Edge>) -> Graph {
    Graph { outputs: outputs.iter().map(|s| (*s).into()).collect(), destinations: destinations.to_vec(), notes: Vec::new(), edges }
}

fn routed(first: u32, second: u32, outputs: &[&str], destinations: &[Option<u16>]) -> Graph {
    graph(
        outputs,
        destinations,
        vec![
            Edge { from: Endpoint::Member(first), to: Endpoint::Output(1) },
            Edge { from: Endpoint::Member(second), to: Endpoint::Output(2) },
        ],
    )
}

fn chain(a: &Arc<Mutex<Tone>>, b: &Arc<Mutex<Tone>>, graph: Graph) -> Arc<Mutex<Chain>> {
    let a: SharedInstrument = a.clone();
    let b: SharedInstrument = b.clone();
    let c = Chain::with_graph(
        vec![Member::instrument(1, a, 1.0, 0.0), Member::instrument(2, b, 1.0, 0.0)],
        graph,
    )
    .unwrap();
    Arc::new(Mutex::new(c))
}

fn peaks(track: &[f32]) -> (f32, f32) {
    track.chunks_exact(2).fold((0.0, 0.0), |(l, r), f| (l.max(f[0].abs()), r.max(f[1].abs())))
}

fn audio(e: &mut Engine, frames: usize, into: &mut [Vec<f32>]) {
    let mut block = vec![0.0; frames * 2];
    e.process(&mut block);
    let got = e.take_track_audio();
    for (all, track) in into.iter_mut().zip(got) {
        all.extend(track);
    }
}

/// The frame a note queued after `block` frames starts on: the next tick boundary (96 PPQ at 130 BPM, 48 kHz).
fn next_tick_frame(block: u64) -> u64 {
    let tick = 48_000.0 * 60.0 / 130.0 / 96.0;
    ((block as f64 / tick).ceil() * tick).round() as u64
}

/// The first frame of a captured track that is not silent.
fn onset(track: &[f32]) -> Option<usize> {
    track.chunks_exact(2).position(|f| f[0] != 0.0 || f[1] != 0.0)
}

/// The output's members are heard on the destination track, through its fader and pan; the other output is not there.
#[test]
fn a_channel_containers_extra_outputs_play_on_their_destinations() {
    let p = project();
    let a = Tone::new(0.8);
    let b = Tone::new(-0.3);
    let g = routed(1, 2, &["Main", "Aux", "Aux 2"], &[None, Some(1), Some(2)]);
    let c = chain(&a, &b, g.clone());
    let mut lib = SampleLibrary::default();
    let mut e = Engine::new(&p, &mut lib, 48_000, PlayMode::Pattern(1));
    e.clear_schedule();
    e.set_instrument(0, c.clone());
    e.set_track_capture(true);
    e.note_on(0, 60, 100);
    let mut heard = vec![Vec::new(); 4];
    audio(&mut e, 2048, &mut heard);

    assert_eq!(a.lock().unwrap().notes, [0], "the external note reached member 1 once");
    assert_eq!(b.lock().unwrap().notes, [0], "the external note reached member 2 once");
    let (al, ar) = peaks(&heard[1]);
    let (bl, br) = peaks(&heard[2]);
    assert!(al > 0.2, "member 1 on track 1's left: {al}");
    assert!(ar < 0.02, "track 1's pan keeps member 1 off its right: {ar}");
    assert!(br > 0.04, "member 2 on track 2's right: {br}");
    assert!(bl < 0.02, "track 2's pan keeps member 2 off its left: {bl}");
    assert!(heard[1].iter().zip(&heard[2]).any(|(x, y)| *x != 0.0 && *y != 0.0), "both outputs played");
    assert_eq!(onset(&heard[1]), Some(0), "the first note starts on its tick's first frame");
    assert_eq!(onset(&heard[2]), Some(0), "the other output starts on the same frame");
    // A graph update while audio runs is read on the next tick: a new output adds a destination, and removing it drops
    // that route. The instruments still receive each note exactly once.
    let g2 = graph(
        &["Main", "Aux", "Aux 2", "Aux 3"],
        &[None, Some(1), Some(2), Some(3)],
        vec![
            Edge { from: Endpoint::Member(1), to: Endpoint::Output(1) },
            Edge { from: Endpoint::Member(2), to: Endpoint::Output(2) },
            Edge { from: Endpoint::Member(1), to: Endpoint::Output(3) },
        ],
    );
    c.lock().unwrap().set_graph(g2).unwrap();
    e.note_on(0, 62, 100);
    let mut heard2 = vec![Vec::new(); 4];
    audio(&mut e, 2048, &mut heard2);
    let second = next_tick_frame(2048);
    assert_eq!(a.lock().unwrap().notes, [0, second], "member 1's second note at the next tick");
    assert_eq!(b.lock().unwrap().notes, [0, second], "member 2's second note at the next tick");
    assert_eq!(onset(&heard2[3]), Some(0), "the added output starts on the next captured tick's first frame");
    assert_eq!(onset(&heard2[1]), Some(0), "the old output starts there too");
    assert!(peaks(&heard2[3]).0 > 0.2, "the added output reaches track 3");
    assert!(peaks(&heard2[1]).0 > 0.2, "the old output still reaches track 1");

    // Remove the added output: no stale destination remains on the next tick.
    let g3 = routed(1, 2, &["Main", "Aux", "Aux 2"], &[None, Some(2), Some(1)]);
    c.lock().unwrap().set_graph(g3).unwrap();
    e.note_on(0, 64, 100);
    let mut heard3 = vec![Vec::new(); 4];
    audio(&mut e, 2048, &mut heard3);
    let third = next_tick_frame(4096);
    assert_eq!(a.lock().unwrap().notes, [0, second, third], "one note on per external note");
    assert_eq!(b.lock().unwrap().notes, [0, second, third], "one note on per external note");
    assert_eq!(onset(&heard3[2]), Some(0), "the re-routed first output starts on the third captured tick's first frame");
    assert_eq!(onset(&heard3[1]), Some(0), "the re-routed second output starts there too");
    assert!(peaks(&heard3[2]).1 > 0.04, "member 1 now plays on track 2's right");
    assert!(peaks(&heard3[1]).0 > 0.04, "member 2 now plays on track 1's left");
    assert_eq!(peaks(&heard3[3]), (0.0, 0.0), "the removed output is silent");
}

/// A mixer-slot container uses the same graph destinations: its extra outputs reach tracks the slot's effect does not
/// otherwise play on, and the source channel still feeds the container.
struct Gain(f32);

impl HostedEffect for Gain {
    fn process_fx(&mut self, _rate: u32, left: &mut [f32], right: &mut [f32]) {
        for x in left.iter_mut().chain(right) {
            *x *= self.0;
        }
    }
}

/// The slot container's extra outputs are heard through the mixer slot path, after the destination fader and pan.
#[test]
fn a_slot_containers_extra_outputs_play_on_their_destinations() {
    let mut p = project();
    p.channels[0].mixer_track = 1;
    p.mixer[3].pan = Some(1.0);
    let a = Tone::new(0.4);
    let g = graph(
        &["Main", "Aux", "Aux 2"],
        &[None, Some(2), Some(3)],
        vec![
            Edge { from: Endpoint::Input, to: Endpoint::Member(1) },
            Edge { from: Endpoint::Member(1), to: Endpoint::Output(1) },
            Edge { from: Endpoint::Input, to: Endpoint::Member(2) },
            Edge { from: Endpoint::Member(2), to: Endpoint::Output(2) },
        ],
    );
    let c = Chain::with_graph(
        vec![Member::new(1, Arc::new(Mutex::new(Gain(0.5))) as SharedEffect, 1.0, 0.0),
             Member::new(2, Arc::new(Mutex::new(Gain(-0.25))) as SharedEffect, 1.0, 0.0)],
        g,
    )
    .unwrap();
    let mut lib = SampleLibrary::default();
    let mut e = Engine::new(&p, &mut lib, 48_000, PlayMode::Pattern(1));
    e.clear_schedule();
    e.set_instrument(0, a);
    e.set_effect(1, 0, Arc::new(Mutex::new(c)) as SharedEffect);
    e.set_track_capture(true);
    e.note_on(0, 60, 100);
    let mut heard = vec![Vec::new(); 4];
    audio(&mut e, 2048, &mut heard);
    let (main_l, main_r) = peaks(&heard[1]);
    let (aux_l, aux_r) = peaks(&heard[2]);
    let (two_l, two_r) = peaks(&heard[3]);
    assert_eq!((main_l, main_r), (0.0, 0.0), "output 0 of the slot container is empty");
    assert!(aux_r > 0.01 && aux_l < 0.001, "the first extra output reaches track 2's right: {aux_l} {aux_r}");
    assert!(two_r > 0.005 && two_l < 0.001, "the second extra output reaches track 3's right: {two_l} {two_r}");
    assert_eq!(onset(&heard[2]), Some(0));
    assert_eq!(onset(&heard[3]), Some(0));
}

/// Muting a destination track silences only that output; the source container and the other destination still play.
#[test]
fn a_destination_tracks_mute_silences_its_output_only() {
    let mut p = project();
    p.mixer[2].enabled = false;
    let a = Tone::new(0.8);
    let b = Tone::new(-0.3);
    let c = chain(&a, &b, routed(1, 2, &["Main", "Aux", "Aux 2"], &[None, Some(1), Some(2)]));
    let mut lib = SampleLibrary::default();
    let mut e = Engine::new(&p, &mut lib, 48_000, PlayMode::Pattern(1));
    e.clear_schedule();
    e.set_instrument(0, c);
    e.set_track_capture(true);
    e.note_on(0, 60, 100);
    let mut heard = vec![Vec::new(); 4];
    audio(&mut e, 2048, &mut heard);
    assert_eq!(peaks(&heard[2]), (0.0, 0.0), "the muted destination is silent");
    assert!(peaks(&heard[1]).0 > 0.2, "the other destination still plays");
    assert_eq!(a.lock().unwrap().notes.len(), 1);
    assert_eq!(b.lock().unwrap().notes.len(), 1);
}
