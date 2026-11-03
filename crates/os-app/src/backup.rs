// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Project backups (settings window, File ▸ Backup). While a project is open the interval of the Autosave choice runs;
//! when it ends and the project changed since the last backup or save, the project is written to
//! `<user folder>/Projects/Backup` with a persistent project identity and a unique generation in each name. Only
//! autosaves belonging to that identity are pruned beyond Maximum. Saving over a file first preserves the old file
//! in a separate durable generation. With Location's "project data folder" (the default), a project with a
//! data folder backs up to `<data folder>/Backup` instead (`crate::data_folder::backup_folder`). Not yet: Never's
//! reminder and the backups "before risky operations".
// spec: ui/settings-choices.md "Backup files", ui/data-folder.md §3

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{Datelike, NaiveDateTime, Timelike};
use os_model::Project; // This code is a part of L e m o n S t u d i o.

use crate::app::App;
use crate::prefs;

/// The Autosave timer and what the last backup wrote.
#[derive(Debug, Default)]
pub struct Backup {
    /// The running interval (None: Never) and when it ends.
    every: Option<Duration>,
    due: Option<Instant>,
    /// Model, tempo, native metadata and plugin edit revision at the last successful backup.
    last: Option<(Project, f64, Vec<u8>, u64)>,
}

/// The interval of Autosave entry `i`: Never none, Rarely 15, Occasionally 10, Regularly 5, Frequently 5, Very
/// frequently 1 minute.
pub fn interval(i: usize) -> Option<Duration> {
    const MINUTES: [u64; 6] = [0, 15, 10, 5, 5, 1];
    MINUTES.get(i).copied().filter(|&m| m > 0).map(|m| Duration::from_secs(m * 60))
}

/// The global backup folder: `<user folder>/Projects/Backup` (a project with a data folder may back up there instead:
/// `crate::data_folder::backup_folder`). Tests: the folder their thread set in `TEST_DIR`, else none (nothing is
/// written).
pub fn folder() -> Option<PathBuf> {
    #[cfg(test)]
    return TEST_DIR.with(|d| d.borrow().clone());
    #[cfg(not(test))]
    os_paths::user_dir().map(|d| d.join("Projects").join("Backup"))
}

#[cfg(test)]
thread_local! {
    pub static TEST_DIR: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// FL's time stamp in backup names: `M-D-YYYY at HhMM`, 24-hour clock, no leading zeros but the minutes'.
pub fn stamp(t: NaiveDateTime) -> String {
    format!("{}-{}-{} at {}h{:02}", t.month(), t.day(), t.year(), t.hour(), t.minute())
}

/// A backup's file name: `<name> (<what> on <stamp>).lemon`; `what` is "autosaved" or "overwritten" (FL's names,
/// Lemon Studio's file: spec flp/lemon-format.md "Backups").
pub fn file_name(name: &str, what: &str, t: NaiveDateTime) -> String {
    format!("{name} ({what} on {}).{}", stamp(t), crate::project_files::EXT)
}

/// The name backups give the project: its file's, or `untitled` for a project never saved.
pub fn project_name(path: Option<&Path>) -> String {
    path.and_then(Path::file_stem).map_or_else(|| "untitled".to_string(), |n| n.to_string_lossy().into_owned())
}

/// A non-secret, random project identity. New projects and Save As get distinct recovery lineages.
pub fn new_identity() -> [u8; 16] {
    use std::hash::{BuildHasher, Hasher};
    let random = std::collections::hash_map::RandomState::new();
    let mut a = random.build_hasher();
    a.write_u128(generation());
    a.write_u32(std::process::id());
    let mut b = random.build_hasher();
    b.write_u64(a.finish());
    b.write_u8(1);
    let mut id = [0; 16];
    id[..8].copy_from_slice(&a.finish().to_le_bytes());
    id[8..].copy_from_slice(&b.finish().to_le_bytes());
    id
}

fn identity_text(id: [u8; 16]) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}

fn generation() -> u128 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    (nanos << 32) | u128::from(NEXT.fetch_add(1, Ordering::Relaxed) as u32)
}

