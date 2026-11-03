// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Publish complete files from a flushed sibling, without exposing a partly written project.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Pending(PathBuf);

impl Drop for Pending {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Replace `target` atomically. An error before publication leaves the old file untouched.
pub fn replace(target: &Path, bytes: &[u8]) -> io::Result<()> {
    publish(target, false, |f| f.write_all(bytes))
}

/// Stream a replacement into a sibling; the previous destination remains intact until publication.
pub fn replace_with(target: &Path, write: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
    publish(target, false, write)
}

/// Publish a new file, failing if its name is already occupied. No existing generation is replaced.
pub fn create(target: &Path, bytes: &[u8]) -> io::Result<()> {
    publish(target, true, |f| f.write_all(bytes))
}

/// Stream a new file into a sibling and publish it only after the writer and its flush succeed.
pub fn create_with(target: &Path, write: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
    publish(target, true, write)
}

/// Copy a previous project into a new, durable backup generation without replacing another backup.
pub fn copy_new(source: &Path, target: &Path) -> io::Result<()> {
    publish(target, true, |out| {
        let mut input = File::open(source)?;
        io::copy(&mut input, out).map(|_| ())
    })
}

fn publish(target: &Path, new: bool, write: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
    if new && target.try_exists()? {
        return Err(io::ErrorKind::AlreadyExists.into());
    }
    let parent = target.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let name = target.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "a file name is required"))?;
    let (pending, mut file) = loop {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let mut temp = name.to_os_string();
        temp.push(format!(".{}.{}.saving", std::process::id(), n));
        let path = parent.join(temp);
        match OpenOptions::new().read(true).write(true).create_new(true).open(&path) {
            Ok(f) => break (Pending(path), f),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    write(&mut file)?;
    file.flush()?;
    if !new {
        if let Ok(metadata) = std::fs::metadata(target) {
            file.set_permissions(metadata.permissions())?;
        }
    }
    file.sync_all()?;
    drop(file);
    if new {
        publish_new(&pending.0, target)?;
    } else {
        std::fs::rename(&pending.0, target)?;
    }
    // Persist the directory entry as well as the contents on systems that support directory fsync.
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

/// Prefer the platform's exclusive rename, including on volumes that do not support hard links. The fallback
/// still creates atomically; no path ever falls back to a rename that could overwrite another generation.
fn publish_new(source: &Path, target: &Path) -> io::Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::ffi::OsStrExt;
        let from = std::ffi::CString::new(source.as_os_str().as_bytes()).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let to = std::ffi::CString::new(target.as_os_str().as_bytes()).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        // SAFETY: both paths are live NUL-terminated buffers; the call retains neither pointer.
        #[cfg(target_os = "linux")]
        let result = unsafe { libc::syscall(libc::SYS_renameat2, libc::AT_FDCWD, from.as_ptr(), libc::AT_FDCWD, to.as_ptr(), libc::RENAME_NOREPLACE) };
        #[cfg(target_os = "macos")]
        let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
        if result == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if !matches!(error.raw_os_error(), Some(libc::ENOSYS | libc::EINVAL | libc::ENOTSUP)) {
            return Err(error);
        }
        std::fs::hard_link(source, target)
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};
        let from: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: live NUL-terminated paths; omitting REPLACE_EXISTING gives exclusive publication.
        if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    std::fs::hard_link(source, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("lemon_durable_{}_{}_{}", std::process::id(), tag, NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&p).unwrap();
        p
    }

    #[test]
    fn partial_write_failure_preserves_original_and_removes_pending_file() {
        let dir = dir("partial");
        let target = dir.join("song.lemon");
        std::fs::write(&target, b"known good project").unwrap();
        let error = publish(&target, false, |f| {
            f.write_all(b"incomplete")?;
            Err(io::Error::other("injected disk full"))
        })
        .unwrap_err();
        assert!(error.to_string().contains("disk full"));
        assert_eq!(std::fs::read(&target).unwrap(), b"known good project");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn complete_replacement_and_exclusive_generation() {
        let dir = dir("replace");
        let target = dir.join("song.lemon");
        create(&target, b"one").unwrap();
        assert_eq!(create(&target, b"collision").unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&target).unwrap(), b"one");
        replace(&target, b"two complete").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"two complete");
        let copied = dir.join("backup.lemon");
        copy_new(&target, &copied).unwrap();
        assert!(copy_new(&target, &copied).is_err());
        assert_eq!(std::fs::read(copied).unwrap(), b"two complete");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rename_failure_preserves_destination() {
        let dir = dir("rename");
        let target = dir.join("directory.lemon");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("keep"), b"original").unwrap();
        assert!(replace(&target, b"cannot replace a directory").is_err());
        assert_eq!(std::fs::read(target.join("keep")).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn streamed_failure_never_exposes_a_partial_destination() {
        let dir = dir("stream");
        let target = dir.join("recording.wav");
        let result = create_with(&target, |out| {
            out.write_all(b"partial header and audio")?;
            assert!(!target.exists());
            Err(io::Error::other("injected conversion failure"))
        });
        assert!(result.is_err());
        assert!(!target.exists());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
