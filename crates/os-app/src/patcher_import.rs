// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Opening the original's Patcher slots that hold one plain serial chain of effects as Lemon Studio containers. The
//! parser is in `os_flp::patcher`; this module maps its nodes to hosted members and replaces the slot in place.
//!
//! spec: plugins/patcher/state.md §1 to §7; plugins/patcher/README.md §3.1, §4 slice 5

use crate::app::State;
use crate::container::{encode, Member, CONTAINER, MAX_MEMBERS};

/// The member a node of the original's Patcher opens as, its plugin found among `effects` the way a plain mixer slot
/// holding that plugin is played on opening (spec: plugins/patcher/state.md §7):
/// 1. with the "installed native plugins first" setting, the installed native plugin (`App::host_native`);
/// 2. an FL effect a Lemon effect recreates: that effect, with the state its reader makes of the node's stream
///    (`App::sync_effects`);
/// 3. a wrapped plugin (the wrapper's stream, `flp/events/plugins.md` §4): the plugin `project_plugins::find` gives,
///    from its defaults when it is a copy in another format (`App::sync_wrapped`);
/// 4. else the installed native plugin of that name.
///
/// A plugin not found is kept with its stream, and the chain passes it by, as a container's member that is not found
/// always is (a plain slot keeps its bytes and plays nothing).
fn member_of(n: &os_flp::patcher::Node, effects: &[crate::plugins::Available]) -> Member {
    use crate::plugins::{recreation_of, recreation_state_from_fl, EDISON_FILE_NAME, EQ_FILE_NAME, LEMON_EQ, LOVE_PHILTER_FILE_NAME, MAXIMUS_FILE_NAME, PRESS, PULP_FILTER, TESLA};
    use crate::project_plugins::{find, find_native, native_first, switched_format};
    let (fl, file) = (n.state.as_slice(), n.plugin.as_str());
    let found = |i: usize, state: Vec<u8>| (effects[i].desc.uid(), effects[i].desc.name.clone(), state);
    let lemon = match file {
        EQ_FILE_NAME => Some(LEMON_EQ),
        LOVE_PHILTER_FILE_NAME => Some(PULP_FILTER),
        MAXIMUS_FILE_NAME => Some(PRESS),
        EDISON_FILE_NAME => Some(TESLA),
        _ => recreation_of(file),
    };
    let wrapped = (file == os_flp::wrapper::WRAPPER).then(|| os_flp::wrapper::decode(fl)).flatten();
    let (uid, name, state) = if let Some(i) = find_native(effects, file).filter(|_| native_first()) {
        found(i, fl.to_vec())
    } else if let Some((i, lemon)) = lemon.and_then(|l| Some((effects.iter().position(|a| a.desc.name == l)?, l))) {
        let state = match file {
            EQ_FILE_NAME => Some(lemon_parametric_eq_2::params::plugin_state_from_fl(fl)),
            LOVE_PHILTER_FILE_NAME => Some(pulp_filter::plugin_state_from_fl(fl)),
            MAXIMUS_FILE_NAME => Some(press::plugin_state_from_fl(fl)),
            EDISON_FILE_NAME => tesla::plugin_state_from_edison(fl),
            _ => recreation_state_from_fl(lemon, fl),
        };
        // No state: the effect's defaults, as for a slot.
        found(i, state.unwrap_or_default())
    } else if let Some(r) = wrapped {
        match find(effects, &r) {
            Some(i) => {
                let other = switched_format(&effects[i].desc, &r);
                found(i, if other.is_none() { r.state } else { Vec::new() })
            }
            None => {
                let format = os_host::Format::parse(&r.format).map_or_else(|| r.format.to_ascii_lowercase(), |f| f.as_str().to_string());
                (format!("{format}:{}", r.id), r.name, r.state)
            }
        }
    } else if let Some(i) = find_native(effects, file) {
        found(i, fl.to_vec())
    } else {
        // A native plugin's identity is its file id (`Descriptor::uid` of a native plugin).
        (format!("native:{file}"), if n.display.is_empty() { file.to_string() } else { n.display.clone() }, fl.to_vec())
    };
    // The node's own name in the map, when the user gave it one (spec: plugins/patcher/state.md §3).
    let own = n.name.trim();
    let label = (!own.is_empty() && own != name && own != n.plugin && !own.eq_ignore_ascii_case(&n.display)).then(|| own.to_string());
    Member { id: 1, instrument: false, uid, name: label.unwrap_or(name), wet: 1.0, dry: 0.0, on: true, state }
}

