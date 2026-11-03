    #[test]
    fn piano_target_channel() {
        use crate::playlist::Mark;
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.z = vec![Win::PianoRoll];
        app.state.selected_channel = 0;
        // Notes on Kick, Clap and Hat (rows 0, 1, 2); Snare and Pluck have none.
        let p = piano::pattern_mut(&mut app.state);
        for ch in 0..3u16 {
            p.notes.push(Note::step(ch, u32::from(ch) * 24));
        }
        frame(&mut app, &mut c, None);
        let b = hit_rect(&app, &Action::PianoTarget);
        let hint = app.hits.iter().rev().find(|h| h.action == Action::PianoTarget).map(|h| h.hint.to_string()).unwrap();
        assert!(hint.starts_with("Target channel"), "{hint}");

        // Click: the menu 4 px left of the name, 11 px below the title's centre (§2).
        click(&mut app, &mut c, b);
        let menu = app.state.playlist.popup.clone().expect("channel menu");
        assert_eq!((menu.x, menu.y), (b.x - 2.0, b.cy() + 11.0));
        let labels: Vec<&str> = menu.items.iter().map(|i| i.label.as_ref()).collect();
        assert_eq!(
            labels,
            ["Current pattern", "Kick", "Clap", "Hat", "Unsorted", "Kick", "Clap", "Hat", "Snare", "Pluck", "", "Auto locate"]
        );
        let ticked: Vec<bool> = menu.items.iter().map(|i| i.mark == Mark::Check(true)).collect();
        assert_eq!(ticked, [false, true, true, true, false, true, true, true, false, false, false, false]);
        assert!(!menu.items[11].enabled, "Auto locate is not available yet");

        // Picking Snare retargets the piano roll; the rack follows.
        let i = menu.items.iter().position(|i| i.label == "Snare").unwrap();
        app.act(Action::PlPopup(i));
        assert_eq!(app.state.selected_channel, 3);
        assert!(app.state.rack.sel.contains(&3) && app.state.rack.sel.len() == 1);
        assert_eq!(app.state.message.as_deref(), Some("Piano roll: Snare"));
        assert!(app.state.playlist.popup.is_none());

        // A filter group gets its own section, before Unsorted.
        app.state.rack.groups.push(crate::rack::Group { name: "Drums".into(), members: [0, 3].into_iter().collect() });
        let menu = crate::piano_tools::target_menu(&app.state, 0.0, 0.0);
        let labels: Vec<&str> = menu.items.iter().map(|i| i.label.as_ref()).collect();
        assert_eq!(labels[4..], ["Drums", "Kick", "Snare", "Unsorted", "Clap", "Hat", "Pluck", "", "Auto locate"]);
        app.state.rack.groups.clear();

        // Wheel (§3): from Snare (no notes) down gives Kick (wrapping), up from Snare gives Hat.
        frame(&mut app, &mut c, None);
        let wheel = |app: &mut App, c: &mut Canvas, dy: f32| {
            let b = hit_rect(app, &Action::PianoTarget);
            app.mouse_move(b.cx(), b.cy());
            app.wheel(0.0, dy, false, false);
            frame(app, c, None);
            app.state.selected_channel
        };
        assert_eq!(wheel(&mut app, &mut c, -1.0), 0);
        assert_eq!(wheel(&mut app, &mut c, -1.0), 1);
        assert_eq!(wheel(&mut app, &mut c, -1.0), 2);
        assert_eq!(wheel(&mut app, &mut c, -1.0), 0, "wraps");
        assert_eq!(wheel(&mut app, &mut c, 1.0), 2, "up goes back");
        assert!(app.state.rack.sel.contains(&2));
        app.state.selected_channel = 3;
        assert_eq!(wheel(&mut app, &mut c, 1.0), 2, "up from a channel without notes");
        // With one channel holding notes, every notch stays on it.
        piano::pattern_mut(&mut app.state).notes.retain(|n| n.channel == 1);
        assert_eq!(wheel(&mut app, &mut c, -1.0), 1);
        assert_eq!(wheel(&mut app, &mut c, -1.0), 1);
        assert_eq!(wheel(&mut app, &mut c, 1.0), 1);

        // Right-click: the channel's settings window (§4).
        let b = hit_rect(&app, &Action::PianoTarget);
        app.mouse_move(b.cx(), b.cy());
        app.mouse_down_right();
        assert_eq!(app.state.pending_open, Some(1), "Clap's sampler window opens");
    }

    /// The browser's title-bar arrows (spec: ui/browser.md §4, "Title-bar icons"): Reread keeps open nodes,
    /// Collapse closes them.
    #[test]
    fn browser_title_icons() {
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.browser_tab = 2;
        app.state.browser_open.insert("2:Plugin library/Effects".into());
        frame(&mut app, &mut c, None);
        let hint = |app: &App, a: Action| app.hits.iter().rev().find(|h| h.action == a).map(|h| h.hint.to_string()).unwrap();
        assert_eq!(hint(&app, Action::BrowserReread), "Reread structure (Ctrl+R)");
        assert_eq!(hint(&app, Action::BrowserCollapse), "Collapse structure (Ctrl+Up)");
        let r = hit_rect(&app, &Action::BrowserReread);
        click(&mut app, &mut c, r);
        assert!(app.state.browser_open.contains("2:Plugin library/Effects"), "Reread keeps open nodes");
        let r = hit_rect(&app, &Action::BrowserCollapse);
        click(&mut app, &mut c, r);
        assert!(!app.state.browser_open.iter().any(|k| k.starts_with("2:")), "Collapse closes the tab's nodes");
    }

    /// The piano roll's magnet button (spec: ui/piano-snap.md): its two menus, where clicks, moves, resizes and the
    /// snap's commands land under each kind of setting, Alt, the zoom-dependent Line unit, kept over a reset.
    #[test]
    fn piano_snap() {
        use crate::snap::EditorSnap;
        use crate::piano_tools::NCmd;
        use crate::playlist::{Cmd, Mark, Snap};
        use winit::keyboard::{Key, NamedKey};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.z = vec![Win::PianoRoll];
        app.state.selected_channel = 0;
        frame(&mut app, &mut c, None);
        let ch = piano::channel(&app.state).unwrap();
        let g = app.piano_geom().unwrap();
        assert_eq!(g.ppt, 1.0, "96 px per beat at PPQ 96: Line is a step");
        let row = |k: i32| g.y_of(k) + 8.0;
        let x_of = |app: &App, t: f32| app.piano_geom().unwrap().x_of(t);
        let mine = |app: &App| -> Vec<(u32, u32, u16)> {
            piano::notes(&app.state).iter().filter(|n| n.channel == ch).map(|n| (n.position, n.length, n.key)).collect()
        };
        let put = |app: &mut App, notes: &[(u32, u32, u16)]| {
            let p = piano::pattern_mut(&mut app.state);
            p.notes.retain(|n| n.channel != ch);
            for &(pos, length, key) in notes {
                p.notes.push(Note { length, key, ..Note::step(ch, pos) });
            }
            p.notes.sort_by_key(|n| n.position);
            app.state.piano.clear_selection();
        };
        let click = |app: &mut App, c: &mut Canvas, x: f32, y: f32| {
            app.mouse_move(x, y);
            app.mouse_down();
            app.mouse_up();
            frame(app, c, None);
        };
        // A left-drag along C5's row between two ticks.
        let drag = |app: &mut App, c: &mut Canvas, t0: f32, t1: f32| {
            frame(app, c, None);
            let (x0, x1, y) = (x_of(app, t0), x_of(app, t1), row(60));
            app.mouse_move(x0, y);
            app.mouse_down();
            for i in 1..=10 {
                app.mouse_move(x0 + (x1 - x0) * i as f32 / 10.0, y);
            }
            app.mouse_up();
            frame(app, c, None);
        };
        // A Draw click at tick `t` on C5's empty row; returns where the new note starts (and removes it).
        let place = |app: &mut App, c: &mut Canvas, t: f32| -> u32 {
            frame(app, c, None);
            let x = x_of(app, t);
            click(app, c, x, row(60));
            let p = piano::pattern_mut(&mut app.state);
            let i = p.notes.iter().position(|n| n.channel == ch && n.key == 60).expect("a note was drawn");
            let at = p.notes.remove(i).position;
            frame(app, c, None);
            at
        };
        let pick = |app: &mut App, p: EditorSnap| crate::playlist::run(app, Cmd::Piano(NCmd::Snap(p)));
        let hint = |app: &App| app.hits.iter().rev().find(|h| h.action == Action::PianoSnapMenu).map(|h| h.hint.to_string()).unwrap();
        let key = |app: &mut App, k: &str, ctrl: bool, shift: bool| {
            app.shift = shift;
            app.key(&Key::Character(k.into()), ctrl, false);
            app.shift = false;
        };
        let nudge = |app: &mut App| {
            app.shift = true;
            app.key(&Key::Named(NamedKey::ArrowRight), false, false);
            app.shift = false;
        };

        // The magnet is live: its hint names the setting; a click opens the Snap menu 11 px left of and below its
        // centre, Main marked (§1, §2, §8).
        let b = hit_rect(&app, &Action::PianoSnapMenu);
        assert_eq!(hint(&app), "Snap to grid: Main (right-click: stock note lengths)");
        click(&mut app, &mut c, b.cx(), b.cy());
        let pop = app.state.playlist.popup.take().expect("the Snap menu opens");
        assert_eq!((pop.x, pop.y), (b.cx() - 11.0, b.cy() + 11.0));
        let marked: Vec<&str> = pop.items.iter().filter(|i| i.mark == Mark::Radio(true)).map(|i| &*i.label).collect();
        assert_eq!(marked, vec!["Main"]);
        assert_eq!(pop.items.iter().filter(|i| !i.sep).count(), 17);
        assert_eq!(pick(&mut app, EditorSnap::Grid(Snap::ThirdBeat)).as_deref(), Some("Piano roll snap: 1/3 beat"));
        frame(&mut app, &mut c, None);
        assert_eq!(hint(&app), "Snap to grid: 1/3 beat (right-click: stock note lengths)");
        assert_eq!((place(&mut app, &mut c, 40.0), place(&mut app, &mut c, 50.0)), (32, 64));

        // Placing (§3, §5): the nearest unit, Cell down, (none) the tick under the pointer, Alt never snaps.
        for (p, t, alt, want) in [
            (EditorSnap::Main, 40.0, false, 48),
            (EditorSnap::Grid(Snap::Line), 30.0, false, 24),
            (EditorSnap::Grid(Snap::Step), 40.0, false, 48),
            (EditorSnap::Grid(Snap::HalfBeat), 40.0, false, 48),
            (EditorSnap::Grid(Snap::Cell), 47.0, false, 24),
            (EditorSnap::Grid(Snap::None), 40.0, false, 40),
            (EditorSnap::Main, 40.0, true, 40),
        ] {
            pick(&mut app, p);
            app.alt = alt;
            assert_eq!(place(&mut app, &mut c, t), want, "{p:?} at {t}, alt {alt}");
            app.alt = false;
        }

        // Line follows the zoom (§4): 1/2 beat at 30 px per beat, a step at 600, no snapping from 768 up.
        pick(&mut app, EditorSnap::Main);
        for (ppb, t, want) in [(30.0, 70.0, 48), (30.0, 80.0, 96), (600.0, 40.0, 48), (800.0, 40.3, 40)] {
            app.state.piano.px_per_beat = ppb;
            frame(&mut app, &mut c, None);
            assert_eq!(place(&mut app, &mut c, t), want, "{ppb} px per beat");
        }
        app.state.piano.px_per_beat = 96.0;
        frame(&mut app, &mut c, None);

        // Events: the edges of a note from 240 to 312 catch clicks within 5 px; Markers the same with a marker.
        put(&mut app, &[(240, 72, 64)]);
        pick(&mut app, EditorSnap::Events);
        for (t, want) in [(244.0, 240), (247.0, 247), (309.0, 312), (318.0, 318)] {
            assert_eq!(place(&mut app, &mut c, t), want, "Events at {t}");
        }
        pick(&mut app, EditorSnap::Markers);
        piano::pattern_mut(&mut app.state).markers = vec![os_model::TimeMarker::new(192, "M")];
        for (t, want) in [(187.0, 192), (195.0, 192), (198.0, 198), (244.0, 244)] {
            assert_eq!(place(&mut app, &mut c, t), want, "Markers at {t}");
        }
        piano::pattern_mut(&mut app.state).markers.clear();

        // Moving snaps the distance, so an off-grid note keeps its offset; a resize snaps the change of length,
        // down to one unit (one tick without a unit); Alt moves freely (§5, §8).
        pick(&mut app, EditorSnap::Main);
        put(&mut app, &[(40, 48, 60)]);
        drag(&mut app, &mut c, 50.0, 80.0);
        assert_eq!(mine(&app), vec![(64, 48, 60)]);
        app.alt = true;
        drag(&mut app, &mut c, 74.0, 104.0);
        app.alt = false;
        assert_eq!(mine(&app), vec![(94, 48, 60)]);
        put(&mut app, &[(40, 48, 60)]);
        drag(&mut app, &mut c, 87.0, 104.0);
        assert_eq!(mine(&app), vec![(40, 72, 60)], "the end dragged +17 ticks lands on 112");
        drag(&mut app, &mut c, 111.0, 20.0);
        assert_eq!(mine(&app), vec![(40, 24, 60)], "a resize keeps one unit");
        pick(&mut app, EditorSnap::Grid(Snap::None));
        drag(&mut app, &mut c, 63.0, 20.0);
        assert_eq!(mine(&app), vec![(40, 1, 60)], "one tick without a unit");
        // Events: the dragged start goes to another note's edge within 5 px.
        pick(&mut app, EditorSnap::Events);
        put(&mut app, &[(40, 48, 60), (240, 72, 64)]);
        drag(&mut app, &mut c, 50.0, 247.0);
        assert!(mine(&app).contains(&(240, 48, 60)), "{:?}", mine(&app));

        // Shift+arrows, Discard lengths and Quick chop use the unit; without one a tick, length 0, one-tick pieces.
        // Quick quantize keeps the global snap (Line) (§6).
        for (p, moved, discarded) in [(EditorSnap::Grid(Snap::None), 101, 0), (EditorSnap::Main, 124, 24), (EditorSnap::Grid(Snap::Beat), 196, 96)] {
            pick(&mut app, p);
            put(&mut app, &[(100, 96, 60)]);
            nudge(&mut app);
            key(&mut app, "d", false, true);
            assert_eq!(mine(&app), vec![(moved, discarded, 60)], "{p:?}");
        }
        pick(&mut app, EditorSnap::Grid(Snap::None));
        put(&mut app, &[(96, 96, 60)]);
        key(&mut app, "u", true, false);
        assert_eq!(mine(&app).len(), 96);
        assert!(mine(&app).iter().all(|n| n.1 == 1));
        pick(&mut app, EditorSnap::Grid(Snap::HalfBeat));
        put(&mut app, &[(96, 96, 60)]);
        key(&mut app, "u", true, false);
        assert_eq!(mine(&app), vec![(96, 48, 60), (144, 48, 60)]);
        // Quick quantize uses the piano roll's snap: nothing changes without a unit (spec: ui/global-snap.md §3).
        pick(&mut app, EditorSnap::Grid(Snap::None));
        put(&mut app, &[(30, 40, 60)]);
        key(&mut app, "q", true, false);
        assert_eq!(mine(&app), vec![(30, 40, 60)]);
        pick(&mut app, EditorSnap::Main);
        key(&mut app, "q", true, false);
        assert_eq!(mine(&app), vec![(24, 48, 60)]);
        pick(&mut app, EditorSnap::Grid(Snap::None));

        // Right-click: Stock note lengths at the mouse; Beat makes the next notes 96 ticks, the snap stays (§7).
        put(&mut app, &[]);
        app.mouse_move(b.cx(), b.cy());
        app.mouse_down_right();
        app.mouse_up();
        let pop = app.state.playlist.popup.take().expect("Stock note lengths opens");
        assert_eq!((pop.x, pop.y), (b.cx(), b.cy()));
        assert!(pop.items[0].head && pop.items[0].label == "Stock note lengths");
        assert!(pop.items.iter().any(|i| i.cmd == Cmd::Piano(NCmd::StockLength(Snap::Beat))));
        let m = crate::playlist::run(&mut app, Cmd::Piano(NCmd::StockLength(Snap::Beat)));
        assert_eq!(m.as_deref(), Some("New notes: Beat"));
        assert_eq!(app.state.piano.snap, EditorSnap::Grid(Snap::None));
        frame(&mut app, &mut c, None);
        assert_eq!(place(&mut app, &mut c, 50.0), 50);
        assert_eq!(app.state.piano.last_len, 96, "the drawn note was a beat long");

        // The setting outlives a project change (§8).
        app.state.piano.reset();
        assert_eq!(app.state.piano.snap, EditorSnap::Grid(Snap::None));
    }

    /// The main toolbar's snap panel (spec: ui/global-snap.md): the combo's menu, right-click and wheel, the magnet
    /// and Backspace, and what follows the global snap: the piano roll's and the playlist's Main, Quick quantize.
    #[test]
    fn global_snap() {
        use crate::piano_tools::NCmd;
        use crate::playlist::{self as pl, Cmd, Mark, Snap};
        use crate::snap::EditorSnap;
        use winit::keyboard::{Key, NamedKey};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.z = vec![Win::PianoRoll];
        app.state.selected_channel = 0;
        let ch = piano::channel(&app.state).unwrap();
        piano::pattern_mut(&mut app.state).notes.retain(|n| n.channel != ch);
        frame(&mut app, &mut c, None);
        let hint = |app: &App, a: Action| app.hits.iter().rev().find(|h| h.action == a).map(|h| h.hint.to_string()).unwrap();
        let now = |app: &App| app.state.global_snap.now;
        let mine = |app: &App| -> Vec<(u32, u32)> { piano::notes(&app.state).iter().filter(|n| n.channel == ch).map(|n| (n.position, n.length)).collect() };
        let put = |app: &mut App, pos: u32, length: u32| {
            let p = piano::pattern_mut(&mut app.state);
            p.notes.retain(|n| n.channel != ch);
            p.notes.push(Note { length, key: 60, ..Note::step(ch, pos) });
            app.state.piano.clear_selection();
        };
        // A Draw click at tick `t` on C5's empty row; returns where the new note starts (and removes it).
        let place = |app: &mut App, c: &mut Canvas, t: f32| -> u32 {
            frame(app, c, None);
            let g = app.piano_geom().unwrap();
            click(app, c, Rect::new(g.x_of(t), g.y_of(60) + 8.0, 0.0, 0.0));
            frame(app, c, None);
            let p = piano::pattern_mut(&mut app.state);
            let i = p.notes.iter().position(|n| n.channel == ch && n.key == 60).expect("a note was drawn");
            p.notes.remove(i).position
        };
        let backspace = |app: &mut App| app.key(&Key::Named(NamedKey::Backspace), false, false);
        let quantize = |app: &mut App| app.key(&Key::Character("q".into()), true, false);

        // The panel is live: Line by default, the magnet left of the combo, the hints of §6.
        let combo = hit_rect(&app, &Action::GlobalSnap);
        let magnet = hit_rect(&app, &Action::GlobalSnapToggle);
        assert_eq!(now(&app), Snap::Line);
        assert!(magnet.right() <= combo.x && magnet.cy() == combo.cy());
        assert_eq!(hint(&app, Action::GlobalSnap), crate::snap::HINT);
        assert_eq!(hint(&app, Action::GlobalSnapToggle), crate::snap::MAGNET_HINT);

        // A click opens the Snap menu under the combo, Line marked (§2); Beat is picked from it.
        click(&mut app, &mut c, combo);
        let pop = app.state.playlist.popup.take().expect("the Snap menu opens");
        assert_eq!((pop.x, pop.y), (combo.x, combo.bottom()));
        assert_eq!(pop.items.iter().filter(|i| !i.sep).count(), 14);
        assert_eq!((pop.items[0].label.as_ref(), pop.items[0].mark), ("Line", Mark::Radio(true)));
        assert!(pop.items.iter().any(|i| i.label == "Beat" && i.cmd == Cmd::GlobalSnap(Snap::Beat)));
        assert_eq!(pl::run(&mut app, Cmd::GlobalSnap(Snap::Beat)).as_deref(), Some("Main snap: Beat"));

        // The piano roll's Main follows it: Draw clicks at ticks 40 and 60 land on the nearest beat (§3).
        assert_eq!(app.state.piano.snap, EditorSnap::Main);
        assert_eq!((place(&mut app, &mut c, 40.0), place(&mut app, &mut c, 60.0)), (0, 96));

        // Quick quantize uses the piano roll's snap: Main reads the global Beat, Step its own unit (§3).
        put(&mut app, 40, 48);
        quantize(&mut app);
        assert_eq!(mine(&app), vec![(0, 96)]);
        pl::run(&mut app, Cmd::Piano(NCmd::Snap(EditorSnap::Grid(Snap::Step))));
        put(&mut app, 40, 48);
        quantize(&mut app);
        assert_eq!(mine(&app), vec![(48, 48)], "the piano roll's Step, not the global Beat");
        pl::run(&mut app, Cmd::Piano(NCmd::Snap(EditorSnap::Main)));

        // The magnet turns snapping off: Main places at the tick, Quick quantize changes nothing (§1, §6).
        click(&mut app, &mut c, magnet);
        assert_eq!(now(&app), Snap::None);
        assert_eq!(app.state.message.as_deref(), Some("Main snap: (none)"));
        assert_eq!(place(&mut app, &mut c, 43.0), 43);
        put(&mut app, 40, 48);
        quantize(&mut app);
        assert_eq!(mine(&app), vec![(40, 48)]);
        // Backspace with the piano roll focused: back to the last setting; it deletes nothing.
        backspace(&mut app);
        assert_eq!(now(&app), Snap::Beat);
        assert_eq!(mine(&app), vec![(40, 48)]);

        // The wheel over the combo: down to Bar, down again wraps to Line, up goes back to Bar (§1).
        frame(&mut app, &mut c, None);
        app.mouse_move(combo.cx(), combo.cy());
        app.wheel(0.0, -1.0, false, false);
        assert_eq!(now(&app), Snap::Bar);
        app.wheel(0.0, -1.0, false, false);
        assert_eq!(now(&app), Snap::Line);
        app.wheel(0.0, 1.0, false, false);
        assert_eq!((now(&app), app.state.message.as_deref()), (Snap::Bar, Some("Main snap: Bar")));
        // Right-click: Stock note lengths at the mouse; the snap stays (§4).
        app.mouse_down_right();
        app.mouse_up();
        let pop = app.state.playlist.popup.take().expect("Stock note lengths opens");
        assert_eq!((pop.x, pop.y), (combo.cx(), combo.cy()));
        assert!(pop.items[0].head && pop.items[0].label == "Stock note lengths");
        assert_eq!(now(&app), Snap::Bar);

        // The playlist's Main follows it too: clicks at 2.78, 4.67 and 7.62 bars land on the nearest bar, and with
        // the global Beat on the nearest beat (§3, ui/playlist.md §4).
        app.state.z = vec![Win::Playlist];
        app.state.playlist.row_h = pl::ROW_H;
        pl::arr_mut(&mut app.state).clips.clear();
        frame(&mut app, &mut c, None);
        let g = pl::PGeom::new(&app.state, app.pl_grid().expect("playlist grid"));
        let ppq = u32::from(app.state.project.ppq);
        assert_eq!(app.state.playlist.snap, EditorSnap::Main);
        let draw = |app: &mut App, c: &mut Canvas, bars: f32, track: u16| -> u32 {
            click(app, c, Rect::new(g.x_of(bars * (4 * ppq) as f32), g.y_of(track) + 30.0, 0.0, 0.0));
            pl::clips(&app.state).iter().find(|c| c.track == track).expect("a clip was placed").position
        };
        assert_eq!(draw(&mut app, &mut c, 2.78, 0), 3 * 4 * ppq);
        assert_eq!(draw(&mut app, &mut c, 4.67, 1), 5 * 4 * ppq);
        assert_eq!(draw(&mut app, &mut c, 7.62, 2), 8 * 4 * ppq);
        app.state.global_snap.set(Snap::Beat);
        assert_eq!(draw(&mut app, &mut c, 2.78, 3), 11 * ppq);
        // Its own menu: Main first and marked; Events and Markers last, radios that work.
        let m = pl::snap_menu(&app.state, 0.0, 0.0);
        assert_eq!((m.items[0].label.as_ref(), m.items[0].mark), ("Main", Mark::Radio(true)));
        let n = m.items.len();
        assert_eq!(m.items[n - 2..].iter().map(|i| (i.label.as_ref(), i.enabled, i.cmd.clone())).collect::<Vec<_>>(), [("Events", true, pl::Cmd::Snap(EditorSnap::Events)), ("Markers", true, pl::Cmd::Snap(EditorSnap::Markers))]);
        // Markers: a click within 7 px of a time marker lands on it (FL: 6.4 px did, 7.6 px did not), farther off on
        // the tick under the pointer; Events: on a clip's start or end the same way. The song position on the ruler
        // follows Markers too.
        pl::arr_mut(&mut app.state).clips.clear();
        pl::arr_mut(&mut app.state).markers = vec![os_model::TimeMarker::new(1000, "M")];
        app.state.playlist.snap = EditorSnap::Markers;
        let at = |app: &App, px: f32| pl::snap_place(&app.state, &g, g.tick_at(g.x_of(1000.0) + px), false);
        assert_eq!((at(&app, 4.0), at(&app, -5.0)), (1000, 1000));
        assert_eq!(at(&app, 8.0), g.tick_at(g.x_of(1000.0) + 8.0) as u32, "farther off: no snapping");
        assert_eq!(pl::snap_place(&app.state, &g, 1000.0 + 3.0 / g.ppt, true), (1000.0 + 3.0 / g.ppt) as u32, "Alt: no snapping");
        assert_eq!(pl::grid_snap(&app.state), Snap::None);
        app.state.playlist.snap = EditorSnap::Events;
        assert_eq!(at(&app, 3.0), g.tick_at(g.x_of(1000.0) + 3.0) as u32, "markers are not events");
        pl::arr_mut(&mut app.state).clips.push(os_model::Clip::new(500, 500, 9, os_model::ClipSource::Pattern(1)));
        assert_eq!((at(&app, 3.0), at(&app, -4.0)), (1000, 1000), "the clip's end");
        app.state.playlist.snap = EditorSnap::Main;
        pl::arr_mut(&mut app.state).clips.clear();
        pl::arr_mut(&mut app.state).markers.clear();
        // Backspace with the playlist focused toggles too.
        backspace(&mut app);
        assert_eq!(now(&app), Snap::None);
        backspace(&mut app);
        assert_eq!(now(&app), Snap::Beat);

        // Program settings: opening another project keeps all three (§6).
        app.state.playlist.snap = EditorSnap::Grid(Snap::Cell);
        app.state.piano.reset();
        app.state.playlist.reset();
        assert_eq!((now(&app), app.state.piano.snap, app.state.playlist.snap), (Snap::Beat, EditorSnap::Main, EditorSnap::Grid(Snap::Cell)));
    }

    /// Piano-roll shortcuts (spec: ui/piano-roll.md §3a, §10a, §11): edits, tool menus, ghosts, markers, channel
    /// cycling and the wheel zooms, driven through `App::key` / `App::wheel` like the shell does.
    #[test]
    fn piano_shortcuts() {
        use crate::piano_tools::NCmd;
        use crate::playlist::Cmd;
        use winit::keyboard::{Key, NamedKey};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.z = vec![Win::PianoRoll];
        app.state.selected_channel = 0;
        frame(&mut app, &mut c, None);
        let ch = piano::channel(&app.state).unwrap();
        let mine = |app: &App| -> Vec<(u32, u32, u16)> {
            piano::notes(&app.state).iter().filter(|n| n.channel == ch).map(|n| (n.position, n.length, n.key)).collect()
        };
        let put = |app: &mut App, notes: &[(u32, u32, u16)]| {
            let p = piano::pattern_mut(&mut app.state);
            p.notes.retain(|n| n.channel != ch);
            for &(pos, length, key) in notes {
                p.notes.push(Note { length, key, ..Note::step(ch, pos) });
            }
            p.notes.sort_by_key(|n| n.position);
            app.state.piano.clear_selection();
        };
        let key = |app: &mut App, k: &str, ctrl: bool, alt: bool, shift: bool| {
            app.shift = shift;
            app.key(&Key::Character(k.into()), ctrl, alt);
            app.shift = false;
        };

        // Ctrl+Q quantizes starts and lengths, Shift+Q only starts (nothing selected = every note).
        put(&mut app, &[(30, 40, 60)]);
        key(&mut app, "q", true, false, false);
        assert_eq!(mine(&app), vec![(24, 48, 60)]);
        put(&mut app, &[(30, 40, 60)]);
        key(&mut app, "Q", false, false, true);
        assert_eq!(mine(&app), vec![(24, 40, 60)]);
        // Ctrl+Z undoes it.
        key(&mut app, "z", true, false, false);
        assert_eq!(mine(&app), vec![(30, 40, 60)]);

        // Shift+D: one step long; Ctrl+U chops at the steps; Ctrl+G glues them back.
        put(&mut app, &[(0, 96, 60)]);
        key(&mut app, "D", false, false, true);
        assert_eq!(mine(&app), vec![(0, 24, 60)]);
        put(&mut app, &[(0, 72, 60)]);
        key(&mut app, "u", true, false, false);
        assert_eq!(mine(&app), vec![(0, 24, 60), (24, 24, 60), (48, 24, 60)]);
        key(&mut app, "g", true, false, false);
        assert_eq!(mine(&app), vec![(0, 72, 60)]);
        // Ctrl+L: legato; the last note lasts to the end of the pattern's bar (spec: ui/piano-keys.md §4 rule 5).
        put(&mut app, &[(0, 10, 60), (48, 10, 62)]);
        key(&mut app, "l", true, false, false);
        assert_eq!(mine(&app), vec![(0, 48, 60), (48, 336, 62)]);
        // A selection limits the edit and follows the results.
        put(&mut app, &[(0, 72, 60), (96, 72, 62)]);
        let first = piano::notes(&app.state).iter().position(|n| n.channel == ch && n.key == 60).unwrap();
        app.state.piano.select(app.state.pattern, ch, vec![first]);
        key(&mut app, "u", true, false, false);
        assert_eq!(mine(&app), vec![(0, 24, 60), (24, 24, 60), (48, 24, 60), (96, 72, 62)]);
        assert_eq!(app.state.piano.selection(app.state.pattern, Some(ch)).len(), 3);
        // Ctrl+X cuts the selection, Ctrl+D deselects.
        key(&mut app, "x", true, false, false);
        assert_eq!(mine(&app), vec![(96, 72, 62)]);
        assert_eq!(app.state.piano.clipboard.len(), 3);

        // Alt+R opens Randomize (spec: ui/piano-randomize.md): Reset's settings write a new melody over the pattern's
        // bars, a note a step from C4 to B6, glued and selected; a notch of Octave moves it an octave up, the seed arrows
        // give another melody and back, the scale menu keeps the keys on the scale; with Pattern off VEL moves the
        // notes' velocities; Accept is one undo step, the settings are kept and × puts the notes back.
        {
            use crate::piano_dlg::{Act as DAct, Knob as DKnob, LCmd, Tool};
            put(&mut app, &[(0, 96, 60), (96, 48, 62)]);
            let before = mine(&app);
            let (grid, end) = (crate::piano_snap::unit(&app.state).unwrap_or(1), piano::pattern_end(&app.state));
            key(&mut app, "r", false, true, false);
            assert!(app.state.shown(Win::PianoDlg) && app.state.piano_dlg.tool == Tool::Randomize);
            let first = mine(&app);
            assert!(first.iter().all(|&(p, _, k)| p % grid == 0 && (48..84).contains(&k)), "{first:?}");
            assert_eq!(first.iter().map(|n| n.1).sum::<u32>(), end, "the steps tile the pattern");
            assert_eq!(app.state.piano.selection(app.state.pattern, Some(ch)).len(), first.len(), "the new notes are selected");
            frame(&mut app, &mut c, None);
            let k = hit_rect(&app, &Action::Knob(KnobId::PianoDlg(DKnob::RandOctave)));
            app.mouse_move(k.cx(), k.cy());
            app.wheel(0.0, 1.0, false, false);
            assert_eq!(app.hint.as_ref(), "Octave: 5");
            let up: Vec<_> = first.iter().map(|&(p, l, k)| (p, l, k + 12)).collect();
            assert_eq!(mine(&app), up, "the same draws an octave up");
            app.act(Action::PianoDlg(DAct::Seed { levels: false, up: true }));
            assert_ne!(mine(&app), up);
            app.act(Action::PianoDlg(DAct::Seed { levels: false, up: false }));
            assert_eq!(mine(&app), up, "‹ undoes ›");
            app.act(Action::PianoDlg(DAct::ScaleMenu));
            let pop = app.state.playlist.popup.take().expect("the scale box opens its menu");
            assert_eq!(pop.items.iter().filter(|i| matches!(i.cmd, Cmd::Limit(LCmd::Scale(_)))).count(), 89, "(none) and 88 entries");
            crate::playlist::run(&mut app, Cmd::Limit(LCmd::Scale(crate::chords::FIRST_SCALE)));
            assert!(mine(&app).iter().all(|n| [0, 2, 4, 5, 7, 9, 11].contains(&(n.2 % 12))), "on C major");
            frame(&mut app, &mut c, None);
            let k = hit_rect(&app, &Action::Knob(KnobId::PianoDlg(DKnob::RandLevel(1))));
            app.mouse_move(k.cx(), k.cy());
            (0..11).for_each(|_| app.wheel(0.0, 1.0, false, false));
            assert_eq!(app.hint.as_ref(), "Velocity: +100%");
            app.act(Action::PianoDlg(DAct::RandPattern));
            assert_eq!(mine(&app), before, "Pattern off: the notes stay");
            assert!(piano::notes(&app.state).iter().any(|n| n.channel == ch && n.velocity != 100), "VEL moved them");
            app.act(Action::PianoDlg(DAct::RandPattern));
            frame(&mut app, &mut c, None);
            app.act(Action::PianoDlg(DAct::Accept));
            assert!(!app.state.shown(Win::PianoDlg));
            let kept = mine(&app);
            assert!(piano::undo(&mut app.state));
            assert_eq!(mine(&app), before, "one undo step");
            key(&mut app, "r", false, true, false);
            assert_eq!((app.state.piano_dlg.rand.octave, app.state.piano_dlg.rand.scale), (5, crate::chords::FIRST_SCALE), "kept");
            assert_eq!(mine(&app), kept, "the same seed and settings, the same notes");
            app.act(Action::CloseWin(Win::PianoDlg));
            assert_eq!(mine(&app), before, "× puts the notes back");
        }
        put(&mut app, &[(0, 96, 60), (0, 96, 64), (0, 96, 67)]);

        // Alt+S opens the Strum dialog (spec: ui/piano-strum.md): it previews, × puts the notes back, Accept is one
        // undo step and the next opening starts from the accepted settings.
        use crate::knobs::KnobId;
        use crate::strum::{Act, Knob};
        let chord = |app: &App| mine(app).iter().map(|n| n.0).collect::<Vec<_>>();
        key(&mut app, "s", false, true, false);
        assert!(app.state.shown(Win::Strum) && app.state.in_front(Win::Strum));
        frame(&mut app, &mut c, None);
        let time = hit_rect(&app, &Action::Knob(KnobId::Strum(Knob::Time)));
        app.mouse_move(time.cx(), time.cy());
        for _ in 0..29 {
            app.wheel(0.0, 1.0, false, false);
        }
        assert_eq!(chord(&app), vec![0, 22, 44], "29 notches: FL's 0:22 per note, previewed at once");
        assert_eq!(app.hint.as_ref(), "Start strength: 0:22");
        let close = hit_rect(&app, &Action::CloseWin(Win::Strum));
        app.mouse_move(close.cx(), close.cy());
        app.mouse_down();
        app.mouse_up();
        assert!(!app.state.shown(Win::Strum));
        assert_eq!(chord(&app), vec![0, 0, 0], "× puts the notes back");
        key(&mut app, "s", false, true, false);
        assert_eq!(chord(&app), vec![0, 0, 0], "× forgot the change");
        app.state.set_knob(KnobId::Strum(Knob::Time), 0.5 + 29.0 / 64.0);
        frame(&mut app, &mut c, None);
        let accept = hit_rect(&app, &Action::Strum(Act::Accept));
        app.mouse_move(accept.cx(), accept.cy());
        app.mouse_down();
        app.mouse_up();
        assert!(!app.state.shown(Win::Strum));
        assert_eq!(chord(&app), vec![0, 22, 44]);
        assert!(piano::undo(&mut app.state));
        assert_eq!(chord(&app), vec![0, 0, 0], "one undo step");
        key(&mut app, "s", false, true, false);
        assert_eq!(chord(&app), vec![0, 22, 44], "reopened on the accepted settings");
        app.act(Action::Strum(Act::Reset));
        assert_eq!(chord(&app), vec![0, 0, 0]);
        app.act(Action::CloseWin(Win::Strum));
        // VIEW > Close all windows (F12) closes it as × does.
        key(&mut app, "s", false, true, false);
        app.state.set_knob(KnobId::Strum(Knob::Time), 0.5 + 29.0 / 64.0);
        frame(&mut app, &mut c, None);
        assert_eq!(chord(&app), vec![0, 22, 44]);
        app.close_windows(CloseAll::All);
        assert!(!app.state.shown(Win::Strum));
        assert_eq!(chord(&app), vec![0, 0, 0], "F12 puts the notes back");
        app.state.z = vec![Win::PianoRoll];

        // Alt+A opens the Arpeggiate dialog (spec: ui/piano-arpeggiate.md): the Default pattern previews at once, ×
        // puts the notes back, a menu entry changes the preview, Accept is one undo step.
        {
            use crate::arpeggiate::{ACmd, Act as AAct, Knob as AKnob, Menu, Sync};
            put(&mut app, &[(0, 96, 60), (0, 96, 64), (0, 96, 67)]);
            let keys = |app: &App| mine(app).iter().map(|n| n.2).collect::<Vec<_>>();
            key(&mut app, "a", false, true, false);
            assert!(app.state.shown(Win::Arp) && app.state.in_front(Win::Arp));
            assert_eq!(keys(&app), vec![60, 64, 67, 60], "previewed with the Default pattern");
            app.act(Action::CloseWin(Win::Arp));
            assert_eq!(keys(&app), vec![60, 64, 67]);
            key(&mut app, "a", false, true, false);
            frame(&mut app, &mut c, None);
            let range = hit_rect(&app, &Action::Knob(KnobId::Arp(AKnob::Range)));
            app.mouse_move(range.cx(), range.cy());
            app.wheel(0.0, 1.0, false, false);
            assert_eq!(app.state.arp.set.range, 2);
            assert_eq!(app.hint.as_ref(), "Range: 2");
            assert_eq!(keys(&app), vec![60, 64, 67, 72], "the second repetition an octave up");
            app.act(Action::Arp(AAct::Menu(Menu::Sync)));
            let pop = app.state.playlist.popup.take().expect("Sync opens its menu");
            assert_eq!(pop.items.iter().map(|i| i.label.as_ref()).collect::<Vec<_>>(), ["Time", "Block", "Chord"]);
            crate::playlist::run(&mut app, Cmd::Arp(ACmd::Sync(Sync::Block)));
            assert_eq!(app.state.arp.set.sync, Sync::Block);
            app.act(Action::Arp(AAct::Menu(Menu::Pattern)));
            let pop = app.state.playlist.popup.take().expect("the pattern menu");
            assert_eq!(pop.items[0].label, "Default");
            frame(&mut app, &mut c, None);
            let accept = hit_rect(&app, &Action::Arp(AAct::Accept));
            app.mouse_move(accept.cx(), accept.cy());
            app.mouse_down();
            app.mouse_up();
            assert!(!app.state.shown(Win::Arp));
            assert_eq!(keys(&app), vec![60, 64, 67, 72]);
            assert!(piano::undo(&mut app.state));
            assert_eq!(keys(&app), vec![60, 64, 67], "one undo step");
            key(&mut app, "a", false, true, false);
            assert_eq!(app.state.arp.set.range, 2, "reopened on the accepted settings");
            app.act(Action::Arp(AAct::Reset));
            assert_eq!(keys(&app), vec![60, 64, 67, 60]);
            app.act(Action::CloseWin(Win::Arp));
        }

        // Alt+F, Alt+Y and Alt+X open the Flam, Flip and Scale levels dialogs in one tool window (spec:
        // ui/piano-flam.md, ui/piano-flip.md, ui/piano-level-scaling.md): each previews, × puts the notes back, Accept
        // is one undo step, and opening another puts the first one's preview back.
        {
            use crate::flam::Preset;
            use crate::piano_dlg::{Act as DAct, Knob as DKnob, Tool};
            let click = |app: &mut App, a: Action| {
                let r = hit_rect(app, &a);
                app.mouse_move(r.cx(), r.cy());
                app.mouse_down();
                app.mouse_up();
            };
            put(&mut app, &[(0, 96, 60), (96, 48, 64)]);
            let ppq = u32::from(app.state.project.ppq);
            key(&mut app, "f", false, true, false);
            assert!(app.state.shown(Win::PianoDlg) && app.state.piano_dlg.tool == Tool::Flam);
            let d = app.state.piano_dlg.flam.offset(ppq, app.state.tempo).unsigned_abs() as u32;
            assert!(d > 0);
            assert_eq!(mine(&app), vec![(0, d, 60), (d, 96, 60), (96, d, 64), (96 + d, 48, 64)], "leading flams, previewed");
            app.act(Action::PianoDlg(DAct::Options));
            let pop = app.state.playlist.popup.take().expect("Options opens the presets");
            assert_eq!(pop.items.iter().map(|i| i.label.as_ref()).collect::<Vec<_>>(), ["Flam", "Early flam", "Double stroke", "Early double stroke"]);
            crate::playlist::run(&mut app, Cmd::FlamPreset(Preset::DoubleStroke));
            let st = ppq / 4;
            assert_eq!(mine(&app), vec![(0, 96, 60), (st, 96, 60), (96, 48, 64), (96 + st, 48, 64)], "a copy a step later");
            // Alt+Y: the flam goes back, the window shows Flip.
            key(&mut app, "y", false, true, false);
            assert_eq!(app.state.piano_dlg.tool, Tool::Flip);
            assert_eq!(mine(&app), vec![(0, 48, 64), (96, 96, 60)], "mirrored in time, start spacing kept");
            frame(&mut app, &mut c, None);
            click(&mut app, Action::PianoDlg(DAct::Vertical));
            assert_eq!(mine(&app), vec![(0, 48, 60), (96, 96, 64)], "and in pitch");
            click(&mut app, Action::CloseWin(Win::PianoDlg));
            assert!(!app.state.shown(Win::PianoDlg));
            assert_eq!(mine(&app), vec![(0, 96, 60), (96, 48, 64)], "× puts the notes back");
            // Alt+X on the velocity lane: Multiply 5 notches down.
            app.state.piano.lane = piano::Lane::Velocity;
            key(&mut app, "x", false, true, false);
            assert_eq!(app.state.piano_dlg.tool, Tool::Levels);
            frame(&mut app, &mut c, None);
            let m = hit_rect(&app, &Action::Knob(KnobId::PianoDlg(DKnob::Mult)));
            app.mouse_move(m.cx(), m.cy());
            for _ in 0..5 {
                app.wheel(0.0, -1.0, false, false);
            }
            assert_eq!(app.hint.as_ref(), "Multiplier: 69%");
            let vels = |app: &App| piano::notes(&app.state).iter().filter(|n| n.channel == ch).map(|n| n.velocity).collect::<Vec<_>>();
            assert_eq!(vels(&app), vec![69, 69], "FL: 100 × 68.75 % = 69");
            click(&mut app, Action::PianoDlg(DAct::Accept));
            assert!(!app.state.shown(Win::PianoDlg));
            assert_eq!(vels(&app), vec![69, 69]);
            assert!(piano::undo(&mut app.state));
            assert_eq!(vels(&app), vec![100, 100], "one undo step");
            // A channel control lane: Scale levels does not open.
            app.state.piano.lane = piano::Lane::ChanVolume;
            key(&mut app, "x", false, true, false);
            assert!(!app.state.shown(Win::PianoDlg));
            assert!(app.state.message.as_deref().is_some_and(|m| m.contains("note property")));
            app.state.piano.lane = piano::Lane::Velocity;
        }
        frame(&mut app, &mut c, None);

        // Alt+K opens Limit (spec: ui/piano-limit.md): the range starts as the notes' own, the menus and the keyboard
        // preview at once, a collapsed section still applies, Accept is one undo step and the settings stay.
        {
            use crate::piano_dlg::{Act as DAct, LCmd, Tool};
            let keys = |app: &App| mine(app).iter().map(|n| n.2).collect::<Vec<_>>();
            let press = |app: &mut App, x: f32, y: f32| {
                app.mouse_move(x, y);
                app.mouse_down();
                app.mouse_up();
            };
            let click = |app: &mut App, a: Action| {
                let r = hit_rect(app, &a);
                press(app, r.cx(), r.cy());
            };
            put(&mut app, &[(0, 24, 61), (24, 24, 63), (48, 24, 66), (72, 24, 70)]);
            key(&mut app, "k", false, true, false);
            assert!(app.state.shown(Win::PianoDlg) && app.state.piano_dlg.tool == Tool::Limit);
            assert_eq!(app.state.piano_dlg.limit.range, Some((61, 70)), "the targets' span");
            assert_eq!(keys(&app), vec![61, 63, 66, 70], "nothing to change yet");
            frame(&mut app, &mut c, None);
            app.act(Action::PianoDlg(DAct::ScaleMenu));
            let pop = app.state.playlist.popup.take().expect("the scale box opens the chords file");
            assert_eq!(pop.items.iter().filter(|i| matches!(i.cmd, Cmd::Limit(LCmd::Scale(_)))).count(), 89, "(none) and 88 entries");
            crate::playlist::run(&mut app, Cmd::Limit(LCmd::Scale(crate::chords::FIRST_SCALE)));
            assert_eq!(keys(&app), vec![62, 64, 67, 61], "C major, ties up; B folds down an octave and stops at the range");
            // A press on A4 makes it the green key: +3.
            frame(&mut app, &mut c, None);
            let (win, kb) = (views::piano_dlg::rect(&app.state), hit_rect(&app, &Action::PianoDlg(DAct::Keys)));
            let at = |app: &App, k: i32| win.x + 8.0 + crate::piano_dlg::white_x(k) - app.state.piano_dlg.kb_scroll + 6.0;
            assert_eq!(views::piano_dlg::kb_key(&app.state, at(&app, 57), kb.bottom() - 4.0, false), 57);
            let x = at(&app, 57);
            press(&mut app, x, kb.bottom() - 4.0);
            assert_eq!(app.state.piano_dlg.limit.transpose, 3);
            assert_eq!(keys(&app), vec![65, 67, 70, 64]);
            // The name strip at C5: the low end is nearer and follows.
            let strip = hit_rect(&app, &Action::PianoDlg(DAct::Ruler));
            let x = at(&app, 60);
            press(&mut app, x, strip.cy());
            assert_eq!(app.state.piano_dlg.limit.range, Some((60, 70)));
            assert_eq!(keys(&app), vec![65, 67, 70, 63]);
            // The right button on C5: no transposition. The wheel scrolls a white key.
            app.mouse_move(at(&app, 60), kb.bottom() - 4.0);
            app.mouse_down_right();
            app.mouse_up();
            assert_eq!(app.state.piano_dlg.limit.transpose, 0);
            let s0 = app.state.piano_dlg.kb_scroll;
            app.wheel(0.0, -1.0, false, false);
            assert_eq!(app.state.piano_dlg.kb_scroll, s0 - 13.0);
            // Collapsed, the key / scale section still applies; the window is 67 px shorter.
            frame(&mut app, &mut c, None);
            click(&mut app, Action::PianoDlg(DAct::Collapse(0)));
            assert_eq!(views::piano_dlg::rect(&app.state).h, 228.0);
            assert_eq!(keys(&app), vec![62, 64, 67, 60]);
            frame(&mut app, &mut c, None);
            click(&mut app, Action::PianoDlg(DAct::Accept));
            assert!(!app.state.shown(Win::PianoDlg));
            assert_eq!(keys(&app), vec![62, 64, 67, 60]);
            assert!(piano::undo(&mut app.state));
            assert_eq!(keys(&app), vec![61, 63, 66, 70], "one undo step");
            key(&mut app, "k", false, true, false);
            let d = &app.state.piano_dlg;
            assert_eq!((d.limit.scale, d.limit.range, d.collapsed), (crate::chords::FIRST_SCALE, Some((60, 70)), [true, false]), "kept");
            assert_eq!(keys(&app), vec![62, 64, 67, 60]);
            app.act(Action::CloseWin(Win::PianoDlg));
            assert_eq!(keys(&app), vec![61, 63, 66, 70], "× puts the notes back");
            app.state.piano_dlg.collapsed = [false; 2];
        }
        frame(&mut app, &mut c, None);

        // Alt+W opens Claw machine (spec: ui/piano-claw.md): the selected notes alone, periods still counted from tick 0,
        // preview at once, the menus and the knob too; Accept is one undo step, the selection holds the result, and the
        // settings stay.
        {
            use crate::piano_dlg::{Act as DAct, CCmd, Knob as DKnob, Tool};
            let keys = |app: &App| mine(app).iter().map(|n| n.2).collect::<Vec<_>>();
            let click = |app: &mut App, a: Action| {
                let r = hit_rect(app, &a);
                app.mouse_move(r.cx(), r.cy());
                app.mouse_down();
                app.mouse_up();
            };
            let st = u32::from(app.state.project.ppq) / 4;
            let steps: Vec<(u32, u32, u16)> = (0..8).map(|i| (i * st, st, 60 + i as u16)).collect();
            put(&mut app, &steps);
            let first4: Vec<usize> = piano::notes(&app.state)
                .iter()
                .enumerate()
                .filter(|(_, n)| n.channel == ch && n.position < 4 * st)
                .map(|(i, _)| i)
                .collect();
            app.state.piano.select(app.state.pattern, ch, first4);
            key(&mut app, "w", false, true, false);
            assert!(app.state.shown(Win::PianoDlg) && app.state.piano_dlg.tool == Tool::Claw);
            assert_eq!(keys(&app), vec![60, 61, 62, 64, 65, 66, 67], "the first beat's last quarter trashed, the selection alone");
            assert!(mine(&app).iter().any(|n| *n == (4 * st, st, 64)), "the notes after it stay");
            frame(&mut app, &mut c, None);
            app.act(Action::PianoDlg(DAct::TrashMenu));
            let pop = app.state.playlist.popup.take().expect("Trash every opens its menu");
            assert_eq!(pop.items.iter().filter(|i| matches!(i.cmd, Cmd::Claw(CCmd::Trash(_)))).count(), 10);
            crate::playlist::run(&mut app, Cmd::Claw(CCmd::Trash(9)));
            assert_eq!(mine(&app), steps, "(none), Time dist at the centre: unchanged");
            frame(&mut app, &mut c, None);
            let k = hit_rect(&app, &Action::Knob(KnobId::PianoDlg(DKnob::ClawTime)));
            app.mouse_move(k.cx(), k.cy());
            for _ in 0..8 {
                app.wheel(0.0, 1.0, false, false);
            }
            assert_eq!(app.hint.as_ref(), "Time distribution: +25%");
            assert!(mine(&app)[0].1 > st && mine(&app)[3].1 < st, "early notes longer, late ones shorter");
            frame(&mut app, &mut c, None);
            click(&mut app, Action::PianoDlg(DAct::Accept));
            assert!(!app.state.shown(Win::PianoDlg));
            assert_eq!(app.state.piano.selection(app.state.pattern, Some(ch)).len(), 4, "the selection holds the result");
            assert!(piano::undo(&mut app.state));
            assert_eq!(mine(&app), steps, "one undo step");
            key(&mut app, "w", false, true, false);
            assert_eq!(app.state.piano_dlg.claw.trash, 9, "kept");
            assert_ne!(mine(&app), steps);
            app.act(Action::CloseWin(Win::PianoDlg));
            assert_eq!(mine(&app), steps, "× puts the notes back");
        }
        frame(&mut app, &mut c, None);

        // Alt+O opens LFO (spec: ui/piano-lfo.md): a sine on the velocities at once, "52%" after a notch of Value, End
        // copies Start, the Shape menu; Accept is one undo step. On a channel lane it writes the lane's control points
        // instead, and × takes them back.
        {
            use crate::lfo::Shape;
            use crate::piano_dlg::{Act as DAct, Knob as DKnob, Tool};
            use os_model::ControlPoint;
            let click = |app: &mut App, a: Action| {
                let r = hit_rect(app, &a);
                app.mouse_move(r.cx(), r.cy());
                app.mouse_down();
                app.mouse_up();
            };
            let vels = |app: &App| piano::notes(&app.state).iter().filter(|n| n.channel == ch).map(|n| n.velocity).collect::<Vec<_>>();
            let vol = |app: &App| piano::controls(&app.state).iter().filter(|c| c.channel() == ch && c.param() == ControlPoint::VOLUME).map(|c| (c.position, c.value)).collect::<Vec<_>>();
            let st = u32::from(app.state.project.ppq) / 4;
            put(&mut app, &(0..16).map(|i| (i * st, st, 60)).collect::<Vec<_>>());
            app.state.piano.lane = piano::Lane::Velocity;
            let before = vels(&app);
            key(&mut app, "o", false, true, false);
            assert!(app.state.shown(Win::PianoDlg) && app.state.piano_dlg.tool == Tool::Lfo);
            assert_eq!(vels(&app)[..9], [64, 76, 87, 94, 96, 94, 87, 76, 64], "Reset's sine, a bar long");
            frame(&mut app, &mut c, None);
            let k = hit_rect(&app, &Action::Knob(KnobId::PianoDlg(DKnob::LfoValue)));
            app.mouse_move(k.cx(), k.cy());
            app.wheel(0.0, 1.0, false, false);
            assert_eq!(app.hint.as_ref(), "52%");
            click(&mut app, Action::PianoDlg(DAct::LfoEnd));
            assert!(app.state.piano_dlg.lfo.end_on && app.state.piano_dlg.lfo.end == app.state.piano_dlg.lfo.start, "End takes Start's values");
            frame(&mut app, &mut c, None);
            app.act(Action::PianoDlg(DAct::ShapeMenu));
            let pop = app.state.playlist.popup.take().expect("the shape box opens its menu");
            assert_eq!(pop.items.iter().filter(|i| matches!(i.cmd, Cmd::LfoShape(_))).count(), 3);
            crate::playlist::run(&mut app, Cmd::LfoShape(Shape::Square));
            assert_eq!(vels(&app)[..9], [98, 98, 98, 98, 98, 98, 98, 98, 34]);
            frame(&mut app, &mut c, None);
            click(&mut app, Action::PianoDlg(DAct::Accept));
            assert!(!app.state.shown(Win::PianoDlg));
            assert!(piano::undo(&mut app.state));
            assert_eq!(vels(&app), before, "one undo step");

            app.state.piano.lane = piano::Lane::ChanVolume;
            key(&mut app, "o", false, true, false);
            assert_eq!(app.state.piano_dlg.tool.title_on(app.state.piano.lane), "Events - LFO");
            assert_eq!(vels(&app), before, "the notes stay");
            let pts = vol(&app);
            assert_eq!(pts.first(), Some(&(0, 9800)), "the settings kept: the square from 76.5 %");
            assert!(pts.iter().all(|p| p.1 == 9800 || p.1 == 3400) && pts.windows(2).all(|w| w[0].1 != w[1].1), "its edges alone");
            frame(&mut app, &mut c, None);
            click(&mut app, Action::PianoDlg(DAct::Accept));
            assert_eq!(vol(&app), pts);
            assert!(piano::undo(&mut app.state));
            assert!(vol(&app).is_empty(), "one undo step");
            key(&mut app, "o", false, true, false);
            assert!(!vol(&app).is_empty());
            app.act(Action::CloseWin(Win::PianoDlg));
            assert!(vol(&app).is_empty(), "× takes the points back");
            app.state.piano.lane = piano::Lane::Velocity;
        }
        frame(&mut app, &mut c, None);

        // Alt+Q opens Quantize (spec: ui/piano-quantize.md): Reset's settings on the Default template at once, "53%"
        // after a notch of Start time, the duration and template menus; Accept is one undo step, the settings and the
        // template are kept for the next opening and × puts the notes back.
        {
            use crate::piano_dlg::{Act as DAct, Knob as DKnob, Tool};
            use crate::quantize::{Mode, QCmd};
            let click = |app: &mut App, a: Action| {
                let r = hit_rect(app, &a);
                app.mouse_move(r.cx(), r.cy());
                app.mouse_down();
                app.mouse_up();
            };
            let mine = |app: &App| piano::notes(&app.state).iter().filter(|n| n.channel == ch).map(|n| (n.position, n.length)).collect::<Vec<_>>();
            assert_eq!(app.state.project.ppq, 96);
            put(&mut app, &[(5, 20, 60), (30, 30, 62), (50, 10, 64), (70, 40, 65)]);
            let before = mine(&app);
            key(&mut app, "q", false, true, false);
            assert!(app.state.shown(Win::PianoDlg) && app.state.piano_dlg.tool == Tool::Quantize);
            assert_eq!(mine(&app), [(3, 22), (27, 27), (49, 5), (71, 44)], "Reset's settings, as FL's");
            frame(&mut app, &mut c, None);
            let k = hit_rect(&app, &Action::Knob(KnobId::PianoDlg(DKnob::QStart)));
            app.mouse_move(k.cx(), k.cy());
            app.wheel(0.0, 1.0, false, false);
            assert_eq!(app.hint.as_ref(), "53%");
            frame(&mut app, &mut c, None);
            app.act(Action::PianoDlg(DAct::DurMenu));
            let pop = app.state.playlist.popup.take().expect("the duration box opens its menu");
            assert_eq!(pop.items.iter().filter(|i| matches!(i.cmd, Cmd::Quant(QCmd::Mode(_)))).count(), 4);
            crate::playlist::run(&mut app, Cmd::Quant(QCmd::Mode(Mode::LeaveDuration)));
            assert_eq!(mine(&app).iter().map(|n| n.1).collect::<Vec<_>>(), [20, 30, 10, 40], "Leave duration keeps the lengths");
            app.act(Action::PianoDlg(DAct::TemplateMenu));
            let pop = app.state.playlist.popup.take().expect("the ▸ opens the template menu");
            assert!(pop.items.iter().any(|i| i.label == "Default" && i.mark == crate::playlist::Mark::Radio(true)), "Default ticked");
            assert!(matches!(pop.items.last().map(|i| &i.cmd), Some(Cmd::Quant(QCmd::Snap(7)))), "the Snap grids last");
            crate::playlist::run(&mut app, Cmd::Quant(QCmd::Snap(1)));
            assert_eq!(app.state.piano_dlg.quant_tpl.0.name, "Beat");
            frame(&mut app, &mut c, None);
            click(&mut app, Action::PianoDlg(DAct::Accept));
            assert!(!app.state.shown(Win::PianoDlg));
            assert!(piano::undo(&mut app.state));
            assert_eq!(mine(&app), before, "one undo step");
            key(&mut app, "q", false, true, false);
            assert_eq!((app.state.piano_dlg.quant.mode, app.state.piano_dlg.quant_tpl.0.name.as_str()), (Mode::LeaveDuration, "Beat"), "kept");
            assert_ne!(mine(&app), before);
            app.act(Action::CloseWin(Win::PianoDlg));
            assert_eq!(mine(&app), before, "× puts the notes back");
        }
        frame(&mut app, &mut c, None);

        // Alt+U opens Chop (spec: ui/piano-chop.md): Reset's settings cut the notes into steps at once, a notch of Time
        // mul doubles them, the ▸ menu ticks Default; Accept is one undo step, the settings are kept for the next
        // opening and × puts the notes back.
        {
            use crate::piano_dlg::{Act as DAct, Knob as DKnob, Tool};
            let mine = |app: &App| piano::notes(&app.state).iter().filter(|n| n.channel == ch).map(|n| (n.position, n.length, n.unk14)).collect::<Vec<_>>();
            put(&mut app, &[(0, 96, 60), (96, 48, 62)]);
            let before = mine(&app);
            key(&mut app, "u", false, true, false);
            assert!(app.state.shown(Win::PianoDlg) && app.state.piano_dlg.tool == Tool::Chop);
            assert_eq!(mine(&app), [(0, 24, 1), (24, 24, 1), (48, 24, 1), (72, 24, 1), (96, 24, 2), (120, 24, 2)], "Default, grouped by note");
            frame(&mut app, &mut c, None);
            let k = hit_rect(&app, &Action::Knob(KnobId::PianoDlg(DKnob::ChopTimeMul)));
            app.mouse_move(k.cx(), k.cy());
            app.wheel(0.0, 1.0, false, false);
            assert_eq!(app.hint.as_ref(), "Time multiplicator: x2");
            assert_eq!(mine(&app), [(0, 48, 1), (48, 48, 1), (96, 48, 2)]);
            frame(&mut app, &mut c, None);
            app.act(Action::PianoDlg(DAct::TemplateMenu));
            let pop = app.state.playlist.popup.take().expect("the ▸ opens the pattern menu");
            assert!(pop.items.iter().any(|i| i.label == "Default" && i.mark == crate::playlist::Mark::Radio(true)), "Default ticked");
            assert!(pop.items.iter().all(|i| i.sep || i.head || i.col || matches!(i.cmd, Cmd::ChopPattern(_)) || !i.children.is_empty()));
            crate::playlist::run(&mut app, Cmd::ChopPattern(None));
            app.act(Action::PianoDlg(DAct::Group));
            assert_eq!(mine(&app), [(0, 48, 0), (48, 48, 0), (96, 48, 0)], "Group notes off");
            frame(&mut app, &mut c, None);
            app.act(Action::PianoDlg(DAct::Accept));
            assert!(!app.state.shown(Win::PianoDlg));
            assert!(piano::undo(&mut app.state));
            assert_eq!(mine(&app), before, "one undo step");
            key(&mut app, "u", false, true, false);
            assert_eq!((app.state.piano_dlg.chop.time_mul, app.state.piano_dlg.chop.group), (4, false), "kept");
            assert_ne!(mine(&app), before);
            app.act(Action::CloseWin(Win::PianoDlg));
            assert_eq!(mine(&app), before, "× puts the notes back");
        }
        frame(&mut app, &mut c, None);

        // The toolbar's tools button opens the Tools menu.
        let r = hit_rect(&app, &Action::PianoToolsMenu);
        app.mouse_move(r.cx(), r.cy());
        app.mouse_down();
        app.mouse_up();
        assert!(app.state.playlist.popup.take().is_some_and(|p| p.items.iter().any(|i| i.cmd == Cmd::Piano(NCmd::Glue))));

        // Alt+V hides and shows the ghost channels.
        assert!(app.state.piano.ghosts);
        key(&mut app, "v", false, true, false);
        assert!(!app.state.piano.ghosts);
        frame(&mut app, &mut c, None);
        key(&mut app, "v", false, true, false);
        assert!(app.state.piano.ghosts);

        // Alt+T adds a time marker at the play-start marker (stopped), snapped to a step.
        app.state.piano.marker = 200;
        key(&mut app, "t", false, true, false);
        assert_eq!(piano::pattern_mut(&mut app.state).markers, vec![os_model::TimeMarker::new(192, "Marker 1")]);
        // It is saved with the pattern and comes back on open.
        let path = std::env::temp_dir().join(format!("os_piano_marker_{}.flp", std::process::id()));
        app.save_to(path.clone());
        assert!(app.state.message.as_deref().is_some_and(|m| m.starts_with("Saved")), "{:?}", app.state.message);
        app.open(path.clone());
        let _ = std::fs::remove_file(&path);
        app.state.z = vec![Win::PianoRoll];
        app.state.selected_channel = 0;
        assert_eq!(piano::pattern_mut(&mut app.state).markers, vec![os_model::TimeMarker::new(192, "Marker 1")]);
        frame(&mut app, &mut c, None);

        // Ctrl+K cycles to the next channel, wrapping; with the typing keyboard off G / K step too.
        let n = app.state.project.channels.len();
        key(&mut app, "k", true, false, false);
        assert_eq!(app.state.selected_channel, 1);
        if app.typing_on() {
            app.toggle_typing();
        }
        key(&mut app, "g", false, false, false);
        key(&mut app, "g", false, false, false);
        assert_eq!(app.state.selected_channel, n - 1);
        // H / J skip channels without notes in the pattern.
        key(&mut app, "j", false, false, false);
        assert!(piano::notes(&app.state).iter().any(|nt| Some(nt.channel) == piano::channel(&app.state)));
        app.state.selected_channel = 0;
        frame(&mut app, &mut c, None);

        // The wheel over the keyboard scrolls like the grid and Ctrl+wheel there zooms time; Ctrl+middle-drag zooms
        // the keys around the press (up = taller); Alt+wheel over the ruler and PgUp / PgDn zoom time.
        let keys = hit_rect(&app, &Action::PianoKeys);
        app.mouse_move(keys.cx(), keys.cy());
        let sy = app.state.piano.scroll_y;
        app.wheel(0.0, 1.0, false, false);
        app.ease_to_end();
        assert!(app.state.piano.scroll_y < sy, "wheel up over the keys scrolls up");
        assert_eq!(app.state.piano.row_h, views::piano::ROW);
        let z = app.state.piano.px_per_beat;
        app.wheel(0.0, 1.0, false, true);
        assert!(app.state.piano.px_per_beat > z, "Ctrl+wheel over the keys zooms time");
        assert_eq!(app.state.piano.row_h, views::piano::ROW);
        frame(&mut app, &mut c, None);
        for area in [Action::PianoKeys, Action::PianoGrid] {
            let r = hit_rect(&app, &area);
            let (x, y) = (r.cx(), r.cy());
            app.state.piano.row_h = views::piano::ROW;
            app.mouse_move(x, y);
            let k0 = app.piano_geom().unwrap().key_at(y);
            app.ctrl = true;
            app.mouse_down_middle();
            app.ctrl = false;
            app.mouse_move(x, y - 60.0);
            assert!(app.state.piano.row_h > views::piano::ROW, "{area:?}: dragging up makes rows taller");
            assert_eq!(app.piano_geom().unwrap().key_at(y), k0, "{area:?}: the key at the press stays");
            app.mouse_move(x, y + 200.0);
            assert_eq!(app.state.piano.row_h, piano::MIN_ROW, "{area:?}: dragging down shrinks to the minimum");
            app.mouse_up();
            frame(&mut app, &mut c, None);
        }
        // Without Ctrl the middle button still pans.
        app.state.piano.row_h = views::piano::ROW;
        let r = hit_rect(&app, &Action::PianoGrid);
        app.mouse_move(r.cx(), r.cy());
        app.mouse_down_middle();
        app.mouse_move(r.cx(), r.cy() - 40.0);
        app.mouse_up();
        assert_eq!(app.state.piano.row_h, views::piano::ROW);
        frame(&mut app, &mut c, None);
        let z = app.state.piano.px_per_beat;
        let ruler = hit_rect(&app, &Action::PianoRuler);
        app.mouse_move(ruler.cx(), ruler.cy());
        app.alt = true;
        app.wheel(0.0, 1.0, false, false);
        app.alt = false;
        assert!(app.state.piano.px_per_beat > z);
        let z = app.state.piano.px_per_beat;
        app.key(&Key::Named(NamedKey::PageDown), false, false);
        assert!(app.state.piano.px_per_beat < z);
        frame(&mut app, &mut c, None);
    }

    /// Piano roll edits repaint only damaged regions; the result must match a full repaint.
    #[test]
    fn piano_damage_matches_full() {
        let Ok(mut a) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut b = Canvas::new(1920, 1080, 1.0).unwrap();
        let mut app = App::new(None);
        app.state.z = vec![Win::PianoRoll, Win::ChannelRack];
        app.state.wins[2].maxed = true;
        app.state.selected_channel = 4;
        app.state.rack_pos = (900.0, 300.0);
        frame(&mut app, &mut a, None);
        let _ = app.take_damage();
        fn step(app: &mut App, c: &mut Canvas, f: impl FnOnce(&mut App)) {
            f(app);
            let d = app.take_damage();
            frame_damage(app, c, d);
        }
        // Add a note (click), drag it up and right, release; resize it; draw a velocity; erase it.
        step(&mut app, &mut a, |ap| { ap.mouse_move(500.0, 500.0); });
        step(&mut app, &mut a, |ap| ap.mouse_down());
        let key0 = piano::notes(&app.state)[0].key;
        for (x, y) in [(530.0, 490.0), (600.0, 450.0), (650.0, 430.0)] {
            step(&mut app, &mut a, move |ap| { ap.mouse_move(x, y); });
        }
        // The dragged note's key is lit on the keyboard while held.
        assert_eq!(app.state.piano.held, Some(i32::from(key0) + 4));
        // It sounds at that key until the button is released, not for a fixed preview length.
        assert_eq!(app.mouse_note.map(|n| n.1), Some(key0 + 4));
        // (The overview mini-map refreshes on release, so compare the keyboard column only.)
        frame(&mut app, &mut b, None);
        let g = app.piano_geom().unwrap();
        let keys = Rect::new(g.grid.x - 73.0, g.grid.y, 73.0, g.grid.h);
        let differs = |x: usize, y: usize| {
            let i = (y * 1920 + x) * 4;
            (0..4).any(|c| a.pixmap.data()[i + c].abs_diff(b.pixmap.data()[i + c]) > 3)
        };
        let bad = (keys.y as usize..keys.bottom() as usize).flat_map(|y| (keys.x as usize..keys.right() as usize).map(move |x| (x, y))).filter(|&(x, y)| differs(x, y)).count();
        assert_eq!(bad, 0, "keyboard while dragging a note (lit key)");
        step(&mut app, &mut a, |ap| ap.mouse_up());
        assert_eq!(app.state.piano.held, None);
        assert_eq!(app.mouse_note, None, "the note stops with the button");
        let n = piano::notes(&app.state)[0];
        assert!(n.position > 0 && n.key == key0 + 4, "note moved right and 4 keys up: {n:?}");
        let r = app.piano_geom().unwrap().note_rect(&n).unwrap();
        step(&mut app, &mut a, |ap| { ap.mouse_move(r.right() - 1.0, r.cy()); });
        step(&mut app, &mut a, |ap| ap.mouse_down());
        step(&mut app, &mut a, |ap| { ap.mouse_move(r.right() + 60.0, r.cy()); });
        step(&mut app, &mut a, |ap| ap.mouse_up());
        assert!(piano::notes(&app.state)[0].length > n.length, "note resized");
        let panel = app.piano_panel.unwrap();
        step(&mut app, &mut a, |ap| { ap.mouse_move(r.x + 1.0, panel.y + 20.0); });
        step(&mut app, &mut a, |ap| ap.mouse_down());
        step(&mut app, &mut a, |ap| { ap.mouse_move(r.x + 1.0, panel.bottom() - 40.0); });
        step(&mut app, &mut a, |ap| ap.mouse_up());
        frame(&mut app, &mut b, None);
        assert_same(&a, &b, "after move/resize/velocity");
        step(&mut app, &mut a, |ap| { ap.mouse_move(r.x + 4.0, r.cy()); });
        step(&mut app, &mut a, |ap| ap.mouse_down_right());
        step(&mut app, &mut a, |ap| ap.mouse_up());
        assert!(piano::notes(&app.state).is_empty(), "right-click erased the note");
        frame(&mut app, &mut b, None);
        assert_same(&a, &b, "after erase");
        // Hold the left button on the keyboard and slide down: each key entered is held (lit).
        let g = app.piano_geom().unwrap();
        let kx = g.grid.x - 30.0;
        step(&mut app, &mut a, |ap| { ap.mouse_move(kx, g.y_of(72) + 8.0); });
        step(&mut app, &mut a, |ap| ap.mouse_down());
        assert_eq!(app.state.piano.held, Some(72));
        assert_eq!(app.mouse_note.map(|n| n.1), Some(72));
        for key in [71, 70, 69] {
            step(&mut app, &mut a, |ap| { ap.mouse_move(kx, g.y_of(key) + 8.0); });
            assert_eq!(app.state.piano.held, Some(key));
            assert_eq!(app.mouse_note.map(|n| i32::from(n.1)), Some(key), "only the key under the mouse sounds");
        }
        frame(&mut app, &mut b, None);
        assert_same(&a, &b, "keyboard drag");
        step(&mut app, &mut a, |ap| ap.mouse_up());
        assert_eq!(app.state.piano.held, None);
        assert_eq!(app.mouse_note, None, "the key stops with the button");
        frame(&mut app, &mut b, None);
        assert_same(&a, &b, "keyboard released");
    }

    fn ruler_app(c: &mut Canvas) -> (App, Rect, crate::playlist::PGeom, u32) {
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.playlist.reset();
        app.state.z = vec![Win::Playlist];
        frame(&mut app, c, None);
        let g = crate::playlist::PGeom::new(&app.state, app.pl_grid().expect("playlist grid"));
        let ruler = hit_rect(&app, &Action::PlRuler);
        let bar = 4 * u32::from(app.state.project.ppq);
        (app, ruler, g, bar)
    }

    fn ruler_press(app: &mut App, c: &mut Canvas, x0: f32, x1: f32, y: f32, right: bool) {
        app.mouse_move(x0, y);
        if right { app.mouse_down_right() } else { app.mouse_down() }
        app.mouse_move(x1, y);
        app.mouse_up();
        frame(app, c, None);
    }

    /// Moving the song position in the playlist ruler switches to SONG mode without starting playback; a time
    /// selection (right-drag) keeps PAT (spec: ui/playlist.md §7, as FL does).
    #[test]
    fn ruler_click_switches_to_song_mode() {
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let (mut app, ruler, g, bar) = ruler_app(&mut c);
        assert!(!app.state.song_mode);
        ruler_press(&mut app, &mut c, g.x_of(bar as f32) + 1.0, g.x_of(3.0 * bar as f32), ruler.cy(), true);
        assert!(app.state.playlist.range.is_some(), "a time selection");
        assert!(!app.state.song_mode, "a time selection keeps PAT");
        // The project's initial values follow the move (spec: ui/settings-choices.md "Advanced: Initialize controls
        // automatically").
        app.state.project.init_controls = vec![os_model::InitControl { head: 0, target: os_model::target::TEMPO, value: 140_000 }];
        app.state.tempo = 90.0;
        ruler_press(&mut app, &mut c, g.x_of(4.0 * bar as f32) + 1.0, g.x_of(4.0 * bar as f32) + 1.0, ruler.cy(), false);
        assert!(app.state.song_mode, "a click on the ruler switches to SONG");
        assert_eq!(app.state.playlist.pos, 4 * bar);
        assert!(!app.transport.playing(), "and does not start playback");
        assert_eq!(app.state.tempo, 140.0, "the initial tempo");
    }

    /// Stretch mode (spec: ui/playlist.md §9b): Shift+M on, then dragging an audio clip's end to twice its length
    /// doubles its channel's resample multiplier and keeps the part of the sample it shows; undo restores both; the
    /// multiplier is saved with a new audio clip channel.
    #[test]
    fn playlist_stretch_mode() {
        use crate::playlist as pl;
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let (mut app, _, _, _) = ruler_app(&mut c);
        app.state.playlist_bar_w = 200.0;
        app.state.playlist.snap = crate::snap::EditorSnap::Grid(pl::Snap::None);
        let wav = std::env::temp_dir().join(format!("os-app-stretch-{}.wav", std::process::id()));
        os_engine::wav::write_f32_stereo(&mut std::fs::File::create(&wav).unwrap(), 48000, &[0.5; 96000]).unwrap();
        pl::add_audio_clip(&mut app.state, &wav.to_string_lossy(), 0, 0).expect("audio clip added");
        let ci = pl::clips(&app.state).len() - 1;
        let row = app.state.project.channels.len() - 1;
        let before = pl::clips(&app.state)[ci];
        pl::run(&mut app, pl::Cmd::StretchMode);
        assert!(app.state.playlist.stretch);
        frame(&mut app, &mut c, None);
        let g = pl::PGeom::new(&app.state, app.pl_grid().unwrap());
        let r = g.clip_rect(&before);
        let y = r.y + r.h - 6.0;
        app.mouse_move(r.right() - 1.0, y);
        app.mouse_down();
        app.mouse_move(r.right() - 1.0 + r.w, y);
        app.mouse_up();
        let after = pl::clips(&app.state)[ci];
        // The time becomes the whole sample's length (1 s at the project tempo) times two; the multiplier stays at 1.
        let sound = app.state.project.channels[row].sound;
        let want = app.state.tempo / 60.0 * 2.0;
        let beats = sound.stretch_time().expect("a stretch time");
        assert!((beats - want).abs() < 0.02 * want, "time {beats} beats, want {want}");
        assert_eq!((sound.stretch[2], sound.stretch_mode()), (0, 0), "multiplier 1, method kept");
        assert!((f64::from(after.length) / f64::from(before.length) - 2.0).abs() < 0.02, "twice as long");
        assert_eq!(after.window, before.window, "the same part of the sample");
        app.key(&winit::keyboard::Key::Character("z".into()), true, false);
        assert_eq!(app.state.project.channels[row].sound.stretch, [0; 4], "undo restores the channel");
        // Saved with a new project's audio clip channel.
        app.key(&winit::keyboard::Key::Character("y".into()), true, false);
        app.state.project.channels[row].sound.set_stretch_multiplier(2.0);
        let path = std::env::temp_dir().join(format!("os-app-stretch-{}.flp", std::process::id()));
        app.save_to(path.clone());
        let back = os_flp::project::load(&os_flp::FlpFile::parse(&std::fs::read(&path).unwrap()).unwrap()).unwrap();
        let ch = back.channels.iter().find(|c| c.kind == ChannelKind::AudioClip).expect("the audio clip channel");
        assert_eq!(ch.sound.stretch, [0, 0, 10000, 0]);
        // And changed in that existing file (its event 215 patched).
        let mut app2 = App::new(Some(path.clone()));
        let row2 = app2.state.project.channels.iter().position(|c| c.kind == ChannelKind::AudioClip).unwrap();
        app2.state.project.channels[row2].sound.set_stretch_multiplier(0.5);
        app2.save_to(path.clone());
        let back = os_flp::project::load(&os_flp::FlpFile::parse(&std::fs::read(&path).unwrap()).unwrap()).unwrap();
        assert_eq!(back.channels[row2].sound.stretch, [0, 0, (-10000i32) as u32, 0]);
        let _ = (std::fs::remove_file(&path), std::fs::remove_file(&wav));
    }

    // spec: ui/settings-choices.md "Display: Colorful waveforms (2026-10-16)"
    /// Colorful waveforms colour a playlist audio clip's waveform at once (a 100 Hz sine: Multiband's orange-red), in
    /// the same shape; back to Off, the clip is drawn exactly as before.
    #[test]
    fn playlist_colorful_waveforms() {
        use crate::playlist as pl;
        use crate::prefs::{set_choice, COLORFUL_WAVEFORMS};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let (mut app, _, _, _) = ruler_app(&mut c);
        app.state.playlist_bar_w = 200.0;
        let wav = std::env::temp_dir().join(format!("os-app-colorful-{}.wav", std::process::id()));
        let sine: Vec<f32> = (0..48000)
            .flat_map(|i| {
                let v = (2.0 * std::f32::consts::PI * 100.0 * i as f32 / 48000.0).sin() * 0.3;
                [v, v]
            })
            .collect();
        os_engine::wav::write_f32_stereo(&mut std::fs::File::create(&wav).unwrap(), 48000, &sine).unwrap();
        pl::add_audio_clip(&mut app.state, &wav.to_string_lossy(), 0, 0).expect("audio clip added");
        let clip = *pl::clips(&app.state).last().unwrap();
        let (w, h) = (1920, 1080);
        let shot = |app: &mut App, c: &mut Canvas| {
            frame(app, c, None);
            let g = pl::PGeom::new(&app.state, app.pl_grid().unwrap());
            let body = g.body(&clip);
            let mut px = vec![0u32; w * h];
            c.copy_to_xrgb(&mut px);
            let (x0, x1) = (body.x.max(0.0) as usize, (body.right() as usize).min(w));
            let (y0, y1) = (body.y.max(0.0) as usize, (body.bottom() as usize).min(h));
            (y0..y1).flat_map(|y| px[y * w + x0..y * w + x1].to_vec()).collect::<Vec<u32>>()
        };
        let orange = |px: &[u32]| {
            px.iter()
                .filter(|&&p| {
                    let (r, g, b) = ((p >> 16) & 0xff, (p >> 8) & 0xff, p & 0xff);
                    r > g + 60 && r > b + 100
                })
                .count()
        };
        let off = shot(&mut app, &mut c);
        assert!(!off.is_empty());
        set_choice(&COLORFUL_WAVEFORMS, 2);
        let multiband = shot(&mut app, &mut c);
        assert_eq!(multiband.len(), off.len());
        assert!(orange(&multiband) > orange(&off) + 200, "orange-red: {} before, {} after", orange(&off), orange(&multiband));
        // The same shape: only the waveform's own pixels (a band through the middle, at this level) change colour.
        let changed = off.iter().zip(&multiband).filter(|(a, b)| a != b).count();
        assert!(changed > 200 && changed < off.len() / 2, "{changed} of {} pixels", off.len());
        set_choice(&COLORFUL_WAVEFORMS, 1);
        let map = shot(&mut app, &mut c);
        assert_ne!(map, multiband, "Color map has colours of its own");
        set_choice(&COLORFUL_WAVEFORMS, 0);
        assert!(shot(&mut app, &mut c) == off, "Off: as before");
        let _ = std::fs::remove_file(&wav);
    }

    /// Audio clips follow their channel (spec: ui/playlist.md §9c): the length is worked out from the trims and the
    /// channel's stretch, pitch and the tempo, each trim point rounded on its own. The numbers are FL 26's for a 6.4 s
    /// sample at 130 BPM, PPQ 96: untrimmed, trimmed to 0..3200 ms and to 1600..4800 ms.
    #[test]
    fn audio_clips_follow_their_channel() {
        use crate::playlist as pl;
        use os_model::{Clip, ClipSource};
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.project.ppq = 96;
        app.state.tempo = 130.0;
        app.state.playlist.reset();
        let wav = std::env::temp_dir().join(format!("os-app-fit-{}.wav", std::process::id()));
        os_engine::wav::write_f32_stereo(&mut std::fs::File::create(&wav).unwrap(), 44100, &vec![0.25; 2 * 282240]).unwrap();
        pl::add_audio_clip(&mut app.state, &wav.to_string_lossy(), 0, 0).expect("audio clip added");
        let row = app.state.project.channels.len() - 1;
        let src = ClipSource::Channel(app.state.project.channels[row].index);
        for (track, w) in [(1, (0.0, 3200.0)), (2, (1600.0, 4800.0))] {
            pl::arr_mut(&mut app.state).clips.push(Clip { window: Some(w), ..Clip::new(0, 1, track, src) });
        }
        let mut check = |app: &mut App, stretch: [u32; 4], pitch: i32, tempo: f64, want: [u32; 3], what: &str| {
            app.state.project.channels[row].sound.stretch = stretch;
            app.state.project.channels[row].pitch_cents = pitch;
            app.state.tempo = tempo;
            pl::fit_audio(&mut app.state);
            let got: Vec<u32> = pl::clips(&app.state).iter().filter(|c| c.source == src).map(|c| c.length).collect();
            assert_eq!(got, want, "{what}");
            // An untrimmed clip shows the whole sample.
            if let Some(whole) = pl::clips(&app.state).iter().find(|c| c.source == src && c.window.is_none()).copied() {
                assert!((pl::window(&app.state, &whole).1 - 6400.0).abs() < 6.0, "{what}: {:?}", pl::window(&app.state, &whole));
            }
        };
        let t = |units: f32| units.to_bits();
        check(&mut app, [0; 4], 0, 130.0, [1331, 666, 665], "plain");
        check(&mut app, [0, 0, 2540, 0], 0, 130.0, [1587, 794, 794], "Resample, MUL 119.25 %");
        check(&mut app, [0, 0, 2540, 0], 10, 130.0, [1578, 789, 789], "and the channel pitch +10 cents");
        check(&mut app, [0, 100, 2540, 5], 0, 130.0, [1587, 794, 794], "Auto, PITCH +100: the length stays");
        check(&mut app, [t(2662.0), 0, 2540, (-1i32) as u32], 10, 130.0, [1587, 794, 793], "Stretch: TIME, no channel pitch");
        check(&mut app, [t(488.0), 0, 0, 0], 0, 130.0, [244, 122, 122], "Resample, TIME 0:10:04");
        check(&mut app, [t(2662.0), 0, 0, (-1i32) as u32], 0, 130.0, [1331, 666, 665], "Stretch, TIME 2662: a point on half a tick rounds up");
        check(&mut app, [0; 4], 0, 522.0, [5345, 2673, 2673], "the tempo without a TIME");
        check(&mut app, [t(2662.0), 0, 0, (-1i32) as u32], 0, 522.0, [1331, 666, 665], "the tempo with a TIME");
        // A clip resized in the playlist keeps its new end (it gets a trim end), refitted alike.
        let i = pl::clips(&app.state).iter().position(|c| c.source == src && c.window.is_none()).unwrap();
        check(&mut app, [0; 4], 0, 130.0, [1331, 666, 665], "plain again");
        let orig = pl::clips(&app.state)[i];
        pl::resize_right(&mut app.state, i, &orig, 1000);
        assert!(!pl::fit_audio(&mut app.state), "a resize already fits");
        check(&mut app, [0, 0, 10000, 0], 0, 130.0, [2000, 1331, 1331], "x2 after the resize");
        // The tick fits them. A fit alone is not an unsaved change (a project opens with other lengths); an edit is.
        for c in &mut pl::arr_mut(&mut app.state).clips {
            c.length = 100;
        }
        app.mark_saved();
        app.tick();
        let got: Vec<u32> = pl::clips(&app.state).iter().filter(|c| c.source == src).map(|c| c.length).collect();
        assert_eq!(got, [2000, 1331, 1331], "fitted by the tick");
        assert!(!app.unsaved(), "a fit is not an edit");
        app.state.project.channels[row].sound.stretch = [0; 4];
        app.tick();
        assert!(app.unsaved(), "a stretch edit is");
        let _ = std::fs::remove_file(&wav);
    }

    /// The tempo counts the bar's beats (spec: audio/timing.md "The tempo counts the bar's beats"): FL 26 gives a 4 s
    /// sample at 140 BPM and PPQ 96 a clip of 448 ticks in 3/8, 896 in 4/4 (4 s × 140 / 60 × 96 × steps / 4). A
    /// stretch TIME is in quarter notes and keeps its ticks.
    #[test]
    fn audio_clips_fit_the_bars_beat() {
        use crate::playlist as pl;
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.project.ppq = 96;
        app.state.tempo = 140.0;
        app.state.playlist.reset();
        let wav = std::env::temp_dir().join(format!("os-app-fit-bar-{}.wav", std::process::id()));
        os_engine::wav::write_f32_stereo(&mut std::fs::File::create(&wav).unwrap(), 44100, &vec![0.25; 2 * 176400]).unwrap();
        app.state.project.bar = Some((3, 2));
        pl::add_audio_clip(&mut app.state, &wav.to_string_lossy(), 0, 0).expect("audio clip added");
        let row = app.state.project.channels.len() - 1;
        let len = |app: &App| pl::clips(&app.state).last().unwrap().length;
        assert_eq!(len(&app), 448, "3/8");
        for (bar, want) in [(Some((4, 4)), 896), (Some((3, 8)), 1792), (Some((5, 3)), 672), (Some((3, 2)), 448)] {
            app.state.project.bar = bar;
            pl::fit_audio(&mut app.state);
            assert_eq!(len(&app), want, "{bar:?}");
        }
        app.state.project.channels[row].sound.stretch = [488f32.to_bits(), 0, 0, 0];
        pl::fit_audio(&mut app.state);
        assert_eq!(len(&app), 244, "TIME 0:10:04 in 3/8");
        let _ = std::fs::remove_file(&wav);
    }

    /// Tools menu (spec: ui/playlist.md §10): quick quantize moves clip starts onto the snap grid (one undo step);
    /// export renders every playing track to its own file, all as long as the song.
    #[test]
    fn playlist_quantize_and_export_tracks() {
        use crate::playlist as pl;
        use os_model::{Clip, ClipSource};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let (mut app, _, _, bar) = ruler_app(&mut c);
        let ppq = u32::from(app.state.project.ppq);
        app.state.playlist.snap = crate::snap::EditorSnap::Grid(pl::Snap::Step);
        let step = ppq / 4;
        let a = pl::arr_mut(&mut app.state);
        a.clips = vec![Clip::new(step + 5, bar, 0, ClipSource::Pattern(1)), Clip::new(2 * bar - 3, bar, 2, ClipSource::Pattern(1))];
        // Each channel plays a note so the tracks make sound.
        let ch = app.state.project.channels[0].index;
        app.state.project.patterns[0].notes = vec![os_model::Note { length: ppq, ..os_model::Note::step(ch, 0) }];
        let m = pl::run(&mut app, pl::Cmd::QuantizeStarts).unwrap();
        assert!(m.contains("moved 2"), "{m}");
        let starts: Vec<u32> = pl::clips(&app.state).iter().map(|c| c.position).collect();
        assert_eq!(starts, vec![step, 2 * bar], "nearest steps");
        app.key(&winit::keyboard::Key::Character("z".into()), true, false);
        assert_eq!(pl::clips(&app.state)[0].position, step + 5, "one undo step");
        // Export: two files (tracks 1 and 3), the same length, each with sound only where its clip is.
        app.state.path = Some(std::env::temp_dir().join(format!("os-export-{}.flp", std::process::id())));
        let m = pl::run(&mut app, pl::Cmd::ExportTracks).unwrap();
        let dir = std::env::temp_dir().join("Rendered").join(format!("os-export-{} tracks", std::process::id()));
        assert!(m.starts_with("Exported 2 tracks"), "{m}");
        let read = |name: &str| os_engine::sample::load(&dir.join(name)).expect(name);
        let (t1, t3) = (read("01 Track 1.wav"), read("03 Track 3.wav"));
        assert!(t1.frames() >= os_engine::laws::tick_to_frame(f64::from(3 * bar - 3), 96, app.state.tempo, 44100) as usize);
        let loud = |s: &os_engine::sample::Sample, a: usize, b: usize| s.left[a..b.min(s.frames())].iter().any(|v| v.abs() > 0.01);
        let f = |t: u32| os_engine::laws::tick_to_frame(f64::from(t), 96, app.state.tempo, 44100) as usize;
        assert!(loud(&t1, f(step), f(step + ppq)) && !loud(&t3, f(step), f(step + ppq)), "track 1's clip only in its file");
        assert!(loud(&t3, f(2 * bar), f(2 * bar + ppq)), "track 3's clip in its file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Arrangements (spec: ui/playlist.md §12): add, rename, clone, merge, delete, switch; saved and read back with
    /// the shown one.
    #[test]
    fn playlist_arrangements() {
        use crate::playlist as pl;
        use os_model::{Clip, ClipSource};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let (mut app, _, _, bar) = ruler_app(&mut c);
        pl::place(&mut app.state, 0, 0);
        let first = app.state.project.arrangement().unwrap().number;
        // The title's arrangement name opens the menu.
        frame(&mut app, &mut c, None);
        let title = hit_rect(&app, &Action::PlArrangement);
        click(&mut app, &mut c, title);
        assert!(app.state.playlist.popup.is_some(), "arrangement menu");
        app.state.playlist.popup = None;
        // Add one, named through the box.
        pl::run(&mut app, pl::Cmd::ArrAdd);
        assert_eq!(app.state.project.arrangements.len(), 2);
        let added = app.state.project.arrangement().unwrap().number;
        assert_ne!(added, first);
        assert!(pl::clips(&app.state).is_empty(), "a new arrangement is empty");
        app.state.playlist.edit.as_mut().unwrap().text = "Verse".into();
        pl::commit_edit(&mut app.state);
        assert_eq!(pl::arrangement_name(&app.state), "Verse");
        app.state.project.arrangement_mut().unwrap().clips.push(Clip::new(bar, bar, 1, ClipSource::Pattern(1)));
        // Clone it, then merge the first arrangement's clips into the clone.
        pl::run(&mut app, pl::Cmd::ArrClone);
        pl::commit_edit(&mut app.state);
        assert_eq!(pl::arrangement_name(&app.state), "Verse #2");
        pl::run(&mut app, pl::Cmd::ArrMerge(first));
        assert_eq!(pl::clips(&app.state).len(), 2, "its own clip and the merged one");
        // Delete the clone: the one before it shows.
        pl::run(&mut app, pl::Cmd::ArrDelete);
        assert_eq!((app.state.project.arrangements.len(), app.state.project.arrangement().unwrap().number), (2, added));
        // Saved and read back with "Verse" shown.
        let path = std::env::temp_dir().join(format!("os-app-arrangements-{}.flp", std::process::id()));
        app.save_to(path.clone());
        let back = os_flp::project::load(&os_flp::FlpFile::parse(&std::fs::read(&path).unwrap()).unwrap()).unwrap();
        let names: Vec<_> = back.arrangements.iter().map(|a| a.name.clone().unwrap_or_default()).collect();
        assert_eq!(names, vec!["Arrangement".to_string(), "Verse".to_string()]);
        assert_eq!(back.arrangement().unwrap().name.as_deref(), Some("Verse"));
        // Edits to a file that already has arrangements: add one more, delete the first, save again.
        let mut app2 = App::new(Some(path.clone()));
        pl::run(&mut app2, pl::Cmd::ArrSelect(first));
        pl::run(&mut app2, pl::Cmd::ArrDelete);
        pl::run(&mut app2, pl::Cmd::ArrAdd);
        pl::commit_edit(&mut app2.state);
        app2.save_to(path.clone());
        let back = os_flp::project::load(&os_flp::FlpFile::parse(&std::fs::read(&path).unwrap()).unwrap()).unwrap();
        let names: Vec<_> = back.arrangements.iter().map(|a| a.name.clone().unwrap_or_default()).collect();
        assert_eq!(names, vec!["Verse".to_string(), "Arrangement 2".to_string()], "removed and added in an existing file");
        assert!(back.arrangements[0].clips.iter().any(|c| c.position == bar), "the kept arrangement keeps its clips");
        let _ = std::fs::remove_file(&path);
    }

    /// Audio clip handles (spec: ui/playlist.md §9a): dragging the fade triangles sets the fades, the tension ring
    /// bends the curve, the gain half-circle changes the gain; one undo step each.
    #[test]
    fn audio_clip_fade_handles() {
        use crate::playlist as pl;
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let (mut app, _, _, _) = ruler_app(&mut c);
        app.state.playlist_bar_w = 400.0;
        let wav = std::env::temp_dir().join(format!("os-app-clip-fades-{}.wav", std::process::id()));
        os_engine::wav::write_f32_stereo(&mut std::fs::File::create(&wav).unwrap(), 48000, &[0.5; 96000]).unwrap();
        pl::add_audio_clip(&mut app.state, &wav.to_string_lossy(), 0, 0).expect("audio clip added");
        let ci = pl::clips(&app.state).len() - 1;
        assert_eq!(pl::clips(&app.state)[ci].gain(), 1.0, "a new audio clip stores gain 1.0");
        frame(&mut app, &mut c, None);
        let g = pl::PGeom::new(&app.state, app.pl_grid().unwrap());
        let body = g.body(&pl::clips(&app.state)[ci]);
        let drag = |app: &mut App, c: &mut Canvas, (x0, y0): (f32, f32), (x1, y1): (f32, f32)| {
            app.mouse_move(x0, y0);
            app.mouse_down();
            for k in 1..=8 {
                let f = k as f32 / 8.0;
                app.mouse_move(x0 + (x1 - x0) * f, y0 + (y1 - y0) * f);
            }
            app.mouse_up();
            frame(app, c, None);
        };
        // Fade-in: the top-left triangle dragged 100 px right.
        drag(&mut app, &mut c, (body.x + 2.0, body.y + 2.0), (body.x + 102.0, body.y + 2.0));
        let clip = pl::clips(&app.state)[ci];
        let ms_per_tick = pl::to_src(&app.state, clip.source, 1.0);
        let ms = |px: f32| f64::from(px / g.ppt) * ms_per_tick;
        let (fin, t) = clip.fade_in().expect("a fade-in");
        // The fade ends under the mouse.
        assert!((f64::from(fin) - ms(102.0)).abs() < 2.0, "fade-in {fin} ms vs {}", ms(102.0));
        assert_eq!(t, 0.0);
        // Fade-out: the top-right triangle dragged 60 px left.
        drag(&mut app, &mut c, (body.right() - 2.0, body.y + 2.0), (body.right() - 62.0, body.y + 2.0));
        let (fout, _) = pl::clips(&app.state)[ci].fade_out().expect("a fade-out");
        assert!((f64::from(fout) - ms(62.0)).abs() < 2.0, "fade-out {fout} ms");
        // Tension: the fade-in's ring dragged up 10 px.
        let (mx, my) = pl::fade_mid(body, body.x, pl::fade_xs(&app.state, &g, &pl::clips(&app.state)[ci]).0, 0.0, false);
        app.mouse_move(mx, my);
        assert_eq!(app.state.playlist.hover_handle, Some(pl::Handle::TensionIn));
        drag(&mut app, &mut c, (mx, my), (mx, my - 10.0));
        let t = pl::clips(&app.state)[ci].fade_in().unwrap().1;
        assert!((t - 2.8).abs() < 0.01, "tension {t}");
        // Gain: the half-circle dragged up 20 px = +3 dB.
        drag(&mut app, &mut c, (body.cx(), body.bottom() - 2.0), (body.cx(), body.bottom() - 22.0));
        let gain = pl::clips(&app.state)[ci].gain();
        assert!((20.0 * gain.log10() - 3.0).abs() < 0.01, "gain {gain}");
        // Each drag is one undo step.
        app.key(&winit::keyboard::Key::Character("z".into()), true, false);
        assert_eq!(pl::clips(&app.state)[ci].gain(), 1.0);
        app.key(&winit::keyboard::Key::Character("z".into()), true, false);
        assert_eq!(pl::clips(&app.state)[ci].fade_in().unwrap().1, 0.0);
        let _ = std::fs::remove_file(&wav);
    }

    /// Automatic crossfades (spec: audio/audio-clips.md "Automatic crossfades", ui/playlist.md §9a): with the switch
    /// on, two overlapping audio clips get the overlap as the earlier clip's fade-out and the later clip's fade-in,
    /// without their bits. They follow a move and the tempo, undo and redo with the edit that made them, go to 0 when
    /// the switch is turned off (tensions kept) and come back when it is turned on; the crossfade's handle sets both
    /// tensions in one undo step; an opened project's crossfades are not a change and save as they were read.
    #[test]
    fn automatic_crossfades_follow_the_clips() {
        use crate::playlist as pl;
        use crate::views::settings::{SAct, CROSSFADES_CHECK};
        use os_model::crossfade::{ms, tension_for_mid};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let (mut app, _, _, _) = ruler_app(&mut c);
        app.state.playlist_bar_w = 400.0;
        app.state.project.auto_crossfades = Some(true);
        app.state.tempo = 130.0;
        let wav = std::env::temp_dir().join(format!("os-app-crossfades-{}.wav", std::process::id()));
        os_engine::wav::write_f32_stereo(&mut std::fs::File::create(&wav).unwrap(), 48000, &vec![0.5; 2 * 96000]).unwrap();
        // Two 2 s clips (416 ticks at 130 BPM), the second from tick 192: 224 ticks of overlap.
        let path = wav.to_string_lossy().into_owned();
        pl::add_audio_clip(&mut app.state, &path, 0, 0).expect("audio clip added");
        pl::add_audio_clip(&mut app.state, &path, 0, 192).expect("audio clip added");
        let srcs: Vec<_> = pl::clips(&app.state).iter().map(|c| c.source).collect();
        let (sa, sb) = (srcs[srcs.len() - 2], srcs[srcs.len() - 1]);
        let idx = |app: &App, src| pl::clips(&app.state).iter().position(|c| c.source == src).unwrap();
        let get = |app: &App, src| pl::clips(&app.state)[idx(app, src)];
        // (the earlier clip's fade-out, the later clip's fade-in), as stored.
        let xf = |app: &App| (get(app, sa).stored_fade_out().map(|f| f.0), get(app, sb).stored_fade_in().map(|f| f.0));
        app.tick();
        assert_eq!(get(&app, sb).length, 416);
        assert_eq!(xf(&app), (Some(ms(224, 130.0, 96)), Some(ms(224, 130.0, 96))));
        assert_eq!((get(&app, sa).raw[56] & 3, get(&app, sb).raw[56] & 3), (0, 0), "without their bits");
        assert_eq!((get(&app, sa).stored_fade_in(), get(&app, sb).stored_fade_out()), (None, None));

        // Moved a beat right with the mouse: 128 ticks of overlap. Undone and redone with the move.
        frame(&mut app, &mut c, None);
        let g = pl::PGeom::new(&app.state, app.pl_grid().unwrap());
        let body = g.body(&get(&app, sb));
        let (x0, y0) = (body.x + body.w * 0.75, body.y + body.h * 0.4);
        app.mouse_move(x0, y0);
        app.mouse_down();
        for k in 1..=8 {
            app.mouse_move(x0 + 96.0 * g.ppt * k as f32 / 8.0, y0);
        }
        app.mouse_up();
        app.tick();
        assert_eq!(get(&app, sb).position, 288);
        assert_eq!(xf(&app), (Some(ms(128, 130.0, 96)), Some(ms(128, 130.0, 96))));
        crate::history::undo(&mut app.state);
        assert_eq!((get(&app, sb).position, xf(&app)), (192, (Some(ms(224, 130.0, 96)), Some(ms(224, 130.0, 96)))), "undone with the move");
        crate::history::redo(&mut app.state);
        assert_eq!((get(&app, sb).position, xf(&app)), (288, (Some(ms(128, 130.0, 96)), Some(ms(128, 130.0, 96)))));
        crate::history::undo(&mut app.state);
        app.tick();

        // The tempo halved: the clips keep their sample's 2 s, now 208 ticks (§9c), so 16 ticks overlap, in
        // milliseconds at the new tempo.
        app.set_tempo(65.0);
        app.tick();
        assert_eq!((get(&app, sa).length, get(&app, sb).position), (208, 192));
        assert_eq!(xf(&app), (Some(ms(16, 65.0, 96)), Some(ms(16, 65.0, 96))));
        app.set_tempo(130.0);
        app.tick();

        // The crossfade's handle, hit where it is drawn at every scale: dragged up 18 px it moves the later clip's
        // gain at the middle by 18/120; the earlier clip's fade-out takes the negative tension; one undo step.
        for k in [1.0f32, 1.5, 2.0] {
            let Ok(mut ck) = Canvas::new((1920.0 * k) as u32, (1080.0 * k) as u32, k) else { return };
            app.invalidate();
            frame(&mut app, &mut ck, None);
            let g = pl::PGeom::new(&app.state, app.pl_grid().unwrap());
            let xfade = pl::crossfade(&app.state, &g, idx(&app, sb)).expect("a crossfade to draw");
            assert_eq!(xfade.a, idx(&app, sa));
            let (hx, hy, ry) = pl::xfade_handle(&g, &get(&app, sb), &xfade);
            assert_eq!(hy, ry, "at tension 0 both gains are 1/2 in the middle");
            app.mouse_move(hx, hy);
            assert_eq!(app.state.playlist.hover_handle, Some(pl::Handle::Crossfade), "at {k}");
            frame(&mut app, &mut ck, None);
            let p = ck.pixmap.pixel((hx * k) as u32, (hy * k - pl::XFADE_R * k * 0.6) as u32).unwrap();
            assert!(p.red() > 200 && p.green() > 200 && p.blue() > 200, "the big circle drawn there at {k}: {p:?}");
            app.mouse_move(hx + 40.0, hy - 30.0);
        }
        let g = pl::PGeom::new(&app.state, app.pl_grid().unwrap());
        let xfade = pl::crossfade(&app.state, &g, idx(&app, sb)).unwrap();
        let (hx, hy, _) = pl::xfade_handle(&g, &get(&app, sb), &xfade);
        app.mouse_move(hx, hy);
        app.mouse_down();
        for k in 1..=6 {
            app.mouse_move(hx, hy - 3.0 * k as f32);
        }
        app.mouse_up();
        let t = tension_for_mid(0.5 + 18.0 / 120.0) as f32;
        assert_eq!((get(&app, sb).fade_in_tension(), get(&app, sa).fade_out_tension()), (t, -t));
        assert_eq!((get(&app, sa).raw[56] & 3, get(&app, sb).raw[56] & 3), (0, 0), "still the crossfade's");
        assert!(app.hint.contains("Crossfade tension: +2.45"), "{}", app.hint);
        app.tick();
        assert_eq!((get(&app, sb).fade_in_tension(), get(&app, sa).fade_out_tension()), (t, -t), "as the overlaps make them");
        crate::history::undo(&mut app.state);
        assert_eq!((get(&app, sb).fade_in_tension(), get(&app, sa).fade_out_tension()), (0.0, 0.0), "one step");
        crate::history::redo(&mut app.state);

        // Switched off: the lengths go to 0, the tensions stay, and no edit sets them; switched on: set again.
        app.settings_act(SAct::Check(CROSSFADES_CHECK));
        assert_eq!(app.state.project.auto_crossfades, Some(false));
        assert_eq!(xf(&app), (None, None));
        assert_eq!((get(&app, sb).fade_in_tension(), get(&app, sa).fade_out_tension()), (t, -t));
        app.tick();
        assert_eq!(xf(&app), (None, None));
        app.settings_act(SAct::Check(CROSSFADES_CHECK));
        app.tick();
        assert_eq!(xf(&app), (Some(ms(224, 130.0, 96)), Some(ms(224, 130.0, 96))));

        // Saved and opened again: worked out on opening, nothing changes; saved again, the same bytes.
        let flp = std::env::temp_dir().join(format!("os-app-crossfades-{}.flp", std::process::id()));
        app.save_to(flp.clone());
        let bytes = |p: &Project| p.arrangements.iter().flat_map(|a| a.clips.iter().map(|c| c.raw[36..57].to_vec())).collect::<Vec<_>>();
        let mut opened = App::new(None);
        opened.open(flp.clone());
        let read = bytes(&opened.state.project);
        opened.tick();
        assert!(!opened.unsaved(), "an opened project's crossfades are not a change");
        assert_eq!(bytes(&opened.state.project), read);
        opened.save_to(flp.clone());
        let back = os_flp::project::load(&os_flp::FlpFile::parse(&std::fs::read(&flp).unwrap()).unwrap()).unwrap();
        assert_eq!(bytes(&back), read);
        assert_eq!(back.auto_crossfades, Some(true));
        let _ = std::fs::remove_file(&flp);
        let _ = std::fs::remove_file(&wav);
    }

    /// Track groups (spec: ui/playlist.md §6a): grouped tracks follow their group track; a collapsed group's members
    /// become strips; the group track's arrow expands it again.
    #[test]
    fn playlist_track_groups() {
        use crate::playlist as pl;
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let (mut app, _, _, _) = ruler_app(&mut c);
        pl::run(&mut app, pl::Cmd::TrackGroup(1));
        pl::run(&mut app, pl::Cmd::TrackGroup(2));
        assert!(pl::is_group_track(&app.state, 0));
        assert_eq!((pl::group_track(&app.state, 2), pl::group_track(&app.state, 3)), (Some(0), None));
        assert_eq!(pl::Rows::new(&app.state).top(3), 3.0 * pl::ROW_H);
        // A member collapses its group.
        pl::run(&mut app, pl::Cmd::GroupCollapse(2));
        let rows = pl::Rows::new(&app.state);
        assert!(rows.strip(1) && rows.strip(2) && !rows.strip(0) && !rows.strip(3));
        assert_eq!(rows.top(3), pl::ROW_H + 2.0 * pl::STRIP_H);
        frame(&mut app, &mut c, None);
        let g = pl::PGeom::new(&app.state, app.pl_grid().unwrap());
        assert_eq!(g.track_at(g.y_of(3) + 1.0), 3, "rows below the strips map back to their tracks");
        let arrow = hit_rect(&app, &Action::PlGroupArrow(0));
        click(&mut app, &mut c, arrow);
        assert!(!pl::Rows::new(&app.state).strip(1), "the arrow expands the group");
        // Undo brings the collapse back.
        app.key(&winit::keyboard::Key::Character("z".into()), true, false);
        assert!(pl::Rows::new(&app.state).strip(1));
    }

    /// Double-clicking an audio clip opens its channel's Lemon Sampler and selects the channel in the rack; one
    /// click does not; a pattern clip opens its pattern in the piano roll (spec: ui/playlist.md §9, §2b).
    #[test]
    fn audio_clip_double_click_opens_its_sampler() {
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let (mut app, ruler, g, bar) = ruler_app(&mut c);
        let wav = std::env::temp_dir().join(format!("os-app-clip-dblclick-{}.wav", std::process::id()));
        os_engine::wav::write_f32_stereo(&mut std::fs::File::create(&wav).unwrap(), 48000, &[0.5; 96000]).unwrap();
        crate::playlist::add_audio_clip(&mut app.state, &wav.to_string_lossy(), 1, 0).expect("audio clip added");
        let row = app.state.project.channels.len() - 1;
        app.state.project.arrangements[0].clips.push(os_model::Clip::new(0, bar, 2, os_model::ClipSource::Pattern(1)));
        frame(&mut app, &mut c, None);
        let _ = ruler;
        let tap = |app: &mut App, c: &mut Canvas, track: u16| {
            let (x, y) = (g.x_of(10.0) + 10.0, g.y_of(track) + 30.0);
            app.mouse_move(x, y);
            app.mouse_down();
            app.mouse_up();
            frame(app, c, None);
        };
        tap(&mut app, &mut c, 1);
        assert_eq!(app.state.pending_open, None, "one click only selects");
        tap(&mut app, &mut c, 1);
        assert_eq!(app.state.pending_open, Some(row), "a double click opens the clip's sampler");
        assert_eq!(app.state.selected_channel, row);
        assert_eq!(app.state.rack.sel, std::collections::BTreeSet::from([row]));
        app.state.pending_open = None;
        app.state.pattern = 2;
        tap(&mut app, &mut c, 2);
        tap(&mut app, &mut c, 2);
        assert_eq!(app.state.pending_open, None, "a pattern clip opens no sampler");
        assert!(app.state.shown(Win::PianoRoll) && app.state.pattern == 1, "it opens its pattern in the piano roll");
        let _ = std::fs::remove_file(&wav);
    }

    /// A pattern picked in the pattern selector becomes the playlist's clip source even after a click on a clip made
    /// another pattern the source, so a new pattern is what Draw places (issue #94).
    #[test]
    fn pattern_selector_sets_the_clip_source() {
        use crate::views::menus::{PATTERNS, PATTERN_PICK};
        use os_model::ClipSource;
        let mut app = App::new(None);
        app.state.playlist.source = Some(ClipSource::Pattern(1));
        app.menu_item(PATTERNS, PATTERN_PICK + 2);
        assert_eq!(app.state.pattern, 2);
        assert_eq!(crate::playlist::source(&app.state), ClipSource::Pattern(2));
        crate::playlist::place(&mut app.state, 0, 0);
        assert_eq!(crate::playlist::clips(&app.state).last().map(|c| c.source), Some(ClipSource::Pattern(2)));
    }

    /// Double-click on audio clips follows General ▸ Input ▸ Manage shortcuts: the row whose modifier is held wins,
    /// Show clip properties does nothing yet, and a row set to "(disabled)" never fires (spec: ui/settings-choices.md
    /// "Input: Manage shortcuts (2026-10-16)").
    #[test]
    fn audio_clip_double_click_follows_manage_shortcuts() {
        use crate::shortcuts::{Form, CLIP_CHANNEL, CLIP_PROPERTIES};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let (mut app, _, g, _) = ruler_app(&mut c);
        let wav = std::env::temp_dir().join(format!("os-app-clip-dblclick-sc-{}.wav", std::process::id()));
        os_engine::wav::write_f32_stereo(&mut std::fs::File::create(&wav).unwrap(), 48000, &[0.5; 96000]).unwrap();
        crate::playlist::add_audio_clip(&mut app.state, &wav.to_string_lossy(), 1, 0).expect("audio clip added");
        let row = app.state.project.channels.len() - 1;
        frame(&mut app, &mut c, None);
        let double = |app: &mut App, c: &mut Canvas, alt: bool| {
            let (x, y) = (g.x_of(10.0) + 10.0, g.y_of(1) + 30.0);
            app.alt = alt;
            for _ in 0..2 {
                app.mouse_move(x, y);
                app.mouse_down();
                app.mouse_up();
                frame(app, c, None);
            }
            app.alt = false;
            app.state.pending_open.take()
        };
        // The defaults: Alt is Show clip properties (no window yet), no modifier Show channel settings.
        assert_eq!(double(&mut app, &mut c, true), None, "Show clip properties opens nothing yet");
        assert_eq!(double(&mut app, &mut c, false), Some(row));
        // Show channel settings to Alt (Show clip properties takes "(none)"): only Alt opens the sampler.
        let mut f = Form::applied();
        f.pick(8, 2);
        f.apply();
        assert_eq!((crate::prefs::choice(&CLIP_PROPERTIES), crate::prefs::choice(&CLIP_CHANNEL)), ("(none)", "Alt"));
        assert_eq!(double(&mut app, &mut c, false), None);
        assert_eq!(double(&mut app, &mut c, true), Some(row));
        // Show channel settings "(disabled)": no double-click opens it.
        f.pick(8, 0);
        f.apply();
        assert_eq!(double(&mut app, &mut c, true), None, "(disabled) never fires");
        assert_eq!(double(&mut app, &mut c, false), None);
        let _ = std::fs::remove_file(&wav);
    }

    /// A pattern playing when the ruler is clicked goes on as the song from the clicked position (needs an audio
    /// device).
    #[test]
    #[ignore]
    fn ruler_click_takes_a_playing_pattern_to_the_song() {
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let (mut app, ruler, g, bar) = ruler_app(&mut c);
        let ch = app.state.project.channels[0].index;
        app.state.project.patterns = vec![Pattern { number: 1, notes: vec![Note { length: 24, key: 60, ..Note::step(ch, 0) }], ..Pattern::default() }];
        app.state.project.arrangements = vec![os_model::Arrangement {
            number: 1,
            clips: vec![os_model::Clip::new(0, 8 * bar, 0, os_model::ClipSource::Pattern(1))],
            ..Default::default()
        }];
        app.act(Action::Play);
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(app.transport.playing() && !app.transport.song());
        ruler_press(&mut app, &mut c, g.x_of(4.0 * bar as f32) + 1.0, g.x_of(4.0 * bar as f32) + 1.0, ruler.cy(), false);
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(app.state.song_mode && app.transport.playing() && app.transport.song(), "the song plays");
        let tick = app.transport.tick();
        assert!(tick >= f64::from(4 * bar) && tick < f64::from(5 * bar), "from the clicked bar: {tick}");
        app.act(Action::Stop);
    }

    /// Playlist (spec: ui/playlist.md): Draw places / moves / resizes clips, undo, Mute tool, right-click delete,
    /// Ctrl+B, time selection + Ctrl+Insert, track LED, automation clip + point, save and reload.
    #[test]
    fn playlist_editing() {
        use crate::playlist::{self as pl, PlTool};
        use winit::keyboard::{Key, NamedKey};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.file = None;
        app.state.path = None;
        app.state.playlist.reset();
        app.state.z = vec![Win::Playlist];
        frame(&mut app, &mut c, None);
        let grid = app.pl_grid().expect("playlist grid");
        let g = pl::PGeom::new(&app.state, grid);
        let bar = 4 * u32::from(app.state.project.ppq);
        // Clicks land 10 px into a cell: Cell places clips at its start (Line would round to the nearest beat).
        app.state.playlist.snap = crate::snap::EditorSnap::Grid(pl::Snap::Cell);
        let at = |t: u32, track: u16| (g.x_of(t as f32) + 10.0, g.y_of(track) + 30.0);
        let drag = |app: &mut App, c: &mut Canvas, (x0, y0): (f32, f32), (x1, y1): (f32, f32), right: bool| {
            app.mouse_move(x0, y0);
            if right { app.mouse_down_right() } else { app.mouse_down() }
            for i in 1..=10 {
                let k = i as f32 / 10.0;
                app.mouse_move(x0 + (x1 - x0) * k, y0 + (y1 - y0) * k);
            }
            app.mouse_up();
            frame(app, c, None);
        };
        let tap = |app: &mut App, c: &mut Canvas, p: (f32, f32), right: bool| drag(app, c, p, p, right);
        let on_track = |app: &App, t: u16| -> Vec<os_model::Clip> { pl::clips(&app.state).iter().filter(|c| c.track == t).copied().collect() };

        // Draw: a click places the current pattern (one bar when empty).
        tap(&mut app, &mut c, at(0, 0), false);
        tap(&mut app, &mut c, at(2 * bar, 2), false);
        assert_eq!(pl::clips(&app.state).len(), 2);
        let a = on_track(&app, 0)[0];
        assert_eq!((a.position, a.length, a.source), (0, bar, os_model::ClipSource::Pattern(1)));
        // Drag the body moves (snapped); dragging the right edge resizes.
        drag(&mut app, &mut c, at(0, 0), (g.x_of(4.0 * bar as f32) + 10.0, g.y_of(0) + 30.0), false);
        assert_eq!(on_track(&app, 0)[0].position, 4 * bar);
        let b = on_track(&app, 2)[0];
        let end = g.x_of(b.end() as f32) - 2.0;
        drag(&mut app, &mut c, (end, g.y_of(2) + 30.0), (end + g.bar_w, g.y_of(2) + 30.0), false);
        assert_eq!(on_track(&app, 2)[0].length, 2 * bar);
        // Undo restores the length.
        app.key(&Key::Character("z".into()), true, false);
        assert_eq!(on_track(&app, 2)[0].length, bar);
        frame(&mut app, &mut c, None);

        // Mute tool toggles; right-click deletes.
        app.act(Action::PlTool(PlTool::Mute));
        tap(&mut app, &mut c, at(2 * bar, 2), false);
        assert!(on_track(&app, 2)[0].muted);
        app.act(Action::PlTool(PlTool::Draw));
        tap(&mut app, &mut c, at(2 * bar, 2), true);
        assert!(on_track(&app, 2).is_empty());

        // Ctrl+A, Ctrl+B duplicates the selection after itself.
        app.key(&Key::Character("a".into()), true, false);
        app.key(&Key::Character("b".into()), true, false);
        let t0: Vec<u32> = on_track(&app, 0).iter().map(|c| c.position).collect();
        assert_eq!(t0.len(), 2, "{t0:?}");
        assert!(t0.contains(&(5 * bar)), "{t0:?}");
        app.key(&Key::Character("d".into()), true, false);
        frame(&mut app, &mut c, None);

        // Right-drag in the ruler selects time; Ctrl+Insert inserts that much space.
        let ruler = hit_rect(&app, &Action::PlRuler);
        drag(&mut app, &mut c, (g.x_of(0.0) + 1.0, ruler.cy()), (g.x_of(bar as f32), ruler.cy()), true);
        assert_eq!(app.state.playlist.range, Some((0, bar)));
        app.key(&Key::Named(NamedKey::Insert), true, false);
        let mut t0: Vec<u32> = on_track(&app, 0).iter().map(|c| c.position).collect();
        t0.sort_unstable();
        assert_eq!(t0, vec![5 * bar, 6 * bar]);
        frame(&mut app, &mut c, None);
        // One left click in the ruler keeps the time selection; a double click clears it.
        let r = (g.x_of(2.0 * bar as f32), ruler.cy());
        app.state.playlist.last_ruler = None;
        tap(&mut app, &mut c, r, false);
        assert_eq!(app.state.playlist.range, Some((0, bar)), "a single click keeps it");
        tap(&mut app, &mut c, r, false);
        assert_eq!(app.state.playlist.range, None, "a double click clears it");
        tap(&mut app, &mut c, r, false);
        drag(&mut app, &mut c, (g.x_of(0.0) + 1.0, ruler.cy()), (g.x_of(bar as f32), ruler.cy()), true);
        tap(&mut app, &mut c, r, false);
        assert_eq!(app.state.playlist.range, Some((0, bar)), "a third click starts a new count");
        frame(&mut app, &mut c, None);

        // The track LED mutes the track.
        let led = hit_rect(&app, &Action::PlLed(0));
        tap(&mut app, &mut c, (led.cx(), led.cy()), false);
        assert!(!pl::track_on(&app.state, 0));

        // An automation clip (tempo), then a left-click on its curve adds a point.
        app.state.playlist.range = None;
        pl::run(&mut app, pl::Cmd::CreateAutomation(os_model::target::TEMPO, "Tempo".into(), 0.5));
        frame(&mut app, &mut c, None);
        let ch = app.state.project.automations.last().unwrap().channel;
        let ac = *pl::clips(&app.state).iter().find(|c| c.source == os_model::ClipSource::Channel(ch)).unwrap();
        let g = pl::PGeom::new(&app.state, app.pl_grid().unwrap());
        let body = g.body(&ac);
        let x = g.x_of((ac.position + ac.length / 4) as f32); // the midpoint is the tension handle
        let n = app.state.project.automation(ch).unwrap().points.len();
        app.mouse_move(x, pl::value_y(body, 0.5));
        app.mouse_down();
        app.mouse_up();
        assert_eq!(app.state.project.automation(ch).unwrap().points.len(), n + 1);

        // Save (new project) and read back.
        let path = std::env::temp_dir().join(format!("openstudio-pl-{}.flp", std::process::id()));
        app.save_to(path.clone());
        let back = os_flp::project::load(&os_flp::FlpFile::parse(&std::fs::read(&path).unwrap()).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        let arr = back.arrangement().expect("arrangement saved");
        assert_eq!(arr.clips.len(), pl::clips(&app.state).len());
        assert!(!arr.tracks[0].enabled);
        assert_eq!(back.automation(ch).map(|a| a.points.len()), Some(n + 1));
    }

    /// The playlist's tool letters switch its tools with Shift too, typing keyboard on or off (it takes the plain letters
    /// as notes); Shift+T and Shift+Z keep Place loop and Zoom to selection (as the piano roll's, spec: ui/piano-keys.md
    /// §4 rule 16).
    #[test]
    fn playlist_shift_letters_switch_tools() {
        use crate::playlist::PlTool;
        use winit::keyboard::{Key, KeyCode};
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.playlist.reset();
        // The piano roll behind: a letter the playlist leaves goes on to the piano roll's tools.
        app.state.z = vec![Win::PianoRoll, Win::Playlist];
        assert!(app.pl_focus());
        // One key press the way main.rs delivers it.
        let press = |app: &mut App, code: KeyCode, l: &str, shift: bool| {
            app.shift = shift;
            if !app.typing_key(code, true, false, false) {
                app.key(&Key::Character(l.into()), false, false);
            }
            app.typing_key(code, false, false, false);
            app.shift = false;
        };
        if app.typing_on() {
            app.toggle_typing();
        }
        press(&mut app, KeyCode::KeyE, "e", false);
        assert_eq!(app.state.playlist.tool, PlTool::Select, "typing keyboard off: E");
        press(&mut app, KeyCode::KeyP, "P", true);
        assert_eq!(app.state.playlist.tool, PlTool::Draw, "typing keyboard off: Shift+P");

        app.toggle_typing();
        assert!(app.typing_on());
        let piano = app.state.piano.tool;
        press(&mut app, KeyCode::KeyE, "e", false);
        assert_eq!(app.state.playlist.tool, PlTool::Draw, "typing keyboard on: E plays a note");
        for (code, l, t) in [
            (KeyCode::KeyE, "E", PlTool::Select),
            (KeyCode::KeyB, "B", PlTool::Paint),
            (KeyCode::KeyS, "S", PlTool::Slip),
            (KeyCode::KeyC, "C", PlTool::Slice),
            (KeyCode::KeyD, "D", PlTool::Delete),
            (KeyCode::KeyY, "Y", PlTool::Playback),
            (KeyCode::KeyP, "P", PlTool::Draw),
        ] {
            press(&mut app, code, l, true);
            assert_eq!(app.state.playlist.tool, t, "typing keyboard on: Shift+{l}");
        }
        assert_eq!(app.state.piano.tool, piano, "the piano roll's tool stays");
        press(&mut app, KeyCode::KeyZ, "Z", true);
        press(&mut app, KeyCode::KeyT, "T", true);
        assert_eq!(app.state.playlist.tool, PlTool::Draw, "Shift+Z zooms, Shift+T places a loop");
    }

    /// Enable groups (spec: ui/enable-groups.md): the toolbar button, OPTIONS > Enable groups and Shift+Alt+G toggle it
    /// and deselect the clips; while off, a grouped clip selects and moves alone and Shift+G groups only the selection;
    /// the groups themselves are kept.
    #[test]
    fn enable_groups_switch() {
        use crate::playlist as pl;
        use winit::keyboard::Key;
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.playlist.reset();
        app.state.playlist.row_h = pl::ROW_H;
        app.state.z = vec![Win::Playlist];
        frame(&mut app, &mut c, None);
        let g = pl::PGeom::new(&app.state, app.pl_grid().expect("playlist grid"));
        let bar = 4 * u32::from(app.state.project.ppq);
        // Clicks land 10 px into a cell: Cell places clips at its start (Line would round to the nearest beat).
        app.state.playlist.snap = crate::snap::EditorSnap::Grid(pl::Snap::Cell);
        let at = |t: u32, track: u16| (g.x_of(t as f32) + 10.0, g.y_of(track) + 30.0);
        let drag = |app: &mut App, c: &mut Canvas, (x0, y0): (f32, f32), (x1, y1): (f32, f32)| {
            app.state.playlist.last_clip = None;
            app.mouse_move(x0, y0);
            app.mouse_down();
            for i in 1..=10 {
                let k = i as f32 / 10.0;
                app.mouse_move(x0 + (x1 - x0) * k, y0 + (y1 - y0) * k);
            }
            app.mouse_up();
            frame(app, c, None);
        };
        let pos = |app: &App, t: u16| pl::clips(&app.state).iter().find(|c| c.track == t).map(|c| (c.position, c.group())).unwrap();
        // Clips on tracks 0 and 1 grouped; a loose clip on track 2.
        for t in 0..3 {
            drag(&mut app, &mut c, at(t as u32 * bar, t), at(t as u32 * bar, t));
        }
        app.state.playlist.sel = vec![0, 1];
        app.shift = true;
        app.key(&Key::Character("G".into()), false, false);
        app.shift = false;

        // The toolbar button (lit while on) turns it off and deselects.
        assert!(crate::enable_groups::on());
        let r = hit_rect(&app, &Action::EnableGroups);
        assert_eq!((r.x, r.y), (679.0, 45.0));
        click(&mut app, &mut c, r);
        assert!(!crate::enable_groups::on());
        assert!(app.state.playlist.sel.is_empty(), "toggling deselects, as in FL");
        // Off: the grouped clip moves alone; the group is kept.
        drag(&mut app, &mut c, at(0, 0), at(bar, 0));
        assert_eq!((pos(&app, 0).0, pos(&app, 1).0), (bar, bar), "only the dragged clip moved");
        assert_eq!(app.state.playlist.sel.len(), 1);
        assert!(pos(&app, 0).1 != 0 && pos(&app, 0).1 == pos(&app, 1).1);
        // Off: Shift+G groups just the selection (track 0 and the loose clip), leaving its old mate behind.
        let i0 = pl::clips(&app.state).iter().position(|c| c.track == 0).unwrap();
        let i2 = pl::clips(&app.state).iter().position(|c| c.track == 2).unwrap();
        app.state.playlist.sel = vec![i0, i2];
        app.shift = true;
        app.key(&Key::Character("G".into()), false, false);
        app.shift = false;
        assert_eq!(pos(&app, 0).1, pos(&app, 2).1);
        assert_ne!(pos(&app, 0).1, pos(&app, 1).1);

        // Shift+Alt+G and OPTIONS > Enable groups toggle it too; on again, a click selects the whole group.
        app.shift = true;
        app.key(&Key::Character("G".into()), false, true);
        app.shift = false;
        assert!(crate::enable_groups::on());
        let opt = (0..60).find(|&i| views::menus::item_at(views::menus::OPTIONS_MENU, i).is_some_and(|it| it.label == "Grouped clips move together")).unwrap();
        app.menu_item(views::menus::OPTIONS_MENU, opt);
        assert!(!crate::enable_groups::on());
        app.menu_item(views::menus::OPTIONS_MENU, opt);
        assert!(crate::enable_groups::on());
        drag(&mut app, &mut c, at(2 * bar, 2), at(3 * bar, 2));
        assert_eq!((pos(&app, 0).0, pos(&app, 2).0), (2 * bar, 3 * bar), "the group moved together");
        assert_eq!(app.state.playlist.sel.len(), 2);
    }

    /// VIEW > Close all windows / unfocused / plugin windows and F12, Ctrl+F12, Alt+F12 (spec: ui/close-windows.md):
    /// the browser stays (and the Tempo tapper for Alt+F12); the focused window is the one in front, none after a
    /// click on the empty workspace.
    #[test]
    fn close_all_windows() {
        use winit::keyboard::{Key, NamedKey};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        let all = vec![Win::Playlist, Win::Mixer, Win::Plugin, Win::ChannelRack, Win::Perf, Win::Tapper, Win::PianoRoll];
        let f12 = Key::Named(NamedKey::F12);
        let view = |label: &str| (0..60).find(|&i| views::menus::item_at(views::menus::VIEW, i).is_some_and(|it| it.label == label)).unwrap();
        for label in ["Close all windows", "Close all plugin windows", "Close all unfocused windows"] {
            let it = views::menus::item_at(views::menus::VIEW, view(label)).unwrap();
            assert!(views::menus::works(views::menus::VIEW, it), "{label} is not greyed out");
        }

        // F12 and the menu entry: everything but the browser.
        app.state.z = all.clone();
        app.key(&f12, false, false);
        assert!(app.state.z.is_empty());
        assert!(app.state.show_browser);
        app.state.z = all.clone();
        app.menu_item(views::menus::VIEW, view("Close all windows"));
        assert!(app.state.z.is_empty());

        // Alt+F12: only the channel settings window (and hosted plugin windows).
        app.state.z = all.clone();
        app.key(&f12, false, true);
        assert_eq!(app.state.z, vec![Win::Playlist, Win::Mixer, Win::ChannelRack, Win::Perf, Win::Tapper, Win::PianoRoll], "the Tempo tapper stays too");
        app.state.z = all.clone();
        app.menu_item(views::menus::VIEW, view("Close all plugin windows"));
        assert!(!app.state.shown(Win::Plugin) && app.state.z.len() == all.len() - 1);

        // Ctrl+F12: all but the one in front (the last clicked).
        app.state.z = all.clone();
        app.state.raise(Win::Mixer);
        app.key(&f12, true, false);
        assert_eq!(app.state.z, vec![Win::Mixer]);
        app.state.z = all.clone();
        app.menu_item(views::menus::VIEW, view("Close all unfocused windows"));
        assert_eq!(app.state.z, vec![Win::PianoRoll]);
        // A click on the empty workspace leaves no window focused: Ctrl+F12 closes them all.
        app.state.z = vec![Win::ChannelRack];
        frame(&mut app, &mut c, None);
        let work = app.state.work_rect();
        app.mouse_move(work.right() - 20.0, work.bottom() - 20.0);
        app.mouse_down();
        app.mouse_up();
        assert!(app.state.no_focus);
        app.key(&f12, true, false);
        assert!(app.state.z.is_empty());
        // Opening or clicking a window focuses it again.
        app.state.raise(Win::Playlist);
        app.state.raise(Win::Mixer);
        assert!(!app.state.no_focus);
        app.key(&f12, true, false);
        assert_eq!(app.state.z, vec![Win::Mixer]);
        // Shift+F12 (Align all channel editors) is not available yet: nothing closes.
        app.shift = true;
        app.key(&f12, false, false);
        app.shift = false;
        assert_eq!(app.state.z, vec![Win::Mixer]);
    }

    /// The toolbar's song position slider (spec: ui/song-position-slider.md), with FL's measured numbers: an 8.25-bar
    /// song; a click beside the handle steps 1/88 of it toward the pointer and holding repeats up to the pointer; the
    /// handle drags; right-click jumps; the wheel moves 3/64 (down = later); PAT mode moves through the pattern (the
    /// play-start marker); an empty playlist disables it in SONG mode.
    #[test]
    fn song_position_slider() {
        use crate::song_pos::{handle_x, PANEL};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project.arrangements =
            vec![os_model::Arrangement { number: 1, clips: vec![os_model::Clip::new(0, 3168, 0, os_model::ClipSource::Pattern(1))], ..Default::default() }];
        app.state.song_mode = true;
        app.state.playlist.pos = 0;
        frame(&mut app, &mut c, None);
        assert_eq!(hit_rect(&app, &Action::SongPos), PANEL);
        let y = 62.0;
        let pos = |app: &App| app.state.playlist.pos;

        // A click beside the handle: one step (36 ticks); the release ends it.
        app.mouse_move(480.0, y);
        app.mouse_down();
        app.mouse_up();
        assert_eq!(pos(&app), 36);
        assert_eq!(app.hint, "Song position: 1:02:12");
        // Held: steps every repeat until the handle is under the pointer (1584 ticks), then it stays.
        app.mouse_down();
        assert_eq!(pos(&app), 72);
        let t0 = std::time::Instant::now();
        for i in 1..60 {
            app.song_pos_tick(t0 + std::time::Duration::from_millis(250 * i));
        }
        assert_eq!(pos(&app), 1584, "stopped under the pointer");
        assert!(app.animating(), "the hold ticks at animation rate");
        app.mouse_up();
        assert!(!matches!(app.drag, Some(Drag::SongPos { .. } | Drag::SongPosHold { .. })));
        // The handle drags (keeping the grab offset).
        frame(&mut app, &mut c, None);
        assert_eq!(handle_x(1584, 3168), 480.0);
        app.mouse_move(482.0, y);
        app.mouse_down();
        app.mouse_move(438.0, y);
        assert_eq!(pos(&app), 764);
        app.mouse_up();
        // Right click jumps to the pointer, clamped to the last tick.
        app.mouse_move(565.0, y);
        app.mouse_down_right();
        assert_eq!(pos(&app), 3167);
        // The wheel: up = earlier, down = later, 149 ticks a notch.
        frame(&mut app, &mut c, None);
        app.wheel(0.0, 1.0, false, false);
        assert_eq!(pos(&app), 3018);
        app.wheel(0.0, -2.0, false, false);
        assert_eq!(pos(&app), 3167);

        // PAT mode: through the (empty, one-bar) pattern; right-click at the middle.
        app.state.song_mode = false;
        frame(&mut app, &mut c, None);
        app.mouse_move(480.0, y);
        app.mouse_down_right();
        assert_eq!(app.state.piano.marker, 192);
        assert_eq!(app.hint, "Song position: 1:09:00");

        // SONG mode with an empty playlist: disabled.
        app.state.song_mode = true;
        app.state.project.arrangements[0].clips.clear();
        frame(&mut app, &mut c, None);
        assert!(app.hits.iter().all(|h| h.action != Action::SongPos));
        app.mouse_down_right();
        assert_eq!(pos(&app), 3167);
    }

    /// Playlist navigation and editing shortcuts (spec: ui/playlist.md §3, §3a, §4, §5, §6): Alt+wheel = track
    /// height (FL's 6 px steps and limits), Ctrl+wheel and the plain wheel over the ruler = time zoom, End = zoom on
    /// selection, Shift+G / Alt+G / Alt+U clip groups (lowest free number, move together, duplicates get their own
    /// group, ungroup deselects, saved), Right-Shift+drag slice in Draw, Shift+Tab snap cycle, Ctrl+Insert without a
    /// time selection, Ctrl+P / Ctrl+Shift+P switch performance mode, F5 raises, the view saved in the project (event 242).
    #[test]
    fn playlist_keybinds() {
        use crate::playlist::{self as pl, Snap};
        use winit::keyboard::{Key, NamedKey};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.file = None;
        app.state.path = None;
        app.state.playlist.reset();
        app.state.playlist.row_h = pl::ROW_H;
        app.state.z = vec![Win::Playlist];
        frame(&mut app, &mut c, None);
        let grid = app.pl_grid().expect("playlist grid");
        let g = pl::PGeom::new(&app.state, grid);
        let bar = 4 * u32::from(app.state.project.ppq);
        // Clicks land 10 px into a cell: Cell places clips at its start (Line would round to the nearest beat).
        app.state.playlist.snap = crate::snap::EditorSnap::Grid(pl::Snap::Cell);
        let at = |t: u32, track: u16| (g.x_of(t as f32) + 10.0, g.y_of(track) + 30.0);
        let drag = |app: &mut App, c: &mut Canvas, (x0, y0): (f32, f32), (x1, y1): (f32, f32)| {
            app.mouse_move(x0, y0);
            app.mouse_down();
            for i in 1..=10 {
                let k = i as f32 / 10.0;
                app.mouse_move(x0 + (x1 - x0) * k, y0 + (y1 - y0) * k);
            }
            app.mouse_up();
            frame(app, c, None);
        };
        let on_track = |app: &App, t: u16| -> Vec<os_model::Clip> {
            let mut v: Vec<os_model::Clip> = pl::clips(&app.state).iter().filter(|c| c.track == t).copied().collect();
            v.sort_by_key(|c| c.position);
            v
        };

        // Shift+Tab cycles Line, Cell, Beat, Bar, none; from Main (the default) it goes to Line.
        app.state.playlist.snap = crate::snap::EditorSnap::Main;
        app.shift = true;
        let mut seen = Vec::new();
        for _ in 0..6 {
            app.key(&Key::Named(NamedKey::Tab), false, false);
            seen.push(app.state.playlist.snap);
        }
        app.shift = false;
        let grid_of = crate::snap::EditorSnap::Grid;
        assert_eq!(seen, vec![grid_of(Snap::Line), grid_of(Snap::Cell), grid_of(Snap::Beat), grid_of(Snap::Bar), grid_of(Snap::None), grid_of(Snap::Line)]);
        app.state.playlist.snap = grid_of(Snap::Cell);

        // Two clips, grouped with Shift+G: they select and move as one.
        drag(&mut app, &mut c, at(0, 0), at(0, 0));
        drag(&mut app, &mut c, at(bar, 1), at(bar, 1));
        assert_eq!(pl::clips(&app.state).len(), 2);
        app.key(&Key::Character("a".into()), true, false);
        app.shift = true;
        app.key(&Key::Character("G".into()), false, false);
        app.shift = false;
        let groups: Vec<u16> = pl::clips(&app.state).iter().map(|c| c.group()).collect();
        assert!(groups[0] != 0 && groups[0] == groups[1], "{groups:?}");
        app.key(&Key::Character("d".into()), true, false);
        drag(&mut app, &mut c, at(0, 0), (g.x_of(2.0 * bar as f32) + 10.0, g.y_of(0) + 30.0));
        assert_eq!((on_track(&app, 0)[0].position, on_track(&app, 1)[0].position), (2 * bar, 3 * bar), "the group moved together");
        assert_eq!(app.state.playlist.sel.len(), 2, "clicking a grouped clip selects its group");
        // A duplicate is a group of its own; the copy keeps the shape.
        app.key(&Key::Character("b".into()), true, false);
        let (orig, copy) = (on_track(&app, 0)[0], on_track(&app, 0)[1]);
        assert_eq!((orig.group(), copy.group(), on_track(&app, 1)[1].group()), (1, 2, 2), "the copy takes the lowest free number");
        // The group survives saving.
        let path = std::env::temp_dir().join(format!("openstudio-pl-groups-{}.flp", std::process::id()));
        app.save_to(path.clone());
        let back = os_flp::project::load(&os_flp::FlpFile::parse(&std::fs::read(&path).unwrap()).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        let saved: Vec<u16> = back.arrangement().unwrap().clips.iter().map(|c| c.group()).collect();
        assert_eq!(saved.iter().filter(|&&g| g == orig.group()).count(), 2, "{saved:?}");
        // Alt+G and Alt+U take groups apart.
        app.key(&Key::Character("d".into()), true, false);
        app.state.playlist.last_clip = None; // not a double click
        drag(&mut app, &mut c, at(2 * bar, 0), at(2 * bar, 0));
        app.key(&Key::Character("g".into()), false, true);
        assert_eq!(on_track(&app, 0)[0].group(), 0);
        assert_eq!(on_track(&app, 1)[0].group(), 0);
        assert!(app.state.playlist.sel.is_empty(), "ungrouping deselects the clips, as in FL");
        // A new group takes the lowest number no clip uses: group 2 left, group 1 free again.
        app.key(&Key::Character("a".into()), true, false);
        let loose: Vec<usize> = (0..pl::clips(&app.state).len()).filter(|&i| pl::clips(&app.state)[i].group() == 0).collect();
        app.state.playlist.sel = loose;
        app.shift = true;
        app.key(&Key::Character("G".into()), false, false);
        app.shift = false;
        assert_eq!((on_track(&app, 0)[0].group(), on_track(&app, 1)[0].group()), (1, 1));
        app.key(&Key::Character("a".into()), true, false);
        app.key(&Key::Character("u".into()), false, true);
        assert!(pl::clips(&app.state).iter().all(|c| c.group() == 0));
        app.key(&Key::Character("d".into()), true, false);

        // End zooms on the selection (all clips when none is selected).
        app.state.playlist.sel = vec![pl::clips(&app.state).iter().position(|c| c.track == 0 && c.position == 2 * bar).unwrap()];
        app.key(&Key::Named(NamedKey::End), false, false);
        assert!((app.state.playlist_bar_w - 600.0f32.min(grid.w)).abs() < 0.01, "one bar fills the view: {}", app.state.playlist_bar_w);
        assert!((app.state.playlist_scroll.0 - 2.0 * app.state.playlist_bar_w).abs() < 0.01);
        assert_eq!(app.state.playlist.row_h, pl::ROW_MAX, "one selected track fills the view's height (up to the maximum)");
        app.state.playlist.row_h = pl::ROW_H;
        app.state.playlist_bar_w = crate::views::playlist::BAR_W;
        app.state.playlist_scroll = (0.0, 0.0);
        app.state.playlist.sel.clear();
        frame(&mut app, &mut c, None);

        // Right-Shift+drag across clips from empty grid (Draw tool) slices them; no clip is placed. The left Shift
        // does not start a slice (FL places a clip there).
        let g = pl::PGeom::new(&app.state, grid);
        app.state.project.arrangements[0].clips.push(os_model::Clip::new(0, 2 * bar, 4, os_model::ClipSource::Pattern(1)));
        let n = pl::clips(&app.state).len();
        app.shift = true;
        app.mouse_move(g.x_of(bar as f32), g.y_of(3) + 30.0);
        app.mouse_down();
        assert!(!matches!(app.state.playlist.overlay, Some(pl::Overlay::Slice { .. })), "left Shift: no slice");
        app.mouse_up();
        // Shift let go first: Ctrl+Shift+Z would redo.
        app.shift = false;
        app.key(&Key::Character("z".into()), true, false);
        assert_eq!(pl::clips(&app.state).len(), n);
        app.shift = true;
        app.rshift = true;
        drag(&mut app, &mut c, (g.x_of(bar as f32), g.y_of(3) + 30.0), (g.x_of(bar as f32), g.y_of(5) + 30.0));
        app.shift = false;
        app.rshift = false;
        assert_eq!(pl::clips(&app.state).len(), n + 1);
        assert_eq!(on_track(&app, 4).iter().map(|c| (c.position, c.length)).collect::<Vec<_>>(), vec![(0, bar), (bar, bar)]);

        // Alt+wheel (headers, ruler or grid) changes the track height by 6 px a notch, 16..280 px; Ctrl+wheel over the
        // headers and the plain wheel over the ruler zoom time.
        let header = hit_rect(&app, &Action::PlHeader(2));
        app.mouse_move(header.cx(), header.cy());
        let w0 = app.state.playlist_bar_w;
        app.alt = true;
        app.wheel(0.0, 2.0, false, false);
        let h = app.state.playlist.row_h;
        assert_eq!((h, app.state.playlist_bar_w), (pl::ROW_H + 12.0, w0), "taller tracks, same zoom");
        frame(&mut app, &mut c, None);
        let g = pl::PGeom::new(&app.state, grid);
        assert_eq!(g.rows.top(3), 3.0 * h);
        assert_eq!(g.clip_rect(&on_track(&app, 4)[0]).h, h - 3.0);
        assert!(hit_rect(&app, &Action::PlLed(0)).bottom() <= g.y_of(0) + h, "the LED stays inside its row");
        for _ in 0..60 {
            app.wheel(0.0, -1.0, false, false);
        }
        assert_eq!(app.state.playlist.row_h, pl::ROW_MIN);
        frame(&mut app, &mut c, None);
        let g = pl::PGeom::new(&app.state, grid);
        assert!(hit_rect(&app, &Action::PlLed(0)).bottom() <= g.y_of(0) + pl::ROW_MIN, "the LED fits the smallest row");
        app.state.playlist_scroll = (0.0, 0.0);
        frame(&mut app, &mut c, None);
        let grid_pt = (g.x_of(20.0 * bar as f32), g.y_of(2) + 5.0);
        app.mouse_move(grid_pt.0, grid_pt.1);
        for _ in 0..60 {
            app.wheel(0.0, 1.0, false, false);
        }
        app.alt = false;
        assert_eq!(app.state.playlist.row_h, pl::ROW_MAX, "Alt+wheel over the grid, up to the maximum");
        app.state.playlist.row_h = pl::ROW_H;
        app.state.playlist_scroll = (0.0, 0.0);
        frame(&mut app, &mut c, None);
        let header = hit_rect(&app, &Action::PlHeader(2));
        app.mouse_move(header.cx(), header.cy());
        app.wheel(0.0, 1.0, false, true);
        assert!(app.state.playlist_bar_w > w0 && app.state.playlist.row_h == pl::ROW_H, "Ctrl+wheel over the headers zooms time");
        app.state.playlist_bar_w = w0;
        app.state.playlist_scroll = (0.0, 0.0);
        frame(&mut app, &mut c, None);
        let ruler = hit_rect(&app, &Action::PlRuler);
        app.mouse_move(ruler.cx(), ruler.cy());
        app.wheel(0.0, 1.0, false, false);
        assert!(app.state.playlist_bar_w > w0, "the plain wheel over the ruler zooms in");
        app.alt = true;
        let w1 = app.state.playlist_bar_w;
        app.wheel(0.0, 1.0, false, false);
        app.alt = false;
        assert_eq!((app.state.playlist_bar_w, app.state.playlist.row_h), (w1, pl::ROW_H + 6.0), "Alt+wheel over the ruler: track height");
        app.state.playlist.row_h = pl::ROW_H;
        app.state.playlist_bar_w = w0;
        app.state.playlist_scroll = (0.0, 0.0);

        // Ctrl+Insert without a time selection inserts one bar at the song position.
        app.state.playlist.range = None;
        app.state.playlist.pos = bar;
        let before: Vec<u32> = on_track(&app, 4).iter().map(|c| c.position).collect();
        app.key(&Key::Named(NamedKey::Insert), true, false);
        let after: Vec<u32> = on_track(&app, 4).iter().map(|c| c.position).collect();
        assert_eq!((before, after), (vec![0, bar], vec![0, 2 * bar]));

        // Ctrl+P and Ctrl+Shift+P switch performance mode (spec: ui/performance-mode.md §1).
        app.key(&Key::Character("p".into()), true, false);
        assert_eq!((app.state.message.as_deref(), app.state.project.performance), (Some("Performance mode: on"), Some(true)));
        app.shift = true;
        app.key(&Key::Character("P".into()), true, false);
        app.shift = false;
        assert_eq!((app.state.message.as_deref(), app.state.project.performance), (Some("Performance mode: off"), Some(false)));

        // The track height, time zoom and scroll are saved with the project and come back when it opens.
        app.state.playlist.row_h = 60.0;
        app.state.playlist_bar_w = 90.0;
        app.state.playlist_scroll = (180.0, 24.0);
        let path = std::env::temp_dir().join(format!("openstudio-pl-view-{}.flp", std::process::id()));
        app.save_to(path.clone());
        let back = os_flp::project::load(&os_flp::FlpFile::parse(&std::fs::read(&path).unwrap()).unwrap()).unwrap();
        let v = back.playlist_view.expect("view saved");
        assert_eq!((v.row_px(), v.height(), v.scroll_y()), (60, 22, 24));
        assert!((v.ticks_per_px() - f64::from(bar) / 90.0).abs() < 1e-9 && (v.left() - 180.0 * f64::from(bar) / 90.0).abs() < 1e-6);
        app.state.playlist.row_h = pl::ROW_H;
        app.state.playlist_bar_w = w0;
        app.state.playlist_scroll = (0.0, 0.0);
        app.open(path.clone());
        let _ = std::fs::remove_file(&path);
        assert_eq!(app.state.playlist.row_h, 60.0);
        assert!((app.state.playlist_bar_w - 90.0).abs() < 1e-3 && (app.state.playlist_scroll.0 - 180.0).abs() < 1e-2);
        assert_eq!(app.state.playlist_scroll.1, 24.0);
        app.state.z = vec![Win::Playlist];

        // F5 brings a covered playlist to the front.
        app.state.z = vec![Win::Playlist, Win::Mixer];
        app.key(&Key::Named(NamedKey::F5), false, false);
        assert_eq!(app.state.z.last(), Some(&Win::Playlist));
        assert!(app.state.shown(Win::Playlist));
    }

    /// Look check for playlist states (markers, time selection, selected / muted clips, automation points,
    /// options menu with a submenu): `OS_SHOT_DIR=dir cargo test -p os-app playlist_shots -- --ignored`.
    #[test]
    #[ignore]
    fn playlist_shots() {
        use crate::playlist as pl;
        let Some(dir) = std::env::var_os("OS_SHOT_DIR") else { return };
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.playlist.reset();
        app.state.z = vec![Win::Playlist];
        app.state.project.patterns[0].notes = (0..16).map(|i| Note { length: 24, key: 60 + (i % 5) as u16 * 2, ..Note::step(0, i * 48) }).collect();
        let bar = 4 * u32::from(app.state.project.ppq);
        {
            let a = pl::arr_mut(&mut app.state);
            for k in 0..4 {
                a.clips.push(os_model::Clip::new(k * 2 * bar, 2 * bar, 0, os_model::ClipSource::Pattern(1)));
            }
            a.clips[1].muted = true;
            a.markers.push(os_model::TimeMarker::new(2 * bar, "Drop"));
            a.tracks[1].color = Some(os_model::Rgb::new(0x90, 0x58, 0x87));
            a.tracks[1].name = Some("Vocals".into());
        }
        app.state.playlist.sel = vec![2];
        app.state.playlist.range = Some((bar, 3 * bar));
        pl::run(&mut app, pl::Cmd::CreateAutomation(os_model::target::TEMPO, "Tempo".into(), 0.3));
        let ch = app.state.project.automations.last().unwrap().channel;
        pl::add_point(&mut app.state, ch, 6.0, 0.9);
        pl::add_point(&mut app.state, ch, 11.0, 0.1);
        app.state.playlist.range = None;
        frame(&mut app, &mut c, None);
        app.state.playlist.hover_clip = pl::clips(&app.state).iter().position(|c| c.source == os_model::ClipSource::Channel(ch));
        let mut popup = pl::options_menu(&app.state, 220.0, 105.0);
        popup.sub = Some(0);
        app.state.playlist.popup = Some(popup);
        frame(&mut app, &mut c, None);
        let dir = std::path::PathBuf::from(dir);
        c.pixmap.save_png(dir.join("pl_states.png")).unwrap();
        // Track heights (Alt+wheel): tall and the minimum.
        app.state.playlist.popup = None;
        for (h, name) in [(96.0, "pl_rows_tall.png"), (pl::ROW_MIN, "pl_rows_short.png")] {
            app.state.playlist.row_h = h;
            frame(&mut app, &mut c, None);
            c.pixmap.save_png(dir.join(name)).unwrap();
        }
        // The picker's three lists with tall items (a right-drag on its scroll bar, ui/scroll-bars.md §3), in both
        // designs. The audio clip's waveform is made up.
        app.state.playlist.row_h = pl::ROW_H;
        app.add_clip_channel(ChannelKind::AudioClip);
        let audio = app.state.project.channels.iter_mut().rfind(|c| c.kind == ChannelKind::AudioClip).unwrap();
        audio.sample_path = Some("shot.wav".into());
        let peaks = (0..400).map(|i| (i as f32 / 9.0).sin().abs() * (1.0 - i as f32 / 400.0)).map(|a| (-a, a)).collect();
        let wave = pl::Wave { rate: 44100, frames: 400 * 64, per: 64, peaks };
        app.state.playlist.waves.borrow_mut().insert("shot.wav".into(), Some(std::rc::Rc::new(wave)));
        for design in [Design::Citrus, Design::Lemon] {
            app.set_design(design, false);
            for (kind, list) in [(pl::PickKind::Pattern, "patterns"), (pl::PickKind::Audio, "audio"), (pl::PickKind::Automation, "automation")] {
                app.state.playlist.picker = kind;
                app.state.playlist.pick_rows[kind as usize] = 90.0;
                frame(&mut app, &mut c, None);
                c.pixmap.save_png(dir.join(format!("pl_pick_{list}_{}.png", format!("{design:?}").to_lowercase()))).unwrap();
            }
        }
    }

    /// Look check for the piano roll's vertical zoom, time markers, Tools menu and snap menus (spec: ui/piano-roll.md
    /// §3a, §10a, §11; ui/piano-snap.md): `OS_SHOT_DIR=dir cargo test -p os-app piano_shots -- --ignored`.
    #[test]
    #[ignore]
    fn piano_shots() {
        let Some(dir) = std::env::var_os("OS_SHOT_DIR") else { return };
        let dir = std::path::PathBuf::from(dir);
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.z = vec![Win::PianoRoll];
        app.state.selected_channel = 0;
        app.state.project.patterns[0].notes = (0..16).map(|i| Note { length: 24, key: 60 + (i % 5) as u16 * 2, ..Note::step(0, i * 48) }).collect();
        app.state.project.patterns[0].notes.push(Note { length: 96, key: 55, ..Note::step(1, 96) });
        crate::piano_tools::add_marker(&mut app.state, 384);
        frame(&mut app, &mut c, None);
        let grid = app.piano_grid.unwrap();
        piano::center_keys(&mut app.state, grid.h);
        for (name, row) in [("piano_row18.png", 18.0), ("piano_row30.png", 30.0), ("piano_row8.png", 8.0)] {
            let y = grid.cy();
            piano::set_rows(&mut app.state, grid, y, row);
            app.state.piano.row_h = row;
            frame(&mut app, &mut c, None);
            c.pixmap.save_png(dir.join(name)).unwrap();
        }
        app.state.piano.row_h = views::piano::ROW;
        piano::center_keys(&mut app.state, grid.h);
        let mut popup = crate::piano_tools::tools_menu(&app.state, 240.0, 120.0);
        popup.sub = popup.items.iter().position(|i| i.label == "Strum");
        app.state.playlist.popup = Some(popup);
        frame(&mut app, &mut c, None);
        c.pixmap.save_png(dir.join("piano_tools_menu.png")).unwrap();
        // The magnet's two menus, opened as a click and a right-click open them.
        app.state.playlist.popup = None;
        app.state.piano.snap = crate::snap::EditorSnap::Grid(crate::playlist::Snap::ThirdBeat);
        frame(&mut app, &mut c, None);
        let b = hit_rect(&app, &Action::PianoSnapMenu);
        app.mouse_move(b.cx(), b.cy());
        app.mouse_down();
        app.mouse_up();
        frame(&mut app, &mut c, None);
        c.pixmap.save_png(dir.join("piano_snap_menu.png")).unwrap();
        app.state.playlist.popup = None;
        frame(&mut app, &mut c, None);
        app.mouse_move(b.cx(), b.cy());
        app.mouse_down_right();
        app.mouse_up();
        frame(&mut app, &mut c, None);
        c.pixmap.save_png(dir.join("piano_snap_lengths.png")).unwrap();
        // The title's target channel, hovered with its menu open (ui/piano-target-channel.md).
        app.state.playlist.popup = None;
        frame(&mut app, &mut c, None);
        let b = hit_rect(&app, &Action::PianoTarget);
        click(&mut app, &mut c, b);
        c.pixmap.save_png(dir.join("piano_target_menu.png")).unwrap();
        // The toolbar's snap panel (ui/global-snap.md): its menu open, the longest label, snapping off.
        app.state.playlist.popup = None;
        frame(&mut app, &mut c, None);
        let combo = hit_rect(&app, &Action::GlobalSnap);
        click(&mut app, &mut c, combo);
        c.pixmap.save_png(dir.join("global_snap_menu.png")).unwrap();
        app.state.playlist.popup = None;
        app.state.global_snap.set(crate::playlist::Snap::SixthStep);
        frame(&mut app, &mut c, None);
        c.pixmap.save_png(dir.join("global_snap_sixth.png")).unwrap();
        app.state.global_snap.toggle();
        frame(&mut app, &mut c, None);
        c.pixmap.save_png(dir.join("global_snap_none.png")).unwrap();
    }

    /// Mixer look with routing, a sidechain, slots, meters and the menus: `OS_SHOT_DIR=dir cargo test -p os-app
    /// mixer_shots -- --ignored` writes mx_states.png, mx_menu.png and mx_slot_menu.png.
    #[test]
    #[ignore]
    fn mixer_shots() {
        use crate::mixer as mx;
        let Some(dir) = std::env::var_os("OS_SHOT_DIR") else { return };
        let dir = std::path::PathBuf::from(dir);
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.z = vec![Win::Mixer];
        app.state.show_browser = false;
        app.plugins.effects = vec![crate::plugin_db::fake("Test Delay", os_host::Kind::Effect)];
        app.state.plugdb.rebuild(&app.plugins);
        mx::select(&mut app.state, 3, false);
        mx::route(&mut app.state, 5, false, false);
        mx::route(&mut app.state, 7, false, true);
        app.state.project.mixer[4].polarity = true;
        app.state.project.mixer[6].swap = true;
        app.state.project.mixer[2].enabled = false;
        app.state.project.mixer[3].name = Some("Drums".into());
        app.state.project.mixer[3].slots[0] = Some(os_model::SlotPlugin { id: os_model::Id::NONE, name: "Test Delay".into(), state: Vec::new() });
        app.state.project.mixer[3].slot_on[1] = false;
        mx::select(&mut app.state, 6, true);
        mx::select(&mut app.state, 3, true);
        app.state.mixer.meters = (0..18)
            .map(|i| {
                let db = [crate::meters::to_db(0.12 * i as f32), crate::meters::to_db(0.1 * i as f32)];
                crate::meters::Meter { db, hold: db.map(|d| if d > crate::meters::SILENT { d + 3.0 } else { d }), held: [0.0; 2] }
            })
            .collect();
        frame(&mut app, &mut c, None);
        c.pixmap.save_png(dir.join("mx_states.png")).unwrap();
        app.state.playlist.popup = Some(mx::track_menu(&app.state, 3, 500.0, 200.0));
        frame(&mut app, &mut c, None);
        c.pixmap.save_png(dir.join("mx_menu.png")).unwrap();
        app.state.playlist.popup = Some(mx::slot_menu(&app, 0, 1500.0, 150.0));
        frame(&mut app, &mut c, None);
        c.pixmap.save_png(dir.join("mx_slot_menu.png")).unwrap();
    }

    /// F8 and the toolbar's plugin button open the menu of every plugin: an effect goes into the selected mixer track's
    /// first empty slot, a generator to the bottom of the channel rack; F8 again closes the menu (spec:
    /// ui/plugin-menu.md §1).
    #[test]
    fn plugin_menu_adds_plugins_where_they_belong() {
        use crate::playlist::Cmd;
        use crate::plugin_menu::PlugCmd;
        use winit::keyboard::{Key, NamedKey};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.plugins = crate::plugins::Plugins::builtins();
        app.plugins.effects.push(crate::plugin_db::fake("Test FX", os_host::Kind::Effect));
        app.state.plugdb.rebuild(&app.plugins);
        frame(&mut app, &mut c, None);
        let f8 = Key::Named(NamedKey::F8);
        let pick = |app: &App, label: &str| app.state.playlist.popup.as_ref().and_then(|p| p.items.iter().find(|i| i.label == label)).map(|i| i.cmd.clone());
        app.key(&f8, false, false);
        {
            let p = app.state.playlist.popup.as_ref().expect("F8 opens the plugin menu");
            assert!(p.items.iter().any(|i| i.head && i.label == "Generators"));
            assert!(p.items.iter().any(|i| i.head && i.label == "Effects"));
        }
        // An effect: the selected track's first empty slot.
        crate::mixer::select(&mut app.state, 2, false);
        app.state.project.mixer[2].slots[0] = Some(os_model::SlotPlugin { id: os_model::Id::NONE, name: "Taken".into(), state: Vec::new() });
        let fx = pick(&app, "Test FX").expect("the effects are listed");
        let index = app.plugins.effects.len() - 1;
        assert_eq!(fx, Cmd::Plugin(PlugCmd::Effect { index, slot: None }));
        crate::playlist::run(&mut app, fx);
        assert!(app.state.playlist.popup.is_none());
        assert_eq!(crate::mixer::slot_name(&app.state, 2, 1).as_deref(), Some("Test FX"));
        // F8 twice: open, then closed.
        app.key(&f8, false, false);
        assert!(crate::plugin_menu::is_open(&app.state));
        app.key(&f8, false, false);
        assert!(app.state.playlist.popup.is_none());
        // The toolbar button opens it under the button; a generator goes to the bottom of the rack and is selected.
        frame(&mut app, &mut c, None);
        let b = hit_rect(&app, &Action::PickerOpen);
        click(&mut app, &mut c, b);
        let p = app.state.playlist.popup.as_ref().expect("the toolbar button opens the plugin menu");
        assert!(p.y >= b.bottom(), "under the button");
        let n = app.state.project.channels.len();
        let osc = pick(&app, crate::osc3::LEMON_TRIO).expect("the generators are listed");
        crate::playlist::run(&mut app, osc);
        assert_eq!(app.state.project.channels.len(), n + 1);
        assert_eq!(app.state.selected_channel, n);
        assert_eq!(app.state.pending_plugin, Some((n, app.plugins.index_of(crate::osc3::LEMON_TRIO).unwrap())));
        // An empty mixer slot's menu lists the effects for that slot.
        let m = crate::mixer::effect_menu(&app.state, 4, 0.0, 0.0);
        assert!(m.items.iter().any(|i| i.cmd == Cmd::Plugin(PlugCmd::Effect { index, slot: Some(4) })));
    }

    /// The menu bar dropdown's ▸ entries (spec: ui/menus.md "Look"): a click opens the submenu, the app's popup, to the
    /// right of the row over the dropdown, which stays open; a click on the entry again closes it, one elsewhere closes
    /// both; a pick closes both. TOOLS > Change declicking for selected channels to ▸ sets the mode of every selected
    /// channel, in the channel and its Lemon Sampler, for playback (spec: ui/channel-rack.md §17, audio/declick.md).
    #[test]
    fn dropdown_submenu_changes_declicking_of_selected_channels() {
        use views::menus::{DECLICK_LABEL, TOOLS_MENU};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.plugins = crate::plugins::Plugins::default();
        app.plugins.available = crate::plugins::builtin();
        app.sync_samplers(None);
        app.mark_saved();
        assert!(app.state.hosted_rows.contains(&1), "Lemon Sampler plays the sampler channels");
        app.state.rack.sel = [1, 3].into();
        let entry = views::menus::MENUS[TOOLS_MENU].cols.iter().flat_map(|(_, items)| items.iter()).position(|it| it.label == DECLICK_LABEL).unwrap();
        frame(&mut app, &mut c, None);
        let tools = hit_rect(&app, &Action::Menu(TOOLS_MENU));
        click(&mut app, &mut c, tools);
        let row = hit_rect(&app, &Action::MenuSub(TOOLS_MENU, entry));
        click(&mut app, &mut c, row);
        let p = app.state.playlist.popup.clone().expect("the submenu opens");
        assert_eq!((app.state.menu_open, app.state.menu_sub), (Some(TOOLS_MENU), Some(entry)), "over the open dropdown");
        assert_eq!((p.x, p.y), (row.right(), row.y - 2.0), "right of its row");
        assert_eq!(p.items, crate::rack::declick_items(&app.state));
        // The entry again closes the submenu only; a click elsewhere closes both.
        click(&mut app, &mut c, row);
        assert!(app.state.playlist.popup.is_none() && app.state.menu_open == Some(TOOLS_MENU));
        click(&mut app, &mut c, row);
        assert!(app.state.playlist.popup.is_some());
        click(&mut app, &mut c, Rect::new(1500.0, 900.0, 4.0, 4.0));
        assert!(app.state.playlist.popup.is_none() && app.state.menu_open.is_none());
        // Generic (bleeding) = mode 3 on the selected channels only.
        click(&mut app, &mut c, tools);
        click(&mut app, &mut c, row);
        let item = hit_rect(&app, &Action::PlPopup(3));
        assert!(app.top_hit(item.cx(), item.cy()).is_some_and(|h| h.action == Action::PlPopup(3)), "the submenu is over the dropdown");
        app.state.piano.dirty = false;
        click(&mut app, &mut c, item);
        assert!(app.state.playlist.popup.is_none() && app.state.menu_open.is_none(), "a pick closes both");
        let modes: Vec<u8> = app.state.project.channels.iter().map(|ch| ch.sound.declick).collect();
        assert_eq!(modes, [0, 3, 0, 3, 0]);
        assert!(app.unsaved() && app.state.piano.dirty, "a change to the project, played at once");
        // Lemon Sampler plays the new mode (and a save keeps it): the change reaches it on the next UI tick.
        app.plugin_tick();
        let mut ch = app.state.project.channels[1].clone();
        ch.sound.declick = 0;
        crate::plugins::store_sampler(&mut ch, &app.plugins.state(1).unwrap());
        assert_eq!(ch.sound.declick, 3);
        // Without a selection the modes are greyed out and change nothing.
        app.state.rack.sel.clear();
        assert!(crate::rack::declick_items(&app.state).iter().all(|i| !i.enabled));
        click(&mut app, &mut c, tools);
        click(&mut app, &mut c, row);
        let item = hit_rect(&app, &Action::PlPopup(5));
        click(&mut app, &mut c, item);
        assert_eq!(app.state.project.channels[1].sound.declick, 3);
        // The channel rack's options menu has the same submenu.
        app.state.rack.sel = [0].into();
        let rack = crate::rack::options_menu(&app.state, 0.0, 0.0);
        let sub = rack.items.iter().find(|i| i.label == DECLICK_LABEL).expect("in the rack's options menu");
        assert_eq!(sub.children, crate::rack::declick_items(&app.state));
        crate::playlist::run(&mut app, sub.children[4].cmd.clone());
        assert_eq!(app.state.project.channels[0].sound.declick, 4);
    }

    /// TOOLS / the channel rack's options > Switch all plugin editors to ▸ (spec: ui/channel-rack.md §17, ui/plugin-window.md
    /// "Docking"): every hosted instance's window, the channels' and the mixer effects', docked or detached, kept for a
    /// window that is closed.
    #[cfg(plugin_windows)]
    #[test]
    fn switch_all_plugin_editors() {
        use views::menus::{submenu, EDITORS_LABEL, TOOLS_MENU};
        let mut app = App::new(None);
        app.plugins = crate::plugins::Plugins::default();
        app.plugins.available = crate::plugins::builtin();
        app.plugins.effects = crate::plugins::builtin_effects();
        app.sync_samplers(None);
        app.plugins.create_effect(1, 0, 0, None).expect("an effect in a mixer slot");
        assert!(app.plugins.hosted.len() >= 2 && app.plugins.hosted.values().all(|h| !h.detached));
        let items = submenu(TOOLS_MENU, EDITORS_LABEL).unwrap()(&app.state);
        let rack = crate::rack::options_menu(&app.state, 0.0, 0.0);
        assert_eq!(rack.items.iter().find(|i| i.label == EDITORS_LABEL).map(|i| &i.children), Some(&items), "the rack's is the same");
        crate::playlist::run(&mut app, items[1].cmd.clone());
        assert!(app.plugins.hosted.values().all(|h| h.detached), "Detached");
        crate::playlist::run(&mut app, items[0].cmd.clone());
        assert!(app.plugins.hosted.values().all(|h| !h.detached), "Attached");
    }

    /// ADD > Effect ▸: the effect plugin menu as a submenu; a pick goes into the selected mixer track's first empty
    /// slot as from F8, and a full track takes nothing (spec: ui/plugin-menu.md §1).
    #[test]
    fn add_effect_submenu_fills_the_selected_track() {
        use crate::playlist::Cmd;
        use crate::plugin_menu::PlugCmd;
        use views::menus::ADD_MENU;
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.plugins = crate::plugins::Plugins::builtins();
        app.plugins.effects.push(crate::plugin_db::fake("Test FX", os_host::Kind::Effect));
        app.state.plugdb.rebuild(&app.plugins);
        crate::mixer::select(&mut app.state, 2, false);
        let index = app.plugins.effects.len() - 1;
        let entry = views::menus::MENUS[ADD_MENU].cols.iter().flat_map(|(_, items)| items.iter()).position(|it| it.label == "Effect").unwrap();
        let pick = |app: &mut App, c: &mut Canvas| {
            frame(app, c, None);
            let add = hit_rect(app, &Action::Menu(ADD_MENU));
            click(app, c, add);
            let row = hit_rect(app, &Action::MenuSub(ADD_MENU, entry));
            click(app, c, row);
            let p = app.state.playlist.popup.clone().expect("the effect menu opens");
            assert!(!p.items.iter().any(|i| i.label == "Generators"), "effects only");
            assert!(p.items.iter().any(|i| i.label == "Manage plugins..."));
            let i = p.items.iter().position(|i| i.cmd == Cmd::Plugin(PlugCmd::Effect { index, slot: None })).expect("Test FX is listed");
            let item = hit_rect(app, &Action::PlPopup(i));
            click(app, c, item);
        };
        app.state.project.mixer[2].slots[0] = Some(os_model::SlotPlugin { id: os_model::Id::NONE, name: "Taken".into(), state: Vec::new() });
        pick(&mut app, &mut c);
        assert_eq!(crate::mixer::slot_name(&app.state, 2, 1).as_deref(), Some("Test FX"));
        assert!(app.state.menu_open.is_none() && app.state.playlist.popup.is_none());
        // Every slot in use: nothing is added.
        for slot in &mut app.state.project.mixer[2].slots {
            slot.get_or_insert(os_model::SlotPlugin { id: os_model::Id::NONE, name: "Taken".into(), state: Vec::new() });
        }
        let before = app.state.project.mixer[2].slots.clone();
        pick(&mut app, &mut c);
        assert_eq!(app.state.project.mixer[2].slots, before);
        assert_eq!(app.state.message.as_deref(), Some("Every slot of this track is in use"));
    }

    /// The menu bar's ways into the browser (spec: ui/browser-commands.md): VIEW's Browser entries show the browser on
    /// the Current project tab with only their section open, and selected; Plugin database (ADD > Browse plugin
    /// database, Shift+F8) shows that tab as it was left; Browser find (Ctrl+F) focuses the search field as a click
    /// does. Text fields keep their keys.
    #[test]
    fn browser_commands_jump_into_the_browser() {
        use crate::views::browser::{project_root, rows, section_key, PLUGIN_TAB, PROJECT_TAB};
        use views::menus::{item_at, works, ADD_MENU, MENUS, TOOLS_MENU, VIEW};
        use winit::keyboard::{Key, NamedKey};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        let index = |m: usize, label: &str| MENUS[m].cols.iter().flat_map(|(_, items)| items.iter()).position(|it| it.label == label).unwrap();
        let (gens, pats, fx) = (section_key("Generators").unwrap(), section_key("Patterns").unwrap(), section_key("Effects").unwrap());
        let s = &mut app.state;
        s.show_browser = false;
        s.browser_tab = 0;
        s.browser_search = "kick".into();
        s.browser_open.insert(gens.clone());
        s.browser_open.remove(&project_root());
        app.menu_item(VIEW, index(VIEW, "Patterns"));
        let s = &app.state;
        assert!(s.show_browser && s.browser_tab == PROJECT_TAB && s.browser_search.is_empty());
        assert_eq!(s.browser_sel.as_ref(), Some(&pats));
        assert!(s.browser_open.contains(&project_root()) && s.browser_open.contains(&pats) && !s.browser_open.contains(&gens));
        app.menu_item(VIEW, index(VIEW, "Effects in use"));
        let open: Vec<(String, Option<bool>)> = rows(&app.state).into_iter().filter(|r| r.depth == 1).map(|r| (r.label, r.open)).collect();
        assert_eq!(open, [("History".into(), Some(false)), ("Patterns".into(), Some(false)), ("Effects".into(), Some(true)), ("Generators".into(), Some(false)), ("Remote control".into(), Some(false)), ("Samples".into(), Some(false))]);
        assert_eq!(app.state.browser_sel.as_ref(), Some(&fx));
        app.menu_item(VIEW, index(VIEW, "Instruments in use"));
        assert_eq!(app.state.browser_sel.as_ref(), Some(&gens));
        // The Plugin database tab as it was left.
        let db_fx = format!("{PLUGIN_TAB}:Plugin library/Effects");
        app.state.browser_open.insert(db_fx.clone());
        app.state.show_browser = false;
        app.menu_item(VIEW, index(VIEW, "Plugin library"));
        let s = &app.state;
        assert!(s.show_browser && s.browser_tab == PLUGIN_TAB && s.browser_open.contains(&db_fx) && s.browser_open.contains(&gens));
        app.state.browser_tab = 0;
        app.menu_item(ADD_MENU, index(ADD_MENU, "Browse plugin library"));
        assert_eq!(app.state.browser_tab, PLUGIN_TAB);
        // Shift+F8 (not the plugin menu F8 opens), but not while the search field has the keyboard.
        app.state.browser_tab = 0;
        app.shift = true;
        app.key(&Key::Named(NamedKey::F8), false, false);
        assert!(app.state.browser_tab == PLUGIN_TAB && app.state.playlist.popup.is_none());
        app.state.browser_tab = 0;
        app.state.browser_search_focus = true;
        app.key(&Key::Named(NamedKey::F8), false, false);
        assert_eq!(app.state.browser_tab, 0);
        app.shift = false;
        app.state.browser_search_focus = false;
        // Browser find, clicked in the dropdown with the browser hidden: typing then searches.
        app.state.show_browser = false;
        frame(&mut app, &mut c, None);
        let tools = hit_rect(&app, &Action::Menu(TOOLS_MENU));
        click(&mut app, &mut c, tools);
        let find = hit_rect(&app, &Action::MenuItem(TOOLS_MENU, index(TOOLS_MENU, "Browser find...")));
        click(&mut app, &mut c, find);
        assert!(app.state.show_browser && app.state.browser_search_focus && app.state.menu_open.is_none());
        app.key(&Key::Character("k".into()), false, false);
        assert_eq!(app.state.browser_search, "k");
        // Ctrl+F does the same; a name box keeps the keys.
        app.key(&Key::Named(NamedKey::Escape), false, false);
        app.state.show_browser = false;
        app.key(&Key::Character("f".into()), true, false);
        assert!(app.state.show_browser && app.state.browser_search_focus);
        app.state.browser_search_focus = false;
        crate::patterns::run(&mut app.state, crate::patterns::Cmd::Rename, (0.0, 0.0));
        app.key(&Key::Character("f".into()), true, false);
        assert!(!app.state.browser_search_focus);
        // Undo history works: the tree lists History.
        assert!(works(VIEW, item_at(VIEW, index(VIEW, "Undo history")).unwrap()));
        // ADD > Browse all installed plugins: the Plugin database's Installed opened and selected; Browse presets: the
        // All tab's Plugin presets (spec: ui/browser-commands.md §2).
        app.state.show_browser = false;
        app.menu_item(ADD_MENU, index(ADD_MENU, "Browse all installed plugins"));
        let installed = format!("{PLUGIN_TAB}:Plugin library/Installed");
        let s = &app.state;
        assert!(s.show_browser && s.browser_tab == PLUGIN_TAB && s.browser_open.contains(&installed) && s.browser_sel.as_ref() == Some(&installed));
        app.menu_item(ADD_MENU, index(ADD_MENU, "Browse presets"));
        let presets = crate::views::browser::presets_key();
        let s = &app.state;
        assert!(s.browser_tab == 0 && s.browser_open.contains(&presets) && s.browser_sel.as_ref() == Some(&presets));
    }

    /// PATTERNS > Open in project browser (spec: ui/browser-commands.md §4-5): the browser shown on the Current project
    /// tab with Patterns opened and the current pattern's row selected (the section itself when the pattern is not
    /// listed); other open sections stay open. Shift+Ctrl+P and the picker's pattern menu do the same.
    #[test]
    fn open_pattern_in_project_browser() {
        use crate::playlist::{self as pl, Cmd};
        use crate::views::browser::{project_root, rows, section_key, Item, PROJECT_TAB};
        use views::menus::{item_at, works, MENUS, PATTERNS};
        use winit::keyboard::Key;
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.project.patterns.push(os_model::Pattern { number: 2, name: Some("Drums".into()), ..Default::default() });
        app.state.pattern = 2;
        let (pats, fx) = (section_key("Patterns").unwrap(), section_key("Effects").unwrap());
        let reset = |app: &mut App| {
            let s = &mut app.state;
            s.show_browser = false;
            s.browser_tab = 0;
            s.browser_search = "kick".into();
            s.browser_sel = None;
            s.browser_open.remove(&pats);
            s.browser_open.remove(&project_root());
            s.browser_open.insert(fx.clone());
        };
        let entry = MENUS[PATTERNS].cols.iter().flat_map(|(_, items)| items.iter()).position(|it| it.label == "Open in project browser").unwrap();
        assert!(works(PATTERNS, item_at(PATTERNS, entry).unwrap()));
        let check = |app: &App, want: &str| {
            let s = &app.state;
            assert!(s.show_browser && s.browser_tab == PROJECT_TAB && s.browser_search.is_empty());
            assert!(s.browser_open.contains(&project_root()) && s.browser_open.contains(&pats) && s.browser_open.contains(&fx));
            assert_eq!(s.browser_sel.as_deref(), Some(want));
        };
        let drums = |app: &App| rows(&app.state).into_iter().find(|r| r.item == Item::Pattern(2)).map(|r| r.key);
        reset(&mut app);
        app.menu_item(PATTERNS, entry);
        let drums = drums(&app).expect("pattern 2 listed");
        check(&app, &drums);
        // A pattern the section does not list: the section is selected.
        reset(&mut app);
        app.state.pattern = 7;
        app.menu_item(PATTERNS, entry);
        check(&app, &pats);
        // Shift+Ctrl+P (the playlist in front keeps it for Performance mode, ui/playlist.md).
        app.state.pattern = 2;
        reset(&mut app);
        app.state.z.retain(|&w| w != Win::Playlist);
        app.shift = true;
        app.key(&Key::Character("P".into()), true, false);
        app.shift = false;
        check(&app, &drums);
        // The picker's pattern menu.
        let popup = views::menus::pattern_popup(0.0, 0.0);
        let item = popup.items.iter().find(|i| i.label == "Open in project browser").expect("in the pattern menu");
        assert!(item.enabled && item.cmd == Cmd::Pattern(crate::patterns::Cmd::OpenInBrowser));
        reset(&mut app);
        pl::run(&mut app, Cmd::Pattern(crate::patterns::Cmd::OpenInBrowser));
        check(&app, &drums);
    }

    /// Toolbar utility and window buttons (spec: ui/toolbar-buttons.md §2): "left / right" hints with the key; Undo,
    /// Save as..., About on Help's right click, Save new version on Save's; the playlist and piano roll shown maximized
    /// and the plugin picker on a right click, which acts on the release over the same button.
    #[test]
    fn toolbar_buttons_left_and_right() {
        fn right(app: &mut App, c: &mut Canvas, r: Rect, release: (f32, f32)) {
            app.mouse_move(r.cx(), r.cy());
            app.mouse_down_right();
            app.mouse_move(release.0, release.1);
            app.mouse_up();
            frame(app, c, None);
        }
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        frame(&mut app, &mut c, None);
        let hint = |app: &App, a: &Action| app.hits.iter().rev().find(|h| &h.action == a).map(|h| h.hint.to_string()).unwrap();
        for (a, h) in [
            (Action::Util(Util::Undo), "Undo / Undo history (Ctrl+Z)"),
            (Action::Util(Util::Help), "Help index / About..."),
            (Action::Util(Util::Save), "Save as... / Save new version (Shift+Ctrl+S)"),
            (Action::MenuItem(0, views::menus::FILE_EXPORT), "Export audio... (Ctrl+R)"),
            (Action::MenuItem(views::menus::OPTIONS_MENU, views::menus::OPT_INFO), "Project info (F11)"),
            (Action::ToggleWin(Win::Playlist), "Arranger / Maximize arranger (F5)"),
            (Action::ToggleWin(Win::PianoRoll), "Piano roll / Maximize Piano roll (F7)"),
            (Action::ToggleWin(Win::Browser), "Browser / Plugin palette (Alt+F8)"),
            (Action::PickerOpen, "Plugin palette (F8)"),
            (Action::ProjectPicker, "Clip palette / Plugin palette (Ctrl+F8)"),
            (Action::ToggleWin(Win::Touch), "Touch controller (Alt+F7)"),
        ] {
            assert_eq!(hint(&app, &a), h);
        }
        assert_eq!(hint(&app, &Action::Util(Util::AudioEditor)), "Open audio editor / New audio editor");
        // Hidden until it has a shop icon of its own.
        assert!(!app.hits.iter().any(|h| h.action == Action::Placeholder("Plugin repository")));
        // Help: the left click does nothing; the right click is About, on the release over the button only.
        let help = hit_rect(&app, &Action::Util(Util::Help));
        app.state.message = None;
        click(&mut app, &mut c, help);
        assert!(app.state.message.is_none() && app.state.dialogs.is_empty());
        right(&mut app, &mut c, help, (help.cx() + 40.0, help.cy()));
        assert!(app.state.message.is_none());
        right(&mut app, &mut c, help, (help.cx(), help.cy()));
        assert!(app.state.message.as_deref().is_some_and(|m| m.starts_with("Lemon Studio ")), "{:?}", app.state.message);
        // Undo: EDIT > Undo; its right click does nothing yet.
        let undo = hit_rect(&app, &Action::Util(Util::Undo));
        app.state.message = None;
        right(&mut app, &mut c, undo, (undo.cx(), undo.cy()));
        assert!(app.state.message.is_none() && app.state.playlist.popup.is_none());
        click(&mut app, &mut c, undo);
        assert_eq!(app.state.message.as_deref(), Some("Nothing to undo"));
        // Save: Save as... (the save dialog even with a file), Save new version on the right.
        let path = std::env::temp_dir().join(format!("os_toolbar_save_{}_song.lemon", std::process::id()));
        let next = path.with_file_name(format!("os_toolbar_save_{}_song_2.lemon", std::process::id()));
        let _ = std::fs::remove_file(&next);
        app.save_to(path.clone());
        let save = hit_rect(&app, &Action::Util(Util::Save));
        let saves = crate::dialog::fake::shown().1;
        crate::dialog::fake::answer(None);
        click(&mut app, &mut c, save);
        assert_eq!(crate::dialog::fake::shown().1, saves + 1);
        assert!(!next.exists());
        right(&mut app, &mut c, save, (save.cx(), save.cy()));
        assert!(next.exists(), "{:?}", app.state.message);
        let _ = (std::fs::remove_file(&path), std::fs::remove_file(&next));
        // Playlist and piano roll: shown maximized and in front; a second right click leaves them maximized.
        for w in [Win::Playlist, Win::PianoRoll] {
            app.state.hide(w);
            let slot = w.slot().unwrap();
            app.state.wins[slot].maxed = false;
            frame(&mut app, &mut c, None);
            let b = hit_rect(&app, &Action::ToggleWin(w));
            for _ in 0..2 {
                right(&mut app, &mut c, b, (b.cx(), b.cy()));
                assert!(app.state.shown(w) && app.state.in_front(w) && app.state.wins[slot].maxed && !app.state.wins[slot].mini, "{w:?}");
            }
        }
        // Browser: the plugin picker, the browser itself left as it is.
        let shown = app.state.show_browser;
        let b = hit_rect(&app, &Action::ToggleWin(Win::Browser));
        right(&mut app, &mut c, b, (b.cx(), b.cy()));
        assert!(crate::plugin_menu::is_open(&app.state) && app.state.show_browser == shown);
    }

    /// The time display's right-click menu (spec: ui/time-display.md §1, §3): every mode and middle unit works, one
    /// dot each; a left click goes between Bar and M:S:CS, and from SMPTE to Bar.
    #[test]
    fn time_display_modes() {
        use crate::clock::{Mode, RATES};
        use crate::playlist::Mark;
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        frame(&mut app, &mut c, None);
        let clock = hit_rect(&app, &Action::Clock);
        let pick = |app: &mut App, c: &mut Canvas, label: &str| {
            app.mouse_move(clock.cx(), clock.cy());
            app.mouse_down_right();
            app.mouse_up();
            frame(app, c, None);
            let items = &app.state.playlist.popup.as_ref().expect("the clock menu").items;
            assert!(items.iter().filter(|it| it.label != "Mode" && it.label != "Middle unit").all(|it| it.enabled));
            let i = items.iter().position(|it| it.label == label).unwrap();
            let r = hit_rect(app, &Action::PlPopup(i));
            click(app, c, r);
            assert!(app.state.playlist.popup.is_none());
        };
        let dots = |app: &App| {
            let p = views::menus::clock_popup(&app.state, 0.0, 0.0);
            p.items.into_iter().filter(|it| it.mark == Mark::Radio(true)).map(|it| it.label).collect::<Vec<_>>()
        };
        assert_eq!(dots(&app), ["Bar : Beat/Step : Tick", "Step"]);
        assert_eq!(views::menus::clock_popup(&app.state, 0.0, 0.0).items.len(), 2 + 2 + RATES.len() + 2);
        pick(&mut app, &mut c, "SMPTE 29.97 fps (drop)");
        assert_eq!(app.state.clock.mode, Mode::Smpte(3));
        assert_eq!(dots(&app), ["SMPTE 29.97 fps (drop)", "Step"]);
        assert_eq!(app.hits.iter().find(|h| h.action == Action::Clock).unwrap().hint, "Song position (minute:second:frame)");
        // A left click from SMPTE: Bar; then M:S:CS and back.
        click(&mut app, &mut c, clock);
        assert_eq!(app.state.clock.mode, Mode::Bar);
        click(&mut app, &mut c, clock);
        assert_eq!(app.state.clock.mode, Mode::Msc);
        click(&mut app, &mut c, clock);
        pick(&mut app, &mut c, "Beat");
        assert_eq!(dots(&app), ["Bar : Beat/Step : Tick", "Beat"]);
        assert_eq!(app.hits.iter().find(|h| h.action == Action::Clock).unwrap().hint, "Song position (bar:beat:tick)");
        pick(&mut app, &mut c, "Minute : Second : Centisecond");
        assert!(app.state.clock.mode == Mode::Msc && app.state.clock.beat);
        // Kept for the next start (settings.txt).
        assert_eq!(App::new(None).state.clock, app.state.clock);
    }

    /// The Toolbars switches but Hide, and the time display's mode, are found again at the next start (settings.txt;
    /// spec: ui/toolbar-menu.md §8, ui/time-display.md §3).
    #[test]
    fn header_settings_are_kept() {
        use views::toolbar::{Opts, Switch};
        let mut app = App::new(None);
        assert_eq!((app.state.toolbar, app.state.clock), (Opts::default(), crate::clock::Clock::default()));
        for sw in [Switch::Separators, Switch::Flat, Switch::HintBar, Switch::Hide] {
            app.toolbar_switch(sw);
        }
        app.act(Action::Clock);
        let next = App::new(None);
        assert_eq!(next.state.toolbar, Opts { separators: false, flat: true, hint_bar: true, hidden: false });
        assert_eq!(next.state.clock.mode, crate::clock::Mode::Msc);
    }

    /// Every drop-down entry fits its row in this font: the label is not clipped and stops before its shortcut or its
    /// ▸ (the columns are FL's widths, widened where an entry needs it; macOS test of #61: TOOLS > Change declicking
    /// for selected channels to ▸ was cut at "channels t").
    #[test]
    fn menu_entries_fit_their_columns() {
        use views::menus::{label_now, shortcut_now, Kind, MENUS};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.rack.sel = [0, 1].into();
        let f = os_ui::Font::new(14);
        for (menu, m) in MENUS.iter().enumerate() {
            app.state.menu_open = Some(menu);
            frame(&mut app, &mut c, None);
            for it in m.cols.iter().flat_map(|(_, items)| items.iter()).filter(|it| matches!(it.kind, Kind::Item)) {
                let Some(h) = app.hits.iter().rev().find(|h| h.hint == it.label || h.action == Action::Placeholder(it.label)) else { continue };
                let label = c.text_width(f, &label_now(&app.state, menu, it));
                let key = if it.shortcut.is_empty() { 0.0 } else { c.text_width(f.italic(), shortcut_now(it)) + 4.0 };
                let end = h.rect.right() - if it.sub { 13.0 } else { 8.0 } - key;
                assert!(h.rect.x + 20.0 + label <= end, "{}: {:?} ends at {} past {end}", m.title, it.label, h.rect.x + 20.0 + label);
            }
        }
    }

    /// The toolbar's pattern box: a short name sits at its right; a name too long for the box keeps its start, cut at
    /// the spin arrow (macOS test of #61: "Lemon Sampler" read "mon Sampler").
    #[test]
    fn pattern_box_keeps_the_start_of_long_names() {
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.pattern = 1;
        // The left end of the box's text area, drawn with pattern 1 named `name`.
        let left = |app: &mut App, c: &mut Canvas, name: &str| {
            crate::patterns::pat_mut(&mut app.state, 1).name = Some(name.into());
            frame(app, c, None);
            let pb = hit_rect(app, &Action::PatternNext);
            let (x0, y0) = (pb.x as u32 + 4, pb.y as u32 + 6);
            (x0..x0 + 12).flat_map(|x| (y0..pb.bottom() as u32 - 6).map(move |y| (x, y))).map(|(x, y)| c.pixmap.pixel(x, y).unwrap()).collect::<Vec<_>>()
        };
        let empty = left(&mut app, &mut c, "");
        assert_eq!(left(&mut app, &mut c, "P1"), empty, "a short name is right-aligned");
        // The first letter shows: names differing only there draw differently.
        let (a, b) = (left(&mut app, &mut c, "Lemon Sampler and more"), left(&mut app, &mut c, "Wemon Sampler and more"));
        assert!(a != empty && a != b, "a long name shows its start");
    }

    /// The floating hint bar wraps a hint too long for a line instead of cutting it (macOS test of #61: "...right-click:
    /// channel mer").
    #[test]
    fn hint_bar_wraps_long_hints() {
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.toolbar.hint_bar = true;
        // The hint bar's second line, with `text` as the message (the mouse over the bar, which has no hint).
        let second = |app: &mut App, c: &mut Canvas, text: &str| {
            app.state.message = Some(text.into());
            let r = views::toolbar::hint_bar_rect(&app.state);
            app.mouse_move(r.cx(), r.bottom() - 4.0);
            frame(app, c, None);
            let y0 = (r.y + 5.0 + c.line_height(os_ui::Font::new(14))) as u32;
            (r.x as u32 + 7..r.x as u32 + 300).flat_map(|x| (y0 + 3..y0 + 12).map(move |y| (x, y))).map(|(x, y)| c.pixmap.pixel(x, y).unwrap()).collect::<Vec<_>>()
        };
        let empty = second(&mut app, &mut c, "Short");
        let long = "Click: plugin window, Shift+click: rename, right-click: channel menu, drag: move the channel to another row";
        assert_ne!(second(&mut app, &mut c, long), empty, "the rest goes on the second line");
    }

    /// The channel rack and time signatures (spec: ui/channel-rack.md §23, FL's numbers): the lit steps, the Auto
    /// length and the mini roll's bars are the pattern's own bars; the step colour groups (a beat) and Set to ▸ N bars
    /// are the project's; a step past an explicit length grows it to the end of the pattern's bar holding it.
    #[test]
    fn rack_follows_time_signatures() {
        use crate::rack::{bar_steps, beat_steps, pattern_bars, pattern_steps, RCmd};
        use os_model::TimeMarker;
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.pattern = 1;
        crate::patterns::pat_mut(&mut app.state, 1).notes.clear();
        let set_to = |app: &App| -> Vec<u32> {
            let items = crate::rack::length_menu(&app.state, 0.0, 0.0).items;
            items.into_iter().filter_map(|it| if let crate::playlist::Cmd::Rack(RCmd::SetLen(Some(v))) = it.cmd { Some(v) } else { None }).collect()
        };
        // 4/4 and no marker: one 16-step bar, beats of 4.
        let s = &app.state;
        assert_eq!((pattern_steps(s), beat_steps(s), bar_steps(s)), (16, 4, 16));
        // A 3/8 pattern in a 4/4 project: 6 lit steps, 12 with step 8 on; the groups stay at 4; Set to counts 16-step
        // bars; the pattern's bars are 6 steps.
        let sig = TimeMarker { kind: TimeMarker::SIGNATURE, numerator: 3, denominator: 8, ..TimeMarker::new(0, "3/8") };
        crate::patterns::pat_mut(&mut app.state, 1).markers.push(sig);
        assert_eq!((pattern_steps(&app.state), beat_steps(&app.state)), (6, 4));
        crate::rack::set_step(&mut app.state, 0, 7, true);
        assert_eq!(pattern_steps(&app.state), 12);
        assert_eq!(set_to(&app), [16, 32, 64, 128, 256, 512]);
        assert_eq!(pattern_bars(&app.state, 12 * 24), [(0, 144), (144, 288)]);
        crate::rack::set_len(&mut app.state, Some(6));
        app.rack_step(0, 8, false);
        app.drag = None;
        assert_eq!(pattern_steps(&app.state), 12, "grown to the second 3/8 bar");
        // A 6/8 project: beats of 2 steps and bars of 12 (an empty pattern without a marker, Set to's 1 bar); 6/16:
        // beats of 1 and bars of 6.
        app.state.project.bar = Some((6, 2));
        app.state.pattern = 2;
        let s = &app.state;
        assert_eq!((pattern_steps(s), beat_steps(s), bar_steps(s)), (12, 2, 12));
        assert_eq!(set_to(&app)[0], 12);
        app.state.project.bar = Some((6, 1));
        let s = &app.state;
        assert_eq!((pattern_steps(s), beat_steps(s)), (6, 1));

        // Drawn: in the 6/8 project the steps alternate in pairs; the 3/8 pattern's mini roll shades its two bars, each
        // half the width.
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        app.state.project.bar = Some((6, 2));
        app.state.raise(Win::ChannelRack);
        frame(&mut app, &mut c, None);
        let at = |c: &Canvas, x: f32, y: f32| c.pixmap.pixel(x as u32, y as u32).unwrap();
        let face = |app: &App, c: &Canvas, k: u32| {
            let r = hit_rect(app, &Action::Step(1, k));
            at(c, r.x + 6.0, r.y + 12.0)
        };
        let f: Vec<_> = (0..5).map(|k| face(&app, &c, k)).collect();
        assert!(f[0] == f[1] && f[1] != f[2] && f[2] == f[3] && f[3] != f[4], "pairs");
        app.state.pattern = 1;
        app.state.rack_roll_view = true;
        frame(&mut app, &mut c, None);
        let b = hit_rect(&app, &Action::RackRoll(1));
        // Clear of the preview's mute button at its left end.
        let shade = |q: f32| at(&c, b.x + b.w * q, b.y + 15.0);
        assert!(shade(0.1) == shade(0.45) && shade(0.45) != shade(0.55) && shade(0.55) == shade(0.95), "two bars over the width");
    }

    /// The rack's preview options (spec: ui/channel-rack.md §17): Show a mute button on the piano roll preview puts the
    /// LED's switch on the preview; Show complete piano roll preview unticked lays the preview under the steps.
    #[test]
    fn rack_preview_mute_button_and_preview_under_the_steps() {
        let mut app = App::new(None);
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        app.state.rack_roll_view = true;
        app.state.rack_size.0 = views::rack::DEFAULT_SIZE.0 + 320.0;
        let ch = app.state.project.channels[1].index;
        let ps = crate::rack::per_step(&app.state);
        let n = app.state.pattern.max(1);
        crate::patterns::pat_mut(&mut app.state, n).notes.push(os_model::Note { length: ps, ..os_model::Note::step(ch, 12 * ps) });
        app.state.raise(Win::ChannelRack);
        frame(&mut app, &mut c, None);
        let roll = hit_rect(&app, &Action::RackRoll(1));
        let button = hit_rect(&app, &Action::ChannelEnable(1));
        assert!(roll.contains(button.cx(), button.cy()), "the button is on the preview");
        click(&mut app, &mut c, button);
        assert!(!app.state.project.channels[1].enabled);
        crate::rack::run(&mut app, crate::rack::RCmd::Opt(crate::rack::PREVIEW_MUTE));
        frame(&mut app, &mut c, None);
        let led = hit_rect(&app, &Action::ChannelEnable(1));
        assert!(!roll.contains(led.cx(), led.cy()), "only the LED");

        // The steps' columns, from the first row's steps (the second row, with a long note, shows its preview).
        app.state.rack_roll_view = false;
        frame(&mut app, &mut c, None);
        let x = |k: u32| (hit_rect(&app, &Action::Step(0, k)).x + 6.0) as u32;
        let (x10, x12) = (x(10), x(12));
        app.state.rack_roll_view = true;
        let column = |c: &Canvas, x: u32| (roll.y as u32 + 2..roll.bottom() as u32 - 2).map(|y| c.pixmap.pixel(x, y).unwrap()).collect::<Vec<_>>();
        let both = |app: &mut App, c: &mut Canvas| {
            frame(app, c, None);
            (column(c, x10), column(c, x12))
        };
        let (a, b) = both(&mut app, &mut c);
        assert_eq!(a, b, "complete: the note is not under its step");
        crate::rack::run(&mut app, crate::rack::RCmd::Opt(crate::rack::COMPLETE_PREVIEW));
        let (a, b) = both(&mut app, &mut c);
        assert_ne!(a, b, "under the steps: the note is under step 12");
    }

    /// The Toolbars menu (spec: ui/toolbar-menu.md §8): on the hint panel, on bare toolbar background and as VIEW >
    /// Toolbars ▸; Separators (on), Flat buttons, Hint bar (off) and Hide (Ctrl+F11) work, the rest stays greyed with
    /// our layout checked. Separators off: no lines, row 1 one pixel down and row 2 one up, also after a partial
    /// repaint. Flat: no face on an unlit button. Hint bar: the floating panel, repainted with the hint panel. Hide:
    /// no toolbar, the browser, workspace and floating windows up by its height; Ctrl+F11 brings them back.
    #[test]
    fn toolbars_menu() {
        use crate::playlist::Mark;
        use crate::views::toolbar::{band, hint_bar_rect, Opts};
        use views::menus::{item_at, submenu, toolbar_items, works, MENUS, VIEW};
        use winit::keyboard::{Key, NamedKey};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        frame(&mut app, &mut c, None);
        assert_eq!(app.state.toolbar, Opts { separators: true, flat: false, hint_bar: false, hidden: false });
        // The menu, from the hint panel and from bare background.
        let open = |app: &mut App, c: &mut Canvas, (x, y): (f32, f32)| {
            app.mouse_move(x, y);
            app.mouse_down_right();
            app.mouse_up();
            frame(app, c, None);
            app.state.playlist.popup.clone().expect("the Toolbars menu")
        };
        let bare = (1490.0, 20.0);
        assert_eq!(app.top_hit(bare.0, bare.1).map(|h| h.action.clone()), Some(Action::DragWindow));
        for at in [(200.0, 20.0), bare] {
            let p = open(&mut app, &mut c, at);
            assert_eq!(p.items, toolbar_items(&app.state));
            app.key(&Key::Named(NamedKey::Escape), false, false);
        }
        let items = toolbar_items(&app.state);
        let row = |label: &str| items.iter().find(|i| i.label == label).unwrap_or_else(|| panic!("{label}"));
        for (label, enabled, mark) in [
            ("Edit", true, Mark::Check(false)),
            ("Separators", true, Mark::Check(true)),
            ("Flat buttons", true, Mark::Check(false)),
            ("Show detected MIDI chords near the status bar", false, Mark::Check(false)),
            ("Status bar", true, Mark::Check(false)),
            ("Hide", true, Mark::Check(false)),
        ] {
            assert_eq!((row(label).enabled, row(label).mark), (enabled, mark), "{label}");
        }
        assert_eq!(row("Hide").key, "Ctrl+F11");
        // The factory presets (no user ones here), the classic layout's checked.
        let presets: Vec<_> = items.iter().skip_while(|i| i.label != "Presets").skip(1).collect();
        assert!(presets.len() == crate::toolbar::PRESETS.len() && presets.iter().all(|i| i.enabled));
        assert_eq!(presets.iter().filter(|i| i.mark == Mark::Radio(true)).map(|i| i.label.as_ref()).collect::<Vec<&str>>(), ["Classic"]);
        // VIEW > Toolbars ▸ is the same menu.
        let entry = MENUS[VIEW].cols.iter().flat_map(|(_, items)| items.iter()).position(|it| it.label == "Toolbars").unwrap();
        assert!(works(VIEW, item_at(VIEW, entry).unwrap()));
        assert_eq!(submenu(VIEW, "Toolbars").unwrap()(&app.state), items);
        let pick = |app: &mut App, c: &mut Canvas, label: &str| {
            let p = open(app, c, (200.0, 20.0));
            let i = p.items.iter().position(|it| it.label == label).unwrap();
            let r = hit_rect(app, &Action::PlPopup(i));
            click(app, c, r);
            app.mouse_move(1300.0, 600.0);
            frame(app, c, None);
        };
        let px = |c: &Canvas, x: u32, y: u32| c.pixmap.pixel(x, y).unwrap();
        let column = |c: &Canvas, x: u32| (0..83).map(|y| px(c, x, y)).collect::<Vec<_>>();
        // Separators off: the lines go, row 1 moves down a pixel, row 2 up one.
        let (r1, r2) = (column(&c, 595), column(&c, 990));
        let line = px(&c, 379, 20);
        pick(&mut app, &mut c, "Separators");
        assert!(!app.state.toolbar.separators);
        assert_ne!(px(&c, 379, 20), line);
        let (s1, s2) = (column(&c, 595), column(&c, 990));
        // Row 41 (the old line) holds row 2's top pixel: the rows close over it.
        assert_eq!(s1[1..=40], r1[0..=39]);
        assert_eq!(s2[41..80], r2[42..81]);
        // A partial repaint of the toolbar repaints it whole, so the pixels move once.
        app.damage = Damage::None;
        app.damage_band(CPU_RECT);
        let (d, b) = (app.take_damage(), band(&app.state, 1920.0));
        assert!(matches!(&d, Some(Some(v)) if v.iter().any(|r| r.contains(b.x, b.y) && r.contains(b.right() - 1.0, b.bottom() - 1.0))), "{d:?}");
        frame_damage(&mut app, &mut c, d);
        assert_eq!((column(&c, 595), column(&c, 990)), (s1, s2));
        pick(&mut app, &mut c, "Separators");
        assert!(app.state.toolbar.separators && column(&c, 595) == r1);
        // Flat buttons: an unlit button is just its icon on the toolbar background.
        assert_ne!(px(&c, 580, 20), px(&c, 611, 20));
        pick(&mut app, &mut c, "Flat buttons");
        assert!(app.state.toolbar.flat);
        assert_eq!(px(&c, 580, 20), px(&c, 611, 20));
        pick(&mut app, &mut c, "Flat buttons");
        // Hint bar: the floating panel at the bottom-left, over the windows; it repaints with the hint panel.
        pick(&mut app, &mut c, "Status bar");
        let hb = hint_bar_rect(&app.state);
        assert_eq!(hb, Rect::new(0.0, 1080.0 - 84.0, 400.0, 82.0));
        assert_eq!(app.top_hit(hb.cx(), hb.cy()).map(|h| h.action.clone()), Some(Action::Swallow));
        let p = px(&c, 2, hb.y as u32 + 2);
        assert!(p.red() < 0x20 && p.green() < 0x20 && p.blue() < 0x20, "{p:?}");
        app.damage = Damage::None;
        app.damage_band(HINT_RECT);
        assert!(matches!(app.take_damage(), Some(Some(v)) if v.iter().any(|r| r.contains(hb.x, hb.y) && r.contains(hb.right() - 1.0, hb.bottom() - 1.0))));
        pick(&mut app, &mut c, "Status bar");
        assert!(!app.state.toolbar.hint_bar);
        // Hide (Ctrl+F11): no toolbar; the browser and the workspace from the top, the floating playlist up with them.
        app.state.show_browser = true;
        app.state.wins[0].maxed = false;
        app.state.wins[0].rect = Rect::new(400.0, 200.0, 900.0, 500.0);
        app.state.raise(Win::Playlist);
        frame(&mut app, &mut c, None);
        let tab = hit_rect(&app, &Action::BrowserTab(1));
        app.key(&Key::Named(NamedKey::F11), true, false);
        frame(&mut app, &mut c, None);
        assert!(app.state.toolbar.hidden && !app.state.settings.open);
        assert_eq!(app.state.work_rect().y, 0.0);
        assert!(!app.hits.iter().any(|h| h.action == Action::Clock || h.action == Action::DragWindow));
        assert_eq!(hit_rect(&app, &Action::BrowserTab(1)).y, tab.y - views::WORK_Y);
        assert_eq!(app.state.win_rect(Win::Playlist).y, 200.0 - views::WORK_Y);
        // Ctrl+F11 again: everything as it was.
        app.key(&Key::Named(NamedKey::F11), true, false);
        frame(&mut app, &mut c, None);
        assert!(!app.state.toolbar.hidden && app.hits.iter().any(|h| h.action == Action::Clock));
        assert_eq!((app.state.work_rect().y, app.state.win_rect(Win::Playlist).y), (views::WORK_Y, 200.0));
        assert_eq!(hit_rect(&app, &Action::BrowserTab(1)), tab);
        // From the menu too; F11 alone is still Project info.
        pick(&mut app, &mut c, "Hide");
        assert!(app.state.toolbar.hidden);
        app.key(&Key::Named(NamedKey::F11), true, false);
        app.key(&Key::Named(NamedKey::F11), false, false);
        assert!(!app.state.toolbar.hidden && app.state.settings.open);
    }

    /// Opens the Toolbars menu on the hint panel and clicks its entry `label`.
    fn toolbars_pick(app: &mut App, c: &mut Canvas, label: &str) {
        let at = hit_rect(app, &Action::DragWindow);
        app.mouse_move(at.x + 200.0, 20.0);
        app.mouse_down_right();
        app.mouse_up();
        frame(app, c, None);
        let i = app.state.playlist.popup.as_ref().expect("the Toolbars menu").items.iter().position(|it| it.label == label).unwrap_or_else(|| panic!("{label}"));
        click(app, c, hit_rect(app, &Action::PlPopup(i)));
    }

    /// Press at `from`, move half way (painted) and on to `to`, release there.
    fn drag_to(app: &mut App, c: &mut Canvas, from: (f32, f32), to: (f32, f32)) {
        app.mouse_move(from.0, from.1);
        app.mouse_down();
        app.mouse_move((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0);
        frame(app, c, None);
        app.mouse_move(to.0, to.1);
        app.mouse_up();
        frame(app, c, None);
    }

    /// A toolbar panel paints the same wherever the layout puts it in its row (spec: ui/toolbar-menu.md §8): the
    /// transport moved from x 379 to the row's start is the same pixels, and its controls answer there, the tempo's
    /// digits too.
    #[test]
    fn moved_panels_paint_the_same() {
        use crate::toolbar::{Layout, Panel};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.mouse_move(1300.0, 600.0);
        frame(&mut app, &mut c, None);
        let block = |c: &Canvas, x0: u32| (0..40).flat_map(|y| (0..192).map(move |x| (x, y))).map(|(x, y)| c.pixmap.pixel(x0 + x, y).unwrap()).collect::<Vec<_>>();
        let home = block(&c, 383);
        let (play, tempo) = (hit_rect(&app, &Action::Play), hit_rect(&app, &Action::Tempo));
        let digits = app.state.tempo_digits.get();
        let mut l = Layout::classic();
        l.rows[0].retain(|&p| p != Panel::Transport);
        l.rows[0].insert(0, Panel::Transport);
        app.set_toolbar_layout(l);
        frame(&mut app, &mut c, None);
        assert!(block(&c, 4) == home, "the transport's pixels moved with it");
        assert_eq!(hit_rect(&app, &Action::Play), Rect { x: play.x - 379.0, ..play });
        assert_eq!(hit_rect(&app, &Action::Tempo), Rect { x: tempo.x - 379.0, ..tempo });
        assert_eq!(app.state.tempo_digits.get(), digits.map(|d| d - 379.0));
        // The hint panel went right by the transport's width: its repaints follow it.
        let _ = app.take_damage();
        app.damage_band(HINT_RECT);
        let panel = views::band_rect(&app.state, HINT_RECT);
        assert_eq!(panel, Rect { x: HINT_RECT.x + 196.0, ..HINT_RECT });
        let damage = app.take_damage().flatten().expect("rects");
        assert!(damage.iter().any(|r| r.contains(panel.x, panel.y) && r.contains(panel.right() - 1.0, panel.bottom() - 1.0)), "{damage:?}");
    }

    /// The toolbar editor (spec: ui/toolbar-menu.md §8): Toolbars ▸ Edit puts grips and the palette up; a panel dragged
    /// onto the palette comes off the toolbar, its tile dragged back puts it on again, a row's grip swaps the rows, the
    /// palette's rows switch makes one row of two (the workspace and its windows move up with the band), Done ends it.
    /// The layout is kept in settings.txt.
    #[test]
    fn toolbar_editor_moves_panels() {
        use crate::toolbar::{BarHit, Layout, Panel, Tile};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.wins[0].maxed = false;
        app.state.wins[0].rect = Rect::new(400.0, 300.0, 900.0, 500.0);
        app.state.raise(Win::Playlist);
        frame(&mut app, &mut c, None);
        toolbars_pick(&mut app, &mut c, "Edit");
        assert!(app.state.bar.editing);
        let pal = views::palette_rect(&app.state).expect("the palette");
        assert_eq!((pal.x, pal.y, pal.w), (0.0, 83.0, 1920.0));
        assert_eq!(app.top_hit(600.0, pal.y + 2.0).map(|h| h.action.clone()), Some(Action::Bar(BarHit::Palette)));
        // A panel's controls give way to the panel itself: no Play hit, the hint panel one thing to drag.
        assert!(!app.hits.iter().any(|h| h.action == Action::Play));
        let hint = hit_rect(&app, &Action::Bar(BarHit::Item(0, 1)));
        assert_eq!(app.top_hit(hint.cx(), hint.cy()).map(|h| h.hint.to_string()).as_deref(), Some("Hint panel - drag to move it, onto the palette to take it off"));
        // Dragged onto the palette: off the toolbar, which keeps the layout in settings.txt.
        drag_to(&mut app, &mut c, (hint.cx(), hint.cy()), (600.0, pal.y + 40.0));
        assert!(!app.state.bar.layout.has(Panel::Hint));
        assert!(!crate::settings_file::get("toolbar_layout").expect("kept").contains("hint"));
        // Its tile, bright again, back at row 1's start; a tile of a panel on the toolbar is dimmed and does not drag.
        let tile = hit_rect(&app, &Action::Bar(BarHit::Tile(Tile::Panel(Panel::Hint))));
        drag_to(&mut app, &mut c, (tile.x + 5.0, tile.cy()), (14.0, 20.0));
        assert_eq!(app.state.bar.layout.rows[0][0], Panel::Hint);
        assert!(!app.hits.iter().any(|h| h.action == Action::Bar(BarHit::Tile(Tile::Panel(Panel::Hint)))));
        // Row 1's grip dropped on row 2: the rows swap.
        let top = app.state.bar.layout.rows[0].clone();
        let grip = hit_rect(&app, &Action::Bar(BarHit::Grip(0)));
        drag_to(&mut app, &mut c, (grip.cx(), grip.cy()), (grip.cx(), 60.0));
        assert_eq!(app.state.bar.layout.rows[1], top);
        // One row: the band 42 px tall, the workspace, the palette and the floating playlist 41 px up.
        let r = hit_rect(&app, &Action::Bar(BarHit::Rows(1)));
        click(&mut app, &mut c, r);
        assert_eq!(app.state.bar.layout.rows.len(), 1);
        assert_eq!((views::band_h(&app.state), app.state.work_rect().y, app.state.win_rect(Win::Playlist).y), (42.0, 42.0, 259.0));
        assert_eq!(views::palette_rect(&app.state).map(|p| p.y), Some(42.0));
        // Done: the controls answer again; the layout is the one the next start loads.
        let r = hit_rect(&app, &Action::Bar(BarHit::Done));
        click(&mut app, &mut c, r);
        assert!(!app.state.bar.editing && views::palette_rect(&app.state).is_none());
        assert!(app.hits.iter().any(|h| h.action == Action::Play));
        assert_eq!(Layout::load(), app.state.bar.layout);
    }

    /// Toolbar presets (spec: ui/toolbar-menu.md §8): a factory one from the Toolbars menu lays the toolbar out at once
    /// (One row: the menu bar in the one row, its menus opening under it); the editor's Save preset as... writes the
    /// layout to the user folder's Toolbar presets, where the menus list it under "User presets", checked; Delete
    /// preset asks, then removes the file.
    #[test]
    fn toolbar_presets() {
        use crate::dialog::DHit;
        use crate::playlist::Mark;
        use crate::toolbar::{BarHit, Layout, Panel, PRESETS};
        use views::menus::{pick, FILE_SAVE};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let dir = std::env::temp_dir().join(format!("lemon-toolbar-presets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        THEME_DIR.with(|d| *d.borrow_mut() = Some(dir.clone()));
        let mut app = App::new(None);
        frame(&mut app, &mut c, None);
        let r = hit_rect(&app, &Action::Menu(0));
        click(&mut app, &mut c, r);
        let save = hit_rect(&app, &pick(0, FILE_SAVE));
        app.state.menu_open = None;
        // One row, from the Toolbars menu: the menu bar 41 px up, FILE opening 41 px higher.
        toolbars_pick(&mut app, &mut c, "One row");
        let one = PRESETS.iter().position(|p| p.name == "One row").unwrap();
        assert_eq!(app.state.bar.layout, PRESETS[one].layout());
        assert_eq!((views::band_h(&app.state), app.state.work_rect().y), (42.0, 42.0));
        let file = hit_rect(&app, &Action::Menu(0));
        assert!(file.bottom() <= 40.0, "{file:?}");
        click(&mut app, &mut c, file);
        assert_eq!(hit_rect(&app, &pick(0, FILE_SAVE)), Rect { y: save.y - 41.0, ..save });
        app.state.menu_open = None;
        frame(&mut app, &mut c, None);

        // A layout of one's own, saved from the editor.
        let mut mine = PRESETS[one].layout();
        mine.rows[0].retain(|&p| p != Panel::Snap);
        app.set_toolbar_layout(mine.clone());
        app.toolbar_edit(true);
        frame(&mut app, &mut c, None);
        let path = dir.join("Toolbar presets").join("Mine.lstoolbar");
        crate::dialog::fake::answer(Some(path.clone()));
        let editor_pick = |app: &mut App, c: &mut Canvas, label: &str| {
            click(app, c, hit_rect(app, &Action::Bar(BarHit::Presets)));
            let items = app.state.playlist.popup.as_ref().expect("the preset menu").items.clone();
            let i = items.iter().position(|it| it.label == label).unwrap_or_else(|| panic!("{label}"));
            assert!(items[i].enabled, "{label}");
            click(app, c, hit_rect(app, &Action::PlPopup(i)));
        };
        editor_pick(&mut app, &mut c, "Save preset as...");
        assert_eq!(std::fs::read_to_string(&path).ok().and_then(|t| Layout::parse(&t)), Some(mine.clone()));
        assert_eq!(app.state.bar.name(), "Mine");
        let items = views::menus::toolbar_items(&app.state);
        let user: Vec<_> = items.iter().skip_while(|i| i.label != "User presets").skip(1).map(|i| (i.label.to_string(), i.mark)).collect();
        assert_eq!(user, [("Mine".to_string(), Mark::Radio(true))]);
        assert!(items.iter().filter(|i| i.mark == Mark::Radio(true)).count() == 1);
        // Classic, then Mine again from its menu entry.
        editor_pick(&mut app, &mut c, "Classic");
        assert_eq!((app.state.bar.layout.clone(), views::band_h(&app.state)), (Layout::classic(), 83.0));
        let r = hit_rect(&app, &Action::Bar(BarHit::Presets));
        click(&mut app, &mut c, r);
        assert!(!app.state.playlist.popup.as_ref().unwrap().items.iter().any(|i| i.label == "Delete preset" && i.enabled), "a factory preset stays");
        app.state.playlist.popup = None;
        editor_pick(&mut app, &mut c, "Mine");
        assert_eq!(app.state.bar.layout, mine);
        // Delete preset: asked first; Yes removes the file, the layout stays.
        editor_pick(&mut app, &mut c, "Delete preset");
        assert!(app.modal_open() && path.exists());
        let r = hit_rect(&app, &Action::Dlg(DHit::Button(0)));
        click(&mut app, &mut c, r);
        assert!(!path.exists() && app.state.bar.user.is_empty());
        assert_eq!((app.state.bar.layout.clone(), app.state.bar.name()), (mine, "Custom".to_string()));
        THEME_DIR.with(|d| *d.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An automation clip's channel window (spec: audio/automation.md "Range", "The LFO"): a new clip's range is 0..1
    /// (its pan knob the minimum at 0, its volume knob the maximum at 1); the LFO's switch and mode flip on a click and
    /// its knobs turn, each an undo step; everything saves into the project and loads back.
    #[test]
    fn automation_clip_window_sets_range_and_lfo() {
        use crate::knobs::{KnobId, LfoKnob};
        use views::menus::{item_at, ADD_MENU};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let entry = |label: &str| (0..).find(|&i| item_at(ADD_MENU, i).is_some_and(|it| it.label == label)).unwrap();
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.wins.iter_mut().for_each(|w| w.maxed = false);
        app.menu_item(ADD_MENU, entry("Automation clip"));
        let row = app.state.project.channels.len() - 1;
        let ch = app.state.project.channels[row].clone();
        assert_eq!((ch.pan, ch.volume), (-1.0, 1.0), "a new clip's range is 0..1");
        assert!(app.state.shown(Win::Plugin) && app.state.plugin_channel == row);
        frame(&mut app, &mut c, None);
        assert_eq!(app.state.knob_text(KnobId::ChanPan(row)), "Minimum: 0%");
        assert_eq!(app.state.knob_text(KnobId::ChanVol(row)), "Maximum: 100%");
        assert_eq!(app.state.knob_text(KnobId::AutoLfo(row, LfoKnob::On)), "LFO: off");
        assert_eq!(app.state.knob_text(KnobId::AutoLfo(row, LfoKnob::Speed)), "LFO speed: 1.00 cycles per beat");
        let lfo = |app: &App| app.state.project.automation(ch.index).unwrap().lfo.clone();
        let steps = app.state.history.done();

        // The switch and the mode: a click flips each.
        for k in [LfoKnob::On, LfoKnob::Mode] {
            let r = hit_rect(&app, &Action::Knob(KnobId::AutoLfo(row, k)));
            click(&mut app, &mut c, r);
        }
        assert_eq!((lfo(&app).switch, lfo(&app).mode), (-1, 0), "on; the untargeted clip's mode was add, now multiply");
        // Amount: dragged up by 64 px at the default rate.
        let id = KnobId::AutoLfo(row, LfoKnob::Amount);
        let r = hit_rect(&app, &Action::Knob(id));
        app.mouse_move(r.cx(), r.cy());
        app.mouse_down();
        app.mouse_move(r.cx(), r.cy() - 128.0);
        app.mouse_up();
        frame(&mut app, &mut c, None);
        assert_eq!(lfo(&app).amount, 64 + (128.0 * id.drag_rate() * 128.0).round() as i32);
        // The range: the minimum up to 25 %, through the window's MIN knob.
        let r = hit_rect(&app, &Action::Knob(KnobId::ChanPan(row)));
        assert!(app.state.plugin_pos.1 < r.y, "the knob is in the channel window");
        app.state.set_knob(KnobId::ChanPan(row), 0.25);
        assert_eq!(app.state.project.channels[row].pan, -0.5);
        assert_eq!(os_model::clip_lfo::range(-0.5, 1.0), (0.25, 0.75));
        // Each change is an undo step; undo puts them back.
        assert_eq!(app.state.history.done(), steps + 3);
        crate::history::undo(&mut app.state);
        assert_eq!(lfo(&app).amount, 64);
        crate::history::undo(&mut app.state);
        assert_eq!(lfo(&app).mode, 1);
        crate::history::redo(&mut app.state);
        crate::history::redo(&mut app.state);
        // Middle click: the minimum goes back to 0, not to the centre.
        app.state.reset_knob(KnobId::ChanPan(row));
        assert_eq!(app.state.project.channels[row].pan, -1.0);
        app.state.set_knob(KnobId::ChanPan(row), 0.25);
        app.state.set_knob(KnobId::AutoLfo(row, LfoKnob::Pulse), 0.75);
        // Saved and loaded back: the levels as the range, the LFO's fields as set.
        let path = std::env::temp_dir().join(format!("os_clip_lfo_{}.flp", std::process::id()));
        app.save_to(path.clone());
        let back = os_flp::project::load(&os_flp::FlpFile::parse(&std::fs::read(&path).unwrap()).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        let bc = back.channels.iter().find(|x| x.index == ch.index).unwrap();
        assert_eq!((bc.pan, bc.volume), (-0.5, 1.0));
        let want = lfo(&app);
        assert_eq!((want.switch, want.mode, want.pulse_width), (-1, 0, 64));
        assert_eq!(back.automation(ch.index).unwrap().lfo, want);
    }

    /// ADD > Audio clip / Automation clip (spec: ui/add-clip-channels.md §3): the channel goes last, named and routed to
    /// the master, alone selected, into its filter group (made before Unsorted) with the filter on it, as the
    /// playlist's clip source; its window opens; the automation clip has FL's default points and no target and saves
    /// so; each add is one undo step. Layer stays greyed out.
    #[test]
    fn add_audio_and_automation_clip_channels() {
        use crate::playlist::{self as pl, PickKind};
        use crate::rack::{Filter, Group};
        use os_model::{AutoMode, ClipSource};
        use views::menus::{item_at, ADD_MENU};
        let entry = |label: &str| (0..).find(|&i| item_at(ADD_MENU, i).is_some_and(|it| it.label == label)).unwrap();
        let mut app = App::new(None);
        app.state.project = test_project();
        // A file's groups: Drums, then Unsorted.
        app.state.rack.groups = vec![Group { name: "Drums".into(), members: [0, 3].into_iter().collect() }];
        app.state.rack.unsorted_at = Some(1);
        app.state.rack.select_only(1);
        app.state.selected_channel = 1;
        let before = (app.state.playlist.source, app.state.playlist.picker);
        let clips = pl::clips(&app.state).len();
        let names = |app: &App| app.state.rack.groups.iter().map(|g| g.name.clone()).collect::<Vec<_>>();
        // Audio clip.
        app.menu_item(ADD_MENU, entry("Audio clip"));
        let s = &app.state;
        let ch = s.project.channels.last().unwrap();
        assert_eq!((s.project.channels.len(), ch.index, ch.kind, ch.name.as_deref()), (6, 5, ChannelKind::AudioClip, Some("Audio Clip")));
        assert_eq!((ch.mixer_track, ch.sample_path.as_deref(), ch.sound), (0, None, Default::default()));
        assert_eq!((s.rack.sel.iter().copied().collect::<Vec<_>>(), s.selected_channel), (vec![5], 5));
        assert_eq!((names(&app), app.state.rack.unsorted_at, app.state.rack.filter), (vec!["Drums".into(), "Audio".into()], Some(2), Filter::Group(1)));
        let s = &app.state;
        assert_eq!(s.rack.groups[1].members.iter().copied().collect::<Vec<_>>(), [5]);
        assert_eq!((s.playlist.source, s.playlist.picker, pl::clips(s).len()), (Some(ClipSource::Channel(5)), PickKind::Audio, clips));
        assert_eq!(s.pending_open, Some(5), "its settings (Lemon Sampler's window) open");
        assert!(app.unsaved());
        // Automation clip.
        app.state.pending_open = None;
        app.menu_item(ADD_MENU, entry("Automation clip"));
        let s = &app.state;
        let ch = s.project.channels.last().unwrap();
        assert_eq!((ch.index, ch.kind, ch.name.as_deref(), ch.mixer_track), (6, ChannelKind::Automation, Some("Automation Clip"), 0));
        let a = s.project.automation(6).expect("its points").clone();
        let pts: Vec<_> = a.points.iter().map(|p| (p.beat, p.value, p.tension, p.mode)).collect();
        assert_eq!(pts, [(0.0, 0.5, 0.0, AutoMode::Single), (4.0, 0.5, 0.0, AutoMode::Single)]);
        assert!(a.targets.is_empty() && a.lfo.mode == 1);
        assert_eq!((names(&app), app.state.rack.unsorted_at, app.state.rack.filter), (vec!["Drums".into(), "Audio".into(), "Automation".into()], Some(3), Filter::Group(2)));
        let s = &app.state;
        assert!(s.shown(Win::Plugin) && s.plugin_channel == 6 && s.pending_open.is_none(), "its channel window opens");
        assert_eq!((s.playlist.source, s.playlist.picker), (Some(ClipSource::Channel(6)), PickKind::Automation));
        // Another audio clip: a unique name, the same group.
        app.menu_item(ADD_MENU, entry("Audio clip"));
        let s = &app.state;
        assert_eq!(s.project.channels[7].name.as_deref(), Some("Audio Clip #2"));
        assert_eq!((s.rack.groups.len(), s.rack.filter), (3, Filter::Group(1)));
        assert_eq!(s.rack.groups[1].members.iter().copied().collect::<Vec<_>>(), [5, 7]);
        // Saved as FL stores them: types 4 and 5, the groups before Unsorted, the automation's points and no link.
        let path = std::env::temp_dir().join(format!("os_add_clips_{}.flp", std::process::id()));
        app.save_to(path.clone());
        let back = os_flp::project::load(&os_flp::FlpFile::parse(&std::fs::read(&path).unwrap()).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(back.groups, ["Drums", "Audio", "Automation", os_model::UNSORTED_GROUP]);
        let kinds: Vec<_> = back.channels[5..].iter().map(|c| (c.kind, c.name.clone().unwrap_or_default(), c.rack.group)).collect();
        assert_eq!(kinds, [(ChannelKind::AudioClip, "Audio Clip".into(), 1), (ChannelKind::Automation, "Automation Clip".into(), 2), (ChannelKind::AudioClip, "Audio Clip #2".into(), 1)]);
        assert_eq!(back.current_group, Some(1));
        let a2 = back.automation(6).expect("saved points");
        assert_eq!((&a2.points, &a2.targets, a2.lfo.mode), (&a.points, &Vec::new(), 1));
        // Undo (Ctrl+Z), one step each: the channel goes, the groups, filter, selection and clip source come back.
        let undo = |app: &mut App| crate::playlist::run(app, crate::playlist::Cmd::Undo);
        assert_eq!(undo(&mut app).as_deref(), Some("Undo: add audio clip"));
        assert_eq!((app.state.project.channels.len(), app.state.rack.groups[1].members.len()), (7, 1));
        assert_eq!(undo(&mut app).as_deref(), Some("Undo: add automation clip"));
        let s = &app.state;
        assert_eq!((s.project.channels.len(), s.project.automation(6).is_none()), (6, true));
        assert_eq!((names(&app), app.state.rack.filter, app.state.selected_channel), (vec!["Drums".into(), "Audio".into()], Filter::Group(1), 5));
        undo(&mut app);
        let s = &app.state;
        assert_eq!(s.project.channels.len(), 5);
        assert_eq!((names(&app), app.state.rack.unsorted_at, app.state.rack.filter), (vec!["Drums".into()], Some(1), Filter::All));
        let s = &app.state;
        assert_eq!((s.rack.sel.iter().copied().collect::<Vec<_>>(), s.selected_channel, (s.playlist.source, s.playlist.picker)), (vec![1], 1, before));
    }

    /// ADD > Layer (spec: ui/add-clip-channels.md §4): a layer channel last in the rack whose children are the selected
    /// channels; it is the only one selected after; saved as a layer and its children; one undo step.
    #[test]
    fn add_layer_channel_with_the_selected_channels_as_children() {
        use views::menus::{item_at, works, ADD_MENU};
        let entry = |label: &str| (0..).find(|&i| item_at(ADD_MENU, i).is_some_and(|it| it.label == label)).unwrap();
        let mut app = App::new(None);
        app.state.project = test_project();
        assert!(works(ADD_MENU, item_at(ADD_MENU, entry("Layer")).unwrap()));
        let n = app.state.project.channels.len();
        app.state.rack.select_only(1);
        app.state.rack.sel.insert(3);
        let wanted = vec![app.state.project.channels[1].index, app.state.project.channels[3].index];
        app.menu_item(ADD_MENU, entry("Layer"));
        let s = &app.state;
        let ch = s.project.channels.last().unwrap();
        assert_eq!((s.project.channels.len(), ch.kind, ch.name.as_deref(), ch.volume), (n + 1, ChannelKind::Layer, Some("Layer"), 1.0));
        assert_eq!(ch.layer_children, wanted);
        assert_eq!((s.selected_channel, s.rack.sel.iter().copied().collect::<Vec<_>>()), (n, vec![n]));
        // A second one is named apart; with nothing else selected it has the first layer as its only child.
        app.menu_item(ADD_MENU, entry("Layer"));
        let ch = app.state.project.channels.last().unwrap();
        assert_eq!((ch.name.as_deref(), ch.layer_children.clone()), (Some("Layer #2"), vec![app.state.project.channels[n].index]));
        // The writer keeps it a layer with its children.
        let back = os_flp::project::load(&os_flp::project::create(&app.state.project).unwrap()).unwrap();
        let saved = back.channels.iter().find(|c| c.name.as_deref() == Some("Layer")).unwrap();
        assert_eq!((saved.kind, saved.layer_children.clone()), (ChannelKind::Layer, wanted));
        let undo = |app: &mut App| crate::playlist::run(app, crate::playlist::Cmd::Undo);
        assert_eq!(undo(&mut app).as_deref(), Some("Undo: add layer"));
        undo(&mut app);
        assert_eq!(app.state.project.channels.len(), n);
    }

    /// PATTERNS > Split by channel (spec: ui/split-by-channel.md §2): rack order; the pattern keeps the first
    /// channel's notes under its name, the others get patterns of their own right after it (later ones move up with
    /// their clips) with the original's colour, markers and settings; each clip of the original gets one clip per new
    /// pattern at its place, under it, unselected. One channel: a rename; no notes: nothing. One undo step.
    #[test]
    fn split_by_channel() {
        use crate::patterns::Cmd as PCmd;
        use crate::playlist::{self as pl, Cmd};
        use os_model::{Clip, ClipSource, ControlPoint, TimeMarker};
        use views::menus::{item_at, pattern_popup, works, MENUS, PATTERNS};
        let mut app = App::new(None);
        app.state.project = test_project();
        // Rack order Kick, Snare, Hat, Clap, Pluck (channel indices 0, 3, 2, 1, 4).
        app.state.project.channels.swap(1, 3);
        let step = |ch: u16, k: u32| Note::step(ch, k * 24);
        let sig = TimeMarker { kind: TimeMarker::SIGNATURE, numerator: 3, denominator: 4, ..TimeMarker::new(0, "3/4") };
        let color = Some(os_model::Rgb::new(0x45, 0x94, 0x6a));
        let p1 = Pattern {
            number: 1,
            color,
            notes: vec![step(1, 0), step(0, 1), step(3, 2), step(0, 3), step(1, 4)],
            controls: vec![ControlPoint::new(0, 0, ControlPoint::VOLUME, 9000)],
            loops: vec![(0, 8)],
            markers: vec![sig.clone()],
            ..Pattern::default()
        };
        let p2 = Pattern { number: 2, name: Some("Verse".into()), notes: vec![step(2, 0)], ..Pattern::default() };
        let p3 = Pattern { number: 3, notes: vec![step(2, 0), step(2, 4)], ..Pattern::default() };
        app.state.project.patterns = vec![p1.clone(), p2, p3];
        let mut cut = Clip::new(1536, 192, 2, ClipSource::Pattern(1));
        cut.window = Some((96.0, 288.0));
        let clips = vec![Clip::new(192, 384, 0, ClipSource::Pattern(1)), Clip::new(768, 384, 1, ClipSource::Pattern(2)), cut];
        pl::arr_mut(&mut app.state).clips = clips.clone();
        app.state.playlist.sel = vec![1];
        let before = (app.state.project.patterns.clone(), clips.clone());
        app.state.pattern = 1;
        let entry = MENUS[PATTERNS].cols.iter().flat_map(|(_, items)| items.iter()).position(|it| it.label == "Split by instrument").unwrap();
        assert!(works(PATTERNS, item_at(PATTERNS, entry).unwrap()));
        let popup = pattern_popup(0.0, 0.0);
        let it = popup.items.iter().find(|i| i.label == "Split by instrument").unwrap();
        assert!(it.enabled && it.cmd == Cmd::Pattern(PCmd::SplitByChannel), "also in the pattern menu");
        app.menu_item(PATTERNS, entry);
        let s = &app.state;
        let pat = |n: u16| s.project.pattern(n).unwrap();
        let summary = |n: u16| (pat(n).name.clone().unwrap_or_default(), pat(n).notes.iter().map(|x| (x.channel, x.position)).collect::<Vec<_>>());
        assert_eq!(summary(1), ("Kick".into(), vec![(0, 24), (0, 72)]));
        assert_eq!(summary(2), ("Snare".into(), vec![(3, 48)]));
        assert_eq!(summary(3), ("Clap".into(), vec![(1, 0), (1, 96)]));
        assert_eq!(summary(4), ("Verse".into(), vec![(2, 0)]), "later patterns move up");
        assert_eq!(pat(5).notes.len(), 2);
        for n in 2..=3 {
            let p = pat(n);
            assert_eq!((p.color, &p.markers, &p.loops, p.length, p.controls.is_empty()), (color, &p1.markers, &p1.loops, None, true), "pattern {n}");
        }
        assert_eq!((pat(1).color, &pat(1).controls, &pat(1).markers), (color, &p1.controls, &p1.markers));
        assert_eq!(s.pattern, 1, "the first stays current");
        // The playlist: one clip per new pattern at each clip of the original, just before it (the original on top).
        let at = |c: &Clip| (c.source, c.position, c.length, c.track, c.window);
        let want: Vec<_> = [(2, 0), (3, 0), (1, 0), (4, 1), (2, 2), (3, 2), (1, 2)]
            .into_iter()
            .map(|(p, k)| at(&Clip { source: ClipSource::Pattern(p), ..clips[k] }))
            .collect();
        assert_eq!(pl::clips(s).iter().map(at).collect::<Vec<_>>(), want);
        assert!(s.playlist.sel.is_empty(), "nothing selected");
        assert!(app.unsaved());
        // One undo step.
        assert_eq!(pl::run(&mut app, Cmd::Undo).as_deref(), Some("Undo: split by instrument"));
        assert_eq!((app.state.project.patterns.clone(), pl::clips(&app.state).to_vec()), before);
        // A pattern with one channel's notes is only renamed; one without notes stays as it is.
        app.state.pattern = 3;
        let n = app.state.history.at();
        app.pattern_cmd(PCmd::SplitByChannel);
        let s = &app.state;
        assert_eq!((s.project.patterns.len(), s.project.pattern(3).unwrap().name.as_deref(), pl::clips(s).len()), (3, Some("Hat"), 3));
        assert_eq!(s.history.at(), n + 1);
        app.state.pattern = 7;
        app.pattern_cmd(PCmd::SplitByChannel);
        assert_eq!((app.state.project.patterns.len(), app.state.history.at()), (3, n + 1));
    }

    /// TOOLS / the channel rack's options > Switch all / selected audio clip channels to ▸ (spec: ui/channel-switches.md
    /// §1, §6): the stretch methods in two columns; a pick asks "Confirm" first; Ok sets the method on the audio clip
    /// channels (all, or the selected ones) and their Lemon Sampler, keeping TIME, PITCH and MUL; Cancel changes nothing;
    /// with no channel concerned nothing is asked. Saving keeps the method.
    #[test]
    fn switch_audio_clip_channels_stretch_method() {
        use crate::dialog::DHit;
        use crate::playlist::{self as pl, Cmd, Mark};
        use os_model::stretch_mode as sm;
        use views::menus::{CLIPS_ALL_LABEL, CLIPS_SEL_LABEL, MENUS, TOOLS_MENU};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        let wav = std::env::temp_dir().join(format!("os-app-switch-{}.wav", std::process::id()));
        os_engine::wav::write_f32_stereo(&mut std::fs::File::create(&wav).unwrap(), 48000, &[0.5; 9600]).unwrap();
        pl::add_audio_clip(&mut app.state, &wav.to_string_lossy(), 0, 0).expect("audio clip added");
        pl::add_audio_clip(&mut app.state, &wav.to_string_lossy(), 1, 0).expect("audio clip added");
        let n = app.state.project.channels.len();
        let (a, b) = (n - 2, n - 1);
        // TIME 4 beats, PITCH +100 cents, MUL x1.19, Resample.
        let kept = [768f32.to_bits(), 100, 2540];
        app.state.project.channels[a].sound.stretch = [kept[0], kept[1], kept[2], 0];
        app.plugins = crate::plugins::Plugins::default();
        app.plugins.available = crate::plugins::builtin();
        app.sync_samplers(None);
        app.mark_saved();
        assert!(app.state.hosted_rows.contains(&a) && app.state.hosted_rows.contains(&b), "Lemon Sampler plays the audio clips");
        let sampler = |app: &App, row: usize| {
            let p = app.plugins.hosted[&row].plugin.lock().unwrap();
            let mut st = [0u32; 4];
            for i in (0..p.param_count()).filter_map(|i| p.param_info(i)) {
                crate::plugins::stretch_param(&i.name, p.param_value(i.id).unwrap(), &mut st);
            }
            st[3] as i32
        };
        let methods = |app: &App| app.state.project.channels.iter().map(|ch| ch.sound.stretch_mode()).collect::<Vec<_>>();
        // The submenu: the methods as audio/time-stretch.md names them, the second column from Auto.
        let labels: Vec<String> = crate::rack::stretch_items(true).iter().map(|i| if i.col { "|".into() } else { i.label.to_string() }).collect();
        let want = "Realtime Resample Stretch Stretch Pro Elastique e3 generic e3 mono Special Slice stretch Slice map | Auto Legacy e2 generic e2 transient e2 mono e2 speech";
        assert_eq!(labels.join(" "), want);
        let items = crate::rack::stretch_items(true);
        assert!(items.iter().all(|i| i.mark == Mark::None && (i.head || i.col || i.enabled)), "commands, no marks");
        let stored: Vec<i32> = items.iter().filter_map(|i| match i.cmd {
            Cmd::Rack(crate::rack::RCmd::StretchClips(true, m)) => Some(m),
            _ => None,
        }).collect();
        assert_eq!(stored, [0, -1, -2, 1, 2, 3, 4, 5, 6, 7, 8, 9], "event 215 int 27");
        // TOOLS > Switch all audio clip channels to > Stretch Pro asks first.
        let entry = MENUS[TOOLS_MENU].cols.iter().flat_map(|(_, items)| items.iter()).position(|it| it.label == CLIPS_ALL_LABEL).unwrap();
        let pick = |app: &mut App, c: &mut Canvas, label: &str| {
            frame(app, c, None);
            let tools = hit_rect(app, &Action::Menu(TOOLS_MENU));
            click(app, c, tools);
            let row = hit_rect(app, &Action::MenuSub(TOOLS_MENU, entry));
            click(app, c, row);
            let i = app.state.playlist.popup.as_ref().expect("the submenu").items.iter().position(|it| it.label == label).unwrap();
            let item = hit_rect(app, &Action::PlPopup(i));
            click(app, c, item);
        };
        pick(&mut app, &mut c, "Stretch Pro");
        let text = "All audio clip channels will be set to stretch pro mode. Some channels may need manual time adjustment.";
        assert_eq!(app.top_message(), Some(("Confirm".into(), text.into())));
        assert!(app.state.menu_open.is_none() && app.state.playlist.popup.is_none());
        let crate::dialog::Body::Msg(m) = &app.state.dialogs.last().unwrap().body else { panic!("a message box") };
        assert_eq!((m.kind, &m.buttons[..]), (crate::dialog::Kind::Warning, &[crate::dialog::Answer::Ok, crate::dialog::Answer::Cancel][..]));
        // Cancel changes nothing.
        let cancel = hit_rect(&app, &Action::Dlg(DHit::Button(1)));
        click(&mut app, &mut c, cancel);
        assert!(!app.modal_open() && !app.unsaved());
        assert_eq!((methods(&app)[a], methods(&app)[b]), (0, 0));
        // Ok sets it on both audio clip channels, their other stretch settings kept; the sampler channels keep theirs.
        pick(&mut app, &mut c, "Stretch Pro");
        app.state.piano.dirty = false;
        let ok = hit_rect(&app, &Action::Dlg(DHit::Button(0)));
        click(&mut app, &mut c, ok);
        assert!(!app.modal_open());
        assert_eq!(methods(&app), [0, 0, 0, 0, 0, sm::STRETCH_PRO, sm::STRETCH_PRO]);
        assert_eq!(app.state.project.channels[a].sound.stretch[..3], kept);
        assert!(app.unsaved() && app.state.piano.dirty, "a change to the project, played at once");
        assert_eq!((sampler(&app, a), sampler(&app, b), sampler(&app, 0)), (sm::STRETCH_PRO, sm::STRETCH_PRO, 0), "Lemon Sampler follows");
        // The rack's options menu: Switch selected... > e2 speech, on the selected audio clip channel only (Enter = Ok).
        app.state.rack.sel = [0, b].into();
        let rack = crate::rack::options_menu(&app.state, 0.0, 0.0);
        let all = rack.items.iter().find(|i| i.label == CLIPS_ALL_LABEL).expect("in the rack's options menu");
        assert_eq!(all.children, crate::rack::stretch_items(true));
        let sel = rack.items.iter().find(|i| i.label == CLIPS_SEL_LABEL).expect("in the rack's options menu");
        assert_eq!(sel.children, crate::rack::stretch_items(false));
        let speech = sel.children.iter().find(|i| i.label == "e2 speech").unwrap().cmd.clone();
        pl::run(&mut app, speech.clone());
        let text = "Selected audio clip channels will be set to e2 speech mode. Some channels may need manual time adjustment.";
        assert_eq!(app.top_message(), Some(("Confirm".into(), text.into())));
        app.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::Enter), false, false);
        assert!(!app.modal_open());
        assert_eq!(methods(&app), [0, 0, 0, 0, 0, sm::STRETCH_PRO, sm::E2_SPEECH]);
        assert_eq!(sampler(&app, b), sm::E2_SPEECH);
        // No audio clip channel selected: nothing is asked, nothing changes.
        app.state.rack.sel = [0, 1].into();
        pl::run(&mut app, speech);
        assert!(!app.modal_open());
        assert_eq!(methods(&app)[a], sm::STRETCH_PRO);
        // A project without audio clip channels: nothing is asked.
        let mut plain = App::new(None);
        plain.state.project = test_project();
        pl::run(&mut plain, Cmd::Rack(crate::rack::RCmd::StretchClips(true, sm::STRETCH)));
        assert!(!plain.modal_open());
        // Saving keeps the methods.
        let path = std::env::temp_dir().join(format!("os-app-switch-{}.flp", std::process::id()));
        app.save_to(path.clone());
        let back = os_flp::project::load(&os_flp::FlpFile::parse(&std::fs::read(&path).unwrap()).unwrap()).unwrap();
        let clips: Vec<[u32; 4]> = back.channels.iter().filter(|c| c.kind == ChannelKind::AudioClip).map(|c| c.sound.stretch).collect();
        assert_eq!(clips, [[kept[0], kept[1], kept[2], sm::STRETCH_PRO as u32], [0, 0, 0, sm::E2_SPEECH as u32]]);
        let _ = (std::fs::remove_file(&path), std::fs::remove_file(&wav));
    }

    /// PATTERNS > Set time signature... (spec: ui/pattern-time-signature.md): the "Time signature change" dialog opens at
    /// 4/4; its value boxes open the app's popup lists (Numerator 1..16, Denominator 2/4/8/16, the current value marked);
    /// Accept puts a time-signature marker named after it ("3/8") at the start of the current pattern, which the marker
    /// lanes show once and saving keeps. With one there already the conflict question comes first: No leaves it, Yes
    /// removes it and opens the dialog, which closed without Accept leaves no start marker. The pattern menu has the
    /// command too.
    #[test]
    fn pattern_time_signature() {
        use crate::dialog::{Answer, Body, DHit, Kind};
        use crate::playlist::{self as pl, Cmd, Mark};
        use crate::timesig::{form_open, list_open, SigForm};
        use os_model::TimeMarker;
        use views::menus::{item_at, works, MENUS, PATTERNS};
        use winit::keyboard::{Key, NamedKey};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.pattern = 1;
        app.state.project.patterns[0].markers = vec![TimeMarker::new(192, "Mid")];
        app.mark_saved();
        let entry = MENUS[PATTERNS].cols.iter().flat_map(|(_, items)| items.iter()).position(|it| it.label == "Set time signature...").unwrap();
        assert!(works(PATTERNS, item_at(PATTERNS, entry).unwrap()));
        let popup = views::menus::pattern_popup(0.0, 0.0);
        let item = popup.items.iter().find(|i| i.label == "Set time signature...").expect("in the pattern menu");
        assert!(item.enabled && item.cmd == Cmd::Pattern(crate::patterns::Cmd::TimeSignature));
        let form = |app: &App| match app.state.dialogs.last().map(|m| &m.body) {
            Some(Body::TimeSig(f)) => Some(*f),
            _ => None,
        };
        let start = |app: &App| app.state.project.pattern(1).unwrap().markers.iter().find(|m| m.position == 0 && m.kind == TimeMarker::SIGNATURE).cloned();
        // The dialog, at 4/4.
        app.menu_item(PATTERNS, entry);
        assert_eq!(form(&app), Some(SigForm { pattern: 1, num: 4, den: 4, song: None }));
        frame(&mut app, &mut c, None);
        // Numerator: 1..16 under the box, 4 marked; 3 picked.
        let num_box = hit_rect(&app, &Action::Dlg(DHit::SigValue(false)));
        click(&mut app, &mut c, num_box);
        assert_eq!(list_open(&app.state), Some(false));
        let p = app.state.playlist.popup.clone().unwrap();
        assert_eq!((p.x, p.y), (num_box.x, num_box.bottom() + 1.0), "under the box");
        let labels: Vec<String> = p.items.iter().map(|i| i.label.to_string()).collect();
        assert_eq!(labels, (1..=16).map(|n: u8| n.to_string()).collect::<Vec<_>>());
        let marked: Vec<String> = p.items.iter().filter(|i| i.mark == Mark::Radio(true)).map(|i| i.label.to_string()).collect();
        assert_eq!(marked, ["4"]);
        let three = hit_rect(&app, &Action::PlPopup(2));
        assert!(app.top_hit(three.cx(), three.cy()).is_some_and(|h| h.action == Action::PlPopup(2)), "the list is over the dialog");
        click(&mut app, &mut c, three);
        assert!(app.state.playlist.popup.is_none() && form_open(&app.state));
        assert_eq!(form(&app).map(|f| (f.num, f.den)), Some((3, 4)));
        // Denominator: 2, 4, 8, 16; 8 picked.
        let den_box = hit_rect(&app, &Action::Dlg(DHit::SigValue(true)));
        click(&mut app, &mut c, den_box);
        let p = app.state.playlist.popup.clone().unwrap();
        assert_eq!(p.items.iter().map(|i| i.label.to_string()).collect::<Vec<_>>(), ["2", "4", "8", "16"]);
        let eight = hit_rect(&app, &Action::PlPopup(2));
        click(&mut app, &mut c, eight);
        assert_eq!(form(&app).map(|f| (f.num, f.den)), Some((3, 8)));
        // A click beside an open list closes it; so does Esc, before it closes the dialog.
        click(&mut app, &mut c, num_box);
        click(&mut app, &mut c, Rect::new(5.0, 5.0, 2.0, 2.0));
        assert!(app.state.playlist.popup.is_none() && form_open(&app.state));
        click(&mut app, &mut c, num_box);
        app.key(&Key::Named(NamedKey::Escape), false, false);
        assert!(app.state.playlist.popup.is_none() && form_open(&app.state));
        assert_eq!(form(&app).map(|f| (f.num, f.den)), Some((3, 8)));
        assert!(start(&app).is_none() && !app.unsaved(), "nothing yet");
        // Accept: a 3/8 marker at the pattern's start, before its other markers.
        app.state.piano.dirty = false;
        frame(&mut app, &mut c, None);
        let accept = hit_rect(&app, &Action::Dlg(DHit::Accept));
        click(&mut app, &mut c, accept);
        assert!(!app.modal_open());
        let want = TimeMarker { kind: TimeMarker::SIGNATURE, numerator: 3, denominator: 8, ..TimeMarker::new(0, "3/8") };
        assert_eq!(app.state.project.pattern(1).unwrap().markers, [want.clone(), TimeMarker::new(192, "Mid")]);
        assert!(app.unsaved() && app.state.piano.dirty);
        assert_eq!(crate::views::playlist::marker_label(&want), "3/8", "shown once in the marker lanes");
        // Saved with the pattern.
        let path = std::env::temp_dir().join(format!("os-app-timesig-{}.flp", std::process::id()));
        app.save_to(path.clone());
        let back = os_flp::project::load(&os_flp::FlpFile::parse(&std::fs::read(&path).unwrap()).unwrap()).unwrap();
        assert_eq!(back.pattern(1).unwrap().markers, [want.clone(), TimeMarker::new(192, "Mid")]);
        let _ = std::fs::remove_file(&path);
        // Again, from the pattern menu: the conflict question; No leaves the marker.
        pl::run(&mut app, Cmd::Pattern(crate::patterns::Cmd::TimeSignature));
        let text = "The pattern already has a time signature defined at the start. Do you want replace it?";
        assert_eq!(app.top_message(), Some(("Time signature conflict".into(), text.into())));
        let Some(Body::Msg(m)) = app.state.dialogs.last().map(|m| &m.body) else { panic!("a message box") };
        assert_eq!((m.kind, &m.buttons[..]), (Kind::Question, &[Answer::Yes, Answer::No][..]));
        frame(&mut app, &mut c, None);
        let no = hit_rect(&app, &Action::Dlg(DHit::Button(1)));
        click(&mut app, &mut c, no);
        assert!(!app.modal_open());
        assert_eq!(start(&app), Some(want.clone()));
        // Yes: the marker goes at once and the dialog opens at 4/4; closed with its × it leaves no start marker.
        app.menu_item(PATTERNS, entry);
        app.key(&Key::Character("y".into()), false, false);
        assert_eq!(form(&app), Some(SigForm { pattern: 1, num: 4, den: 4, song: None }));
        assert!(start(&app).is_none());
        frame(&mut app, &mut c, None);
        let close = hit_rect(&app, &Action::Dlg(DHit::Close));
        click(&mut app, &mut c, close);
        assert!(!app.modal_open());
        assert_eq!(app.state.project.pattern(1).unwrap().markers, [TimeMarker::new(192, "Mid")]);
        // No marker: straight to the dialog; Enter accepts. Another pattern gets its own.
        app.state.pattern = 2;
        app.menu_item(PATTERNS, entry);
        assert_eq!(form(&app), Some(SigForm { pattern: 2, num: 4, den: 4, song: None }));
        app.key(&Key::Named(NamedKey::Enter), false, false);
        assert!(!app.modal_open());
        let four = TimeMarker { kind: TimeMarker::SIGNATURE, ..TimeMarker::new(0, "4/4") };
        assert_eq!(app.state.project.pattern(2).unwrap().markers, [four]);
        assert!(start(&app).is_none(), "pattern 1 keeps none");
    }

    /// A pattern's length follows its own time signature (spec: audio/song-structure.md, "Time signatures and a
    /// pattern's length"; ui/pattern-time-signature.md §2, §4-5): the table's rows, the same in the playlist and the
    /// piano roll, the channel rack's length still first. Set time signature... names its marker "3/8", shown once (the
    /// playlist's "4/4" markers too); the pattern's clips that kept their natural length take the new one, and the old
    /// one back when Yes takes the marker away; clips resized or cut by hand keep theirs. The piano roll's bars and
    /// beats follow the pattern's signature: 3/8, a bar every 6 steps.
    #[test]
    fn pattern_length_follows_its_time_signature() {
        use crate::piano::{pattern_length, Geom};
        use crate::playlist::{self as pl, Cmd};
        use os_model::{Clip, ClipSource, TimeMarker};
        use views::playlist::marker_label;
        use winit::keyboard::{Key, NamedKey};
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.pattern = 1;
        let note = |end: u32| Note { length: 100, ..Note::step(0, end - 100) };
        let sig = |pos: u32, num: u8, den: u8| TimeMarker { kind: TimeMarker::SIGNATURE, numerator: num, denominator: den, ..TimeMarker::new(pos, &format!("{num}/{den}")) };
        let len = |app: &App| {
            let (playlist, piano) = (pl::source_len(&app.state, ClipSource::Pattern(1)), pattern_length(&app.state, 1));
            assert_eq!(playlist, piano, "the playlist and the piano roll agree");
            playlist
        };
        let set = |app: &mut App, project: (u16, u16), arr: Vec<TimeMarker>, own: Vec<TimeMarker>, end: u32| {
            app.state.project.bar = Some((project.0, 16 / project.1));
            pl::arr_mut(&mut app.state).markers = arr;
            let p = &mut app.state.project.patterns[0];
            (p.notes, p.markers, p.length) = (vec![note(end)], own, None);
        };
        // The table: 96 PPQ, the last note ends at 300; the clip at 768.
        let rows = [
            ((4, 4), vec![], vec![], 384),
            ((3, 4), vec![], vec![], 576),
            ((4, 4), vec![sig(0, 3, 4)], vec![], 384),
            ((4, 4), vec![sig(768, 3, 4)], vec![], 384),
            ((4, 4), vec![], vec![sig(0, 3, 4)], 576),
            ((4, 4), vec![], vec![sig(192, 3, 4)], 480),
        ];
        pl::arr_mut(&mut app.state).clips = vec![Clip::new(768, 384, 0, ClipSource::Pattern(1))];
        for (i, (project, arr, own, want)) in rows.into_iter().enumerate() {
            set(&mut app, project, arr, own, 300);
            assert_eq!(len(&app), want, "row {}", i + 1);
        }
        set(&mut app, (4, 4), vec![], vec![sig(0, 3, 4)], 500);
        assert_eq!(len(&app), 576, "a note ending at 500 with the 3/4 marker at 0");
        set(&mut app, (4, 4), vec![], vec![sig(0, 3, 4)], 384);
        assert_eq!(len(&app), 576, "at least one bar");
        set(&mut app, (4, 4), vec![], vec![], 384);
        assert_eq!(len(&app), 384, "a note ending on a bar line");
        app.state.project.patterns[0].length = Some(192);
        assert_eq!(len(&app), 192, "the channel rack's length wins");
        // The piano roll's grid at 3/4 from tick 192: 4/4 bars before it, a bar line at it, 3/4 bars after it.
        set(&mut app, (4, 4), vec![], vec![sig(192, 3, 4)], 300);
        let g = Geom::new(&app.state, Rect::new(0.0, 0.0, 800.0, 400.0));
        assert_eq!(g.bar_lines(1000.0), [(0, 1), (192, 2), (480, 3), (768, 4)]);
        assert_eq!([96, 192, 288, 384, 480].map(|t| g.line_at(t)), [(false, true), (true, true), (false, true), (false, true), (true, true)]);
        // Set time signature... 3/8 on a 4/4 pattern with clips: natural length (in both arrangements), resized and cut
        // by hand, and another pattern's.
        set(&mut app, (4, 4), vec![], vec![], 300);
        let natural = Clip::new(0, 384, 0, ClipSource::Pattern(1));
        let resized = Clip::new(384, 768, 1, ClipSource::Pattern(1));
        let cut = Clip { window: Some((96.0, 480.0)), ..Clip::new(1536, 384, 2, ClipSource::Pattern(1)) };
        let other = Clip::new(0, 384, 3, ClipSource::Pattern(2));
        pl::arr_mut(&mut app.state).clips = vec![natural, resized, cut, other];
        let mut second = app.state.project.arrangements[0].clone();
        second.clips = vec![natural];
        app.state.project.arrangements.push(second);
        let lengths = |app: &App| app.state.project.arrangements.iter().map(|a| a.clips.iter().map(|c| c.length).collect::<Vec<_>>()).collect::<Vec<_>>();
        assert_eq!(len(&app), 384);
        pl::run(&mut app, Cmd::Pattern(crate::patterns::Cmd::TimeSignature));
        app.sig_pick(false, 3);
        app.sig_pick(true, 8);
        app.key(&Key::Named(NamedKey::Enter), false, false);
        assert!(!app.modal_open());
        let marker = &app.state.project.pattern(1).unwrap().markers[0];
        assert_eq!((marker, marker.name.as_str(), marker_label(marker).as_str()), (&sig(0, 3, 8), "3/8", "3/8"));
        assert_eq!(len(&app), 432, "three bars of 144");
        assert_eq!(lengths(&app), [vec![432, 768, 384, 384], vec![432]]);
        let g = Geom::new(&app.state, Rect::new(0.0, 0.0, 800.0, 400.0));
        assert_eq!((g.step, g.beat, g.bar), (24, 48, 144));
        let bars: Vec<u32> = (0..=12).filter(|&i| g.line_at(i * 24).0).collect();
        let beats: Vec<u32> = (0..=12).filter(|&i| g.line_at(i * 24).1).collect();
        assert_eq!((bars, beats), (vec![0, 6, 12], vec![0, 2, 4, 6, 8, 10, 12]), "a bar every 6 steps, a beat every 2");
        assert_eq!(g.bar_lines(500.0), [(0, 1), (144, 2), (288, 3), (432, 4)]);
        // An empty 3/8 pattern is one 3/8 bar (§4).
        app.state.project.patterns[0].notes.clear();
        assert_eq!(len(&app), 144);
        app.state.project.patterns[0].notes = vec![note(300)];
        // Yes to the conflict question takes the marker away at once: back to 4/4 bars and lengths.
        pl::run(&mut app, Cmd::Pattern(crate::patterns::Cmd::TimeSignature));
        app.key(&Key::Character("y".into()), false, false);
        app.key(&Key::Named(NamedKey::Escape), false, false);
        assert!(!app.modal_open() && app.state.project.pattern(1).unwrap().markers.is_empty());
        assert_eq!(len(&app), 384);
        assert_eq!(lengths(&app), [vec![384, 768, 384, 384], vec![384]]);
        // The playlist's own time-signature markers ("4/4", from the same dialog) are shown once too; they leave the
        // pattern's length alone.
        pl::run(&mut app, Cmd::AddSignature);
        assert!(crate::timesig::form_open(&app.state));
        app.key(&Key::Named(NamedKey::Enter), false, false);
        let m = pl::arr_mut(&mut app.state).markers.last().cloned().unwrap();
        assert_eq!((m.name.as_str(), marker_label(&m).as_str()), ("4/4", "4/4"));
        assert_eq!(len(&app), 384);
        // A named signature marker still shows its name after the signature.
        assert_eq!(marker_label(&TimeMarker { name: "Verse".into(), ..sig(0, 3, 4) }), "3/4 Verse");
        // The piano roll draws the 3/8 grid.
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        crate::timesig::set_start(&mut app.state, 1, 3, 8);
        frame(&mut app, &mut c, None);
    }

    /// The ▸ menu's Quantize song jumps (spec: ui/playlist.md §7a): Off by default, each entry a radio; the unit is a
    /// beat or 1, 2, 4 or 8 of the project's bars.
    #[test]
    fn playlist_quantize_song_jumps() {
        use crate::playlist::Mark;
        let mut app = App::new(None);
        let items = |app: &App| {
            let m = crate::playlist::options_menu(&app.state, 0.0, 0.0);
            m.items.into_iter().find(|i| i.label == "Quantize song jumps").unwrap().children
        };
        let labels: Vec<String> = items(&app).iter().map(|i| i.label.to_string()).collect();
        assert_eq!(labels, ["Off", "Beats", "Bars", "2 bars", "4 bars", "8 bars"]);
        assert!(items(&app).iter().all(|i| i.enabled));
        assert_eq!(items(&app)[0].mark, Mark::Radio(true));
        assert_eq!(crate::playlist::jump_unit(&app.state), None);
        let ppq = u32::from(app.state.project.ppq);
        for (k, unit) in [(1, ppq), (2, ppq * 4), (5, ppq * 32)] {
            let c = items(&app)[k].cmd.clone();
            crate::playlist::run(&mut app, c);
            assert_eq!(items(&app)[k].mark, Mark::Radio(true));
            assert_eq!(crate::playlist::jump_unit(&app.state), Some(unit));
        }
    }

    /// The playlist's time markers (spec: ui/playlist.md §7a): the ▸ menu's Time markers items and the marker menus,
    /// what they add, Move content (FL's measured case), the actions and the time signature dialog, Delete in the
    /// time selection, undo, position order, the song position on a click and on opening.
    #[test]
    fn playlist_time_markers() {
        use crate::playlist::{self as pl, Cmd, Mark, PlDrag};
        use os_model::{Arrangement, Clip, ClipSource, TimeMarker as M};
        use winit::keyboard::{Key, NamedKey};
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.project.arrangements = vec![Arrangement { clips: (0..5).map(|i| Clip::new(384 * i, 384, 0, ClipSource::Pattern(1))).collect(), ..Default::default() }];
        pl::arr_mut(&mut app.state);
        let marks = |app: &App| pl::markers(&app.state).iter().map(|m| (m.position, m.kind, m.name.clone())).collect::<Vec<_>>();
        let mk = |p: u32, k: u8, n: &str| (p, k, n.to_string());
        let set = |app: &mut App, list: &[(u32, u8, &str)]| {
            pl::arr_mut(&mut app.state).markers = list.iter().map(|&(p, k, n)| M { kind: k, ..M::new(p, n) }).collect();
        };

        // The ▸ menu's submenu in FL's order; Add two... is greyed without a time selection, as in FL.
        let menu = pl::options_menu(&app.state, 0.0, 0.0);
        let sub = &menu.items.iter().find(|it| it.label == "Time markers").expect("Time markers").children;
        let want = [
            ("Add one...", true),
            ("Add auto", true),
            ("Add two...", false),
            ("Add one every", true),
            ("Add jump to next bar", true),
            ("Add time signature change...", true),
            ("Add key marker", true),
            ("Place loop", true),
            ("Delete", true),
            ("Change to marker loop", true),
        ];
        assert_eq!(sub.iter().map(|it| (&*it.label, it.enabled)).collect::<Vec<_>>(), want);
        assert!(menu.items.iter().any(|it| it.label == "Quantize song jumps" && it.enabled && it.children.len() == 6));
        pl::run(&mut app, Cmd::MarkerEveryMenu);
        let every = app.state.playlist.popup.take().expect("its list");
        assert_eq!(every.items.iter().map(|it| it.cmd.clone()).collect::<Vec<_>>(), [1, 2, 4, 8].map(Cmd::MarkerEvery));
        app.state.playlist.range = Some((0, 384));
        let menu = pl::options_menu(&app.state, 0.0, 0.0);
        let sub = &menu.items.iter().find(|it| it.label == "Time markers").expect("Time markers").children;
        assert!(sub[2].label == "Add two..." && sub[2].enabled, "enabled with a time selection");
        app.state.playlist.range = None;

        // Add auto and Add jump to next bar, at the song position, kept in position order; each one undo step.
        let steps = app.state.history.len();
        app.state.playlist.pos = 768;
        pl::run(&mut app, Cmd::AddAutoMarker);
        app.state.playlist.pos = 384;
        pl::run(&mut app, Cmd::AddJumpMarker);
        assert_eq!(marks(&app), [mk(384, M::SKIP, " "), mk(768, M::PLAIN, "Auto"), mk(768, M::PLAIN, " ")]);
        assert_eq!(app.state.history.len(), steps + 2);
        pl::run(&mut app, Cmd::Undo);
        assert_eq!(marks(&app), [mk(768, M::PLAIN, "Auto")]);

        // Place loop: at the time selection's start, at 0 without one; a second time takes it away.
        set(&mut app, &[]);
        pl::run(&mut app, Cmd::PlaceLoop);
        assert_eq!(marks(&app), [mk(0, M::LOOP, " ")]);
        pl::run(&mut app, Cmd::PlaceLoop);
        assert_eq!(marks(&app), []);
        app.state.playlist.range = Some((792, 1560));
        pl::run(&mut app, Cmd::PlaceLoop);
        assert_eq!(marks(&app), [mk(792, M::LOOP, " ")]);

        // Add one every bar over the selection, as FL measured: "Part #1" at 792, "Part #2" at 1176.
        set(&mut app, &[]);
        pl::run(&mut app, Cmd::MarkerEvery(1));
        app.state.playlist.edit.as_mut().expect("the base name box").text = "Part".into();
        pl::commit_edit(&mut app.state);
        assert_eq!(marks(&app), [mk(792, M::PLAIN, "Part #1"), mk(1176, M::PLAIN, "Part #2")]);

        // Delete and Change to marker loop: the markers in the selection (start in, end out); all without one.
        set(&mut app, &[(384, M::PLAIN, "A"), (768, M::START, "B"), (1536, M::PLAIN, "C")]);
        app.state.playlist.range = Some((384, 1536));
        pl::run(&mut app, Cmd::DeleteMarkers);
        assert_eq!(marks(&app), [mk(1536, M::PLAIN, "C")]);
        set(&mut app, &[(384, M::PLAIN, "A"), (768, M::START, "B")]);
        app.state.playlist.range = None;
        pl::run(&mut app, Cmd::ToMarkerLoop);
        assert_eq!(marks(&app), [mk(384, M::MARKER_LOOP, "A"), mk(768, M::MARKER_LOOP, "B")]);

        // Move content right on B (FL: A 384, B 768, C 1536, clips to 1920): B's section and C's trade places.
        set(&mut app, &[(384, M::PLAIN, "A"), (768, M::PLAIN, "B"), (1536, M::PLAIN, "C")]);
        let before = app.state.project.arrangements[0].clone();
        pl::run(&mut app, Cmd::MoveContent(1, true));
        let starts = |app: &App| pl::clips(&app.state).iter().map(|c| c.position).collect::<Vec<_>>();
        assert_eq!(starts(&app), [0, 384, 1152, 1536, 768]);
        assert_eq!(marks(&app), [mk(384, M::PLAIN, "A"), mk(768, M::PLAIN, "C"), mk(1152, M::PLAIN, "B")]);
        // Move content left on B is its mirror: everything is back.
        pl::run(&mut app, Cmd::MoveContent(2, false));
        assert_eq!(app.state.project.arrangements[0], before);
        // Nothing to trade with: no change, no undo step.
        let steps = app.state.history.len();
        pl::run(&mut app, Cmd::MoveContent(2, true));
        pl::run(&mut app, Cmd::MoveContent(0, false));
        assert_eq!((&app.state.project.arrangements[0], app.state.history.len()), (&before, steps));
        // One undo step takes a move back.
        pl::run(&mut app, Cmd::MoveContent(1, true));
        pl::run(&mut app, Cmd::Undo);
        assert_eq!(app.state.project.arrangements[0], before);

        // The marker menu: the row's items at the clicked tick, the header, then the actions with the marker's own.
        let m = pl::marker_menu(&app.state, 0, 400, 0.0, 0.0);
        let labels: Vec<&str> = m.items.iter().map(|it| &*it.label).collect();
        assert_eq!(labels[..10], ["Add marker...", "Add time signature...", "Place loop", "A at 2:01:00", "Delete", "Rename...", "", "Move content left", "Move content right", "Action"]);
        assert_eq!(labels[10..], ["None", "Start", "Loop", "Marker loop", "Marker skip", "Marker pause", "Time signature", "Start recording", "Stop recording"]);
        assert!(m.items[3].head && m.items[2].cmd == Cmd::PlaceLoopAt(400));
        assert_eq!(m.items[10..].iter().filter(|it| it.mark == Mark::Radio(true)).count(), 1);
        assert_eq!(m.items[10].mark, Mark::Radio(true), "A is a plain marker");
        assert_eq!(pl::lane_menu(400, 0.0, 0.0).items.len(), 3);

        // An action changes the kind only; Time signature asks first and names the marker after the signature.
        pl::run(&mut app, Cmd::MarkerAction(1, M::SKIP));
        assert_eq!(marks(&app)[1], mk(768, M::SKIP, "B"));
        pl::run(&mut app, Cmd::MarkerAction(1, M::SIGNATURE));
        assert!(crate::timesig::form_open(&app.state));
        assert_eq!(marks(&app)[1].1, M::SKIP, "not before Accept");
        app.key(&Key::Named(NamedKey::Enter), false, false);
        assert_eq!(marks(&app)[1], mk(768, M::SIGNATURE, "4/4"));
        // Add time signature... from the row's menu: a new one at its tick.
        pl::run(&mut app, Cmd::MarkerSigAt(1152));
        app.key(&Key::Named(NamedKey::Enter), false, false);
        assert_eq!(marks(&app)[2], mk(1152, M::SIGNATURE, "4/4"));

        // A click on a marker moves the song position to it, and leaves no undo step.
        set(&mut app, &[(384, M::PLAIN, "A"), (768, M::START, "B")]);
        app.state.playlist.pos = 0;
        let grid = Rect::new(300.0, 200.0, 1200.0, 600.0);
        let g = pl::PGeom::new(&app.state, grid);
        app.mouse = (g.x_of(768.0) + 4.0, 190.0);
        let steps = app.state.history.len();
        pl::press_markers(&mut app, grid, false);
        assert_eq!(app.state.playlist.pos, 768);
        let Some(Drag::Pl(d @ PlDrag::Marker { .. })) = app.drag.take() else { panic!("a marker drag") };
        pl::drag_end(&mut app, d, grid);
        assert_eq!(app.state.history.len(), steps);
        // A right click away from the markers: the row's menu at the snapped tick.
        app.mouse = (g.x_of(1100.0), 190.0);
        pl::press_markers(&mut app, grid, true);
        assert!(app.state.playlist.popup.take().is_some_and(|p| p.items.len() == 3));

        // An opened project's song position is at its Start marker.
        app.state.playlist.pos = 0;
        pl::load_view(&mut app.state);
        assert_eq!(app.state.playlist.pos, 768);

        // Add two...: the base-name box, then FL's "<base> -  start" / "<base> -  end" (two spaces) at the time
        // selection's ends (measured: bars 7 to 9 gave 2304 and 3072); one undo step.
        set(&mut app, &[]);
        app.state.playlist.range = Some((2304, 3072));
        let steps = app.state.history.len();
        pl::run(&mut app, Cmd::AddTwoMarkers);
        app.state.playlist.edit.as_mut().expect("the base name box").text = "Sec".into();
        pl::commit_edit(&mut app.state);
        assert_eq!(marks(&app), [mk(2304, M::PLAIN, "Sec -  start"), mk(3072, M::PLAIN, "Sec -  end")]);
        assert_eq!(app.state.history.len(), steps + 1);
        // Without a selection it does nothing.
        app.state.playlist.range = None;
        pl::run(&mut app, Cmd::AddTwoMarkers);
        assert!(app.state.playlist.edit.is_none());
        app.state.playlist.range = Some((2304, 3072));
        pl::run(&mut app, Cmd::AddTwoMarkers);
        app.state.playlist.edit.as_mut().expect("the box").text = String::new();
        pl::commit_edit(&mut app.state);
        assert_eq!(marks(&app)[1].2, "Marker -  start");
        app.state.playlist.range = None;

        // Every kind's tab paints, with the Start marker's darker pre-roll.
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let kinds = [M::PLAIN, M::MARKER_LOOP, M::SKIP, M::PAUSE, M::LOOP, M::START, M::SIGNATURE, M::PUNCH_IN, M::PUNCH_OUT, M::KEY];
        pl::arr_mut(&mut app.state).markers = kinds.iter().enumerate().map(|(i, &k)| M { kind: k, ..M::new(192 * i as u32, "Name") }).collect();
        frame(&mut app, &mut c, None);
    }

    /// Add key marker ▸ (spec: ui/playlist.md §7a, flp/events/playlist.md "Key markers"): FL's two columns, a root
    /// that keeps the list open, and key markers at the song position with the events FL saved for them.
    #[test]
    fn playlist_key_markers() {
        use crate::playlist::{self as pl, Cmd, Mark};
        use os_model::{Arrangement, TimeMarker as M};
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.project.arrangements = vec![Arrangement::default()];
        pl::arr_mut(&mut app.state);
        // The list: Root note C … B (C checked), a column break, Scale: Default, Black notes (checked), the 37 scales.
        pl::run(&mut app, Cmd::KeyMarkerMenu);
        let list = app.state.playlist.popup.clone().expect("the list opens");
        let labels: Vec<&str> = list.items.iter().map(|it| &*it.label).collect();
        assert_eq!(labels[..3], ["Root note", "C", "C#"]);
        assert_eq!(labels[13..18], ["", "Scale", "Default", "Black notes", ""]);
        assert!(list.items[13].col && list.items[17].sep && list.items.len() == 18 + 37);
        assert_eq!((labels[18], labels[54]), ("Major (Ionian)", "Other Whole Tone"));
        assert_eq!((list.items[1].mark, list.items[16].mark), (Mark::Radio(true), Mark::Check(true)));
        // It paints in two columns: the scales start right of the root notes, at the top.
        if let Ok(mut c) = Canvas::new(1920, 1080, 1.0) {
            app.state.playlist.popup.as_mut().expect("open").y = 100.0;
            frame(&mut app, &mut c, None);
            let (root, scale) = (hit_rect(&app, &Action::PlPopup(1)), hit_rect(&app, &Action::PlPopup(15)));
            assert!(scale.x > root.x + root.w - 1.0 && scale.y < root.y + root.h, "{root:?} {scale:?}");
        }
        // A root note keeps the list open where it was.
        let was = app.state.playlist.popup.as_ref().map(|p| (p.x, p.y));
        app.mouse = (900.0, 900.0);
        pl::run(&mut app, Cmd::KeyRoot(2));
        let again = app.state.playlist.popup.clone().expect("still open");
        assert_eq!((Some((again.x, again.y)), again.items[3].mark), (was, Mark::Radio(true)));
        // A scale adds the marker at the song position and closes the list; one undo step.
        app.state.playlist.pos = 1536;
        let steps = app.state.history.len();
        pl::run(&mut app, Cmd::AddKeyMarker(0));
        assert!(app.state.playlist.popup.is_none());
        assert_eq!(app.state.history.len(), steps + 1);
        let m = pl::markers(&app.state)[0].clone();
        assert_eq!((m.position, m.kind, m.name.as_str(), m.root, m.flag, m.scale_index, m.scale), (1536, M::KEY, "D Major (Ionian)", 2, 0, 0, 0x94A));
        assert_eq!(pl::key_items(&app.state)[18].mark, Mark::Check(true), "the scale added is checked");
        // FL's saves, each marker's events 46, 45, 168 and 101.
        let ev = |root: u8, k: i32| {
            let m = M::key(0, root, k);
            (m.name, m.root, m.flag, m.scale_index, m.scale)
        };
        assert_eq!(ev(0, 0), ("C Major (Ionian)".into(), 0, 0, 0, 1322));
        assert_eq!(ev(1, 0), ("C# Major (Ionian)".into(), 1, 0, 0, 661));
        assert_eq!(ev(4, 0), ("E Major (Ionian)".into(), 4, 0, 0, 2642));
        assert_eq!(ev(5, 0), ("F Major (Ionian)".into(), 5, 0, 0, 1321));
        assert_eq!(ev(0, 10), ("C Minor Natural (Aeolian)".into(), 0, 0, 10, 1189));
        assert_eq!(ev(11, 36), ("B Other Whole Tone".into(), 11, 0, 36, 2730));
        assert_eq!(ev(0, 15), ("C Other Arabic".into(), 0, 0, 15, 806));
        assert_eq!(ev(2, -2), ("Black notes".into(), 0, 0, -2, 1322));
        assert_eq!(ev(2, -1), ("Default".into(), 2, 1, -1, 4095));
        // Undo takes it away.
        pl::run(&mut app, Cmd::Undo);
        assert!(pl::markers(&app.state).is_empty());
    }

    /// Settings > Plugins (spec: ui/plugin-manager.md §2, §4): Add folder... keeps the folder and scans it in the
    /// background; a file that is no plugin shows in the table as an error; ★ and Show change the menus; the Find box
    /// filters; a folder switches off and goes.
    #[test]
    fn plugin_manager_folders_scan_and_table() {
        use crate::views::settings::{Plug, SAct, COMBINE_CHECK, PLUGINS};
        use winit::keyboard::{Key, NamedKey};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.plugins = crate::plugins::Plugins::builtins();
        app.state.plugdb.rebuild(&app.plugins);
        app.open_settings(PLUGINS);
        frame(&mut app, &mut c, None);
        let finish = |app: &mut App| {
            let t0 = std::time::Instant::now();
            while app.sweep.is_some() && t0.elapsed() < std::time::Duration::from_secs(20) {
                std::thread::sleep(std::time::Duration::from_millis(5));
                app.scan_tick();
            }
            assert!(app.sweep.is_none(), "the scan ends");
        };
        let dir = std::env::temp_dir().join(format!("os_plugin_folder_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("Broken.clap"), b"not a plugin").unwrap();
        crate::dialog::fake::answer(Some(dir.clone()));
        let r = hit_rect(&app, &Action::Settings(SAct::Button("Add folder...")));
        click(&mut app, &mut c, r);
        assert!(app.state.plugdb.prefs.folders.iter().any(|f| std::path::Path::new(&f.path) == dir), "the folder is kept");
        assert!(app.sweep.is_some(), "adding a folder scans");
        finish(&mut app);
        frame(&mut app, &mut c, None);
        let row = app.state.plugdb.rows.iter().find(|r| r.file == "Broken.clap").expect("the failed file has a row");
        assert_eq!(row.status, "error");
        assert!(!row.toggles);
        assert!(app.state.message.as_deref().is_some_and(|m| m.contains("1 file with errors")), "{:?}", app.state.message);
        // Star, then hide, an effect (one no scan found: as if listed before the scan).
        app.plugins.effects.push(crate::plugin_db::fake("Test FX", os_host::Kind::Effect));
        app.state.plugdb.rebuild(&app.plugins);
        let row_of = |app: &App| app.state.plugdb.rows.iter().position(|r| r.name == "Test FX").unwrap();
        // Scrolled to its row: the built-in plugins alone can fill the table.
        app.state.plugdb.top = row_of(&app);
        frame(&mut app, &mut c, None);
        let r = hit_rect(&app, &Action::Settings(SAct::Plug(Plug::Star(row_of(&app)))));
        click(&mut app, &mut c, r);
        assert!(app.state.plugdb.lists.fx.iter().any(|e| e.name == "Test FX" && e.favorite), "starred");
        let r = hit_rect(&app, &Action::Settings(SAct::Plug(Plug::Show(row_of(&app)))));
        click(&mut app, &mut c, r);
        assert!(!app.state.plugdb.lists.fx.iter().any(|e| e.name == "Test FX"), "hidden plugins leave the menus");
        // Combining formats switches off.
        let r = hit_rect(&app, &Action::Settings(SAct::Check(COMBINE_CHECK)));
        click(&mut app, &mut c, r);
        assert!(!app.state.plugdb.prefs.combine);
        // The Find box: typing filters, Esc clears it and keeps the window open.
        let r = hit_rect(&app, &Action::Settings(SAct::Plug(Plug::Find)));
        click(&mut app, &mut c, r);
        for ch in ["b", "R", "o"] {
            app.key(&Key::Character(ch.into()), false, false);
        }
        assert_eq!(app.state.plugdb.rows.len(), 1, "only Broken.clap matches");
        app.key(&Key::Named(NamedKey::Escape), false, false);
        assert!(app.state.plugdb.find.is_empty() && !app.state.plugdb.finding && app.state.settings.open);
        assert!(app.state.plugdb.rows.len() > 1);
        // The folder: off (scans again), then removed.
        frame(&mut app, &mut c, None);
        let k = crate::plugin_db::search_dirs(&app.state.plugdb.prefs).iter().position(|d| d.path == dir).unwrap();
        let r = hit_rect(&app, &Action::Settings(SAct::Plug(Plug::Folder(k))));
        click(&mut app, &mut c, r);
        assert!(app.state.plugdb.prefs.folders.iter().all(|f| !f.on));
        finish(&mut app);
        assert!(!app.state.plugdb.cache.files.iter().any(|f| f.path.ends_with("Broken.clap")), "a skipped folder's files drop out");
        frame(&mut app, &mut c, None);
        let r = hit_rect(&app, &Action::Settings(SAct::Plug(Plug::Remove(k))));
        click(&mut app, &mut c, r);
        assert!(app.state.plugdb.prefs.folders.is_empty());
        finish(&mut app);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// VST2 plugins through the app's own paths, with real plugins: `OS_VST2_DIR` is a folder of the DPF-Plugins
    /// release (Kars and 3BandEQ, each also there as VST3 and CLAP), `OS_VST2_DIR32` (optional) the release's 32-bit
    /// folder, `OS_SHOT_DIR` (optional) gets pictures of Settings > Plugins, `OS_VST2_KEEP` (optional) a copy of the saved
    /// project (for opening in other software).
    /// - Settings > Plugins (spec: ui/plugin-manager.md §2, §4): both folders added with Add folder... and scanned, the
    ///   32-bit VST2 plugins listed as failed with that reason, VST2 picked as the preferred format.
    /// - Kars added from the rack's Add one ▸ menu, 3BandEQ from its insert's slot menu, an automation clip made on the
    ///   EQ's master gain and drawn rising: a song export of eight C5 notes has each note at its beat, at its pitch and
    ///   louder than the one before. Played live (with an audio device), then stopped: silent at once.
    /// - Saved, reopened (both found as VST2 with their state, the clip kept) and exported again; then opened with only
    ///   the VST3 and CLAP copies listed: those copies play them, from their own defaults, and the hint bar says so.
    ///   Saved so, the file keeps both VST2 plugins byte for byte, also after an export that played the copies and
    ///   automated the EQ's; once the user edits Kars' copy (the parameter menu's Paste value), it is saved as that copy.
    ///
    /// Loads the plugins in this process, so ignored:
    /// `OS_VST2_DIR=<dir> cargo test -p os-app --bin lemonstudio vst2_plugins_through_the_app -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn vst2_plugins_through_the_app() {
        use crate::playlist::{self as pl, Cmd};
        use crate::plugin_menu::PlugCmd;
        use crate::plugins::fx_key;
        use crate::views::settings::{Plug, SAct, PLUGINS};
        use os_host::Format;
        use std::time::Duration;
        use winit::keyboard::{Key, NamedKey};
        let Some(dir) = std::env::var_os("OS_VST2_DIR").map(PathBuf::from) else { return };
        let dir32 = std::env::var_os("OS_VST2_DIR32").map(PathBuf::from);
        let shots = std::env::var_os("OS_SHOT_DIR").map(PathBuf::from);
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut a = App::new(None);
        a.state.project = test_project();
        a.plugins = crate::plugins::Plugins::builtins();
        let rate = crate::transport::Transport::device_rate().unwrap_or(48000);
        a.state.plugdb.rebuild(&a.plugins);
        // Only the added folders are scanned: the standard ones hold the user's own plugins.
        a.state.plugdb.prefs.standard_off = crate::plugin_db::search_dirs(&a.state.plugdb.prefs).iter().map(|d| d.path.display().to_string()).collect();
        a.open_settings(PLUGINS);
        frame(&mut a, &mut c, None);
        let finish = |a: &mut App| {
            let t0 = std::time::Instant::now();
            while a.sweep.is_some() && t0.elapsed() < std::time::Duration::from_secs(120) {
                std::thread::sleep(std::time::Duration::from_millis(5));
                a.scan_tick();
            }
            assert!(a.sweep.is_none(), "the scan ends");
        };
        for d in std::iter::once(&dir).chain(&dir32) {
            crate::dialog::fake::answer(Some(d.clone()));
            let r = hit_rect(&a, &Action::Settings(SAct::Button("Add folder...")));
            click(&mut a, &mut c, r);
            finish(&mut a);
        }
        println!("scan: {:?}", a.state.message);
        let r = hit_rect(&a, &Action::Settings(SAct::Plug(Plug::Prefer(crate::plugin_db::Prefer::Vst2))));
        click(&mut a, &mut c, r);
        assert_eq!(a.state.plugdb.prefs.prefer, crate::plugin_db::Prefer::Vst2);
        let vst2: Vec<&crate::plugin_db::TableRow> = a.state.plugdb.all_rows.iter().filter(|r| r.format == "VST2" || r.file.ends_with("-vst.dll")).collect();
        for r in &vst2 {
            println!("  {:22} {:5} {:10} {:8} {:30} {}", r.name, r.format, r.kind, r.status, r.file, r.note);
        }
        let ok = |name: &str| vst2.iter().any(|r| r.name.replace(' ', "") == name && r.format == "VST2" && r.status == "ok");
        assert!(ok("Kars") && ok("3BandEQ"), "Kars and 3BandEQ listed as VST2");
        if dir32.is_some() {
            let unsupported = crate::plugin_db::Status::Unsupported.text();
            assert!(vst2.iter().any(|r| r.status == unsupported && r.note.contains("32-bit")), "32-bit plugins listed as not supported, with the reason");
            assert!(!vst2.iter().any(|r| r.status == "error" && r.note.contains("32-bit")), "and not as errors");
        }
        if let Some(d) = &shots {
            frame(&mut a, &mut c, None);
            c.pixmap.save_png(d.join("vst2_settings_plugins.png")).unwrap();
            let r = hit_rect(&a, &Action::Settings(SAct::Plug(Plug::Find)));
            click(&mut a, &mut c, r);
            for ch in "-vst".chars() {
                a.key(&Key::Character(ch.to_string().into()), false, false);
            }
            frame(&mut a, &mut c, None);
            c.pixmap.save_png(d.join("vst2_settings_plugins_vst.png")).unwrap();
            a.key(&Key::Named(NamedKey::Escape), false, false);
        }
        a.state.settings.open = false;

        // Kars from the rack's Add one ▸ menu: with VST2 preferred, its one entry is the VST2 copy.
        let add = crate::rack::add_one_items(&a.state).into_iter().find(|it| it.label == "Kars").expect("Add one lists Kars");
        let row = a.state.project.channels.len();
        pl::run(&mut a, add.cmd);
        for _ in 0..200 {
            a.tick();
            if a.state.pending_plugin.is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(a.state.hosted_rows.contains(&row), "Kars hosted: {:?}", a.state.message);
        assert_eq!(a.plugins.descriptor(row).map(|d| d.format), Some(Format::Vst2));
        // 3BandEQ in the first slot of Kars' insert, from the slot's menu.
        let track = a.state.project.channels[row].mixer_track;
        crate::mixer::select(&mut a.state, track, false);
        let eq = a.plugins.effects.iter().position(|e| e.desc.format == Format::Vst2 && e.desc.name.replace(' ', "") == "3BandEQ").expect("3BandEQ");
        let menu = crate::mixer::effect_menu(&a.state, 0, 0.0, 0.0);
        let cmd = Cmd::Plugin(PlugCmd::Effect { index: eq, slot: Some(0) });
        assert!(menu.items.iter().any(|it| it.cmd == cmd), "the slot menu lists the VST2 copy");
        pl::run(&mut a, cmd);
        let key = fx_key(track, 0);
        for _ in 0..200 {
            a.tick();
            if a.plugins.hosted.contains_key(&key) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(a.plugins.descriptor(key).map(|d| d.format), Some(Format::Vst2), "the EQ is hosted: {:?}", a.state.message);
        // Eight C5 notes, a beat each, on the pattern placed at the song's start.
        let ch = a.state.project.channels[row].index;
        let ppq = u32::from(a.state.project.ppq);
        a.state.project.patterns[0].notes = (0..8).map(|i| os_model::Note { length: ppq, key: 60, ..os_model::Note::step(ch, i * ppq) }).collect();
        a.state.playlist.reset();
        pl::arr_mut(&mut a.state).clips.push(os_model::Clip::new(0, 8 * ppq, 0, os_model::ClipSource::Pattern(1)));
        // The EQ's master gain automated from its lowest to its highest over the eight beats.
        a.params_tick();
        let master = a.state.params.lists[&key].params.iter().position(|p| p.name == "Master").expect("a Master parameter");
        println!("automate: {:?}", a.params_automate(key, master));
        let auto = a.state.project.channels.last().unwrap().index;
        {
            let x = a.state.project.automation_mut(auto).unwrap();
            assert_eq!(x.points.len(), 2);
            x.points[0].value = 0.0;
            x.points[1].value = 1.0;
        }
        let tempo = f64::from(a.state.tempo);
        let export = |a: &mut App, tag: &str| -> Vec<f32> {
            let path = std::env::temp_dir().join(format!("lemon-vst2-{tag}-{}.wav", std::process::id()));
            a.transport.export(&a.state.project, os_engine::PlayMode::Song, rate, crate::export::Job::wav(&path));
            let t0 = std::time::Instant::now();
            let _secs = loop {
                match a.transport.export_status() {
                    crate::transport::Export::Done(r) => break r.expect("export").0,
                    _ if t0.elapsed() > Duration::from_secs(60) => panic!("export did not finish"),
                    _ => std::thread::sleep(Duration::from_millis(10)),
                }
            };
            let bytes = std::fs::read(&path).expect("file written");
            let _ = std::fs::remove_file(&path);
            bytes[44..].chunks_exact(8).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect()
        };
        // Per note: (frames from its beat to where it first reaches half its peak, peak, pitch in Hz by autocorrelation).
        let beat = f64::from(rate) * 60.0 / tempo;
        let notes = |left: &[f32]| -> Vec<(i64, f32, f64)> {
            (0..8)
                .map(|i| {
                    let s = (i as f64 * beat).round() as usize;
                    let w = &left[s..(s + beat as usize).min(left.len())];
                    let peak = w.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                    let from = s.saturating_sub(512);
                    let onset = left[from..].iter().position(|v| v.abs() > 0.5 * peak).map_or(i64::MAX, |k| (from + k) as i64 - s as i64);
                    let x = &w[2400..2400 + 4096];
                    let corr = |lag: usize| x.iter().zip(&x[lag..]).map(|(p, q)| p * q).sum::<f32>();
                    let best = (80..480).map(corr).fold(f32::MIN, f32::max);
                    let lag = (80..480).find(|&l| corr(l) > 0.9 * best && corr(l) >= corr(l - 1) && corr(l) >= corr(l + 1)).unwrap_or(0);
                    (onset, peak, f64::from(rate) / lag.max(1) as f64)
                })
                .collect()
        };
        let check = |label: &str, n: &[(i64, f32, f64)]| {
            println!("{label}: {:?}", n.iter().map(|(o, p, f)| format!("{o:+} fr, {p:.3}, {f:.0} Hz")).collect::<Vec<_>>());
            for (i, &(onset, peak, hz)) in n.iter().enumerate() {
                assert!(onset.abs() <= 64, "{label}: note {i} starts {onset} frames from its beat");
                assert!((hz / 261.63).log2().abs() < 1.0 / 12.0, "{label}: note {i} at {hz:.0} Hz, not C5");
                if i > 0 {
                    assert!(peak > n[i - 1].1 * 1.2, "{label}: note {i} is not louder than the one before");
                }
            }
        };
        a.tick();
        let first = notes(&export(&mut a, "first"));
        check("export", &first);

        // Live: the pattern plays on the device, Stop silences it. The export left the EQ's Master at the top of its
        // clip, far too loud for the speakers, so it plays at Master's default (pattern playback leaves it there).
        if crate::transport::Transport::device_rate().is_some() {
            {
                let mut p = a.plugins.hosted[&key].plugin.lock().unwrap();
                let info = p.param_info(master).unwrap();
                assert!(p.set_param(info.id, info.default));
            }
            a.transport.take_peaks();
            a.transport.play(&a.state.project, os_engine::PlayMode::Pattern(1), 0.0);
            std::thread::sleep(Duration::from_millis(1200));
            let heard = a.transport.take_peaks();
            let tick = a.transport.tick();
            a.transport.stop();
            std::thread::sleep(Duration::from_millis(100));
            a.transport.take_peaks();
            std::thread::sleep(Duration::from_millis(200));
            let after = a.transport.take_peaks();
            println!("live: peaks {heard:?} while playing (at tick {tick:.0}), {after:?} from 100 ms after Stop");
            assert!(heard.0 > 0.01, "heard while playing");
            assert!(heard.0 < 1.0, "played at a level the speakers take, not clipped ({:.2})", heard.0);
            assert_eq!(after, (0.0, 0.0), "silent after Stop");
        }

        // Saved: both plugins stored as wrapped VST2 plugins with their state, the clip aimed at the EQ. Kars' last
        // parameter is first moved off its default, so the state that comes back is known not to be the default one.
        let path = std::env::temp_dir().join(format!("lemon-vst2-{}.flp", std::process::id()));
        let moved = {
            let mut p = a.plugins.hosted[&row].plugin.lock().unwrap();
            let info = p.param_info(p.param_count() - 1).unwrap();
            let v = if p.param_value(info.id).is_some_and(|v| v > 0.5) { 0.25 } else { 0.75 };
            assert!(p.set_param(info.id, v));
            p.idle();
            println!("Kars {} set to {v} ({:?})", info.name, p.param_text(info.id, v));
            (info.id, v)
        };
        let (ch_state, fx_state) = (a.plugins.state(row).unwrap(), a.plugins.state(key).unwrap());
        a.save_to(path.clone());
        if let Some(keep) = std::env::var_os("OS_VST2_KEEP") {
            std::fs::copy(&path, &keep).unwrap();
        }
        assert!(a.state.message.as_deref().is_some_and(|m| m.starts_with("Saved")), "{:?}", a.state.message);
        let saved = os_flp::project::load(&os_flp::FlpFile::parse(&std::fs::read(&path).unwrap()).unwrap()).unwrap();
        let r = crate::project_plugins::wrapped_channel(&saved.channels[row]).expect("a wrapped channel");
        assert_eq!((r.format.as_str(), r.id.as_str(), r.name.as_str()), ("vst2", a.plugins.descriptor(row).unwrap().id.as_str(), "Kars"));
        assert_eq!(r.state, ch_state);
        let slot = saved.mixer.iter().find(|m| m.index == track).and_then(|m| m.slots[0].as_ref());
        let s = slot.and_then(crate::project_plugins::wrapped_slot).expect("a wrapped slot");
        assert_eq!((s.format.as_str(), s.state == fx_state), ("vst2", true));
        let target = a.state.project.automations.last().unwrap().target();
        assert!(saved.automations.iter().any(|x| x.target() == target), "the clip is saved");
        println!("saved: Kars state {} bytes ({:?}...), 3BandEQ state {} bytes", ch_state.len(), &ch_state[..ch_state.len().min(25)], fx_state.len());

        // Reopened: found as VST2 again, with the saved state, and sounding the same.
        let lists = |keep: &dyn Fn(Format) -> bool| {
            let mut p = crate::plugins::Plugins::builtins();
            p.available = a.plugins.available.iter().filter(|x| keep(x.desc.format)).cloned().collect();
            p.effects = a.plugins.effects.iter().filter(|x| keep(x.desc.format)).cloned().collect();
            p
        };
        let mut b = App::new(None);
        b.plugins = lists(&|_| true);
        b.open(path.clone());
        b.sync_wrapped();
        assert!(b.state.hosted_rows.contains(&row), "{:?}", b.state.message);
        assert_eq!(b.plugins.descriptor(row).map(|d| d.format), Some(Format::Vst2));
        assert_eq!(b.plugins.descriptor(key).map(|d| d.format), Some(Format::Vst2));
        assert_eq!(b.plugins.state(row).as_deref(), Some(&ch_state[..]));
        assert_eq!(b.plugins.state(key).as_deref(), Some(&fx_state[..]));
        let back = b.plugins.hosted[&row].plugin.lock().unwrap().param_value(moved.0);
        assert!(back.is_some_and(|v| (v - moved.1).abs() < 1e-6), "Kars' moved parameter came back as {back:?}");
        b.tick();
        check("reopened", &notes(&export(&mut b, "reopened")));

        // With no VST2 copy listed, the project's VST2 plugins are played by their copies in another format, from those
        // copies' defaults (the saved state is the VST2 plugin's), and the hint bar names them.
        let mut v = App::new(None);
        v.plugins = lists(&|f| f != Format::Vst2);
        v.open(path.clone());
        v.sync_wrapped();
        let _ = std::fs::remove_file(&path);
        for k in [row, key] {
            assert!(v.plugins.descriptor(k).is_some_and(|d| d.format != Format::Vst2), "{k} played by a copy: {:?}", v.state.message);
        }
        let msg = v.state.message.clone().unwrap_or_default();
        assert!(msg.contains("from its default settings") && msg.contains("Kars (VST2 as") && msg.contains("3 Band EQ (VST2 as"), "{msg}");
        let fresh = {
            let mut f = lists(&|f| f != Format::Vst2);
            let i = v.plugins.available.iter().position(|x| Some(&x.desc) == v.plugins.descriptor(row).as_ref()).unwrap();
            f.create_with(row, i, None).unwrap();
            f.state(row)
        };
        assert_eq!(v.plugins.state(row), fresh, "Kars' copy starts from its defaults");
        // Saved untouched: the copies leave both wrappers as the file had them, so the VST2 plugins and their settings
        // stay in the project (spec: flp/events/plugins.md §4).
        let path2 = std::env::temp_dir().join(format!("lemon-vst2-copies-{}.flp", std::process::id()));
        let reread = |p: &std::path::Path| os_flp::project::load(&os_flp::FlpFile::parse(&std::fs::read(p).unwrap()).unwrap()).unwrap();
        let wrappers = |p: &os_model::Project| {
            let slot = p.mixer.iter().find(|m| m.index == track).and_then(|m| m.slots[0].clone());
            ((p.channels[row].plugin.clone(), p.channels[row].plugin_state.clone()), slot)
        };
        v.save_to(path2.clone());
        assert!(v.state.message.as_deref().is_some_and(|m| m.starts_with("Saved")), "{:?}", v.state.message);
        assert!(wrappers(&reread(&path2)) == wrappers(&saved), "untouched copies keep the VST2 plugins byte for byte");
        // Played (a song export): playing is not the user's edit, nor is the automation clip moving the EQ copy's
        // Master, so both copies still keep the VST2 plugins.
        v.tick();
        let n = notes(&export(&mut v, "substituted"));
        println!("substituted: {:?}", n.iter().map(|(o, p, f)| format!("{o:+} fr, {p:.3}, {f:.0} Hz")).collect::<Vec<_>>());
        v.params_tick();
        v.save_to(path2.clone());
        assert!(wrappers(&reread(&path2)) == wrappers(&saved), "played and automated copies keep the VST2 plugins");
        // Kars' copy edited by the user (Paste value onto its last parameter, Volume, in the parameter menu): saved as
        // that copy with its state.
        assert_eq!(v.browse_params(row), None);
        let last = v.state.params.lists[&row].params.len() - 1;
        let id = v.state.params.lists[&row].params[last].id;
        let x = v.plugins.hosted[&row].plugin.lock().unwrap().param_value(id).unwrap();
        v.state.value_clip = Some(if x > 0.5 { 0.25 } else { 0.75 });
        assert_eq!(crate::playlist::run(&mut v, crate::playlist::Cmd::Param(crate::params::PCmd::Paste(row, last))), None);
        v.plugins.hosted[&row].plugin.lock().unwrap().idle();
        let kars = v.plugins.state(row).unwrap();
        assert_ne!(Some(&kars), fresh.as_ref(), "Kars' copy changed");
        v.save_to(path2.clone());
        let again = reread(&path2);
        let _ = std::fs::remove_file(&path2);
        let r = crate::project_plugins::wrapped_channel(&again.channels[row]).expect("a wrapped channel");
        let d = v.plugins.descriptor(row).unwrap();
        assert_eq!((r.format.as_str(), r.name.as_str(), r.state == kars), (d.format.as_str(), d.name.as_str(), true));
        println!("Kars' changed copy saved as {} ({} bytes of state)", r.format, kars.len());
    }

    /// "Monophonic painting in piano roll" (spec: ui/settings-choices.md "Miscellaneous: seven more checks, Default
    /// template and Startup project"): on, a draw-tool click and each note of a paint drag delete the notes of other
    /// keys whose time overlaps the new note, in the same channel and pattern, as part of the paint's undo step. Off,
    /// and for other channels or notes beside it, the notes stay.
    #[test]
    fn monophonic_painting_in_piano_roll() {
        use crate::prefs::{set_check, MONO_PAINTING};
        use os_model::Note;
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let note = |ch: u16, key: u16, at: u32, len: u32| Note { key, length: len, ..Note::step(ch, at) };
        let keys = |app: &App, ch: u16| {
            let mut v: Vec<(u16, u32)> = piano::notes(&app.state).iter().filter(|n| n.channel == ch).map(|n| (n.key, n.position)).collect();
            v.sort_unstable();
            v
        };
        for on in [false, true] {
            set_check(&MONO_PAINTING, on);
            let mut app = App::new(None);
            app.state.z = vec![Win::PianoRoll];
            app.state.selected_channel = 0;
            // A draw-tool click: an eighth-long G5 (ppq 96) over channel 1's C5; channel 2's G5 and channel 1's later
            // D5 are not under it. The click's drag then moves the new note a beat on (it still is the one dragged).
            let start = vec![note(0, 60, 0, 96), note(1, 67, 0, 96), note(0, 62, 384, 96)];
            piano::pattern_mut(&mut app.state).notes = start.clone();
            frame(&mut app, &mut c, None);
            let g = app.piano_geom().expect("piano roll shown");
            let at = |g: &Geom, key: i32, tick: u32| (g.x_of(tick as f32) + 2.0, g.y_of(key) + g.row / 2.0);
            let (x, y) = at(&g, 67, 0);
            app.mouse_move(x, y);
            app.mouse_down();
            let (x, y) = at(&g, 67, 96);
            app.mouse_move(x, y);
            app.mouse_up();
            let want: Vec<(u16, u32)> = if on { vec![(62, 384), (67, 96)] } else { vec![(60, 0), (62, 384), (67, 96)] };
            assert_eq!(keys(&app, 0), want, "draw click, check {on}");
            assert_eq!(keys(&app, 1), vec![(67, 0)], "another channel's notes stay");
            assert!(piano::undo(&mut app.state));
            assert_eq!(piano::notes(&app.state), &start[..], "one undo step, check {on}");

            // A paint drag of E5s an eighth apart over a run of C5s: each E5 deletes the C5 under it.
            let run: Vec<Note> = (0..3).map(|k| note(0, 60, 96 * k, 48)).collect();
            piano::pattern_mut(&mut app.state).notes = run.clone();
            app.state.piano.last_len = 48;
            app.act(Action::PianoTool(Tool::Paint));
            frame(&mut app, &mut c, None);
            let g = app.piano_geom().expect("piano roll shown");
            let (x, y) = at(&g, 64, 0);
            app.mouse_move(x, y);
            app.mouse_down();
            let (x, y) = at(&g, 64, 200);
            app.mouse_move(x, y);
            app.mouse_up();
            let painted: Vec<(u16, u32)> = (0..5).map(|k| (64, 48 * k)).collect();
            let kept: Vec<(u16, u32)> = if on { Vec::new() } else { run.iter().map(|n| (60, n.position)).collect() };
            let mut want = [kept, painted].concat();
            want.sort_unstable();
            assert_eq!(keys(&app, 0), want, "paint drag, check {on}");
            assert!(piano::undo(&mut app.state));
            assert_eq!(piano::notes(&app.state), &run[..], "one undo step for the drag, check {on}");
        }
        set_check(&MONO_PAINTING, false);
    }

    /// Show ASIO panel works while an ASIO driver is open; the buffer length combo stays.
    #[test]
    fn show_asio_panel_works_while_a_driver_is_open() {
        use crate::audio_cfg::{AudioCfg, Set};
        use crate::views::settings::{SAct, AUDIO, BUFFER_LENGTH, SHOW_ASIO_PANEL};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.open_settings(AUDIO);
        let shown = |app: &App| {
            let combo = app.hits.iter().any(|h| matches!(h.action, Action::Settings(SAct::Combo(BUFFER_LENGTH, ..))));
            (combo, app.hits.iter().any(|h| h.action == Action::Settings(SAct::Button(SHOW_ASIO_PANEL))))
        };
        frame(&mut app, &mut c, None);
        assert_eq!(shown(&app), (true, false), "a Windows device");
        // Only the page's copy of the settings: no driver is opened.
        app.state.settings.audio = AudioCfg::default().with(Set::Asio("Some ASIO".into()));
        app.invalidate();
        frame(&mut app, &mut c, None);
        assert_eq!(shown(&app), (true, false), "an ASIO driver, not open");
        app.state.settings.asio_open = true;
        app.invalidate();
        frame(&mut app, &mut c, None);
        assert_eq!(shown(&app), (true, cfg!(windows)), "an ASIO driver, open");
    }

    /// The Windows build names ASIO on the Audio and About pages, so both show the ASIO logo with Steinberg's notice
    /// (the ASIO SDK's usage guidelines); the other builds have no ASIO and show neither.
    #[test]
    fn the_asio_logo_goes_where_asio_is_named() {
        use crate::views::settings::{page, Row, AUDIO, TABS};
        let app = App::new(None);
        let logo = |r: &Row| matches!(r, Row::AsioLogo);
        let shows = |tab| page(tab, &app.state).iter().flat_map(|g| &g.rows).any(|r| logo(r) || matches!(r, Row::Pair(_, b) if logo(b)));
        assert_eq!(shows(AUDIO), cfg!(windows), "Audio");
        assert_eq!(shows(TABS.len() - 1), cfg!(windows), "About");
        assert_eq!(crate::views::settings::asio_logo().map(|l| (l.0, l.1)), cfg!(windows).then_some((474, 443)), "the logo decodes");
    }

    /// Windows devices have a Mode combo (spec: audio/wasapi-low-latency.md 4.2): in exclusive mode a line says other
    /// programs can't play on the device, and its buffer lengths are the device's periods; under an ASIO driver the
    /// combo is greyed. Linux devices have one too (the sound server, or ALSA directly). Elsewhere there is none.
    #[test]
    fn the_mode_combo_on_windows_devices() {
        use crate::audio_cfg::{AudioCfg, Set, SMALLEST};
        use crate::device::Periods;
        use crate::views::settings::{page, Row, SAct, AUDIO, BUFFER_LENGTH, MODE};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.open_settings(AUDIO);
        let rows = |app: &App| page(AUDIO, &app.state).into_iter().flat_map(|g| g.rows).collect::<Vec<_>>();
        let mode = |app: &App| rows(app).iter().find_map(|r| match r { Row::Combo(v, _, l) if *l == MODE => Some(v.clone()), _ => None });
        let alone = |app: &App| rows(app).iter().any(|r| matches!(r, Row::Text(t, _) if t.starts_with("Other programs can't play on")));
        let has_mode = cfg!(windows) || cfg!(target_os = "linux");
        assert_eq!((mode(&app), alone(&app)), (has_mode.then(|| "Shared".to_string()), false));
        if cfg!(target_os = "linux") {
            app.state.settings.audio = AudioCfg::default().with(Set::Mode(true));
            assert_eq!((mode(&app).as_deref(), alone(&app)), (Some("Exclusive"), true));
        }
        if !cfg!(windows) {
            return;
        }
        // Exclusive, on this PC's Realtek endpoint as measured (only the page's copy of the settings: nothing opens).
        let realtek = Periods { rate: 48000, shared: Some((480, 480, 480, 480)), shared_buffer: 1056, exclusive: (100_000, 30_000), exclusive_rates: vec![] };
        app.state.settings.audio = AudioCfg::default().with(Set::Mode(true));
        app.state.settings.periods = Some(realtek);
        app.state.settings.rate = Some(48000);
        assert_eq!((mode(&app).as_deref(), alone(&app)), (Some("Exclusive"), true));
        app.invalidate();
        frame(&mut app, &mut c, None);
        let combo = |app: &App, label: &str| app.hits.iter().rev().find(|h| matches!(&h.action, Action::Settings(SAct::Combo(l, ..)) if *l == label)).map(|h| h.rect);
        let r = combo(&app, BUFFER_LENGTH).expect("buffer length");
        click(&mut app, &mut c, r);
        let labels: Vec<String> = app.state.playlist.popup.take().expect("a menu").items.iter().map(|i| i.label.to_string()).collect();
        assert_eq!(labels[..3], ["Device (480 frames (10.0 ms))", "Smallest (144 frames (3.0 ms))", "256 frames (5.3 ms)"]);
        app.state.settings.audio.buffer = Some(SMALLEST);
        let buffer = rows(&app).iter().find_map(|r| match r { Row::Combo(v, _, l) if *l == BUFFER_LENGTH => Some(v.clone()), _ => None });
        assert_eq!(buffer.as_deref(), Some("Smallest (144 frames (3.0 ms))"));
        // Under an ASIO driver: shown greyed, no menu.
        app.state.settings.audio = AudioCfg::default().with(Set::Mode(true)).with(Set::Asio("Some ASIO".into()));
        app.invalidate();
        frame(&mut app, &mut c, None);
        assert_eq!((mode(&app).as_deref(), alone(&app)), (Some("Exclusive"), false));
        assert!(combo(&app, MODE).is_none(), "greyed");
    }

    /// With no ASIO driver installed, the Audio page says to install one (Windows only); with one, it doesn't.
    #[test]
    fn no_asio_driver_says_to_install_one() {
        use crate::views::settings::{page, Row, AUDIO, NO_ASIO_DRIVER};
        let mut app = App::new(None);
        let says = |app: &App| page(AUDIO, &app.state).iter().flat_map(|g| &g.rows).any(|r| matches!(r, Row::Text(t, _) if t == NO_ASIO_DRIVER));
        app.state.settings.asio_drivers = Vec::new();
        assert_eq!(says(&app), cfg!(windows), "none installed");
        app.state.settings.asio_drivers = vec!["ASIO4ALL v2".into()];
        assert!(!says(&app), "one installed");
    }

    /// The Audio page with an ASIO driver chosen (the first installed one), and its Output device menu, as pictures,
    /// for looking (not an assertion): `OS_AUDIO_SHOT=<folder>`. Windows, with a driver installed.
    #[test]
    fn audio_shot() {
        use crate::audio_cfg::{AudioCfg, Set};
        use crate::views::settings::{SAct, AUDIO, BUFFER_LENGTH, MODE, OUTPUT_DEVICE};
        let Some(out) = std::env::var_os("OS_AUDIO_SHOT") else { return };
        let out = std::path::PathBuf::from(out);
        let Some(driver) = crate::device::asio_drivers().into_iter().next() else { return };
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        AudioCfg::default().with(Set::Asio(driver)).apply();
        let mut app = App::new(None);
        // The driver opens with the audio thread; the page then shows its buffer length.
        let t = std::time::Instant::now();
        while crate::device::opens() == 0 && t.elapsed() < std::time::Duration::from_secs(3) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        app.open_settings(AUDIO);
        app.audio_status();
        app.invalidate();
        frame(&mut app, &mut c, None);
        c.pixmap.save_png(out.join("01_audio_asio.png")).unwrap();
        let r = app.hits.iter().find(|h| matches!(h.action, Action::Settings(SAct::Combo(OUTPUT_DEVICE, ..)))).map(|h| h.rect).unwrap();
        click(&mut app, &mut c, r);
        c.pixmap.save_png(out.join("02_output_device_menu.png")).unwrap();
        app.state.playlist.popup = None;
        frame(&mut app, &mut c, None);
        let r = app.hits.iter().find(|h| matches!(h.action, Action::Settings(SAct::Combo(BUFFER_LENGTH, ..)))).map(|h| h.rect).unwrap();
        click(&mut app, &mut c, r);
        c.pixmap.save_png(out.join("03_buffer_length_menu.png")).unwrap();
        app.state.playlist.popup = None;
        app.open_settings(crate::views::settings::TABS.len() - 1);
        frame(&mut app, &mut c, None);
        c.pixmap.save_png(out.join("04_about.png")).unwrap();
        AudioCfg::default().apply();
        // As a PC without an ASIO driver has it.
        app.open_settings(AUDIO);
        app.state.settings.asio_drivers.clear();
        app.invalidate();
        frame(&mut app, &mut c, None);
        c.pixmap.save_png(out.join("05_audio_no_asio_driver.png")).unwrap();
        // The default Windows device: the Mode menu, then exclusive mode's page and buffer lengths.
        let combo = |app: &App, label: &str| app.hits.iter().find(|h| matches!(&h.action, Action::Settings(SAct::Combo(l, ..)) if *l == label)).map(|h| h.rect).unwrap();
        let r = combo(&app, MODE);
        click(&mut app, &mut c, r);
        c.pixmap.save_png(out.join("06_mode_menu.png")).unwrap();
        app.state.playlist.popup = None;
        // As picked on the page: the driver is let go of, and the device opens in exclusive mode with the first note.
        app.set_audio(Set::Mode(true));
        let t = std::time::Instant::now();
        while app.transport.opened().is_some() && t.elapsed() < std::time::Duration::from_secs(3) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        app.open_settings(AUDIO);
        app.audio_status();
        app.invalidate();
        frame(&mut app, &mut c, None);
        c.pixmap.save_png(out.join("07_audio_exclusive.png")).unwrap();
        let r = combo(&app, BUFFER_LENGTH);
        click(&mut app, &mut c, r);
        c.pixmap.save_png(out.join("08_exclusive_buffer_length_menu.png")).unwrap();
        app.state.playlist.popup = None;
        AudioCfg::default().apply();
        app.open_settings(AUDIO);
        frame(&mut app, &mut c, None);
        let r = combo(&app, BUFFER_LENGTH);
        click(&mut app, &mut c, r);
        c.pixmap.save_png(out.join("09_shared_buffer_length_menu.png")).unwrap();
    }

    /// Settings > MIDI with this PC's devices, its curve window, the OPTIONS menu and the Remote control box, as
    /// pictures, for looking (not an assertion): `OS_MIDI_SHOT=<folder>`.
    #[test]
    fn midi_shot() {
        use crate::views::midi_page::MAct;
        use crate::views::settings::{SAct, MIDI};
        let Some(out) = std::env::var_os("OS_MIDI_SHOT") else { return };
        let out = std::path::PathBuf::from(out);
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.midi = crate::midi_in::MidiIn::open(|| {});
        app.open_settings(MIDI);
        frame(&mut app, &mut c, None);
        c.pixmap.save_png(out.join("01_midi_page.png")).unwrap();
        if let Some(r) = app.hits.iter().find(|h| matches!(h.action, Action::Settings(SAct::Midi(MAct::Input(0))))).map(|h| h.rect) {
            click(&mut app, &mut c, r);
            c.pixmap.save_png(out.join("02_input_selected.png")).unwrap();
        }
        if let Some(r) = app.hits.iter().find(|h| matches!(h.action, Action::Settings(SAct::Midi(MAct::Curve(_))))).map(|h| h.rect) {
            click(&mut app, &mut c, r);
            c.pixmap.save_png(out.join("03_curve.png")).unwrap();
        }
        app.state.settings.open = false;
        app.state.midi.curve = None;
        app.invalidate();
        frame(&mut app, &mut c, None);
        let r = hit_rect(&app, &Action::Menu(crate::views::menus::OPTIONS_MENU));
        click(&mut app, &mut c, r);
        c.pixmap.save_png(out.join("04_options_menu.png")).unwrap();
        app.state.menu_open = None;
        app.remote_cmd(crate::remote::RCmd::Link(crate::typein::Target::Knob(crate::knobs::KnobId::ChanVol(0))));
        app.invalidate();
        frame(&mut app, &mut c, None);
        c.pixmap.save_png(out.join("05_link_box.png")).unwrap();
    }

    /// The header's menus and switches as pictures, for looking (not an assertion): `OS_HEADER_SHOT=<folder>`.
    #[test]
    fn header_shot() {
        use crate::views::toolbar::Switch;
        use views::menus::{ADD_MENU, MENUS, PATTERNS, VIEW};
        let Some(out) = std::env::var_os("OS_HEADER_SHOT") else { return };
        let out = std::path::PathBuf::from(out);
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.pattern = 1;
        // Kick on every beat, a clap on 2 and 4; two clips of the pattern.
        let note = |ch: u16, at: u32| Note { length: 48, ..Note::step(ch, at) };
        app.state.project.patterns[0].notes = vec![note(0, 0), note(1, 96), note(0, 192), note(1, 288), note(0, 384), note(1, 480)];
        crate::playlist::arr_mut(&mut app.state).clips = vec![
            os_model::Clip::new(0, 768, 0, os_model::ClipSource::Pattern(1)),
            os_model::Clip::new(1536, 768, 2, os_model::ClipSource::Pattern(1)),
        ];
        let save = |app: &mut App, c: &mut Canvas, name: &str| {
            app.invalidate();
            frame(app, c, None);
            c.pixmap.save_png(out.join(format!("{name}.png"))).unwrap();
        };
        let entry = |m: usize, label: &str| MENUS[m].cols.iter().flat_map(|(_, items)| items.iter()).position(|it| it.label == label).unwrap();
        save(&mut app, &mut c, "01_default");
        // The Toolbars menu on the hint panel.
        app.mouse_move(200.0, 20.0);
        app.mouse_down_right();
        app.mouse_up();
        save(&mut app, &mut c, "02_toolbars_menu");
        app.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape), false, false);
        // VIEW > Toolbars ▸.
        frame(&mut app, &mut c, None);
        let view = hit_rect(&app, &Action::Menu(VIEW));
        click(&mut app, &mut c, view);
        let row = hit_rect(&app, &Action::MenuSub(VIEW, entry(VIEW, "Toolbars")));
        click(&mut app, &mut c, row);
        save(&mut app, &mut c, "03_view_toolbars");
        app.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape), false, false);
        app.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape), false, false);
        // Flat buttons, no separators, the hint bar, a button hovered.
        app.toolbar_switch(Switch::Flat);
        app.toolbar_switch(Switch::Separators);
        app.toolbar_switch(Switch::HintBar);
        frame(&mut app, &mut c, None);
        let save_btn = app.hits.iter().find(|h| h.hint.starts_with("Save")).map(|h| h.rect).unwrap();
        app.mouse_move(save_btn.cx(), save_btn.cy());
        save(&mut app, &mut c, "04_flat_noseps_hintbar");
        app.toolbar_switch(Switch::Flat);
        app.toolbar_switch(Switch::Separators);
        app.toolbar_switch(Switch::HintBar);
        app.toolbar_switch(Switch::Hide);
        save(&mut app, &mut c, "05_hidden");
        app.toolbar_switch(Switch::Hide);
        // The clock in SMPTE and in B:B:T, a second and a half in.
        app.state.clock.mode = crate::clock::Mode::Smpte(4);
        save(&mut app, &mut c, "06_clock_smpte");
        app.state.clock.mode = crate::clock::Mode::Bar;
        app.state.clock.beat = true;
        let clock = hit_rect(&app, &Action::Clock);
        app.mouse_move(clock.cx(), clock.cy());
        app.mouse_down_right();
        app.mouse_up();
        save(&mut app, &mut c, "07_clock_menu");
        app.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape), false, false);
        app.state.clock = Default::default();
        // ADD and PATTERNS menus.
        frame(&mut app, &mut c, None);
        let add = hit_rect(&app, &Action::Menu(ADD_MENU));
        click(&mut app, &mut c, add);
        save(&mut app, &mut c, "08_add_menu");
        app.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape), false, false);
        frame(&mut app, &mut c, None);
        let pats = hit_rect(&app, &Action::Menu(PATTERNS));
        click(&mut app, &mut c, pats);
        save(&mut app, &mut c, "09_patterns_menu");
        app.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape), false, false);
        // The time signature dialog, then 3/8 in the piano roll.
        app.menu_item(PATTERNS, entry(PATTERNS, "Set time signature..."));
        save(&mut app, &mut c, "10_timesig_dialog");
        app.state.dialogs.clear();
        crate::timesig::set_start(&mut app.state, 1, 3, 8);
        app.act(Action::ShowMax(Win::PianoRoll));
        save(&mut app, &mut c, "11_piano_3_8");
        app.act(Action::ShowMax(Win::Playlist));
        save(&mut app, &mut c, "11b_playlist_3_8");
        // Split by channel, then the playlist.
        app.menu_item(PATTERNS, entry(PATTERNS, "Split by instrument"));
        save(&mut app, &mut c, "12_split_playlist");
        // ADD > Automation clip and Audio clip.
        app.menu_item(ADD_MENU, entry(ADD_MENU, "Automation clip"));
        save(&mut app, &mut c, "13_automation_clip");
        app.menu_item(ADD_MENU, entry(ADD_MENU, "Audio clip"));
        save(&mut app, &mut c, "14_audio_clip");
        // The channel rack with pattern 1 in 3/8 (ui/channel-rack.md §23): lit to its length, grouped by the project's
        // beat; its mini roll over the pattern's bars; then the same in a 6/8 project.
        app.state.pattern = 1;
        let sig = os_model::TimeMarker { kind: os_model::TimeMarker::SIGNATURE, numerator: 3, denominator: 8, ..os_model::TimeMarker::new(0, "3/8") };
        app.state.project.patterns[0].markers = vec![sig];
        app.state.project.patterns[0].length = None;
        crate::rack::run(&mut app, crate::rack::RCmd::SetFilter(crate::rack::Filter::All));
        app.state.rack_size.0 = views::rack::DEFAULT_SIZE.0 + 320.0;
        app.state.raise(Win::ChannelRack);
        save(&mut app, &mut c, "15_rack_38");
        app.state.rack_roll_view = true;
        save(&mut app, &mut c, "16_rack_mini_38");
        app.state.rack_roll_view = false;
        app.state.project.bar = Some((6, 2));
        save(&mut app, &mut c, "17_rack_project_68");
    }

    // spec: ui/playlist.md §10a
    #[test]
    fn picker_audio_and_automation_items_have_their_channel_menu() {
        use crate::playlist::{self as pl, Cmd, PickCmd};
        let mut app = App::new(None);
        app.state.project = test_project();
        let auto = pl::create_automation(&mut app.state, os_model::target::TEMPO, "Tempo", 0.5);
        pl::add_point(&mut app.state, auto, 2.0, 0.9);
        let labels = |p: &pl::Popup| p.items.iter().filter(|i| !i.sep).map(|i| i.label.to_string()).collect::<Vec<_>>();
        let menu = pl::pick_menu(&app.state, auto, 0.0, 0.0);
        let want = ["Select in arranger", "Group selected...", "Rename, color and icon...", "Change color...", "Random color", "Change icon...", "Chokes itself", "Clone", "Clone with links", "Delete..."];
        assert_eq!(labels(&menu), want);
        app.state.project.channels[0].kind = ChannelKind::AudioClip;
        let audio = app.state.project.channels[0].index;
        let want = ["Preview", "Select in arranger", "Group selected...", "Rename, color and icon...", "Change color...", "Random color", "Change icon...", "Load sample...", "Edit sample...", "Chokes itself", "Clone", "Delete..."];
        assert_eq!(labels(&pl::pick_menu(&app.state, audio, 0.0, 0.0)), want);
        // Select in playlist: every clip of the source.
        app.state.playlist.sel.clear();
        pl::run(&mut app, Cmd::Pick(PickCmd::SelectInPlaylist, auto));
        let of_auto: Vec<usize> = pl::clips(&app.state).iter().enumerate().filter(|(_, c)| c.source == os_model::ClipSource::Channel(auto)).map(|(i, _)| i).collect();
        assert!(!of_auto.is_empty());
        assert_eq!(app.state.playlist.sel, of_auto);
        // Clone: a channel "<name> #2" whose clip has the same curve and targets.
        let n = app.state.project.channels.len();
        let name = app.state.project.channel(auto).and_then(|c| c.name.clone()).unwrap();
        pl::run(&mut app, Cmd::Pick(PickCmd::Clone, auto));
        assert_eq!(app.state.project.channels.len(), n + 1);
        let copy = app.state.project.channels.iter().find(|c| c.name.as_deref() == Some(&format!("{name} #2"))).expect("the clone").index;
        let (a, b) = (app.state.project.automation(auto).unwrap(), app.state.project.automation(copy).unwrap());
        assert_eq!((&a.points, &a.targets), (&b.points, &b.targets));
        // Delete asks first.
        pl::run(&mut app, Cmd::Pick(PickCmd::Delete, copy));
        assert!(app.state.rack.pending_delete.is_some() || app.state.rack.delete_now.is_some());
    }

    /// The channel window opens next to the rack until it is moved by hand, and where it was moved after that (spec:
    /// ui/channel-rack.md "Left click").
    #[test]
    fn channel_window_reopens_where_it_was_moved() {
        let mut app = App::new(None);
        app.add_clip_channel(ChannelKind::Automation);
        let row = app.state.project.channels.len() - 1;
        assert!(app.state.shown(Win::Plugin), "an automation clip's channel window opens");
        let beside = app.state.plugin_pos;
        app.state.hide(Win::Plugin);
        app.state.plugin_moved = true;
        app.state.plugin_pos = (beside.0 + 120.0, beside.1 + 40.0);
        app.open_channel_window(row);
        assert_eq!(app.state.plugin_pos, (beside.0 + 120.0, beside.1 + 40.0), "the channel window reopens where it was moved");
    }

    /// A typed note whose key is up ends though its release never came (issue #274: on Linux a key let go of over a
    /// docked plugin editor goes to the editor); the keys still down keep playing.
    #[test]
    fn typed_notes_end_when_their_key_is_up() {
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.selected_channel = 1;
        app.state.toggles.push("typing");
        assert!(app.typing_key(KeyCode::KeyZ, true, false, false));
        assert!(app.typing_key(KeyCode::KeyX, true, false, false));
        assert_eq!(app.typing_held.len(), 2);
        // All keys still down: nothing ends.
        app.release_keys_up(|_| true);
        assert_eq!(app.typing_held.len(), 2);
        // Z let go of where the app did not see it.
        app.release_keys_up(|code| code == KeyCode::KeyX);
        assert_eq!(app.typing_held.iter().map(|h| h.0).collect::<Vec<_>>(), [KeyCode::KeyX]);
        assert_eq!(app.state.piano.typed.len(), 1, "the piano roll's keyboard lights the held key alone");
        // Its own release, coming late, changes nothing.
        assert!(!app.typing_key(KeyCode::KeyZ, false, false, false));
        app.release_keys_up(|_| false);
        assert!(app.typing_held.is_empty());
    }
}

/// Options ▸ Detached (Lemon Studio's design, spec: ui/mixer.md §6.2, ui/channel-rack.md §17): the window bookkeeping,
/// the menu entries, the paints and the input routed by surface. The native windows themselves are the shell's
/// (`detached_win`).
#[cfg(test)]
mod detach_tests {
    use super::*;

    fn main_paint(app: &mut App) -> Option<Canvas> {
        let mut c = Canvas::new(1600, 900, 1.0).ok()?;
        app.invalidate();
        app.paint(&mut c, None);
        Some(c)
    }

    fn hit_of(hits: &[Hit], a: &Action) -> Option<Rect> {
        hits.iter().rev().find(|h| &h.action == a).map(|h| h.rect)
    }

    fn screen(app: &App, w: Win) -> &Screen {
        &app.screens.iter().find(|d| d.0 == w).expect("its surface").1
    }

    /// The Mixer's Options ▸ Detached works and is ticked while detached; unticked, the Mixer is back in the main window
    /// where it was, as it was there.
    #[test]
    fn the_mixer_menu_detaches_and_reattaches() {
        let mut app = App::new(None);
        app.state.raise(Win::Mixer);
        let r = app.state.float_rect(Win::Mixer);
        let entry = |app: &App| {
            let m = crate::mixer::options_menu(&app.state, 0.0, 0.0);
            m.items.iter().find(|i| i.label == "Detached").map(|i| (i.enabled, i.mark.clone(), i.cmd.clone())).expect("the entry")
        };
        let (enabled, mark, cmd) = entry(&app);
        assert_eq!((enabled, mark), (true, crate::playlist::Mark::Check(false)));
        assert_eq!(cmd, crate::playlist::Cmd::Detach(Win::Mixer, true));
        crate::playlist::run(&mut app, cmd);
        assert!(app.state.is_detached(Win::Mixer));
        assert_eq!(app.detached_windows(), vec![Win::Mixer]);
        assert_eq!(app.state.detached_size(Win::Mixer), Some((r.w, r.h)), "at the size it had");
        assert_eq!(app.state.win_rect(Win::Mixer), Rect::new(0.0, 0.0, r.w, r.h), "its whole native window");
        assert!(!app.state.is_max(Win::Mixer), "it floats in its own window");
        assert_eq!(app.state.z.last(), Some(&Win::Mixer), "brought to the front");
        let (_, mark, cmd) = entry(&app);
        assert_eq!((mark, cmd.clone()), (crate::playlist::Mark::Check(true), crate::playlist::Cmd::Detach(Win::Mixer, false)));
        crate::playlist::run(&mut app, cmd);
        assert!(!app.state.is_detached(Win::Mixer) && app.detached_windows().is_empty());
        assert_eq!(app.state.win_rect(Win::Mixer), r, "back where it was");
    }

    /// Unticking Detached in the detached Mixer's own menu puts it back with the main window's mouse and hits intact.
    #[test]
    fn reattaching_from_its_own_window_keeps_the_main_surface() {
        let mut app = App::new(None);
        app.state.raise(Win::Mixer);
        app.set_detached(Win::Mixer, true);
        app.mouse_move(640.0, 480.0);
        app.hits.push(Hit { rect: Rect::new(1.0, 2.0, 3.0, 4.0), action: Action::Swallow, hint: "".into(), win: None, menu: 0, reveal: None });
        let n = app.hits.len();
        app.on_surface(Win::Mixer, |a| {
            a.mouse_move(10.0, 10.0);
            crate::playlist::run(a, crate::playlist::Cmd::Detach(Win::Mixer, false));
        });
        assert!(!app.state.is_detached(Win::Mixer) && app.screens.is_empty());
        assert_eq!((app.mouse, app.hits.len()), ((640.0, 480.0), n));
    }

    /// Closing a detached window takes its native window away (it stays detached); showing it again brings it back.
    #[test]
    fn a_detached_window_follows_show_and_hide() {
        let mut app = App::new(None);
        app.state.raise(Win::Mixer);
        app.set_detached(Win::Mixer, true);
        app.act(Action::CloseWin(Win::Mixer));
        assert!(app.detached_windows().is_empty() && app.state.is_detached(Win::Mixer));
        app.state.raise(Win::Mixer);
        assert_eq!(app.detached_windows(), vec![Win::Mixer]);
    }

    /// Detached, the Mixer is painted in its own surface at (0, 0), not in the main window: its hits are that surface's,
    /// and input given to that surface reaches them (its title bar asks the shell to move the native window), while the
    /// main window's mouse stays where it was.
    #[test]
    fn a_detached_mixer_paints_and_takes_input_on_its_own_surface() {
        let mut app = App::new(None);
        app.state.raise(Win::Mixer);
        let Some(_) = main_paint(&mut app) else { return };
        assert!(app.hits.iter().any(|h| h.win == Some(Win::Mixer)), "in the main window");
        app.set_detached(Win::Mixer, true);
        main_paint(&mut app);
        assert!(!app.hits.iter().any(|h| h.win == Some(Win::Mixer)), "out of the main window");
        assert!(app.detached_dirty(Win::Mixer), "its window has something to show");

        let (w, h) = app.state.detached_size(Win::Mixer).unwrap();
        let mut c = Canvas::new(w as u32, h as u32, 1.0).unwrap();
        app.paint_detached(Win::Mixer, &mut c);
        assert!(!app.detached_dirty(Win::Mixer));
        let title = hit_of(&screen(&app, Win::Mixer).hits, &Action::WinDrag(Win::Mixer)).expect("the title bar");
        assert!(title.y < 30.0 && title.right() <= w, "at the window's top, inside it: {title:?}");
        assert!(hit_of(&app.hits, &Action::WinDrag(Win::Mixer)).is_none(), "not among the main window's hits");

        app.mouse_move(3.0, 3.0);
        let (x, y) = (title.cx(), title.cy());
        app.on_surface(Win::Mixer, |a| {
            a.mouse_move(x, y);
            a.mouse_down();
            assert_eq!(a.state.surface, Some(Win::Mixer));
        });
        assert_eq!(app.native, vec![(Win::Mixer, Native::Move)], "the window system moves it");
        assert_eq!(app.mouse, (3.0, 3.0), "the main window's mouse is its own");
        assert_eq!(screen(&app, Win::Mixer).mouse, (x, y));
        assert_eq!((app.state.surface, app.state.overlay_on), (None, Some(Win::Mixer)));
        assert!(app.detached_dirty(Win::Mixer), "input on it paints it again");
        app.on_surface(Win::Mixer, App::mouse_up);
        app.native.clear();

        // Its frame's buttons work the native window.
        if let Some(m) = hit_of(&screen(&app, Win::Mixer).hits, &Action::MaxWin(Win::Mixer)) {
            app.on_surface(Win::Mixer, |a| {
                a.mouse_move(m.cx(), m.cy());
                a.mouse_down();
                a.mouse_up();
            });
            assert_eq!(app.native, vec![(Win::Mixer, Native::Maximize)]);
            assert!(!app.state.is_max(Win::Mixer));
        }
    }

    /// A popup opened from the detached Mixer is painted there, not in the main window; one opened from the main window
    /// stays in the main window.
    #[test]
    fn popups_go_to_the_surface_they_were_opened_from() {
        let mut app = App::new(None);
        app.state.raise(Win::Mixer);
        app.set_detached(Win::Mixer, true);
        let (w, h) = app.state.detached_size(Win::Mixer).unwrap();
        let Ok(mut c) = Canvas::new(w as u32, h as u32, 1.0) else { return };
        app.paint_detached(Win::Mixer, &mut c);
        app.on_surface(Win::Mixer, |a| {
            a.mouse_move(w / 2.0, h / 2.0);
            a.mouse_down_right();
            a.mouse_up();
            a.state.playlist.popup = Some(crate::mixer::options_menu(&a.state, 20.0, 30.0));
        });
        main_paint(&mut app);
        assert!(app.overlays.is_empty(), "not in the main window");
        app.paint_detached(Win::Mixer, &mut c);
        assert!(!screen(&app, Win::Mixer).overlays.is_empty(), "in the Mixer's window");

        app.state.playlist.popup = None;
        app.mouse_move(800.0, 600.0);
        app.mouse_down();
        app.mouse_up();
        assert_eq!(app.state.overlay_on, None);
        app.state.playlist.popup = Some(crate::mixer::options_menu(&app.state, 800.0, 600.0));
        main_paint(&mut app);
        assert!(!app.overlays.is_empty(), "in the main window");
    }

    /// The Channel rack detaches the same way: its window is the size of what it paints, it is laid out at that window's
    /// top-left while painted or clicked there (and back at its place afterwards), and its resize edges and title bar
    /// work the native window. The native window's size becomes the rack's.
    #[test]
    fn the_channel_rack_detaches_too() {
        let mut app = App::new(None);
        app.state.raise(Win::ChannelRack);
        let pos = app.state.rack_pos;
        app.set_detached(Win::ChannelRack, true);
        let (w, h) = app.state.detached_size(Win::ChannelRack).unwrap();
        let area = views::rack::paint_area(&app.state);
        assert_eq!((w, h), (area.w, area.h));
        assert_eq!(app.detached_origin(Win::ChannelRack).map(|r| (r.x, r.y)), Some((area.x, area.y)));
        let Ok(mut c) = Canvas::new(w as u32, h as u32, 1.0) else { return };
        app.paint_detached(Win::ChannelRack, &mut c);
        assert_eq!(app.state.rack_pos, pos, "its place in the main window is kept");
        let sc = screen(&app, Win::ChannelRack);
        let title = hit_of(&sc.hits, &Action::WinDrag(Win::ChannelRack)).expect("the title bar");
        assert!(title.x < 40.0 && title.y < 30.0, "{title:?}");
        let corner = hit_of(&sc.hits, &Action::RackResize(views::rack::EDGE_RIGHT | views::rack::EDGE_BOTTOM)).expect("the corner");
        app.on_surface(Win::ChannelRack, |a| {
            a.mouse_move(corner.cx(), corner.cy());
            a.mouse_down();
        });
        assert_eq!(app.native, vec![(Win::ChannelRack, Native::Resize(EDGE_R | EDGE_B))]);
        assert!(app.drag.is_none(), "the window system resizes it");
        app.on_surface(Win::ChannelRack, App::mouse_up);
        main_paint(&mut app);
        assert!(!app.hits.iter().any(|h| h.win == Some(Win::ChannelRack)), "out of the main window");

        app.set_detached_size(Win::ChannelRack, (w + 96.0, h + 40.0));
        assert_eq!(app.state.detached_size(Win::ChannelRack), Some((w + 96.0, h + 40.0)), "the rack takes its window's size");
        app.set_detached(Win::ChannelRack, false);
        assert_eq!(app.state.rack_pos, pos);
    }

    /// A shortcut pressed while the detached Mixer's window has the keyboard acts there, and the box it opens (F2's
    /// rename) is painted in that window, though the last click was in the main window. Pressed while the main window
    /// has the keyboard, the box is the main window's.
    #[test]
    fn keyboard_popups_go_to_the_window_with_the_keyboard() {
        use winit::keyboard::{Key, NamedKey};
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.raise(Win::Mixer);
        app.set_detached(Win::Mixer, true);
        let (w, h) = app.state.detached_size(Win::Mixer).unwrap();
        let Ok(mut c) = Canvas::new(w as u32, h as u32, 1.0) else { return };
        app.paint_detached(Win::Mixer, &mut c);
        assert_eq!(app.state.overlay_on, None, "no click in it yet");
        app.key_on(Some(Win::Mixer), &Key::Named(NamedKey::F2), false, false);
        assert!(app.state.playlist.edit.is_some(), "F2 renames the mixer track");
        assert_eq!(app.state.overlay_on, Some(Win::Mixer));
        main_paint(&mut app);
        assert!(app.overlays.is_empty(), "not in the main window");
        app.paint_detached(Win::Mixer, &mut c);
        assert!(!screen(&app, Win::Mixer).overlays.is_empty(), "in the Mixer's window");

        app.key_on(Some(Win::Mixer), &Key::Named(NamedKey::Escape), false, false);
        assert!(app.state.playlist.edit.is_none());
        app.key_on(None, &Key::Named(NamedKey::F2), false, false);
        assert!(app.state.playlist.edit.is_some());
        assert_eq!(app.state.overlay_on, None, "opened with the main window's keyboard");
    }

    /// A browser effect dragged out of the main window over the detached Mixer's window moves there (its ghost with it)
    /// and drops into the slot under the cursor, though the main window, which the drag started in, gets the release.
    #[test]
    fn a_browser_effect_drops_into_a_detached_mixer() {
        use crate::views::browser::{rows, Item};
        let mut app = App::new(None);
        app.state.project = test_project();
        app.plugins.effects = vec![crate::plugin_db::fake("Test FX", os_host::Kind::Effect)];
        app.state.plugdb.rebuild(&app.plugins);
        app.state.browser_tab = 2;
        app.state.browser_open.extend(["2:Plugin library/Effects".into(), "2:Plugin library/Effects/Other".into()]);
        app.state.raise(Win::Mixer);
        app.set_detached(Win::Mixer, true);
        let Some(_) = main_paint(&mut app) else { return };
        let (w, h) = app.state.detached_size(Win::Mixer).unwrap();
        let mut c = Canvas::new(w as u32, h as u32, 1.0).unwrap();
        app.paint_detached(Win::Mixer, &mut c);
        let i = rows(&app.state).iter().position(|r| r.item == Item::Effect(0)).expect("effect row");
        let src = hit_of(&app.hits, &Action::BrowserRow(i)).expect("the browser row");
        let dst = hit_of(&screen(&app, Win::Mixer).hits, &Action::MixerArea).expect("the mixer");
        app.mouse_move(src.cx(), src.cy());
        app.mouse_down();
        app.mouse_move(src.cx() + 30.0, src.cy());
        assert!(app.cross_drag());
        assert_eq!(app.state.drag_on, None, "over the main window");
        app.on_surface(Win::Mixer, |a| a.mouse_move(dst.cx(), dst.cy()));
        assert_eq!(app.state.drag_on, Some(Win::Mixer), "over the Mixer's window");
        assert!(app.detached_dirty(Win::Mixer), "its ghost is drawn there");
        app.paint_detached(Win::Mixer, &mut c);
        app.mouse_up_from(None);
        assert!(app.drag.is_none() && app.state.drag_on.is_none());
        let t = app.state.selected_mixer;
        assert_eq!(crate::mixer::slot_name(&app.state, t, 0).as_deref(), Some("Test FX"));
    }

    /// A browser generator dragged onto the detached Channel rack's window adds a channel; dragged back over the main
    /// window before the release, it drops there instead.
    #[test]
    fn a_browser_generator_drops_into_a_detached_rack() {
        use crate::views::browser::{rows, Item};
        let mut app = App::new(None);
        app.state.project = test_project();
        app.plugins.available.push(crate::plugin_db::fake("Test Synth", os_host::Kind::Instrument));
        let synth = app.plugins.available.len() - 1;
        app.state.plugdb.rebuild(&app.plugins);
        app.state.browser_tab = 2;
        app.state.browser_open.extend(["2:Plugin library/Generators".into(), "2:Plugin library/Generators/Other".into()]);
        app.state.raise(Win::ChannelRack);
        app.set_detached(Win::ChannelRack, true);
        let Some(_) = main_paint(&mut app) else { return };
        let (w, h) = app.state.detached_size(Win::ChannelRack).unwrap();
        let mut c = Canvas::new(w as u32, h as u32, 1.0).unwrap();
        app.paint_detached(Win::ChannelRack, &mut c);
        let Some(i) = rows(&app.state).iter().position(|r| r.item == Item::Generator(synth)) else { panic!("generator row") };
        let src = hit_of(&app.hits, &Action::BrowserRow(i)).expect("the browser row");
        let n = app.state.project.channels.len();
        let drag = |app: &mut App, back: bool| {
            app.mouse_move(src.cx(), src.cy());
            app.mouse_down();
            app.mouse_move(src.cx() + 30.0, src.cy());
            app.on_surface(Win::ChannelRack, |a| a.mouse_move(w / 2.0, h / 2.0));
            if back {
                app.mouse_move(src.cx() + 30.0, src.cy());
            }
            app.mouse_up_from(None);
        };
        drag(&mut app, true);
        assert_eq!(app.state.project.channels.len(), n, "dropped back on the browser");
        drag(&mut app, false);
        assert_eq!(app.state.project.channels.len(), n + 1, "a new channel");
    }

    /// A scroll-bar right-drag in the detached Mixer holds the cursor as in the main window: the moves the shell gives
    /// that surface (`App::on`) scroll it, the main window's mouse stays put, and the release ends it.
    #[test]
    fn a_scroll_bar_holds_the_cursor_in_a_detached_window() {
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.raise(Win::Mixer);
        app.set_detached(Win::Mixer, true);
        app.set_detached_size(Win::Mixer, (500.0, 400.0));
        let Ok(mut c) = Canvas::new(500, 400, 1.0) else { return };
        app.paint_detached(Win::Mixer, &mut c);
        let Some(bar) = screen(&app, Win::Mixer).bars.first().cloned() else { panic!("the mixer's scroll bar") };
        app.mouse_move(3.0, 3.0);
        let (x, y) = (bar.track.cx(), bar.track.cy());
        app.on(Some(Win::Mixer), |a| {
            a.mouse_move(x, y);
            a.mouse_down_right();
        });
        assert!(app.cursor_locked(), "held");
        let probe = |a: &App| format!("{:?}", (a.scroll_get(bar.target), a.zoom_get(bar.target), a.rows_get(bar.target)));
        let before = probe(&app);
        for k in 1..=10 {
            let d = k as f32 * 8.0;
            app.on(Some(Win::Mixer), |a| a.mouse_move(x + d, y + d));
        }
        assert_eq!(app.mouse, (3.0, 3.0), "the main window's mouse is its own");
        assert_ne!(probe(&app), before, "it scrolled");
        app.mouse_up_from(Some(Win::Mixer));
        assert!(!app.cursor_locked());
    }
