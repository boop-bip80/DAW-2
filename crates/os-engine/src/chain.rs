// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! A container's graph chain (Lemon Studio's Patcher, slices 1 and 3): one mixer slot effect, or one channel's
//! instrument, whose members are routed by the container's saved graph and each have their own wet and dry levels. A
//! member is an effect, which works on the signal connected to it, or an instrument (in a channel's container), which
//! plays the channel's notes and puts out its own sound in place of its connection. Members run in the order the
//! connections require; a member's output goes to every destination it connects to, and each named output is the sum of
//! what connects to it. Output 0 is the container's main output (the slot or channel); each other output has a mixer
//! track destination in the graph (`Graph::destinations`) and is handed to that track through the engine's existing
//! extra-output path. A member's dry part waits for the delay that member reports, so
//! the two parts meet in time, and the chain reports the longest member path to output 0 for the mixer's delay
//! compensation. A member switched off is skipped: the block passes through it untouched, it adds no delay, and its
//! parameters, levels and automation stay, so switching it on plays it as before. Tempo, transport and reset reach
//! every member; the slot's sidechain input reaches the first effect in the processing order only; the channel's notes,
//! controllers, voicing and mod X / Y reach every instrument member, while its volume and pan stay the engine's, on the
//! chain's output. Automation and controller links reach a member as the container's parameters: the member's id and an
//! item packed in the parameter index (`os_model::target::member_item`), the item a parameter of the member's plugin or
//! the member's wet or dry level. A graph's note processors transform every incoming note in list order before the
//! instrument members see it; an unknown processor is refused by [`Graph::plan`] instead of being dropped silently.
// spec: plugins/patcher/README.md Â§3.2, Â§3.3, Â§4 slices 1 and 3, Â§4.2, Â§4.4; audio/mixer.md "An effect slot";
// audio/delay-compensation.md

use std::collections::VecDeque;

use os_model::container::{Endpoint, Graph, GraphError, NoteKind};
use os_model::target::{member_of, MEMBER_DRY, MEMBER_WET};

use crate::instrument::{ChannelLevels, ChannelVoicing, HostedEffect, Instrument, NoteLevels, SharedEffect, SharedInstrument, TransportInfo};

/// What a member plays: an effect on the chain's signal, or an instrument whose sound takes the signal's place.
#[derive(Clone)]
pub enum Node {
    Fx(SharedEffect),
    Inst(SharedInstrument),
}

/// One plugin of the chain: its id in the container, its levels (the output is `wet` times the plugin's output plus
/// `dry` times the member's input) and the line its dry part waits in.
pub struct Member {
    pub id: u32,
    pub node: Node,
    pub wet: f32,
    pub dry: f32,
    /// Whether it plays; switched off, it is skipped and its input passes through untouched.
    pub on: bool,
    /// The delay the plugin reported after its last block, and the dry part's line as long as that.
    delay: u32,
    line: [VecDeque<f32>; 2],
}

impl Member {
    /// An effect member.
    pub fn new(id: u32, fx: SharedEffect, wet: f32, dry: f32) -> Member {
        Member::of(id, Node::Fx(fx), wet, dry)
    }

    /// An instrument member (a channel's container): it plays the channel's notes.
    pub fn instrument(id: u32, inst: SharedInstrument, wet: f32, dry: f32) -> Member {
        Member::of(id, Node::Inst(inst), wet, dry)
    }

    fn of(id: u32, node: Node, wet: f32, dry: f32) -> Member {
        Member { id, node, wet, dry, on: true, delay: 0, line: [VecDeque::new(), VecDeque::new()] }
    }

    /// Whether the member's input reaches its output beside the plugin.
    fn blends(&self) -> bool {
        self.dry != 0.0 || self.wet != 1.0
    }

    /// The dry line takes `delay` samples, empty (a new delay, or a reset).
    fn set_delay(&mut self, delay: u32) {
        self.delay = delay;
        for l in &mut self.line {
            l.clear();
            l.resize(delay as usize, 0.0);
        }
    }

    /// A parameter value for the member's plugin (a busy plugin misses it, as a busy slot's does).
    fn param(&self, offset: u32, index: u32, value: f64) {
        match &self.node {
            Node::Fx(fx) => {
                if let Ok(mut fx) = fx.try_lock() {
                    fx.param_fx(offset, index, value);
                }
            }
            Node::Inst(inst) => {
                if let Ok(mut inst) = inst.try_lock() {
                    inst.param(offset, index, value);
                }
            }
        }
    }

    fn tick_params(&self) -> bool {
        match &self.node {
            Node::Fx(fx) => fx.try_lock().is_ok_and(|fx| fx.tick_params_fx()),
            Node::Inst(inst) => inst.try_lock().is_ok_and(|i| i.tick_params()),
        }
    }

    fn param_range(&self, index: usize) -> Option<(f64, f64)> {
        match &self.node {
            Node::Fx(fx) => fx.try_lock().ok()?.param_range_fx(index),
            Node::Inst(inst) => inst.try_lock().ok()?.param_range(index),
        }
    }

    /// Processes one block in place. `key` is the slot's sidechain input for the first effect that takes it; `dry`
    /// holds this member's input while its plugin runs. A switched-off member is left as it is (its input passes
    /// through), and its delay adds nothing.
    fn process_block(&mut self, rate: u32, left: &mut [f32], right: &mut [f32], key: &mut Option<(&[f32], &[f32])>, dry: &mut [Vec<f32>; 2]) {
        if !self.on {
            return;
        }
        let n = left.len().min(right.len());
        let blend = self.blends();
        if blend {
            for (d, src) in dry.iter_mut().zip([&left[..n], &right[..n]]) {
                d.clear();
                d.extend_from_slice(src);
            }
        }
        let delay = match &self.node {
            Node::Fx(fx) => match fx.try_lock() {
                Ok(mut fx) => {
                    match key.take() {
                        Some(k) => fx.process_fx_keyed(rate, &mut left[..n], &mut right[..n], k),
                        None => fx.process_fx(rate, &mut left[..n], &mut right[..n]),
                    }
                    fx.reported_delay_fx()
                }
                Err(_) => self.delay,
            },
            Node::Inst(inst) => match inst.try_lock() {
                Ok(mut i) => {
                    i.process(rate, &mut left[..n], &mut right[..n]);
                    i.reported_delay()
                }
                Err(_) => {
                    left[..n].fill(0.0);
                    right[..n].fill(0.0);
                    self.delay
                }
            },
        };
        if delay != self.delay {
            self.set_delay(delay);
        }
        if !blend {
            return;
        }
        for (c, out) in [&mut left[..n], &mut right[..n]].into_iter().enumerate() {
            let line = &mut self.line[c];
            for (o, &x) in out.iter_mut().zip(&dry[c]) {
                let late = if self.delay == 0 {
                    x
                } else {
                    line.push_back(x);
                    line.pop_front().unwrap_or(0.0)
                };
                *o = *o * self.wet + late * self.dry;
            }
        }
    }
}

