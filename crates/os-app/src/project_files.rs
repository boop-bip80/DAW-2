// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! FILE's project file rows (also TOOLS ▸ Delete unused files and save project): Revert to last backup, Save as
//! template..., New from template ▸, Save to new project folder... and Delete unused files and save project.
//!
//! Revert opens the newest backup of the project (autosaved or overwritten, by the time in its name) from the
//! project's backup folder and the global one, under the project's own name: the project file is not touched until the
//! next save. Lemon Studio has no factory templates: Save as template... writes a copy of the project into the
//! Templates folder of the user folder (`crate::templates`, category `User` unless a category folder was picked), and
//! New from template ▸ lists the templates there by category, then Empty. Save to new project folder... saves into a
//! new folder named after the project, in the folder picked, which becomes its data folder. Delete unused files looks
//! only in the data folder's Samples, Recorded and Rendered folders, and only when no other project file lies in the
//! data folder.
// spec: ui/project-templates.md, ui/menu-submenus.md §1, §4, ui/data-folder.md

use std::path::{Path, PathBuf};

use os_model::DataFolder;

use crate::app::{App, State};
use crate::dialog::{spans, Answer, Kind, Msg, Next, Reply, Then};
use crate::playlist::{Cmd, PItem};

/// The category Save as template... writes to when no category folder was picked.
pub const USER_CATEGORY: &str = "User";
/// The data folder's folders Delete unused files looks in: the copied samples, the recordings and the renders.
pub const SCANNED: [&str; 3] = [crate::data_folder::SAMPLES_DIR, "Recorded", "Rendered"];
/// The names the delete question lists before "and N more".
const LISTED: usize = 10;

/// The newest backup of project `name` in folders `dirs`: an autosave or an "overwritten on" copy, by the time in its
/// name (an autosave wins a tie: it is the later state).
pub fn newest_backup(dirs: &[PathBuf], name: &str) -> Option<PathBuf> {
    newest_backup_for(dirs, name, None)
}

fn newest_backup_for(dirs: &[PathBuf], name: &str, identity: Option<[u8; 16]>) -> Option<PathBuf> {
    let mut best: Option<((u32, u32, u32, u32, u32), u128, bool, PathBuf)> = None;
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for e in entries.flatten() {
            let Some(file) = e.file_name().to_str().map(str::to_string) else { continue };
            if identity.is_some_and(|id| !crate::backup::belongs_to(&file, id)) {
                continue;
            }
            let found = crate::backup::saved_at(&file, name, "autosaved").map(|t| (t, true)).or_else(|| crate::backup::saved_at(&file, name, "overwritten").map(|t| (t, false)));
            let Some((t, auto)) = found else { continue };
            let generation = crate::backup::generation_order(&file);
            if best.as_ref().is_none_or(|(bt, bg, ba, _)| (t, generation, auto) > (*bt, *bg, *ba)) {
                best = Some((t, generation, auto, e.path()));
            }
        }
    }
    best.map(|(_, _, _, p)| p)
}

/// Where Save as template... writes the template picked in the save dialog as `chosen`:
/// `<root>/<category>/<name>/<name>.flp`, the category being the folder picked when it is one directly in `root` (or the template's own folder in one),
/// else `User`.
pub fn template_target(root: &Path, chosen: &Path) -> PathBuf {
    let name = chosen.file_stem().map_or_else(|| "Template".to_string(), |n| n.to_string_lossy().into_owned());
    let parent = chosen.parent();
    let cat = match (parent, parent.and_then(Path::parent), parent.and_then(Path::parent).and_then(Path::parent)) {
        (Some(p), Some(pp), _) if pp == root => p.file_name(),
        (Some(p), Some(pp), Some(ppp)) if ppp == root && p.file_name().is_some_and(|n| n.to_string_lossy() == name) => pp.file_name(),
        _ => None,
    };
    let cat = cat.map_or_else(|| USER_CATEGORY.to_string(), |c| c.to_string_lossy().into_owned());
    crate::templates::file_in(root, &cat, &name)
}

/// FILE ▸ New from template ▸: a row per category of the Templates folder with its templates (each starts a new
/// project from that template), then Empty (the built-in project).
pub fn template_items(_: &State) -> Vec<PItem> {
    let mut items: Vec<PItem> = crate::templates::root()
        .map(|r| crate::templates::scan(&r))
        .unwrap_or_default()
        .into_iter()
        .map(|c| {
            let children = c.templates.iter().map(|t| PItem::new(t.clone(), Cmd::NewTemplate(format!("{}/{t}", c.name)))).collect();
            PItem { children, ..PItem::new(c.name, Cmd::Nothing) }
        })
        .collect();
    items.push(PItem::new(crate::templates::EMPTY, Cmd::NewTemplate(crate::templates::EMPTY.into())));
    items
}

