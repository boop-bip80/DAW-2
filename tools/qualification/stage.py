#!/usr/bin/env python3
# Copyright (C) 2026 Lemon Studio contributors
# SPDX-License-Identifier: GPL-3.0-or-later
# This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

"""Stage an unsigned CI release candidate; verify its modules, licenses and hashes.

The archive is a portable *candidate*, not a clean-machine installer or a signed
release. `owned_workflow.py` must exercise the executable extracted from the archive before
approval for any publication can be considered. The external, hardware and
signing gates remain separate.
"""

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import platform as host_platform
import re
import shutil
import subprocess
import sys
import tarfile
import tomllib
import zipfile


ROOT = Path(__file__).resolve().parents[2]
PLUGINS = ROOT / "tools/bundle-plugins/src/main.rs"
LICENSES = ("LICENSE", "THIRD-PARTY-NOTICES.txt")
TOOLCHAIN = "1.99.1"


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def expected_plugins():
    # Parse only the bundler's PLUGINS table; comments or unrelated data must not count.
    source = PLUGINS.read_text(encoding="utf-8")
    table = source[source.index("const PLUGINS:"):].split("];", 1)[0]
    names = re.findall(r'\("[a-z0-9-]+",\s*"[a-z0-9_]+",\s*"([^"]+)"\)', table)
    if len(names) != 25 or len(set(names)) != len(names):
        raise ValueError("the plugin bundler's 25 entries changed; review the release qualification before publishing")
    return sorted(names)


def pinned_toolchain():
    with (ROOT / "rust-toolchain.toml").open("rb") as source:
        version = tomllib.load(source)["toolchain"]["channel"]
    if version != TOOLCHAIN:
        raise ValueError(f"toolchain pin {version} differs from the audited CI pin {TOOLCHAIN}; review both")
    return version


def copy(source, target, preserve_macos_bundle=False):
    if not source.exists():
        raise FileNotFoundError(f"missing release input: {source}")
    if source.is_symlink():
        raise ValueError(f"refusing to package an unverified symlink: {source}")
    if source.is_dir():
        if preserve_macos_bundle:
            # `ditto` preserves code-signature-related resource forks and xattrs on macOS.
            subprocess.run(["ditto", str(source), str(target)], check=True)
        else:
            shutil.copytree(source, target, symlinks=True)
    else:
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)