/// Adds `src` into `dst` sample by sample (both blocks the same length).
fn add_block(dst: &mut [Vec<f32>; 2], src_l: &[f32], src_r: &[f32]) {
    for (d, s) in dst[0].iter_mut().zip(src_l) {
        *d += *s;
    }
    for (d, s) in dst[1].iter_mut().zip(src_r) {
        *d += *s;
    }
}

/// A note event waiting for a delay processor's due frame. The key has already been transposed.
#[derive(Debug, Clone, Copy)]
enum PendingEvent {
    On { key: u8, velocity: f32, start: f32, levels: NoteLevels, voice: Option<u32> },
    Off(u8),
    Choke(u8, Option<u32>),
}

/// A pending note and the frame it is due, from the chain's first processed block.
#[derive(Debug, Clone, Copy)]
struct Pending {
    at: u64,
    event: PendingEvent,
}

/// The chain: its members, the graph that routes them, the buffers one block's dry parts are kept in, and whether
/// its last block put out sound (an instrument container keeps its effects' tails playing while it does).
#[derive(Default)]
pub struct Chain {
    pub members: Vec<Member>,
    graph: Graph,
    /// Member indices in the order their graph connections require.
    order: Vec<usize>,
    /// Whether each member can reach an output (a member cut off from every output is not processed).
    active: Vec<bool>,
    /// One input block per member: the sum of the connections into it.
    inputs: Vec<[Vec<f32>; 2]>,
    /// The last block's signal on each named output.
    outputs: Vec<[Vec<f32>; 2]>,
    dry: [Vec<f32>; 2],
    sounding: bool,
    /// The graph's note processors, in the order they transform a channel's notes.
    notes: Vec<NoteKind>,
    /// Frames the instrument path has processed, for delayed notes.
    clock: u64,
    /// The sample rate the engine set; the fallback is used only by a direct test that did not set one.
    rate: u32,
    /// Ticks per quarter note from the channel's voicing (96 until it is given).
    ppq: u16,
    /// The transport at the current tick, for a delay in ticks.
    transport: Option<TransportInfo>,
    /// Delayed note events, earliest first; empty when no delay processor is active.
    pending: Vec<Pending>,
    /// Input key, the key actually sent, and the delay the note started with, for each held note, oldest first. The
    /// note API has no note id, so the position in this list is the identity: a note-off consumes the oldest held note
    /// of its key, while a later same-key retrigger keeps its own entry. These values are captured at note-on and are
    /// not recomputed from the current graph, so a live processor edit cannot make a release early, late or missing.
    held: Vec<(u8, u8, u32)>,
}

/// Below this, a block's output is taken as silence (`Chain::active`).
const QUIET: f32 = 1e-5;

impl Chain {
    /// A serial chain (the graph of a container before it was edited).
    pub fn new(members: Vec<Member>) -> Chain {
        let ids: Vec<u32> = members.iter().map(|m| m.id).collect();
        Chain::with_graph(members, Graph::serial(&ids)).expect("a serial container graph is valid")
    }

    /// A chain routed by `graph`. The graph must name only members in `members`, must be acyclic, and every note
    /// processor must be a built-in kind this build understands.
    pub fn with_graph(members: Vec<Member>, graph: Graph) -> Result<Chain, GraphError> {
        let ids: Vec<u32> = members.iter().map(|m| m.id).collect();
        let plan = graph.plan(&ids)?;
        let notes = note_kinds(&graph)?;
        let order = plan.order.iter().map(|id| ids.iter().position(|x| x == id).expect("graph order names a member")).collect();
        let active = ids.iter().zip(&plan.reaches_output).map(|(&id, &on)| ids.iter().position(|x| *x == id).is_some() && on).collect();
        let outputs = graph.outputs.len();
        Ok(Chain {
            members,
            graph,
            order,
            active,
            inputs: vec![[Vec::new(), Vec::new()]; ids.len()],
            outputs: vec![[Vec::new(), Vec::new()]; outputs],
            dry: [Vec::new(), Vec::new()],
            sounding: false,
            notes,
            clock: 0,
            rate: 0,
            ppq: 96,
            transport: None,
            pending: Vec::new(),
            held: Vec::new(),
        })
    }

    /// The graph the chain renders.
    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    /// The number of named outputs in the graph.
    pub fn output_count(&self) -> usize {
        self.graph.outputs.len()
    }

    /// The last processed block on output `k` (left, right). Output 0 is what reaches the mixer slot or channel; the
    /// other outputs are the graph's extra outputs, which the engine routes to [`Chain::output_destination`].
    pub fn output(&self, k: usize) -> Option<(&[f32], &[f32])> {
        self.outputs.get(k).map(|o| (&o[0][..], &o[1][..]))
    }

    /// The mixer track output `k` plays on (`None` for output 0, which stays in the container's slot or channel, or an
    /// extra output with no destination).
    pub fn output_destination(&self, k: usize) -> Option<usize> {
        self.graph.destination(k).map(usize::from)
    }

    /// Rebuilds the routing from a saved graph, keeping the members and their instances. `Ok(())` only when the graph
    /// is valid and playable; an error leaves the current routing in place. A note that is already held keeps the
    /// transformed key and delay it started with; this edit changes the next note, not its release.
    pub fn set_graph(&mut self, graph: Graph) -> Result<(), GraphError> {
        let ids: Vec<u32> = self.members.iter().map(|m| m.id).collect();
        let plan = graph.plan(&ids)?;
        let notes = note_kinds(&graph)?;
        self.order = plan.order.iter().map(|id| ids.iter().position(|x| x == id).expect("graph order names a member")).collect();
        self.active = ids.iter().zip(&plan.reaches_output).map(|(&id, &on)| ids.iter().position(|x| *x == id).is_some() && on).collect();
        self.outputs.clear();
        self.outputs.resize_with(graph.outputs.len(), || [Vec::new(), Vec::new()]);
        self.notes = notes;
        self.graph = graph;
        Ok(())
    }

