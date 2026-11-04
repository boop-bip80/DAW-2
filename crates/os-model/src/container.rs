// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! A container's routing graph: its named outputs, note processors and the connections between its input, its plugins
//! (members) and its outputs. The audio engine renders the connections and the channel's notes pass through the note
//! processors before its instruments; the app edits and saves the graph.

use std::fmt;

/// A node an edge starts or ends at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Endpoint {
    /// The container's audio input.
    Input,
    /// A member's output.
    Member(u32),
    /// One of the container's outputs.
    Output(u8),
}

/// One connection: the sound at `from` goes to `to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    pub from: Endpoint,
    pub to: Endpoint,
}

/// The built-in note processors a container can run before its instruments. Their state is strict: an unknown plugin
/// or a malformed state is reported, never treated as an identity transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKind {
    /// Move every note's key by `semitones`; keys outside 0..=127 are clamped to the nearest playable key.
    Transpose { semitones: i8 },
    /// Scale every note's velocity by `percent` (0..=200); the result is clamped to 0..=1.
    Velocity { percent: u8 },
    /// Start every note (and its end) `ticks` later, at the project's tick length. 0 = no delay.
    Delay { ticks: u32 },
}

impl NoteKind {
    pub const TRANSPOSE_UID: &'static str = "lemon-note:transpose";
    pub const VELOCITY_UID: &'static str = "lemon-note:velocity";
    pub const DELAY_UID: &'static str = "lemon-note:delay";

    /// The plugin identity the built-in processor is saved under.
    pub fn uid(self) -> &'static str {
        match self {
            NoteKind::Transpose { .. } => Self::TRANSPOSE_UID,
            NoteKind::Velocity { .. } => Self::VELOCITY_UID,
            NoteKind::Delay { .. } => Self::DELAY_UID,
        }
    }

    /// The editor's name for the kind.
    pub fn name(self) -> &'static str {
        match self {
            NoteKind::Transpose { .. } => "Transpose",
            NoteKind::Velocity { .. } => "Velocity",
            NoteKind::Delay { .. } => "Delay",
        }
    }

    /// The state bytes the processor saves: one byte for transpose or velocity, four for a delay in ticks.
    pub fn state(self) -> Vec<u8> {
        match self {
            NoteKind::Transpose { semitones } => vec![semitones as u8],
            NoteKind::Velocity { percent } => vec![percent],
            NoteKind::Delay { ticks } => ticks.to_le_bytes().to_vec(),
        }
    }

    /// The kind and settings a saved processor names, or `None` when its plugin is not one of ours or its state is
    /// malformed.
    pub fn from_saved(uid: &str, state: &[u8]) -> Option<NoteKind> {
        match uid {
            Self::TRANSPOSE_UID if state.len() == 1 => {
                let semitones = state[0] as i8;
                (-48..=48).contains(&semitones).then_some(NoteKind::Transpose { semitones })
            }
            Self::VELOCITY_UID if state.len() == 1 => {
                (state[0] <= 200).then_some(NoteKind::Velocity { percent: state[0] })
            }
            Self::DELAY_UID if state.len() == 4 => {
                Some(NoteKind::Delay { ticks: u32::from_le_bytes(state.try_into().ok()?) })
            }
            _ => None,
        }
    }

    /// A short description for the graph editor, with the settings that will be applied.
    pub fn description(self) -> String {
        match self {
            NoteKind::Transpose { semitones } => format!("Transpose {semitones:+} semitones"),
            NoteKind::Velocity { percent } => format!("Velocity {percent}%"),
            NoteKind::Delay { ticks } => format!("Delay {ticks} tick{}", if ticks == 1 { "" } else { "s" }),
        }
    }
}

/// A note processor before the instruments: its plugin, name and saved state. The built-in kinds are [`NoteKind`];
/// any other plugin is kept in the graph and reported as unsupported by [`Graph::plan`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NoteProcessor {
    pub uid: String,
    pub name: String,
    pub state: Vec<u8>,
}

impl NoteProcessor {
    /// A saved processor for one of the built-in kinds.
    pub fn builtin(kind: NoteKind) -> NoteProcessor {
        NoteProcessor { uid: kind.uid().into(), name: kind.name().into(), state: kind.state() }
    }

    /// The transform this record names, or an explanation when it is not one the engine can play.
    pub fn kind(&self) -> Result<NoteKind, String> {
        NoteKind::from_saved(&self.uid, &self.state)
            .ok_or_else(|| format!("note processor \"{}\" ({}) is not supported", if self.name.is_empty() { &self.uid } else { &self.name }, self.uid))
    }
}

/// A container's routing graph.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Graph {
    pub outputs: Vec<String>,
    /// The mixer track each output plays on. Output 0 is the container's own slot or channel, so its entry is ignored;
    /// an extra output without a destination is silent (its entry is `None`).
    pub destinations: Vec<Option<u16>>,
    pub notes: Vec<NoteProcessor>,
    pub edges: Vec<Edge>,
}

