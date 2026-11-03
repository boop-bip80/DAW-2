// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! One inventory and path rewriter for the external audio files understood by Lemon Studio. Unknown plugin
//! dependencies are explicit: a successful walk never implies that an opaque third-party state is self-contained.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use os_model::{ChannelKind, Project};

use crate::app::App;

#[derive(Debug, Default)]
pub struct Inventory {
    /// Rack row or effect hosting key, and the stored path. Repeated references are retained for missing-file UI.
    pub references: Vec<(usize, String)>,
    /// Plugins for which the host cannot enumerate external dependencies.
    pub opaque: BTreeSet<String>,
}

pub type Paths = BTreeMap<String, String>;

/// The hosted-plugin JSON encoding has two valid framings: bare UTF-8 JSON and a u64 little-endian byte count
/// followed by JSON. The first byte of a valid length prefix can itself be `{` when payload length is 123 modulo
/// 256; checking a magic first character silently corrupts such states. Check the whole prefix instead.
pub(crate) fn persisted_json(blob: &[u8]) -> (&[u8], bool) {
    if let Some(header) = blob.get(..8) {
        let len = u64::from_le_bytes(header.try_into().expect("eight header bytes"));
        if len == (blob.len() - 8) as u64 {
            return (&blob[8..], true);
        }
    }
    (blob, false)
}

/// Capture current built-in and hosted state without changing the live model. The same checked snapshots used
/// by Save feed collection and cleanup, including edits that have not yet reached the model.
pub fn snapshot(app: &mut App) -> Result<Project, String> {
    let before = app.state.project.clone();
    let result = app.try_store_builtin_states().and_then(|()| {
        let mut project = app.state.project.clone();
        for (row, ch) in project.channels.iter_mut().enumerate() {
            if !ch.kind.is_sampler() && !crate::osc3::is_osc3(ch) {
                continue;
            }
            if let Some(path) = app.plugins.sampler_path(row) {
                if ch.sample_path.as_deref().is_none_or(|old| crate::plugins::resolve_sample(old) != path) {
                    ch.sample_path = (!path.is_empty()).then_some(path);
                }
            }
        }
        crate::project_plugins::store_hosted(&mut project, &app.plugins)?;
        Ok(project)
    });
    app.state.project = before;
    result
}

pub fn inspect(project: &Project) -> Result<Inventory, String> {
    let mut inventory = Inventory::default();
    walk(&mut project.clone(), &mut |owner, path| {
        inventory.references.push((owner, path.to_string()));
        None
    }, &mut inventory.opaque)?;
    Ok(inventory)
}

/// Rewrite a clone, committing nothing when a supported state is malformed.
pub fn rewritten(project: &Project, paths: &Paths) -> Result<Project, String> {
    let mut candidate = project.clone();
    walk(&mut candidate, &mut |_, old| replacement(paths, old), &mut BTreeSet::new())?;
    Ok(candidate)
}

fn replacement(paths: &Paths, old: &str) -> Option<String> {
    paths.get(old).or_else(|| paths.get(&crate::plugins::resolve_sample(old))).cloned()
}

/// Register the stored spelling and its resolved source before moving files or changing search folders.
pub fn map_path(paths: &mut Paths, old: &str, new: String) {
    paths.insert(crate::plugins::resolve_sample(old), new.clone());
    paths.insert(old.to_string(), new);
}

type Visit<'a> = dyn FnMut(usize, &str) -> Option<String> + 'a;

fn visit_path(owner: usize, path: &mut String, visit: &mut Visit<'_>) {
    if !path.trim().is_empty() {
        if let Some(to) = visit(owner, path) {
            *path = to;
        }
    }
}

fn walk(project: &mut Project, visit: &mut Visit<'_>, opaque: &mut BTreeSet<String>) -> Result<(), String> {
    for (row, ch) in project.channels.iter_mut().enumerate() {
        if let Some(path) = &mut ch.sample_path {
            visit_path(row, path, visit);
        }
        if ch.kind == ChannelKind::Plugin {
            if let (Some(name), Some(state)) = (&ch.plugin, &mut ch.plugin_state) {
                file_state(row, name, state, visit, opaque, 0)?;
            }
        }
    }
    for track in &mut project.mixer {
        for (slot, plugin) in track.slots.iter_mut().enumerate() {
            if let Some(plugin) = plugin {
                file_state(crate::plugins::fx_key(track.index, slot as u8), &plugin.name, &mut plugin.state, visit, opaque, 0)?;
            }
        }
    }
    Ok(())
}

