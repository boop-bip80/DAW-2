// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! `cargo run -p package -- --platform <linux|macos|windows> [--profile debug|release] [--out dist]` stages the
//! package a release ships: the program, the plugins bundled for other DAWs, the release notes and the platform's own
//! entry (the desktop entry and icon on Linux, the app bundle on macOS, the icon on Windows). The release workflow runs
//! this, so what CI packages is what a checkout stages here.
//!
//! `cargo run -p package -- check --platform <platform> --dir <staged package>` then verifies that layout, and exits
//! non-zero when an artifact a beta download needs is missing. It reads the staged files, never a fixture, so a broken
//! build output fails before a tag instead of at download time.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const LINUX: &str = "lemon-studio-linux-x64";
const MACOS: &str = "lemon-studio-macos";
const WINDOWS: &str = "lemon-studio-windows-x64";

const NOTES: &str = "RELEASE-NOTES.md";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Platform {
    Linux,
    MacOs,
    Windows,
}

impl Platform {
    fn parse(text: &str) -> Result<Self, String> {
        match text {
            "linux" => Ok(Self::Linux),
            "macos" => Ok(Self::MacOs),
            "windows" => Ok(Self::Windows),
            other => Err(format!("unknown platform {other:?}: linux, macos or windows")),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Linux => "Linux",
            Self::MacOs => "macOS",
            Self::Windows => "Windows",
        }
    }

