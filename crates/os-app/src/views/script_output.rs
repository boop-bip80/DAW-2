// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! VIEW > Script output: the control plane's console (`crate::control`). Every request a script,
//! test or assistant sends is logged by `App::rpc_line` and shown here with its outcome.

use os_ui::kit::lemon as lk;
use os_ui::{kit, Font, Rect};

use crate::app::{Action, State, Ui, Win};
use crate::icons::{self, Icon};
use crate::tools::{ScriptAct, OUTPUT_LINES};

pub const SIZE: (f32, f32) = (660.0, 330.0);
const TITLE_H: f32 = 30.0;
const LIST_Y: f32 = 52.0;
const LINE_H: f32 = 17.0;

pub fn rect(s: &State) -> Rect {
    Rect::new(s.tools.script.pos.0, s.tools.script.pos.1, SIZE.0, SIZE.1)
}

pub fn bounds(s: &State) -> Rect {
    rect(s).outset(1.0)
}

pub fn list_rect(s: &State) -> Rect {
    let w = rect(s);
    Rect::new(w.x + 8.0, w.y + LIST_Y, w.w - 16.0, w.h - LIST_Y - 8.0)
}

pub fn lines_fit(s: &State) -> usize {
    (list_rect(s).h / LINE_H).floor().max(1.0) as usize
}

pub fn paint(ui: &mut Ui, s: &State) {
    let t = ui.t;
    let win = rect(s);
    let lemon = super::lemon(ui);
    if lemon {
        lk::outlined(ui.c, win, lk::r_panel(t), t.window_border, t.window_body);
    } else {
        ui.c.fill_rect(win, t.window_border);
        ui.c.fill_rect(win.inset(1.0), t.window_body);
        ui.c.fill_rect(Rect::new(win.x + 1.0, win.y + 1.0, win.w - 2.0, 1.0), t.window_title_hi);
    }
    ui.hit(win, Action::Swallow, "");
    let bar = Rect::new(win.x + 1.0, win.y + 1.0, win.w - 2.0, TITLE_H - 2.0);
    ui.hit(bar, Action::WinDrag(Win::ScriptOutput), "Drag to move");
    icons::draw(ui.c, Icon::FileText, Rect::new(win.x + 9.0, win.y + 7.0, 15.0, 15.0), t.glyph);
    ui.c.text_clip(Font::new(17), win.x + 30.0, win.y + 5.0, win.w - 195.0, t.text, "Script output");
    let n = s.tools.script.lines.len();
    let fit = lines_fit(s);
    let max_scroll = n.saturating_sub(fit);
    let scroll = s.tools.script.scroll.min(max_scroll);
    if n > fit {
        for (i, (dx, dir, tip, enabled)) in [
            (-124.0, kit::Dir::Up, "Older output", scroll < max_scroll),
            (-106.0, kit::Dir::Down, "Newer output", scroll > 0),
        ]
        .into_iter()
        .enumerate()
        {
            let r = Rect::new(win.right() + dx, win.y + 5.0, 16.0, 20.0);
            let hov = ui.hit(
                r,
                Action::ScriptOutput(ScriptAct::Scroll(if i == 0 { 1 } else { -1 })),
                tip,
            );
            let face = if hov && enabled { t.s(0x2a2d2f) } else { t.s(0x212425) };
            if lemon {
                lk::outlined(ui.c, r, lk::r_btn(t), t.s(0x161819), face);
            } else {
                kit::bordered(ui.c, r, 1.0, t.s(0x161819), face);
            }
            let col = if enabled { t.glyph } else { t.glyph_dim };
            kit::triangle(ui.c, r.cx(), r.cy(), 3.5, dir, col);
        }
    }
    let clear = Rect::new(win.right() - 86.0, win.y + 5.0, 58.0, 20.0);
    let hov = ui.hit(clear, Action::ScriptOutput(ScriptAct::Clear), "Clear the script output");
    if lemon {
        lk::outlined(ui.c, clear, lk::r_btn(t), t.s(0x161819), lk::face(t.s(0x2a2d2f), kit::State::hover(hov)));
    } else {
        kit::bordered(ui.c, clear, 2.0, t.s(0x161819), kit::face(t.s(0x212425), kit::State::hover(hov)));
    }
    ui.c.text_center(Font::new(12), clear, if hov { t.glyph } else { t.text }, "Clear");
    let close = Rect::new(win.right() - 22.0, win.y + 6.0, 16.0, 16.0);
    let hov = ui.hit(close, Action::CloseWin(Win::ScriptOutput), "Close");
    icons::draw(ui.c, Icon::Close, close.inset(1.5), if hov { t.tx(0xffffff) } else { t.glyph });

    let f = Font::new(12);
    let mut summary = if n == 0 {
        "No script output yet".to_string()
    } else {
        format!("{n} line{} · newest last", if n == 1 { "" } else { "s" })
    };
    if scroll > 0 {
        summary.push_str(" · scrolled back");
    }
    if n >= OUTPUT_LINES {
        summary.push_str(" · oldest dropped");
    }
    ui.c.text_clip(f, win.x + 12.0, win.y + 36.0, 220.0, t.text_dim, &summary);
    if scroll == 0 {
        ui.c.text_clip(
            f,
            win.x + 235.0,
            win.y + 36.0,
            win.w - 375.0,
            t.text_dim,
            "Requests from --control, scripts, tests and assistants",
        );
    }

    let list = list_rect(s);
    ui.c.fill_rect(list, t.rack_inset_border);
    ui.c.fill_rect(list.inset(1.0), t.rack_inset);
    if n == 0 {
        ui.c.text_center(Font::new(13), Rect::new(list.x, list.y, list.w, 54.0), t.text_dim, "No script output yet");
        ui.c.text_center(
            Font::new(12),
            Rect::new(list.x, list.y + 26.0, list.w, 54.0),
            t.text_dim,
            "Start with --control <socket> and send JSON-RPC requests; they appear here as they run.",
        );
        return;
    }
    let end = n.saturating_sub(scroll);
    let first = end.saturating_sub(fit);
    ui.c.push_clip(list);
    let lf = Font::new(11);
    for (i, line) in s.tools.script.lines.iter().enumerate().skip(first).take(fit) {
        let y = list.y + (i - first) as f32 * LINE_H + 3.0;
        let color = if line.contains(" -> error ") { t.meter_clip } else { t.text };
        ui.c.text_clip(lf, list.x + 6.0, y, list.w - 12.0, color, line);
    }
    ui.c.pop_clip();
}
