# What this build is

**This is a beta.** Projects, settings and plugin states can still change between builds: keep a copy of anything you
would hate to lose, and keep the original FL Studio project file.

**Known-risky areas** — please try these first, and say what breaks:

- **Project files.** FL Studio projects are read, never written: Lemon Studio opens an `.flp` and saves its own
  `.lemon` project. A project saved before FL Studio 24.2 is converted as it opens.
- **Plugin hosting.** Third-party VST2, VST3 and CLAP plugins, and Lemon Studio's own bundles in the package's
  `plugins` folder.

Bugs and feature requests: <https://github.com/ItsAlphaNeon/Lemon-Studio/issues>. A bug report asks for
`Diagnostics.txt` (HELP > Diagnostics) and, after a crash, `Crash.txt` from the user folder.

## Settings and projects

Settings, themes and presets live in the platform's user folder; projects live wherever you save them.

| Platform | Settings, themes and presets |
| --- | --- |
| Windows | `%APPDATA%\Lemon Studio` |
| macOS | `~/Library/Application Support/Lemon Studio` |
| Linux | `$XDG_CONFIG_HOME/Lemon Studio` (usually `~/.config/Lemon Studio`) |

`OPENSTUDIO_USER_DIR` overrides the folder. A `.lemon` project is Lemon Studio's own format and is **not** expected to
open in FL Studio.

## Reporting a bug

HELP > Diagnostics writes `Diagnostics.txt` into that folder; a crash leaves `Crash.txt` beside it with the message,
where it happened and, with `RUST_BACKTRACE=1`, a backtrace. Attach both to the issue.