/// Compare existing files by resolved identity, not by the spelling of their paths. In particular a path through
/// `..` or a symlink must protect the very same sample from cleanup. An unreadable candidate is never presumed
/// unused; a missing *reference*, on the other hand, cannot refer to an existing candidate.
fn same_file(a: &Path, b: &Path) -> std::io::Result<bool> {
    let a = std::fs::canonicalize(a)?;
    let b = match std::fs::canonicalize(b) {
        Ok(path) => path,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    if a == b || (cfg!(windows) && a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()) {
        return Ok(true);
    }
    let left = std::fs::metadata(a)?;
    let right = std::fs::metadata(b)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return Ok(left.dev() == right.dev() && left.ino() == right.ino());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        return Ok(left.volume_serial_number().zip(left.file_index()) == right.volume_serial_number().zip(right.file_index())
            && left.volume_serial_number().is_some() && left.file_index().is_some());
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (left, right);
        Ok(false)
    }
}

/// The files under `dir`, every folder down.
fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => files_under(&p, out),
            Ok(t) if t.is_file() => out.push(p),
            _ => {}
        }
    }
}

/// The files of data folder `dir` the project does not use (`used`: the files it refers to): those in its Samples,
/// What Save writes: Lemon Studio's own project file (spec: flp/lemon-format.md).
pub const EXT: &str = "lemon";

/// The project files Lemon Studio opens: its own, and the FLPs it migrates from (spec: flp/lemon-format.md).
pub const PROJECT_EXTS: [&str; 2] = [EXT, "flp"];

/// Whether `p` is a project file by its extension (`.lemon` or `.flp`).
pub fn is_project(p: &Path) -> bool {
    p.extension().is_some_and(|e| PROJECT_EXTS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// Whether `p` is an FLP: Save does not write over it but asks for a `.lemon` name (spec: flp/lemon-format.md
/// "Opening and saving").
pub fn is_flp(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("flp"))
}

/// `p` with the `.lemon` extension.
pub fn as_lemon(p: &Path) -> PathBuf {
    p.with_extension(EXT)
}

/// Recorded and Rendered folders, project files never. Sorted by path.
pub fn unused_files(dir: &Path, used: &[PathBuf]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for sub in SCANNED {
        files_under(&dir.join(sub), &mut found);
    }
    // Uncertain identity means this candidate is NOT safe to move. Missing referenced files return Ok(false).
    found.retain(|f| !is_project(f) && !used.iter().any(|u| same_file(f, u).unwrap_or(true)));
    found.sort();
    found
}

/// Whether data folder `dir` holds a project file other than `own`: then it is not this project's alone, and Delete
/// unused files leaves it alone.
pub fn shared_folder(dir: &Path, own: &Path) -> bool {
    fn inspect(dir: &Path, own: &Path, name: &str) -> std::io::Result<bool> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            // A linked folder can hide other projects; linked files can change after enumeration.
            if kind.is_symlink() { return Ok(true) }
            if kind.is_dir() {
                if inspect(&path, own, name)? { return Ok(true) }
            } else if kind.is_file() && is_project(&path) {
                // A recovery generation for our own project is already inspected for dependencies by used_files.
                let own_backup = path.parent().is_some_and(|p| p.file_name().is_some_and(|n| n == "Backup"))
                    && path.file_name().and_then(|n| n.to_str()).is_some_and(|file| {
                        crate::backup::saved_at(file, name, "autosaved").is_some()
                            || crate::backup::saved_at(file, name, "overwritten").is_some()
                    });
                if !own_backup && !same_file(&path, own)? { return Ok(true) }
            }
        }
        Ok(false)
    }
    // An inaccessible subtree cannot establish exclusive ownership of the data folder.
    inspect(dir, own, &crate::backup::project_name(Some(own))).unwrap_or(true)
}

