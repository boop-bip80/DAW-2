// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The two tool windows' state: TOOLS > Control builder and VIEW > Script output.
//!
//! The Control builder is a live view of the controller links the Remote control settings keep
//! (`crate::remote`): each row names the target, its controller, its mode and formula, and can be
//! edited in the existing MIDI learn box or removed. Its assignment buttons open that same box for
//! the selected channel's volume and pan and the selected mixer track's fader and pan, so a link
//! is always made by the code that plays it.
//!
//! The Script output is the control plane's console (`crate::control`): every request a script,
//! test or assistant sends through `--control` is shown with its outcome, without its parameters
//! or token. The window is `views/script_output.rs`; the request hook is `control::rpc`.

use std::collections::VecDeque;

use crate::app::{App, State, Win};
use crate::knobs::KnobId;
use crate::typein::Target;

/// How many script output lines are kept. A script can poll `events`; the window stays bounded.
pub const OUTPUT_LINES: usize = 200;

/// The Control builder's controls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BuilderAct {
    /// Open the Remote control box for a new or existing link.
    Edit(Target),
    /// Remove the link of this target.
    Remove(Target),
    /// Move the list by `steps` rows.
    Scroll(i32),
}

/// The Script output's controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptAct {
    Clear,
    /// Move back (`1`) or forward (`-1`) through the kept lines. 0 is the newest.
    Scroll(i32),
}

/// Where the Control builder is and which row is first.
pub struct BuilderUi {
    /// Top-left corner (logical px).
    pub pos: (f32, f32),
    /// It has been placed in the workspace once (`pos` is no longer the default).
    pub placed: bool,
    /// First link shown.
    pub scroll: usize,
}

/// Where the Script output is and what it has shown.
pub struct ScriptUi {
    /// Top-left corner (logical px).
    pub pos: (f32, f32),
    /// It has been placed in the workspace once (`pos` is no longer the default).
    pub placed: bool,
    /// The output, oldest first; at most [`OUTPUT_LINES`].
    pub lines: VecDeque<String>,
    /// Lines hidden below the window: 0 follows the newest.
    pub scroll: usize,
}

impl ScriptUi {
    /// Adds a line, dropping the oldest when the window is full. One line is bounded too, so a
    /// long client name or answer cannot hide the rest of the console.
    pub fn push(&mut self, line: String) {
        if self.lines.len() >= OUTPUT_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(clip(&line, 220));
        self.scroll = 0;
    }

    /// Clears the console.
    pub fn clear(&mut self) {
        self.lines.clear();
        self.scroll = 0;
    }
}

/// The state both tool windows share.
pub struct ToolsUi {
    pub builder: BuilderUi,
    pub script: ScriptUi,
}

impl Default for ToolsUi {
    fn default() -> Self {
        ToolsUi {
            builder: BuilderUi { pos: (610.0, 170.0), placed: false, scroll: 0 },
            script: ScriptUi { pos: (650.0, 220.0), placed: false, lines: VecDeque::new(), scroll: 0 },
        }
    }
}

/// One row of the Control builder.
#[derive(Debug, Clone, PartialEq)]
pub struct BuilderRow {
    pub target: Target,
    /// What it drives and its controller, as the browser's Remote control section names it.
    pub label: String,
    /// Port, mode, formula and smoothing.
    pub detail: String,
}

/// The links as the Control builder lists them, in the order the settings keep them.
pub fn rows(s: &State) -> Vec<BuilderRow> {
    crate::remote::links()
        .into_iter()
        .map(|l| {
            let port = l.port.map_or_else(|| "Any port".into(), |p| format!("Port {p}"));
            let smooth = l.smooth.map_or_else(String::new, |ms| format!(" · {ms} ms smoothing"));
            BuilderRow {
                target: l.target,
                label: crate::remote::label(s, &l),
                detail: format!("{port} · {} · {}{smooth}", l.mode.name(), l.formula.name()),
            }
        })
        .collect()
}

/// The controls the Control builder offers to assign: the selected channel's volume and pan, then
/// the selected mixer track's fader and pan. Only controls the project has are offered.
pub fn selected_targets(s: &State) -> Vec<(&'static str, Target)> {
    let mut out = Vec::new();
    if s.project.channels.get(s.selected_channel).is_some() {
        out.push(("Channel volume", Target::Knob(KnobId::ChanVol(s.selected_channel))));
        out.push(("Channel pan", Target::Knob(KnobId::ChanPan(s.selected_channel))));
    }
    if crate::mixer::track(s, s.selected_mixer).is_some() {
        out.push(("Mixer fader", Target::Fader(s.selected_mixer)));
        out.push(("Mixer pan", Target::Knob(KnobId::MixPan(s.selected_mixer))));
    }
    out
}

