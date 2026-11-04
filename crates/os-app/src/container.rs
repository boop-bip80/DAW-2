// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Containers, Lemon Studio's Patcher (slices 1 and 3): a mixer slot holding a graph-routed chain of effects, or a channel
//! whose instrument is such a chain led by instruments, each member with its own wet and dry levels. The slot menu's
//! Wrap in container moves the slot's effect into a new container in the same slot; Wrap all in container moves the
//! track's effects, in slot order, into one container in this slot and empties the others; the channel menu's Wrap in
//! container moves the channel's instrument into a container on the channel, which plays the channel's notes. The
//! container's window (`ContainerUi`, `views::container`) lists the members with their wet and dry knobs, removes,
//! moves, adds and replaces them and opens their windows. Each edit is one undo step.
//!
//! The project keeps a container as a slot or channel plugin of its own name whose state lists the members (`Member`):
//! each plugin's identity, kind, name, levels and saved state, and the container's routing graph. The chain is built
//! from that list and graph (`App::sync_containers`) and plays as the slot's effect or the channel's instrument
//! (`os_engine::chain::Chain`). A
//! container is named by its place, a track and a slot, the slot [`CHANNEL`] for a channel's (the track then being its
//! rack row). The members' instances are hosted under keys of their own (`plugins::MEMBER_KEY`), for their windows,
//! parameters and presets. A member's parameters and its wet and dry levels are automated and linked as parameters of
//! the container's slot or channel: the member's id and the item packed in the parameter index
//! (`os_model::target::member_item`), so that clips and links follow the member when it moves. Their windows' edits and
//! presets are undo steps by an owner id of their own (`control::command::member_owner`).
// spec: plugins/patcher/README.md §3.1, §3.3, §4 slices 1 and 3, §4.1, §4.2, §4.3, §4.4

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use os_engine::chain::{self, Chain};
use os_model::target::{self, Target as Auto, MEMBER_DRY, MEMBER_IDS, MEMBER_WET};
use os_model::{Id, Project, SlotPlugin};
pub use os_model::container::{Edge, Endpoint, Graph, NoteKind, NoteProcessor};

use crate::app::{App, State, Win};
use crate::knobs::KnobId;
use crate::playlist::{Cmd, Mark, PItem, Popup};
use crate::plugins::{fx_key, fx_of, Hosted, Plugins, FX_KEY, MEMBER_KEY};

/// The slot plugin name a container is kept under (Lemon Studio's own; the original's Patcher state is not read yet).
pub const CONTAINER: &str = "Lemon Container";

/// The state's first bytes and version (1: without the members' ids; 2: without their kinds; 3: without the switch;
/// 4: without the graph; 5: without the outputs' mixer destinations).
const MAGIC: &[u8; 4] = b"LCNT";
const VERSION: u32 = 6;

/// A member's kind in the state (version 3): bit 0 set for an instrument.
const INSTRUMENT: u32 = 1;

/// The slot a channel's container is named by: a container's place is (rack row, `CHANNEL`) on a channel, (track,
/// slot) in the mixer.
pub const CHANNEL: u8 = 0xFF;

/// One plugin of a container, as the project keeps it.
#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    /// Its number in the container, kept while it is moved (an edit keeps its instance and window).
    pub id: u32,
    /// An instrument (it plays the channel's notes; a channel's container only), else an effect.
    pub instrument: bool,
    /// The plugin's identity (`os_host::Descriptor::uid`) and name.
    pub uid: String,
    pub name: String,
    /// The output is `wet` times the effect's output plus `dry` times its input.
    pub wet: f32,
    pub dry: f32,
    /// Whether it plays. Switched off, it is skipped by the chain (its input passes through untouched) and adds no
    /// delay; its settings, levels, automation and links stay.
    pub on: bool,
    /// The plugin's own saved state (empty: the plugin's defaults).
    pub state: Vec<u8>,
}

/// The member ids of `members`.
fn member_ids(members: &[Member]) -> Vec<u32> {
    members.iter().map(|m| m.id).collect()
}

/// The serial graph of `members` (the route old projects and a fresh container have).
fn serial_graph(members: &[Member]) -> Graph {
    Graph::serial(&member_ids(members))
}

fn endpoint_bytes(e: Endpoint) -> (u32, u32) {
    match e {
        Endpoint::Input => (0, 0),
        Endpoint::Member(id) => (1, id),
        Endpoint::Output(i) => (2, u32::from(i)),
    }
}

fn endpoint_from_bytes(tag: u32, id: u32) -> Option<Endpoint> {
    match tag {
        0 => Some(Endpoint::Input),
        1 => Some(Endpoint::Member(id)),
        2 => Some(Endpoint::Output(u8::try_from(id).ok()?)),
        _ => None,
    }
}

/// The state of a container slot: the members in order and its graph.
pub fn encode(members: &[Member]) -> Vec<u8> {
    encode_state(members, &serial_graph(members))
}

fn encode_state(members: &[Member], graph: &Graph) -> Vec<u8> {
    let mut b = MAGIC.to_vec();
    b.extend(VERSION.to_le_bytes());
    b.extend((members.len() as u32).to_le_bytes());
    let bytes = |b: &mut Vec<u8>, v: &[u8]| {
        b.extend((v.len() as u32).to_le_bytes());
        b.extend_from_slice(v);
    };
    for m in members {
        b.extend(m.id.to_le_bytes());
        b.extend((if m.instrument { INSTRUMENT } else { 0 }).to_le_bytes());
        b.extend((if m.on { 1u32 } else { 0 }).to_le_bytes());
        bytes(&mut b, m.uid.as_bytes());
        bytes(&mut b, m.name.as_bytes());
        b.extend(m.wet.to_le_bytes());
        b.extend(m.dry.to_le_bytes());
        bytes(&mut b, &m.state);
    }
    b.extend((graph.outputs.len() as u32).to_le_bytes());
    for o in &graph.outputs {
        bytes(&mut b, o.as_bytes());
    }
    for i in 0..graph.outputs.len() {
        b.extend(u32::from(graph.destination(i).unwrap_or(u16::MAX)).to_le_bytes());
    }
    b.extend((graph.notes.len() as u32).to_le_bytes());
    for n in &graph.notes {
        bytes(&mut b, n.uid.as_bytes());
        bytes(&mut b, n.name.as_bytes());
        bytes(&mut b, &n.state);
    }
    b.extend((graph.edges.len() as u32).to_le_bytes());
    for e in &graph.edges {
        let (ft, fi) = endpoint_bytes(e.from);
        let (tt, ti) = endpoint_bytes(e.to);
        b.extend(ft.to_le_bytes());
        b.extend(fi.to_le_bytes());
        b.extend(tt.to_le_bytes());
        b.extend(ti.to_le_bytes());
    }
    b
}

/// The members a container slot's state lists; `None` for bytes that are not one.
pub fn decode(b: &[u8]) -> Option<Vec<Member>> {
    decode_state(b).map(|(m, _)| m)
}

/// The members and graph a container slot's state holds; states from before version 5 get the serial graph.
fn decode_state(b: &[u8]) -> Option<(Vec<Member>, Graph)> {
    if b.get(..4)? != MAGIC {
        return None;
    }
    let mut at = 4;
    let word = |at: &mut usize| -> Option<u32> {
        let w = u32::from_le_bytes(b.get(*at..*at + 4)?.try_into().ok()?);
        *at += 4;
        Some(w)
    };
    let bytes = |at: &mut usize| -> Option<Vec<u8>> {
        let n = word(at)? as usize;
        let s = b.get(*at..at.checked_add(n)?)?.to_vec();
        *at += n;
        Some(s)
    };
    let version = word(&mut at)?;
    if !(1..=VERSION).contains(&version) {
        return None;
    }
    let count = word(&mut at)?;
    let mut v = Vec::new();
    for i in 0..count {
        let id = if version >= 2 { word(&mut at)? } else { i + 1 };
        let instrument = version >= 3 && word(&mut at)? & INSTRUMENT != 0;
        let on = version < 4 || word(&mut at)? != 0;
        let uid = String::from_utf8(bytes(&mut at)?).ok()?;
        let name = String::from_utf8(bytes(&mut at)?).ok()?;
        let wet = f32::from_bits(word(&mut at)?);
        let dry = f32::from_bits(word(&mut at)?);
        let state = bytes(&mut at)?;
        v.push(Member { id, instrument, uid, name, wet, dry, on, state });
    }
    let graph = if version < 5 {
        serial_graph(&v)
    } else {
        let mut outputs = Vec::new();
        for _ in 0..word(&mut at)? {
            outputs.push(String::from_utf8(bytes(&mut at)?).ok()?);
        }
        if outputs.is_empty() || outputs.len() > usize::from(u8::MAX) + 1 {
            return None;
        }
        let destinations = if version >= 6 {
            let mut v = Vec::with_capacity(outputs.len());
            for _ in 0..outputs.len() {
                let d = word(&mut at)?;
                v.push((d != u32::from(u16::MAX)).then_some(u16::try_from(d).ok()?));
            }
            v
        } else {
            vec![None; outputs.len()]
        };
        let mut notes = Vec::new();
        for _ in 0..word(&mut at)? {
            notes.push(NoteProcessor {
                uid: String::from_utf8(bytes(&mut at)?).ok()?,
                name: String::from_utf8(bytes(&mut at)?).ok()?,
                state: bytes(&mut at)?,
            });
        }
        let mut edges = Vec::new();
        for _ in 0..word(&mut at)? {
            let from = endpoint_from_bytes(word(&mut at)?, word(&mut at)?)?;
            let to = endpoint_from_bytes(word(&mut at)?, word(&mut at)?)?;
            edges.push(Edge { from, to });
        }
        Graph { outputs, destinations, notes, edges }
    };
    Some((v, graph))
}

/// Whether channel `ch` is a container (its instrument a chain).
pub fn is_container_channel(ch: &os_model::Channel) -> bool {
    ch.kind == os_model::ChannelKind::Plugin && ch.plugin.as_deref() == Some(CONTAINER)
}

/// The state of the container at place (`t`, `k`): slot `k` of track `t`, or rack row `t`'s for `k` = [`CHANNEL`].
fn state_in(p: &Project, t: u16, k: u8) -> Option<&[u8]> {
    if k == CHANNEL {
        let ch = p.channels.get(usize::from(t)).filter(|c| is_container_channel(c))?;
        return ch.plugin_state.as_deref().or(Some(&[][..]));
    }
    let slot = p.mixer.iter().find(|m| m.index == t)?.slots.get(usize::from(k))?.as_ref()?;
    (slot.name == CONTAINER).then_some(slot.state.as_slice())
}

/// `state_in` to write.
fn state_in_mut(p: &mut Project, t: u16, k: u8) -> Option<&mut Vec<u8>> {
    if k == CHANNEL {
        let ch = p.channels.get_mut(usize::from(t)).filter(|c| is_container_channel(c))?;
        return Some(ch.plugin_state.get_or_insert_with(Vec::new));
    }
    let slot = p.mixer.iter_mut().find(|m| m.index == t)?.slots.get_mut(usize::from(k))?.as_mut()?;
    (slot.name == CONTAINER).then_some(&mut slot.state)
}

/// The members of the container at place (`t`, `k`) (a mixer slot, or a channel's for `k` = [`CHANNEL`]).
pub fn members(s: &State, t: u16, k: u8) -> Option<Vec<Member>> {
    state_all(&s.project, t, k).map(|(m, _)| m)
}

/// The members and graph of the container at place (`t`, `k`); a channel just made a container and not saved yet
/// holds an empty serial one.
fn state_all(p: &Project, t: u16, k: u8) -> Option<(Vec<Member>, Graph)> {
    let b = state_in(p, t, k)?;
    if b.is_empty() && k == CHANNEL {
        return Some((Vec::new(), serial_graph(&[])));
    }
    decode_state(b)
}

/// The routing graph of the container at place (`t`, `k`).
pub fn graph(s: &State, t: u16, k: u8) -> Option<Graph> {
    state_all(&s.project, t, k).map(|(_, g)| g)
}

/// How many members the container at place (`t`, `k`) holds, read from its state's header alone (the window's size,
/// worked out on every frame without decoding the members' states).
pub fn member_count(s: &State, t: u16, k: u8) -> Option<usize> {
    let b = state_in(&s.project, t, k)?;
    let word = |at: usize| Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?));
    (b.get(..4)? == MAGIC && (1..=VERSION).contains(&word(4)?)).then(|| word(8).map(|n| n as usize)).flatten()
}

/// A level knob's hint: "Lemon Limiter wet: 60%".
pub fn level_text(name: &str, wet: bool, v: f32) -> String {
    format!("{name} {}: {:.0}%", if wet { "wet" } else { "dry" }, v * 100.0)
}

/// Writes `list` as the state of the container at place (`t`, `k`), keeping its graph: a serial graph follows the new
/// list (an add, remove or move), while a graph edited by hand only drops connections to removed members.
fn set_members(s: &mut State, t: u16, k: u8, list: &[Member]) {
    let old = state_all(&s.project, t, k);
    let graph = match old {
        Some((old, graph)) if graph != serial_graph(&old) => graph.pruned(&member_ids(list)),
        _ => Graph::serial(&member_ids(list)),
    };
    set_graph(s, t, k, list, &graph);
}

/// Writes `list` and `graph` as the state of the container at place (`t`, `k`).
fn set_graph(s: &mut State, t: u16, k: u8, list: &[Member], graph: &Graph) {
    if let Some(b) = state_in_mut(&mut s.project, t, k) {
        *b = encode_state(list, graph);
    }
}

/// The `Plugins::containers` key of the container at place (`t`, `k`): its slot's `fx_key`, or its channel's row.
pub fn place_key(t: u16, k: u8) -> usize {
    if k == CHANNEL {
        usize::from(t)
    } else {
        fx_key(t, k)
    }
}

/// The place of the container under `Plugins::containers` key `key` (`place_key`'s other way).
pub fn key_place(key: usize) -> Option<(u16, u8)> {
    if key < FX_KEY {
        return Some((u16::try_from(key).ok()?, CHANNEL));
    }
    fx_of(key)
}

/// The project id of the slot plugin or channel holding the container at place (`t`, `k`).
pub fn place_id(s: &State, t: u16, k: u8) -> Option<Id> {
    let p = &s.project;
    if k == CHANNEL {
        return p.channels.get(usize::from(t)).map(|c| c.id);
    }
    p.mixer.iter().find(|m| m.index == t)?.slots.get(usize::from(k))?.as_ref().map(|s| s.id)
}

/// The place of the container held by the slot plugin or channel with project id `id` (`place_id`'s other way).
pub fn id_place(s: &State, id: u32) -> Option<(u16, u8)> {
    let p = &s.project;
    if let Some(r) = p.channels.iter().position(|c| c.id.0 == id) {
        return Some((u16::try_from(r).ok()?, CHANNEL));
    }
    p.mixer.iter().find_map(|m| Some((m.index, m.slots.iter().position(|s| s.as_ref().is_some_and(|s| s.id.0 == id))? as u8)))
}

/// What the container at place (`t`, `k`) is shown as: its track's name for a slot's ("Bus 2"), its channel's name.
pub fn place_name(s: &State, t: u16, k: u8) -> String {
    if k != CHANNEL {
        return crate::mixer::name(s, t);
    }
    let ch = s.project.channels.get(usize::from(t));
    ch.and_then(|c| c.name.clone()).unwrap_or_else(|| format!("Channel {}", usize::from(t) + 1))
}

/// The container's title, "Container (Bus 2)": its slot's label, or for a channel's "Container (Lemon Trio)".
pub fn title(s: &State, t: u16, k: u8) -> Option<String> {
    if k != CHANNEL {
        return crate::params::effect_label(s, t, k);
    }
    state_in(&s.project, t, k)?;
    Some(format!("Container ({})", place_name(s, t, k)))
}

/// The automation target of item `item` of the container at place (`t`, `k`): a parameter of its slot or channel.
pub fn item_target(s: &State, t: u16, k: u8, item: u16) -> Option<u32> {
    if k == CHANNEL {
        return Some(target::channel_param(s.project.channels.get(usize::from(t))?.index, item));
    }
    Some(target::slot_param(t, k, item))
}

/// The container at place (`t`, `k`) and the item an automation target on its slot's or channel's parameters names.
pub fn target_place(s: &State, tg: Auto) -> Option<(u16, u8, u16)> {
    let (t, k, i) = match tg {
        Auto::SlotParam(m, k, i) => (m, k, i),
        Auto::ChannelParam(ch, i) => (u16::try_from(s.project.channels.iter().position(|c| c.index == ch)?).ok()?, CHANNEL, i),
        _ => return None,
    };
    state_in(&s.project, t, k)?;
    Some((t, k, i))
}

/// The wet and dry levels of a slot at stored mix `mix` (0..1) and switch `on`, as the slot blends them (spec:
/// audio/mixer.md "An effect slot"): an effect switched off keeps only its dry part (the manual's "mix 0").
fn levels(mix: f64, on: bool) -> (f32, f32) {
    if !on {
        return (0.0, 1.0);
    }
    let d = os_dsp::mixer::dry_level((mix.clamp(0.0, 1.0) * 12800.0).round() as i32);
    (1.0 - d, d)
}

/// A container being played: the slot's or channel's id, the state its chain was built from, the members' instances
/// and the chain.
pub struct Live {
    pub id: Id,
    /// The slot's state the chain was built from: compared every tick, decoded only when it changed.
    src: Vec<u8>,
    /// The members' ids and identities by position (another list rebuilds the chain; other levels only set them).
    shape: Vec<(u32, String)>,
    /// Whether it plays a channel (its members' instruments play the channel's notes).
    pub channel: bool,
    /// The members' `hosted` keys by position (`None`: its effect was not found, left out of the chain).
    pub keys: Vec<Option<usize>>,
    pub fx: Arc<Mutex<Chain>>,
}

fn shape(members: &[Member]) -> Vec<(u32, String)> {
    members.iter().map(|m| (m.id, m.uid.clone())).collect()
}