    /// Member `i`'s levels (a change in the container's list).
    pub fn set_levels(&mut self, i: usize, wet: f32, dry: f32) {
        if let Some(m) = self.members.get_mut(i) {
            (m.wet, m.dry) = (wet, dry);
        }
    }

    /// Member `i` switched on or off (a change in the container's list). Switching either way empties its dry line, so
    /// that audio from before the switch cannot come back with it.
    pub fn set_on(&mut self, i: usize, on: bool) {
        if let Some(m) = self.members.get_mut(i) {
            if m.on != on {
                m.on = on;
                let d = m.delay;
                m.set_delay(d);
            }
        }
    }

    /// The member and item that parameter `index` of the container names.
    fn item(&self, index: u32) -> Option<(&Member, u16)> {
        let (id, item) = member_of(u16::try_from(index).ok()?)?;
        Some((self.members.iter().find(|m| m.id == id)?, item))
    }

    /// The instrument members' plugins that are not busy elsewhere, for the channel's notes and settings.
    fn instruments(&self, mut f: impl FnMut(&mut dyn Instrument)) {
        for m in &self.members {
            if let Node::Inst(inst) = &m.node {
                if m.on {
                    if let Ok(mut i) = inst.try_lock() {
                        f(&mut *i);
                    }
                }
            }
        }
    }

    /// Renders one block through the graph: input edges seed members or outputs, members run in topological order,
    /// each member's output fans out to every connection it has, and each named output is the sum of its incoming
    /// member (or input) signals. Output 0 replaces `left` / `right`; [`Chain::output`] exposes the rest.
    fn run(&mut self, rate: u32, left: &mut [f32], right: &mut [f32], key: Option<(&[f32], &[f32])>) {
        let n = left.len().min(right.len());
        if self.graph.outputs.is_empty() {
            return;
        }
        let Chain { members, graph, order, active, inputs, outputs, dry, .. } = self;
        if outputs.len() != graph.outputs.len() {
            outputs.clear();
            outputs.resize_with(graph.outputs.len(), || [Vec::new(), Vec::new()]);
        }
        for b in inputs.iter_mut() {
            b[0].clear();
            b[0].resize(n, 0.0);
            b[1].clear();
            b[1].resize(n, 0.0);
        }
        for b in outputs.iter_mut() {
            b[0].clear();
            b[0].resize(n, 0.0);
            b[1].clear();
            b[1].resize(n, 0.0);
        }
        // The container's input goes to every input edge.
        for e in &graph.edges {
            if e.from != Endpoint::Input {
                continue;
            }
            match e.to {
                Endpoint::Member(id) => {
                    if let Some(i) = members.iter().position(|m| m.id == id) {
                        add_block(&mut inputs[i], &left[..n], &right[..n]);
                    }
                }
                Endpoint::Output(k) => {
                    if let Some(o) = outputs.get_mut(usize::from(k)) {
                        add_block(o, &left[..n], &right[..n]);
                    }
                }
                Endpoint::Input => {}
            }
        }
        let mut key = key;
        for &i in order.iter() {
            if !active.get(i).copied().unwrap_or(false) {
                continue;
            }
            let mut l = std::mem::take(&mut inputs[i][0]);
            let mut r = std::mem::take(&mut inputs[i][1]);
            let dl = std::mem::take(&mut dry[0]);
            let dr = std::mem::take(&mut dry[1]);
            members[i].process_block(rate, &mut l, &mut r, &mut key, &mut [dl, dr]);
            let id = members[i].id;
            for e in &graph.edges {
                if e.from != Endpoint::Member(id) {
                    continue;
                }
                match e.to {
                    Endpoint::Member(j) => {
                        if let Some(j) = members.iter().position(|m| m.id == j) {
                            add_block(&mut inputs[j], &l, &r);
                        }
                    }
                    Endpoint::Output(k) => {
                        if let Some(o) = outputs.get_mut(usize::from(k)) {
                            add_block(o, &l, &r);
                        }
                    }
                    Endpoint::Input => {}
                }
            }
        }
        // Output 0 is the container's main output (what the mixer slot or channel hears).
        match outputs.first() {
            Some(o) => {
                let m = n.min(o[0].len()).min(o[1].len());
                left[..m].copy_from_slice(&o[0][..m]);
                right[..m].copy_from_slice(&o[1][..m]);
                left[m..n].fill(0.0);
                right[m..n].fill(0.0);
            }
            None => {
                left[..n].fill(0.0);
                right[..n].fill(0.0);
            }
        }
    }

    /// The delay on output 0: the longest member path that reaches it, with switched-off members adding none.
    fn delay(&self) -> u32 {
        let mut best = 0;
        for e in &self.graph.edges {
            if e.from == Endpoint::Input && e.to == Endpoint::Output(0) {
                best = 0;
            }
        }
        for m in &self.members {
            if let Some(d) = self.path_delay(m.id, &mut Vec::new()) {
                best = best.max(d);
            }
        }
        best
    }

    fn path_delay(&self, id: u32, seen: &mut Vec<u32>) -> Option<u32> {
        if seen.contains(&id) {
            return None;
        }
        let m = self.members.iter().find(|m| m.id == id)?;
        seen.push(id);
        let own = if m.on { m.delay } else { 0 };
        let mut best = None;
        for e in &self.graph.edges {
            if e.from != Endpoint::Member(id) {
                continue;
            }
            let after = match e.to {
                Endpoint::Output(0) => Some(0),
                Endpoint::Member(next) => self.path_delay(next, seen),
                _ => None,
            };
            if let Some(after) = after {
                let d = own + after;
                best = Some(best.map_or(d, |b: u32| b.max(d)));
            }
        }
        seen.pop();
        best
    }

    /// A member's parameter goes to its plugin; its wet and dry levels are set here, for the whole of the next block.
    fn set_param(&mut self, offset: u32, index: u32, value: f64) {
        let Some((id, item)) = u16::try_from(index).ok().and_then(member_of) else { return };
        let Some(m) = self.members.iter_mut().find(|m| m.id == id) else { return };
        let level = value.clamp(0.0, 1.0) as f32;
        match item {
            MEMBER_WET => m.wet = level,
            MEMBER_DRY => m.dry = level,
            k => m.param(offset, u32::from(k), value),
        }
    }

    /// Each member's plugin takes its parameters as it would in a slot or channel; the levels take events.
    fn ticks_at(&self, index: u32) -> bool {
        match self.item(index) {
            Some((m, k)) if k < MEMBER_WET => m.tick_params(),
            _ => false,
        }
    }

