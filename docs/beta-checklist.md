# Beta checklist

What stands between Lemon Studio and its beta: the alpha bar (a fully functional DAW at FL Studio parity, see
[alpha-checklist.md](alpha-checklist.md)) plus the things a build has to have before people outside the project are
expected to run it on their own machines.

Beta does not mean every item of the full roadmap on the Discord server is done. It means: the systems a project needs
are there, a release can be downloaded and run on all three platforms, and the remaining work is polish rather than
foundation.

Tick an item in the change that finishes it; for one under way, add "(in progress, #PR)" after it.

## Alpha blockers still open

None. The engine item is done and tested: `cargo test -p os-engine
a_pattern_point_moves_a_send_and_the_main_pitch` covers the automation half (a pattern control point on a send level
and on the main pitch, a channel that does not follow the main pitch, and a replay from another position), and
`a_looped_voice_wraps_at_the_loop_end`, `a_looped_voice_fades_out_on_note_off` and
`a_song_ends_with_a_looping_voice_ringing` cover the sampler: the channel's loop points or the file's `smpl` sustain
loop reach `Sample::loop_range` and `ChannelState`, each `Voice` carries the range, `render_voice_into` wraps the read
position at the loop's end, a note off fades the voice across the loop and frees it, a sample without a loop still
plays once, and a looping voice is released at the song's end so a render finishes.

## Shipping

- [x] **Prebuilt binaries for Windows, macOS and Linux.** Every tag runs
  [release.yml](../.github/workflows/release.yml): it builds the program and every plugin, then
  `cargo run -p package -- --platform <platform> --profile release` stages the package — the program, its icon and
  desktop entry (Linux) or app bundle (macOS), the plugin bundles for other DAWs, and the release notes — checks that
  layout, and only then archives it, so a missing artifact fails before the tag rather than at download time. CI runs
  the same tool and check on Linux for every push ([ci.yml](../.github/workflows/ci.yml)). Here it staged
  `dist/lemon-studio-linux-x64` from the real debug build (50 plugin bundles: 25 VST3, 25 CLAP) and the check exited 0;
  renaming `lemonstudio.desktop` made the check exit 1 naming it, and restoring it made it exit 0 again. Remaining: the
  Windows and macOS packages are staged by the same tool in CI, which cannot be observed from this container.
- [ ] **macOS signing and notarization actually applied.** The release workflow signs with a Developer ID and
  notarizes when `MACOS_CERTIFICATE`, `MACOS_CERTIFICATE_PASSWORD`, `MACOS_SIGN_IDENTITY`, `MACOS_NOTARY_APPLE_ID`,
  `MACOS_NOTARY_TEAM_ID` and `MACOS_NOTARY_PASSWORD` are set; without them the build stays ad hoc signed. Remaining: the
  owner adds those six repository secrets (a Developer ID Application certificate and an app-specific password); no
  certificate can be created or tested from here.
- [x] **macOS entitlements for a hardened runtime.** `crates/os-app/assets/entitlements.plist` asks for audio input
  and turns off hardened-runtime library validation, without which a signed build could not load third-party plugins.
- [ ] **Plugin crashes contained on macOS and Linux.** Host calls into plugins run under a signal guard on macOS and
  Linux (`crates/os-host/src/guard.rs`), with the same handling of a stopped plugin as on Windows: `cargo test -p
  os-host --locked` passes here (14 tests, including a crash in a nested call, crashes in a row and on other threads,
  and the module coming back with its code). Remaining: macOS plugin editors, which the system's run loop calls outside
  any host call — a crash there still closes Lemon Studio, and settling it needs a Mac.
- [x] **Release notes.** [release-notes.md](release-notes.md) is the summary every tag's release opens with: what the
  beta is, the known-risky areas (project files, plugin hosting) and the tracker, where settings and projects live, and
  how to report a bug. [release.yml](../.github/workflows/release.yml) prepends it to GitHub's generated notes
  (`--notes` with `--generate-notes`), and `cargo test -p watermark
  the_release_notes_tell_a_beta_tester_what_they_need` fails when the file, the workflow's use of it, or the bug
  report's request for the diagnostics goes missing (checked by moving the file away: the test fails).

## What a beta tester needs to be told

- [x] **A download that does not need a Rust toolchain.** The README links the releases page and explains what is in
  each package.
- [x] **Where settings and projects live on each platform,** and that a `.lemon` project is not expected to open in
  FL Studio. The release notes' "Settings and projects" table names the folder on each platform (and
  `OPENSTUDIO_USER_DIR`) and says a `.lemon` project is Lemon Studio's own, not an FL Studio one; the README's platform
  sections name the same folders. Kept true by `the_release_notes_tell_a_beta_tester_what_they_need`.
- [x] **How to report a bug:** HELP > Diagnostics writes `Diagnostics.txt`, and a panic leaves `Crash.txt` in the
  same folder with the message, the place and (with `RUST_BACKTRACE=1`) a backtrace
  (`diagnostics::tests::a_crash_leaves_a_report`); the bug form in `.github/ISSUE_TEMPLATE/bug_report.yml` asks for
  both, and `the_release_notes_tell_a_beta_tester_what_they_need` keeps it asking.

## Verification before the first beta tag

- [ ] `cargo build --workspace --all-targets --locked` and `cargo test --workspace --locked` on Windows, Linux and
  macOS. [ci.yml](../.github/workflows/ci.yml) runs both on `windows-latest`, `ubuntu-24.04` and `macos-latest` for
  every push to `main` and every pull request; on this HEAD the Linux pair was run here and both exit 0. Remaining: a
  green run of those three CI jobs for the beta commit, which cannot be observed from this container.
- [ ] The Linux package runs from a clean folder: the program starts, the plugins folder is found, and a project with
  built-in instruments plays. Remaining: a desktop session and an audio device — this container has neither, so the
  program cannot open a window or be heard.
- [ ] The macOS package opens without a Gatekeeper block on a machine that has never built it (needs the Developer ID
  signing above, or the "Open Anyway" path). Remaining: the owner's signing secrets and a Mac to open the download on.
- [ ] The Windows package runs from a clean folder and the plugin scan finds the bundled plugins. Remaining: a Windows
  machine; the packaging step itself is in the release workflow.