/// The undo step names of the menu entries.
/// The slot menu's Wrap in container and the channel menu's (the original's Patcherize) are named alike.
pub const WRAP: &str = "Wrap in container";
pub const WRAP_ALL: &str = "Wrap all in container";
pub const REMOVE: &str = "Remove effect from container";
pub const MOVE: &str = "Move effect in container";
pub const ADD: &str = "Add effect to container";
pub const ADD_INSTRUMENT: &str = "Add instrument to container";
pub const REPLACE: &str = "Replace effect in container";
pub const LEVEL: &str = "Effect level in container";
pub const SWITCH: &str = "Switch effect in container";

/// A new instrument member's levels: its sound added to what the members before it put out (a layer).
pub const INSTRUMENT_LEVELS: (f32, f32) = (1.0, 1.0);

/// The most effects a container holds (the ids its members' automation targets have room for).
pub const MAX_MEMBERS: usize = MEMBER_IDS as usize;

/// The undo step of a headless graph edit.
pub const GRAPH: &str = "Edit container graph";

/// The container's window (Lemon Studio's design: spec plugins/patcher/README.md §4.3): where it is, the container it
/// shows, and the levels that container's chain plays, read every tick (`App::mirror_levels`) so that an automated
/// level turns its knob.
#[derive(Debug, Clone)]
pub struct ContainerUi {
    pub pos: (f32, f32),
    /// The place of the container shown: track and slot, or rack row and [`CHANNEL`].
    pub at: (u16, u8),
    /// The chain's wet and dry by member id, as last read.
    pub live: BTreeMap<u32, (f32, f32)>,
}

impl Default for ContainerUi {
    fn default() -> Self {
        ContainerUi { pos: (420.0, 160.0), at: (0, 0), live: BTreeMap::new() }
    }
}

/// The container graph window: where it is, and whether it has been placed (first opening puts it beside the
/// container's window; afterwards it stays where it was left).
#[derive(Debug, Clone)]
pub struct GraphUi {
    pub pos: (f32, f32),
    pub placed: bool,
}

impl Default for GraphUi {
    fn default() -> Self {
        GraphUi { pos: (452.0, 172.0), placed: false }
    }
}

/// What the window's controls do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CAct {
    /// Member `.0`'s name (its place in the list): opens its window; a right click, its menu.
    Open(usize),
    /// Member `.0`'s ▸ button: its menu.
    Menu(usize),
    /// Add effect: the effect list of an empty slot (a channel's container: the instruments too).
    Add,
    /// The Graph... button: the container's routing graph window.
    Graph,
}

/// The graph window's buttons. A connection is made from Connect..., whose menu lists the sources and, under each, the
/// destinations; the other menus list everything so that nothing is out of reach when the window is full.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GAct {
    /// The source and destination menus of a new connection.
    ConnectMenu,
    AddOutput,
    /// The mixer tracks, to add an output already routed to one (the graph window's Add output...).
    AddOutputMenu,
    /// Every named output: route it to a mixer track, or remove it (the first stays).
    OutputMenu,
    /// Every connection, to remove one.
    DisconnectMenu,
    /// A connection's × button.
    Disconnect(Endpoint, Endpoint),
    /// An output's × button.
    RemoveOutput(u8),
    /// A note processor's × button.
    RemoveNote(usize),
    /// Every note processor, to add, edit, move or remove one.
    NoteMenu,
}

/// Shows the window of the container at place (`t`, `slot`).
pub fn show(s: &mut State, t: u16, slot: u8) {
    if s.container.at != (t, slot) {
        s.container.live.clear();
    }
    s.container.at = (t, slot);
    s.raise(Win::Container);
}

/// Shows the container's graph window, beside the container's window the first time.
pub fn show_graph(s: &mut State) {
    if !s.graph.placed {
        let (w, h) = (crate::views::graph::W, crate::views::graph::H);
        s.graph.pos = (
            (s.container.pos.0 + 28.0).clamp(0.0, (s.screen.0 - w).max(0.0)),
            (s.container.pos.1 + 28.0).clamp(0.0, (s.screen.1 - h).max(0.0)),
        );
        s.graph.placed = true;
    }
    s.raise(Win::Graph);
}

/// What the graph calls a node: "Input", "1. Lemon Limiter" or "Output 2".
pub fn node_name(list: &[Member], e: Endpoint) -> String {
    match e {
        Endpoint::Input => "Input".into(),
        Endpoint::Member(id) => list
            .iter()
            .position(|m| m.id == id)
            .map_or_else(|| format!("Effect {id}"), |i| format!("{}. {}", i + 1, list[i].name)),
        Endpoint::Output(i) => format!("Output {}", u16::from(i) + 1),
    }
}

/// What a member's level knob shows: the level the chain plays while the window shows its container, else the list's.
pub fn shown_level(s: &State, t: u16, k: u8, id: u32, wet: bool) -> f32 {
    let live = s.container.live.get(&id).filter(|_| s.container.at == (t, k)).map(|l| if wet { l.0 } else { l.1 });
    live.or_else(|| level(s, t, k, id, wet)).unwrap_or(0.0)
}

/// A level knob turned (`State::set_knob`): the list takes the level, no undo step (a drag's is taken at its press), and
/// the knob shows it until the chain plays it.
pub fn put_level(s: &mut State, t: u16, k: u8, id: u32, wet: bool, v: f32) {
    let Some(list) = leveled(s, t, k, id, wet, v) else { return };
    set_members(s, t, k, &list);
    if s.container.at == (t, k) {
        if let Some(l) = s.container.live.get_mut(&id) {
            *(if wet { &mut l.0 } else { &mut l.1 }) = v;
        }
    }
    s.piano.dirty = true;
}

/// The right-click menu of a member's level knob: Reset, then its level's parameter menu (`params::level_items`).
pub fn knob_menu(s: &State, id: KnobId, x: f32, y: f32) -> Option<Popup> {
    let KnobId::Member(t, k, m, wet) = id else { return None };
    let mut items = vec![PItem::new("Reset", Cmd::Reset(crate::knobs::Control::Knob(id)))];
    items.extend(crate::params::level_items(s, level_automation(s, t, k, m, wet), crate::typein::Target::Member(t, k, m, wet)));
    Some(Popup { x, y, items, sub: None, scroll: [0; 2] })
}

/// The commands of the window's menus run with their container's track selected (as a slot window's do); a channel's
/// container's run with its row in the track's place, which is how they find it.
fn on_track(t: u16, items: &mut [PItem]) {
    for it in items.iter_mut() {
        if let Cmd::Mixer(_) = it.cmd {
            it.cmd = Cmd::SlotWin(t, Box::new(std::mem::replace(&mut it.cmd, Cmd::Nothing)));
        }
        on_track(t, &mut it.children);
    }
}

/// What the container's list does to a member.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    /// Opens its plugin window (no undo step).
    Open,
    /// Switches it off, or on again: the chain skips it, passing the block through.
    Switch,
    Up,
    Down,
    Remove,
}

/// The container's headless graph edits (`App::edit_graph`): the state the graph editor edits. Each is one undo step,
/// and the live chain follows the saved graph on its next tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphCmd {
    Connect(Endpoint, Endpoint),
    Disconnect(Endpoint, Endpoint),
    AddOutput,
    /// Add an output already pointed at mixer track `.0` (the graph window's Add output...): one undo step.
    AddOutputTo(u16),
    /// Point output `.0` at mixer track `.1` (`None`: no destination; output 0 always stays on its slot or channel).
    RouteOutput(u8, Option<u16>),
    RemoveOutput(u8),
    AddNote { uid: String, name: String },
    /// Add one of the built-in note processors (the graph window's Notes... menu).
    AddNoteKind(NoteKind),
    /// Replace note processor `.0` with built-in kind `.1`.
    SetNote(usize, NoteKind),
    /// Move note processor `.0` to position `.1`, shifting the others.
    MoveNote(usize, usize),
    RemoveNote(usize),
}

/// Whether slot `slot` of the current track can be wrapped: a hosted effect, or (for Wrap all) a container.
pub fn can_wrap(app: &App, slot: u8, all: bool) -> bool {
    let t = app.state.selected_mixer;
    app.plugins.hosted.contains_key(&fx_key(t, slot)) || (all && members(&app.state, t, slot).is_some())
}

/// Whether rack row `row`'s instrument can be wrapped: a plugin channel whose plugin is hosted (not a sampler, whose
/// sample is the channel's).
pub fn can_wrap_channel(s: &State, row: usize) -> bool {
    let ch = s.project.channels.get(row);
    ch.is_some_and(|c| c.kind == os_model::ChannelKind::Plugin && !is_container_channel(c)) && s.hosted_rows.contains(&row)
}

fn pct(v: f32) -> i32 {
    (v * 100.0).round() as i32
}

/// The wet (`wet`) or dry level of member `id` of the container in slot `k` of track `t`.
pub fn level(s: &State, t: u16, k: u8, id: u32, wet: bool) -> Option<f32> {
    members(s, t, k)?.into_iter().find(|m| m.id == id).map(|m| if wet { m.wet } else { m.dry })
}

/// The type-in box's caption: "Bus 2 - Lemon Limiter wet" (a channel's container: the channel's name first).
pub fn level_name(s: &State, t: u16, k: u8, id: u32, wet: bool) -> String {
    let name = members(s, t, k).and_then(|l| l.into_iter().find(|m| m.id == id)).map_or_else(|| "Effect".into(), |m| m.name);
    format!("{} - {name} {}", place_name(s, t, k), if wet { "wet" } else { "dry" })
}

/// Sets the wet (`wet`) or dry level of member `id` of the container in slot `k` of track `t` to `v` (0..1): one undo
/// step. The chain takes the level on the next tick, keeping its instances (`App::sync_containers`).
pub fn set_level(s: &mut State, t: u16, k: u8, id: u32, wet: bool, v: f32) {
    let Some(list) = leveled(s, t, k, id, wet, v) else { return };
    crate::control::edit(s, LEVEL, region(t, k), |s| set_members(s, t, k, &list));
    s.piano.dirty = true;
}

/// `set_level` from a controller (a remote link): one undo step while the same level keeps moving, none while Undo
/// knob tweaks is off, as a knob's.
pub fn set_level_run(s: &mut State, t: u16, k: u8, id: u32, wet: bool, v: f32) {
    let Some(list) = leveled(s, t, k, id, wet, v) else { return };
    let run = crate::history::Run::Member(t, k, id, wet);
    crate::control::run_edit(s, LEVEL, region(t, k), run, true, |s| set_members(s, t, k, &list));
    s.piano.dirty = true;
}

/// What an edit of the container at place (`t`, `k`) changes, for its undo step: its track, or its channel.
fn region(t: u16, k: u8) -> crate::control::command::Region {
    use crate::control::command::Region;
    if k == CHANNEL {
        Region::Channel(usize::from(t))
    } else {
        Region::Track(t)
    }
}

/// The list with the level set (`None`: no such member, or the level is `v` already).
fn leveled(s: &State, t: u16, k: u8, id: u32, wet: bool, v: f32) -> Option<Vec<Member>> {
    let mut list = members(s, t, k)?;
    let m = list.iter_mut().find(|m| m.id == id)?;
    let level = if wet { &mut m.wet } else { &mut m.dry };
    if *level == v {
        return None;
    }
    *level = v;
    Some(list)
}

/// The automation target of member `id`'s wet (`wet`) or dry level in the container at place (`t`, `k`) (spec:
/// plugins/patcher/README.md §4.2, §4.4).
pub fn level_target(s: &State, t: u16, k: u8, id: u32, wet: bool) -> Option<u32> {
    item_target(s, t, k, target::member_item(id, if wet { MEMBER_WET } else { MEMBER_DRY })?)
}

/// The member level an automation target names, when it is one: track (or row), slot (or [`CHANNEL`]), member id, wet.
pub fn level_of(s: &State, tg: Auto) -> Option<(u16, u8, u32, bool)> {
    let (m, k, i) = target_place(s, tg)?;
    let (id, item) = target::member_of(i)?;
    (item >= MEMBER_WET && members(s, m, k)?.iter().any(|x| x.id == id)).then_some((m, k, id, item == MEMBER_WET))
}

/// "Create automation clip" on a member's level: target, clip name ("Bus 2 - Lemon Limiter wet"), current value.
pub fn level_automation(s: &State, t: u16, k: u8, id: u32, wet: bool) -> Option<(u32, String, f64)> {
    Some((level_target(s, t, k, id, wet)?, level_name(s, t, k, id, wet), f64::from(level(s, t, k, id, wet)?)))
}

/// The member that the container under `Plugins::containers` key `place` holds under id `id`: its hosted key, when its
/// plugin is loaded.
pub fn member_key(s: &State, place: usize, id: u32) -> Option<usize> {
    let (t, k) = key_place(place)?;
    s.params.members.iter().find(|(_, m)| **m == (t, k, id)).map(|(&key, _)| key)
}

/// The name of the member hosted under `key` and its container's title, "Lemon Limiter in Container (Bus 2)".
pub fn member_label(s: &State, key: usize) -> Option<String> {
    let &(t, k, id) = s.params.members.get(&key)?;
    let name = members(s, t, k)?.into_iter().find(|m| m.id == id)?.name;
    Some(format!("{name} in {}", title(s, t, k)?))
}

/// Whether member `id` of the container at place (`t`, `k`) is an instrument.
pub fn is_instrument(s: &State, t: u16, k: u8, id: u32) -> bool {
    members(s, t, k).is_some_and(|l| l.iter().any(|m| m.id == id && m.instrument))
}

/// The smallest id no member of `list` has, up to the targets' limit (`None`: the container is full).
fn free_id(list: &[Member]) -> Option<u32> {
    (1..=MEMBER_IDS).find(|id| list.iter().all(|m| m.id != *id))
}

fn label(i: usize, m: &Member) -> String {
    format!("{}. {} (wet {}%, dry {}%)", i + 1, m.name, pct(m.wet), pct(m.dry))
}

/// The slot menu's Container ▸ for a container in slot `slot` of the current track: its members, "1. Lemon Limiter
/// (wet 100%, dry 0%)", each opening its window, then Edit list... (the container's window, `show`).
pub fn list_items(s: &State, slot: u8) -> Option<Vec<PItem>> {
    let m = members(s, s.selected_mixer, slot)?;
    let mx = |i: usize, op: Op| Cmd::Mixer(crate::mixer::MCmd::Member(slot, i, op));
    let mut v: Vec<PItem> = m.iter().enumerate().map(|(i, m)| PItem::new(label(i, m), mx(i, Op::Open))).collect();
    if v.is_empty() {
        v.push(PItem::new("(empty)", Cmd::Nothing).off());
    }
    v.push(PItem::sep());
    v.push(PItem::new("Edit list...", Cmd::Mixer(crate::mixer::MCmd::ContainerList(slot))));
    Some(v)
}

/// Member `i`'s menu in the container's window, on the container in slot `slot` of track `t` (its Move and Remove act on
/// the selected track's: `on_track` for another): Open editor, Wet: …% ▸ and Dry: …% ▸ (the parameter menu:
/// automation, remote control, the value box), Move up, Move down, Remove.
fn member_ops(s: &State, t: u16, slot: u8, m: &[Member], i: usize) -> Vec<PItem> {
    let mx = |op: Op| Cmd::Mixer(crate::mixer::MCmd::Member(slot, i, op));
    let on = |it: PItem, yes: bool| if yes { it } else { it.off() };
    // A level's submenu: the parameter menu of a knob (automation, remote control, value), Enter value... in percents.
    let level = |id: u32, wet: bool, v: f32| {
        let label = format!("{}: {}%", if wet { "Wet" } else { "Dry" }, pct(v));
        let create = level_automation(s, t, slot, id, wet);
        PItem::sub(label, crate::params::level_items(s, create, crate::typein::Target::Member(t, slot, id, wet)))
    };
    let mem = &m[i];
    vec![
        PItem::new("Open editor", mx(Op::Open)),
        level(mem.id, true, mem.wet),
        level(mem.id, false, mem.dry),
        PItem::new(if mem.on { "Switch off" } else { "Switch on" }, mx(Op::Switch)),
        on(PItem::new("Move up", mx(Op::Up)), i > 0),
        on(PItem::new("Move down", mx(Op::Down)), i + 1 < m.len()),
        PItem::new("Remove", mx(Op::Remove)),
    ]
}

/// Before an edit of the containers' lists: each member's state as its instance holds it now, so that the edit (and its
/// undo) keeps what was set in the members' windows. A save stores them the same way (`project_plugins::store_hosted`).
pub fn store_members(proj: &mut Project, plugins: &Plugins) {
    for (&key, live) in &plugins.containers {
        let Some((t, k)) = key_place(key) else { continue };
        let Some((mut list, graph)) = state_all(proj, t, k) else { continue };
        let mut changed = false;
        for m in &mut list {
            let Some(at) = live.shape.iter().position(|s| (s.0, s.1.as_str()) == (m.id, m.uid.as_str())) else { continue };
            let Some(h) = live.keys[at].and_then(|k| plugins.hosted.get(&k)) else { continue };
            if let Some(state) = h.plugin.lock().ok().and_then(|p| p.save_state()).filter(|st| *st != m.state) {
                m.state = state;
                changed = true;
            }
        }
        if changed {
            if let Some(p) = state_in_mut(proj, t, k) {
                *p = encode_state(&list, &graph);
            }
        }
    }
}

/// Where a wrapped slot's controls went: the member that took its plugin, and, for a slot that was a container
/// itself, the ids its members had and the ids they have in the new container.
struct Moved {
    member: u32,
    container: bool,
    members: Vec<(u32, u32)>,
}

/// The target a control that wrapping track `t`'s slot `slot` into a container in slot `to` moved now names: a slot
/// parameter becomes the parameter of the member that took the slot's plugin, and a container member's parameter that
/// member's parameter in the new container. `None` when the target names something else, or a plugin parameter beyond
/// what a member holds.
fn wrapped_target(tg: u32, t: u16, to: u8, moves: &BTreeMap<u8, Moved>) -> Option<u32> {
    let Auto::SlotParam(track, slot, k) = target::decode(tg) else { return None };
    if track != t {
        return None;
    }
    let m = moves.get(&slot)?;
    let (id, item) = if m.container {
        let (old, item) = target::member_of(k)?;
        (m.members.iter().find(|(id, _)| *id == old)?.1, item)
    } else {
        (m.member, k)
    };
    if item >= target::MEMBER_ITEMS {
        return None;
    }
    target::member_item(id, item).map(|k| target::slot_param(t, to, k))
}