    fn range(&self, index: usize) -> Option<(f64, f64)> {
        let (m, k) = self.item(u32::try_from(index).ok()?)?;
        if k >= MEMBER_WET {
            return Some((0.0, 1.0));
        }
        m.param_range(usize::from(k))
    }

    /// A note's transformed key, velocity and total delay in ticks, in the graph's processor order.
    fn transform(&self, key: u8, velocity: f32) -> (u8, f32, u32) {
        let mut key = i16::from(key);
        let mut velocity = velocity.clamp(0.0, 1.0);
        let mut ticks = 0u32;
        for op in &self.notes {
            match *op {
                NoteKind::Transpose { semitones } => key = (key + i16::from(semitones)).clamp(0, 127),
                NoteKind::Velocity { percent } => velocity = (velocity * f32::from(percent.min(200)) / 100.0).clamp(0.0, 1.0),
                NoteKind::Delay { ticks: d } => ticks = ticks.saturating_add(d),
            }
        }
        (key as u8, velocity, ticks)
    }

    /// The key a note would get now, without changing its velocity. Used for an end whose note did not go through
    /// `note_on` (a defensive fallback), while held notes use their recorded key.
    fn transposed_key(&self, key: u8) -> u8 {
        self.transform(key, 0.0).0
    }

    /// The delay in frames for `ticks` at the transport's current tempo and this chain's tick length.
    fn delay_frames(&self, ticks: u32) -> u64 {
        if ticks == 0 {
            return 0;
        }
        let rate = if self.rate == 0 { 48_000 } else { self.rate };
        let tempo = self.transport.map_or(120.0, |t| t.tempo).max(1.0);
        let per_beat = f64::from(rate) * 60.0 / tempo;
        (f64::from(ticks) * per_beat / f64::from(self.ppq.max(1))).round() as u64
    }

    /// Sends a note event now or queues it for a delay processor. `offset` is a frame in the next process call.
    fn schedule(&mut self, offset: u32, event: PendingEvent, ticks: u32) {
        let delay = self.delay_frames(ticks);
        if delay == 0 {
            self.send_event(event, offset);
        } else {
            self.pending.push(Pending { at: self.clock + u64::from(offset) + delay, event });
        }
    }

    /// Sends every pending event due in the next `n` frames, at its own frame in the block. Events due exactly at
    /// the block's end wait for the next block, so none is lost or sent twice.
    fn flush_pending(&mut self, n: usize) {
        if self.pending.is_empty() || n == 0 {
            return;
        }
        let mut all = std::mem::take(&mut self.pending);
        all.sort_by_key(|p| p.at);
        let end = self.clock + n as u64;
        let split = all.partition_point(|p| p.at < end);
        let later = all.split_off(split);
        for p in all {
            let offset = (p.at.saturating_sub(self.clock)).min(n as u64 - 1) as u32;
            self.send_event(p.event, offset);
        }
        self.pending = later;
    }

    /// Hands one transformed note event to every instrument member.
    fn send_event(&mut self, event: PendingEvent, offset: u32) {
        self.instruments(|i| match event {
            PendingEvent::On { key, velocity, start, levels, voice: Some(voice) } => {
                i.voice_start(offset, key, velocity, start, levels, voice);
            }
            PendingEvent::On { key, velocity, start, levels, voice: None } => {
                i.note_start(offset, key, velocity, start, levels);
            }
            PendingEvent::Off(key) => i.note_off(offset, key),
            PendingEvent::Choke(key, Some(voice)) => i.voice_choke(offset, key, voice),
            PendingEvent::Choke(key, None) => i.choke(offset, key),
        });
    }

    /// The key and delay an incoming key is sounding with; a note off keeps the delay its note on had.
    fn held_key(&self, key: u8) -> Option<(u8, u32)> {
        self.held.iter().rev().find(|(k, _, _)| *k == key).map(|&(_, sent, ticks)| (sent, ticks))
    }

    /// Drops the oldest held mapping for `key`, if any. Note-offs arrive in note-on order for overlapping same-key
    /// notes; consuming the newest would give the later note the earlier note's delay (and leave the earlier note
    /// sounding until the later release).
    fn release_key(&mut self, key: u8) -> Option<(u8, u32)> {
        let at = self.held.iter().position(|(k, _, _)| *k == key)?;
        let (_, sent, ticks) = self.held.remove(at);
        Some((sent, ticks))
    }

    fn reset_all(&mut self) {
        self.pending.clear();
        self.held.clear();
        self.clock = 0;
        for m in &mut self.members {
            match &m.node {
                Node::Fx(fx) => {
                    if let Ok(mut fx) = fx.try_lock() {
                        fx.reset_fx();
                    }
                }
                Node::Inst(inst) => {
                    if let Ok(mut i) = inst.try_lock() {
                        i.reset();
                    }
                }
            }
            let d = m.delay;
            m.set_delay(d);
        }
        self.sounding = false;
    }

    fn transport_all(&mut self, info: &TransportInfo) {
        self.transport = Some(*info);
        for m in &self.members {
            match &m.node {
                Node::Fx(fx) => {
                    if let Ok(mut fx) = fx.try_lock() {
                        fx.transport_fx(info);
                    }
                }
                Node::Inst(inst) => {
                    if let Ok(mut i) = inst.try_lock() {
                        i.transport(info);
                    }
                }
            }
        }
    }

}

impl HostedEffect for Chain {
    fn process_fx(&mut self, rate: u32, left: &mut [f32], right: &mut [f32]) {
        self.run(rate, left, right, None);
    }

    fn process_fx_keyed(&mut self, rate: u32, left: &mut [f32], right: &mut [f32], key: (&[f32], &[f32])) {
        self.run(rate, left, right, Some(key));
    }

    fn reset_fx(&mut self) {
        self.reset_all();
    }

    fn transport_fx(&mut self, info: &TransportInfo) {
        self.transport_all(info);
    }

    fn reported_delay_fx(&self) -> u32 {
        self.delay()
    }

    fn extra_outputs_fx(&self) -> usize {
        self.output_count().saturating_sub(1)
    }

    fn extra_output_fx(&self, k: usize) -> Option<(&[f32], &[f32])> {
        self.output(k + 1)
    }

    fn extra_output_destination_fx(&self, k: usize) -> Option<usize> {
        self.output_destination(k + 1)
    }

