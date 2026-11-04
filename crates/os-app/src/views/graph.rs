// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The container's routing-graph window: the nodes, the connections between them, the named outputs and the note
//! processors. Connect... picks a source and then a destination through menus; connections, extra outputs and note
//! processors can be removed here. Each edit is one undo step (`container::GraphCmd`).
// spec: plugins/patcher/README.md §4 slice 3

use os_ui::kit;
use os_ui::kit::lemon as lk;
use os_ui::{Font, Rect};

use super::FrameInfo;
use crate::app::{Action, State, Ui, Win};
use crate::container::{graph, members, node_name, title, GAct};
use crate::icons::{self, Icon};

pub const W: f32 = 600.0;
/// Tall enough for twelve connections and three note processors; the menus reach anything below them.
pub const H: f32 = 560.0;
const EDGE_H: f32 = 22.0;
const MAX_EDGES: usize = 12;
const MAX_NOTES: usize = 3;

pub fn rect(s: &State) -> Rect {
    Rect::new(s.graph.pos.0, s.graph.pos.1, W, H)
}

pub fn bounds(s: &State) -> Rect {
    rect(s).outset(1.0)
}

/// Text with its middle at `cy`.
fn label(ui: &mut Ui, f: Font, x: f32, cy: f32, col: os_ui::Color, s: &str) {
    let y = (cy - ui.c.line_height(f) / 2.0).round();
    ui.c.text(f, x, y, col, s);
}

fn button(ui: &mut Ui, r: Rect, a: Action, hint: &'static str, text: &str, on: bool) {
    let hov = ui.hit(r, if on { a.clone() } else { Action::Swallow }, hint) && on;
    let down = on && ui.down(r, &a);
    if on {
        kit::button(ui.c, ui.t, r, kit::State::new(hov, down, false));
    } else {
        kit::button(ui.c, ui.t, r, kit::State::NORMAL.with_disabled(true));
    }
    let ink = if !on {
        ui.t.text_faint
    } else if hov {
        ui.t.tx(0xffffff)
    } else {
        ui.t.text
    };
    let f = Font::new(13);
    let w = ui.c.text_width(f, text);
    label(ui, f, (r.cx() - w / 2.0).round(), r.cy(), ink, text);
}

