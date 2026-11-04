// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! TOOLS > Control builder: the project's MIDI controller links, with Edit, Remove and assignment
//! buttons for the selected channel and mixer track. The links and the MIDI learn box are
//! `crate::remote`; this file only draws and dispatches to them.

use os_ui::kit::lemon as lk;
use os_ui::{kit, Font, Rect};

use crate::app::{Action, State, Ui, Win};
use crate::icons::{self, Icon};
use crate::tools::{rows, selected_targets, BuilderAct};

pub const SIZE: (f32, f32) = (620.0, 360.0);
const TITLE_H: f32 = 30.0;
const ROW_H: f32 = 32.0;
const LIST_Y: f32 = 58.0;
const FOOTER_H: f32 = 66.0;

pub fn rect(s: &State) -> Rect {
    Rect::new(s.tools.builder.pos.0, s.tools.builder.pos.1, SIZE.0, SIZE.1)
}

pub fn bounds(s: &State) -> Rect {
    rect(s).outset(1.0)
}

pub fn list_rect(s: &State) -> Rect {
    let w = rect(s);
    Rect::new(w.x + 10.0, w.y + LIST_Y, w.w - 20.0, (w.h - LIST_Y - FOOTER_H).max(ROW_H))
}

pub fn rows_fit(s: &State) -> usize {
    (list_rect(s).h / ROW_H).floor().max(1.0) as usize
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
    ui.hit(bar, Action::WinDrag(Win::ControlBuilder), "Drag to move");
    icons::draw(ui.c, Icon::MidiControl, Rect::new(win.x + 9.0, win.y + 7.0, 15.0, 15.0), t.glyph);
    ui.c.text_clip(Font::new(17), win.x + 30.0, win.y + 5.0, win.w - 60.0, t.text, "Control builder");
    let close = Rect::new(win.right() - 22.0, win.y + 6.0, 16.0, 16.0);
    let hov = ui.hit(close, Action::CloseWin(Win::ControlBuilder), "Close");
    icons::draw(ui.c, Icon::Close, close.inset(1.5), if hov { t.tx(0xffffff) } else { t.glyph });

    let subtitle = Font::new(12);
    let n = rows(s).len();
    let summary = match n {
        0 => "No controller links yet".to_string(),
        1 => "1 controller link".to_string(),
        n => format!("{n} controller links"),
    };
    ui.c.text(subtitle, win.x + 12.0, win.y + 38.0, t.text_dim, &summary);
    ui.c.text_clip(subtitle, win.x + 150.0, win.y + 38.0, win.w - 220.0, t.text_dim, "Links are kept in the program's settings, not the project");

    let list = list_rect(s);
    let all = rows(s);
    let fit = rows_fit(s);
    let max = all.len().saturating_sub(fit);
    let first = s.tools.builder.scroll.min(max);
    if all.len() > fit {
        for (i, dir, hint) in [(0, -1, "Show earlier links"), (1, 1, "Show later links")] {
            let r = Rect::new(win.right() - 42.0 + i as f32 * 18.0, win.y + 35.0, 16.0, 16.0);
            let hov = ui.hit(r, Action::ControlBuilder(BuilderAct::Scroll(dir)), hint);
            let c = if hov { t.glyph } else { t.text_dim };
            kit::triangle(ui.c, r.cx(), r.cy(), 3.5, if dir < 0 { kit::Dir::Up } else { kit::Dir::Down }, c);
        }
    }

    if all.is_empty() {
        ui.c.text_center(Font::new(13), Rect::new(list.x, list.y, list.w, 58.0), t.text_dim, "No controller links yet");
        ui.c.text_center(
            Font::new(12),
            Rect::new(list.x, list.y + 28.0, list.w, 48.0),
            t.text_dim,
            "Assign a selected control below, or choose MIDI learn on a knob or mixer box.",
        );
    } else {
        ui.c.push_clip(list);
        for (i, row) in all.iter().enumerate().skip(first).take(fit) {
            let r = Rect::new(list.x, list.y + (i - first) as f32 * ROW_H, list.w, ROW_H);
            if (i - first) % 2 == 1 {
                ui.c.fill_rect(r, t.rack_inset.shade(0.03));
            }
            let edit = Rect::new(r.right() - 112.0, r.y + 4.0, 48.0, 24.0);
            let remove = Rect::new(r.right() - 58.0, r.y + 4.0, 48.0, 24.0);
            let hov = ui.hit(r, Action::ControlBuilder(BuilderAct::Edit(row.target)), format!("Edit this link: {}", row.label));
            if hov {
                ui.c.fill_rect(r, t.rack_inset.shade(0.08));
            }
            ui.c.text_clip(Font::new(13), r.x + 6.0, r.y + 4.0, r.w - 130.0, t.text, &row.label);
            ui.c.text_clip(Font::new(11), r.x + 6.0, r.y + 18.0, r.w - 130.0, t.text_dim, &row.detail);
            button(ui, edit, "Edit", Action::ControlBuilder(BuilderAct::Edit(row.target)), format!("Edit the controller link of {}", row.label));
            button(ui, remove, "Remove", Action::ControlBuilder(BuilderAct::Remove(row.target)), format!("Remove the controller link of {}", row.label));
        }
        ui.c.pop_clip();
        if all.len() > fit {
            let k = list.h / all.len() as f32;
            let bar = Rect::new(list.right() - 3.0, list.y + first as f32 * k, 2.0, (fit as f32 * k).max(6.0));
            ui.c.fill_rect(bar, t.glyph.with_alpha(110));
        }
    }

    let targets = selected_targets(s);
    let fy = win.bottom() - 48.0;
    ui.c.text(Font::new(12), win.x + 12.0, fy - 13.0, t.text_dim, "Assign the selected channel and mixer controls:");
    if targets.is_empty() {
        ui.c.text(Font::new(12), win.x + 12.0, fy + 4.0, t.text_dim, "Select a channel and a mixer track first.");
        return;
    }
    let gap = 8.0;
    let bw = ((win.w - 24.0 - gap * 3.0) / 4.0).min(150.0);
    let start = win.x + 12.0;
    for (i, (label, target)) in targets.iter().enumerate() {
        let r = Rect::new(start + i as f32 * (bw + gap), fy, bw, 24.0);
        button(ui, r, label, Action::ControlBuilder(BuilderAct::Edit(*target)), format!("MIDI learn for {label}"));
    }
}

fn button(ui: &mut Ui, r: Rect, label: &str, action: Action, hint: String) {
    let t = ui.t;
    let hov = ui.hit(r, action, hint);
    if super::lemon(ui) {
        lk::outlined(ui.c, r, lk::r_btn(t), t.s(0x161819), lk::face(t.s(0x2a2d2f), kit::State::hover(hov)));
    } else {
        kit::bordered(ui.c, r, 2.0, t.s(0x161819), kit::face(t.s(0x212425), kit::State::hover(hov)));
    }
    ui.c.text_center(Font::new(12), r, if hov { t.glyph } else { t.text }, label);
}