impl App {
    /// TOOLS > Control builder: open it in the middle of the workspace the first time, or hide it
    /// / raise it as any other window.
    pub(crate) fn toggle_control_builder(&mut self) {
        if self.state.shown(Win::ControlBuilder) && self.state.in_front(Win::ControlBuilder) {
            self.state.hide(Win::ControlBuilder);
            return self.invalidate();
        }
        let s = &mut self.state;
        if !s.tools.builder.placed {
            let (w, h) = crate::views::control_builder::SIZE;
            let work = s.work_rect();
            let top = crate::views::work_y(s);
            s.tools.builder.pos = (
                (work.x + (work.w - w) / 2.0).clamp(0.0, (s.screen.0 - w).max(0.0)).round(),
                (work.y + (work.h - h) / 2.0).clamp(top, (s.screen.1 - h).max(top)).round(),
            );
            s.tools.builder.placed = true;
        }
        s.raise(Win::ControlBuilder);
        self.invalidate();
    }

    /// VIEW > Script output: the same placement for the console.
    pub(crate) fn toggle_script_output(&mut self) {
        if self.state.shown(Win::ScriptOutput) && self.state.in_front(Win::ScriptOutput) {
            self.state.hide(Win::ScriptOutput);
            return self.invalidate();
        }
        let s = &mut self.state;
        if !s.tools.script.placed {
            let (w, h) = crate::views::script_output::SIZE;
            let work = s.work_rect();
            let top = crate::views::work_y(s);
            s.tools.script.pos = (
                (work.x + (work.w - w) / 2.0).clamp(0.0, (s.screen.0 - w).max(0.0)).round(),
                (work.y + (work.h - h) / 2.0).clamp(top, (s.screen.1 - h).max(top)).round(),
            );
            s.tools.script.placed = true;
        }
        s.raise(Win::ScriptOutput);
        self.invalidate();
    }

    /// A Control builder control.
    pub(crate) fn builder_act(&mut self, a: BuilderAct) {
        match a {
            // Edit and Assign both use the existing MIDI learn box. `LinkBox::new` loads the
            // existing link, so Edit changes a mapping rather than replacing it blindly.
            BuilderAct::Edit(t) => return self.remote_cmd(crate::remote::RCmd::Link(t)),
            BuilderAct::Remove(t) => {
                crate::remote::remove(t);
                self.state.message = Some("Controller link removed".into());
            }
            BuilderAct::Scroll(d) => {
                let fit = crate::views::control_builder::rows_fit(&self.state);
                let max = rows(&self.state).len().saturating_sub(fit);
                let at = self.state.tools.builder.scroll as i32 + d;
                self.state.tools.builder.scroll = at.clamp(0, max as i32) as usize;
            }
        }
        self.invalidate();
    }

    /// A Script output control.
    pub(crate) fn script_act(&mut self, a: ScriptAct) {
        match a {
            ScriptAct::Clear => {
                self.state.tools.script.clear();
                self.state.message = Some("Script output cleared".into());
            }
            ScriptAct::Scroll(d) => {
                let fit = crate::views::script_output::lines_fit(&self.state);
                let max = self.state.tools.script.lines.len().saturating_sub(fit);
                let at = self.state.tools.script.scroll as i32 + d;
                self.state.tools.script.scroll = at.clamp(0, max as i32) as usize;
            }
        }
        self.invalidate();
    }
}

/// A value as one console line shows it: its JSON, clipped so one large `state` answer cannot hide
/// the rest of the console.
pub fn short_value(v: &serde_json::Value) -> String {
    clip(&v.to_string(), 180)
}

