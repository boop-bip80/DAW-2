# Production-readiness and release qualification

**Decision: NO-GO for a post-beta production release.** This document describes the release contract for
[`boop-bip80/DAW-2`](https://github.com/boop-bip80/DAW-2), not a past green badge or test run of the upstream
Lemon Studio repository. The release pipeline added on this stabilization branch must itself pass on the exact
candidate commit before any technical result is described as verified.

**Evidence status (2026-11-03):** stabilization is committed locally and the synthetic Rust workspace test profile
passed locally on Linux (the loopback-socket test was skipped because the container forbids local socket binding).
The new three-platform CI qualification, archive checks and signed/installer end-to-end tests have **not** been
observed passing on GitHub Actions. No external licensed compatibility fixtures, physical-device session logs,
clean-machine acceptance results, trusted signatures or notarization tickets have been supplied. Green CI on the
upstream repository would not change this fork's status.

## The fastest rigorous path: three coordinated release moves

### Move 1 — Freeze correctness on a candidate commit

1. Finish and review the current project-integrity, transport/render, plugin-state and audio-take stabilization
   changes together. Do not silently introduce incompatible native project format changes after the freeze.
2. Require **all three** rows of this fork's `.github/workflows/ci.yml` on the **same commit** to pass:
   `cargo build --workspace --all-targets --locked`, `cargo test --workspace --locked`, release builds, plugin
   bundles and the staged executable qualification below. Confirm `rust-toolchain.toml` matches CI (1.99.1).
3. Treat a skipped fixture as a *missing qualification*, never a pass. The built-in synthetic fixture is mandatory
   and uses a new private test project, not an imported commercial project or any user's project.

### Move 2 — Qualify the exact candidate package users would exercise

CI stages a platform-specific portable candidate from built native modules. Its staging script checks the binary
header, every expected VST3/CLAP module (25 plugins × 2 formats), bundled license/notices, corresponding source
commit, SHA-256 inventory and archive checksum. Linux's staged assets include the desktop entry and MIME XML for
`.lemon` and `.flp`. The macOS app carries `LICENSE` and `THIRD-PARTY-NOTICES.txt` *inside* its signed resource seal
and advertises both extensions to Finder. Nested Cargo builds all use `--locked`.

The owned qualification invokes **the binary inside the stage**, never an unrelated debug binary, then requires:

| Exercise | Machine-verifiable acceptance |
|---|---|
| Native project | Create a synthetic sample/channel/pattern/playlist, save a real `.lemon` with its native header, and cold-open in a second process |
| Mixer persistence | All three EQ bands' frequency, gain and width survive the cold reopen unchanged |
| Audio export | Render a finite, stereo 48-kHz float WAV with nonzero peak; second independent render must be byte-identical |
| Retained recording | Recover a `.wav.part` with a deliberately torn journal; preserve the existing destination and use a distinct recovered filename |
| Packaging | Verify each staged module's native binary type, all 50 plugin-format entries, GPL/notices copies, archive and payload checksums |

Commands for reproducing the Linux release candidate from a source checkout, after building app and bundles:

```bash
cargo test --workspace --locked
cargo build --release -p os-app --bin lemonstudio --locked
cargo run -p bundle-plugins --locked
python tools/qualification/stage.py stage --platform linux --binary target/release/lemonstudio \
  --bundled target/bundled --output /tmp/lemon-release-candidate --commit "$(git rev-parse HEAD)"
python tools/qualification/owned_workflow.py \
  --binary /tmp/lemon-release-candidate/payload/bin/lemonstudio \
  --output /tmp/lemon-owned-result
python tools/qualification/stage.py verify --output /tmp/lemon-release-candidate
```

The two output directories above must not already exist. On Windows and macOS, CI selects the appropriate staged
`.exe` or `.app/Contents/MacOS/lemonstudio` executable and package format. On macOS the pipeline also verifies
that the **ad-hoc** app signature survived staging; that signature is not distribution trust.

The owner authorized publishing the stabilization branch and short-lived test artifacts on 2026-11-02/03.
After the staged smoke and hash checks pass, CI uploads **portable candidate archives, `.sha256` sidecars, the
staging report and owned synthetic-workflow report/logs** as a GitHub Actions artifact named
`lemon-<platform>-<commit>`, retained for 14 days. These artifacts are publicly downloadable to people with
repository access, are unsigned (macOS: only ad-hoc-signed), and **are not GitHub Releases or vetted distributions**.
No external licensed project corpus, user recordings, private credentials or third-party compatibility fixtures are
included. A failing job must not upload a candidate as successful.

### Move 3 — Close external go/no-go gates, sign and distribute

The following are **independently mandatory**; neither a passing unit test nor the owned synthetic smoke can
substitute for them:

| Gate | Release-blocking evidence required | Current verdict |
|---|---|---|
| Real FL Studio compatibility | Licensed `.flp` project corpus + original/resaved pairs across supported historical versions; all selected semantic parts equal. `legacy_pairs.rs` now fails on mismatches, not just parse failures | **No-go: fixtures/results absent** |
| Audio/FX parity | `OS_FX_NULL`, `OS_PC_NULL`, `OS_FC_NULL` `.case` fixture directories and independently reviewed tolerances/baselines; documented audible edge cases | **No-go: fixtures/results absent** |
| Real audio devices | Record/replay/export/monitor on supported WASAPI shared/exclusive and ASIO, Linux ALSA (and PipeWire/PulseAudio through its documented compatibility path), and Core Audio combinations. JACK-specific mode is explicitly deferred until implemented/qualified. Include 44.1/48/96 kHz, 64/128/256-frame buffers where supported, MIDI loopback, device loss/recovery, at least 60-minute sessions, underrun counts and log hashes | **No-go: device matrix absent** |
| Data integrity | Repeated forced-exit and low-disk/permission-failure saves; independently cold-open last committed `.lemon`; prove existing `.flp`, earlier `.lemon`, backups and WAV takes are preserved. Verify backup rotation, plugin state and mid-recording journals | **No-go: adversarial acceptance absent** |
| Long-form export in all advertised formats | Prove at least 60-minute song/stem exports at supported 44.1/48/96-kHz settings to WAV, FLAC, MP3 and OGG, including sample-rate conversion, non-finite/length checks, tail behavior and failure rollback. Today only native-rate 32-bit float WAV streams; buffered codec/resample paths stop at a 16-million-float limit (about 2.9 minutes of stereo audio at 48 kHz), and multi-file publication is not crash-atomic | **No-go: streaming codec/resample and crash recovery incomplete** |
| Third-party plugin matrix | Representative VST3, CLAP and VST2 plugins on each OS (with lawful redistributable test instruments), editor lifecycle, parameter automation, preset/state restoration, PDC, offline export, save/cold-open. Document crash and failure matrix | **No-go: compatibility matrix absent** |
| Plugin crash safety policy | Explicit supported risk model for untrusted native plugins. Windows SEH catches only some in-process faults; Unix executes foreign code directly, without crash recovery. For an unqualified third-party-plugin stability promise, implement out-of-process isolation first | **No-go for generalized plugin crash containment** |
| Install and trust | Independent clean-machine install/uninstall and file associations on all three OSes, dependency/SBOM review, durable GPL Corresponding Source availability, reviewed complete third-party notices; Developer ID + notarization for macOS and appropriate Windows signing/reputation work | **No-go: portable unsigned candidate only** |
| Release provenance | Immutable tag/commit, reviewed changelog, manifest/hashes, corresponding source archive, published checksums and signatures, security/contact/rollback instructions, authorized artifact destination | **No-go: not published/approved** |

Run the separate fail-closed external fixture profile only once all licensed fixture environment variables are
available. They are deliberately **not copied to the repository or a public CI artifact**:

```bash
# Required: OPENSTUDIO_FLP_CORPUS and OPENSTUDIO_FLP_PAIRS are nonempty text manifests,
# and OS_FX_NULL, OS_PC_NULL, OS_FC_NULL each point to a directory containing .case files.
python tools/qualification/external.py --output /tmp/lemon-external-qualification
```

An unset/missing/empty variable or missing `.case`/project file must fail this step, not convert it into a skip.
For original-versus-resaved `.flp` pairs, the comparison test fails on semantic differences (including those formerly
only logged in `differ`). Follow the corpus and audio parity results with independent human review of legitimate
round-trip exceptions; do not weaken a failing check simply to make the release green.

## What is not claimed

- This is **not** a downloadable production release, MSI/MSIX, `.deb`/`.rpm`/AppImage, or notarized DMG.
- A native `.lemon` save is not a promise that every legacy `.flp` will be losslessly mapped on import/export.
- A byte-identical built-in-only synthetic render is not proof of external plugin parity or bit-identity across OSes.
- A GitHub Actions matrix green check, by itself, does not prove real-time audio reliability on hardware.
- Ad-hoc macOS `codesign` is only a local loading requirement; it does not establish developer identity or trust.
- Neither VST3 nor CLAP plugin modules are verified compatible by their binary headers alone; functional host tests
  and clean-machine distribution checks are separate gates.

**Release captain decision rule:** after all gates above have dated, reproducible evidence for the same immutable
commit, publish a signed release-candidate to a controlled test group; only after clean-machine and multi-session
validation approve a post-beta production release. Any uncovered crash, truncated file, incorrect render or silent
fixture skip immediately returns the decision to NO-GO.