    fn param_fx(&mut self, offset: u32, index: u32, value: f64) {
        self.set_param(offset, index, value);
    }

    fn tick_params_at(&self, index: u32) -> bool {
        self.ticks_at(index)
    }

    fn param_range_fx(&self, index: usize) -> Option<(f64, f64)> {
        self.range(index)
    }
}

/// A channel's container: the chain as the channel's instrument (spec: plugins/patcher/README.md Â§4.4). Its input is
/// silence; the instrument members' sound goes through the effects after them.
impl Instrument for Chain {
    fn set_sample_rate(&mut self, rate: u32) {
        self.rate = rate;
    }

    fn note_on(&mut self, offset: u32, key: u8, velocity: f32) {
        self.note_start(offset, key, velocity, 0.0, NoteLevels::default());
    }

    fn note_off(&mut self, offset: u32, key: u8) {
        // The held mapping is the release contract: use the key and delay the note was started with, even if the graph
        // has been edited since. The defensive fallback is only for an off whose on never reached this chain.
        let (sent, ticks) = self.release_key(key).unwrap_or_else(|| {
            let (sent, _, ticks) = self.transform(key, 0.0);
            (sent, ticks)
        });
        self.schedule(offset, PendingEvent::Off(sent), ticks);
    }

    fn note_on_from(&mut self, offset: u32, key: u8, velocity: f32, start: f32) {
        self.note_start(offset, key, velocity, start, NoteLevels::default());
    }

    fn note_start(&mut self, offset: u32, key: u8, velocity: f32, start: f32, levels: NoteLevels) {
        let (sent, velocity, ticks) = self.transform(key, velocity);
        self.held.push((key, sent, ticks));
        self.schedule(offset, PendingEvent::On { key: sent, velocity, start, levels, voice: None }, ticks);
    }

    fn voice_start(&mut self, offset: u32, key: u8, velocity: f32, start: f32, levels: NoteLevels, voice: u32) {
        let (sent, velocity, ticks) = self.transform(key, velocity);
        self.held.push((key, sent, ticks));
        self.schedule(offset, PendingEvent::On { key: sent, velocity, start, levels, voice: Some(voice) }, ticks);
    }

    fn voice_choke(&mut self, offset: u32, key: u8, voice: u32) {
        let (sent, ticks) = self.held_key(key).unwrap_or_else(|| {
            let (sent, _, ticks) = self.transform(key, 0.0);
            (sent, ticks)
        });
        self.schedule(offset, PendingEvent::Choke(sent, Some(voice)), ticks);
    }

    fn choke(&mut self, offset: u32, key: u8) {
        let (sent, ticks) = self.held_key(key).unwrap_or_else(|| {
            let (sent, _, ticks) = self.transform(key, 0.0);
            (sent, ticks)
        });
        self.schedule(offset, PendingEvent::Choke(sent, None), ticks);
    }

    fn tune(&mut self, offset: u32, key: u8, semitones: f32) {
        let sent = self.held_key(key).map_or_else(|| self.transposed_key(key), |(sent, _)| sent);
        self.instruments(|i| i.tune(offset, sent, semitones));
    }

    fn set_bend_range(&mut self, semitones: u32) {
        self.instruments(|i| i.set_bend_range(semitones));
    }

    /// Only when every instrument inside plays a note's tuning: otherwise the engine moves the keys by whole semitones,
    /// which every instrument plays right.
    fn takes_tuning(&self) -> bool {
        let mut all = None;
        self.instruments(|i| all = Some(all.unwrap_or(true) && i.takes_tuning()));
        all.unwrap_or(false)
    }

    fn process(&mut self, rate: u32, left: &mut [f32], right: &mut [f32]) {
        self.rate = rate;
        let n = left.len().min(right.len());
        self.flush_pending(n);
        left.fill(0.0);
        right.fill(0.0);
        self.run(rate, left, right, None);
        self.clock += n as u64;
        self.sounding = left.iter().chain(right.iter()).any(|x| x.abs() > QUIET);
    }

    /// While an instrument inside rings, a delayed note is waiting, or the chain's last block was not silent (an
    /// effect's tail).
    fn active(&self) -> bool {
        let mut any = self.sounding || !self.pending.is_empty();
        self.instruments(|i| any |= i.active());
        any
    }

    fn voices(&self) -> usize {
        let mut n = 0;
        self.instruments(|i| n += i.voices());
        n
    }

    fn reset(&mut self) {
        self.reset_all();
    }

    fn param(&mut self, offset: u32, index: u32, value: f64) {
        self.set_param(offset, index, value);
    }

    fn tick_params_at(&self, index: u32) -> bool {
        self.ticks_at(index)
    }

    fn param_range(&self, index: usize) -> Option<(f64, f64)> {
        self.range(index)
    }

    /// The instruments inside get the channel's mod X and mod Y, at full volume and centred: the engine puts the
    /// channel's volume and pan on the chain's output (the channel's settings act after the container).
    fn channel_levels(&mut self, levels: ChannelLevels) -> bool {
        let inner = ChannelLevels { volume: 1.0, pan: 0.0, ..levels };
        self.instruments(|i| {
            i.channel_levels(inner);
        });
        false
    }

    fn channel_voicing(&mut self, voicing: &ChannelVoicing) {
        self.ppq = voicing.ppq;
        self.instruments(|i| i.channel_voicing(voicing));
    }

    fn transport(&mut self, info: &TransportInfo) {
        self.transport_all(info);
    }

    fn midi(&mut self, offset: u32, bytes: [u8; 3]) {
        self.instruments(|i| i.midi(offset, bytes));
    }

    fn reported_delay(&self) -> u32 {
        self.delay()
    }

    fn extra_outputs(&self) -> usize {
        self.output_count().saturating_sub(1)
    }

    fn extra_output(&self, k: usize) -> Option<(&[f32], &[f32])> {
        self.output(k + 1)
    }

    fn extra_output_destination(&self, k: usize) -> Option<usize> {
        self.output_destination(k + 1)
    }
}