/// `s` clipped to `max` characters, with an ellipsis when it was longer.
pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(3)).collect();
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Win;
    use crate::control::rpc::Client;
    use crate::control::Level;
    use crate::views::menus::{item_at, pick, TOOLS_MENU, VIEW};
    use os_ui::Canvas;

    fn open(app: &mut App, menu: usize, label: &str) -> usize {
        let i = (0..400)
            .find(|&i| item_at(menu, i).is_some_and(|it| it.label == label))
            .unwrap_or_else(|| panic!("{label} is not in menu {menu}"));
        app.act(pick(menu, i));
        i
    }

    /// TOOLS > Control builder opens, lists the real controller links, edits one through the MIDI
    /// learn box, removes it, and closes without touching the project.
    #[test]
    fn the_control_builder_lists_edits_and_removes_controller_links() {
        let mut app = App::new(None);
        app.state.project = crate::app::test_project();
        crate::remote::set_links(&[]);
        let target = Target::Knob(KnobId::ChanVol(1));
        crate::remote::add(crate::remote::Link { port: Some(2), ch: 2, cc: 21, smooth: Some(50), ..crate::remote::Link::new(2, 21, target) }, true);

        let i = open(&mut app, TOOLS_MENU, "Control builder");
        assert!(crate::views::menus::works(TOOLS_MENU, item_at(TOOLS_MENU, i).unwrap()), "the menu entry is enabled");
        assert!(app.state.shown(Win::ControlBuilder), "TOOLS > Control builder opens the builder");
        assert!(app.state.in_front(Win::ControlBuilder), "the builder is in front");

        let list = rows(&app.state);
        assert_eq!(list.len(), 1, "the builder reads the links the Remote control settings keep");
        assert!(list[0].label.contains("channel 3, ctrl 21"), "{}", list[0].label);
        assert!(list[0].detail.contains("Port 2 · Default · Input · 50 ms smoothing"), "{}", list[0].detail);

        app.act(crate::app::Action::ControlBuilder(BuilderAct::Edit(target)));
        let b = app.state.link_box.as_ref().expect("Edit opens the MIDI learn box");
        assert_eq!((b.target, b.port, b.ch, b.cc), (target, Some(2), 2, Some(21)));
        app.state.link_box = None;

        app.act(crate::app::Action::ControlBuilder(BuilderAct::Remove(target)));
        assert!(!crate::remote::links().iter().any(|l| l.target == target), "Remove deletes the link");

        app.act(crate::app::Action::CloseWin(Win::ControlBuilder));
        assert!(!app.state.shown(Win::ControlBuilder));
        crate::remote::set_links(&[]);
    }

    /// VIEW > Script output shows a real control-plane request and its result, and Clear empties it.
    #[test]
    fn the_script_output_shows_control_plane_output() {
        let mut app = App::new(None);
        app.state.project = crate::app::test_project();
        let mut c = Client { name: "Script".into(), level: Level::Edit, saves: false, token: None };

        open(&mut app, VIEW, "Script output");
        assert!(app.state.shown(Win::ScriptOutput), "VIEW > Script output opens the console");

        let reply = app.rpc_line(&mut c, r#"{"jsonrpc":"2.0","id":1,"method":"state"}"#).expect("state answers");
        assert!(reply.contains("\"result\""), "{reply}");
        let line = app.state.tools.script.lines.back().expect("the request is logged").clone();
        assert!(line.starts_with("Script: state -> ok "), "{line}");
        assert!(line.chars().count() <= 220, "the console does not repeat the whole project: {line}");

        app.rpc_line(&mut c, r#"{"jsonrpc":"2.0","id":2,"method":"no_such_method"}"#);
        let line = app.state.tools.script.lines.back().unwrap();
        assert!(line.contains("no_such_method -> error -32601"), "{line}");

        app.act(crate::app::Action::ScriptOutput(ScriptAct::Clear));
        assert!(app.state.tools.script.lines.is_empty());
        app.act(crate::app::Action::CloseWin(Win::ScriptOutput));
        assert!(!app.state.shown(Win::ScriptOutput));
    }

    /// Both windows paint as real windows: their controls are hit-testable, the assignment button
    /// opens the MIDI learn box, Clear empties the console, and the × closes each one.
    #[test]
    fn both_tool_windows_paint_and_their_buttons_work() {
        let Ok(mut canvas) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = crate::app::test_project();
        crate::remote::set_links(&[]);

        open(&mut app, TOOLS_MENU, "Control builder");
        app.paint(&mut canvas, None);
        let target = selected_targets(&app.state).first().expect("a selected channel and mixer").1;
        let assign = app
            .hits
            .iter()
            .rev()
            .find(|h| h.action == crate::app::Action::ControlBuilder(BuilderAct::Edit(target)))
            .map(|h| h.rect)
            .expect("the assignment button is drawn");
        app.mouse_move(assign.cx(), assign.cy());
        app.mouse_down();
        app.mouse_up();
        assert_eq!(app.state.link_box.as_ref().map(|b| b.target), Some(target), "the button opens MIDI learn");
        app.state.link_box = None;
        app.paint(&mut canvas, None);
        let close = app
            .hits
            .iter()
            .rev()
            .find(|h| h.action == crate::app::Action::CloseWin(Win::ControlBuilder))
            .map(|h| h.rect)
            .expect("the close button is drawn");
        app.mouse_move(close.cx(), close.cy());
        app.mouse_down();
        app.mouse_up();
        assert!(!app.state.shown(Win::ControlBuilder), "the × closes the builder");

        open(&mut app, VIEW, "Script output");
        let mut c = Client { name: "Painter".into(), level: Level::Edit, saves: false, token: None };
        app.rpc_line(&mut c, r#"{"jsonrpc":"2.0","id":1,"method":"state"}"#);
        app.paint(&mut canvas, None);
        let clear = app
            .hits
            .iter()
            .rev()
            .find(|h| h.action == crate::app::Action::ScriptOutput(ScriptAct::Clear))
            .map(|h| h.rect)
            .expect("the Clear button is drawn");
        app.mouse_move(clear.cx(), clear.cy());
        app.mouse_down();
        app.mouse_up();
        assert!(app.state.tools.script.lines.is_empty(), "Clear empties the console");
        app.paint(&mut canvas, None);
        let close = app
            .hits
            .iter()
            .rev()
            .find(|h| h.action == crate::app::Action::CloseWin(Win::ScriptOutput))
            .map(|h| h.rect)
            .expect("the close button is drawn");
        app.mouse_move(close.cx(), close.cy());
        app.mouse_down();
        app.mouse_up();
        assert!(!app.state.shown(Win::ScriptOutput), "the × closes the console");
        crate::remote::set_links(&[]);
    }
}