fn generation_name(name: &str, what: &str, t: NaiveDateTime, id: [u8; 16], ext: &str) -> String {
    format!("{name} ({what} on {}) [{}-{:032x}].{ext}", stamp(t), identity_text(id), generation())
}

/// The identity and generation suffix on a current backup. Legacy names deliberately have no identity.
fn suffix(file: &str) -> Option<(&str, &str, u128)> {
    let stem = file.strip_suffix(".lemon").or_else(|| file.strip_suffix(".flp"))?;
    let (base, tail) = stem.rsplit_once(" [")?;
    let (id, generation) = tail.strip_suffix(']')?.split_once('-')?;
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) || generation.len() != 32 {
        return None;
    }
    Some((base, id, u128::from_str_radix(generation, 16).ok()?))
}

pub(crate) fn belongs_to(file: &str, id: [u8; 16]) -> bool {
    suffix(file).is_some_and(|(_, found, _)| found == identity_text(id))
}

pub(crate) fn generation_order(file: &str) -> u128 {
    suffix(file).map_or(0, |(_, _, generation)| generation)
}

/// When file `file` was autosaved, if it is an autosave of `name`: (year, month, day, hour, minute).
fn autosaved_at(file: &str, name: &str) -> Option<(u32, u32, u32, u32, u32)> {
    saved_at(file, name, "autosaved")
}

/// Read both the current identity-qualified names and old names for manual legacy recovery.
pub(crate) fn saved_at(file: &str, name: &str, what: &str) -> Option<(u32, u32, u32, u32, u32)> {
    let stem = suffix(file).map(|(base, _, _)| base)
        .or_else(|| file.strip_suffix(".lemon").or_else(|| file.strip_suffix(".flp")))?;
    let rest = stem.strip_prefix(name)?.strip_prefix(" (")?.strip_prefix(what)?.strip_prefix(" on ")?.strip_suffix(')')?;
    let (date, time) = rest.split_once(" at ")?;
    let mut d = date.split('-').map(str::parse::<u32>);
    let (Some(Ok(m)), Some(Ok(day)), Some(Ok(y)), None) = (d.next(), d.next(), d.next(), d.next()) else { return None };
    let (h, min) = time.split_once('h')?;
    Some((y, m, day, h.parse().ok()?, min.parse().ok()?))
}