fn file_state(owner: usize, name: &str, state: &mut Vec<u8>, visit: &mut Visit<'_>, opaque: &mut BTreeSet<String>, depth: usize) -> Result<(), String> {
    if depth > 32 {
        return Err("asset inventory: container nesting exceeds 32 levels".into());
    }
    if crate::drums::is_file_name(name) {
        if state.is_empty() { return Ok(()) }
        drum_paths(owner, state, None, visit)?;
    } else if crate::slicer::is_file_name(name) {
        if state.is_empty() { return Ok(()) }
        let mut slicer = lemon_slicer::state::State::parse(state).ok_or("asset inventory: invalid Fruity Slicer state")?;
        let mut path = slicer.path();
        visit_path(owner, &mut path, visit);
        if path != slicer.path() {
            slicer.set_path(&path);
            *state = slicer.to_bytes();
        }
    } else if name == crate::container::CONTAINER {
        if state.is_empty() { return Ok(()) }
        let mut members = crate::container::decode(state).ok_or("asset inventory: invalid container state")?;
        let before = members.clone();
        for member in &mut members {
            if member.name == crate::container::CONTAINER {
                file_state(owner, &member.name, &mut member.state, visit, opaque, depth + 1)?;
            } else if member.uid.starts_with("clap:") && builtin_name(&member.name) {
                builtin_state(owner, &member.name, &mut member.state, visit)?;
            } else {
                opaque.insert(member.name.clone());
            }
        }
        if members != before { *state = crate::container::encode(&members); }
    } else if name == os_flp::wrapper::WRAPPER {
        let mut plugin = os_flp::wrapper::decode(state).ok_or("asset inventory: invalid wrapped plugin state")?;
        if plugin.format == "clap" && builtin_name(&plugin.name) {
            let before = plugin.state.clone();
            builtin_state(owner, &plugin.name, &mut plugin.state, visit)?;
            if plugin.state != before { *state = os_flp::wrapper::encode(Some(state), &plugin); }
        } else {
            opaque.insert(plugin.name);
        }
    } else if builtin_name(name) {
        builtin_state(owner, name, state, visit)?;
    } else if !matches!(name, crate::osc3::FILE_NAME | crate::citrus::FILE_NAME | crate::lemon_midi_out::FILE_NAME
        | crate::plugins::EQ_FILE_NAME | crate::plugins::EDISON_FILE_NAME | crate::plugins::LOVE_PHILTER_FILE_NAME
        | crate::plugins::MAXIMUS_FILE_NAME | "Fruity Stereo Shaper") && crate::plugins::recreation_of(name).is_none() {
        opaque.insert(name.to_string());
    }
    Ok(())
}

fn builtin_name(name: &str) -> bool {
    matches!(name, crate::plugins::LEMON_SAMPLER | crate::osc3::LEMON_TRIO | crate::citrus::CITRUS
        | crate::slicer::LEMON_SLICER | crate::drums::LEMON_DRUMS | crate::plugins::LEMON_EQ
        | crate::plugins::PULP_FILTER | crate::plugins::PRESS)
        || crate::plugins::LINKED_EFFECTS.contains(&name)
}

fn file_backed_builtin(name: &str) -> bool {
    matches!(name, crate::plugins::LEMON_SAMPLER | crate::osc3::LEMON_TRIO | crate::slicer::LEMON_SLICER | crate::drums::LEMON_DRUMS)
}

/// Rewrite only path leaves of the FPC tree: velocity layers, pad settings and unknown chunks stay intact.
fn drum_paths(owner: usize, state: &mut Vec<u8>, mut resolved: Option<&mut Vec<String>>, visit: &mut Visit<'_>) -> Result<(), String> {
    use lemon_drum_sampler::state::{State, Node, PAD, P_LAYER, L_PATH};
    let mut drums = State::parse(state).ok_or("asset inventory: invalid FPC state")?;
    let mut index = 0;
    let mut changed = false;
    for pad in &mut drums.nodes {
        if matches!(pad, Node::Leaf(PAD, _)) { return Err("asset inventory: unreadable FPC pad".into()) }
        let Node::Tree(PAD, nodes) = pad else { continue };
        for layer in nodes {
            if matches!(layer, Node::Leaf(P_LAYER, _)) { return Err("asset inventory: unreadable FPC layer".into()) }
            let Node::Tree(P_LAYER, nodes) = layer else { continue };
            for node in nodes {
                let Node::Leaf(L_PATH, bytes) = node else { continue };
                let mut path = lemon_drum_sampler::state::text(bytes);
                let before = path.clone();
                visit_path(owner, &mut path, visit);
                if path != before {
                    *bytes = path.encode_utf16().flat_map(u16::to_le_bytes).collect();
                    changed = true;
                }
            }
            if let Some(path) = resolved.as_mut().and_then(|p| p.get_mut(index)) {
                visit_path(owner, path, visit);
            }
            index += 1;
        }
    }
    if changed { *state = drums.to_bytes(); }
    Ok(())
}