/// The target a control that wrapping rack row `row`'s instrument moved now names: channel index `ch`'s plugin
/// parameter `k` becomes member 1's parameter in the channel's container.
fn wrapped_channel_target(tg: u32, ch: u16) -> Option<u32> {
    let Auto::ChannelParam(c, k) = target::decode(tg) else { return None };
    if c != ch || k >= target::MEMBER_ITEMS {
        return None;
    }
    target::member_item(1, k).map(|k| target::channel_param(ch, k))
}

/// Moves every automation clip target and controller link target that `f` maps, so that clips and links follow what
/// they drove into a container. Returns how many moved. Inside the wrap's undo step.
fn move_targets(s: &mut State, f: &dyn Fn(u32) -> Option<u32>) -> usize {
    let mut n = 0;
    for a in &mut s.project.automations {
        for tg in &mut a.targets {
            if let Some(new) = f(*tg) {
                *tg = new;
                n += 1;
            }
        }
    }
    for l in &mut s.project.controller_links {
        if let Some(new) = f(l.target()) {
            l.set_target(new);
            n += 1;
        }
    }
    n
}

/// Whether endpoint `e` exists in `graph` and may start (`from`) or end a connection.
fn endpoint_ok(members: &[Member], graph: &Graph, e: Endpoint, from: bool) -> bool {
    match e {
        Endpoint::Input => from,
        Endpoint::Output(i) => !from && usize::from(i) < graph.outputs.len(),
        Endpoint::Member(id) => members.iter().any(|m| m.id == id),
    }
}

/// Whether adding `edge` would make a loop through the graph's connections.
fn loops(graph: &Graph, edge: Edge) -> bool {
    if edge.from == edge.to {
        return true;
    }
    let mut stack = vec![edge.to];
    let mut seen = Vec::new();
    while let Some(at) = stack.pop() {
        if at == edge.from {
            return true;
        }
        if seen.contains(&at) {
            continue;
        }
        seen.push(at);
        for e in &graph.edges {
            if e.from == at {
                stack.push(e.to);
            }
        }
    }
    false
}

/// A note processor's menu entries: add one of the built-in kinds, or edit its settings, order or remove it.
fn note_add_items(k: u8, ppq: u16) -> Vec<PItem> {
    let delay = (u32::from(ppq) / 4).max(1);
    vec![
        PItem::new("Transpose +12 semitones", Cmd::Mixer(crate::mixer::MCmd::Graph(k, GraphCmd::AddNoteKind(NoteKind::Transpose { semitones: 12 })))),
        PItem::new("Velocity 50%", Cmd::Mixer(crate::mixer::MCmd::Graph(k, GraphCmd::AddNoteKind(NoteKind::Velocity { percent: 50 })))),
        PItem::new("Delay a 16th note", Cmd::Mixer(crate::mixer::MCmd::Graph(k, GraphCmd::AddNoteKind(NoteKind::Delay { ticks: delay })))),
    ]
}

fn note_children(k: u8, ppq: u16, i: usize, n: &NoteProcessor, count: usize) -> Vec<PItem> {
    let mut items = Vec::new();
    let on = |it: PItem, yes: bool| if yes { it } else { it.off() };
    match n.kind() {
        Err(e) => items.push(PItem::new(e, Cmd::Nothing).off()),
        Ok(NoteKind::Transpose { semitones }) => {
            let set = |v: i32, label: &str| PItem::new(
                label.to_string(),
                Cmd::Mixer(crate::mixer::MCmd::Graph(k, GraphCmd::SetNote(i, NoteKind::Transpose { semitones: v.clamp(-48, 48) as i8 }))),
            );
            items.push(PItem::new(format!("Now: {semitones:+} semitones"), Cmd::Nothing).off());
            items.push(set(i32::from(semitones) + 1, "Up a semitone"));
            items.push(set(i32::from(semitones) - 1, "Down a semitone"));
            items.push(set(i32::from(semitones) + 12, "Up an octave"));
            items.push(set(i32::from(semitones) - 12, "Down an octave"));
        }
        Ok(NoteKind::Velocity { percent }) => {
            for v in [25u8, 50, 75, 100, 125, 150, 200] {
                let mut item = PItem::new(
                    format!("{v}%"),
                    Cmd::Mixer(crate::mixer::MCmd::Graph(k, GraphCmd::SetNote(i, NoteKind::Velocity { percent: v }))),
                );
                if v == percent {
                    item = item.mark(Mark::Check(true));
                }
                items.push(item);
            }
        }
        Ok(NoteKind::Delay { ticks }) => {
            let ppq = u32::from(ppq).max(1);
            let mut choices = vec![("No delay", 0u32), ("1/16 note", ppq / 4), ("1/8 note", ppq / 2), ("1/4 note", ppq), ("1/2 note", ppq * 2), ("1 bar", ppq * 4)];
            for (label, v) in choices.drain(..) {
                let mut item = PItem::new(
                    format!("{label} ({v} ticks)"),
                    Cmd::Mixer(crate::mixer::MCmd::Graph(k, GraphCmd::SetNote(i, NoteKind::Delay { ticks: v }))),
                );
                if v == ticks {
                    item = item.mark(Mark::Check(true));
                }
                items.push(item);
            }
        }
    }
    items.push(PItem::sep());
    items.push(on(
        PItem::new("Move up", Cmd::Mixer(crate::mixer::MCmd::Graph(k, GraphCmd::MoveNote(i, i.saturating_sub(1))))),
        i > 0,
    ));
    items.push(on(
        PItem::new("Move down", Cmd::Mixer(crate::mixer::MCmd::Graph(k, GraphCmd::MoveNote(i, (i + 1).min(count.saturating_sub(1)))))),
        i + 1 < count,
    ));
    items.push(PItem::new("Remove", Cmd::Mixer(crate::mixer::MCmd::Graph(k, GraphCmd::RemoveNote(i)))));
    items
}

impl App {
    /// Wrap in container (`all` = false) and Wrap all in container on slot `slot` of the current track (spec: plugins/
    /// patcher/README.md §3.3). The slot's effect, or every effect of the track in slot order, becomes a member with the
    /// levels its slot blended it at; the container takes the slot, switched on at full mix, and keeps the slot's name,
    /// colour and icon; the other wrapped slots are emptied with theirs. A slot whose plugin is not hosted stays where it
    /// is (it has no state to move); a container wrapped by Wrap all brings its members. Run inside the undo step
    /// (`mixer::run`).
    pub(crate) fn wrap_in_container(&mut self, slot: u8, all: bool) -> Option<String> {
        let t = self.state.selected_mixer;
        let track = crate::mixer::track(&self.state, t)?;
        let slots: Vec<u8> = if all { (0..10u8).filter(|&k| track.slots[usize::from(k)].is_some()).collect() } else { vec![slot] };
        let mut list = Vec::new();
        let mut taken = Vec::new();
        let mut left = Vec::new();
        // Where each wrapped slot's effect went in the list: its first member's index, and the ids its members had
        // when it was a container itself (`None` for a plain effect).
        let mut wrapped_at: Vec<(u8, usize, Option<Vec<u32>>)> = Vec::new();
        for k in slots {
            let (mix, on) = (track.slot_mix[usize::from(k)], track.slot_on[usize::from(k)]);
            if let Some(inner) = members(&self.state, t, k) {
                wrapped_at.push((k, list.len(), Some(inner.iter().map(|m| m.id).collect())));
                list.extend(inner);
                taken.push(k);
                continue;
            }
            let Some(h) = self.plugins.hosted.get(&fx_key(t, k)) else {
                left.push(k);
                continue;
            };
            let Ok(p) = h.plugin.lock() else {
                left.push(k);
                continue;
            };
            let (wet, dry) = levels(mix, on);
            wrapped_at.push((k, list.len(), None));
            list.push(Member { id: 0, instrument: false, uid: p.desc().uid(), name: p.desc().name.clone(), wet, dry, on: true, state: p.save_state().unwrap_or_default() });
            taken.push(k);
        }
        if !taken.contains(&slot) {
            return Some("This slot's plugin is not hosted: nothing to wrap".into());
        }
        if list.len() > MEMBER_IDS as usize {
            return Some(format!("A container holds at most {MEMBER_IDS} effects"));
        }
        for (i, m) in list.iter_mut().enumerate() {
            m.id = i as u32 + 1;
        }
        let n = list.len();
        // The clips and links that drove the wrapped slots follow their effects into the container.
        let moves: BTreeMap<u8, Moved> = wrapped_at
            .into_iter()
            .map(|(k, at, ids)| {
                let container = ids.is_some();
                let old = ids.unwrap_or_default();
                let members = old.iter().enumerate().map(|(i, &id)| (id, (at + i + 1) as u32)).collect();
                (k, Moved { member: (at + 1) as u32, container, members })
            })
            .collect();
        move_targets(&mut self.state, &|tg| wrapped_target(tg, t, slot, &moves));
        // The slot's name, colour and icon go with the container (`clear_slot` forgets them).
        let name = self.state.mixer.slot_names.get(&(t, slot)).cloned();
        let color = self.state.mixer.slot_colors.get(&(t, slot)).copied();
        let icon = self.state.playlist.icons.get(&crate::playlist::IconOwner::Slot(t, slot)).copied();
        for &k in &taken {
            // The hosted instances go with an undo kept (`FxReq::Delete`), the slots' names, colours and icons too.
            crate::mixer::clear_slot(&mut self.state, t, k);
        }
        if let Some(m) = crate::mixer::track_mut(&mut self.state, t) {
            m.slots[usize::from(slot)] = Some(SlotPlugin { id: Id::NONE, name: CONTAINER.into(), state: encode(&list) });
        }
        if let Some(v) = name {
            self.state.mixer.slot_names.insert((t, slot), v);
        }
        if let Some(v) = color {
            self.state.mixer.slot_colors.insert((t, slot), v);
        }
        if let Some(v) = icon {
            self.state.playlist.icons.insert(crate::playlist::IconOwner::Slot(t, slot), v);
        }
        self.state.piano.dirty = true;
        let mut msg = format!("{} effect{} in a container", n, if n == 1 { "" } else { "s" });
        if !left.is_empty() {
            let list: Vec<String> = left.iter().map(|k| (k + 1).to_string()).collect();
            msg += &format!("; not hosted, left in place: slot {}", list.join(", "));
        }
        Some(msg)
    }

    /// The container list's entries on member `i` of the container in slot `slot` of the current track: Open editor, or
    /// (inside the undo step, `mixer::run`) Move up, Move down, Remove. The chain follows on the next tick.
    pub(crate) fn edit_member(&mut self, slot: u8, i: usize, op: Op) -> Option<String> {
        let t = self.state.selected_mixer;
        let mut list = members(&self.state, t, slot)?;
        if i >= list.len() {
            return None;
        }
        match op {
            Op::Open => return self.open_member(t, slot, list[i].id),
            Op::Switch => list[i].on = !list[i].on,
            Op::Up if i > 0 => list.swap(i, i - 1),
            Op::Down if i + 1 < list.len() => list.swap(i, i + 1),
            Op::Remove => {
                list.remove(i);
            }
            _ => return None,
        }
        set_members(&mut self.state, t, slot, &list);
        self.state.piano.dirty = true;
        None
    }

    /// A headless graph edit on the container in slot `slot` of the current track (a channel's for [`CHANNEL`]), one
    /// undo step (`mixer::run`). Nodes are the container's input, its members and its named outputs; note processors
    /// transform the channel's notes in list order. The chain takes the new routing and note list on its next tick.
    pub(crate) fn edit_graph(&mut self, slot: u8, cmd: GraphCmd) -> Option<String> {
        let t = self.state.selected_mixer;
        let Some(list) = members(&self.state, t, slot) else { return Some("This slot has no container".into()) };
        let Some(mut graph) = graph(&self.state, t, slot) else { return Some("This slot has no container".into()) };
        match cmd {
            GraphCmd::Connect(from, to) => {
                if !endpoint_ok(&list, &graph, from, true) || !endpoint_ok(&list, &graph, to, false) {
                    return Some("The graph has no such node".into());
                }
                let edge = Edge { from, to };
                if graph.edges.contains(&edge) {
                    return Some("Those nodes are connected already".into());
                }
                if loops(&graph, edge) {
                    return Some("A connection would make a loop".into());
                }
                graph.edges.push(edge);
            }
            GraphCmd::Disconnect(from, to) => {
                let before = graph.edges.len();
                graph.edges.retain(|e| (e.from, e.to) != (from, to));
                if graph.edges.len() == before {
                    return Some("Those nodes are not connected".into());
                }
            }
            GraphCmd::AddOutput => {
                if graph.outputs.len() > usize::from(u8::MAX) {
                    return Some("A container holds at most 256 outputs".into());
                }
                graph.outputs.push(format!("Output {}", graph.outputs.len() + 1));
                graph.destinations.push(None);
            }
            GraphCmd::AddOutputTo(t) => {
                if graph.outputs.len() > usize::from(u8::MAX) {
                    return Some("A container holds at most 256 outputs".into());
                }
                if !self.state.project.mixer.iter().any(|m| m.index == t) {
                    return Some(format!("The mixer has no track {t}"));
                }
                graph.outputs.push(format!("Output {}", graph.outputs.len() + 1));
                graph.destinations.push(Some(t));
            }
            GraphCmd::RouteOutput(i, dest) => {
                if i == 0 {
                    return Some("Output 1 is the container's own slot or channel".into());
                }
                if usize::from(i) >= graph.outputs.len() {
                    return Some("The graph has no such output".into());
                }
                if dest.is_some_and(|t| !self.state.project.mixer.iter().any(|m| m.index == t)) {
                    return Some("The mixer has no such track".into());
                }
                graph.destinations.resize(graph.outputs.len(), None);
                graph.destinations[usize::from(i)] = dest;
            }
            GraphCmd::RemoveOutput(i) => {
                if graph.outputs.len() <= 1 {
                    return Some("A container keeps at least one output".into());
                }
                if usize::from(i) >= graph.outputs.len() {
                    return Some("The graph has no such output".into());
                }
                graph.outputs.remove(usize::from(i));
                if usize::from(i) < graph.destinations.len() {
                    graph.destinations.remove(usize::from(i));
                }
                graph.edges.retain(|e| e.from != Endpoint::Output(i) && e.to != Endpoint::Output(i));
                for e in &mut graph.edges {
                    for p in [&mut e.from, &mut e.to] {
                        if let Endpoint::Output(j) = p {
                            if *j > i {
                                *j -= 1;
                            }
                        }
                    }
                }
            }
            GraphCmd::AddNote { uid, name } => graph.notes.push(NoteProcessor { uid, name, state: Vec::new() }),
            GraphCmd::AddNoteKind(kind) => {
                if slot != CHANNEL {
                    return Some("Note processors act on a channel's container; this slot has no notes".into());
                }
                graph.notes.push(NoteProcessor::builtin(kind));
            }
            GraphCmd::SetNote(i, kind) => {
                let Some(n) = graph.notes.get_mut(i) else {
                    return Some("The graph has no such note processor".into());
                };
                *n = NoteProcessor::builtin(kind);
            }
            GraphCmd::MoveNote(i, to) => {
                if i >= graph.notes.len() || to >= graph.notes.len() {
                    return Some("The graph has no such note processor".into());
                }
                let n = graph.notes.remove(i);
                graph.notes.insert(to, n);
            }
            GraphCmd::RemoveNote(i) => {
                if i >= graph.notes.len() {
                    return Some("The graph has no such note processor".into());
                }
                graph.notes.remove(i);
            }
        }
        set_graph(&mut self.state, t, slot, &list, &graph);
        self.state.piano.dirty = true;
        None
    }

    /// Add effect ▸ in the container's list: effect `index` (of `Plugins::effects`; an instrument of
    /// `Plugins::available` for `instrument`, a channel's container only) at the end of the container in slot `slot` of
    /// the current track (the row in the track's place for a channel's), from the plugin's defaults: an effect wet
    /// only, an instrument at [`INSTRUMENT_LEVELS`]. Inside the undo step (`mixer::run`).
    pub(crate) fn add_member(&mut self, slot: u8, index: usize, instrument: bool) -> Option<String> {
        let t = self.state.selected_mixer;
        let mut list = members(&self.state, t, slot)?;
        if instrument && slot != CHANNEL {
            return None;
        }
        let a = if instrument { self.plugins.available.get(index)? } else { self.plugins.effects.get(index)? };
        // The smallest free id: a removed member's goes to the next one added (its clips and links would follow it).
        let Some(id) = free_id(&list) else { return Some(format!("A container holds at most {MEMBER_IDS} effects")) };
        let (wet, dry) = if instrument { INSTRUMENT_LEVELS } else { (1.0, 0.0) };
        list.push(Member { id, instrument, uid: a.desc.uid(), name: a.desc.name.clone(), wet, dry, on: true, state: Vec::new() });
        set_members(&mut self.state, t, slot, &list);
        self.state.piano.dirty = true;
        None
    }

    /// A member window's Replace ▸: effect `index` (of `Plugins::effects`; an instrument of `Plugins::available` for
    /// `instrument`) in place of member `i` of the container in slot `slot` of the current track, from the plugin's
    /// defaults; the member keeps its place, id and levels, so its level's clips and links stay on it (as a slot's do
    /// when its effect is replaced). Inside the undo step (`mixer::run`).
    pub(crate) fn replace_member(&mut self, slot: u8, i: usize, index: usize, instrument: bool) -> Option<String> {
        let t = self.state.selected_mixer;
        let mut list = members(&self.state, t, slot)?;
        let a = if instrument { self.plugins.available.get(index)? } else { self.plugins.effects.get(index)? };
        let m = list.get_mut(i)?;
        (m.instrument, m.uid, m.name, m.state) = (instrument, a.desc.uid(), a.desc.name.clone(), Vec::new());
        set_members(&mut self.state, t, slot, &list);
        self.state.piano.dirty = true;
        None
    }