/// The built-in note processors in `graph`, in list order; an unknown or malformed one is refused by name.
fn note_kinds(graph: &Graph) -> Result<Vec<NoteKind>, GraphError> {
    graph.notes.iter().map(|n| n.kind().map_err(GraphError::UnsupportedNote)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use os_model::container::{Edge, NoteKind, NoteProcessor};
    use std::sync::{Arc, Mutex};

    /// A test effect: a gain, put out `late` samples late. Its parameter 0 sets the gain (0..1 to 0..4); it takes values
    /// per tick when `ticks`.
    struct Late {
        gain: f32,
        late: usize,
        line: [VecDeque<f32>; 2],
        ticks: bool,
        got: Vec<(u32, u32, f64)>,
    }

    impl Late {
        fn shared(gain: f32, late: usize) -> SharedEffect {
            Arc::new(Mutex::new(Late::new(gain, late)))
        }

        fn new(gain: f32, late: usize) -> Late {
            Late { gain, late, line: [VecDeque::from(vec![0.0; late]), VecDeque::from(vec![0.0; late])], ticks: false, got: Vec::new() }
        }
    }

    impl HostedEffect for Late {
        fn param_fx(&mut self, offset: u32, index: u32, value: f64) {
            self.got.push((offset, index, value));
            if index == 0 {
                self.gain = value as f32 * 4.0;
            }
        }
        fn tick_params_fx(&self) -> bool {
            self.ticks
        }
        fn param_range_fx(&self, index: usize) -> Option<(f64, f64)> {
            (index == 0).then_some((0.0, 4.0))
        }
        fn process_fx(&mut self, _rate: u32, left: &mut [f32], right: &mut [f32]) {
            for (c, buf) in [left, right].into_iter().enumerate() {
                for x in buf.iter_mut() {
                    self.line[c].push_back(*x * self.gain);
                    *x = self.line[c].pop_front().unwrap();
                }
            }
        }
        fn reported_delay_fx(&self) -> u32 {
            self.late as u32
        }
    }

    fn impulse(chain: &mut Chain, len: usize) -> Vec<f32> {
        let mut out = Vec::new();
        for b in 0..4 {
            let mut l = vec![0.0; len];
            let mut r = vec![0.0; len];
            if b == 1 {
                l[0] = 1.0;
                r[0] = 1.0;
            }
            chain.process_fx(48000, &mut l, &mut r);
            assert_eq!(l, r);
            out.extend(l);
        }
        out
    }

    /// The old serial render path, kept as a test reference: members run directly in list order, with no graph
    /// routing or output buffers. This is deliberately separate from `Chain::run` so the serial-graph test compares two
    /// implementations instead of the graph constructor against itself.
    fn legacy_serial_impulse(members: &mut [Member], len: usize) -> Vec<f32> {
        let mut dry = [Vec::new(), Vec::new()];
        let mut out = Vec::new();
        for b in 0..4 {
            let mut l = vec![0.0; len];
            let mut r = vec![0.0; len];
            if b == 1 {
                l[0] = 1.0;
                r[0] = 1.0;
            }
            let mut key = None;
            for m in members.iter_mut() {
                m.process_block(48000, &mut l, &mut r, &mut key, &mut dry);
            }
            assert_eq!(l, r);
            out.extend(l);
        }
        out
    }

    /// Members run in order; the chain's delay is the sum of theirs; a member's dry part meets its wet part in time.
    #[test]
    fn a_serial_chain_with_wet_and_dry() {
        let mut c = Chain::new(vec![Member::new(1, Late::shared(0.5, 3), 1.0, 0.0), Member::new(2, Late::shared(2.0, 5), 0.25, 0.75)]);
        // The first block finds out the delays.
        impulse(&mut c, 16);
        assert_eq!(c.reported_delay_fx(), 8);
        let out = impulse(&mut c, 16);
        // In at sample 16: the first member puts out 0.5 at 19; the second takes it 5 later at 24, times 2 times wet
        // 0.25 plus its dry 0.5 times 0.75, delayed to the same 24.
        let at: Vec<(usize, f32)> = out.iter().copied().enumerate().filter(|&(_, v)| v != 0.0).collect();
        assert_eq!(at, vec![(24, 0.5 * 2.0 * 0.25 + 0.5 * 0.75)]);
        c.set_levels(1, 0.0, 1.0);
        let out = impulse(&mut c, 16);
        assert_eq!(out.iter().position(|&v| v != 0.0), Some(24), "dry only: still delayed to match");
        // A busy member passes the block through.
        let Node::Fx(held) = c.members[0].node.clone() else { panic!("an effect") };
        let _guard = held.lock().unwrap();
        let mut l = vec![1.0; 4];
        let mut r = vec![1.0; 4];
        c.members[1].wet = 1.0;
        c.members[1].dry = 0.0;
        c.reset_fx();
        c.process_fx(48000, &mut l, &mut r);
        assert_eq!(c.reported_delay_fx(), 8);
    }

    /// A member switched off is skipped by the processing path: its block comes out unchanged, its delay leaves the
    /// chain's, and switching it on processes it again. Its parameters still reach it while it is off.
    #[test]
    fn a_switched_off_member_passes_the_block_through() {
        let mut c = Chain::new(vec![Member::new(1, Late::shared(2.0, 3), 1.0, 0.0)]);
        impulse(&mut c, 8);
        assert_eq!(c.reported_delay_fx(), 3);
        c.set_on(0, false);
        assert!(!c.members[0].on);
        assert_eq!(c.reported_delay_fx(), 0, "an off member adds no delay");
        let (mut l, mut r) = (vec![1.0; 8], vec![1.0; 8]);
        c.process_fx(48000, &mut l, &mut r);
        assert_eq!(l, vec![1.0; 8], "no gain, no delay: the block passed through");
        assert_eq!(r, vec![1.0; 8]);
        // Its parameters still reach it while it is off: the gain set then plays when it is switched on.
        c.param_fx(0, u32::from(os_model::target::member_item(1, 0).unwrap()), 0.75);
        c.set_on(0, true);
        assert_eq!(c.reported_delay_fx(), 3);
        let (mut l, mut r) = (vec![0.0; 8], vec![0.0; 8]);
        l[0] = 1.0;
        r[0] = 1.0;
        c.process_fx(48000, &mut l, &mut r);
        assert_eq!(l[3], 3.0, "on again: the gain set while it was off plays, with the effect's delay");
    }

    /// The container's parameters reach its members by id (wherever they are in the chain): a parameter to the effect,
    /// the levels to the member; an id or item the chain has not goes nowhere.
    #[test]
    fn members_parameters_and_levels() {
        use os_model::target::member_item;
        let ticked = Late { ticks: true, ..Late::new(1.0, 0) };
        let (a, b) = (Arc::new(Mutex::new(Late::new(1.0, 0))), Arc::new(Mutex::new(ticked)));
        let mut c = Chain::new(vec![Member::new(4, b.clone(), 1.0, 0.0), Member::new(2, a.clone(), 1.0, 0.0)]);
        let k = |id: u32, item: u16| u32::from(member_item(id, item).unwrap());
        c.param_fx(5, k(2, 0), 0.5);
        c.param_fx(0, k(2, 9), 0.25);
        assert_eq!(a.lock().unwrap().got, vec![(5, 0, 0.5), (0, 9, 0.25)]);
        assert_eq!(a.lock().unwrap().gain, 2.0);
        assert!(b.lock().unwrap().got.is_empty());
        c.param_fx(0, k(4, MEMBER_WET), 0.5);
        c.param_fx(0, k(4, MEMBER_DRY), 1.5);
        assert_eq!((c.members[0].wet, c.members[0].dry), (0.5, 1.0), "levels clamp to 0..1");
        // Not a member: a plain index, an id not in the chain.
        c.param_fx(0, 3, 1.0);
        c.param_fx(0, k(3, 0), 1.0);
        assert_eq!(a.lock().unwrap().got.len(), 2);
        assert!(b.lock().unwrap().got.is_empty());
        // Per tick as the member's effect takes them; the levels and other indexes as events.
        assert!(HostedEffect::tick_params_at(&c, k(4, 0)));
        assert!(!HostedEffect::tick_params_at(&c, k(2, 0)));
        assert!(!HostedEffect::tick_params_at(&c, k(4, MEMBER_WET)));
        assert_eq!(c.param_range_fx(k(2, 0) as usize), Some((0.0, 4.0)));
        assert_eq!(c.param_range_fx(k(2, MEMBER_DRY) as usize), Some((0.0, 1.0)));
        assert_eq!(c.param_range_fx(k(2, 1) as usize), None);
        // The levels sound: member 4 at wet 0.5, dry 1 over member 2 at gain 2.
        let (mut l, mut r) = (vec![1.0; 4], vec![1.0; 4]);
        c.process_fx(48000, &mut l, &mut r);
        assert_eq!(l, vec![(0.5 + 1.0) * 2.0; 4]);
    }
    /// A test effect: a hard clip (order with a gain is audible: gain then clip is not clip then gain).
    struct Clip {
        limit: f32,
    }

    impl Clip {
        fn shared(limit: f32) -> SharedEffect {
            Arc::new(Mutex::new(Clip { limit }))
        }
    }

    impl HostedEffect for Clip {
        fn process_fx(&mut self, _rate: u32, left: &mut [f32], right: &mut [f32]) {
            for x in left.iter_mut().chain(right.iter_mut()) {
                *x = x.clamp(-self.limit, self.limit);
            }
        }
    }

    /// A graph with one output that runs `order` from input to output.
    fn routed(order: &[u32]) -> Graph {
        let mut edges = Vec::new();
        if let Some(&first) = order.first() {
            edges.push(Edge { from: Endpoint::Input, to: Endpoint::Member(first) });
            for i in 1..order.len() {
                edges.push(Edge { from: Endpoint::Member(order[i - 1]), to: Endpoint::Member(order[i]) });
            }
            edges.push(Edge { from: Endpoint::Member(*order.last().unwrap()), to: Endpoint::Output(0) });
        } else {
            edges.push(Edge { from: Endpoint::Input, to: Endpoint::Output(0) });
        }
        Graph { outputs: vec!["Main".into()], destinations: vec![None], notes: Vec::new(), edges }
    }

    fn block(c: &mut Chain, sample: f32) -> Vec<f32> {
        let mut l = vec![sample; 8];
        let mut r = vec![sample; 8];
        c.process_fx(48000, &mut l, &mut r);
        assert_eq!(l, r);
        l
    }

    /// The same two members in two graphs: the graph's order decides whether the gain comes before the clip. The member
    /// list is [gain, clip] in both chains; only the connections differ.
    #[test]
    fn the_graph_decides_processing_order() {
        let members = || vec![Member::new(1, Late::shared(2.0, 0), 1.0, 0.0), Member::new(2, Clip::shared(0.5), 1.0, 0.0)];
        let mut gain_then_clip = Chain::with_graph(members(), routed(&[1, 2])).unwrap();
        let mut clip_then_gain = Chain::with_graph(members(), routed(&[2, 1])).unwrap();
        assert_eq!(block(&mut gain_then_clip, 1.0), vec![0.5; 8], "gain 2 then clip 0.5");
        assert_eq!(block(&mut clip_then_gain, 1.0), vec![1.0; 8], "clip 0.5 then gain 2");
        assert_ne!(block(&mut gain_then_clip, 1.0), block(&mut clip_then_gain, 1.0));
    }

    /// Each output hears only the members connected to it: output 0 has member 1, output 2 has member 2, and output
    /// 1 receives both through fan-out. Nothing leaks to an output a member is not connected to.
    #[test]
    fn a_second_output_gets_its_connected_members() {
        let graph = Graph {
            outputs: vec!["Main".into(), "Aux".into(), "Aux 2".into()],
            destinations: vec![None, Some(1), Some(2)],
            notes: Vec::new(),
            edges: vec![
                Edge { from: Endpoint::Input, to: Endpoint::Member(1) },
                Edge { from: Endpoint::Member(1), to: Endpoint::Output(0) },
                Edge { from: Endpoint::Member(1), to: Endpoint::Output(1) },
                Edge { from: Endpoint::Input, to: Endpoint::Member(2) },
                Edge { from: Endpoint::Member(2), to: Endpoint::Output(1) },
                Edge { from: Endpoint::Member(2), to: Endpoint::Output(2) },
            ],
        };
        let mut c = Chain::with_graph(vec![Member::new(1, Late::shared(3.0, 0), 1.0, 0.0), Member::new(2, Late::shared(5.0, 0), 1.0, 0.0)], graph).unwrap();
        assert_eq!(block(&mut c, 1.0), vec![3.0; 8], "output 0 is member 1 only, not member 2");
        assert_eq!(c.output(1).unwrap().0, vec![8.0; 8], "output 1 is the fan-out of member 1 plus member 2");
        assert_eq!(c.output(2).unwrap().0, vec![5.0; 8], "output 2 is member 2 only, not member 1");
        assert_eq!(c.output(3), None);
    }

    /// A serial container renders exactly as the old serial path did: the test's direct serial loop and the
    /// serial-graph renderer get the same samples through delay discovery, and both equal the golden value.
    #[test]
    fn a_serial_graph_matches_the_legacy_chain() {
        let members = || vec![Member::new(1, Late::shared(0.5, 3), 1.0, 0.0), Member::new(2, Late::shared(2.0, 5), 0.25, 0.75)];
        let mut old_members = members();
        let old_out = legacy_serial_impulse(&mut old_members, 16);
        let mut routed = Chain::with_graph(members(), Graph::serial(&[1, 2])).unwrap();
        impulse(&mut routed, 16);
        let new_out = impulse(&mut routed, 16);
        assert_eq!(new_out, old_out, "the graph path reproduces the direct serial reference exactly");
        let nonzero: Vec<(usize, f32)> = old_out.iter().copied().enumerate().filter(|&(_, v)| v != 0.0).collect();
        assert_eq!(nonzero, vec![(24, 0.5 * 2.0 * 0.25 + 0.5 * 0.75)], "the serial golden value");
    }

    /// Note processors are part of the graph's playable state; an unknown one is refused by name.
    #[test]
    fn a_graph_with_unknown_notes_is_rejected() {
        let mut graph = Graph::serial(&[1]);
        graph.notes.push(NoteProcessor { uid: "clap:note".into(), name: "Note".into(), state: Vec::new() });
        let made = Chain::with_graph(vec![Member::new(1, Late::shared(1.0, 0), 1.0, 0.0)], graph);
        assert!(matches!(made, Err(GraphError::UnsupportedNote(_))));
        let mut graph = Graph::serial(&[1]);
        graph.notes.push(NoteProcessor::builtin(NoteKind::Transpose { semitones: 12 }));
        assert!(Chain::with_graph(vec![Member::new(1, Late::shared(1.0, 0), 1.0, 0.0)], graph).is_ok());
    }

    /// A test instrument: while a key is held it puts out `level` times the velocity on both sides, `late` samples late;
    /// it notes what it was given.
    struct Tone {
        level: f32,
        held: Vec<(u8, f32)>,
        late: u32,
        got: Vec<String>,
        tuning: bool,
    }

    impl Tone {
        fn shared(level: f32, late: u32, tuning: bool) -> Arc<Mutex<Tone>> {
            Arc::new(Mutex::new(Tone { level, held: Vec::new(), late, got: Vec::new(), tuning }))
        }
    }

    impl Instrument for Tone {
        fn note_on(&mut self, _offset: u32, key: u8, velocity: f32) {
            self.held.push((key, velocity));
        }
        fn note_off(&mut self, _offset: u32, key: u8) {
            self.held.retain(|h| h.0 != key);
        }
        fn process(&mut self, _rate: u32, left: &mut [f32], right: &mut [f32]) {
            let v: f32 = self.held.iter().map(|h| h.1 * self.level).sum();
            left.fill(v);
            right.fill(v);
        }
        fn active(&self) -> bool {
            !self.held.is_empty()
        }
        fn voices(&self) -> usize {
            self.held.len()
        }
        fn param(&mut self, offset: u32, index: u32, value: f64) {
            self.got.push(format!("param {offset} {index} {value}"));
        }
        fn channel_levels(&mut self, levels: ChannelLevels) -> bool {
            self.got.push(format!("levels {} {} {}", levels.volume, levels.pan, levels.mod_x));
            true
        }
        fn takes_tuning(&self) -> bool {
            self.tuning
        }
        fn reported_delay(&self) -> u32 {
            self.late
        }
    }

    /// A channel's container: the channel's notes reach every instrument inside, whose sound goes through the effects
    /// after it; an instrument at dry 1 adds its sound to what came before (a layer); the channel's volume and pan stay
    /// the engine's; parameters reach an instrument member by id; an effect's tail keeps the chain active.
    #[test]
    fn an_instrument_container() {
        use os_model::target::member_item;
        let (a, b) = (Tone::shared(1.0, 0, true), Tone::shared(0.25, 0, false));
        let fx = Late::shared(0.5, 0);
        let mut c = Chain::new(vec![
            Member::instrument(1, a.clone(), 1.0, 0.0),
            Member::instrument(3, b.clone(), 1.0, 1.0),
            Member::new(2, fx, 1.0, 0.0),
        ]);
        let inst: &mut dyn Instrument = &mut c;
        assert!(!inst.active());
        inst.note_on(0, 60, 0.8);
        assert_eq!(inst.voices(), 2, "both instruments play the note");
        let (mut l, mut r) = (vec![9.0; 4], vec![9.0; 4]);
        inst.process(48000, &mut l, &mut r);
        // (0.8 from the first, plus the second's 0.2 at dry 1) through the effect's gain of 0.5.
        assert_eq!(l, vec![(0.8 + 0.2) * 0.5; 4]);
        assert_eq!(l, r);
        assert!(inst.active());
        assert!(!inst.takes_tuning(), "the second does not take a tuning");
        assert!(!inst.channel_levels(ChannelLevels { volume: 0.5, pan: -1.0, mod_x: 0.25, mod_y: 0.0 }), "the engine applies the volume");
        assert_eq!(a.lock().unwrap().got, vec!["levels 1 0 0.25".to_string()]);
        let k = |id: u32, item: u16| u32::from(member_item(id, item).unwrap());
        inst.param(7, k(3, 12), 0.5);
        inst.param(0, k(1, MEMBER_DRY), 1.0);
        assert_eq!(b.lock().unwrap().got.last().map(String::as_str), Some("param 7 12 0.5"));
        assert_eq!(c.members[0].dry, 1.0);
        let inst: &mut dyn Instrument = &mut c;
        assert_eq!(inst.param_range(k(3, MEMBER_WET) as usize), Some((0.0, 1.0)));
        inst.note_off(0, 60);
        assert_eq!(inst.voices(), 0);
        let (mut l, mut r) = (vec![0.0; 4], vec![0.0; 4]);
        inst.process(48000, &mut l, &mut r);
        assert!(!inst.active(), "no voice, a silent block");
        // A delay inside an instrument is the chain's; its dry part waits for it.
        let late = Tone::shared(1.0, 2, false);
        let mut c = Chain::new(vec![Member::instrument(1, late, 1.0, 0.0), Member::new(2, Late::shared(1.0, 3), 1.0, 0.0)]);
        let (mut l, mut r) = (vec![0.0; 4], vec![0.0; 4]);
        Instrument::process(&mut c, 48000, &mut l, &mut r);
        assert_eq!(Instrument::reported_delay(&c), 5);
        assert_eq!(HostedEffect::reported_delay_fx(&c), 5);
    }
}