def native_module(path, platform):
    """Reject placeholder/incorrect-platform executables and plugin libraries."""
    if not path.is_file() or path.stat().st_size < 4096:
        raise ValueError(f"missing/trivial native module: {path}")
    with path.open("rb") as stream:
        signature = stream.read(4)
    formats = {
        "linux": (b"\x7fELF",),
        "windows": (b"MZ",),
        "macos": (b"\xcf\xfa\xed\xfe", b"\xfe\xed\xfa\xcf", b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca"),
    }
    if not any(signature.startswith(magic) for magic in formats[platform]):
        raise ValueError(f"wrong native module type for {platform}: {path} ({signature!r})")


def validate_plugins(folder, platform, names):
    entries = {p.name for p in folder.iterdir()}
    expected = {f"{name}.{ext}" for name in names for ext in ("vst3", "clap")}
    if entries != expected:
        raise ValueError(f"plugin inventory mismatch: missing={sorted(expected - entries)} unexpected={sorted(entries - expected)}")
    host_arch = host_platform.machine().lower()
    if platform != "macos" and host_arch not in ("amd64", "x86_64", "arm64", "aarch64"):
        raise ValueError(f"unsupported candidate architecture {host_arch}; review release staging")
    arm = host_arch in ("arm64", "aarch64")
    arch = ("arm64-win" if arm else "x86_64-win") if platform == "windows" else ("aarch64-linux" if arm else "x86_64-linux")
    for name in names:
        vst3 = folder / f"{name}.vst3"
        if platform == "macos":
            vst3_binary = vst3 / "Contents/MacOS" / name
            clap_binary = folder / f"{name}.clap/Contents/MacOS" / name
        else:
            suffix = ".vst3" if platform == "windows" else ".so"
            vst3_binary = vst3 / "Contents" / arch / f"{name}{suffix}"
            clap_binary = folder / f"{name}.clap"
        native_module(vst3_binary, platform)
        native_module(clap_binary, platform)


def inventory(payload):
    contents = []
    for path in sorted(payload.rglob("*")):
        if path.is_symlink():
            raise ValueError(f"unexpected symlink in candidate: {path}")
        if not path.is_file():
            continue
        name = path.relative_to(payload).as_posix()
        if name in ("SHA256SUMS", "BUILD-MANIFEST.json"):
            continue
        contents.append({"path": name, "bytes": path.stat().st_size, "sha256": sha256(path)})
    return contents


def checksums(contents):
    return "".join(f"{item['sha256']}  {item['path']}\n" for item in contents)


def verify_macos_signatures(payload, names):
    if sys.platform != "darwin":
        raise ValueError("signed macOS candidates must be qualified on a macOS runner")
    bundles = [payload / "Lemon Studio.app"]
    bundles.extend(payload / "Plugins" / f"{name}.{extension}" for name in names for extension in ("vst3", "clap"))
    for bundle in bundles:
        checked = subprocess.run(["codesign", "--verify", "--verbose=2", str(bundle)], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        if checked.returncode:
            raise ValueError(f"invalid ad-hoc code signature in {bundle}: {checked.stdout}")


def stage(args):
    actual_commit = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, check=True,
                                   capture_output=True, text=True).stdout.strip()
    if args.commit != actual_commit:
        raise ValueError(f"claimed source commit {args.commit} != checked-out HEAD {actual_commit}")
    platform = args.platform
    if platform == "macos" and sys.platform != "darwin":
        raise ValueError("macOS candidates require the native runner to preserve and verify bundle signatures")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    payload = output / "payload"
    payload.mkdir()
    for source in LICENSES:
        copy(ROOT / source, payload / source)
    (payload / "RELEASE-STATUS.txt").write_text(
        "LEMON STUDIO: CI RELEASE CANDIDATE (NOT A PRODUCTION RELEASE)\n"
        "CI tests the executable from this staged candidate; successful CI may upload it as a\n"
        "short-lived GitHub Actions artifact, NOT as a vetted production release.\n"
        "Not covered: physical audio/MIDI hardware, licensed compatibility corpora, third-party plugin\n"
        "matrix, independent clean-machine installation, trusted signing/notarization and user acceptance.\n"
        "macOS app/plugin signatures are AD-HOC ONLY, not Developer ID distribution signatures.\n"
        "Linux packages are portable binaries and desktop assets, not .deb/.rpm/AppImage installers.\n"
        "Windows packages are portable binaries, not MSIX/MSI installers.\n",
        encoding="utf-8",
    )
    (payload / "SOURCE-COMMIT.txt").write_text(
        f"Source: https://github.com/boop-bip80/DAW-2\nCommit: {args.commit}\n"
        "For a public GPL release, keep a corresponding source archive permanently available.\n",
        encoding="utf-8",
    )

    if platform == "macos":
        # Keep the CI `--binary` contract meaningful even though codesign changes the copy's hash.
        native_module(args.binary, platform)
        app = payload / "Lemon Studio.app"
        copy(args.bundled / "Lemon Studio.app", app, preserve_macos_bundle=True)
        binary = app / "Contents/MacOS/lemonstudio"
        # App signing changes the Mach-O executable bytes, so it cannot hash-equal the unsigned
        # `target/release/lemonstudio` used by bundle-app before codesign. Verify its code signature below.
        for source in LICENSES:
            if sha256(app / "Contents/Resources" / source) != sha256(ROOT / source):
                raise ValueError(f"macOS standalone .app missing current {source} in signed resources")
    else:
        binary = payload / "bin" / ("lemonstudio.exe" if platform == "windows" else "lemonstudio")
        copy(args.binary, binary)
        if platform == "linux":
            assets = ROOT / "crates/os-app/assets"
            copy(assets / "lemonstudio.png", payload / "share/icons/hicolor/256x256/apps/lemonstudio.png")
            copy(assets / "lemonstudio.desktop", payload / "share/applications/lemonstudio.desktop")
            copy(assets / "lemonstudio-mime.xml", payload / "share/mime/packages/lemonstudio-mime.xml")
        else:
            copy(ROOT / "crates/os-app/assets/lemonstudio.ico", payload / "share/lemonstudio.ico")
    native_module(binary, platform)

    names = expected_plugins()
    plugindir = payload / "Plugins"
    plugindir.mkdir()
    for name in names:
        for extension in ("vst3", "clap"):
            candidate = f"{name}.{extension}"
            copy(args.bundled / candidate, plugindir / candidate, preserve_macos_bundle=platform == "macos")
    validate_plugins(plugindir, platform, names)

    contents = inventory(payload)
    manifest = {
        "schema": 1,
        "status": "unsigned-ci-candidate; production-go-pending",
        "commit": args.commit,
        "platform": platform,
        "host_architecture": host_platform.machine(),
        "rust_toolchain": pinned_toolchain(),
        "rustc_version_verbose": subprocess.run(["rustc", "-Vv"], check=True, capture_output=True, text=True).stdout.strip(),
        "cargo_version": subprocess.run(["cargo", "-V"], check=True, capture_output=True, text=True).stdout.strip(),
        "cargo_lock_sha256": sha256(ROOT / "Cargo.lock"),
        "binary": binary.relative_to(payload).as_posix(),
        "built_in_plugin_count": len(names),
        "plugin_format_count": 2 * len(names),
        "files": contents,
        "exclusions": ["clean-machine-installer", "trusted-code-signing-or-notarization", "external-compatibility-corpora", "physical-audio-and-midi", "third-party-plugin-validation"],
    }
    (payload / "BUILD-MANIFEST.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    (payload / "SHA256SUMS").write_text(checksums(contents), encoding="utf-8")

    stem = f"lemonstudio-{platform}-{args.commit[:12]}-ci-candidate"
    if platform == "macos":
        archive = output / (stem + ".zip")
        # Ditto archives retain macOS extended attributes; Python tarfile and zipfile do not.
        # Keep the .app at the archive root, as a Finder-openable portable candidate.
        subprocess.run(["ditto", "-c", "-k", "--sequesterRsrc", str(payload), str(archive)], check=True)
    elif platform == "windows":
        archive = output / (stem + ".zip")
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as z:
            for path in sorted(payload.rglob("*")):
                if path.is_file():
                    z.write(path, "Lemon Studio/" + path.relative_to(payload).as_posix())
    else:
        archive = output / (stem + ".tar.gz")
        with tarfile.open(archive, "w:gz") as tar:
            tar.add(payload, arcname="Lemon Studio", recursive=True)
    digest = sha256(archive)
    (output / (archive.name + ".sha256")).write_text(f"{digest}  {archive.name}\n", encoding="utf-8")
    (output / "stage-result.json").write_text(json.dumps({
        "status": "staged-unqualified", "platform": platform, "archive": archive.name,
        "archive_sha256": digest, "binary": manifest["binary"], "plugin_count": len(names),
    }, indent=2) + "\n", encoding="utf-8")
    verify(output)
    print(json.dumps({"archive": str(archive), "archive_sha256": digest, "staged_binary": str(binary)}, indent=2))


def verify(output, owned_result=None):
    payload = output.resolve() / "payload"
    manifest = json.loads((payload / "BUILD-MANIFEST.json").read_text(encoding="utf-8"))
    if manifest["rust_toolchain"] != pinned_toolchain():
        raise ValueError("candidate manifest differs from pinned Rust toolchain")
    if manifest["cargo_lock_sha256"] != sha256(ROOT / "Cargo.lock"):
        raise ValueError("candidate manifest differs from checked-out Cargo.lock")
    actual = inventory(payload)
    if manifest["files"] != actual or (payload / "SHA256SUMS").read_text(encoding="utf-8") != checksums(actual):
        raise ValueError("release candidate manifest/checksums do not match packaged files")
    native_module(payload / manifest["binary"], manifest["platform"])
    names = expected_plugins()
    if manifest["built_in_plugin_count"] != len(names) or manifest["plugin_format_count"] != 2 * len(names):
        raise ValueError("candidate plugin counts disagree with the bundle source table")
    validate_plugins(payload / "Plugins", manifest["platform"], names)
    if manifest["platform"] == "macos":
        verify_macos_signatures(payload, names)
    for source in LICENSES:
        if sha256(payload / source) != sha256(ROOT / source):
            raise ValueError(f"release candidate {source} differs from source checkout")
    result_path = output / "stage-result.json"
    result = json.loads(result_path.read_text(encoding="utf-8"))
    archive = output / result["archive"]
    if sha256(archive) != result["archive_sha256"]:
        raise ValueError("outer archive checksum mismatch")
    if (output / (archive.name + ".sha256")).read_text(encoding="utf-8") != f"{result['archive_sha256']}  {archive.name}\n":
        raise ValueError("outer .sha256 sidecar mismatch")
    if owned_result is not None:
        extracted = json.loads((output / "archive-qualification.json").read_text(encoding="utf-8"))
        qualified = json.loads(owned_result.read_text(encoding="utf-8"))
        if extracted["binary_sha256"] != qualified["binary_sha256"]:
            raise ValueError("smoke-tested executable differs from extracted archive")
    print(f"Verified {len(actual)} files and {manifest['plugin_format_count']} native plugin modules: {payload}")



def unpack(output):
    """Extract the distributable and verify every file before its binary is tested."""
    output = output.resolve(strict=True)
    verify(output)
    payload = output / "payload"
    manifest = json.loads((payload / "BUILD-MANIFEST.json").read_text(encoding="utf-8"))
    result = json.loads((output / "stage-result.json").read_text(encoding="utf-8"))
    platform = manifest["platform"]
    archive = output / result["archive"]
    extraction = output / "unpacked"
    extraction.mkdir(exist_ok=False)

    def safe_member(name):
        path = PurePosixPath(name.replace("\\", "/"))
        if not path.parts or path.is_absolute() or ".." in path.parts or ":" in path.parts[0]:
            raise ValueError(f"unsafe archive member path: {name}")

    if platform == "macos":
        # Ditto preserves resource forks and extended attributes in signed bundles.
        subprocess.run(["ditto", "-x", "-k", str(archive), str(extraction)], check=True)
        root = extraction
    elif platform == "windows":
        with zipfile.ZipFile(archive) as packaged:
            for entry in packaged.infolist():
                safe_member(entry.filename)
            packaged.extractall(extraction)
        root = extraction / "Lemon Studio"
    elif platform == "linux":
        with tarfile.open(archive, "r:gz") as packaged:
            for entry in packaged.getmembers():
                safe_member(entry.name)
                if not (entry.isfile() or entry.isdir()):
                    raise ValueError(f"unexpected link/device archive member: {entry.name}")
            packaged.extractall(extraction, filter="data")
        root = extraction / "Lemon Studio"
    else:
        raise ValueError(f"unknown candidate platform {platform}")

    if not root.is_dir():
        raise ValueError("archive lacks the documented candidate root")
    if inventory(root) != inventory(payload):
        raise ValueError("extracted archive differs from staged hash-verified payload")
    for name in ("BUILD-MANIFEST.json", "SHA256SUMS"):
        if sha256(root / name) != sha256(payload / name):
            raise ValueError(f"extracted archive {name} differs from staged payload")
    binary = root / manifest["binary"]
    native_module(binary, platform)
    if platform == "linux" and not binary.stat().st_mode & 0o111:
        raise ValueError("extracted Linux executable lost its execute permission")
    names = expected_plugins()
    validate_plugins(root / "Plugins", platform, names)
    if platform == "macos":
        verify_macos_signatures(root, names)
    attestation = {
        "status": "passed",
        "archive": archive.name,
        "archive_sha256": sha256(archive),
        "extracted_binary": binary.relative_to(output).as_posix(),
        "binary_sha256": sha256(binary),
        "verified_files": len(inventory(root)) + 2,
        "native_plugin_modules": 2 * len(names),
    }
    (output / "archive-qualification.json").write_text(json.dumps(attestation, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(attestation, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    staging = sub.add_parser("stage")
    staging.add_argument("--platform", choices=("linux", "windows", "macos"), required=True)
    staging.add_argument("--binary", type=Path, required=True)
    staging.add_argument("--bundled", type=Path, required=True)
    staging.add_argument("--output", type=Path, required=True, help="a directory that must not yet exist")
    staging.add_argument("--commit", required=True, help="the full Git commit of the candidate sources")
    verifying = sub.add_parser("verify")
    verifying.add_argument("--output", type=Path, required=True)
    verifying.add_argument("--owned-result", type=Path)
    unpacking = sub.add_parser("unpack")
    unpacking.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "stage":
        if not re.fullmatch(r"[a-f0-9]{40}", args.commit):
            parser.error("--commit must be a 40-character lowercase Git commit SHA")
        stage(args)
    elif args.command == "verify":
        verify(args.output, args.owned_result)
    else:
        unpack(args.output)


if __name__ == "__main__":
    main()