    /// The channel menu's Wrap in container (the original's Patcherize on a channel; spec: plugins/patcher/
    /// README.md §3.3, §4.4): rack row `row`'s instrument becomes the first member of a container on the channel, at
    /// [`INSTRUMENT_LEVELS`], with its state; the channel keeps its name, colour, notes, steps, mixer track and settings,
    /// which act on the container's output. One undo step: the channel takes a new id, so the instrument kept under the
    /// old one (`Plugins::keep`) comes back with an undo, as Replace's does.
    pub(crate) fn wrap_channel(&mut self, row: usize) -> Option<String> {
        crate::control::edit_app(self, WRAP, crate::control::command::Region::Project, |app| app.wrap_channel_now(row))
    }

    fn wrap_channel_now(&mut self, row: usize) -> Option<String> {
        if !can_wrap_channel(&self.state, row) || !self.plugins.hosted.contains_key(&row) {
            return Some("This channel's plugin is not hosted: nothing to wrap".into());
        }
        let member = {
            let h = self.plugins.hosted.get(&row)?;
            let p = h.plugin.lock().ok()?;
            let (wet, dry) = INSTRUMENT_LEVELS;
            Member { id: 1, instrument: true, uid: p.desc().uid(), name: p.desc().name.clone(), wet, dry, on: true, state: p.save_state().unwrap_or_default() }
        };
        let name = member.name.clone();
        self.plugins.keep(row);
        self.plugins.drop_key(row);
        self.state.hosted_rows.retain(|&r| r != row);
        self.transport.set_instruments(self.plugins.instruments());
        let s = &mut self.state;
        let ch = &mut s.project.channels[row];
        // Channel parameters name the channel's index, which its automation and links keep.
        let index = ch.index;
        // A new id at once, before the container plays the channel.
        ch.id = Id::NONE;
        s.control.settle(&mut s.project);
        let ch = &mut s.project.channels[row];
        ch.plugin = Some(CONTAINER.into());
        ch.plugin_state = Some(encode(&[member]));
        // The clips and links that drove the old instrument follow it into the container.
        move_targets(s, &|tg| wrapped_channel_target(tg, index));
        s.piano.dirty = true;
        let t = u16::try_from(row).ok()?;
        show(s, t, CHANNEL);
        Some(format!("{name} in a container"))
    }

    /// The container window's controls (`CAct`).
    pub(crate) fn container_act(&mut self, a: CAct) {
        if a == CAct::Graph {
            show_graph(&mut self.state);
            self.invalidate();
            return;
        }
        let (t, k) = self.state.container.at;
        let Some(list) = members(&self.state, t, k) else { return };
        let (x, y) = self.mouse;
        let s = &self.state;
        let mut items = match a {
            CAct::Open(i) => {
                let Some(id) = list.get(i).map(|m| m.id) else { return };
                if let Some(msg) = self.open_member(t, k, id) {
                    self.state.message = Some(msg);
                }
                return;
            }
            CAct::Menu(i) if i < list.len() => member_ops(s, t, k, &list, i),
            CAct::Menu(_) => return,
            CAct::Add if list.len() >= MAX_MEMBERS => {
                self.state.message = Some(format!("A container holds at most {MEMBER_IDS} effects"));
                return;
            }
            CAct::Add => crate::plugin_menu::items(s, crate::plugin_menu::Target::Container(k), true),
            CAct::Graph => return,
        };
        on_track(t, &mut items);
        self.state.playlist.popup = Some(Popup { x, y, items, sub: None, scroll: [0; 2] });
    }

    /// The graph window's buttons (`GAct`). A menu's commands are wrapped like the container window's (`on_track`), so a
    /// channel's container edits the selected row and a slot's the selected track; each command is one undo step.
    pub(crate) fn graph_act(&mut self, a: GAct) {
        let (t, k) = self.state.container.at;
        let Some(list) = members(&self.state, t, k) else { return };
        let Some(g) = graph(&self.state, t, k) else { return };
        let (x, y) = self.mouse;
        match a {
            GAct::AddOutput => return self.run_graph_cmd(GraphCmd::AddOutput),
            GAct::RemoveOutput(i) => return self.run_graph_cmd(GraphCmd::RemoveOutput(i)),
            GAct::Disconnect(from, to) => return self.run_graph_cmd(GraphCmd::Disconnect(from, to)),
            GAct::RemoveNote(i) => return self.run_graph_cmd(GraphCmd::RemoveNote(i)),
            _ => {}
        }
        let mut items: Vec<PItem> = Vec::new();
        let s = &self.state;
        match a {
            GAct::AddOutputMenu => {
                for m in &s.project.mixer {
                    items.push(PItem::new(
                        format!("Add output to {}", crate::mixer::name(s, m.index)),
                        Cmd::Mixer(crate::mixer::MCmd::Graph(k, GraphCmd::AddOutputTo(m.index))),
                    ));
                }
                if items.is_empty() {
                    self.state.message = Some("The project has no mixer tracks".into());
                    return;
                }
            }
            GAct::ConnectMenu => {
                use std::iter::once;
                let sources = once(Endpoint::Input).chain(list.iter().map(|m| Endpoint::Member(m.id)));
                for from in sources {
                    let mut targets = Vec::new();
                    for to in list.iter().map(|m| Endpoint::Member(m.id)).chain((0..g.outputs.len()).filter_map(|i| u8::try_from(i).ok()).map(Endpoint::Output)) {
                        if to != from && !g.edges.contains(&Edge { from, to }) {
                            targets.push(PItem::new(format!("To {}", node_name(&list, to)), Cmd::Mixer(crate::mixer::MCmd::Graph(k, GraphCmd::Connect(from, to)))));
                        }
                    }
                    if !targets.is_empty() {
                        items.push(PItem::sub(format!("From {}", node_name(&list, from)), targets));
                    }
                }
                if items.is_empty() {
                    self.state.message = Some("Every connection in this graph is made already".into());
                    return;
                }
            }
            GAct::OutputMenu => {
                for (i, name) in g.outputs.iter().enumerate() {
                    let i = i as u8;
                    let mut children = Vec::new();
                    if i == 0 {
                        children.push(PItem::new("Output 1 is this slot or channel", Cmd::Nothing).off());
                    } else {
                        for m in &s.project.mixer {
                            let mut item = PItem::new(
                                format!("Route to {}", crate::mixer::name(s, m.index)),
                                Cmd::Mixer(crate::mixer::MCmd::Graph(k, GraphCmd::RouteOutput(i, Some(m.index)))),
                            );
                            if g.destination(usize::from(i)) == Some(m.index) {
                                item = item.mark(Mark::Check(true));
                            }
                            children.push(item);
                        }
                        children.push(PItem::new(
                            "No destination",
                            Cmd::Mixer(crate::mixer::MCmd::Graph(k, GraphCmd::RouteOutput(i, None))),
                        ).mark(if g.destination(usize::from(i)).is_none() { Mark::Check(true) } else { Mark::None }));
                        children.push(PItem::sep());
                        children.push(PItem::new(
                            "Remove this output",
                            Cmd::Mixer(crate::mixer::MCmd::Graph(k, GraphCmd::RemoveOutput(i))),
                        ));
                    }
                    items.push(PItem::sub(format!("{name} (output {})", i as usize + 1), children));
                }
            }
            GAct::DisconnectMenu => {
                for e in &g.edges {
                    items.push(PItem::new(
                        format!("{} -> {}", node_name(&list, e.from), node_name(&list, e.to)),
                        Cmd::Mixer(crate::mixer::MCmd::Graph(k, GraphCmd::Disconnect(e.from, e.to))),
                    ));
                }
                if items.is_empty() {
                    self.state.message = Some("This graph has no connections".into());
                    return;
                }
            }
            GAct::NoteMenu => {
                if k == CHANNEL {
                    items.push(PItem::sub("Add a note processor", note_add_items(k, s.project.ppq)));
                } else {
                    items.push(PItem::new("Note processors act on a channel's container", Cmd::Nothing).off());
                }
                if !g.notes.is_empty() {
                    items.push(PItem::sep());
                }
                for (i, n) in g.notes.iter().enumerate() {
                    items.push(PItem::sub(format!("{}. {}", i + 1, n.name), note_children(k, s.project.ppq, i, n, g.notes.len())));
                }
            }
            GAct::AddOutput | GAct::RemoveOutput(_) | GAct::Disconnect(..) | GAct::RemoveNote(_) => return,
        }
        on_track(t, &mut items);
        self.state.playlist.popup = Some(Popup { x, y, items, sub: None, scroll: [0; 2] });
        self.invalidate();
    }

    /// One graph command on the container of the current graph window, as a menu command runs it: an undo step and a
    /// message when it is refused.
    fn run_graph_cmd(&mut self, cmd: GraphCmd) {
        let (t, k) = self.state.container.at;
        let c = Cmd::SlotWin(t, Box::new(Cmd::Mixer(crate::mixer::MCmd::Graph(k, cmd))));
        if let Some(m) = crate::playlist::run(self, c) {
            self.state.message = Some(m);
        }
        self.invalidate();
    }

    /// A right click in the container's window: a member's name opens its menu, a level knob its parameter menu.
    /// Returns whether `a` was one.
    pub(crate) fn container_right(&mut self, a: &crate::app::Action) -> bool {
        use crate::app::Action;
        let (x, y) = self.mouse;
        match *a {
            Action::Container(CAct::Open(i) | CAct::Menu(i)) => self.container_act(CAct::Menu(i)),
            Action::Knob(id @ KnobId::Member(..)) => self.state.playlist.popup = knob_menu(&self.state, id, x, y),
            _ => return false,
        }
        true
    }

    /// Every tick while the container's window is shown: the levels its chain plays (an automated level moves its knob),
    /// repainted when they change. A container gone (an undo, a deleted slot) closes the window.
    fn mirror_levels(&mut self) {
        if !self.state.shown(Win::Container) {
            return;
        }
        let (t, k) = self.state.container.at;
        if members(&self.state, t, k).is_none() {
            self.state.hide(Win::Container);
            self.state.hide(Win::Graph);
            self.invalidate();
            return;
        }
        let Some(live) = self.plugins.containers.get(&place_key(t, k)) else { return };
        let Ok(c) = live.fx.try_lock() else { return };
        let now: BTreeMap<u32, (f32, f32)> = c.members.iter().map(|m| (m.id, (m.wet, m.dry))).collect();
        drop(c);
        if now != self.state.container.live {
            self.state.container.live = now;
            self.damage_win(Win::Container);
        }
    }

    /// Opens the window of member `id` of the container at place (`t`, `slot`).
    pub(crate) fn open_member(&mut self, t: u16, slot: u8, id: u32) -> Option<String> {
        let live = self.plugins.containers.get(&place_key(t, slot))?;
        let Some(key) = live.shape.iter().position(|s| s.0 == id).and_then(|at| live.keys[at]) else {
            return Some("This plugin is not loaded (it was not found)".into());
        };
        self.open_member_key(key)
    }

    /// Opens the window of the member hosted under `key`.
    pub(crate) fn open_member_key(&mut self, key: usize) -> Option<String> {
        #[cfg(not(plugin_windows))]
        {
            let _ = key;
            Some(crate::app::off_text(crate::app::PLUGIN_WINDOWS))
        }
        #[cfg(plugin_windows)]
        {
            let info = self.member_infos().remove(&key)?;
            match self.plugins.open_window(key, info) {
                Ok(()) => {
                    self.state.raise(crate::app::Win::Hosted(key));
                    None
                }
                Err(e) => Some(format!("Plugin: {e}")),
            }
        }
    }

    /// The plugin frame's title of every member hosted, by key: the member's name over its container's slot's title.
    #[cfg(plugin_windows)]
    pub(crate) fn member_infos(&self) -> std::collections::HashMap<usize, crate::plugin_window::ChannelInfo> {
        let mut v = std::collections::HashMap::new();
        for (&slot_key, live) in &self.plugins.containers {
            let info = match fx_of(slot_key) {
                Some(_) => crate::mixer::fx_info(&self.state.project, &self.state.mixer, slot_key),
                None => crate::app::channel_info(&self.state.project, slot_key),
            };
            let Some(info) = info else { continue };
            for key in live.keys.iter().flatten() {
                let Some(h) = self.plugins.hosted.get(key) else { continue };
                v.insert(*key, crate::plugin_window::ChannelInfo { name: format!("{} ({})", h.name, info.name), ..info.clone() });
            }
        }
        v
    }

    /// Where member `key` plays, for the performance monitor: "Bus 2, slot 1, container", "Lemon Trio, container".
    pub(crate) fn member_place(&self, key: usize) -> Option<String> {
        let slot_key = self.plugins.containers.iter().find(|(_, l)| l.keys.contains(&Some(key)))?.0;
        Some(match key_place(*slot_key)? {
            (r, CHANNEL) => format!("{}, container", place_name(&self.state, r, CHANNEL)),
            (0, k) => format!("Master, slot {}, container", k + 1),
            (t, k) => format!("Bus {t}, slot {}, container", k + 1),
        })
    }