/// File-backed plugin overrides are JSON-encoded *strings* inside the CLAP state JSON. Missing optional overrides
/// can be reconstructed from their raw sampler state, but a present malformed override must never be mistaken for
/// an empty dependency list during project cleanup or portable export.
fn stored_override<T: serde::de::DeserializeOwned>(fields: &serde_json::Map<String, serde_json::Value>, key: &str, name: &str) -> Result<Option<T>, String> {
    let Some(value) = fields.get(key) else { return Ok(None) };
    let text = value.as_str().ok_or_else(|| format!("asset inventory: malformed {name} {key} override (not a string)"))?;
    serde_json::from_str(text).map(Some).map_err(|e| format!("asset inventory: malformed {name} {key} override: {e}"))
}

fn builtin_state(owner: usize, name: &str, state: &mut Vec<u8>, visit: &mut Visit<'_>) -> Result<(), String> {
    if !file_backed_builtin(name) || state.is_empty() { return Ok(()) }
    let (body, prefixed) = persisted_json(state);
    let mut json: serde_json::Value = serde_json::from_slice(body)
        .map_err(|e| format!("asset inventory: invalid {name} state: {e}"))?;
    let before = json.clone();
    let fields = json.get_mut("fields").and_then(|f| f.as_object_mut()).ok_or_else(|| format!("asset inventory: {name} state has no fields"))?;
    if name == crate::drums::LEMON_DRUMS {
        let mut raw = lemon_drum_sampler::params::fl_from_plugin_state(state).ok_or("asset inventory: invalid drum state")?;
        let mut paths: Vec<String> = stored_override(fields, "sample-paths", name)?.unwrap_or_default();
        let (old_raw, old_paths) = (raw.clone(), paths.clone());
        drum_paths(owner, &mut raw, Some(&mut paths), visit)?;
        if raw != old_raw || paths != old_paths {
            fields.insert("state".into(), serde_json::Value::String(format!("\"{}\"", hex(&raw))));
            fields.insert("sample-paths".into(), serde_json::Value::String(serde_json::to_string(&paths).map_err(|e| e.to_string())?));
        }
    } else if name == crate::slicer::LEMON_SLICER {
        let raw = lemon_slicer::params::fl_from_plugin_state(state).ok_or("asset inventory: invalid slicer state")?;
        let mut slicer = lemon_slicer::state::State::parse(&raw).ok_or("asset inventory: invalid slicer state")?;
        let mut stored = slicer.path();
        let old_stored = stored.clone();
        visit_path(owner, &mut stored, visit);
        if stored != old_stored {
            slicer.set_path(&stored);
            fields.insert("state".into(), serde_json::Value::String(format!("\"{}\"", hex(&slicer.to_bytes()))));
        }
        let mut path: String = stored_override(fields, "sample-path", name)?.unwrap_or_default();
        let old = path.clone();
        visit_path(owner, &mut path, visit);
        if path != old {
            fields.insert("sample-path".into(), serde_json::Value::String(serde_json::to_string(&path).map_err(|e| e.to_string())?));
        }
    } else if let Some(path) = stored_override::<String>(fields, "sample-path", name)? {
        let mut to = path.clone();
        visit_path(owner, &mut to, visit);
        if to != path {
            fields.insert("sample-path".into(), serde_json::Value::String(serde_json::to_string(&to).map_err(|e| e.to_string())?));
        }
    }
    if json != before {
        let bytes = serde_json::to_vec(&json).map_err(|e| e.to_string())?;
        *state = if prefixed { (bytes.len() as u64).to_le_bytes().to_vec() } else { Vec::new() };
        state.extend(bytes);
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Resolve a supported built-in's runtime file fields when it is instantiated from a portable project/container.
/// Its raw FPC/Slicer state keeps the portable stored paths; only the runtime overrides become absolute.
pub fn for_host(name: &str, state: &[u8]) -> Result<Vec<u8>, String> {
    for_host_in(name, state, None)
}

fn for_host_in(name: &str, state: &[u8], base: Option<&Path>) -> Result<Vec<u8>, String> {
    if !file_backed_builtin(name) || state.is_empty() { return Ok(state.to_vec()) }
    let (body, prefixed) = persisted_json(state);
    let mut json: serde_json::Value = serde_json::from_slice(body).map_err(|e| format!("{name}: {e}"))?;
    let fields = json.get_mut("fields").and_then(|v| v.as_object_mut()).ok_or_else(|| format!("{name}: missing persisted fields"))?;
    let resolve = |path: &str| -> String {
        if path.is_empty() { return String::new() }
        if let Some(base) = base.filter(|_| !Path::new(path).is_absolute()) {
            let local = base.join(path.replace('\\', std::path::MAIN_SEPARATOR_STR));
            if local.is_file() { return local.to_string_lossy().into_owned() }
        }
        crate::plugins::resolve_sample(path)
    };
    if name == crate::drums::LEMON_DRUMS {
        let _: Option<Vec<String>> = stored_override(fields, "sample-paths", name)?;
        let raw = lemon_drum_sampler::params::fl_from_plugin_state(state).ok_or("invalid drum sample state")?;
        let paths: Vec<String> = lemon_drum_sampler::params::stored_paths(&raw).iter().map(|p| resolve(p)).collect();
        fields.insert("sample-paths".into(), serde_json::Value::String(serde_json::to_string(&paths).map_err(|e| e.to_string())?));
    } else {
        let mut path: String = stored_override(fields, "sample-path", name)?.unwrap_or_default();
        if name == crate::slicer::LEMON_SLICER {
            let raw = lemon_slicer::params::fl_from_plugin_state(state).ok_or("invalid slicer sample state")?;
            let stored = lemon_slicer::state::State::parse(&raw).ok_or("invalid slicer sample state")?.path();
            if !stored.is_empty() { path = stored; }
        }
        fields.insert("sample-path".into(), serde_json::Value::String(serde_json::to_string(&resolve(&path)).map_err(|e| e.to_string())?));
    }
    let bytes = serde_json::to_vec(&json).map_err(|e| e.to_string())?;
    let mut out = if prefixed { (bytes.len() as u64).to_le_bytes().to_vec() } else { Vec::new() };
    out.extend(bytes);
    Ok(out)
}

/// Receipt for a reversible asset relink. Copied files are intentionally kept if the later save fails.
pub struct Change {
    before: Project,
    states: Vec<(usize, Vec<u8>)>,
    pub files: usize,
    pub opaque: BTreeSet<String>,
}

impl Change {
    pub fn rollback(self, app: &mut App) -> Result<(), String> {
        let mut failed = Vec::new();
        for (key, state) in self.states.iter().rev() {
            if !app.plugins.set_state(*key, state) { failed.push(*key); }
        }
        app.state.project = self.before;
        app.state.piano.dirty = true;
        crate::missing::apply(app);
        if failed.is_empty() { Ok(()) } else { Err(format!("plugins {failed:?} refused the previous sample paths; the original project was retained")) }
    }
}

/// A collected project can still depend on files managed privately by a third-party plugin.
pub fn collection_notice(app: &mut App, change: Option<&Change>) {
    if let Some(change) = change.filter(|c| !c.opaque.is_empty()) {
        let current = app.state.message.take().unwrap_or_default();
        app.state.message = Some(format!("{current}; plugin-managed files of {} still require those plugins' collection commands", change.opaque.iter().cloned().collect::<Vec<_>>().join(", ")));
    }
}

/// Relink supported live plugins and the model together. If a live instance refuses its candidate, restore every
/// instance already changed and retain the model. The receipt can also roll back a subsequent save failure.
pub fn apply(app: &mut App, candidate: Project, paths: &Paths, base: Option<&Path>) -> Result<Change, String> {
    let mut edits = Vec::new();
    for (&key, _) in &app.plugins.hosted {
        let Some(desc) = app.plugins.descriptor(key) else { continue };
        if !crate::project_plugins::is_builtin(&desc) || !file_backed_builtin(&desc.name) { continue }
        let before = app.plugins.state(key).ok_or_else(|| format!("{} could not save its state for relinking", desc.name))?;
        let mut after = before.clone();
        builtin_state(key, &desc.name, &mut after, &mut |_, old| replacement(paths, old))?;
        if before != after {
            after = for_host_in(&desc.name, &after, base)?;
            edits.push((key, before, after));
        }
    }
    let mut change = Change { before: app.state.project.clone(), states: Vec::new(), files: 0, opaque: BTreeSet::new() };
    for (key, before, after) in edits {
        // A rejected load can have partially changed a plugin; include it in the rollback as well.
        change.states.push((key, before));
        if !app.plugins.set_state(key, &after) {
            let reason = format!("plugin {key} refused the collected sample paths");
            return match change.rollback(app) {
                Ok(()) => Err(reason),
                Err(e) => Err(format!("{reason}; {e}")),
            };
        }
    }
    app.state.project = candidate;
    app.state.piano.dirty = true;
    Ok(change)
}

/// Exact bounded-memory comparison. Equal length is only an early rejection, never evidence of identity.
pub fn same_content(a: &Path, b: &Path) -> io::Result<bool> {
    let mut a = File::open(a)?;
    let mut b = File::open(b)?;
    let ma = a.metadata()?;
    let mb = b.metadata()?;
    if !ma.is_file() || !mb.is_file() || ma.len() != mb.len() { return Ok(false) }
    let (mut left, mut right) = ([0u8; 64 * 1024], [0u8; 64 * 1024]);
    let mut remaining = ma.len();
    while remaining > 0 {
        let n = remaining.min(left.len() as u64) as usize;
        a.read_exact(&mut left[..n])?;
        b.read_exact(&mut right[..n])?;
        if left[..n] != right[..n] { return Ok(false) }
        remaining -= n as u64;
    }
    Ok(a.read(&mut left[..1])? == 0 && b.read(&mut right[..1])? == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drum_layers() -> Vec<u8> {
        use lemon_drum_sampler::state::{new_layer, Node, State, PAD};
        let mut state = State::default();
        state.set_sample(1, "drums/kick.wav");
        let pad = state.nodes.iter_mut().filter(|n| matches!(n, Node::Tree(PAD, _))).nth(1).unwrap();
        let Node::Tree(_, children) = pad else { unreachable!() };
        children.push(new_layer("drums/quiet-kick.wav"));
        state.to_bytes()
    }

    fn channel(name: &str, state: Vec<u8>) -> Project {
        let mut project = crate::app::test_project();
        let row = crate::app::add_plugin_channel(&mut project, name);
        project.channels[row].plugin_state = Some(state);
        project
    }

    #[test]
    fn inventory_and_rewriter_visit_every_fpc_layer() {
        let project = channel(crate::drums::LEMON_DRUMS, drum_layers());
        let mut project = project;
        let row = project.channels.len() - 1;
        project.channels[row].plugin = Some(crate::drums::FILE_NAME.into());
        let before = inspect(&project).unwrap();
        assert_eq!(before.references.iter().map(|(_, p)| p.as_str()).collect::<Vec<_>>(), ["drums/kick.wav", "drums/quiet-kick.wav"]);
        let mut paths = Paths::new();
        map_path(&mut paths, "drums/kick.wav", "Samples/kick.wav".into());
        map_path(&mut paths, "drums/quiet-kick.wav", "Samples/quiet-kick.wav".into());
        let candidate = rewritten(&project, &paths).unwrap();
        assert_eq!(inspect(&candidate).unwrap().references.iter().map(|(_, p)| p.as_str()).collect::<Vec<_>>(),
            ["Samples/kick.wav", "Samples/quiet-kick.wav"]);
        assert_eq!(inspect(&project).unwrap().references, before.references, "the original remains untouched");
    }

    #[test]
    fn inventory_descends_into_slicer_wrapper_and_nested_container() {
        let mut slice = lemon_slicer::state::State::default();
        slice.set_path("loops/melody.wav");
        let slicer_state = crate::slicer::plugin_state(&slice.to_bytes());
        let wrapped = os_model::PluginRef {
            format: "clap".into(), name: crate::slicer::LEMON_SLICER.into(),
            path: "Lemon Slicer.clap".into(), state: slicer_state.clone(), ..Default::default()
        };
        let project = channel(os_flp::wrapper::WRAPPER, os_flp::wrapper::encode(None, &wrapped));
        assert!(inspect(&project).unwrap().references.iter().any(|(_, path)| path == "loops/melody.wav"));

        let slicer_member = crate::container::Member {
            id: 1, instrument: true, uid: "clap:lemon-slicer".into(),
            name: crate::slicer::LEMON_SLICER.into(), wet: 1.0, dry: 0.0, state: slicer_state,
        };
        let nested = crate::container::Member {
            id: 2, instrument: true, uid: "clap:lemon-container".into(),
            name: crate::container::CONTAINER.into(), wet: 1.0, dry: 0.0,
            state: crate::container::encode(&[slicer_member]),
        };
        let project = channel(crate::container::CONTAINER, crate::container::encode(&[nested]));
        let inventory = inspect(&project).unwrap();
        assert!(inventory.references.iter().any(|(_, path)| path == "loops/melody.wav"));
        assert!(inventory.opaque.is_empty(), "a supported nested built-in is fully enumerable");
    }

    #[test]
    fn malformed_builtin_sample_overrides_fail_closed() {
        let valid = crate::drums::plugin_state(&drum_layers());
        let offset = if valid.first() == Some(&b'{') { 0 } else { 8 };
        let mut json: serde_json::Value = serde_json::from_slice(&valid[offset..]).unwrap();
        json["fields"]["sample-paths"] = serde_json::Value::String("not JSON".into());
        let bytes = serde_json::to_vec(&json).unwrap();
        let mut broken = if offset == 8 { (bytes.len() as u64).to_le_bytes().to_vec() } else { Vec::new() };
        broken.extend(bytes);
        let mut project = channel(crate::drums::LEMON_DRUMS, broken.clone());
        let row = project.channels.len() - 1;
        project.channels[row].plugin = Some(os_flp::wrapper::WRAPPER.into());
        let wrapper = os_model::PluginRef {
            format: "clap".into(), name: crate::drums::LEMON_DRUMS.into(),
            path: "Lemon Drums.clap".into(), state: broken.clone(), ..Default::default()
        };
        project.channels[row].plugin_state = Some(os_flp::wrapper::encode(None, &wrapper));
        assert!(inspect(&project).is_err(), "bad overrides cannot become an empty dependency list");
        assert!(for_host(crate::drums::LEMON_DRUMS, &broken).is_err(), "runtime must not silently sanitize corrupt persisted state");

        let sampler = serde_json::json!({"fields": {"sample-path": "not a JSON string"}});
        let bytes = serde_json::to_vec(&sampler).unwrap();
        assert!(for_host(crate::plugins::LEMON_SAMPLER, &bytes).is_err());
    }

    #[test]
    fn length_prefixed_json_with_open_brace_as_first_header_byte_is_not_bare_json() {
        let sample = serde_json::to_string("Samples/kick.wav").unwrap();
        let mut body = serde_json::to_vec(&serde_json::json!({"fields":{"sample-path":sample},"params":{}})).unwrap();
        while body.len() % 256 != usize::from(b'{') { body.push(b' '); }
        let mut blob = (body.len() as u64).to_le_bytes().to_vec();
        blob.extend_from_slice(&body);
        assert_eq!(blob[0], b'{', "a valid header can look exactly like the opening of bare JSON");
        let (decoded, framed) = persisted_json(&blob);
        assert!(framed);
        assert_eq!(decoded, body);
        assert_eq!(crate::plugins::sample_path_of(&blob).as_deref(), Some("Samples/kick.wav"));

        let runtime = for_host(crate::plugins::LEMON_SAMPLER, &blob).unwrap();
        let expected = crate::plugins::resolve_sample("Samples/kick.wav");
        assert_eq!(crate::plugins::sample_path_of(&runtime).as_deref(), Some(expected.as_str()));
        let changed = crate::plugins::with_sample_path(&blob, "Samples/hat.wav").unwrap();
        assert!(persisted_json(&changed).1, "framing survives a state edit");
        assert_eq!(crate::plugins::sample_path_of(&changed).as_deref(), Some("Samples/hat.wav"));
        let changed = crate::plugins::with_params(&blob, &[("test", serde_json::json!({"f32":0.75}))]).unwrap();
        let v: serde_json::Value = serde_json::from_slice(persisted_json(&changed).0).unwrap();
        assert_eq!(v["params"]["test"]["f32"], 0.75);

        let mut rewritten = blob.clone();
        builtin_state(0, crate::plugins::LEMON_SAMPLER, &mut rewritten, &mut |_, old| {
            assert_eq!(old, "Samples/kick.wav");
            Some("Samples/renamed.wav".to_string())
        }).unwrap();
        assert_eq!(crate::plugins::sample_path_of(&rewritten).as_deref(), Some("Samples/renamed.wav"));
        assert!(persisted_json(&rewritten).1);
    }
}