/// Atomically move the original into quarantine on the same filesystem (the data folder). The manifest is
/// committed first so an interrupted cleanup has an explicit recovery map, without any copy/unlink race that
/// could discard edits made during a large sample copy. Files are kept intact in the recoverable folder.
fn quarantine(dir: &Path, project: &Path, files: &[PathBuf]) -> Result<(usize, PathBuf), String> {
    let root = dir.join("Unused files");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
    let recovery = (0u32..).find_map(|n| {
        let path = root.join(format!("{now}-{}-{n}", std::process::id()));
        match std::fs::create_dir(&path) {
            Ok(()) => Some(Ok(path)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => None,
            Err(e) => Some(Err(e.to_string())),
        }
    }).expect("a directory name")?;
    let mut entries = Vec::new();
    for file in files {
        let relative = file.strip_prefix(dir).map_err(|_| format!("{} is outside the data folder", file.display()))?;
        entries.push(serde_json::json!({"original": relative.to_string_lossy(), "recovery": relative.to_string_lossy()}));
    }
    let manifest = serde_json::json!({
        "version": 1,
        "project": project.to_string_lossy(),
        "restore": "Copy each recovered file back to its original relative path in the project data folder. Keep an existing original unless its contents match. Recovery files are never automatically deleted.",
        "files": entries,
    });
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    crate::durable_file::create(&recovery.join("restore.json"), &bytes).map_err(|e| e.to_string())?;
    let mut moved = 0;
    for file in files {
        let relative = file.strip_prefix(dir).map_err(|e| e.to_string())?;
        let to = recovery.join(relative);
        if let Some(parent) = to.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
        if to.exists() { return Err(format!("quarantine path {} unexpectedly already exists; no file was overwritten", to.display())) }
        std::fs::rename(file, &to).map_err(|e| format!("{} could not be quarantined: {e}", file.display()))?;
        moved += 1;
    }
    Ok((moved, recovery))
}

impl App {
    /// FILE ▸ Revert to last backup: the newest backup of the project, after the unsaved changes question as Open asks
    /// it; without one, a box says so.
    pub(crate) fn revert_to_backup(&mut self) {
        let Some(path) = self.state.path.clone() else { return };
        let name = crate::backup::project_name(Some(&path));
        let mut dirs: Vec<PathBuf> = crate::data_folder::backup_folder(&self.state, Some(&path)).into_iter().collect();
        dirs.extend(crate::backup::folder().filter(|d| !dirs.contains(d)));
        match newest_backup_for(&dirs, &name, Some(self.state.recovery_id)) {
            Some(b) => self.ask_discard("reverting to the last backup", Next::Revert(b)),
            None => {
                let file = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                self.tell(Kind::Info, "Restore backup...", spans(&["There is no backup of ", &file, "."]));
            }
        }
    }

    /// The revert goes on: backup `b` is opened as the project, keeping the project's file and name (Save writes it
    /// over the project file); the project counts as changed. It is not a recent or last used project.
    pub(crate) fn revert_from(&mut self, b: PathBuf) {
        let Some(own) = self.state.path.clone() else { return };
        let (kept, recent) = (crate::templates::last_kept(), crate::recent::list());
        self.open(b.clone());
        crate::templates::restore_last(kept);
        crate::recent::restore(&recent);
        // Not opened: `open` said why.
        if self.state.path.as_deref() != Some(b.as_path()) {
            return;
        }
        self.state.path = Some(own);
        // The project file still holds the last save: unsaved until saved again.
        self.saved.1 = f64::NAN;
        let n = b.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        self.state.message = Some(format!("Reverted to {n}"));
    }

    /// FILE ▸ Save as template...: the save dialog, in the Templates folder's User category, for the template's name.
    pub(crate) fn save_template_dialog(&mut self) {
        use crate::filedlg::{FileDlg, Mode};
        let Some(root) = crate::templates::root() else {
            self.state.message = Some("Save as template: no user folder".into());
            return;
        };
        let start = root.join(USER_CATEGORY);
        let _ = std::fs::create_dir_all(&start);
        let name = format!("{}.{EXT}", crate::backup::project_name(self.state.path.as_deref()));
        let dlg = FileDlg::new(Mode::Save, "Save as template", "template", Then::SaveTemplate).filter("Lemon Studio project", &[EXT]).start(Some(&start)).name(&name);
        self.file_dialog(dlg);
    }

    /// The template's name was given (`chosen`, from the save dialog): a copy of the project is written as that
    /// template (`template_target`); the project keeps its file and name.
    pub(crate) fn template_chosen(&mut self, chosen: PathBuf) {
        let Some(root) = crate::templates::root() else { return };
        let target = template_target(&root, &chosen);
        if let Some(Err(e)) = target.parent().map(std::fs::create_dir_all) {
            self.state.message = Some(format!("Save as template failed: {e}"));
            return;
        }
        let name = target.file_stem().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        self.state.message = Some(match self.save_copy(target) {
            Some(Ok(())) => format!("Saved the template {name}"),
            Some(Err(e)) => format!("Save as template failed: {e}"),
            None => "Save as template: wait for the project to load or the export to end".into(),
        });
    }

    /// A row of New from template ▸ (`Cmd::NewTemplate`: "Empty" or "<category>/<template>"): after the unsaved changes
    /// question as New asks it, a new untitled project from it.
    pub(crate) fn pick_new_template(&mut self, pick: &str) {
        let file = pick.split_once('/').and_then(|(cat, name)| Some(crate::templates::file_in(&crate::templates::root()?, cat, name)));
        self.ask_discard("starting a new project", Next::Template(file));
    }

    /// The new project from template `file` (None: Empty, or a template that does not load: the built-in project); the
    /// New project form follows as after New.
    pub(crate) fn new_from_pick(&mut self, file: Option<PathBuf>) {
        let name = file.as_deref().and_then(Path::file_stem).map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        if !file.is_some_and(|f| self.new_from_file(&f, &name)) {
            self.empty_project();
        }
        self.offer_new_form();
    }

    /// FILE ▸ Save to new project folder...: a folder picker, starting in the project's folder, for where the new
    /// folder goes.
    pub(crate) fn new_folder_dialog(&mut self) {
        use crate::filedlg::{FileDlg, Mode};
        let Some(path) = self.state.path.clone() else { return };
        let dlg = FileDlg::new(Mode::Folder, "Save to new project folder", "project folder", Then::NewProjectFolder).start(path.parent());
        self.file_dialog(dlg);
    }

    /// The place was picked (`root`): the project is saved as `<root>/<name>/<name>.lemon`, named after the project, that
    /// new folder its data folder (the used samples copied there while Copy used samples to the project folder is on). A
    /// project file already there is refused.
    pub(crate) fn new_folder_chosen(&mut self, root: PathBuf) {
        const TITLE: &str = "Save to new project folder";
        let name = crate::backup::project_name(self.state.path.as_deref());
        let dir = root.join(&name);
        let file = dir.join(format!("{name}.{EXT}"));
        if file.exists() {
            return self.tell(Kind::Warning, TITLE, spans(&["", &file.display().to_string(), " already exists. Nothing was saved."]));
        }
        if let Err(e) = std::fs::create_dir_all(&dir) {
            return self.tell(Kind::Error, TITLE, spans(&["", &dir.display().to_string(), &format!(" could not be made: {e}")]));
        }
        let before = self.state.project.clone();
        let change = if crate::prefs::check(&crate::prefs::COPY_USED_SAMPLES) {
            match self.collect_assets_to(&dir) {
                Ok(change) => Some(change),
                Err(e) => { self.state.message = Some(format!("Save to new project folder stopped: {e}")); return; }
            }
        } else { None };
        let df = self.state.project.data_folder.get_or_insert_with(DataFolder::default);
        df.choice = DataFolder::RELATIVE;
        df.folder = crate::data_folder::folder_text(&dir);
        if let Err(e) = self.save_to_result(file) {
            let restored = change.map(|c| c.rollback(self)).transpose();
            self.state.project = before;
            crate::missing::apply(self);
            let detail = restored.err().map_or_else(String::new, |e| format!("; {e}"));
            self.state.message = Some(format!("Save to new project folder failed: {e}{detail}; the original project was retained"));
        } else {
            crate::assets::collection_notice(self, change.as_ref());
            crate::missing::apply(self);
        }
        crate::data_folder::reread(&self.state);
    }

    /// Current live references, plus the previous saved project and retained recovery generations. Unknown or
    /// unreadable state prevents cleanup instead of turning a failed enumeration into an empty dependency list.
    fn used_files(&mut self) -> Result<Vec<PathBuf>, String> {
        let project = crate::assets::snapshot(self)?;
        let mut used = Vec::new();
        let mut add = |p: &os_model::Project| -> Result<(), String> {
            let inventory = crate::assets::inspect(p)?;
            if !inventory.opaque.is_empty() {
                return Err(format!("External files of {} cannot be enumerated safely", inventory.opaque.into_iter().collect::<Vec<_>>().join(", ")));
            }
            for (_, path) in inventory.references {
                used.push(PathBuf::from(crate::plugins::resolve_sample(&path)));
                used.push(PathBuf::from(path));
            }
            Ok(())
        };
        add(&project)?;
        let mut saved = self.state.path.clone().filter(|p| p.is_file()).into_iter().collect::<Vec<_>>();
        let mut dirs: Vec<PathBuf> = crate::data_folder::backup_folder(&self.state, self.state.path.as_deref()).into_iter().collect();
        if let Some(global) = crate::backup::folder().filter(|d| !dirs.contains(d)) { dirs.push(global); }
        let name = crate::backup::project_name(self.state.path.as_deref());
        for dir in dirs {
            let entries = match std::fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(format!("Recovery folder {} could not be checked: {e}", dir.display())),
            };
            for entry in entries {
                let entry = entry.map_err(|e| format!("Recovery folder {} could not be checked: {e}", dir.display()))?;
                let file = entry.file_name().to_string_lossy().into_owned();
                if is_project(&entry.path()) && (crate::backup::belongs_to(&file, self.state.recovery_id)
                    || file.starts_with(&format!("{name} ("))) {
                    saved.push(entry.path());
                }
            }
        }
        for file in saved {
            let bytes = std::fs::read(&file).map_err(|e| format!("Recovery project {} could not be checked: {e}", file.display()))?;
            let mut parsed = os_flp::FlpFile::parse(&bytes).map_err(|e| format!("Recovery project {} could not be checked: {e}", file.display()))?;
            os_flp::lemon::validate_version(&parsed).map_err(|e| e.to_string())?;
            os_flp::project::upgrade(&mut parsed).map_err(|e| e.to_string())?;
            let p = os_flp::project::load(&parsed).map_err(|e| e.to_string())?;
            add(&p)?;
        }
        Ok(used)
    }

    fn cleanup_candidates(&mut self, dir: &Path, path: &Path) -> Result<Vec<PathBuf>, String> {
        if shared_folder(dir, path) { return Err("The data folder holds other projects too".into()) }
        if self.state.history.entries().len() > 1 {
            return Err("Undo or redo history may still need these files. Save and reopen the project before cleaning its data folder".into());
        }
        let used = self.used_files()?;
        Ok(unused_files(dir, &used))
    }

    /// List only files proved unused. They will be quarantined with a restore manifest after a successful save;
    /// undo history, opaque plugin dependencies or unreadable recovery projects block cleanup conservatively.
    pub(crate) fn delete_unused_ask(&mut self) {
        const TITLE: &str = "Clean up";
        let Some(path) = self.state.path.clone() else { return };
        let Some(dir) = crate::data_folder::folder(&self.state).filter(|d| d.is_dir()) else {
            self.save_to(path);
            self.prefix_message("No project data folder to clean");
            return;
        };
        let files = match self.cleanup_candidates(&dir, &path) {
            Ok(files) => files,
            Err(e) => return self.tell(Kind::Warning, TITLE, spans(&[&e, ". No files were moved or deleted."])),
        };
        if files.is_empty() {
            self.save_to(path);
            self.prefix_message("No unused files");
            return;
        }
        let mut text = vec![(format!("Save the project and move these {} unused files into a recoverable ‘Unused files’ folder?\n", files.len()), false)];
        for f in files.iter().take(LISTED) {
            text.push(("\n".into(), false));
            text.push((f.strip_prefix(&dir).unwrap_or(f).display().to_string(), true));
        }
        if files.len() > LISTED { text.push((format!("\nand {} more", files.len() - LISTED), false)); }
        text.push(("\n\nThe recovery folder contains the original files and a manifest explaining where to restore them.".into(), false));
        self.show_msg(Msg::new(Kind::Question, TITLE, text, &[Answer::Yes, Answer::No], Then::DeleteUnused(files)));
    }

    /// Recheck after confirmation and save before moving anything. A stale dialog cannot delete a newly used file.
    pub(crate) fn delete_unused_done(&mut self, files: Vec<PathBuf>, reply: &Reply) {
        if *reply != Reply::Button(Answer::Yes) { return }
        let Some(path) = self.state.path.clone() else { return };
        let Some(dir) = crate::data_folder::folder(&self.state).filter(|d| d.is_dir()) else { return };
        let candidates = match self.cleanup_candidates(&dir, &path) {
            Ok(candidates) => candidates,
            Err(e) => { self.state.message = Some(format!("Clean up stopped: {e}. No files were moved or deleted.")); return; }
        };
        let approved: Vec<PathBuf> = files.into_iter().filter(|p| candidates.contains(p)).collect();
        if let Err(e) = self.save_to_result(path.clone()) {
            self.state.message = Some(format!("Clean up stopped because the project could not be saved: {e}. No files were moved or deleted."));
            return;
        }
        // Save may refresh hosted plugin state and rewrite paths. Re-evaluate the *persisted* project and its
        // backups, intersecting with both the original approval and the pre-save inventory before moving files.
        let latest = match self.cleanup_candidates(&dir, &path) {
            Ok(candidates) => candidates,
            Err(e) => { self.state.message = Some(format!("Clean up stopped after saving: {e}. No files were moved or deleted.")); return; }
        };
        let files: Vec<PathBuf> = approved.into_iter().filter(|p| latest.contains(p)).collect();
        if files.is_empty() { self.prefix_message("No unused files"); return }
        match quarantine(&dir, &path, &files) {
            Ok((count, recovery)) => self.prefix_message(&format!("Moved {count} unused files to {}", recovery.display())),
            Err(e) => self.prefix_message(&format!("Clean up stopped: {e}; all files remain in their original location or the recovery folder")),
        }
        crate::data_folder::reread(&self.state);
    }

    /// `what` goes before the message the save left: "<what>. Saved ...".
    fn prefix_message(&mut self, what: &str) {
        let m = self.state.message.take().unwrap_or_default();
        self.state.message = Some(if m.is_empty() { what.to_string() } else { format!("{what}. {m}") });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("os_projfiles_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn at(m: u32, d: u32, h: u32, min: u32) -> chrono::NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, m, d).unwrap().and_hms_opt(h, min, 0).unwrap()
    }

    /// The newest backup of the name wins, across folders and both kinds; other names never count.
    #[test]
    fn backup_picking() {
        use crate::backup::file_name;
        let (a, b) = (scratch("bk_a"), scratch("bk_b"));
        assert_eq!(newest_backup(&[a.clone(), b.clone()], "song"), None, "no backup");
        for (dir, f) in [
            (&a, file_name("song", "autosaved", at(10, 1, 9, 5))),
            (&a, file_name("song", "autosaved", at(9, 30, 23, 59))),
            (&b, file_name("song", "overwritten", at(10, 1, 10, 0))),
            (&b, file_name("song2", "autosaved", at(12, 1, 0, 0))),
            (&a, file_name("son", "autosaved", at(12, 1, 0, 0))),
        ] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        assert_eq!(newest_backup(&[a.clone(), b.clone()], "song"), Some(b.join(file_name("song", "overwritten", at(10, 1, 10, 0)))));
        assert_eq!(newest_backup(std::slice::from_ref(&a), "song"), Some(a.join(file_name("song", "autosaved", at(10, 1, 9, 5)))));
        // A tie: the autosave.
        std::fs::write(a.join(file_name("song", "autosaved", at(10, 1, 10, 0))), b"x").unwrap();
        assert_eq!(newest_backup(&[b.clone(), a.clone()], "song"), Some(a.join(file_name("song", "autosaved", at(10, 1, 10, 0)))));
        for d in [a, b] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// Revert opens the newest backup under the project's own path, as a change; with no backup, a box and nothing else.
    #[test]
    fn revert_opens_the_newest_backup() {
        let dir = scratch("revert");
        let bk = dir.join("bk");
        std::fs::create_dir_all(&bk).unwrap();
        crate::backup::TEST_DIR.with(|d| *d.borrow_mut() = Some(bk.clone()));
        let song = dir.join("song.flp");
        let mut app = App::new(None);
        app.state.tempo = 111.0;
        app.save_to(song.clone());
        app.revert_to_backup();
        assert!(app.modal_open(), "no backup: a box says so");
        app.state.dialogs.clear();
        assert_eq!(app.state.tempo, 111.0);
        app.state.tempo = 123.0;
        // Later than the "overwritten on" copy the next save makes now.
        assert_eq!(app.write_backup(at(12, 31, 23, 59)).map(|r| r.is_ok()), Some(true));
        app.state.tempo = 150.0;
        app.save_to(song.clone());
        app.revert_to_backup();
        assert_eq!(app.state.tempo, 123.0, "the backup is open");
        assert_eq!(app.state.path.as_deref(), Some(song.as_path()), "under the project's own name");
        assert!(app.unsaved(), "the project file still holds the last save");
        crate::backup::TEST_DIR.with(|d| *d.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Templates go to User, or to the category folder picked; New from template ▸ lists them by category, then Empty;
    /// a pick starts an untitled project from it.
    #[test]
    fn template_listing() {
        let root = scratch("tpl");
        assert_eq!(template_target(&root, &root.join("User").join("Mine.lemon")), root.join("User").join("Mine").join("Mine.lemon"));
        assert_eq!(template_target(&root, &root.join("Beats").join("Trap.lemon")), root.join("Beats").join("Trap").join("Trap.lemon"));
        assert_eq!(template_target(&root, &root.join("Beats").join("Trap").join("Trap.lemon")), root.join("Beats").join("Trap").join("Trap.lemon"));
        assert_eq!(template_target(&root, &std::env::temp_dir().join("Elsewhere.lemon")), root.join("User").join("Elsewhere").join("Elsewhere.lemon"));
        crate::templates::TEST_DIR.with(|d| *d.borrow_mut() = Some(root.clone()));
        let rows = template_items(&App::new(None).state);
        assert_eq!(rows.iter().map(|r| r.label.to_string()).collect::<Vec<_>>(), ["Empty"], "no templates: Empty alone");
        let mut app = App::new(None);
        app.state.tempo = 97.0;
        let work = scratch("tpl_work");
        let song = work.join("song.lemon");
        app.save_to(song.clone());
        app.template_chosen(root.join("User").join("Slow.lemon"));
        assert_eq!(app.state.path.as_deref(), Some(song.as_path()), "the project keeps its file");
        assert!(root.join("User").join("Slow").join("Slow.lemon").is_file(), "{:?}", app.state.message);
        let rows = template_items(&app.state);
        assert_eq!(rows.iter().map(|r| r.label.to_string()).collect::<Vec<_>>(), ["User", "Empty"]);
        assert_eq!(rows[0].children.len(), 1);
        assert_eq!((rows[0].children[0].label.as_ref(), &rows[0].children[0].cmd), ("Slow", &Cmd::NewTemplate("User/Slow".into())));
        app.state.tempo = 140.0;
        app.save_to(song.clone());
        app.pick_new_template("User/Slow");
        assert_eq!((app.state.tempo, app.state.path.clone()), (97.0, None), "an untitled project from the template");
        assert!(!app.unsaved());
        // A template saved by the original (an FLP) is listed and opens too.
        let fl = root.join("User").join("Old");
        std::fs::create_dir_all(&fl).unwrap();
        app.state.tempo = 88.0;
        app.save_to(fl.join("Old.flp"));
        assert!(template_items(&app.state)[0].children.iter().any(|r| r.label.as_ref() == "Old"));
        app.pick_new_template("User/Old");
        assert_eq!((app.state.tempo, app.state.path.clone()), (88.0, None), "an untitled project from the FLP template");
        app.pick_new_template("Empty");
        assert_eq!((app.state.tempo, app.state.path.clone()), (130.0, None), "the built-in project");
        crate::templates::TEST_DIR.with(|d| *d.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&work);
    }

    /// Save to new project folder: `<name>/<name>.lemon` in the folder picked, the data folder there; one already there
    /// is refused.
    #[test]
    fn new_project_folder() {
        let dir = scratch("newdir");
        let mut app = App::new(None);
        app.save_to(dir.join("Tune.flp"));
        app.new_folder_chosen(dir.clone());
        let file = dir.join("Tune").join("Tune.lemon");
        assert_eq!(app.state.path.as_deref(), Some(file.as_path()), "{:?}", app.state.message);
        assert!(file.is_file());
        assert_eq!(crate::data_folder::folder(&app.state), Some(dir.join("Tune")));
        app.new_folder_chosen(dir.clone());
        assert!(app.modal_open(), "already there: refused");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Unused files: only the data folder's Samples, Recorded and Rendered, never what the project uses or project files;
    /// a folder with other projects is left alone. Yes quarantines and saves.
    #[test]
    fn unused_file_finding() {
        let dir = scratch("unused");
        for f in ["Samples/kick.wav", "Samples/old.wav", "Samples/sub/deep.wav", "Recorded/take 1.wav", "Rendered/x.flp", "Backup/song (autosaved on 1-1-2026 at 0h00).flp", "loose.wav"] {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        }
        let used = [dir.join("samples").join("KICK.wav"), dir.join("Recorded").join("other.wav")];
        let used = if cfg!(windows) { used.to_vec() } else { vec![dir.join("Samples").join("kick.wav")] };
        let found = unused_files(&dir, &used);
        assert_eq!(found, [dir.join("Recorded").join("take 1.wav"), dir.join("Samples").join("old.wav"), dir.join("Samples").join("sub").join("deep.wav")]);
        // The project in the folder, with a channel on old.wav.
        let mut app = App::new(None);
        let song = dir.join("song.flp");
        app.state.project.data_folder = Some(DataFolder { choice: DataFolder::RELATIVE, folder: String::new() });
        app.save_to(song.clone());
        // Recovery projects are part of dependency analysis. Use an actual valid old project here;
        // an unreadable backup must stop cleanup rather than make its samples appear unused.
        std::fs::copy(&song, dir.join("Backup/song (autosaved on 1-1-2026 at 0h00).flp")).unwrap();
        // A nested .flp is another project even inside Rendered: refuse cleanup until it is removed.
        assert!(shared_folder(&dir, &song));
        std::fs::remove_file(dir.join("Rendered/x.flp")).unwrap();
        if let Some(ch) = app.state.project.channels.first_mut() {
            ch.sample_path = Some(dir.join("Samples").join("old.wav").to_string_lossy().into_owned());
        }
        let left = unused_files(&dir, &app.used_files().unwrap());
        assert!(!left.contains(&dir.join("Samples").join("old.wav")) && left.contains(&dir.join("Samples").join("kick.wav")), "{left:?}");
        app.delete_unused_ask();
        assert!(app.modal_open(), "asked first");
        assert!(dir.join("Samples").join("kick.wav").is_file(), "nothing deleted before the answer");
        app.state.dialogs.clear();
        app.delete_unused_done(left.clone(), &Reply::Button(Answer::No));
        assert!(left.iter().all(|f| f.is_file()), "No: nothing deleted");
        app.delete_unused_done(left.clone(), &Reply::Button(Answer::Yes));
        assert!(left.iter().all(|f| !f.exists()));
        assert!(dir.join("Samples").join("old.wav").is_file() && dir.join("loose.wav").is_file() && song.is_file());
        assert!(!app.unsaved() && app.state.message.as_deref().is_some_and(|m| m.starts_with("Moved")), "{:?}", app.state.message);
        let recovery = std::fs::read_dir(dir.join("Unused files")).unwrap().next().unwrap().unwrap().path();
        let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(recovery.join("restore.json")).unwrap()).unwrap();
        assert_eq!(manifest["version"], 1);
        for f in &left {
            let relative = f.strip_prefix(&dir).unwrap();
            assert_eq!(std::fs::read(recovery.join(relative)).unwrap(), b"x", "quarantine preserves original bytes");
            assert!(manifest["files"].as_array().unwrap().iter().any(|r| r["original"].as_str() == Some(relative.to_string_lossy().as_ref())));
        }
        // Another project in the folder: refused, nothing deleted.
        std::fs::write(dir.join("Samples").join("again.wav"), b"x").unwrap();
        std::fs::write(dir.join("other.flp"), b"x").unwrap();
        assert!(shared_folder(&dir, &song) && !shared_folder(&dir.join("Samples"), &song));
        app.delete_unused_ask();
        assert!(app.modal_open() && dir.join("Samples").join("again.wav").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cleanup_comparison_uses_file_identity_and_normalized_paths() {
        let dir = scratch("aliases");
        let samples = dir.join("Samples");
        std::fs::create_dir_all(&samples).unwrap();
        let kick = samples.join("kick.wav");
        std::fs::write(&kick, b"this is an in-use sample").unwrap();
        let used = vec![samples.join("sub").join("..").join("kick.wav")];
        // A resolvable parent must exist for filesystem canonicalization.
        std::fs::create_dir(samples.join("sub")).unwrap();
        assert!(same_file(&kick, &used[0]).unwrap());
        assert!(!unused_files(&dir, &used).contains(&kick));
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let link = dir.join("external alias.wav");
            symlink(&kick, &link).unwrap();
            assert!(same_file(&kick, &link).unwrap());
            let hardlink = dir.join("hardlink.wav");
            std::fs::hard_link(&kick, &hardlink).unwrap();
            assert!(same_file(&kick, &hardlink).unwrap());
            assert!(!unused_files(&dir, &[link]).contains(&kick));
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn nested_projects_and_unknown_ownership_stop_cleanup() {
        let dir = scratch("nested_ownership");
        let own = dir.join("song.lemon");
        std::fs::write(&own, b"some project").unwrap();
        let nested = dir.join("nested").join("other.lemon");
        std::fs::create_dir_all(nested.parent().unwrap()).unwrap();
        std::fs::write(&nested, b"another project").unwrap();
        assert!(shared_folder(&dir, &own), "another project in a subfolder owns assets here too");
        std::fs::remove_file(nested).unwrap();
        assert!(!shared_folder(&dir, &own));
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(&dir, dir.join("uninspectable-link")).unwrap();
            assert!(shared_folder(&dir, &own), "linked directories make ownership ambiguous");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_recovery_project_blocks_cleanup() {
        let dir = scratch("unreadable_backup");
        let backup = dir.join("Backup");
        std::fs::create_dir_all(&backup).unwrap();
        let mut app = App::new(None);
        let own = dir.join("song.lemon");
        app.state.project.data_folder = Some(DataFolder { choice: DataFolder::RELATIVE, folder: String::new() });
        app.save_to(own.clone());
        std::fs::write(backup.join("song (autosaved on 1-1-2026 at 0h00).flp"), b"not an audio project").unwrap();
        let samples = dir.join("Samples");
        std::fs::create_dir_all(&samples).unwrap();
        let unused = samples.join("unused.wav");
        std::fs::write(&unused, b"keep").unwrap();
        assert!(app.cleanup_candidates(&dir, &own).is_err());
        assert!(unused.exists(), "a bad recovery generation must never cause deletion");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The rows that need a saved project are greyed out until it is saved; Save as template... and New from
    /// template ▸ are not.
    #[test]
    fn rows_need_a_saved_project() {
        use crate::views::menus::{active, item_at, DELETE_UNUSED_LABEL, TOOLS_MENU};
        let mut app = App::new(None);
        let row = |menu: usize, label: &str| (0..200).find_map(|i| item_at(menu, i).filter(|it| it.label == label)).unwrap();
        let rows = [(0, "Restore backup..."), (0, "Save to new project folder..."), (0, DELETE_UNUSED_LABEL), (TOOLS_MENU, DELETE_UNUSED_LABEL)];
        for (m, l) in rows {
            assert!(!active(&app.state, m, row(m, l)), "{l}: never saved");
        }
        assert!(active(&app.state, 0, row(0, "Save as template...")) && active(&app.state, 0, row(0, "New from template")));
        app.state.path = Some(std::env::temp_dir().join("song.flp"));
        for (m, l) in rows {
            assert!(active(&app.state, m, row(m, l)), "{l}: saved");
        }
    }
}