pub fn paint(ui: &mut Ui, s: &State, _f: &FrameInfo) {
    let t = ui.t;
    let win = rect(s);
    let (x0, y0) = (win.x, win.y);
    let lemon = super::lemon(ui);
    let (bg, line) = (t.s(0x202324), t.s(0x16191a));
    if lemon {
        lk::outlined(ui.c, win, lk::r_panel(t), line, bg);
    } else {
        ui.c.fill_rect(win, line);
        ui.c.fill_rect(win.inset(1.0), bg);
    }
    ui.hit(win, Action::Swallow, "");
    let (track, slot) = s.container.at;
    let name = title(s, track, slot).unwrap_or_else(|| "Container".into());
    ui.hit(Rect::new(x0 + 1.0, y0 + 1.0, W - 30.0, 28.0), Action::WinDrag(Win::Graph), "");
    label(ui, Font::new(17), x0 + 9.0, y0 + 15.0, t.text, &format!("Graph - {name}"));
    let close = Rect::new(x0 + W - 20.0, y0 + 10.0, 10.0, 10.0);
    let hov = ui.hit(close.outset(4.0), Action::CloseWin(Win::Graph), "Close");
    icons::draw(ui.c, Icon::Close, close.outset(1.0), if hov { t.tx(0xffffff) } else { t.glyph });

    let Some(list) = members(s, track, slot) else { return };
    let Some(g) = graph(s, track, slot) else { return };
    label(ui, Font::new(12), x0 + 10.0, y0 + 40.0, t.text_dim, "Click Connect..., choose a source, then a destination. Each edit is one undo step.");
    let mut nodes = vec!["Input".to_string()];
    nodes.extend(list.iter().map(|m| node_name(&list, crate::container::Endpoint::Member(m.id))));
    nodes.extend((0..g.outputs.len()).filter_map(|i| u8::try_from(i).ok()).map(|i| node_name(&list, crate::container::Endpoint::Output(i))));
    ui.c.text_clip(Font::new(13), x0 + 10.0, y0 + 58.0, W - 20.0, t.text, &nodes.join("  |  "));

    button(ui, Rect::new(x0 + 10.0, y0 + 86.0, 90.0, 24.0), Action::Graph(GAct::ConnectMenu), "Connect two nodes", "Connect...", true);
    button(ui, Rect::new(x0 + 108.0, y0 + 86.0, 98.0, 24.0), Action::Graph(GAct::AddOutputMenu), "Add an output routed to a mixer track", "Add output...", true);
    button(ui, Rect::new(x0 + 214.0, y0 + 86.0, 90.0, 24.0), Action::Graph(GAct::OutputMenu), "Route or remove a named output", "Outputs...", true);
    button(ui, Rect::new(x0 + 312.0, y0 + 86.0, 108.0, 24.0), Action::Graph(GAct::DisconnectMenu), "Remove a connection", "Disconnect...", !g.edges.is_empty());
    button(ui, Rect::new(x0 + 460.0, y0 + 86.0, 118.0, 24.0), Action::Graph(GAct::NoteMenu), "Add, edit, move or remove a note processor", "Notes...", true);

    label(ui, Font::new(11), x0 + 10.0, y0 + 126.0, t.text_dim, &format!("CONNECTIONS ({})", g.edges.len()));
    let mut edges_end = 140.0;
    if g.edges.is_empty() {
        label(ui, Font::new(13), x0 + 14.0, y0 + 148.0, t.text_faint, "(none)");
        edges_end += 18.0;
    } else {
        for (i, e) in g.edges.iter().take(MAX_EDGES).enumerate() {
            let row = Rect::new(x0 + 8.0, y0 + 140.0 + EDGE_H * i as f32, W - 16.0, EDGE_H);
            if i > 0 {
                ui.c.fill_rect(Rect::new(row.x, row.y, row.w, 1.0), t.s(0x2a2e30));
            }
            let text = format!("{}  ->  {}", node_name(&list, e.from), node_name(&list, e.to));
            label(ui, Font::new(13), row.x + 5.0, row.cy(), t.text, &text);
            let x = Rect::new(row.right() - 20.0, row.cy() - 6.0, 12.0, 12.0);
            let hov = ui.hit(x.outset(4.0), Action::Graph(GAct::Disconnect(e.from, e.to)), "Remove this connection");
            icons::draw(ui.c, Icon::Close, x, if hov { t.tx(0xffffff) } else { t.glyph_dim });
            edges_end += EDGE_H;
        }
        let hidden = g.edges.len().saturating_sub(MAX_EDGES);
        if hidden > 0 {
            label(ui, Font::new(12), x0 + 14.0, y0 + 140.0 + edges_end - 140.0, t.text_faint, &format!("{hidden} more; use Disconnect..."));
            edges_end += 18.0;
        }
    }

    let notes_y = y0 + 140.0 + edges_end - 140.0 + 14.0;
    label(ui, Font::new(11), x0 + 10.0, notes_y, t.text_dim, &format!("NOTE PROCESSORS ({})", g.notes.len()));
    for (i, n) in g.notes.iter().take(MAX_NOTES).enumerate() {
        let ry = notes_y + 12.0 + 20.0 * i as f32;
        let text = match n.kind() {
            Ok(k) => k.description(),
            Err(_) => format!("{} (unsupported)", n.name),
        };
        let f = Font::new(12);
        let ty = (ry - ui.c.line_height(f) / 2.0).round();
        ui.c.text_clip(f, x0 + 14.0, ty, W - 40.0, t.text, &text);
        let x = Rect::new(x0 + W - 28.0, ry - 6.0, 12.0, 12.0);
        let hov = ui.hit(x.outset(4.0), Action::Graph(GAct::RemoveNote(i)), "Remove this note processor");
        icons::draw(ui.c, Icon::Close, x, if hov { t.tx(0xffffff) } else { t.glyph_dim });
    }
    if g.notes.is_empty() {
        label(ui, Font::new(12), x0 + 14.0, notes_y + 14.0, t.text_faint, "(none)");
    } else if g.notes.len() > MAX_NOTES {
        label(ui, Font::new(12), x0 + 14.0, notes_y + 12.0 + 20.0 * MAX_NOTES as f32, t.text_faint, &format!("{} more; use Notes...", g.notes.len() - MAX_NOTES));
    }

    let main_track = if slot == crate::container::CHANNEL {
        s.project.channels.get(usize::from(track)).map_or(0, |c| c.mixer_track)
    } else {
        track
    };
    let routes: Vec<String> = (0..g.outputs.len())
        .map(|i| {
            let to = if i == 0 { Some(main_track) } else { g.destination(i) };
            match to {
                Some(t) => format!("Output {} -> {}", i + 1, crate::mixer::name(s, t)),
                None => format!("Output {} -> not routed", i + 1),
            }
        })
        .collect();
    let unrouted = (1..g.outputs.len()).any(|i| g.destination(i).is_none());
    let unsupported = g.notes.iter().find_map(|n| n.kind().err());
    let notes: Vec<String> = g.notes.iter().filter_map(|n| n.kind().ok()).map(|n| n.description()).collect();
    let warn = if let Some(ref e) = unsupported {
        format!("{e}; playback keeps the serial audio route and leaves the notes unchanged. {}.", routes.join("; "))
    } else if slot != crate::container::CHANNEL && !notes.is_empty() {
        format!("These note processors are saved, but a slot container has no notes; they act on a channel's container. {}.", routes.join("; "))
    } else if notes.is_empty() {
        if unrouted {
            format!("No note processors. Choose Outputs... to route every extra output. {}.", routes.join("; "))
        } else {
            routes.join("; ")
        }
    } else if unrouted {
        format!("Notes: {}. Choose Outputs... to route every extra output. {}.", notes.join(" -> "), routes.join("; "))
    } else {
        format!("Notes: {}. {}", notes.join(" -> "), routes.join("; "))
    };
    let col = if unsupported.is_some() || unrouted || (slot != crate::container::CHANNEL && !notes.is_empty()) { t.a(0xd8a33c) } else { t.text_dim };
    ui.c.text_clip(Font::new(12), x0 + 10.0, win.bottom() - 22.0, W - 20.0, col, &warn);
}
