// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

use std::sync::{Arc, Mutex, mpsc};
use os_engine::{Engine, PlayMode, SampleLibrary, HostedEffect, Instrument, NoteLevels};
use os_model::{Channel, ChannelKind, ControlPoint, MixerTrack, Note, Pattern, Project};

fn project() -> Project {
    Project {
        ppq:96, tempo:Some(120.0),
        channels:vec![Channel {
            id:os_model::Id::NONE,index:0,kind:ChannelKind::Sampler,name:None,plugin:None,
            plugin_state:None,color:None,enabled:true,volume:0.78125,pan:0.0,pitch_cents:0,
            mixer_track:0,sample_path:None,layer_children:vec![],envelopes:Default::default(),
            rack:Default::default(),misc:Default::default(),sound:Default::default(),
        }],
        patterns:vec![Pattern { number:1,notes:vec![Note {length:1,..Note::step(0,0)}],..Default::default() }],
        mixer:vec![MixerTrack::new(0)],..Default::default()
    }
}

struct Delay { ring: Vec<[f32;2]>, at: usize }
impl HostedEffect for Delay {
    fn process_fx(&mut self,_:u32,l:&mut[f32],r:&mut[f32]) {
        for (l,r) in l.iter_mut().zip(r) {
            let v=self.ring[self.at]; self.ring[self.at]=[*l,*r]; *l=v[0]; *r=v[1];
            self.at=(self.at+1)%self.ring.len();
        }
    }
}

#[test]
fn fixed_tail_preserves_an_echo_after_a_silent_gap_and_legacy_remains_explicit() {
    let render=|fixed:Option<u64>| {
        let mut e=Engine::new(&project(),&mut SampleLibrary::default(),48000,PlayMode::Pattern(1));
        e.set_effect(0,0,Arc::new(Mutex::new(Delay {ring:vec![[0.0;2];12000],at:0})));
        e.set_span(0,1); e.set_tail_frames(fixed); e.render_all(256)
    };
    let legacy=render(None);
    let safe=render(Some(48000));
    assert!(legacy.iter().all(|s|s.abs()<1e-6));
    assert!(safe.chunks_exact(2).skip(12000).any(|s|s.iter().any(|s|s.abs()>0.01)));
    assert!(safe.len()/2>=48250 && safe.len()/2<=48500);
}

#[derive(Default)]
struct Parameters { events:Vec<(u32,u32,f64)>, calls:usize }
impl HostedEffect for Parameters {
    fn process_fx(&mut self,_:u32,_:&mut[f32],_:&mut[f32]) {self.calls+=1;}
    fn param_fx(&mut self,o:u32,i:u32,v:f64) {self.events.push((o,i,v));}
    fn param_range_fx(&self,_:usize)->Option<(f64,f64)> {Some((0.0,1.0))}
}

#[test]
fn offline_one_shot_automation_is_identical_under_editor_lock_contention() {
    let render=|contended:bool| {
        let p=Project {ppq:96,tempo:Some(120.0),patterns:vec![Pattern {number:1,
            controls:vec![ControlPoint {position:0,kind:os_model::target::slot_param(0,0,0),value:32768}],
            ..Default::default()}],mixer:vec![MixerTrack::new(0)],..Default::default()};
        let mut e=Engine::new(&p,&mut SampleLibrary::default(),48000,PlayMode::Pattern(1));
        let probe=Arc::new(Mutex::new(Parameters::default())); e.set_effect(0,0,probe.clone()); e.set_span(0,96);
        let worker=contended.then(|| {
            let p=probe.clone();let (tx,rx)=mpsc::sync_channel(0);
            let t=std::thread::spawn(move|| {let guard=p.lock().unwrap();tx.send(()).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(150));drop(guard);});
            rx.recv().unwrap();t
        });
        e.render_all(256);
        if let Some(worker)=worker {worker.join().unwrap();}
        let p=probe.lock().unwrap();(p.events.clone(),p.calls)
    };
    let ordinary=render(false);let contended=render(true);
    assert_eq!(ordinary.0,[(0,0,0.5)]);
    assert_eq!(ordinary,contended);
}

#[derive(Default)]
struct Notes { events:Vec<(bool,u32,u8,u8)> }
impl Instrument for Notes {
    fn note_on(&mut self,o:u32,k:u8,_:f32) {self.events.push((true,o,k,0));}
    fn note_off(&mut self,o:u32,k:u8) {self.events.push((false,o,k,0));}
    fn note_start(&mut self,o:u32,k:u8,_:f32,_:f32,l:NoteLevels) {self.events.push((true,o,k,l.channel));}
    fn note_end(&mut self,o:u32,k:u8,ch:u8) {self.events.push((false,o,k,ch));}
    fn process(&mut self,_:u32,l:&mut[f32],r:&mut[f32]) {l.fill(0.0);r.fill(0.0);}
}

#[test]
fn timestamped_live_notes_retain_offsets_and_release_their_channel_and_source() {
    let mut e=Engine::new(&project(),&mut SampleLibrary::default(),48000,PlayMode::Pattern(1));
    let notes=Arc::new(Mutex::new(Notes::default()));e.set_instrument(0,notes.clone());e.clear_schedule();
    e.note_on_input(0,60,100,3,11,10);
    e.note_on_input(0,60,100,7,22,25);
    e.note_off_input(0,60,7,99,50); // Unknown source cannot release source22.
    e.note_off_input(0,60,7,22,100);
    e.note_off_input(0,60,3,11,180);
    e.next_tick().unwrap();
    assert_eq!(notes.lock().unwrap().events,[(true,10,60,3),(true,25,60,7),(false,100,60,7),(false,180,60,3)]);
}

#[test]
fn two_sources_mapped_to_the_same_midi_note_hold_until_both_release() {
    let mut e=Engine::new(&project(),&mut SampleLibrary::default(),48000,PlayMode::Pattern(1));
    let notes=Arc::new(Mutex::new(Notes::default()));e.set_instrument(0,notes.clone());e.clear_schedule();
    e.note_on_input(0,60,100,2,11,10);e.note_on_input(0,60,100,2,22,25);
    e.note_off_input(0,60,2,11,100);e.note_off_input(0,60,2,22,180);
    e.next_tick().unwrap();
    assert_eq!(notes.lock().unwrap().events,[(true,10,60,2),(false,180,60,2)]);
}

#[test]
fn duplicate_opaque_note_on_does_not_leave_a_plugin_note_held_after_one_off() {
    let mut e=Engine::new(&project(),&mut SampleLibrary::default(),48000,PlayMode::Pattern(1));
    let notes=Arc::new(Mutex::new(Notes::default()));e.set_instrument(0,notes.clone());e.clear_schedule();
    e.note_on_input(0,60,100,2,11,10);
    e.note_on_input(0,60,100,2,11,25); // Duplicate notification for the same source.
    e.note_off_input(0,60,2,11,100);
    e.next_tick().unwrap();
    assert_eq!(notes.lock().unwrap().events,[(true,10,60,2),(false,100,60,2)]);
}

#[test]
fn selected_track_capture_does_not_retain_unselected_stems() {
    let mut p=project();p.mixer.push(MixerTrack::new(1));p.mixer.push(MixerTrack::new(2));
    let mut e=Engine::new(&p,&mut SampleLibrary::default(),48000,PlayMode::Pattern(1));
    e.set_track_capture_selection(&[1]);let mut out=Vec::new();e.render_ticks(4,&mut out);
    let captured=e.take_track_audio();assert!(captured.len()>=3);
    assert!(captured[0].is_empty() && captured[2].is_empty());assert!(!captured[1].is_empty());
}