    /// Every tick: the containers played follow the project's container slots and channels (a wrap, an edit of the
    /// list or graph, an undo, an opened project, a slot or channel moved). A container whose list changed is built
    /// again from it: a member still there (by its id and plugin) keeps its instance and window; a new one is an
    /// instance of its plugin started from its saved state, or left out of the chain when its plugin is not found (the
    /// list keeps it). A list whose levels alone changed sets them on the chain. The saved graph is the chain's routing;
    /// a graph the renderer cannot honour (an unsupported note processor, a missing member, a loop) is reported and the
    /// members play in series instead of being silently dropped.
    pub(crate) fn sync_containers(&mut self) {
        let mut want: Vec<(usize, Id)> = Vec::new();
        for m in &self.state.project.mixer {
            for (k, p) in m.slots.iter().enumerate() {
                if let Some(p) = p.as_ref().filter(|p| p.name == CONTAINER) {
                    want.push((fx_key(m.index, k as u8), p.id));
                }
            }
        }
        for (r, ch) in self.state.project.channels.iter().enumerate() {
            if is_container_channel(ch) && u16::try_from(r).is_ok() {
                want.push((r, ch.id));
            }
        }
        if want.is_empty() && self.plugins.containers.is_empty() {
            self.state.params.members.clear();
            self.mirror_levels();
            return;
        }
        let mut changed = false;
        let mut channels = false;
        let mut graph_error: Option<String> = None;
        // Gone, or moved to another slot or row (found by the slot's or channel's id).
        let keys: Vec<usize> = self.plugins.containers.keys().copied().collect();
        for key in keys {
            if want.iter().any(|w| w.0 == key) {
                continue;
            }
            let live = self.plugins.containers.remove(&key).unwrap();
            channels |= live.channel;
            let to = want.iter().find(|w| w.1 == live.id && !live.id.is_none() && !self.plugins.containers.contains_key(&w.0) && (w.0 < FX_KEY) == live.channel).map(|w| w.0);
            match to {
                Some(to) => {
                    self.plugins.containers.insert(to, live);
                }
                None => {
                    for k in live.keys.into_iter().flatten() {
                        self.plugins.drop_key(k);
                    }
                }
            }
            changed = true;
        }
        let mut missing = Vec::new();
        for (key, id) in want {
            let Some((t, k)) = key_place(key) else { continue };
            let Some(p) = state_in(&self.state.project, t, k) else { continue };
            if let Some(live) = self.plugins.containers.get_mut(&key) {
                live.id = id;
                if live.src == p {
                    continue;
                }
            }
            let list = decode(p).unwrap_or_default();
            let graph = decode_state(p).map(|(_, g)| g).unwrap_or_else(|| serial_graph(&list));
            let src = p.to_vec();
            if let Some(live) = self.plugins.containers.get_mut(&key).filter(|l| l.shape == shape(&list)) {
                let ids: Vec<u32> = live.shape.iter().zip(&live.keys).filter_map(|((id, _), k)| k.is_some().then_some(*id)).collect();
                let graph = match graph.plan(&ids) {
                    Ok(_) => graph,
                    Err(e) => {
                        graph_error = Some(e.to_string());
                        Graph::serial(&ids)
                    }
                };
                if let Ok(mut c) = live.fx.lock() {
                    let playing = list.iter().zip(&live.keys).filter(|(_, k)| k.is_some());
                    for (at, (m, _)) in playing.enumerate() {
                        c.set_levels(at, m.wet, m.dry);
                        c.set_on(at, m.on);
                    }
                    if let Err(e) = c.set_graph(graph) {
                        graph_error = Some(e.to_string());
                    }
                }
                live.src = src;
                continue;
            }
            // Built again: the instances of the members still there are kept.
            let mut pool: Vec<((u32, String), usize)> = match self.plugins.containers.remove(&key) {
                Some(old) => old.shape.into_iter().zip(old.keys).filter_map(|(s, k)| Some((s, k?))).collect(),
                None => Vec::new(),
            };
            let mut keys = Vec::new();
            let channel = k == CHANNEL;
            for m in &list {
                if let Some(at) = pool.iter().position(|(s, _)| (s.0, s.1.as_str()) == (m.id, m.uid.as_str())) {
                    keys.push(Some(pool.remove(at).1));
                    continue;
                }
                let state = (!m.state.is_empty()).then_some(m.state.as_slice());
                // An instrument plays in a channel's container only.
                let made = if !m.instrument {
                    self.plugins.effect_index_of_uid(&m.uid).and_then(|i| self.plugins.instantiate_effect(i, state).ok())
                } else if channel {
                    self.plugins.index_of_uid(&m.uid).and_then(|i| self.plugins.instantiate(i, state).ok())
                } else {
                    None
                };
                match made {
                    Some((plugin, _)) => {
                        let k = MEMBER_KEY + self.plugins.next_member;
                        self.plugins.next_member += 1;
                        self.plugins.hosted.insert(k, Hosted::new(plugin));
                        keys.push(Some(k));
                    }
                    None => {
                        missing.push(m.name.clone());
                        keys.push(None);
                    }
                }
            }
            for (_, k) in pool {
                self.plugins.drop_key(k);
            }
            let hosted = &self.plugins.hosted;
            let fx: Vec<chain::Member> = list
                .iter()
                .zip(&keys)
                .filter_map(|(m, k)| {
                    let plugin = hosted.get(&(*k)?)?.plugin.clone();
                    let mut member = if m.instrument {
                        chain::Member::instrument(m.id, plugin as os_engine::SharedInstrument, m.wet, m.dry)
                    } else {
                        chain::Member::new(m.id, plugin as os_engine::SharedEffect, m.wet, m.dry)
                    };
                    member.on = m.on;
                    Some(member)
                })
                .collect();
            let ids: Vec<u32> = fx.iter().map(|m| m.id).collect();
            let graph = match graph.plan(&ids) {
                Ok(_) => graph,
                Err(e) => {
                    graph_error = Some(e.to_string());
                    Graph::serial(&ids)
                }
            };
            let chain = Chain::with_graph(fx, graph).expect("the selected graph was validated");
            let live = Live { id, src, shape: shape(&list), channel, keys, fx: Arc::new(Mutex::new(chain)) };
            self.plugins.containers.insert(key, live);
            channels |= channel;
            changed = true;
        }
        if !missing.is_empty() {
            self.state.message = Some(format!("Container: not found, left out of the chain: {}", missing.join(", ")));
        }
        if let Some(e) = graph_error {
            let graph = format!("Container graph: {e}; playing the members in series");
            self.state.message = Some(match self.state.message.take() {
                Some(m) => format!("{m}; {graph}"),
                None => graph,
            });
        }
        if channels {
            self.transport.set_instruments(self.plugins.instruments());
        }
        if changed {
            // Which member each hosted key is, for its parameters' targets and links (`params::target_of`).
            let members = &mut self.state.params.members;
            members.clear();
            for (&slot_key, live) in &self.plugins.containers {
                let Some((t, k)) = key_place(slot_key) else { continue };
                for ((id, _), key) in live.shape.iter().zip(&live.keys) {
                    if let Some(key) = key {
                        members.insert(*key, (t, k, *id));
                    }
                }
            }
            self.transport.set_effects(self.plugins.effects());
            self.state.piano.dirty = true;
        }
        self.mirror_levels();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mixer::{run, MCmd};
    use crate::app::Win;

    /// Authentic pre-graph state bytes: v1 has no ids, v2 no kinds, v3 no switch and v4 no graph. Kept separate from
    /// the current encoder so a round-trip test cannot hide a decoder regression.
    fn legacy_state(version: u32, members: &[Member]) -> Vec<u8> {
        let mut b = MAGIC.to_vec();
        b.extend(version.to_le_bytes());
        b.extend((members.len() as u32).to_le_bytes());
        let bytes = |b: &mut Vec<u8>, v: &[u8]| {
            b.extend((v.len() as u32).to_le_bytes());
            b.extend_from_slice(v);
        };
        for m in members {
            if version >= 2 {
                b.extend(m.id.to_le_bytes());
            }
            if version >= 3 {
                b.extend((if m.instrument { INSTRUMENT } else { 0 }).to_le_bytes());
            }
            if version >= 4 {
                b.extend((if m.on { 1u32 } else { 0 }).to_le_bytes());
            }
            bytes(&mut b, m.uid.as_bytes());
            bytes(&mut b, m.name.as_bytes());
            b.extend(m.wet.to_le_bytes());
            b.extend(m.dry.to_le_bytes());
            bytes(&mut b, &m.state);
        }
        b
    }

    /// `encode_state`'s version 6 bytes with the destination map removed and the version set to 5.
    fn without_destinations(mut b: Vec<u8>, members: &[Member], outputs: usize) -> Vec<u8> {
        b[4..8].copy_from_slice(&5u32.to_le_bytes());
        let mut at = 12;
        for m in members {
            at += 32 + m.uid.len() + m.name.len() + m.state.len();
        }
        at += 4;
        for _ in 0..outputs {
            let n = u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize;
            at += 4 + n;
        }
        b.drain(at..at + 4 * outputs);
        b
    }

    /// The state round-trips; version 1 (no ids) numbers the members; other bytes are not a container.
    #[test]
    fn the_list_is_kept() {
        let m = vec![
            Member { id: 3, instrument: true, uid: "clap:a".into(), name: "A".into(), wet: 1.0, dry: 0.0, on: true, state: vec![1, 2, 3] },
            Member { id: 1, instrument: false, uid: "vst3:b".into(), name: "B".into(), wet: 0.25, dry: 0.75, on: true, state: vec![] },
        ];
        assert_eq!(decode(&encode(&m)), Some(m.clone()));
        assert_eq!(decode_state(&encode(&m)), Some((m.clone(), serial_graph(&m))));
        assert_eq!(decode(&encode(&[])), Some(vec![]));
        let custom = Graph {
            outputs: vec!["Main".into(), "Aux".into()],
            destinations: vec![None, Some(1)],
            notes: vec![NoteProcessor { uid: "clap:note".into(), name: "Note".into(), state: vec![7, 8] }],
            edges: vec![
                Edge { from: Endpoint::Input, to: Endpoint::Member(3) },
                Edge { from: Endpoint::Member(3), to: Endpoint::Output(1) },
            ],
        };
        assert_eq!(decode_state(&encode_state(&m, &custom)), Some((m.clone(), custom.clone())));
        // Version 5 had named outputs but no destinations: its extra outputs read back unrouted.
        let v5 = without_destinations(encode_state(&m, &custom), &m, custom.outputs.len());
        let unrouted = Graph { destinations: vec![None; custom.outputs.len()], ..custom.clone() };
        assert_eq!(decode_state(&v5), Some((m.clone(), unrouted)));
        // Every pre-graph format (1: no ids, 2: no kinds, 3: no switch, 4: no graph) decodes to the serial graph.
        let old = vec![
            Member { id: 1, instrument: false, uid: "clap:a".into(), name: "A".into(), wet: 0.5, dry: 0.5, on: true, state: vec![] },
            Member { id: 2, instrument: false, uid: "vst3:b".into(), name: "B".into(), wet: 0.75, dry: 0.25, on: true, state: vec![4] },
        ];
        for version in 1..=4 {
            let bytes = legacy_state(version, &old);
            assert_eq!(decode(&bytes), Some(old.clone()), "version {version} members");
            assert_eq!(decode_state(&bytes), Some((old.clone(), serial_graph(&old))), "version {version} gets the serial graph");
        }
        assert_eq!(decode(b"LCNT"), None);
        assert_eq!(decode(&[0; 16]), None);
        assert_eq!(levels(1.0, true), (1.0, 0.0));
        assert_eq!(levels(0.3, false), (0.0, 1.0));
        let (w, d) = levels(0.5, true);
        assert!((w - 0.5).abs() < 1e-6 && (d - 0.5).abs() < 1e-6);
    }

    /// A tick as the program runs it for the mixer: ids for new slots, the mixer's requests, instances following ids.
    fn tick(a: &mut App) {
        a.state.control.settle(&mut a.state.project);
        a.mixer_tick();
        a.follow_ids(false, true);
    }

    /// An app with Lemon Studio's effects, the limiter in slot 1 and the delay in slot 2 of track 2; the two names.
    fn two_effects() -> (App, String, String) {
        let mut a = App::new(None);
        a.state.project = crate::app::test_project();
        a.reset_hosted();
        a.plugins.effects = crate::plugins::builtin_effects();
        a.state.plugdb.rebuild(&a.plugins);
        let find = |a: &App, n: &str| a.plugins.effects.iter().position(|e| e.desc.name.contains(n)).unwrap();
        let (lim, del) = (find(&a, "Limiter"), find(&a, "Delay"));
        let names = (a.plugins.effects[lim].desc.name.clone(), a.plugins.effects[del].desc.name.clone());
        crate::mixer::select(&mut a.state, 2, false);
        run(&mut a, MCmd::AddEffect(0, lim));
        run(&mut a, MCmd::AddEffect(1, del));
        tick(&mut a);
        (a, names.0, names.1)
    }

    fn names(a: &App, slot: u8) -> Vec<String> {
        members(&a.state, 2, slot).unwrap_or_default().into_iter().map(|m| m.name).collect()
    }

    /// Wrap all in container: the track's two hosted effects become one container in the slot the command was used on,
    /// with their levels; the other slot is emptied; the chain plays the slot; one undo puts both slots and their
    /// instances back. Wrap in container on one slot does the same for that slot only.
    #[test]
    fn wrap_the_tracks_effects_in_a_container() {
        let (mut a, first, second) = two_effects();
        if let Some(m) = crate::mixer::track_mut(&mut a.state, 2) {
            m.slot_mix[1] = 0.5;
        }
        // The slot's name and colour go with the container; the emptied slot's go.
        a.state.mixer.slot_names.insert((2, 1), "Echo".into());
        a.state.mixer.slot_colors.insert((2, 0), os_model::Rgb { r: 1, g: 2, b: 3 });
        let menu = crate::mixer::slot_menu(&a, 1, 0.0, 0.0);
        let item = |label: &str| menu.items.iter().find(|i| i.label == label).unwrap().clone();
        assert!(item(WRAP).enabled && item(WRAP_ALL).enabled);
        assert_eq!(item(WRAP_ALL).cmd, Cmd::Mixer(MCmd::Wrap(1, true)));
        crate::playlist::run(&mut a, item(WRAP_ALL).cmd);
        tick(&mut a);
        let t = crate::mixer::track(&a.state, 2).unwrap();
        assert!(t.slots[0].is_none(), "the other slot is emptied");
        assert_eq!(t.slots[1].as_ref().map(|p| p.name.as_str()), Some(CONTAINER));
        assert_eq!((t.slot_mix[1], t.slot_on[1]), (1.0, true));
        assert_eq!(a.state.mixer.slot_names.get(&(2, 1)).map(String::as_str), Some("Echo"));
        assert!(a.state.mixer.slot_colors.is_empty());
        let list = members(&a.state, 2, 1).unwrap();
        assert_eq!(names(&a, 1), [first.clone(), second.clone()]);
        assert_eq!((list[0].id, list[1].id), (1, 2));
        assert_eq!((list[0].wet, list[0].dry), (1.0, 0.0));
        assert!((list[1].wet - 0.5).abs() < 1e-6 && (list[1].dry - 0.5).abs() < 1e-6);
        assert!(!a.plugins.hosted.contains_key(&fx_key(2, 0)) && !a.plugins.hosted.contains_key(&fx_key(2, 1)));
        let live = &a.plugins.containers[&fx_key(2, 1)];
        assert_eq!(live.fx.lock().unwrap().members.len(), 2);
        assert!(live.keys.iter().all(|k| k.is_some_and(|k| k >= MEMBER_KEY && a.plugins.hosted.contains_key(&k))), "members hosted under their own keys");
        assert_eq!(a.plugins.effects().iter().filter(|e| (e.0, e.1) == (2, 1)).count(), 1, "the chain plays the slot");
        assert_eq!(a.plugins.effects().len(), 1, "the members are not slots");
        // The slot menu lists the members.
        let menu = crate::mixer::slot_menu(&a, 1, 0.0, 0.0);
        let list = menu.items.iter().find(|i| i.label == "Container").expect("the list");
        assert_eq!(list.children[1].label, format!("2. {second} (wet 50%, dry 50%)"));

        // Undo: both slots and their instances come back; the members go.
        crate::history::undo(&mut a.state);
        tick(&mut a);
        let t = crate::mixer::track(&a.state, 2).unwrap();
        assert!(t.slots[0].is_some() && t.slots[1].as_ref().is_some_and(|p| p.name != CONTAINER));
        assert_eq!(t.slot_mix[1], 0.5);
        assert!(a.plugins.containers.is_empty());
        assert!(a.plugins.hosted.contains_key(&fx_key(2, 0)) && a.plugins.hosted.contains_key(&fx_key(2, 1)));
        assert!(a.plugins.hosted.range(MEMBER_KEY..).next().is_none());

        // Wrap in container: that slot only.
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Wrap(0, false)));
        tick(&mut a);
        let t = crate::mixer::track(&a.state, 2).unwrap();
        assert_eq!(t.slots[0].as_ref().map(|p| p.name.as_str()), Some(CONTAINER));
        assert!(t.slots[1].as_ref().is_some_and(|p| p.name != CONTAINER));
        assert_eq!(members(&a.state, 2, 0).map(|m| m.len()), Some(1));
        assert!(a.plugins.hosted.contains_key(&fx_key(2, 1)) && a.plugins.containers.contains_key(&fx_key(2, 0)));
        // Wrap all then takes the container's member and the other effect.
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Wrap(0, true)));
        tick(&mut a);
        assert_eq!(names(&a, 0), [first, second]);
        assert!(crate::mixer::track(&a.state, 2).unwrap().slots[1].is_none());
    }

    /// The container's list: moving a member keeps its instance; removing one drops it; Add effect ▸ adds the effect
    /// at the end from its defaults; each is one undo step, and an undo of a removal brings the member back with the
    /// state its instance had.
    #[test]
    fn edit_the_containers_list() {
        let (mut a, first, second) = two_effects();
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Wrap(0, true)));
        tick(&mut a);
        let keys = |a: &App| a.plugins.containers[&fx_key(2, 0)].keys.clone();
        let before = keys(&a);
        assert_eq!(label(1, &members(&a.state, 2, 0).unwrap()[1]), format!("2. {second} (wet 100%, dry 0%)"));
        let ops = &menu_of(&mut a, 1);
        assert_eq!(ops.iter().map(|i| i.label.as_ref()).collect::<Vec<_>>(), ["Open editor", "Wet: 100%", "Dry: 0%", "Switch off", "Move up", "Move down", "Remove"]);
        assert!(ops[4].enabled && !ops[5].enabled, "the last one moves up only");
        // Move up.
        crate::playlist::run(&mut a, ops[4].cmd.clone());
        tick(&mut a);
        assert_eq!(names(&a, 0), [second.clone(), first.clone()]);
        assert_eq!(keys(&a), vec![before[1], before[0]], "the instances are kept");
        assert_eq!(crate::history::undo(&mut a.state).as_deref(), Some(MOVE));
        assert_eq!(crate::history::redo(&mut a.state).as_deref(), Some(MOVE));
        tick(&mut a);
        assert_eq!(keys(&a), vec![before[1], before[0]], "and kept through an undo and a redo");
        // A member's state, as its window would leave it, then Remove.
        let k = before[1].unwrap();
        let state = a.plugins.hosted[&k].plugin.lock().unwrap().save_state().unwrap();
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Member(0, 0, Op::Remove)));
        tick(&mut a);
        assert_eq!(names(&a, 0), [first.clone()]);
        assert!(!a.plugins.hosted.contains_key(&k), "its instance goes");
        assert_eq!(a.plugins.containers[&fx_key(2, 0)].fx.lock().unwrap().members.len(), 1);
        crate::history::undo(&mut a.state);
        tick(&mut a);
        assert_eq!(names(&a, 0), [second.clone(), first.clone()]);
        assert_eq!(members(&a.state, 2, 0).unwrap()[0].state, state);
        // Add effect: the picker of an empty slot, adding at the end.
        a.container_act(CAct::Add);
        let add = a.state.playlist.popup.take().unwrap().items;
        let lim = add.iter().find(|i| i.label == first).expect("the limiter in the picker").cmd.clone();
        assert!(matches!(&lim, Cmd::SlotWin(2, c) if matches!(**c, Cmd::Mixer(MCmd::MemberAdd(0, _)))));
        crate::playlist::run(&mut a, lim);
        tick(&mut a);
        assert_eq!(names(&a, 0), [second, first.clone(), first]);
        assert_eq!(members(&a.state, 2, 0).unwrap()[2].id, 3);
        // The smallest free id: a removed member's comes back to the next one added.
        let with = |ids: &[u32]| -> Vec<Member> { ids.iter().map(|&id| Member { id, ..members(&a.state, 2, 0).unwrap()[0].clone() }).collect() };
        assert_eq!(free_id(&with(&[2, 3])), Some(1));
        assert_eq!(free_id(&with(&[1, 2, 4])), Some(3));
        assert_eq!(free_id(&with(&(1..=MEMBER_IDS).collect::<Vec<_>>())), None, "at most 15");
        assert_eq!(a.plugins.containers[&fx_key(2, 0)].fx.lock().unwrap().members.len(), 3);
        assert_eq!(crate::history::undo(&mut a.state).as_deref(), Some(ADD));
        assert_eq!(crate::history::redo(&mut a.state).as_deref(), Some(ADD));
        tick(&mut a);
        // A save stores the members' states.
        let mut proj = a.state.project.clone();
        crate::project_plugins::store_hosted(&mut proj, &a.plugins);
        let saved = proj.mixer.iter().find(|m| m.index == 2).unwrap().slots[0].as_ref().map(|p| decode(&p.state).unwrap()).unwrap();
        assert!(saved.iter().all(|m| !m.state.is_empty()));
        // Every member window has a title.
        #[cfg(plugin_windows)]
        assert_eq!(a.member_infos().len(), 3);
    }

    /// The container window's menu of member `i`, the window showing the container in slot 1 of track 2.
    fn menu_of(a: &mut App, i: usize) -> Vec<PItem> {
        show(&mut a.state, 2, 0);
        a.container_act(CAct::Menu(i));
        a.state.playlist.popup.take().expect("the member's menu").items
    }

    /// The command of entry `label` of a menu's items.
    fn cmd_of(items: &[PItem], label: &str) -> Cmd {
        items.iter().find(|i| i.label == label).unwrap_or_else(|| panic!("no {label:?}")).cmd.clone()
    }

    /// A member's wet and dry: the list's Wet: …% ▸ and Dry: …% ▸ Enter value... open the value box in percent; each
    /// typed value is one undo step; the chain takes the levels and keeps its instances.
    #[test]
    fn a_members_wet_and_dry() {
        use crate::typein::{accept, text, Target};
        let (mut a, _, second) = two_effects();
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Wrap(0, true)));
        tick(&mut a);
        let keys = a.plugins.containers[&fx_key(2, 0)].keys.clone();
        let ops = &menu_of(&mut a, 1);
        let (wet, dry) = (Target::Member(2, 0, 2, true), Target::Member(2, 0, 2, false));
        assert_eq!((ops[1].label.as_ref(), cmd_of(&ops[1].children, "Enter value...")), ("Wet: 100%", Cmd::TypeIn(wet)));
        assert_eq!((ops[2].label.as_ref(), cmd_of(&ops[2].children, "Enter value...")), ("Dry: 0%", Cmd::TypeIn(dry)));
        assert_eq!(text(&a.state, wet, false), "100%");
        assert_eq!(crate::typein::caption(&a.state, dry), format!("Value for {} - {second} dry", crate::mixer::name(&a.state, 2)));
        accept(&mut a.state, wet, false, "60");
        accept(&mut a.state, dry, false, "40%");
        tick(&mut a);
        let m = &members(&a.state, 2, 0).unwrap()[1];
        assert!((m.wet - 0.6).abs() < 1e-6 && (m.dry - 0.4).abs() < 1e-6);
        let live = &a.plugins.containers[&fx_key(2, 0)];
        assert_eq!(live.keys, keys, "the instances are kept");
        let c = live.fx.lock().unwrap();
        assert!((c.members[1].wet - 0.6).abs() < 1e-6 && (c.members[1].dry - 0.4).abs() < 1e-6, "the chain plays the levels");
        drop(c);
        assert_eq!(crate::history::undo(&mut a.state).as_deref(), Some(LEVEL));
        tick(&mut a);
        assert_eq!(members(&a.state, 2, 0).unwrap()[1].dry, 0.0);
        assert_eq!(a.plugins.containers[&fx_key(2, 0)].fx.lock().unwrap().members[1].dry, 0.0);
        assert_eq!(a.plugins.containers[&fx_key(2, 0)].keys, keys);
    }

    /// The effects inside a container automate and link as a slot's effect does (spec: plugins/patcher/README.md §4.2):
    /// a member's parameter is a parameter of the container's slot naming the member and the index; the browser lists
    /// the member with its parameters, whose menu makes a clip on that target, links it by place and id and enters its
    /// value; the clip keeps naming the member when it moves; a controller drives it. Its levels have the parameter
    /// menu too, their clip named as the value box names them; a controller on one is one undo step while it moves.
    #[test]
    fn members_automate_and_link() {
        use crate::remote::{Link, RCmd};
        use crate::typein::Target as T;
        use crate::views::browser::{rows, Item};
        let (mut a, _, second) = two_effects();
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Wrap(0, true)));
        tick(&mut a);
        a.params_tick();
        let key = a.plugins.containers[&fx_key(2, 0)].keys[1].unwrap();
        assert_eq!(a.state.params.members.get(&key), Some(&(2, 0, 2)));
        let tgt = target::slot_param(2, 0, target::member_item(2, 0).unwrap());
        assert_eq!(crate::params::target_of(&a.state, key, 0), Some(tgt));
        assert_eq!(crate::params::key_of(&a.state, target::decode(tgt)), Some((key, 0)));
        assert_eq!(crate::params::target_of(&a.state, key, usize::from(MEMBER_WET)), None, "past the parameters' room");
        let pname = crate::params::param_name(&a.state, key, 0).unwrap();

        // Show all parameters: the member's node under Effects, after its container.
        assert_eq!(a.browse_params(key), None);
        let node = format!("1:Current project/Effects/{second} in {}", crate::params::effect_label(&a.state, 2, 0).unwrap());
        assert_eq!(a.state.browser_sel.as_deref(), Some(node.as_str()));
        let row = rows(&a.state).iter().position(|r| r.item == Item::Param(key, 0)).expect("the parameter row");
        assert!(a.params_browser_right(row, 0.0, 0.0));
        let menu = a.state.playlist.popup.take().unwrap();
        let Cmd::CreateAutomation(t, name, _) = cmd_of(&menu.items, "Create automation clip") else { panic!() };
        assert_eq!(t, tgt);
        assert_eq!(name, format!("{second} in {} - {} - {pname}", crate::plugins::slot_label(&crate::mixer::slot_name(&a.state, 2, 0).unwrap()), crate::mixer::name(&a.state, 2)));
        let link = T::MemberParam(2, 0, 2, 0);
        assert_eq!(cmd_of(&menu.items, "MIDI learn..."), Cmd::Remote(RCmd::Link(link)));
        assert_eq!(cmd_of(&menu.items, "Enter value..."), Cmd::TypeIn(link));
        assert_eq!(crate::typein::get(&a.state, link), crate::params::param_value(&a.state, key, 0));
        crate::playlist::run(&mut a, Cmd::CreateAutomation(t, name.clone(), 0.5));
        assert_eq!(crate::params::driving_clip(&a.state, tgt), Some(a.state.project.channels.last().unwrap().index));
        assert_eq!(crate::init::name(&a.state, tgt), name, "the clip's name, from its target");
        // Moved to the front of the list: the same member, the same target.
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Member(0, 1, Op::Up)));
        tick(&mut a);
        assert_eq!(a.plugins.containers[&fx_key(2, 0)].keys[0], Some(key));
        assert_eq!(crate::params::key_of(&a.state, target::decode(tgt)), Some((key, 0)));
        assert_eq!(crate::params::target_of(&a.state, key, 0), Some(tgt));

        // A controller on the parameter, kept by place and id.
        crate::remote::set_links(&[]);
        crate::remote::add(Link::new(0, 20, link), true);
        assert_eq!(crate::remote::links().iter().map(|l| l.target).collect::<Vec<_>>(), [link]);
        assert!(a.remote_cc("Test", 0, 20, 127));
        a.plugins.hosted[&key].plugin.lock().unwrap().idle();
        assert!((crate::params::param_value(&a.state, key, 0).unwrap() - 1.0).abs() < 1e-6);
        assert!(crate::remote::label(&a.state, &crate::remote::links()[0]).starts_with(&format!("{second} in ")));

        // The levels: the parameter menu, a clip named as the value box names them, a controller.
        let ops = menu_of(&mut a, 0);
        let wet = &ops[1];
        assert_eq!(wet.label, "Wet: 100%");
        let level = level_target(&a.state, 2, 0, 2, true).unwrap();
        let caption = level_name(&a.state, 2, 0, 2, true);
        assert_eq!(cmd_of(&wet.children, "Create automation clip"), Cmd::CreateAutomation(level, caption.clone(), 1.0));
        assert_eq!(cmd_of(&wet.children, "MIDI learn..."), Cmd::Remote(RCmd::Link(T::Member(2, 0, 2, true))));
        assert_eq!(crate::init::name(&a.state, level), caption);
        assert_eq!(level_of(&a.state, target::decode(level)), Some((2, 0, 2, true)));
        assert_eq!(crate::params::key_of(&a.state, target::decode(level)), None, "not a parameter of the effect");
        crate::remote::add(Link::new(0, 21, T::Member(2, 0, 2, false)), true);
        a.state.history.reset();
        assert!(a.remote_cc("Test", 0, 21, 64));
        assert!(a.remote_cc("Test", 0, 21, 127));
        assert_eq!(members(&a.state, 2, 0).unwrap()[0].dry, 1.0);
        assert_eq!(a.state.history.len(), 1, "one step while it moves");
        crate::remote::set_links(&[]);
    }

    /// The container's window (spec plugins/patcher/README.md §4.3): opening the slot shows it; each member has wet and
    /// dry knobs, a drag on one is one undo step on the list's level and the chain follows; a level the chain plays
    /// (automation) turns its knob without touching the list; a right click on a knob gives Reset and its level's
    /// parameter menu, on a name the member's menu; the window closes when its container goes. Painted, its controls
    /// are there.
    #[test]
    fn the_containers_window() {
        use crate::app::Action;
        use crate::typein::Target as T;
        let (mut a, _, second) = two_effects();
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Wrap(0, true)));
        tick(&mut a);
        crate::mixer::select(&mut a.state, 1, false);
        a.open_fx(2, 0);
        assert!(a.state.shown(Win::Container) && a.state.container.at == (2, 0));
        assert_eq!(member_count(&a.state, 2, 0), Some(2));
        tick(&mut a);
        assert_eq!(a.state.container.live.get(&2), Some(&(1.0, 0.0)), "the chain's levels");
        let (wet, dry) = (KnobId::Member(2, 0, 2, true), KnobId::Member(2, 0, 2, false));
        assert_eq!(a.state.knob_text(wet), format!("{second} wet: 100%"));
        assert_eq!((wet.target(), dry.default_value()), (T::Member(2, 0, 2, true), 0.0));

        // A drag: the list's level, one step; the chain plays it.
        a.state.history.reset();
        a.mouse = (500.0, 500.0);
        a.act(Action::Knob(wet));
        a.mouse_move(500.0, 600.0);
        a.mouse_move(500.0, 756.0);
        a.mouse_up();
        tick(&mut a);
        let want = 1.0 - 256.0 / 1024.0;
        assert!((members(&a.state, 2, 0).unwrap()[1].wet - want).abs() < 1e-5);
        assert_eq!(a.state.history.len(), 1, "one step for the drag");
        assert!((a.plugins.containers[&fx_key(2, 0)].fx.lock().unwrap().members[1].wet - want).abs() < 1e-5);
        assert!((a.state.knob(wet) - want).abs() < 1e-5);
        assert_eq!(crate::history::undo(&mut a.state).as_deref(), Some(LEVEL));
        tick(&mut a);
        assert_eq!(members(&a.state, 2, 0).unwrap()[1].wet, 1.0);
        assert_eq!(a.plugins.containers[&fx_key(2, 0)].fx.lock().unwrap().members[1].wet, 1.0);

        // A level the chain plays (as automation sets it) turns the knob; the list keeps its own.
        a.plugins.containers[&fx_key(2, 0)].fx.lock().unwrap().set_levels(1, 0.3, 0.7);
        tick(&mut a);
        assert!((a.state.knob(wet) - 0.3).abs() < 1e-6 && (a.state.knob(dry) - 0.7).abs() < 1e-6);
        assert_eq!(members(&a.state, 2, 0).unwrap()[1].wet, 1.0);

        // Right clicks: a knob's menu, a name's menu.
        assert!(a.container_right(&Action::Knob(dry)));
        let menu = a.state.playlist.popup.take().unwrap().items;
        assert_eq!(menu[0].label, "Reset");
        assert_eq!(cmd_of(&menu, "Enter value..."), Cmd::TypeIn(T::Member(2, 0, 2, false)));
        assert_eq!(cmd_of(&menu, "Create automation clip"), Cmd::CreateAutomation(level_target(&a.state, 2, 0, 2, false).unwrap(), level_name(&a.state, 2, 0, 2, false), 0.0));
        a.state.set_knob(dry, 0.4);
        a.state.history.reset();
        crate::playlist::run(&mut a, menu[0].cmd.clone());
        assert_eq!(members(&a.state, 2, 0).unwrap()[1].dry, 0.0, "Reset: back to dry 0");
        assert_eq!(a.state.history.newest_done(), Some(LEVEL));
        assert!(a.container_right(&Action::Container(CAct::Open(0))));
        let ops = a.state.playlist.popup.take().unwrap().items;
        assert_eq!(ops.iter().map(|i| i.label.as_ref()).collect::<Vec<_>>(), ["Open editor", "Wet: 100%", "Dry: 0%", "Switch off", "Move up", "Move down", "Remove"]);
        assert_eq!(ops[5].cmd, Cmd::SlotWin(2, Box::new(Cmd::Mixer(MCmd::Member(0, 0, Op::Down)))), "on the container's track");

        // Painted: the knobs, the menu buttons and Add effect are in the window.
        if let Ok(mut c) = os_ui::Canvas::new(1920, 1080, 1.0) {
            a.state.raise(Win::Container);
            a.paint(&mut c, None);
            let win = crate::views::container::rect(&a.state);
            for act in [Action::Knob(wet), Action::Knob(dry), Action::Container(CAct::Menu(1)), Action::Container(CAct::Open(0)), Action::Container(CAct::Add)] {
                let r = a.hits.iter().rev().find(|h| h.action == act).unwrap_or_else(|| panic!("no hit for {act:?}")).rect;
                assert!(win.contains(r.cx(), r.cy()), "{act:?} in the window");
            }
        }

        // The container gone, the window goes.
        crate::playlist::run(&mut a, Cmd::SlotWin(2, Box::new(Cmd::Mixer(MCmd::SlotDelete(0)))));
        tick(&mut a);
        assert!(!a.state.shown(Win::Container));
    }

    /// A member window's menu has a slot effect's entries (no Piano roll, no channel column): Replace ▸ puts another
    /// effect in the member's place, keeping its id and levels (one undo step); Delete... removes it from the container.
    #[test]
    fn a_member_windows_menu() {
        let (mut a, first, second) = two_effects();
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Wrap(0, true)));
        tick(&mut a);
        a.state.set_knob(KnobId::Member(2, 0, 2, false), 0.25);
        tick(&mut a);
        crate::mixer::select(&mut a.state, 1, false);
        let key = a.plugins.containers[&fx_key(2, 0)].keys[1].unwrap();
        let mut m = crate::playlist::plugin_window_menu(key, false, 0.0, 0.0);
        crate::playlist::fill_plugin_window_menu(&a.state, key, &mut m);
        assert!(!m.items.iter().any(|i| i.col || i.label == "Piano roll" || i.label == "Route to free mixer track"), "a slot's set");
        let replace = m.items.iter().find(|i| i.label == "Replace").unwrap();
        let lim = replace.children.iter().find(|i| i.label == first).expect("the effects to pick").cmd.clone();
        assert!(matches!(&lim, Cmd::SlotWin(2, c) if matches!(**c, Cmd::Mixer(MCmd::MemberReplace(0, 1, _)))));
        crate::playlist::run(&mut a, lim);
        tick(&mut a);
        assert_eq!(names(&a, 0), [first.clone(), first.clone()]);
        let m2 = members(&a.state, 2, 0).unwrap()[1].clone();
        assert_eq!((m2.id, m2.wet, m2.dry), (2, 1.0, 0.25), "its id and levels stay");
        assert_ne!(a.plugins.containers[&fx_key(2, 0)].keys[1], Some(key), "a new instance");
        assert_eq!(crate::history::undo(&mut a.state).as_deref(), Some(REPLACE));
        tick(&mut a);
        assert_eq!(names(&a, 0), [first.clone(), second]);
        // Delete...: out of the container.
        let key = a.plugins.containers[&fx_key(2, 0)].keys[1].unwrap();
        let mut m = crate::playlist::plugin_window_menu(key, false, 0.0, 0.0);
        crate::playlist::fill_plugin_window_menu(&a.state, key, &mut m);
        let del = m.items.iter().find(|i| i.label == "Delete...").unwrap().clone();
        assert_eq!(del.cmd, Cmd::SlotWin(2, Box::new(Cmd::Mixer(MCmd::Member(0, 1, Op::Remove)))));
        crate::playlist::run(&mut a, del.cmd);
        tick(&mut a);
        assert_eq!(names(&a, 0), [first]);
    }

    /// A member's window edits and presets are undo steps (spec §4.3): its instance's owner is the id made of its
    /// container's slot's and its own, which finds it again after the list is reordered; a parameter's undo and a
    /// state's undo reach it by that id.
    #[test]
    fn member_edits_and_presets_undo() {
        use crate::control::command::{key_owner, member_owner, owner_key, PluginSide, PLUGIN_PARAMETER, PLUGIN_PRESET};
        let (mut a, _, _) = two_effects();
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Wrap(0, true)));
        tick(&mut a);
        a.params_tick();
        let key = a.plugins.containers[&fx_key(2, 0)].keys[1].unwrap();
        let slot = crate::mixer::track(&a.state, 2).unwrap().slots[0].as_ref().unwrap().id.0;
        let owner = member_owner(slot, 2).unwrap();
        assert_eq!(a.plugins.hosted[&key].owner.0, owner, "hosted under its owner id");
        assert_eq!((owner_key(&a.state, owner), key_owner(&a.state, key)), (Some(key), Some(owner)));
        assert_eq!(member_owner(slot, 16), None);
        // Moved to the front: the same owner finds it.
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Member(0, 1, Op::Up)));
        tick(&mut a);
        assert_eq!(owner_key(&a.state, owner), Some(key));

        // A value typed in (or a window's gesture: the same step by owner): one step, undone on the instance.
        let id = a.plugins.param_infos(key)[0].id;
        let before = a.plugins.param_value(key, id).unwrap();
        let to = if before > 0.5 { 0.1 } else { 0.9 };
        assert!(a.plugins.hosted[&key].plugin.lock().unwrap().set_param(id, to));
        a.state.history.reset();
        crate::control::param_set(&mut a.state, key, 0, Some(before), to);
        assert_eq!(a.state.history.newest_done(), Some(PLUGIN_PARAMETER));
        crate::history::undo(&mut a.state);
        a.host_commands();
        assert!((a.plugins.param_value(key, id).unwrap() - before).abs() < 1e-6, "undone on the member");
        crate::history::redo(&mut a.state);
        a.host_commands();
        assert!((a.plugins.param_value(key, id).unwrap() - to).abs() < 1e-6);

        // A preset: the state and the preset shown before it come back on the member (its window's Presets box names
        // the preset); no name is touched.
        let shown = std::path::PathBuf::from("C:/x/Before.fst");
        let side = PluginSide { state: a.plugins.state(key).unwrap(), preset: Some(shown.clone()), name: None, fill: 0 };
        let names = a.state.mixer.slot_names.clone();
        crate::control::state_edit(&mut a.state, key, PLUGIN_PRESET, side);
        assert_eq!(a.state.history.newest_done(), Some(PLUGIN_PRESET));
        assert_eq!(a.plugins.presets.get(&key), None);
        crate::history::undo(&mut a.state);
        a.host_commands();
        assert_eq!(a.plugins.presets.get(&key), Some(&shown), "the state went back into the member");
        crate::history::redo(&mut a.state);
        a.host_commands();
        assert_eq!(a.plugins.presets.get(&key), None, "and the redo takes the preset away again");
        assert_eq!(a.state.mixer.slot_names, names);
    }
    /// An app with Lemon Studio's instruments and effects, rack row 0 played by Lemon Trio; its ticks.
    fn trio_channel() -> App {
        let mut a = App::new(None);
        a.plugins = crate::plugins::Plugins::default();
        a.plugins.available = crate::plugins::builtin();
        a.plugins.effects = crate::plugins::builtin_effects();
        a.state.plugdb.rebuild(&a.plugins);
        a.plugin_tick();
        a.plugin_tick();
        let trio = a.plugins.index_of(crate::osc3::LEMON_TRIO).unwrap();
        crate::plugin_menu::replace_generator(&mut a, trio, 0);
        ticks(&mut a);
        a
    }

    fn ticks(a: &mut App) {
        (0..3).for_each(|_| a.plugin_tick());
    }

    /// The first leaf of `items` (and their submenus) labelled `label`.
    fn leaf(items: &[PItem], label: &str) -> Option<Cmd> {
        items.iter().find_map(|i| if i.children.is_empty() { (i.label == label).then(|| i.cmd.clone()) } else { leaf(&i.children, label) })
    }

    /// The channel menu's Wrap in container (spec plugins/patcher/README.md §4.4): the channel's instrument goes into a
    /// container on the channel, with its state, at wet and dry 100 %; the channel stays (its name, notes, mixer track),
    /// the container's chain plays it and its window opens; one undo step brings the instrument back, a redo the
    /// container. Add ▸ in its window offers instruments and effects; an instrument added layers over the first, an
    /// effect works on both. A member's parameters and levels are the channel's parameters naming it; its undo steps
    /// go by the channel's id. Saved to a .lemon file and opened again, the container comes back.
    #[test]
    fn wrap_a_channels_instrument() {
        use crate::typein::Target as T;
        use os_engine::Instrument;
        let mut a = App::new(None);
        a.plugins = crate::plugins::Plugins::default();
        a.plugins.available = crate::plugins::builtin();
        a.plugin_tick();
        let item = |a: &App| crate::rack::channel_menu(&a.state, 0, 0.0, 0.0).items.into_iter().find(|i| i.label == WRAP).expect("Wrap in container");
        assert!(!item(&a).enabled, "a sampler channel's sample is the channel's");
        let mut a = trio_channel();
        let before = a.state.project.channels[0].clone();
        let wrap = item(&a);
        assert!(wrap.enabled);
        crate::playlist::run(&mut a, wrap.cmd);
        ticks(&mut a);
        assert_eq!(a.state.history.newest_done(), Some(WRAP));
        let ch = &a.state.project.channels[0];
        assert!(is_container_channel(ch));
        assert_eq!((&ch.name, ch.mixer_track, ch.volume), (&before.name, before.mixer_track, before.volume), "the channel stays");
        let list = members(&a.state, 0, CHANNEL).unwrap();
        assert_eq!(list.len(), 1);
        let m = &list[0];
        assert_eq!((m.id, m.instrument, m.name.as_str(), m.wet, m.dry), (1, true, crate::osc3::LEMON_TRIO, 1.0, 1.0));
        assert!(!m.state.is_empty(), "with its state");
        assert!(!a.state.hosted_rows.contains(&0) && !a.plugins.hosted.contains_key(&0), "no instance under the row");
        assert!(a.state.shown(Win::Container) && a.state.container.at == (0, CHANNEL));
        assert_eq!(title(&a.state, 0, CHANNEL), Some(format!("Container ({})", place_name(&a.state, 0, CHANNEL))));
        assert!(!crate::rack::channel_menu(&a.state, 0, 0.0, 0.0).items.iter().any(|i| i.label == WRAP && i.enabled), "not twice");
        // The chain plays the channel's notes.
        let live = &a.plugins.containers[&0];
        assert!(live.channel && live.keys[0].is_some_and(|k| k >= MEMBER_KEY));
        assert!(a.plugins.instruments().iter().any(|(r, i)| *r == 0 && Arc::ptr_eq(i, &(live.fx.clone() as os_engine::SharedInstrument))));
        assert!(a.plugins.effects().is_empty(), "not a slot's");
        {
            let mut c = live.fx.lock().unwrap();
            c.note_on(0, 69, 0.8);
            let mut peak = 0.0f32;
            for _ in 0..8 {
                let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
                Instrument::process(&mut *c, 48000, &mut l, &mut r);
                peak = l.iter().chain(&r).fold(peak, |p, x| p.max(x.abs()));
            }
            assert!(peak > 0.01, "the instrument sounds through the chain: {peak}");
            Instrument::reset(&mut *c);
        }

        // Undone: the channel and its instrument come back; redone, the container.
        let wrapped = a.state.project.channels[0].clone();
        assert_eq!(crate::history::undo(&mut a.state).as_deref(), Some(WRAP));
        ticks(&mut a);
        assert_eq!(a.state.project.channels[0], before);
        assert!(a.plugins.containers.is_empty() && a.state.hosted_rows.contains(&0));
        assert_eq!(a.plugins.descriptor(0).map(|d| d.name), Some(crate::osc3::LEMON_TRIO.to_string()));
        assert!(!a.state.shown(Win::Container), "the window goes with its container");
        assert_eq!(crate::history::redo(&mut a.state).as_deref(), Some(WRAP));
        ticks(&mut a);
        assert_eq!(a.state.project.channels[0], wrapped);
        assert!(a.plugins.containers.contains_key(&0) && !a.plugins.hosted.contains_key(&0));

        // Add ▸: instruments and effects; an instrument at wet and dry 100 % (a layer), an effect wet only.
        a.open_channel_window(0);
        assert!(a.state.shown(Win::Container) && a.state.container.at == (0, CHANNEL));
        a.container_act(CAct::Add);
        let add = a.state.playlist.popup.take().unwrap().items;
        assert!(add.iter().any(|i| i.head && i.label == "Instruments") && add.iter().any(|i| i.head && i.label == "Effects"));
        let lim = a.plugins.effects.iter().find(|e| e.desc.name.contains("Limiter")).unwrap().desc.name.clone();
        let inst = leaf(&add, crate::osc3::LEMON_TRIO).expect("the instruments to pick");
        assert!(matches!(&inst, Cmd::SlotWin(0, c) if matches!(**c, Cmd::Mixer(MCmd::MemberAddInst(CHANNEL, _)))));
        let fx = leaf(&add, &lim).expect("the effects to pick");
        assert!(matches!(&fx, Cmd::SlotWin(0, c) if matches!(**c, Cmd::Mixer(MCmd::MemberAdd(CHANNEL, _)))));
        crate::playlist::run(&mut a, inst);
        assert_eq!(a.state.history.newest_done(), Some(ADD_INSTRUMENT));
        crate::playlist::run(&mut a, fx);
        ticks(&mut a);
        let list = members(&a.state, 0, CHANNEL).unwrap();
        assert_eq!(list.iter().map(|m| (m.id, m.instrument, m.wet, m.dry)).collect::<Vec<_>>(), [(1, true, 1.0, 1.0), (2, true, 1.0, 1.0), (3, false, 1.0, 0.0)]);
        let live = &a.plugins.containers[&0];
        assert_eq!(live.fx.lock().unwrap().members.len(), 3);
        assert!(matches!(live.fx.lock().unwrap().members[1].node, chain::Node::Inst(_)));

        // Parameters and levels: the channel's parameters naming the member.
        a.params_tick();
        let key = a.plugins.containers[&0].keys[0].unwrap();
        let index = a.state.project.channels[0].index;
        assert_eq!(a.state.params.members.get(&key), Some(&(0, CHANNEL, 1)));
        let tgt = target::channel_param(index, target::member_item(1, 0).unwrap());
        assert_eq!(crate::params::target_of(&a.state, key, 0), Some(tgt));
        assert_eq!(crate::params::key_of(&a.state, target::decode(tgt)), Some((key, 0)));
        let level = level_target(&a.state, 0, CHANNEL, 2, false).unwrap();
        assert_eq!(level, target::channel_param(index, target::member_item(2, MEMBER_DRY).unwrap()));
        assert_eq!(level_of(&a.state, target::decode(level)), Some((0, CHANNEL, 2, false)));
        assert_eq!(crate::typein::text(&a.state, T::Member(0, CHANNEL, 2, false), false), "100%");
        assert!(crate::init::name(&a.state, level).starts_with(&place_name(&a.state, 0, CHANNEL)));
        crate::typein::accept(&mut a.state, T::Member(0, CHANNEL, 2, false), false, "25");
        assert_eq!(a.state.history.newest_done(), Some(LEVEL));
        ticks(&mut a);
        assert_eq!(a.plugins.containers[&0].fx.lock().unwrap().members[1].dry, 0.25, "the chain plays the level");
        // Undo steps of its own edits go by the channel's id.
        let owner = crate::control::command::member_owner(a.state.project.channels[0].id.0, 1).unwrap();
        assert_eq!(crate::control::command::key_owner(&a.state, key), Some(owner));
        assert_eq!(crate::control::command::owner_key(&a.state, owner), Some(key));
        assert_eq!(a.plugins.hosted[&key].owner.0, owner);
        // A member window's Replace ▸ lists instruments for an instrument.
        let mut m = crate::playlist::plugin_window_menu(key, false, 0.0, 0.0);
        crate::playlist::fill_plugin_window_menu(&a.state, key, &mut m);
        let replace = m.items.iter().find(|i| i.label == "Replace").unwrap();
        let pick = leaf(&replace.children, crate::osc3::LEMON_TRIO).expect("instruments");
        assert!(matches!(&pick, Cmd::SlotWin(0, c) if matches!(**c, Cmd::Mixer(MCmd::MemberReplaceInst(CHANNEL, 0, _)))));

        // Saved and opened again.
        let d = std::env::temp_dir().join(format!("lemon-container-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let path = d.join("Channel.lemon");
        a.save_to(path.clone());
        let saved = members(&a.state, 0, CHANNEL).unwrap();
        let mut back = App::new(None);
        back.plugins = crate::plugins::Plugins::default();
        back.plugins.available = crate::plugins::builtin();
        back.plugins.effects = crate::plugins::builtin_effects();
        back.open(path);
        ticks(&mut back);
        assert_eq!(members(&back.state, 0, CHANNEL), Some(saved));
        assert_eq!(back.plugins.containers[&0].fx.lock().unwrap().members.len(), 3);
        assert!(back.plugins.containers[&0].keys.iter().all(Option::is_some));
        let _ = std::fs::remove_dir_all(&d);
    }
    /// The clips and links that drove a wrapped slot's parameters move onto the member that took its effect: Wrap in
    /// container on one slot, Wrap all with each slot's member, and a channel's instrument. Levels and other slots'
    /// targets keep their place; an undo brings the old targets back.
    #[test]
    fn wrapping_moves_the_slots_clips_and_links_onto_its_member() {
        use os_model::ControllerLink;
        let (mut a, _, _) = two_effects();
        let param = target::slot_param(2, 1, 3);
        let other = target::slot_param(2, 0, 2);
        let level = target::slot(2, 1, target::SLOT_MIX);
        for tg in [param, other, level] {
            a.state.project.automations.push(os_flp::project::new_automation(0, tg, 0.5, 4.0));
        }
        a.state.project.controller_links.push(ControllerLink::new(0, 0, param));
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Wrap(1, false)));
        tick(&mut a);
        let member = target::slot_param(2, 1, target::member_item(1, 3).unwrap());
        assert_eq!(a.state.project.automations[0].target(), Some(member));
        assert_eq!(a.state.project.controller_links[0].target(), member);
        assert_eq!(a.state.project.automations[1].target(), Some(other), "another slot's clip stays");
        assert_eq!(a.state.project.automations[2].target(), Some(level), "the slot's level stays");
        crate::history::undo(&mut a.state);
        tick(&mut a);
        assert_eq!(a.state.project.automations[0].target(), Some(param));
        assert_eq!(a.state.project.controller_links[0].target(), param);

        // Wrap all: slot 0 becomes member 1 and slot 1 member 2, and each slot's clip follows its effect.
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Wrap(1, true)));
        tick(&mut a);
        assert_eq!(a.state.project.automations[0].target(), Some(target::slot_param(2, 1, target::member_item(2, 3).unwrap())));
        assert_eq!(a.state.project.controller_links[0].target(), target::slot_param(2, 1, target::member_item(2, 3).unwrap()));
        assert_eq!(a.state.project.automations[1].target(), Some(target::slot_param(2, 1, target::member_item(1, 2).unwrap())));
        assert_eq!(a.state.project.automations[2].target(), Some(level), "the container keeps the slot's level");

        // A container wrapped again by Wrap all: its members' old ids follow them onto their new ones.
        crate::history::undo(&mut a.state);
        tick(&mut a);
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Wrap(1, false)));
        tick(&mut a);
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Wrap(0, true)));
        tick(&mut a);
        let moved = target::slot_param(2, 0, target::member_item(2, 3).unwrap());
        assert_eq!(a.state.project.automations[0].target(), Some(moved));
        assert_eq!(a.state.project.controller_links[0].target(), moved);
        assert_eq!(a.state.project.automations[1].target(), Some(target::slot_param(2, 0, target::member_item(1, 2).unwrap())));
    }

    /// Wrapping a channel's instrument moves the clips and links that drove its parameters onto member 1, and leaves
    /// the channel's own controls alone; an undo brings the old targets back.
    #[test]
    fn wrapping_a_channel_moves_its_clips_and_links_onto_its_member() {
        use os_model::ControllerLink;
        let mut a = trio_channel();
        let index = a.state.project.channels[0].index;
        let param = target::channel_param(index, 5);
        let level = target::channel(index, target::CHANNEL_VOLUME);
        a.state.project.automations.push(os_flp::project::new_automation(0, param, 0.5, 4.0));
        a.state.project.automations.push(os_flp::project::new_automation(0, level, 0.5, 4.0));
        a.state.project.controller_links.push(ControllerLink::new(0, 0, param));
        assert!(a.wrap_channel(0).is_some());
        ticks(&mut a);
        let member = target::channel_param(index, target::member_item(1, 5).unwrap());
        assert_eq!(a.state.project.automations[0].target(), Some(member));
        assert_eq!(a.state.project.controller_links[0].target(), member);
        assert_eq!(a.state.project.automations[1].target(), Some(level), "the channel's volume stays its own");
        crate::history::undo(&mut a.state);
        ticks(&mut a);
        assert_eq!(a.state.project.automations[0].target(), Some(param));
        assert_eq!(a.state.project.controller_links[0].target(), param);
    }
    /// The container's saved graph is the live chain's routing: a graph edit reaches the chain on the next tick,
    /// including a second output and supported note processors. An unsupported note processor is reported and the
    /// chain falls back to the serial list instead of silently dropping it.
    #[test]
    fn the_container_graph_drives_the_live_chain() {
        let (mut a, _, _) = two_effects();
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Wrap(1, true)));
        tick(&mut a);
        let list = members(&a.state, 2, 1).unwrap();
        let second = list[1].id;
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Graph(1, GraphCmd::AddOutput)));
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Graph(1, GraphCmd::Connect(Endpoint::Member(second), Endpoint::Output(1)))));
        tick(&mut a);
        let saved = graph(&a.state, 2, 1).unwrap();
        let live = &a.plugins.containers[&fx_key(2, 1)];
        let chain = live.fx.lock().unwrap();
        assert_eq!(chain.graph(), &saved);
        assert_eq!(chain.output_count(), 2);
        assert!(saved.edges.contains(&Edge { from: Endpoint::Member(second), to: Endpoint::Output(1) }));
        drop(chain);

        // A slot container has no notes, so its Notes... menu and command refuse a note processor instead of saving
        // one that could never run; the slot's audio routing is unchanged.
        let before_notes = graph(&a.state, 2, 1).unwrap();
        assert!(a.edit_graph(1, GraphCmd::AddNoteKind(NoteKind::Transpose { semitones: 12 })).is_some());
        tick(&mut a);
        assert!(graph(&a.state, 2, 1).unwrap().notes.is_empty());
        assert_eq!(graph(&a.state, 2, 1).unwrap().edges, before_notes.edges);

        // An unsupported saved record (an old project, for example) is not silently played: it is reported, and the
        // chain keeps the serial route.
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Graph(1, GraphCmd::AddNote { uid: "clap:note".into(), name: "Note".into() })));
        tick(&mut a);
        assert_eq!(graph(&a.state, 2, 1).unwrap().notes.len(), 1);
        let live = &a.plugins.containers[&fx_key(2, 1)];
        assert_eq!(live.fx.lock().unwrap().graph(), &serial_graph(&list), "the unsupported graph falls back to the serial route");
        assert!(a.state.message.as_deref().is_some_and(|m| m.contains("not supported")), "the refusal names the processor: {:?}", a.state.message);

        // Removing the unsupported record takes the saved graph back into the chain, second output and all.
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Graph(1, GraphCmd::RemoveNote(0))));
        tick(&mut a);
        let saved = graph(&a.state, 2, 1).unwrap();
        assert!(saved.notes.is_empty());
        assert_eq!(a.plugins.containers[&fx_key(2, 1)].fx.lock().unwrap().graph(), &saved);
    }

    /// A channel container's note processors are real graph edits and reach the live chain: Notes... adds a built-in,
    /// edits its settings, reorders it, removes it, and each step is one undo step. A slot container cannot add one.
    #[test]
    fn a_channel_containers_note_processors_reach_the_live_chain() {
        let mut a = trio_channel();
        assert!(a.wrap_channel(0).is_some());
        ticks(&mut a);
        a.state.container.at = (0, CHANNEL);
        let live_key = 0;
        assert!(a.plugins.containers.contains_key(&live_key), "the channel has a live container chain");

        a.graph_act(GAct::NoteMenu);
        let popup = a.state.playlist.popup.take().expect("the Notes... menu");
        let add = popup.items.iter().find(|i| i.label == "Add a note processor").expect("the add submenu");
        let transpose = add.children.iter().find(|i| i.label == "Transpose +12 semitones").expect("Transpose +12");
        crate::playlist::run(&mut a, transpose.cmd.clone());
        tick(&mut a);
        assert_eq!(graph(&a.state, 0, CHANNEL).unwrap().notes[0].kind(), Ok(NoteKind::Transpose { semitones: 12 }));
        assert_eq!(
            a.plugins.containers[&live_key].fx.lock().unwrap().graph().notes[0].kind(),
            Ok(NoteKind::Transpose { semitones: 12 }),
            "the live chain took the added processor"
        );

        // Edit the processor from its own menu: one semitone up.
        a.graph_act(GAct::NoteMenu);
        let popup = a.state.playlist.popup.take().expect("the Notes... menu");
        let first = popup.items.iter().find(|i| i.label == "1. Transpose").expect("the first processor");
        let up = first.children.iter().find(|i| i.label == "Up a semitone").expect("Up a semitone");
        crate::playlist::run(&mut a, up.cmd.clone());
        tick(&mut a);
        assert_eq!(graph(&a.state, 0, CHANNEL).unwrap().notes[0].kind(), Ok(NoteKind::Transpose { semitones: 13 }));

        // Add a velocity processor, then move the transpose below it. The graph and the live chain both reorder.
        a.graph_act(GAct::NoteMenu);
        let popup = a.state.playlist.popup.take().expect("the Notes... menu");
        let add = popup.items.iter().find(|i| i.label == "Add a note processor").expect("the add submenu");
        let velocity = add.children.iter().find(|i| i.label == "Velocity 50%").expect("Velocity 50%");
        crate::playlist::run(&mut a, velocity.cmd.clone());
        tick(&mut a);
        a.graph_act(GAct::NoteMenu);
        let popup = a.state.playlist.popup.take().expect("the Notes... menu");
        let first = popup.items.iter().find(|i| i.label == "1. Transpose").expect("the first processor");
        let down = first.children.iter().find(|i| i.label == "Move down").expect("Move down");
        crate::playlist::run(&mut a, down.cmd.clone());
        tick(&mut a);
        let order: Vec<String> = graph(&a.state, 0, CHANNEL).unwrap().notes.iter().map(|n| n.kind().unwrap().name().to_string()).collect();
        assert_eq!(order, ["Velocity", "Transpose"]);
        assert_eq!(a.plugins.containers[&live_key].fx.lock().unwrap().graph().notes.len(), 2, "both processors reach playback");

        // Remove the velocity processor; the transpose stays and keeps playing.
        a.graph_act(GAct::NoteMenu);
        let popup = a.state.playlist.popup.take().expect("the Notes... menu");
        let first = popup.items.iter().find(|i| i.label == "1. Velocity").expect("the first processor");
        let remove = first.children.iter().find(|i| i.label == "Remove").expect("Remove");
        crate::playlist::run(&mut a, remove.cmd.clone());
        tick(&mut a);
        assert_eq!(graph(&a.state, 0, CHANNEL).unwrap().notes.len(), 1);
        assert_eq!(graph(&a.state, 0, CHANNEL).unwrap().notes[0].kind(), Ok(NoteKind::Transpose { semitones: 13 }));
        assert_eq!(a.plugins.containers[&live_key].fx.lock().unwrap().graph().notes.len(), 1);
    }

    /// The graph window's controls edit the same state as the headless commands, through the same one-undo-step path:
    /// it opens from the container window, disconnects a connection, adds an output, offers a connection through its
    /// menus, and removes an output and an unsupported note record.
    #[test]
    fn the_graph_window_edits_the_containers_graph() {
        let (mut a, _, _) = two_effects();
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Wrap(1, true)));
        tick(&mut a);
        a.state.container.at = (2, 1);
        a.container_act(CAct::Graph);
        assert!(a.state.shown(Win::Graph), "Graph... opens the graph window");

        let list = members(&a.state, 2, 1).unwrap();
        let edge = graph(&a.state, 2, 1).unwrap().edges[0];
        a.graph_act(GAct::Disconnect(edge.from, edge.to));
        tick(&mut a);
        assert!(!graph(&a.state, 2, 1).unwrap().edges.contains(&edge));
        assert_eq!(a.state.history.newest_done(), Some(GRAPH));
        crate::history::undo(&mut a.state);
        tick(&mut a);
        assert!(graph(&a.state, 2, 1).unwrap().edges.contains(&edge), "the window's edit is undone");

        a.graph_act(GAct::AddOutput);
        tick(&mut a);
        assert_eq!(graph(&a.state, 2, 1).unwrap().outputs.len(), 2);
        assert_eq!(a.state.history.newest_done(), Some(GRAPH));

        // Connect... lists the sources; under Input, every destination not connected yet is offered, a second output
        // included. Its command is the same headless command, wrapped for the selected track.
        a.graph_act(GAct::ConnectMenu);
        let popup = a.state.playlist.popup.take().expect("the connect menu");
        let from_input = popup.items.iter().find(|i| i.label == "From Input").expect("the Input source");
        let to_second = from_input.children.iter().find(|i| i.label == "To Output 2").expect("the second output");
        assert!(to_second.enabled);
        crate::playlist::run(&mut a, to_second.cmd.clone());
        tick(&mut a);
        assert!(graph(&a.state, 2, 1).unwrap().edges.contains(&Edge { from: Endpoint::Input, to: Endpoint::Output(1) }));
        assert_eq!(a.state.history.newest_done(), Some(GRAPH));

        // An unsupported note record is shown and can be removed; a second output can be removed. Both reach the chain.
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Graph(1, GraphCmd::AddNote { uid: "clap:note".into(), name: "Note".into() })));
        tick(&mut a);
        assert_eq!(graph(&a.state, 2, 1).unwrap().notes.len(), 1);
        a.graph_act(GAct::RemoveNote(0));
        tick(&mut a);
        assert!(graph(&a.state, 2, 1).unwrap().notes.is_empty());
        a.graph_act(GAct::RemoveOutput(1));
        tick(&mut a);
        assert_eq!(graph(&a.state, 2, 1).unwrap().outputs.len(), 1);
        assert_eq!(list, members(&a.state, 2, 1).unwrap(), "the members are untouched");

        // Closing the container's window closes its graph window too.
        a.act(crate::app::Action::CloseWin(Win::Container));
        assert!(!a.state.shown(Win::Container) && !a.state.shown(Win::Graph));
    }

    /// The container's graph is state: a serial graph from its member list; connect, outputs and note processors are
    /// headless commands, one undo step each; loops and unknown nodes are rejected; member edits keep a hand-edited
    /// graph, dropping only the removed member's connections; save and open keep it.
    #[test]
    fn the_containers_graph_state_is_edited_undone_and_saved() {
        let (mut a, _, _) = two_effects();
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Wrap(1, true)));
        tick(&mut a);
        let list = members(&a.state, 2, 1).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(graph(&a.state, 2, 1), Some(serial_graph(&list)));
        assert_eq!(
            serial_graph(&list).edges.first(),
            Some(&Edge { from: Endpoint::Input, to: Endpoint::Member(list[0].id) }),
            "the serial graph starts at the first member"
        );

        // A serial graph follows a member edit (a switch leaves the routes serial).
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Member(1, 0, Op::Switch)));
        tick(&mut a);
        let list = members(&a.state, 2, 1).unwrap();
        assert_eq!(graph(&a.state, 2, 1), Some(serial_graph(&list)));

        // Headless graph edits: the input straight to output 0 alongside the serial path, a second output, a note
        // processor, and the second member to the second output.
        let cmds = [
            GraphCmd::Connect(Endpoint::Input, Endpoint::Output(0)),
            GraphCmd::AddOutput,
            GraphCmd::Connect(Endpoint::Member(list[1].id), Endpoint::Output(1)),
            GraphCmd::AddNote { uid: "clap:note".into(), name: "Note".into() },
        ];
        for cmd in cmds {
            crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Graph(1, cmd)));
            tick(&mut a);
        }
        assert_eq!(a.state.history.newest_done(), Some(GRAPH));
        let g = graph(&a.state, 2, 1).unwrap();
        assert_eq!(g.outputs, ["Output", "Output 2"]);
        assert_eq!(g.notes.len(), 1);
        assert_eq!((g.notes[0].uid.as_str(), g.notes[0].name.as_str(), g.notes[0].state.as_slice()), ("clap:note", "Note", &[][..]));
        assert!(g.edges.contains(&Edge { from: Endpoint::Input, to: Endpoint::Output(0) }));
        assert!(g.edges.contains(&Edge { from: Endpoint::Member(list[1].id), to: Endpoint::Output(1) }));

        // Undo the note processor and redo it: one undo step each.
        assert_eq!(crate::history::undo(&mut a.state).as_deref(), Some(GRAPH));
        tick(&mut a);
        assert!(graph(&a.state, 2, 1).unwrap().notes.is_empty());
        assert_eq!(crate::history::redo(&mut a.state).as_deref(), Some(GRAPH));
        tick(&mut a);
        assert_eq!(graph(&a.state, 2, 1).unwrap().notes.len(), 1);

        // Disconnect: the connection goes; an undo brings it back.
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Graph(1, GraphCmd::Disconnect(Endpoint::Input, Endpoint::Output(0)))));
        tick(&mut a);
        assert!(!graph(&a.state, 2, 1).unwrap().edges.contains(&Edge { from: Endpoint::Input, to: Endpoint::Output(0) }));
        assert_eq!(crate::history::undo(&mut a.state).as_deref(), Some(GRAPH));
        tick(&mut a);
        assert!(graph(&a.state, 2, 1).unwrap().edges.contains(&Edge { from: Endpoint::Input, to: Endpoint::Output(0) }));

        // Another output, a branch to it, and removing the middle output: the branch follows the output down.
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Graph(1, GraphCmd::AddOutput)));
        tick(&mut a);
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Graph(1, GraphCmd::Connect(Endpoint::Member(list[1].id), Endpoint::Output(2)))));
        tick(&mut a);
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Graph(1, GraphCmd::RemoveOutput(1))));
        tick(&mut a);
        let g = graph(&a.state, 2, 1).unwrap();
        assert_eq!(g.outputs, ["Output", "Output 3"]);
        assert!(g.edges.contains(&Edge { from: Endpoint::Member(list[1].id), to: Endpoint::Output(1) }));
        assert_eq!(crate::history::undo(&mut a.state).as_deref(), Some(GRAPH));
        tick(&mut a);
        assert_eq!(graph(&a.state, 2, 1).unwrap().outputs, ["Output", "Output 2", "Output 3"]);

        // A note processor goes, and an undo brings it back.
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Graph(1, GraphCmd::RemoveNote(0))));
        tick(&mut a);
        assert!(graph(&a.state, 2, 1).unwrap().notes.is_empty());
        assert!(a.edit_graph(1, GraphCmd::RemoveNote(9)).is_some());
        assert_eq!(crate::history::undo(&mut a.state).as_deref(), Some(GRAPH));
        tick(&mut a);
        assert_eq!(graph(&a.state, 2, 1).unwrap().notes.len(), 1);

        // Loops and unknown nodes are rejected, and change nothing.
        let before = graph(&a.state, 2, 1).unwrap();
        assert!(a.edit_graph(1, GraphCmd::Connect(Endpoint::Member(list[0].id), Endpoint::Member(list[0].id))).is_some());
        assert!(a.edit_graph(1, GraphCmd::Connect(Endpoint::Member(99), Endpoint::Output(0))).is_some());
        assert!(a.edit_graph(1, GraphCmd::RemoveOutput(7)).is_some());
        assert_eq!(graph(&a.state, 2, 1), Some(before));

        // Saved and opened again: the graph and its members come back.
        let saved = graph(&a.state, 2, 1).unwrap();
        let saved_members = members(&a.state, 2, 1).unwrap();
        let d = std::env::temp_dir().join(format!("lemon-container-graph-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let path = d.join("Graph.lemon");
        a.save_to(path.clone());
        let mut back = App::new(None);
        back.plugins = crate::plugins::Plugins::default();
        back.plugins.effects = crate::plugins::builtin_effects();
        back.plugins.available = crate::plugins::builtin();
        back.open(path);
        (0..3).for_each(|_| back.plugin_tick());
        assert_eq!(graph(&back.state, 2, 1), Some(saved.clone()));
        assert_eq!(members(&back.state, 2, 1), Some(saved_members));

        // Removing a member keeps the graph, dropping only its connections.
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Member(1, 1, Op::Remove)));
        tick(&mut a);
        assert_eq!(members(&a.state, 2, 1).unwrap().len(), 1);
        let g = graph(&a.state, 2, 1).unwrap();
        assert_eq!(g.outputs, saved.outputs);
        assert_eq!(g.notes, saved.notes);
        assert!(g.edges.iter().all(|e| e.from != Endpoint::Member(list[1].id) && e.to != Endpoint::Member(list[1].id)));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Add output... and Outputs... in the graph window are the same headless commands that edit the state: an output
    /// created already routed to a mixer track, a route changed or cleared, and an output removed with its route.
    #[test]
    fn graph_commands_route_extra_outputs_to_mixer_tracks() {
        let (mut a, _, _) = two_effects();
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Wrap(1, true)));
        tick(&mut a);
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Graph(1, GraphCmd::AddOutputTo(3))));
        tick(&mut a);
        let g = graph(&a.state, 2, 1).unwrap();
        assert_eq!(g.outputs.len(), 2);
        assert_eq!(g.destination(1), Some(3), "the added output starts routed to track 3");
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Graph(1, GraphCmd::RouteOutput(1, Some(4)))));
        tick(&mut a);
        assert_eq!(graph(&a.state, 2, 1).unwrap().destination(1), Some(4));
        let refused = crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Graph(1, GraphCmd::RouteOutput(0, Some(4)))));
        assert!(refused.is_some(), "output 0 belongs to the container's slot or channel");
        tick(&mut a);
        assert_eq!(graph(&a.state, 2, 1).unwrap().destination(0), None);
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Graph(1, GraphCmd::RemoveOutput(1))));
        tick(&mut a);
        let g = graph(&a.state, 2, 1).unwrap();
        assert_eq!(g.outputs, ["Output"]);
        assert_eq!(g.destination(1), None);
    }

    /// A member's menu switches it off and on: the chain skips it, its settings, levels, automation and links stay,
    /// undo and redo follow, and a saved project keeps the switch.
    #[test]
    fn a_member_switches_off_and_on_with_undo_and_saves() {
        use os_model::ControllerLink;
        let (mut a, _, _) = two_effects();
        crate::playlist::run(&mut a, Cmd::Mixer(MCmd::Wrap(1, false)));
        tick(&mut a);
        let key = a.plugins.containers[&fx_key(2, 1)].keys[0].unwrap();
        let param = target::slot_param(2, 1, target::member_item(1, 3).unwrap());
        let other = target::slot_param(2, 0, 2);
        a.state.project.automations.push(os_flp::project::new_automation(0, param, 0.5, 4.0));
        a.state.project.automations.push(os_flp::project::new_automation(0, other, 0.5, 4.0));
        a.state.project.controller_links.push(ControllerLink::new(0, 0, param));
        let before = members(&a.state, 2, 1).unwrap()[0].clone();
        assert!(before.on);
        let ops = member_ops(&a.state, 2, 1, &[before.clone()], 0);
        let switch = ops.iter().find(|i| i.label == "Switch off").expect("Switch off");
        assert_eq!(switch.cmd, Cmd::Mixer(MCmd::Member(1, 0, Op::Switch)));
        crate::playlist::run(&mut a, switch.cmd.clone());
        tick(&mut a);
        assert_eq!(a.state.history.newest_done(), Some(SWITCH));
        let after = members(&a.state, 2, 1).unwrap()[0].clone();
        assert!(!after.on);
        assert_eq!(
            (&after.uid, &after.name, after.wet, after.dry, &after.state),
            (&before.uid, &before.name, before.wet, before.dry, &before.state),
            "its settings and levels stay"
        );
        assert_eq!(a.plugins.containers[&fx_key(2, 1)].keys[0], Some(key), "its instance stays");
        assert!(!a.plugins.containers[&fx_key(2, 1)].fx.lock().unwrap().members[0].on, "the chain skips it");
        assert_eq!(a.state.project.automations[0].target(), Some(param), "its automation stays");
        assert_eq!(a.state.project.automations[1].target(), Some(other));
        assert_eq!(a.state.project.controller_links[0].target(), param, "its link stays");
        assert_eq!(member_ops(&a.state, 2, 1, &[after.clone()], 0).iter().find(|i| i.label == "Switch on").map(|i| i.cmd.clone()), Some(Cmd::Mixer(MCmd::Member(1, 0, Op::Switch))));

        // Undo and redo: the switch goes back and forth.
        crate::history::undo(&mut a.state);
        tick(&mut a);
        assert!(members(&a.state, 2, 1).unwrap()[0].on);
        assert!(a.plugins.containers[&fx_key(2, 1)].fx.lock().unwrap().members[0].on);
        crate::history::redo(&mut a.state);
        tick(&mut a);
        assert!(!members(&a.state, 2, 1).unwrap()[0].on);
        assert_eq!(a.state.project.automations[0].target(), Some(param));
        assert_eq!(a.state.project.controller_links[0].target(), param);

        // A saved project keeps the switch.
        let d = std::env::temp_dir().join(format!("lemon-container-switch-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let path = d.join("Switch.lemon");
        a.save_to(path.clone());
        let mut back = App::new(None);
        back.plugins = crate::plugins::Plugins::default();
        back.plugins.effects = crate::plugins::builtin_effects();
        back.plugins.available = crate::plugins::builtin();
        back.open(path);
        (0..3).for_each(|_| back.plugin_tick());
        assert!(!members(&back.state, 2, 1).unwrap()[0].on);
        assert!(!back.plugins.containers[&fx_key(2, 1)].fx.lock().unwrap().members[0].on);
        let reopened = members(&back.state, 2, 1).unwrap()[0].clone();
        assert_eq!(
            (&reopened.uid, &reopened.name, reopened.wet, reopened.dry, &reopened.state),
            (&before.uid, &before.name, before.wet, before.dry, &before.state),
            "the switched-off member keeps its settings, levels and state"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

}