/// Prune only this project's identity. Same-named projects, legacy backups and overwritten copies are untouched.
pub fn prune(dir: &Path, name: &str, id: [u8; 16], max: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut found: Vec<_> = entries.flatten().filter_map(|e| {
        let file = e.file_name().into_string().ok()?;
        if !belongs_to(&file, id) || !e.file_type().ok()?.is_file() {
            return None;
        }
        let at = autosaved_at(&file, name)?;
        Some((at, generation_order(&file), e.path()))
    }).collect();
    if found.len() > max {
        found.sort();
        for (_, _, p) in &found[..found.len() - max] {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Preserve the file being replaced before the primary save can publish. Failure aborts that save.
pub fn keep_overwritten(target: &Path, dir: Option<PathBuf>, fallback_id: [u8; 16]) -> Result<(), String> {
    let Some(dir) = dir.filter(|_| target.is_file()) else { return Ok(()) };
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create backup folder {}: {e}", dir.display()))?;
    let id = std::fs::read(target).ok().and_then(|b| os_flp::FlpFile::parse(&b).ok())
        .and_then(|f| crate::lemon_state::project_id(&f)).unwrap_or(fallback_id);
    let ext = if target.extension().is_some_and(|e| e.eq_ignore_ascii_case("flp")) { "flp" } else { "lemon" };
    let to = dir.join(generation_name(&project_name(Some(target)), "overwritten", chrono::Local::now().naive_local(), id, ext));
    crate::durable_file::copy_new(target, &to).map_err(|e| format!("cannot preserve previous project at {}: {e}", to.display()))
}

impl App {
    /// The Autosave timer (`App::tick`): when its interval ends and the project changed since the last backup or save,
    /// a backup is written. A new Autosave choice starts the timer again.
    pub(crate) fn backup_tick(&mut self, now: Instant) {
        let every = interval(prefs::index(&prefs::AUTOSAVE));
        if every != self.backup.every {
            self.backup.every = every;
            self.backup.due = every.map(|e| now + e);
        }
        let (Some(every), Some(due)) = (every, self.backup.due) else { return };
        if now < due {
            return;
        }
        if self.changed_since_backup() {
            match self.write_backup(chrono::Local::now().naive_local()) {
                // A project loading or an export rendering: at a later tick.
                None => return,
                Some(Err(e)) => {
                    self.state.message = Some(format!("Backup failed: {e}"));
                    self.invalidate();
                }
                Some(Ok(_)) => {}
            }
        }
        self.backup.due = Some(now + every);
    }

    /// Whether the project changed since it was last backed up, and since it was last saved (or opened, or created).
    pub(crate) fn changed_since_backup(&self) -> bool {
        self.unsaved() && self.backup.last.as_ref().is_none_or(|(p, t, native, revision)| {
            *p != self.state.project || *t != self.state.tempo
                || *native != crate::lemon_state::saved_form(&self.state, &self.plugins)
                || *revision != self.state.plugin_edit_revision
        })
    }

    /// Writes a backup of the project as it is now, named for local time `t`, then deletes its oldest autosaves beyond
    /// Maximum (`prefs::BACKUP_MAXIMUM`). Every successful call publishes a distinct generation, including repeated
    /// edits in the same minute; None while the project cannot be written now.
    pub(crate) fn write_backup(&mut self, t: NaiveDateTime) -> Option<Result<PathBuf, String>> {
        let Some(dir) = crate::data_folder::backup_folder(&self.state, self.state.path.as_deref()) else { return Some(Err("no user folder".into())) };
        let name = project_name(self.state.path.as_deref());
        let id = self.state.recovery_id;
        let target = dir.join(generation_name(&name, "autosaved", t, id, crate::project_files::EXT));
        if let Err(e) = std::fs::create_dir_all(&dir) {
            return Some(Err(format!("{}: {e}", dir.display())));
        }
        let snap = (self.state.project.clone(), self.state.tempo,
            crate::lemon_state::saved_form(&self.state, &self.plugins), self.state.plugin_edit_revision);
        let done = self.save_backup_copy(target.clone())?;
        if done.is_ok() {
            self.backup.last = Some(snap);
            prune(&dir, &name, id, usize::from(prefs::number(&prefs::BACKUP_MAXIMUM)));
        }
        Some(done.map(|()| target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Action;
    use crate::views::settings::{self, SAct};

    fn at(m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, m, d).unwrap().and_hms_opt(h, min, 30).unwrap()
    }

    /// A fresh folder for this test's backups (this thread's `folder`).
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("os_backup_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        TEST_DIR.with(|d| *d.borrow_mut() = Some(dir.clone()));
        dir
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir).map(|r| r.flatten().filter_map(|e| e.file_name().into_string().ok()).collect()).unwrap_or_default();
        v.sort();
        v
    }

    #[test]
    fn names_follow_fl() {
        assert_eq!(file_name("gs7", "autosaved", at(10, 15, 0, 13)), "gs7 (autosaved on 10-15-2026 at 0h13).lemon");
        assert_eq!(file_name("untitled", "overwritten", at(3, 4, 18, 5)), "untitled (overwritten on 3-4-2026 at 18h05).lemon");
        assert_eq!(project_name(None), "untitled");
        assert_eq!(project_name(Some(Path::new("/music/My song.flp"))), "My song");
        assert_eq!(autosaved_at("gs7 (autosaved on 10-15-2026 at 0h13).lemon", "gs7"), Some((2026, 10, 15, 0, 13)));
        assert_eq!(autosaved_at("gs7 (autosaved on 10-15-2026 at 0h13).flp", "gs7"), Some((2026, 10, 15, 0, 13)), "an older backup");
        for other in ["gs7 (overwritten on 10-15-2026 at 0h13).flp", "gs70 (autosaved on 10-15-2026 at 0h13).flp", "gs7.flp"] {
            assert_eq!(autosaved_at(other, "gs7"), None, "{other}");
        }
        assert_eq!(interval(2), Some(Duration::from_secs(600)), "Occasionally");
        assert_eq!((interval(0), interval(5), interval(6)), (None, Some(Duration::from_secs(60)), None));
    }

    /// The oldest autosaves of the project's name go; other names and "overwritten on" copies stay.
    #[test]
    fn prune_keeps_the_newest() {
        let dir = scratch("prune");
        std::fs::create_dir_all(&dir).unwrap();
        let id = new_identity();
        let files = [
            generation_name("a", "autosaved", at(9, 30, 23, 59), id, "lemon"),
            generation_name("a", "autosaved", at(10, 1, 0, 0), id, "lemon"),
            generation_name("a", "autosaved", at(10, 1, 9, 5), id, "lemon"),
            generation_name("a", "autosaved", at(10, 1, 10, 0), id, "lemon"),
            generation_name("a", "overwritten", at(1, 1, 0, 0), id, "lemon"),
            generation_name("a", "autosaved", at(1, 1, 0, 0), new_identity(), "lemon"),
            file_name("a", "autosaved", at(1, 1, 0, 0)),
        ];
        for f in &files {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        prune(&dir, "a", id, 2);
        let mut want = files[2..].to_vec();
        want.sort();
        assert_eq!(names(&dir), want);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// After the interval, a changed project is backed up once; unchanged or just saved, it is not. A backup is not a
    /// save: the project keeps its name, its unsaved changes and the message.
    #[test]
    fn autosave_writes_changed_projects() {
        let dir = scratch("timer");
        let mut app = App::new(None);
        prefs::set_choice(&prefs::AUTOSAVE, 5);
        let t0 = Instant::now();
        let minute = Duration::from_secs(61);
        app.backup_tick(t0);
        app.backup_tick(t0 + minute);
        assert!(names(&dir).is_empty(), "nothing changed");
        app.state.tempo += 7.0;
        app.state.message = Some("hello".into());
        app.backup_tick(t0 + minute + Duration::from_secs(30));
        assert!(names(&dir).is_empty(), "the interval runs on");
        app.backup_tick(t0 + 2 * minute);
        let files = names(&dir);
        assert_eq!(files.len(), 1, "{files:?}");
        assert!(autosaved_at(&files[0], "untitled").is_some(), "{files:?}");
        assert!(app.unsaved() && app.state.path.is_none() && app.state.message.as_deref() == Some("hello"), "a copy, not a save");
        let opened = App::new(Some(dir.join(&files[0])));
        assert_eq!(opened.state.tempo, app.state.tempo, "the backup holds the change");
        // Backed up already: the next interval writes nothing.
        assert!(!app.changed_since_backup());
        app.backup_tick(t0 + 3 * minute);
        assert_eq!(names(&dir).len(), 1);
        // Never: no timer.
        prefs::set_choice(&prefs::AUTOSAVE, 0);
        app.state.tempo += 1.0;
        app.backup_tick(t0 + 4 * minute);
        app.backup_tick(t0 + 9 * minute);
        assert_eq!(names(&dir).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Backups are named for their time; past Maximum the oldest go. Saving over a file keeps the old one.
    #[test]
    fn backups_and_overwritten_copies() {
        let dir = scratch("files");
        let mut app = App::new(None);
        let work = dir.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let song = work.join("song.flp");
        app.save_to(song.clone());
        assert!(song.is_file());
        assert_eq!(names(&dir), ["work"], "a new file: nothing saved over");
        app.state.tempo = 99.0;
        app.save_to(song.clone());
        let over: Vec<String> = names(&dir).into_iter().filter(|n| n.starts_with("song (overwritten on ")).collect();
        assert_eq!(over.len(), 1, "{:?}", names(&dir));
        assert_ne!(std::fs::read(dir.join(&over[0])).unwrap(), std::fs::read(&song).unwrap(), "the file as it was");
        // Autosaves of "song", newest kept: 100 by default, here 5.
        assert_eq!(prefs::number(&prefs::BACKUP_MAXIMUM), 100);
        prefs::set_number(&prefs::BACKUP_MAXIMUM, 5);
        for i in 0..7 {
            app.state.tempo = 100.0 + f64::from(i);
            let t = at(10, 1 + i / 60, 12, i % 60);
            let path = app.write_backup(t).unwrap().unwrap();
            assert!(belongs_to(path.file_name().unwrap().to_str().unwrap(), app.state.recovery_id));
        }
        let auto: Vec<String> = names(&dir).into_iter().filter(|n| autosaved_at(n, "song").is_some()).collect();
        assert_eq!(auto.len(), 5);
        assert!(auto.iter().all(|n| autosaved_at(n, "song").unwrap().4 >= 2));
        assert!(auto.iter().any(|n| autosaved_at(n, "song") == Some((2026, 10, 1, 12, 6))), "the newest");
        // Lowered, nothing goes until the next backup.
        prefs::set_number(&prefs::BACKUP_MAXIMUM, 2);
        assert_eq!(names(&dir).into_iter().filter(|n| autosaved_at(n, "song").is_some()).count(), 5);
        app.state.tempo = 1.0;
        app.write_backup(at(10, 1, 13, 0)).unwrap().unwrap();
        let auto: Vec<String> = names(&dir).into_iter().filter(|n| autosaved_at(n, "song").is_some()).collect();
        assert_eq!(auto.iter().map(|n| autosaved_at(n, "song").unwrap()).collect::<Vec<_>>(), [(2026, 10, 1, 12, 6), (2026, 10, 1, 13, 0)]);
        prefs::set_number(&prefs::BACKUP_MAXIMUM, prefs::BACKUP_MAXIMUM.default);
        assert_eq!(app.state.path.as_deref(), Some(song.as_path()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn same_minute_and_same_named_projects_have_independent_generations() {
        let dir = scratch("identities");
        let mut a = App::new(None);
        let mut b = App::new(None);
        assert_ne!(a.state.recovery_id, b.state.recovery_id);
        prefs::set_number(&prefs::BACKUP_MAXIMUM, 1);
        let t = at(11, 2, 10, 15);
        a.state.tempo = 101.0;
        let a1 = a.write_backup(t).unwrap().unwrap();
        b.state.tempo = 202.0;
        let b1 = b.write_backup(t).unwrap().unwrap();
        a.state.tempo = 103.0;
        let a2 = a.write_backup(t).unwrap().unwrap();
        assert_ne!(a1, a2, "same-minute edits must produce a new generation");
        assert!(!a1.exists(), "only this identity's older generation is pruned");
        assert!(b1.exists() && a2.exists());
        assert_eq!(App::new(Some(b1)).state.tempo, 202.0);
        assert_eq!(App::new(Some(a2)).state.tempo, 103.0);
        prefs::set_number(&prefs::BACKUP_MAXIMUM, prefs::BACKUP_MAXIMUM.default);
        TEST_DIR.with(|d| *d.borrow_mut() = None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn native_only_and_successive_plugin_only_edits_trigger_backups() {
        let dir = scratch("metadata");
        let mut app = App::new(None);
        app.state.mixer.waveforms = !app.state.mixer.waveforms;
        assert!(app.changed_since_backup());
        app.write_backup(at(11, 2, 10, 20)).unwrap().unwrap();
        assert!(!app.changed_since_backup());
        app.state.mark_plugin_edited();
        assert!(app.changed_since_backup());
        app.write_backup(at(11, 2, 10, 20)).unwrap().unwrap();
        assert!(!app.changed_since_backup());
        app.state.mark_plugin_edited();
        assert!(app.changed_since_backup(), "a sticky dirty boolean cannot represent the next plugin edit");
        app.write_backup(at(11, 2, 10, 20)).unwrap().unwrap();
        assert_eq!(names(&dir).len(), 3);
        assert!(!app.changed_since_backup());
        TEST_DIR.with(|d| *d.borrow_mut() = None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn native_identity_reopens_but_save_as_and_templates_get_new_identities() {
        let dir = scratch("identity_save");
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = App::new(None);
        let first = app.state.recovery_id;
        let song = dir.join("song.lemon");
        app.save_to_result(song.clone()).unwrap();
        let mut opened = App::new(Some(song.clone()));
        assert_eq!(opened.state.recovery_id, first);
        opened.save_to_result(dir.join("song_2.lemon")).unwrap();
        assert_ne!(opened.state.recovery_id, first);
        let mut template = App::new(None);
        assert!(template.new_from_file(&song, "test"));
        assert_ne!(template.state.recovery_id, first);
        TEST_DIR.with(|d| *d.borrow_mut() = None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_backup_preservation_leaves_primary_and_dirty_state_unchanged() {
        let dir = scratch("failed_preserve");
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = App::new(None);
        let song = dir.join("song.lemon");
        app.save_to_result(song.clone()).unwrap();
        let original = std::fs::read(&song).unwrap();
        let blocked = dir.join("not_a_directory");
        std::fs::write(&blocked, b"keep").unwrap();
        TEST_DIR.with(|d| *d.borrow_mut() = Some(blocked));
        app.state.tempo = 97.0;
        let model = app.state.project.clone();
        let id = app.state.recovery_id;
        let result = app.save_to_result(song.clone());
        assert!(result.is_err());
        assert_eq!(std::fs::read(&song).unwrap(), original);
        assert_eq!(app.state.project, model);
        assert_eq!(app.state.recovery_id, id);
        assert_eq!(app.state.path.as_ref(), Some(&song));
        assert!(app.unsaved());
        TEST_DIR.with(|d| *d.borrow_mut() = None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// File ▸ Backup: Autosave and Location are live combos, Maximum a live value box the wheel moves within 1 to 999.
    #[test]
    fn the_file_page() {
        let Ok(mut c) = os_ui::Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.open_settings(settings::FILE);
        app.paint(&mut c, None);
        for c in [&prefs::AUTOSAVE, &prefs::BACKUP_LOCATION] {
            assert!(app.hits.iter().any(|h| matches!(&h.action, Action::Settings(SAct::Combo(l, ..)) if *l == c.label)), "{}", c.label);
        }
        assert!(!app.hits.iter().any(|h| h.action == Action::Placeholder("Maximum")));
        let r = app.hits.iter().find(|h| h.action == Action::Settings(SAct::Number(prefs::BACKUP_MAXIMUM.label))).expect("the Maximum box").rect;
        app.mouse_move(r.cx(), r.cy());
        app.wheel(0.0, -1.0, false, false);
        assert_eq!(prefs::number(&prefs::BACKUP_MAXIMUM), 99);
        assert_eq!(app.state.message.as_deref(), Some("Maximum: 99"));
        prefs::set_number(&prefs::BACKUP_MAXIMUM, 1);
        app.wheel(0.0, -1.0, false, false);
        assert_eq!(prefs::number(&prefs::BACKUP_MAXIMUM), 1, "at least one");
        prefs::set_number(&prefs::BACKUP_MAXIMUM, 999);
        app.wheel(0.0, 1.0, false, false);
        assert_eq!(prefs::number(&prefs::BACKUP_MAXIMUM), 999, "at most 999");
        prefs::set_number(&prefs::BACKUP_MAXIMUM, prefs::BACKUP_MAXIMUM.default);
        prefs::set_number(&prefs::BACKUP_MAXIMUM, 42);
        let shown = settings::page(settings::FILE, &app.state).into_iter().flat_map(|g| g.rows).any(|r| match r {
            settings::Row::Pair(_, b) => matches!(*b, settings::Row::ValueR("Maximum", ref v) if v == "42"),
            _ => false,
        });
        assert!(shown, "the page shows the value");
        prefs::set_number(&prefs::BACKUP_MAXIMUM, prefs::BACKUP_MAXIMUM.default);
    }
}