/// Why a graph cannot be rendered. An unknown note processor is reported by name rather than silently ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphError {
    EmptyOutputs,
    TooManyOutputs,
    UnknownMember(u32),
    UnknownOutput(u8),
    BadEdge(Edge),
    Cycle,
    UnsupportedNote(String),
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphError::EmptyOutputs => f.write_str("a container graph needs an output"),
            GraphError::TooManyOutputs => f.write_str("a container holds at most 256 outputs"),
            GraphError::UnknownMember(id) => write!(f, "no member {id} in the container"),
            GraphError::UnknownOutput(k) => write!(f, "no output {k} in the container"),
            GraphError::BadEdge(e) => write!(f, "a connection from {:?} to {:?} is not valid", e.from, e.to),
            GraphError::Cycle => f.write_str("the connections make a loop"),
            GraphError::UnsupportedNote(name) => f.write_str(name),
        }
    }
}

impl std::error::Error for GraphError {}

/// How the engine renders a graph: the member order (member ids) and which members reach an output at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphPlan {
    /// Member ids in the order their connections require.
    pub order: Vec<u32>,
    /// For each id in the list passed to [`Graph::plan`], whether it can reach an output.
    pub reaches_output: Vec<bool>,
}

impl Graph {
    /// A serial graph: the input through every member in list order to output 0.
    pub fn serial(member_ids: &[u32]) -> Graph {
        let mut edges = Vec::new();
        match member_ids {
            [] => edges.push(Edge { from: Endpoint::Input, to: Endpoint::Output(0) }),
            [first] => {
                edges.push(Edge { from: Endpoint::Input, to: Endpoint::Member(*first) });
                edges.push(Edge { from: Endpoint::Member(*first), to: Endpoint::Output(0) });
            }
            _ => {
                edges.push(Edge { from: Endpoint::Input, to: Endpoint::Member(member_ids[0]) });
                for w in member_ids.windows(2) {
                    edges.push(Edge { from: Endpoint::Member(w[0]), to: Endpoint::Member(w[1]) });
                }
                let last = member_ids[member_ids.len() - 1];
                edges.push(Edge { from: Endpoint::Member(last), to: Endpoint::Output(0) });
            }
        }
        Graph { outputs: vec!["Output".into()], destinations: vec![None], notes: Vec::new(), edges }
    }

    /// The mixer track output `k` plays on; output 0 is its container's own slot or channel, so it has no map.
    pub fn destination(&self, k: usize) -> Option<u16> {
        if k == 0 {
            return None;
        }
        self.destinations.get(k).copied().flatten()
    }

    /// The graph with connections to members that are gone dropped (the rest of an edit keeps them).
    pub fn pruned(&self, member_ids: &[u32]) -> Graph {
        let mut g = self.clone();
        g.edges.retain(|e| {
            [e.from, e.to].into_iter().all(|p| match p {
                Endpoint::Member(id) => member_ids.contains(&id),
                Endpoint::Input | Endpoint::Output(_) => true,
            })
        });
        g
    }

    /// Checks that every edge names a real member or output and runs in the allowed direction.
    pub fn validate(&self, member_ids: &[u32]) -> Result<(), GraphError> {
        if self.outputs.is_empty() {
            return Err(GraphError::EmptyOutputs);
        }
        if self.outputs.len() > usize::from(u8::MAX) + 1 {
            return Err(GraphError::TooManyOutputs);
        }
        let known_member = |id: u32| member_ids.contains(&id);
        let known_output = |k: u8| usize::from(k) < self.outputs.len();
        let member = |id: u32| if known_member(id) { Ok(()) } else { Err(GraphError::UnknownMember(id)) };
        let output = |k: u8| if known_output(k) { Ok(()) } else { Err(GraphError::UnknownOutput(k)) };
        for e in &self.edges {
            match (e.from, e.to) {
                (Endpoint::Input, Endpoint::Member(id)) => member(id)?,
                (Endpoint::Input, Endpoint::Output(k)) => output(k)?,
                (Endpoint::Member(a), Endpoint::Member(b)) => {
                    member(a)?;
                    member(b)?;
                }
                (Endpoint::Member(a), Endpoint::Output(k)) => {
                    member(a)?;
                    output(k)?;
                }
                (Endpoint::Output(k), _) => {
                    output(k)?;
                    return Err(GraphError::BadEdge(*e));
                }
                _ => return Err(GraphError::BadEdge(*e)),
            }
        }
        Ok(())
    }