/// A Patcher slot whose patch is one plain serial chain of effects becomes a container in the same slot, its nodes
/// the members in chain order at wet 100 %, dry 0 % (every link level is 1.0), each with its plugin and state. The
/// slot keeps its id, switch, mix, name, colour and icon. Any other Patcher slot, and every Patcher channel (an
/// instrument patch), stays as the project had it. What was opened and what was not, and why, is returned for the
/// hint bar. The project is opened that way, not changed: a save writes the containers (decision #7).
// spec: plugins/patcher/state.md §1 to §7; plugins/patcher/README.md §3.1, §4 slice 5
pub fn open_patchers(s: &mut State, effects: &[crate::plugins::Available]) -> Option<String> {
    use os_flp::patcher::{self, NotChain, PATCHER};
    let mut opened = Vec::new();
    let mut left = Vec::new();
    let places: Vec<(u16, u8)> = s
        .project
        .mixer
        .iter()
        .flat_map(|m| m.slots.iter().enumerate().filter(|(_, p)| p.as_ref().is_some_and(|p| p.name == PATCHER)).map(move |(k, _)| (m.index, k as u8)))
        .collect();
    for (t, k) in places {
        let place = format!("{}, slot {}", crate::mixer::name(s, t), k + 1);
        let Some(slot) = crate::mixer::track_mut(s, t).and_then(|m| m.slots[usize::from(k)].as_mut()) else { continue };
        let Some(patch) = patcher::parse(&slot.state) else {
            left.push(format!("{place} (a state Lemon Studio does not read)"));
            continue;
        };
        let order = match patch.serial_chain() {
            Ok(order) if order.len() <= MAX_MEMBERS => order,
            Ok(_) => {
                left.push(format!("{place} (more than {MAX_MEMBERS} effects)"));
                continue;
            }
            Err(why) => {
                left.push(format!("{place} ({})", why.text()));
                continue;
            }
        };
        let list: Vec<Member> = order.iter().zip(1..).map(|(&i, id)| Member { id, ..member_of(&patch.nodes[i], effects) }).collect();
        slot.name = CONTAINER.into();
        slot.state = encode(&list);
        opened.push(place);
    }
    for (r, ch) in s.project.channels.iter().enumerate() {
        if ch.plugin.as_deref() == Some(PATCHER) {
            let name = ch.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| format!("channel {}", r + 1));
            left.push(format!("{name} ({})", NotChain::Instrument.text()));
        }
    }
    let mut parts = Vec::new();
    if !opened.is_empty() {
        parts.push(format!("{} opened as {}", opened.join(", "), if opened.len() == 1 { "a container" } else { "containers" }));
    }
    if !left.is_empty() {
        parts.push(format!("{}: {}", if left.len() == 1 { "left as it was" } else { "left as they were" }, left.join(", ")));
    }
    (!parts.is_empty()).then(|| format!("Patcher: {}", parts.join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::members;
    use os_model::{Id, SlotPlugin};

    fn chunk(id: u32, body: &[u8]) -> Vec<u8> {
        let mut b = id.to_le_bytes().to_vec();
        b.extend((body.len() as u64).to_le_bytes());
        b.extend_from_slice(body);
        b
    }

    fn words(w: &[u32]) -> Vec<u8> {
        w.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    fn utf16(s: &str) -> Vec<u8> {
        s.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect()
    }

    /// A serial Patcher stream: `nodes` as (plugin, display, state), linked Audio input -> each node -> Audio output.
    fn patcher_state(nodes: &[(&str, &str, &[u8])]) -> Vec<u8> {
        let mut b = 4u32.to_le_bytes().to_vec();
        for (i, &(plugin, name, state)) in nodes.iter().enumerate() {
            let id = 1004 + i as u32 * 3;
            let mut g = chunk(5000, &words(&[2, id, 0, 0, 0]));
            g.extend(chunk(5004, &utf16(plugin)));
            g.extend(chunk(5008, &utf16(plugin)));
            g.extend(chunk(5010, &utf16(name)));
            g.extend(chunk(5005, state));
            g.extend(chunk(5009, &words(&[0, 0, id + 1, 0, os_flp::patcher::AUDIO_IN, 0, 0])));
            g.extend(chunk(5009, &words(&[0, 0, id + 2, 0, os_flp::patcher::AUDIO_OUT, 0, 0])));
            g.extend(chunk(5001, &[]));
            b.extend(chunk(0, &g));
        }
        for (id, name) in [(1000, "Audio input"), (1001, "Audio output")] {
            b.extend(chunk(0, &[chunk(1400, &words(&[id, 0, 0, 7, 2, 0, 0])), chunk(1402, &utf16(name)), chunk(1401, &[])].concat()));
        }
        let input = |k: u32| 1005 + 3 * k;
        let output = |k: u32| 1006 + 3 * k;
        let mut links = vec![(if nodes.is_empty() { 1001 } else { input(0) }, 1000)];
        links.extend((1..nodes.len() as u32).map(|k| (input(k), output(k - 1))));
        if !nodes.is_empty() {
            links.push((1001, output(nodes.len() as u32 - 1)));
        }
        for &(to, from) in &links {
            let d = [words(&[to, 0, from, 0]), 1.0f32.to_le_bytes().to_vec(), words(&[2, 0, 0])].concat();
            b.extend(chunk(0, &[chunk(1300, &d), chunk(1301, &[])].concat()));
        }
        b.extend(chunk(1001, &[]));
        b
    }

    /// A serial patch in a mixer slot becomes a container with its effects in chain order; a patch with link levels and
    /// a Patcher channel stay as they were, and the hint names each.
    #[test]
    fn the_originals_serial_patches_open_as_containers() {
        crate::project_plugins::NATIVE_FIRST.with(|n| n.set(Some(false)));
        let mut a = crate::app::App::new(None);
        a.state.project = crate::app::test_project();
        a.reset_hosted();
        a.plugins.effects = crate::plugins::builtin_effects();
        let effect = |a: &crate::app::App, n: &str| a.plugins.effects.iter().position(|e| e.desc.name == n).unwrap_or_else(|| panic!("{n}"));
        let lim = effect(&a, "Lemon Limiter");
        let delay = effect(&a, "Lemon Delay");
        let serial = patcher_state(&[("Fruity Limiter", "Compressor", &[7; 169]), (os_flp::wrapper::WRAPPER, "Echo", &[]), ("Fruity Delay 3", "Delay", &[9; 4])]);
        let patcher = |state: Vec<u8>| Some(SlotPlugin { id: Id::NONE, name: os_flp::patcher::PATCHER.into(), state });
        let t = crate::mixer::track_mut(&mut a.state, 2).unwrap();
        t.slots[0] = patcher(serial);
        a.state.project.channels[0].plugin = Some(os_flp::patcher::PATCHER.into());
        a.state.project.channels[0].name = Some("Keys".into());
        let msg = crate::patcher_import::open_patchers(&mut a.state, &a.plugins.effects).expect("a hint");
        let bus = crate::mixer::name(&a.state, 2);
        assert!(msg.starts_with(&format!("Patcher: {bus}, slot 1 opened as a container")), "{msg}");
        assert!(msg.ends_with("Keys (an instrument patch)"), "{msg}");
        let list = members(&a.state, 2, 0).expect("a container");
        assert_eq!(list.len(), 3, "three serial nodes become three members");
        assert_eq!(list[0].uid, a.plugins.effects[lim].desc.uid(), "the recreation is found");
        assert_eq!(list[1].name, "Echo", "the wrapped node keeps its name");
        assert_eq!(list[2].uid, a.plugins.effects[delay].desc.uid(), "the delay recreation is found");
        assert_eq!(a.state.project.channels[0].plugin.as_deref(), Some(os_flp::patcher::PATCHER));
        crate::project_plugins::NATIVE_FIRST.with(|n| n.set(None));
    }

    /// Opening a project goes through the same import: a saved mixer slot whose plugin is the original's Patcher and
    /// whose patch is a serial chain is a container in the opened project, before the first audio tick. This is the
    /// app-level wiring, not just the converter.
    #[test]
    fn opening_a_project_imports_its_serial_patcher_slots() {
        crate::project_plugins::NATIVE_FIRST.with(|n| n.set(Some(false)));
        let serial = patcher_state(&[("Fruity Limiter", "Compressor", &[7; 169]), ("Fruity Delay 3", "Delay", &[9; 4])]);
        let mut p = crate::app::test_project();
        p.arrangements.push(os_flp::project::default_arrangement());
        let t = p.mixer.iter_mut().find(|m| m.index == 2).unwrap();
        t.slots[0] = Some(SlotPlugin { id: Id::NONE, name: os_flp::patcher::PATCHER.into(), state: serial });
        let path = std::env::temp_dir().join(format!("lemon-patcher-open-{}.flp", std::process::id()));
        std::fs::write(&path, os_flp::project::create(&p).unwrap().to_bytes().unwrap()).unwrap();
        let mut a = crate::app::App::new(None);
        a.plugins.effects = crate::plugins::builtin_effects();
        a.open(path.clone());
        let _ = std::fs::remove_file(&path);
        assert!(a.state.message.as_deref().is_some_and(|m| m.contains("Patcher: Bus 2, slot 1 opened as a container")), "{:?}", a.state.message);
        let list = members(&a.state, 2, 0).expect("the opened slot is a container");
        assert_eq!(list.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["Compressor", "Delay"]);
        crate::project_plugins::NATIVE_FIRST.with(|n| n.set(None));
    }
}
