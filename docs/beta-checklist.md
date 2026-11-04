# Beta checklist

What stands between Lemon Studio and its beta: the alpha bar (a fully functional DAW at FL Studio parity, see
[alpha-checklist.md](alpha-checklist.md)) plus the things a build has to have before people outside the project are
expected to run it on their own machines.

Beta does not mean every item of the full roadmap on the Discord server is done. It means: the systems a project needs
are there, a release can be downloaded and run on all three platforms, and the remaining work is polish rather than
foundation.

Tick an item in the change that finishes it; for one under way, add "(in progress, #PR)" after it.

## Alpha blockers still open

- [ ] **Engine rework (sampler voice, automation).** The README's roadmap names it as the next big step. The sampler
  voice and the automation walk are the two places where the engine still takes shortcuts that a dense project can
  hear.

## Shipping

- [x] **Prebuilt binaries for Windows, macOS and Linux.** Every tag runs
  [release.yml](../.github/workflows/release.yml): it builds the program and every plugin, packages the program with
  its icon and desktop entry (Linux) or app bundle (macOS), packages the plugin bundles for other DAWs, and attaches
  all of it to the GitHub release as a prerelease.
- [ ] **macOS signing and notarization actually applied.** The release workflow signs with a Developer ID and
  notarizes when `MACOS_CERTIFICATE`, `MACOS_CERTIFICATE_PASSWORD`, `MACOS_SIGN_IDENTITY`, `MACOS_NOTARY_APPLE_ID`,
  `MACOS_NOTARY_TEAM_ID` and `MACOS_NOTARY_PASSWORD` are set; without them the build stays ad hoc signed. The
  workflow is wired, the certificate is not in the repository.
- [x] **macOS entitlements for a hardened runtime.** `crates/os-app/assets/entitlements.plist` asks for audio input
  and turns off hardened-runtime library validation, without which a signed build could not load third-party plugins.
- [ ] **Plugin crashes contained on macOS and Linux.** Calls into plugins run under a signal guard there, with the same
  handling of a stopped plugin as on Windows, and so do CLAP and VST3 editors' run loop callbacks on Linux. Left:
  macOS editors, which the system's run loop calls outside any host call.
- [ ] **Release notes.** `--generate-notes` drafts them from merged pull requests; they still need a human pass that
  names the known-risky areas (project files, plugin hosting) and points at the issue tracker.

## What a beta tester needs to be told

- [x] **A download that does not need a Rust toolchain.** The README links the releases page and explains what is in
  each package.
- [ ] **Where settings and projects live on each platform,** and that a `.lemon` project is not expected to open in
  FL Studio. (The README's per-platform notes cover this; the release notes should repeat the short version.)
- [ ] **How to report a bug:** HELP > Diagnostics writes `Diagnostics.txt`; the issue template asks for it.

## Verification before the first beta tag

- [ ] `cargo build --workspace --all-targets --locked` and `cargo test --workspace --locked` on Windows, Linux and
  macOS (CI does this on every push to `main`).
- [ ] The Linux package runs from a clean folder: the program starts, the plugins folder is found, and a project with
  built-in instruments plays.
- [ ] The macOS package opens without a Gatekeeper block on a machine that has never built it (needs the Developer ID
  signing above, or the "Open Anyway" path).
- [ ] The Windows package runs from a clean folder and the plugin scan finds the bundled plugins.