    /// The member order and reachability for rendering. Every note processor must be a built-in kind whose saved state
    /// this build understands.
    pub fn plan(&self, member_ids: &[u32]) -> Result<GraphPlan, GraphError> {
        self.validate(member_ids)?;
        for n in &self.notes {
            if let Err(name) = n.kind() {
                return Err(GraphError::UnsupportedNote(name));
            }
        }
        let n = member_ids.len();
        let at = |id: u32| member_ids.iter().position(|&x| x == id);
        let mut indegree = vec![0usize; n];
        for e in &self.edges {
            if let (Endpoint::Member(a), Endpoint::Member(b)) = (e.from, e.to) {
                let (_a, b) = (at(a).ok_or(GraphError::UnknownMember(a))?, at(b).ok_or(GraphError::UnknownMember(b))?);
                indegree[b] += 1;
            }
        }
        // Kahn's order, taking the earliest member in the list when several are ready. A serial graph keeps its list
        // order exactly; independent branches are deterministic.
        let mut order = Vec::with_capacity(n);
        let mut left = indegree.clone();
        while order.len() < n {
            let Some(i) = (0..n).find(|&i| left[i] == 0 && !order.contains(&member_ids[i])) else { return Err(GraphError::Cycle) };
            order.push(member_ids[i]);
            for e in &self.edges {
                if let (Endpoint::Member(a), Endpoint::Member(b)) = (e.from, e.to) {
                    if a == member_ids[i] {
                        let b = at(b).ok_or(GraphError::UnknownMember(b))?;
                        left[b] = left[b].saturating_sub(1);
                    }
                }
            }
        }
        let mut reaches = vec![false; n];
        loop {
            let mut changed = false;
            for e in &self.edges {
                match (e.from, e.to) {
                    (Endpoint::Member(a), Endpoint::Output(_)) => {
                        if let Some(a) = at(a) {
                            changed |= !reaches[a];
                            reaches[a] = true;
                        }
                    }
                    (Endpoint::Member(a), Endpoint::Member(b)) => {
                        if let (Some(a), Some(b)) = (at(a), at(b)) {
                            if reaches[b] && !reaches[a] {
                                reaches[a] = true;
                                changed = true;
                            }
                        }
                    }
                    _ => {}
                }
            }
            if !changed {
                break;
            }
        }
        Ok(GraphPlan { order, reaches_output: reaches })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_serial_graph_keeps_the_list_order() {
        let g = Graph::serial(&[4, 9]);
        assert_eq!(g.outputs, ["Output"]);
        assert_eq!(
            g.edges,
            vec![
                Edge { from: Endpoint::Input, to: Endpoint::Member(4) },
                Edge { from: Endpoint::Member(4), to: Endpoint::Member(9) },
                Edge { from: Endpoint::Member(9), to: Endpoint::Output(0) },
            ]
        );
        let plan = g.plan(&[4, 9]).unwrap();
        assert_eq!(plan.order, [4, 9]);
        assert_eq!(plan.reaches_output, [true, true]);
    }

    #[test]
    fn the_plan_follows_connections_and_rejects_bad_state() {
        let g = Graph {
            outputs: vec!["Main".into(), "Aux".into()],
            destinations: vec![None, Some(3)],
            notes: Vec::new(),
            edges: vec![
                Edge { from: Endpoint::Input, to: Endpoint::Member(1) },
                Edge { from: Endpoint::Member(1), to: Endpoint::Member(2) },
                Edge { from: Endpoint::Member(2), to: Endpoint::Output(1) },
                Edge { from: Endpoint::Member(1), to: Endpoint::Output(0) },
            ],
        };
        let plan = g.plan(&[1, 2]).unwrap();
        assert_eq!(plan.order, [1, 2]);
        assert_eq!(plan.reaches_output, [true, true]);
        assert_eq!(g.outputs.len(), 2);
        assert_eq!(g.destination(0), None, "output 0 is the container itself");
        assert_eq!(g.destination(1), Some(3));
        assert_eq!(g.destination(2), None);

        let mut looped = g.clone();
        looped.edges.push(Edge { from: Endpoint::Member(2), to: Endpoint::Member(1) });
        assert_eq!(looped.plan(&[1, 2]), Err(GraphError::Cycle));

        let mut unknown = g.clone();
        unknown.edges.push(Edge { from: Endpoint::Input, to: Endpoint::Output(2) });
        assert_eq!(unknown.plan(&[1, 2]), Err(GraphError::UnknownOutput(2)));

        let mut notes = g.clone();
        notes.notes.push(NoteProcessor::builtin(NoteKind::Velocity { percent: 50 }));
        assert!(notes.plan(&[1, 2]).is_ok());
        notes.notes.push(NoteProcessor { uid: "clap:note".into(), name: "Note".into(), state: Vec::new() });
        assert!(matches!(notes.plan(&[1, 2]), Err(GraphError::UnsupportedNote(ref e)) if e.contains("clap:note") && e.contains("not supported")));
    }

    #[test]
    fn pruning_drops_only_missing_members() {
        let g = Graph {
            outputs: vec!["Main".into()],
            destinations: vec![None],
            notes: Vec::new(),
            edges: vec![
                Edge { from: Endpoint::Input, to: Endpoint::Member(1) },
                Edge { from: Endpoint::Member(1), to: Endpoint::Member(2) },
                Edge { from: Endpoint::Member(2), to: Endpoint::Output(0) },
            ],
        };
        assert_eq!(g.pruned(&[1]).edges, vec![Edge { from: Endpoint::Input, to: Endpoint::Member(1) }]);
    }
}