    fn folder(self) -> &'static str {
        match self {
            Self::Linux => LINUX,
            Self::MacOs => MACOS,
            Self::Windows => WINDOWS,
        }
    }

    /// Where the program sits inside the package.
    fn program(self) -> &'static str {
        match self {
            Self::MacOs => "Lemon Studio.app/Contents/MacOS/lemonstudio",
            Self::Windows => "lemonstudio.exe",
            Self::Linux => "lemonstudio",
        }
    }

    /// The platform's own entry points, which a package without them is not runnable.
    fn extras(self) -> &'static [&'static str] {
        match self {
            Self::Linux => &["lemonstudio.desktop", "lemonstudio.png"],
            Self::MacOs => &["Lemon Studio.app/Contents/Info.plist"],
            Self::Windows => &["lemonstudio.ico"],
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("package: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("package [check] --platform <linux|macos|windows> [--profile debug|release] [--out dist] [--dir <package>]");
        return Ok(());
    }
    let check = args.first().is_some_and(|a| a == "check");
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let platform = Platform::parse(&flag("--platform").unwrap_or_else(|| "linux".into()))?;
    if check {
        let dir = PathBuf::from(flag("--dir").unwrap_or_else(|| Path::new("dist").join(platform.folder()).display().to_string()));
        return verify(platform, &dir);
    }
    let profile = flag("--profile").unwrap_or_else(|| "release".into());
    if profile != "release" && profile != "debug" {
        return Err(format!("unknown profile {profile:?}: debug or release"));
    }
    let out = PathBuf::from(flag("--out").unwrap_or_else(|| "dist".into()));
    let dir = stage(platform, &profile, &out)?;
    println!("package: {} staged in {}", platform.name(), dir.display());
    // A package is only worth archiving once it passes the same checks a download would.
    verify(platform, &dir)
}

/// Copies the real build output into `out/<package>` and returns that directory.
fn stage(platform: Platform, profile: &str, out: &Path) -> Result<PathBuf, String> {
    let target = Path::new("target").join(profile);
    let dir = out.join(platform.folder());
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let missing = |what: &str, from: &Path| {
        format!("{what} is missing at {}: build it first (cargo build --workspace --{} --locked, and cargo run -p bundle-plugins --locked for the plugins)", from.display(), profile)
    };

    // The program, or the app bundle that carries it.
    let exe = if platform == Platform::Windows { "lemonstudio.exe" } else { "lemonstudio" };
    let program = target.join(exe);
    if platform == Platform::MacOs {
        // The app bundle already holds the program, its icon and its Info.plist.
        let bundle = Path::new("target/bundled").join("Lemon Studio.app");
        if !bundle.is_dir() {
            return Err(missing("the app bundle", &bundle));
        }
        copy_tree(&bundle, &dir.join("Lemon Studio.app"))?;
    } else {
        if !program.is_file() {
            return Err(missing("the program", &program));
        }
        copy_file(&program, &dir.join(exe))?;
        if platform == Platform::Linux {
            copy_file(Path::new("crates/os-app/assets/lemonstudio.desktop"), &dir.join("lemonstudio.desktop"))?;
            copy_file(Path::new("crates/os-app/assets/lemonstudio.png"), &dir.join("lemonstudio.png"))?;
        } else {
            copy_file(Path::new("crates/os-app/assets/lemonstudio.ico"), &dir.join("lemonstudio.ico"))?;
        }
    }

    // The plugin bundles other DAWs load.
    let plugins = Path::new("target/bundled");
    if !plugins.is_dir() {
        return Err(missing("the plugin bundles", plugins));
    }
    copy_plugins(plugins, &dir.join("plugins"))?;

    // What the release says, in the package itself.
    copy_file(Path::new("docs/release-notes.md"), &dir.join(NOTES))?;
    Ok(dir)
}

/// Verifies a staged package holds what a download needs; every miss is named.
fn verify(platform: Platform, dir: &Path) -> Result<(), String> {
    let mut missing: Vec<String> = Vec::new();
    let shown = |p: &Path| p.display().to_string();

    let program = dir.join(platform.program());
    if !program.is_file() {
        missing.push(format!("the program ({})", shown(&program)));
    } else if platform != Platform::Windows && !is_executable(&program) {
        missing.push(format!("the program's execute bit ({})", shown(&program)));
    }
    for extra in platform.extras() {
        let path = dir.join(extra);
        if !path.is_file() {
            missing.push(format!("{extra}"));
        }
    }

    let notes = dir.join(NOTES);
    match std::fs::read_to_string(&notes) {
        Ok(text) if text.contains("Known-risky areas") => {}
        Ok(_) => missing.push(format!("{NOTES} without the risky areas")),
        Err(_) => missing.push(format!("{NOTES} (the release summary)")),
    }

    let plugins = dir.join("plugins");
    let mut vst3 = 0;
    let mut clap = 0;
    match std::fs::read_dir(&plugins) {
        Ok(entries) => {
            for entry in entries.flatten() {
                match entry.path().extension().and_then(|e| e.to_str()) {
                    Some("vst3") => vst3 += 1,
                    Some("clap") => clap += 1,
                    _ => {}
                }
            }
            if vst3 == 0 {
                missing.push("plugins/*.vst3 (the plugin bundles for other DAWs)".into());
            }
            if clap == 0 {
                missing.push("plugins/*.clap (the plugin bundles for other DAWs)".into());
            }
        }
        Err(_) => missing.push("plugins/ (the plugin bundles for other DAWs)".into()),
    }

    if missing.is_empty() {
        println!(
            "package: {} holds the program, its entry, the release notes and {} plugin bundles ({vst3} VST3, {clap} CLAP) in {}",
            platform.name(),
            vst3 + clap,
            dir.display()
        );
        return Ok(());
    }
    let mut text = format!("{} is not a package a download can be made of:\n", dir.display());
    for item in &missing {
        text.push_str(&format!("  missing {item}\n"));
    }
    Err(text.trim_end().to_string())
}

fn copy_file(from: &Path, to: &Path) -> Result<(), String> {
    if !from.is_file() {
        return Err(format!("{} is missing", from.display()));
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::copy(from, to).map_err(|e| format!("{} -> {}: {e}", from.display(), to.display()))?;
    Ok(())
}

/// The plugin bundles only: other things `target/bundled` may hold (an app bundle, say) are not plugins.
fn copy_plugins(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let mut copied = 0;
    for entry in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?.flatten() {
        let path = entry.path();
        if matches!(path.extension().and_then(|e| e.to_str()), Some("vst3" | "clap")) {
            copy_tree(&path, &to.join(entry.file_name()))?;
            copied += 1;
        }
    }
    if copied == 0 {
        return Err(format!("no plugin bundles in {}", from.display()));
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    if from.is_dir() {
        std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
        for entry in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?.flatten() {
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        return Ok(());
    }
    copy_file(from, to)
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_: &Path) -> bool {
    true
}
