// Copyright (C) 2026 ItsAlphaNeon
// Copyright (C) 2026 thacer13
// Copyright (C) 2026 Samrat Dutta
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Application state, actions and the per-frame paint/hit-test pass.
//!
//! Painting is immediate: every frame repaints from `State` and records hit regions. Hover uses
//! the topmost hit from the previous frame, so occluded controls never highlight.

use crate::control::command::Region;
use crate::history::ChannelControl;
use std::borrow::Cow;
use std::path::PathBuf;

use os_engine::PlayMode;
use os_model::{Channel, ChannelKind, MixerTrack, Note, Pattern, Project};
use os_theme::{Design, Palette, Theme};
use os_ui::{Canvas, Rect};
use winit::keyboard::KeyCode;

use crate::piano::{self, Geom, Overlay, PianoState, Tool};
use crate::transport::{State as TransportState, Transport};
use crate::views;

/// Child windows. Playlist, Mixer and Piano roll float by default and can be moved, resized,
/// minimized (title bar only) and maximized (filling the workspace); the Channel rack and the
/// channel plugin window float. All share one stacking order (`State::z`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Win {
    Playlist,
    Mixer,
    ChannelRack,
    PianoRoll,
    Browser,
    Plugin,
    /// The Plugin performance monitor (spec: ui/plugin-performance.md).
    Perf,
    /// The Tempo tapper (spec: ui/tempo-tapper.md).
    Tapper,
    /// The Touch controller (spec: ui/touch-controller.md).
    Touch,
    /// The piano roll's Strum dialog (spec: ui/piano-strum.md).
    Strum,
    /// The piano roll's Arpeggiate dialog (spec: ui/piano-arpeggiate.md).
    Arp,
    /// The piano roll's Flam, Flip and Scale levels dialogs, one at a time (spec: ui/piano-flam.md, ui/piano-flip.md,
    /// ui/piano-level-scaling.md).
    PianoDlg,
    /// The event editor (spec: ui/event-editor.md).
    Events,
    /// A container's window: its effects with their wet and dry knobs (Lemon Studio's design: spec
    /// plugins/patcher/README.md §4.3).
    Container,
    /// A docked hosted-plugin window (a native child window; rack row), stacked with the drawn windows.
    Hosted(usize),
}

impl Win {
    /// Index into `State::wins` for the windows with a stored rect, move and resize.
    pub fn slot(self) -> Option<usize> {
        match self {
            Win::Playlist => Some(0),
            Win::Mixer => Some(1),
            Win::PianoRoll => Some(2),
            _ => None,
        }
    }

    /// Smallest outer size (the layouts stay intact down to this).
    pub fn min_size(self) -> (f32, f32) {
        match self {
            Win::Playlist => (520.0, 220.0),
            Win::Mixer => (560.0, 400.0),
            Win::PianoRoll => (540.0, 320.0),
            _ => (0.0, 0.0),
        }
    }
}

/// The three VIEW > Layout close commands (spec: ui/close-windows.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseAll {
    /// Every window (F12).
    All,
    /// Every window but the focused one (Ctrl+F12).
    Unfocused,
    /// The channel settings window and the hosted plugin windows (Alt+F12).
    Plugins,
}


/// Resize edge bits for `Action::WinResize`.
pub const EDGE_L: u8 = 1;
pub const EDGE_R: u8 = 2;
pub const EDGE_T: u8 = 4;
pub const EDGE_B: u8 = 8;

/// Where a Playlist / Mixer / Piano roll window sits when floating, and its title-bar states.
#[derive(Clone, Copy)]
pub struct WinState {
    pub rect: Rect,
    /// Fills the workspace.
    pub maxed: bool,
    /// Collapsed to its title bar.
    pub mini: bool,
}

/// Title bar height of the three docked-style windows.
pub const TITLE_H: f32 = 23.0;

/// Toolbar utility buttons whose left and right clicks differ (spec: ui/toolbar-buttons.md §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Util {
    Undo,
    Help,
    Save,
    /// The audio editor button: Tesla on the master track; `NewAudioEditor` is its right click.
    AudioEditor,
    NewAudioEditor,
}

/// Mouse cursor shapes the shell should show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cursor {
    Default,
    EwResize,
    NsResize,
    NwseResize,
    NeswResize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Settings window (spec: ui/settings.md).
    Settings(views::settings::SAct),
    /// A control that is drawn but not functional yet (`Ui::off`): hovering and clicking only say so.
    Placeholder(&'static str),
    /// Typing keyboard to piano switch (toolbar, OPTIONS menu, Ctrl+T).
    Typing,
    /// Metronome switch (toolbar, OPTIONS menu, Ctrl+M); right-click on the button picks the sound.
    Metronome,
    /// The Record button (spec: ui/recording.md §2); right-click for its options.
    Record,
    /// A record option's switch: Start on input, Recording precount, Blend (spec: ui/recording.md §3).
    RecOpt(crate::record::Opt),
    /// The toolbar's microphone: One-click audio recording's menu, or Stop while it records (spec:
    /// ui/one-click-recording.md §2).
    OneClick,
    /// Auto scrolling switch (toolbar, OPTIONS menu); right-click on the button for continuous scrolling.
    AutoScroll,
    /// Enable groups switch (toolbar, OPTIONS menu, Shift+Alt+G).
    EnableGroups,
    /// Multilink to controllers (toolbar button; spec: ui/remote-control.md §4).
    Multilink,
    /// Step edit switch (toolbar, OPTIONS menu, Ctrl+E; spec: ui/step-edit.md).
    StepEdit,
    /// The toolbar's song position slider (spec: ui/song-position-slider.md).
    SongPos,
    /// The time display (song position read-out): a click toggles its mode, right-click opens its menu.
    Clock,
    /// The toolbar's snap combo: the Snap menu; right-click: Stock note lengths; wheel: the next / previous setting
    /// (spec: ui/global-snap.md §1).
    GlobalSnap,
    /// The toolbar's snap magnet: snapping on / off (spec: ui/global-snap.md §1).
    GlobalSnapToggle,
    /// The toolbar's scope / spectrum panel: click switches the display.
    Monitor,
    /// The toolbar's CPU panel: double-click opens the Plugin performance monitor, right-click its menu.
    CpuPanel,
    /// The Plugin performance monitor's controls (spec: ui/plugin-performance.md).
    Perf(crate::perf::PerfAct),
    /// The Tempo tapper's pad and Apply tempo switch (spec: ui/tempo-tapper.md).
    Tapper(crate::tapper::TapAct),
    /// The Touch controller's controls (spec: ui/touch-controller.md).
    Touch(crate::touch::TouchAct),
    /// The Strum dialog's switches and buttons (spec: ui/piano-strum.md).
    Strum(crate::strum::Act),
    /// The container window's names and buttons (`crate::container::CAct`).
    Container(crate::container::CAct),
    /// The Arpeggiate dialog's buttons, switches and menus (spec: ui/piano-arpeggiate.md).
    Arp(crate::arpeggiate::Act),
    /// The Flam, Flip and Scale levels dialogs' switches and buttons (spec: as `Win::PianoDlg`).
    PianoDlg(crate::piano_dlg::Act),
    /// The event editor's buttons and areas (spec: ui/event-editor.md).
    Events(crate::events::EvAct),
    /// Monitor debug panel: a slider (index into `monitor::PARAMS`), Reset, Save, Close.
    MonParam(usize),
    MonReset,
    MonSave,
    MonClose,
    /// Cable physics debug panel (views/cabledebug.rs).
    CableDebug(crate::cable::DebugAct),
    Minimize,
    Maximize,
    Close,
    DragWindow,
    /// The toolbar editor's panels, grips, palette tiles and buttons (spec: ui/toolbar-menu.md §8).
    Bar(crate::toolbar::BarHit),
    Play,
    Stop,
    PatSong(bool),
    Tempo,
    Menu(usize),
    MenuItem(usize, usize),
    /// A ▸ entry of a menu bar dropdown (menu, flat item index): opens its submenu (`views::menus::submenu`).
    MenuSub(usize, usize),
    ToggleWin(Win),
    PatternPrev,
    PatternNext,
    PatternAdd,
    Fader(u16),
    /// A mixer control (spec: ui/mixer.md).
    Mix(crate::mixer::MHit),
    ChannelSelect(usize),
    ChannelEnable(usize),
    Step(usize, u32),
    /// Channel rack: activity / select slot, mixer track box (drag), add channel, resize edges
    /// (`views::rack::EDGE_*` bits).
    RackSelect(usize),
    RackMixer(usize),
    RackAdd,
    /// A rotary control (drag vertically, wheel steps).
    Knob(crate::knobs::KnobId),
    /// Toolbar plugin button: the plugin menu (spec: ui/plugin-menu.md §1).
    PickerOpen,
    /// Toolbar project button: the project picker (spec: ui/project-picker.md §2).
    ProjectPicker,
    /// A toolbar utility button (spec: ui/toolbar-buttons.md §2).
    Util(Util),
    /// Show a window maximized and in front (the playlist / piano roll buttons' right click, spec:
    /// ui/toolbar-buttons.md §2).
    ShowMax(Win),
    RackResize(u8),
    /// Step / piano-roll view toggle, and a row's mini piano roll (spec: ui/channel-rack.md §6).
    RackRollView,
    RackRoll(usize),
    /// Floating window title bar (drag to move) and close button.
    WinDrag(Win),
    CloseWin(Win),
    /// Title-bar buttons of Playlist / Mixer / Piano roll, and their resize edges (`EDGE_*` bits).
    MinWin(Win),
    MaxWin(Win),
    WinResize(Win, u8),
    /// Browser tree row (index into `views::browser::rows`); press selects / toggles / starts a drag.
    BrowserRow(usize),
    /// Browser tab 0..5 (spec: ui/browser.md §1).
    BrowserTab(usize),
    /// Browser list background (wheel scrolls).
    BrowserList,
    /// Title-bar up arrow: close every node of the tab.
    BrowserCollapse,
    /// Title-bar curved arrow: Reread structure (spec: ui/browser.md §4, "Title-bar icons").
    BrowserReread,
    /// Title-bar ▸: the browser options menu (spec: ui/browser.md §4).
    BrowserMenu,
    /// The search field (focus it, §5).
    BrowserSearch,
    /// The cross right of the search field while it has text: clears it (§5).
    BrowserClear,
    /// The splitter right of the browser: drag to resize it, double-click for the default width (§9).
    BrowserSplitter,
    PlaylistArea,
    MixerArea,
    RackArea,
    Swallow,
    /// The value box of "Enter value..." (spec: ui/type-in-value.md).
    TypeIn(crate::typein::TiAct),
    /// The Remote control settings box (spec: ui/remote-control.md).
    Remote(crate::remote::RAct),
    /// Piano roll: empty grid, a note (index into the pattern's notes), keyboard, velocity panel.
    PianoGrid,
    PianoNote(usize),
    PianoKeys,
    PianoVelocity,
    /// The band between the grid and the event panel (spec: ui/event-panel-splitter.md).
    PianoSplitter,
    /// Event panel "Control" button and its menu entries (index into `Lane::ALL`).
    PianoLaneMenu,
    /// Toolbar "Piano roll tools" button: the Tools menu (spec: ui/piano-roll.md §11).
    PianoToolsMenu,
    /// Toolbar magnet: the Snap menu; right-click: Stock note lengths (spec: ui/piano-snap.md §1).
    PianoSnapMenu,
    /// The channel name in the title: the channel menu; right-click: channel settings; wheel: channels with notes
    /// (spec: ui/piano-target-channel.md).
    PianoTarget,
    PianoLanePick(usize),
    /// Arrow button of the pitch lane's scroll bar: +1 up, -1 down.
    PitchStep(i32),
    PianoUndo,
    /// Piano-roll toolbar: make a tool active (spec: ui/piano-roll.md §9).
    PianoTool(Tool),
    /// Piano-roll time ruler: left-drag moves the play-start marker, right-drag selects a time
    /// range (spec: ui/piano-roll.md §10).
    PianoRuler,
    /// A scroll bar (left-drag scrolls; right-drag scrolls horizontally and zooms vertically).
    Scrollbar(Scroll),
    /// A scroll bar's arrow button: steps the axis by an eighth of its view (-1 back, 1 forward).
    ScrollStep(Scroll, i32),
    /// Playlist (spec: ui/playlist.md): title-bar tools, snap magnet, ▸ options menu, time ruler, marker lane,
    /// track header name area and mute LED, picker tabs and items, popup menu items (top level, submenu).
    PlTool(crate::playlist::PlTool),
    PlSnap,
    PlOptions,
    PlRuler,
    PlMarkers,
    PlHeader(u16),
    PlLed(u16),
    /// A group track's collapse arrow (spec: ui/playlist.md §6a).
    PlGroupArrow(u16),
    /// The arrangement name in the playlist title: the arrangement menu (spec: ui/playlist.md §12).
    PlArrangement,
    /// Stretch mode switch in the playlist title (Shift+M, spec: ui/playlist.md §9b).
    PlStretch,
    PlPickTab(crate::playlist::PickKind),
    PlPick(os_model::ClipSource),
    /// Picker list background / footer "+".
    PlPickList,
    PlPickAdd,
    PlPopup(usize),
    PlPopupSub(usize),
    /// Channel rack title bar (spec: ui/channel-rack.md §7): options ▸, display filter combo, name-width dots,
    /// speaker (play / pause pattern), pattern length box, loop mode, graph editor; per-row loop boxes (§15),
    /// graph editor area and tabs (§14), Advanced fill dialog (§22).
    RackOptions,
    RackFilter,
    RackDots,
    RackSpeaker,
    RackLen,
    RackLoop,
    RackGraph,
    RackLoopBox(usize),
    RackGraphArea,
    RackGraphTab(usize),
    RackFill(crate::rack::FillHit),
    /// Pattern name box, suggestions, swatches, delete confirmation (spec: ui/pattern-menu.md).
    Pat(crate::patterns::Hit),
    /// A message box or the file dialog (spec: ui/dialogs.md).
    Dlg(crate::dialog::DHit),
    /// The Song info box: a click closes it (spec: flp/events/project-info.md "The Song info box").
    SongInfo,
}

/// Scrollable view axes. Scroll values are in content pixels (see `App::scroll_get`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scroll {
    PianoX,
    PianoY,
    /// The channel pitch lane's window (content = ±4800 cents).
    PitchY,
    PlaylistX,
    PlaylistY,
    MixerX,
    RackX,
    RackY,
    /// Playlist picker list.
    PlPick,
    /// The settings window's page (spec: ui/settings.md).
    Settings,
    /// Settings ▸ Plugins: the plugin table (whole rows).
    PlugTable,
    /// A popup menu wider than the window, and its open submenu (whole columns; spec: ui/menus.md "Menus wider than
    /// the window").
    Popup,
    PopupSub,
}

/// A scroll bar painted this frame.
#[derive(Debug, Clone, Copy)]
pub struct Bar {
    pub target: Scroll,
    pub track: Rect,
    pub vertical: bool,
    /// Largest scroll value (content pixels).
    pub max: f32,
    /// Visible extent along the axis (content pixels).
    pub view: f32,
    /// Thumb length in pixels.
    pub thumb: f32,
}

impl Bar {
    fn len(&self) -> f32 {
        if self.vertical { self.track.h } else { self.track.w }
    }

    /// Content pixels per pixel of thumb travel.
    fn ratio(&self) -> f32 {
        let travel = self.len() - self.thumb;
        if travel > 0.0 { self.max / travel } else { 0.0 }
    }

    /// Where the thumb starts along the track at scroll `pos`.
    fn thumb_at(&self, pos: f32) -> f32 {
        if self.max > 0.0 { pos / self.max * (self.len() - self.thumb) } else { 0.0 }
    }

    /// The thumb end under `along` (pixels along the track) on a bar whose thumb ends zoom (spec: ui/scroll-bars.md
    /// §4): Some(true) = the right end. The grab zone reaches `BAR_EDGE_PX` outside the end and up to that far inside,
    /// leaving at least half a small thumb to scroll by.
    fn edge_at(&self, pos: f32, along: f32) -> Option<bool> {
        if !matches!(self.target, Scroll::PlaylistX | Scroll::PianoX) {
            return None;
        }
        let (a, b) = (self.thumb_at(pos), self.thumb_at(pos) + self.thumb);
        let inner = BAR_EDGE_PX.min(self.thumb / 4.0);
        if (a - BAR_EDGE_PX..a + inner).contains(&along) {
            Some(false)
        } else if (b - inner..=b + BAR_EDGE_PX).contains(&along) {
            Some(true)
        } else {
            None
        }
    }
}

/// Grab zone at each end of a zooming scroll bar's thumb, in pixels (spec: ui/scroll-bars.md §4).
const BAR_EDGE_PX: f32 = 4.0;
/// Hint over a zooming thumb end.
const BAR_EDGE_HINT: &str = "Drag to zoom - the other end stays put";

/// What needs repainting before the next present.
#[derive(Clone, PartialEq)]
enum Damage {
    None,
    /// Separate regions (a playhead, the clock and a meter are far apart: one bounding box would cover most of the
    /// frame).
    Rects(Vec<Rect>),
    Full,
}

/// Right-drag sideways on a vertical scroll bar: the rows grow by this factor per [`BAR_ROW_PX`] moved right
/// (the piano roll's Ctrl+middle-drag rate, spec: ui/scroll-bars.md).
const BAR_ROW_STEP: f32 = 1.15;
const BAR_ROW_PX: f32 = 12.0;

/// The window a scroll target scrolls (`None`: the settings window, repainted whole).
pub(crate) fn scroll_win(t: Scroll) -> Option<Win> {
    match t {
        Scroll::PianoX | Scroll::PianoY | Scroll::PitchY => Some(Win::PianoRoll),
        Scroll::PlaylistX | Scroll::PlaylistY | Scroll::PlPick => Some(Win::Playlist),
        Scroll::MixerX => Some(Win::Mixer),
        Scroll::RackX | Scroll::RackY => Some(Win::ChannelRack),
        Scroll::Settings | Scroll::PlugTable | Scroll::Popup | Scroll::PopupSub => None,
    }
}

/// The smallest scroll step whose move is whole device pixels on the playlist's drawing grid (1/256 px) at `scale`:
/// 1 px at 100 %, 2 at 150 %, 4 at 125 % and 175 %. None at a scale off that grid (no move is exact there).
fn blit_step(scale: f32) -> Option<f32> {
    (0..=16).map(|e| 2f32.powi(e - 8)).find(|q| (f64::from(*q) * f64::from(scale)).fract() == 0.0)
}

/// `r` moved by (dx, dy).
fn moved_by(r: Rect, dx: f64, dy: f64) -> Rect {
    Rect::new(r.x + dx as f32, r.y + dy as f32, r.w, r.h)
}

/// The parts of `a` outside `b`: up to four rects (above, below, left and right of their overlap).
fn rect_minus(a: Rect, b: Rect) -> Vec<Rect> {
    let o = a.intersect(&b);
    if o.is_empty() {
        return if a.is_empty() { Vec::new() } else { vec![a] };
    }
    let ltrb = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::new(x0, y0, x1 - x0, y1 - y0);
    [
        ltrb(a.x, a.y, a.right(), o.y),
        ltrb(a.x, o.bottom(), a.right(), a.bottom()),
        ltrb(a.x, o.y, o.x, o.bottom()),
        ltrb(o.right(), o.y, a.right(), o.bottom()),
    ]
    .into_iter()
    .filter(|r| !r.is_empty())
    .collect()
}

/// Something drawn over the playlist (`App::playlist_covers`): what it may paint or hit, the window it is (None for
/// the toolbar) and the part of it its frame fills with opaque colour first, while it floats (`App::win_solid`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Cover {
    bounds: Rect,
    win: Option<Win>,
    solid: Option<Rect>,
}

/// A canvas area a paint moved instead of repainting it (a playlist scroll): by `dx`, `dy` device pixels right and
/// down, except the pixels wholly inside `keep` (windows over it that did not change, `os_ui::keep_area`).
#[derive(Debug, Clone, PartialEq)]
pub struct Moved {
    pub area: Rect,
    pub dx: i32,
    pub dy: i32,
    pub keep: Vec<Rect>,
}

/// Past this many separate damage rects, the two whose merge adds the least area are merged.
const MAX_DAMAGE_RECTS: usize = 32;

/// Add `r` to a damage list, merging it with rects it overlaps or nearly touches when the merge adds little area.
fn add_damage(v: &mut Vec<Rect>, r: Rect) {
    if r.is_empty() {
        return;
    }
    let area = |r: &Rect| r.w * r.h;
    let mut r = r;
    loop {
        let mut merged = false;
        v.retain(|o| {
            let u = o.union(&r);
            if area(&u) <= area(o) + area(&r) + 2048.0 {
                r = u;
                merged = true;
                false
            } else {
                true
            }
        });
        if !merged {
            break;
        }
    }
    v.push(r);
    while v.len() > MAX_DAMAGE_RECTS {
        let mut best = (f32::MAX, 0, 1);
        for i in 0..v.len() {
            for j in i + 1..v.len() {
                let cost = area(&v[i].union(&v[j])) - area(&v[i]) - area(&v[j]);
                if cost < best.0 {
                    best = (cost, i, j);
                }
            }
        }
        let b = v.swap_remove(best.2);
        v[best.1] = v[best.1].union(&b);
    }
}

/// Regions whose look follows playback; views register them while painting (`Ui::live`) and `App::tick` repaints
/// just them while playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Live {
    /// The song position display.
    Clock,
    /// Lights for the notes sounding now (rack select slots, piano-roll keys).
    Sounding,
    /// Mixer peak meters.
    Meter,
}

/// The playback state the frame shows. Taken in `App::tick`, where the damage for playheads is worked out, so a paint
/// draws them exactly where the damage expects them.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Shown {
    pub playing: bool,
    pub paused: bool,
    pub tick: f64,
    pub song: bool,
    pub sounding: Vec<(u16, u16)>,
    /// The tempo playback plays at (tempo automation moves it); `None` while stopped.
    pub tempo: Option<f64>,
}

/// The hint panel (spec §2): its text changes whenever the hovered control does.
pub(crate) const HINT_RECT: Rect = Rect::new(73.0, 5.0, 289.0, 31.0);
/// The toolbar's output monitor (scope / analyzer and peak bars).
const MONITOR_RECT: Rect = Rect::new(880.0, 4.0, 168.0, 33.0);
/// The toolbar's CPU panel.
pub(crate) const CPU_RECT: Rect = Rect::new(1053.0, 4.0, 149.0, 32.0);
/// What a tempo change repaints in the toolbar (the tempo box and the transport around it).
const TEMPO_RECT: Rect = Rect::new(380.0, 0.0, 200.0, 83.0);

/// Actions whose hover state has no visual feedback (large background regions).
fn hover_is_visible(a: &Action) -> bool {
    !matches!(
        a,
        Action::DragWindow
            | Action::PlaylistArea
            | Action::MixerArea
            | Action::Mix(crate::mixer::MHit::Strip(_) | crate::mixer::MHit::Scale)
            | Action::RackArea
            | Action::Swallow
            | Action::PianoGrid
            | Action::PianoNote(_)
            | Action::PianoKeys
            | Action::PianoVelocity
            | Action::PianoRuler
            | Action::PlRuler
            | Action::PlMarkers
            | Action::PlHeader(_)
            | Action::Scrollbar(_)
            | Action::RackResize(_)
            | Action::RackGraphArea
            | Action::WinDrag(_)
            | Action::WinResize(..)
            | Action::Tapper(crate::tapper::TapAct::Pad)
    )
}

/// Pixels from a note's right end that grab the end for resizing (spec: ui/piano-roll.md §8).
fn resize_zone(note_w: f32) -> f32 {
    (note_w / 3.0).clamp(2.0, 8.0)
}

pub struct Hit {
    pub rect: Rect,
    pub action: Action,
    pub hint: Cow<'static, str>,
    /// The child window the control belongs to (clicking raises it).
    pub win: Option<Win>,
    /// A menu row the wheel steps to (`App::menu_wheel`): which menu it is in, 0 = not a menu row.
    pub menu: u8,
    /// A menu row in a column scrolled out of view (`Ui::hidden_menu_row`): the menu's scroll that shows it.
    pub reveal: Option<usize>,
}

/// A part of the frame that paints independently: skipped by a repaint whose damage misses where it drew.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seg {
    Browser,
    Win(Win),
    Toolbar,
    /// The playlist's clip source list (inside `Win(Playlist)`: a scroll of the grid leaves it alone).
    PlPicker,
}

/// What each part registered and where it drew in the last paint, so a partial repaint can keep the hits of the parts
/// it skips (hits are rebuilt every paint; topmost last).
#[derive(Default)]
pub struct Keep {
    /// Last paint's hits / bars, from which skipped parts take theirs back.
    prev: Vec<Hit>,
    prev_bars: Vec<Bar>,
    prev_live: Vec<(Live, Rect)>,
    parts: Vec<Part>,
    next: Vec<Part>,
    /// This paint covers the whole frame (drawn bounds are complete, nothing is skipped).
    full: bool,
}

/// A detached window's part of the app's input and paint state (`App::on_surface`): swapped in while that window paints
/// or takes input, so its hits, mouse position and hover are its own, in its own logical px.
#[derive(Default)]
struct Screen {
    hits: Vec<Hit>,
    bars: Vec<Bar>,
    live: Vec<(Live, Rect)>,
    keep: Keep,
    overlays: Vec<Rect>,
    mouse: (f32, f32),
    hover: Option<Action>,
    hover_rect: Option<Rect>,
    /// Something it shows changed: the shell paints it again (`App::detached_dirty`).
    dirty: bool,
}

/// What a detached window's drawn frame asks of its native window (Lemon Studio's design, spec: ui/mixer.md §6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Native {
    /// The title bar was pressed: the window system moves the window.
    Move,
    /// A resize zone was pressed (`EDGE_*` bits): the window system resizes the window.
    Resize(u8),
    Minimize,
    Maximize,
}

struct Part {
    seg: Seg,
    hits: std::ops::Range<usize>,
    bars: std::ops::Range<usize>,
    live: std::ops::Range<usize>,
    /// Everything the part drew (logical px), or might draw.
    area: Rect,
}

impl Ui<'_> { // This code is a part of L e m o n S t u d i o.
    /// Paint one part of the frame, or, when it has painted before and `area` (where it paints now) and where it
    /// drew last time both miss the clip, keep last paint's hits for it instead.
    pub fn part(&mut self, seg: Seg, area: Rect, paint: impl FnOnce(&mut Self)) {
        let (h0, b0, l0) = (self.hits.len(), self.bars.len(), self.live.len());
        let old = self.keep.parts.iter().position(|p| p.seg == seg);
        let keep = !self.keep.full
            && old.is_some_and(|i| {
                let p = &self.keep.parts[i];
                p.hits.end <= self.keep.prev.len()
                    && p.bars.end <= self.keep.prev_bars.len()
                    && p.live.end <= self.keep.prev_live.len()
                    && !self.c.visible(p.area.union(&area))
            });
        let area = match old {
            Some(i) if keep => {
                let (hits, bars, live, area) = {
                    let p = &self.keep.parts[i];
                    (p.hits.clone(), p.bars.clone(), p.live.clone(), p.area)
                };
                let dummy = || Hit { rect: Rect::default(), action: Action::Swallow, hint: Cow::Borrowed(""), win: None, menu: 0, reveal: None };
                let prev = &mut self.keep.prev;
                self.hits.extend(hits.map(|k| std::mem::replace(&mut prev[k], dummy())));
                self.bars.extend_from_slice(&self.keep.prev_bars[bars]);
                self.live.extend_from_slice(&self.keep.prev_live[live]);
                area
            }
            _ => {
                // Parts may sit inside another part: that one learns what this one drew too.
                let before = self.c.begin_drawn();
                paint(self);
                let drawn = self.c.end_drawn(before).map_or(area, |d| d.union(&area));
                // A partial paint only saw part of what the view draws: keep last paint's extent as well.
                match old {
                    Some(i) if !self.keep.full => drawn.union(&self.keep.parts[i].area),
                    _ => drawn,
                }
            }
        };
        self.keep.next.push(Part { seg, hits: h0..self.hits.len(), bars: b0..self.bars.len(), live: l0..self.live.len(), area });
    }

    /// Register a region whose look follows playback (repainted while playing, see `Live`).
    pub fn live(&mut self, kind: Live, r: Rect) {
        self.live.push((kind, r));
    }

    /// Paints `paint` at display scale `scale` instead of the frame's (popup menus, spec: ui/ui-scaling.md §1). It works
    /// in logical px of that scale: a frame position `x` is `x / k` there (`k`, handed to it, = `scale` / frame scale);
    /// what it registers (hits, bars, live regions) comes back in the frame's px.
    pub fn scaled(&mut self, scale: f32, paint: impl FnOnce(&mut Self, f32)) {
        let k = scale / self.c.scale;
        if !k.is_finite() || k <= 0.0 || (k - 1.0).abs() < 1e-4 {
            return paint(self, 1.0);
        }
        let (h0, b0, l0, mouse) = (self.hits.len(), self.bars.len(), self.live.len(), self.mouse);
        self.mouse = (mouse.0 / k, mouse.1 / k);
        let old = self.c.rescale(scale);
        paint(self, k);
        self.c.unscale(old);
        self.mouse = mouse;
        let up = |r: Rect| Rect::new(r.x * k, r.y * k, r.w * k, r.h * k);
        for h in &mut self.hits[h0..] {
            h.rect = up(h.rect);
        }
        for b in &mut self.bars[b0..] {
            (b.track, b.thumb) = (up(b.track), b.thumb * k);
        }
        for (_, r) in &mut self.live[l0..] {
            *r = up(*r);
        }
    }
}

/// What views paint with.
pub struct Ui<'a> {
    pub c: &'a mut Canvas,
    pub t: &'a Palette,
    hits: &'a mut Vec<Hit>,
    pub bars: &'a mut Vec<Bar>,
    hover: &'a Option<Action>,
    pressed: &'a Option<Action>,
    pub mouse: (f32, f32),
    /// The window being painted (tags hits).
    pub win: Option<Win>,
    keep: &'a mut Keep,
    live: &'a mut Vec<(Live, Rect)>,
    /// Square toolbar buttons are drawn flat (the Toolbars menu, spec: ui/toolbar-menu.md §3).
    pub flat: bool,
}

impl Ui<'_> {
    /// Whether `action` is held down by the left button with the cursor still over `rect`.
    pub fn down(&self, rect: Rect, action: &Action) -> bool {
        self.pressed.as_ref() == Some(action) && rect.contains(self.mouse.0, self.mouse.1)
    }

    /// Whether `action` is held down by the left button, wherever the cursor is (a drag that began on it).
    pub fn held(&self, action: &Action) -> bool {
        self.pressed.as_ref() == Some(action)
    }

    /// Whether the mouse is over `rect` and last frame's topmost hit there was registered by this window
    /// (for decorations without their own action, e.g. a knob drawn under its hit region).
    pub fn hovering(&self, rect: Rect) -> bool {
        self.hover.is_some() && rect.contains(self.mouse.0, self.mouse.1)
    }

    /// Hits registered so far this frame.
    pub fn hit_count(&self) -> usize {
        self.hits.len()
    }

    /// Where the hits and live regions stand now, for `forget`.
    pub fn mark(&self) -> (usize, usize) {
        (self.hits.len(), self.live.len())
    }

    /// Drops the hits and live regions registered since `mark`: a picture of controls that must not answer (the
    /// toolbar editor's panels and palette tiles, spec: ui/toolbar-menu.md §8).
    pub fn forget(&mut self, (hits, live): (usize, usize)) {
        self.hits.truncate(hits);
        self.live.truncate(live);
    }

    /// Register a hit region; returns whether it is hovered (topmost last frame).
    pub fn hit(&mut self, rect: Rect, action: Action, hint: impl Into<Cow<'static, str>>) -> bool {
        let hovered = self.hover.as_ref() == Some(&action) && rect.contains(self.mouse.0, self.mouse.1);
        self.hits.push(Hit { rect, action, hint: hint.into(), win: self.win, menu: 0, reveal: None });
        hovered
    }

    /// Marks the hit just registered as a row of menu `menu` (1 or more) that the wheel steps to.
    pub fn menu_row(&mut self, menu: u8) {
        if let Some(h) = self.hits.last_mut() {
            h.menu = menu;
        }
    }

    /// A row of menu `menu` in a column scrolled out of view, for the wheel to step to: the menu's scroll `reveal` shows
    /// it at `rect`. Its hit has no height, so the pointer never finds it.
    pub fn hidden_menu_row(&mut self, rect: Rect, action: Action, menu: u8, reveal: usize) {
        let rect = Rect::new(rect.x, rect.cy(), rect.w, 0.0);
        self.hits.push(Hit { rect, action, hint: Cow::Borrowed(""), win: self.win, menu, reveal: Some(reveal) });
    }

    /// A control that is drawn but does nothing yet: its hit only explains that (hint and click message), so it never
    /// lights up, toggles or acts. Draw the control greyed out: `kit::State::disabled`, `menu_pop_dim` text, or `veil`.
    pub fn off(&mut self, rect: Rect, name: &'static str) {
        self.hits.push(Hit { rect, action: Action::Placeholder(name), hint: Cow::Owned(off_text(name)), win: self.win, menu: 0, reveal: None });
    }

    /// Greys out what was just drawn in `rect` (a disabled control without a disabled look of its own): a veil of the
    /// background colour `bg` under it.
    pub fn veil(&mut self, rect: Rect, bg: os_ui::Color) {
        self.c.fill_rect(rect, bg.with_alpha(OFF_VEIL));
    }
}

/// Which menu a row belongs to (`Hit::menu`): a menu bar dropdown, a popup menu, the popup's open submenu.
pub const MENU_DROPDOWN: u8 = 1;
pub const MENU_POPUP: u8 = 2;
pub const MENU_SUB: u8 = 3;

/// Opacity of `Ui::veil`: a greyed-out control keeps about 40 % of its contrast.
pub const OFF_VEIL: u8 = 155;

/// Hint and message of a control or menu entry that is not implemented yet.
pub fn off_text(name: &str) -> String {
    format!("{name}: not available yet")
}

/// What is not available where plugin windows are not (`cfg(plugin_windows)`: Windows, Linux and macOS so far).
#[cfg(not(plugin_windows))]
pub const PLUGIN_WINDOWS: &str = "Plugin windows on this system";

/// The plugin window's menus, preset list and Misc lists are app popups painted in the main window: off Windows a
/// detached window would cover them (Windows lifts them above it).
#[cfg(all(plugin_windows, not(windows)))]
const PLUGIN_POPUPS: &str = "Menus of detached plugin windows on Linux (dock the window)";

#[derive(Debug, Clone)]
pub enum Drag {
    /// Settings > MIDI: a handle of the mapping curve window (0 start, 1 middle, 2 end).
    MidiCurve(u8),
    /// Settings > MIDI: a Port or offset box, a step per 8 px up or down (`steps` taken so far).
    MidiBox { part: crate::views::midi_page::MAct, y0: f32, steps: i32 },
    /// A value box of the Remote control settings box: a step per 8 px up or down (`steps` taken so far).
    LinkBox { part: crate::remote::RAct, y0: f32, steps: i32 },
    /// A Touch controller pad or key held: the note follows the mouse (spec: ui/touch-controller.md §2).
    Touch,
    /// A stroke in the event editor (spec: ui/event-editor.md §3).
    Events(crate::events::Stroke),
    /// Vertical drag on the pattern name box steps the number (spec: ui/pattern-menu.md §1).
    PatternBox { y0: f32, p0: u16 },
    /// Move a floating window (offset of the mouse from its top-left).
    Window { win: Win, dx: f32, dy: f32 },
    /// The toolbar editor: a panel, a palette tile, the logo or a row carried (spec: ui/toolbar-menu.md §8).
    Bar(crate::toolbar::BarDrag),
    /// Resize a Playlist / Mixer / Piano roll window (`r0` = its rect when the drag began).
    WinResize { win: Win, edges: u8, m0: (f32, f32), r0: Rect },
    /// Resize the channel rack from its right / bottom edges.
    RackResize { edges: u8, m0: (f32, f32), s0: (f32, f32) },
    /// Change a channel's mixer track (spec: ui/channel-rack.md §5).
    RackMixer { ch: usize, y0: f32, v0: u16 },
    Fader { track: u16, y0: f32, v0: f64 },
    /// A press on a track's equaliser graph `g`: band `band` follows the cursor.
    EqGraph { track: u16, band: u8, g: Rect },
    /// The Limit dialog's keys held (either button): the key under the mouse becomes the green key; its name strip held:
    /// range end `high` (else the low one) follows; its scroll bar's handle held `dx` px right of its left edge (spec:
    /// ui/piano-limit.md §1, §5).
    LimitKeys,
    LimitEnd { high: bool },
    LimitScroll { dx: f32 },
    /// A plain press on a mixer strip, held: the strip under the cursor is selected as it slides across, to preview
    /// tracks one after another.
    MixSelect,
    Tempo { y0: f32, t0: f64, step: f64 },
    /// The song position slider's handle (`dx` = grab offset from its centre), or the button held on its track (the
    /// next step is due at `next`) (spec: ui/song-position-slider.md §3).
    SongPos { dx: f32 },
    SongPosHold { next: std::time::Instant },
    /// The piano roll's grid / event panel splitter: `dy` = grab offset from the band's top, `top` = the grid's top,
    /// `h` = the height the grid and the panel share.
    PianoSplit { dy: f32, top: f32, h: f32 },
    /// The browser's splitter: `dx` = grab offset from the browser's right edge (spec: ui/browser.md §9).
    BrowserSplit { dx: f32 },
    /// Monitor debug panel slider.
    MonParam(usize),
    /// Cable physics debug panel slider.
    CableParam(usize),
    /// Piano roll: move notes (originals by index), resize their ends, erase, draw velocities.
    NoteMove { x0: f32, y0: f32, orig: Vec<(usize, Note)> },
    NoteResize { x0: f32, orig: Vec<(usize, Note)> },
    NoteErase,
    /// Mute tool swipe: every note touched gets `on` (the first note's new state, §9).
    NoteMute { on: bool },
    /// Paint tool: notes back to back on `key` from `start`, `len` apart (§9).
    NotePaint { key: i32, start: u32, len: u32 },
    /// Select tool marquee from (x0, y0); `base` = selection kept (Ctrl adds).
    Marquee { x0: f32, y0: f32, base: Vec<usize> },
    /// Slice tool line, Zoom tool box, Playback scrub.
    Slice,
    ZoomBox { x0: f32, y0: f32 },
    Scrub { tick: f32 },
    /// Ruler: move the play-start marker; right-drag a time selection from `t0`.
    Marker,
    /// Ruler in SONG mode over a pattern the playlist plays: the song position follows (see `piano_song_seek`).
    SongRuler,
    TimeSel { t0: u32 },
    /// Painting the event panel (note property or channel control points).
    Velocity { last_x: f32, last_y: f32 },
    /// Right-drag in a channel lane deletes its points (a Lemon Studio choice, spec §7b).
    ControlErase { last_x: f32 },
    Knob { id: crate::knobs::KnobId, y0: f32, v0: f32 },
    /// Left button held on the piano keyboard: each key entered is auditioned (§8).
    PianoKeys { key: i32 },
    /// Scroll axes from a start mouse position: value = start + (mouse - m0) × ratio per axis
    /// (ratio < 0 grabs the content, as with the middle button).
    Scroll { m0: (f32, f32), axes: Vec<(Scroll, bool, f32, f32)> },
    /// Ctrl+middle-drag in the piano roll: vertical movement zooms the key rows around `y0` (up = taller rows,
    /// spec: ui/piano-roll.md §3a).
    PianoRowZoom { y0: f32, row0: f32 },
    /// Right-drag on a horizontal scroll bar: horizontal mouse movement scrolls the bar, vertical movement
    /// zooms its axis around the view centre (up = zoom in).
    BarZoom { t: Scroll, m0: (f32, f32), p0: f32, ratio: f32, z0: f32, center: f32, view: f32 },
    /// Left-drag on an end of the Playlist's or Piano roll's horizontal thumb (spec: ui/scroll-bars.md §4): the end
    /// follows the mouse while the other end stays on the same time, so the view zooms. `fixed` = the other end and `units` = the
    /// whole bar, both in content pixels at zoom 1; `grab` = where the mouse held the end, relative to it.
    BarEdge { t: Scroll, right: bool, fixed: f32, units: f32, grab: f32 },
    /// Right-drag on a vertical scroll bar: vertical movement scrolls (as in FL), horizontal movement resizes the
    /// rows around the view centre where the bar has rows (right = taller). `last` = mouse at the previous move,
    /// `row` = unrounded row height so slow drags still add up (None: the bar only scrolls). Spec: ui/scroll-bars.md.
    BarRows { t: Scroll, last: (f32, f32), row: Option<f32> },
    /// A playlist drag (spec: ui/playlist.md).
    Pl(crate::playlist::PlDrag),
    /// A browser item pressed (`started` once it moved far enough), spec: ui/browser.md §3.
    Browser { item: views::browser::Item, label: String, m0: (f32, f32), started: bool },
    /// Channel rack (spec: ui/channel-rack.md): step painting along the start row (§9), slot range select (§8),
    /// name width (§7), pattern length (§7), a row's loop length (§15), graph editor values (§14).
    RackPaint { row: usize, on: bool, last: u32 },
    RackSel { from: usize },
    RackDots { x0: f32, p0: u32 },
    RackLen { y0: f32, v0: u32 },
    RackLoopLen { row: usize, y0: f32, v0: i32 },
    RackGraph,
    /// A dialog's caption (offset of the mouse from its top-left), the file list's scroll bar (`ratio` = list pixels
    /// per pixel moved), spec: ui/dialogs.md.
    Dialog { dx: f32, dy: f32 },
    DialogBar { y0: f32, s0: f32, ratio: f32 },
    /// A slider declared in `crate::prefs` follows the mouse along its track (`w` wide from `x`).
    PrefSlider { label: &'static str, x: f32, w: f32 },
    /// General ▸ Display's PPI ruler, pressed at `x0` while it showed `n0` (`crate::ppi::dragged`).
    Ppi { x0: f32, n0: u16 },
    /// Theme ▸ Adjustments knob `i`, pressed at height `y0` at position `p0` (`crate::theme_adjust`).
    ThemeKnob { i: usize, y0: f32, p0: f32 },
    /// The colour picker's saturation / brightness square and hue strip (their rects).
    PickSv(Rect),
    PickHue(Rect),
}

pub struct State {
    pub project: Project,
    /// The toolbar's scope / analyzer and peak bars (fed from the audio tap in `App::tick`).
    pub monitor: crate::monitor::Monitor,
    /// The plugin database: choices, last scan, the plugin menus' lists, the plugin manager's table (spec:
    /// ui/plugin-manager.md, ui/plugin-menu.md).
    pub plugdb: crate::plugin_db::Db,
    /// The toolbar's CPU panel (read in `App::tick`).
    pub cpu: crate::cpu::Cpu,
    /// The Plugin performance monitor's readings and options (read with the CPU panel while it is shown).
    pub perf: crate::perf::Perf,
    /// The Tempo tapper: where it is, the count of taps, the pad's flash.
    pub tapper: crate::tapper::TapperUi,
    /// The container's window: where it is, which container it shows, the levels its chain plays.
    pub container: crate::container::ContainerUi,
    pub touch: crate::touch::TouchUi,
    /// The event editor: the control it shows, its view, tool and selection.
    pub events: crate::events::EventsUi,
    /// Edit automation in a window: the second event editor, behind `events` (spec: ui/event-editor.md §3a).
    pub events_back: Option<crate::events::EventsUi>,
    /// The piano roll's Strum dialog: where it is, its settings and its preview session.
    pub strum: crate::strum::StrumUi,
    /// The piano roll's Arpeggiate dialog: where it is, its settings and pattern, its preview session.
    pub arp: crate::arpeggiate::ArpUi,
    /// The piano roll's Flam, Flip and Scale levels dialogs: where each was, their settings, the preview session.
    pub piano_dlg: crate::piano_dlg::Ui,
    pub rec: crate::record::Rec,
    /// Mixer selection, slot requests, meters (spec: ui/mixer.md).
    pub mixer: crate::mixer::MixerUi,
    /// Rack rows played by a hosted plugin.
    pub hosted_rows: Vec<usize>,
    /// Requests for the plugin host, handled on the next tick: (row, `Plugins::available` index) to create, row to open.
    pub pending_plugin: Option<(usize, usize)>,
    pub pending_open: Option<usize>,
    /// Samples to load into rows' Lemon Sampler instances on the next tick (browser send, drops on a rack row).
    pub sample_loads: Vec<(usize, String)>,
    /// An undo or redo changed the timebase (`history::Timebase`): playback stops on the next tick.
    pub retimed: bool,
    pub file: Option<os_flp::FlpFile>,
    pub path: Option<PathBuf>,
    /// Open child windows, bottom to top.
    pub z: Vec<Win>,
    /// The empty workspace took the last click: no window has the focus (Ctrl+F12 closes them all,
    /// spec: ui/close-windows.md §2).
    pub no_focus: bool,
    pub show_browser: bool,
    pub rack_pos: (f32, f32),
    pub rack_size: (f32, f32),
    /// Horizontal step scroll of the rack, pixels.
    pub rack_scroll_x: f32,
    /// Channel rack selection, filters, view options, graph editor, loops (spec: ui/channel-rack.md §7-22).
    pub rack: crate::rack::RackUi,
    /// Rows show mini piano rolls instead of steps.
    pub rack_roll_view: bool,
    /// Channel shown in the plugin window, and where that window is.
    pub plugin_channel: usize,
    /// The plugin window was moved by hand: it reopens there instead of next to the rack (spec: ui/channel-rack.md
    /// "Left click").
    pub plugin_moved: bool,
    pub plugin_pos: (f32, f32),
    pub selected_channel: usize,
    pub selected_mixer: u16,
    pub pattern: u16,
    pub song_mode: bool,
    pub menu_open: Option<usize>,
    /// The open dropdown's submenu, shown as `playlist.popup`: the flat index of its ▸ entry.
    pub menu_sub: Option<usize>,
    /// Piano roll event panel Control menu.
    pub lane_menu: bool,
    /// Settings window (OPTIONS > ... settings, F10 / F11).
    pub settings: views::settings::SettingsUi,
    /// The `.flstheme` that colours the Classic design, as its file holds it; `None`: none (the stock Dark look, or a
    /// Lemon Studio `theme.txt`). Theme ▸ Options rewrite it (spec: ui/settings-choices.md "Light mode and clip note
    /// colours").
    pub fl_text: Option<String>,
    /// The theme as it was chosen (a Themes tile, Load theme..., the one kept from the last session), before Theme ▸
    /// Adjustments changed it: their middle clicks restore its values and its tile stays framed (`None`: Default).
    pub theme_base: Option<String>,
    pub playlist_scroll: (f32, f32),
    /// Playlist horizontal zoom, pixels per bar.
    pub playlist_bar_w: f32,
    pub mixer_scroll: f32,
    pub rack_scroll: f32,
    /// Browser (spec: ui/browser.md): active tab, opened node keys, list scroll (px), selected row key,
    /// and the drag label with the mouse position while an item is dragged.
    pub browser_tab: usize,
    pub browser_open: std::collections::HashSet<String>,
    /// Folders dropped on the window: All-tab roots (spec: ui/browser.md §6).
    pub browser_folders: Vec<PathBuf>,
    /// Their Browser names (File ▸ Browser extra search folders, `crate::search_folders`): a root's label in place of
    /// the folder's name.
    pub browser_names: std::collections::BTreeMap<PathBuf, String>,
    /// Directory listings of dropped folders, until reread.
    pub browser_dirs: std::cell::RefCell<std::collections::HashMap<PathBuf, std::rc::Rc<Vec<views::browser::Entry>>>>,
    /// The mixer's routing cables, hung by the mixer view as it paints and swung on the tick.
    pub cables: std::cell::RefCell<crate::cable::Cables>,
    pub browser_scroll: f32,
    pub browser_sel: Option<String>,
    pub browser_drag: Option<(String, f32, f32)>,
    /// Search text and whether the search box has keyboard focus (spec: ui/browser.md §5).
    pub browser_search: String,
    pub browser_search_focus: bool,
    /// Options-menu toggles that are on (`views::browser::OPT_*`).
    pub browser_opts: std::collections::HashSet<&'static str>,
    /// Navigation history of selected row keys and the position in it.
    pub browser_hist: Vec<String>,
    pub browser_hist_pos: usize,
    /// Width of the docked browser incl. its splitter, as dragged (`views::browser_w` keeps it in range), kept between
    /// runs, and the last press on the splitter, for its double click (spec: ui/browser.md §9).
    pub browser_w: f32,
    pub browser_split_click: Option<std::time::Instant>,
    /// The hovered row whose label is scrolling, as last painted (spec: ui/browser.md §9).
    pub browser_marquee: std::cell::RefCell<Option<views::browser::Marquee>>,
    /// Favorite files (spec: ui/browser.md §10) and the index of the browser's disk folders (§11).
    pub browser_favs: Vec<crate::browser::Fav>,
    pub browser_index: crate::browser_index::Index,
    /// Plugin parameter lists and the last tweaked parameters (spec: plugins/plugin-parameters.md).
    pub params: crate::params::ParamUi,
    /// "Copy value": a normalized value for "Paste value", one for every control and automation point, as in FL
    /// (spec: ui/playlist.md §8 "Point menu").
    pub value_clip: Option<f64>,
    pub tempo: f64,
    /// Where the tempo box's tenths, hundredths and thousandths digits start (x), as last painted: a drag from a digit
    /// steps the tempo by that digit (views/toolbar.rs).
    pub tempo_digits: std::cell::Cell<[f32; 3]>,
    /// The time display's mode and middle unit (spec: ui/time-display.md): app-wide, for the session.
    pub clock: crate::clock::Clock,
    /// The Toolbars menu's switches (spec: ui/toolbar-menu.md §8): app-wide, for the session.
    pub toolbar: views::toolbar::Opts,
    /// The toolbar's layout, its editor and the user's presets (spec: ui/toolbar-menu.md §8).
    pub bar: crate::toolbar::Bar,
    pub message: Option<String>,
    pub toggles: Vec<&'static str>,
    /// Settings > MIDI's devices and status (`crate::midi_play`; spec: ui/midi-settings.md).
    pub midi: crate::midi_play::Page,
    /// The Remote control settings box, while open, and Multilink to controllers (`crate::remote`).
    pub link_box: Option<crate::remote::LinkBox>,
    pub multilink: crate::remote::Multi,
    pub maximized: bool,
    pub piano: PianoState,
    pub playlist: crate::playlist::PlaylistState,
    /// The project's undo history (spec: ui/settings-choices.md "Undo history: the four checks (2026-10-16)").
    pub history: crate::history::History,
    /// The control plane: ids, its commands' requests for the app, events, proposals (`crate::control`).
    pub control: crate::control::Control,
    /// The main toolbar's snap, which the editors' Main follows (spec: ui/global-snap.md).
    pub global_snap: crate::snap::Global,
    pub pat_ui: crate::patterns::PatternUi,
    /// Playlist, Mixer, Piano roll (see `Win::slot`).
    pub wins: [WinState; 3],
    /// Size of the shell window in logical pixels (set each paint).
    pub screen: (f32, f32),
    /// The main window's monitor's own scale (device pixels per logical one, whatever Main GUI scaling says; set at
    /// every window event): General ▸ Display's PPI ruler follows it (`crate::ppi`).
    pub display_scale: f32,
    /// The UI's scale at the last paint (the canvas's device px per logical one, Main GUI scaling applied): the top
    /// band's own scale is reckoned against it (`views::band_k`).
    pub ui_scale: f32,
    /// Open message boxes and file dialogs, bottom to top (spec: ui/dialogs.md).
    pub dialogs: Vec<crate::dialog::Modal>,
    /// The working time's clock and the Song info box (Settings > Info, `project_info`).
    pub credits: crate::project_info::Credits,
    /// The current theme's rule for the colours of new audio and automation clips' channels (`App::clip_colors`, kept
    /// up to date by `App::follow_settings`).
    pub clip_colors: crate::clip_colors::ClipColors,
    /// A setting inside a hosted plugin changed since the project was opened or saved (its knobs, its own window): the
    /// project counts as unsaved, as the plugin's state is saved with it.
    pub plugin_edited: bool,
    /// Monotonic editor revision, including successive plugin-only edits between autosaves.
    pub plugin_edit_revision: u64,
    /// Persistent native-project identity used to isolate recovery generations.
    pub recovery_id: [u8; 16],
    /// Windows shown in a native window of their own (Options ▸ Detached; spec: ui/mixer.md §6.2, ui/channel-rack.md
    /// §17; Lemon Studio's design), each with that window's size in logical px. Being in `z` still says whether one is
    /// shown; detached, it is drawn there at (0, 0) and the main window leaves it out (`App::on_surface`).
    pub detached: Vec<(Win, (f32, f32))>,
    /// The detached windows' native frames as the window system places them (outer x, y, inner w, h: points on macOS,
    /// pixels elsewhere), kept for opening them there again (`layout`).
    pub detached_frames: Vec<(Win, [i32; 4])>,
    /// The surface being painted or taking input: `None` the main window, else that detached window.
    pub surface: Option<Win>,
    /// The surface the last press came from: the popups and boxes it opens are drawn there (`App::on_surface`).
    pub overlay_on: Option<Win>,
    /// The surface a drag between windows is over (`App::cross_drag`): its ghost is drawn there and it drops there.
    pub drag_on: Option<Win>,
}

impl State {
    pub(crate) fn mark_plugin_edited(&mut self) {
        self.plugin_edited = true;
        self.plugin_edit_revision = self.plugin_edit_revision.wrapping_add(1);
    }

    /// The applied `.flstheme` (the stock Dark theme when there is none): its option `override_clips` ("Audio and
    /// automation clips use note colors"; spec: ui/settings-choices.md "Light mode and clip note colours").
    pub fn fl_theme(&self) -> os_theme::FlTheme {
        self.fl_text.as_deref().map_or_else(os_theme::FlTheme::dark, os_theme::FlTheme::parse)
    }

    pub fn shown(&self, w: Win) -> bool {
        self.z.contains(&w)
    }

    /// Whether `w` is maximized and not collapsed (it fills the workspace). A detached window never is.
    pub fn is_max(&self, w: Win) -> bool {
        w.slot().is_some_and(|i| self.wins[i].maxed && !self.wins[i].mini) && !self.is_detached(w)
    }

    /// Whether `w` is shown in a native window of its own (Options ▸ Detached).
    pub fn is_detached(&self, w: Win) -> bool {
        self.detached.iter().any(|d| d.0 == w)
    }

    /// The size (logical px) of `w`'s own native window, when it is detached. The Channel rack's follows what it paints
    /// (its size and the graph editor hanging below it, `rack_area`); the Mixer's is the native window's.
    pub fn detached_size(&self, w: Win) -> Option<(f32, f32)> {
        let d = self.detached.iter().find(|d| d.0 == w)?;
        Some(if w == Win::ChannelRack { self.rack_area().1 } else { d.1 })
    }

    /// Smallest size (logical px) of `w`'s native window when detached.
    pub fn detached_min(&self, w: Win) -> (f32, f32) {
        match w {
            Win::ChannelRack => {
                let (_, (aw, ah)) = self.rack_area();
                (views::rack::min_w(self) + (aw - self.rack_size.0).max(0.0), views::rack::MIN_H + (ah - self.rack_size.1).max(0.0))
            }
            w => w.min_size(),
        }
    }

    /// The Channel rack's place in what it paints (`views::rack::paint_area`, with the graph editor hanging below it):
    /// its offset in that area, and the area's size.
    pub fn rack_area(&self) -> ((f32, f32), (f32, f32)) {
        let (r, a) = (views::rack::rect(self), views::rack::paint_area(self));
        ((r.x - a.x, r.y - a.y), (a.w, a.h))
    }

    /// The maximized window in front (the one filling the workspace), if any.
    pub fn docked(&self) -> Option<Win> {
        self.z.iter().rev().copied().find(|&w| self.is_max(w))
    }

    /// The workspace: right of the browser, below the toolbar.
    pub fn work_rect(&self) -> Rect {
        let left = if self.show_browser { views::browser_w(self) } else { 0.0 };
        let top = views::work_y(self);
        Rect::new(left, top, (self.screen.0 - left).max(0.0), (self.screen.1 - top).max(0.0))
    }

    /// The window's floating rect, kept at least its minimum size and at most the screen's, with its part in the
    /// workspace (`win_clamp::keep`).
    pub fn float_rect(&self, w: Win) -> Rect {
        if let (Some(_), Some((dw, dh))) = (w.slot(), self.detached_size(w)) {
            return Rect::new(0.0, 0.0, dw, dh);
        }
        let Some(i) = w.slot() else { return Rect::new(0.0, 0.0, 0.0, 0.0) };
        let r = self.wins[i].rect;
        let (mw, mh) = w.min_size();
        let (sw, sh) = self.screen;
        let rw = r.w.min(sw).max(mw);
        let rh = r.h.min(sh - views::work_y(self)).max(mh);
        let (x, y) = crate::win_clamp::keep(r.x, r.y, rw, self.work_rect());
        Rect::new(x, y, rw, rh)
    }

    /// Where window `w` paints and takes clicks: its rect plus, while it floats, the resize zones past its edges.
    pub fn win_area(&self, w: Win) -> Rect {
        let r = self.win_rect(w);
        match w.slot() {
            Some(i) if !self.wins[i].maxed && !self.wins[i].mini => r.outset(4.0),
            _ => r,
        }
    }

    /// Where the window is painted: the workspace when maximized, a title bar when minimized.
    pub fn win_rect(&self, w: Win) -> Rect {
        // Detached: its whole native window (`App::on_surface`).
        if let (Some(_), Some((dw, dh))) = (w.slot(), self.detached_size(w)) {
            return Rect::new(0.0, 0.0, dw, dh);
        }
        let Some(i) = w.slot() else { return Rect::new(0.0, 0.0, 0.0, 0.0) };
        let f = self.float_rect(w);
        let ws = self.wins[i];
        if ws.mini {
            Rect::new(f.x, f.y, f.w, TITLE_H)
        } else if ws.maxed {
            self.work_rect()
        } else {
            f
        }
    }

    /// Show `w` and bring it to the front.
    pub fn raise(&mut self, w: Win) {
        self.z.retain(|&o| o != w);
        self.z.push(w);
        self.no_focus = false;
    }

    pub fn hide(&mut self, w: Win) {
        self.z.retain(|&o| o != w);
    }

    /// Whether `w` is in front of everything that could cover it: the top window for floating
    /// windows, the top maximized window for maximized ones (floating windows never cover them fully).
    pub fn in_front(&self, w: Win) -> bool {
        if self.is_max(w) { self.docked() == Some(w) } else { self.z.last() == Some(&w) }
    }
}

pub struct App {
    pub state: State,
    pub plugins: crate::plugins::Plugins,
    pub theme: Theme,
    pub transport: Transport,
    pub hits: Vec<Hit>,
    keep: Keep,
    /// Live regions from the last paint.
    live: Vec<(Live, Rect)>,
    /// Playback state the frame shows (see `Shown`).
    shown: Shown,
    /// Rects of last frame's overlays (menus, popups...): docked plugin windows are clipped under them.
    overlays: Vec<Rect>,
    /// Each detached window's own hits, mouse and hover while another surface is active (`on_surface`).
    screens: Vec<(Win, Screen)>,
    /// What the shell is to do with a detached window's native frame: its title bar dragged, its buttons pressed.
    pub native: Vec<(Win, Native)>,
    /// Where the canvas sits in the main window's client area (physical px; the restored window's border).
    pub canvas_off: (i32, i32),
    /// The open popup belongs to this row's detached plugin window: it is shown lifted above that window.
    #[cfg_attr(not(plugin_windows), allow(dead_code))]
    popup_lift: Option<usize>,
    /// The open popup is a menu a hosted plugin asked for (`hosted` key, menu id): closed without a pick, the plugin is
    /// told (`plugin_popup`).
    pub(crate) plugin_menu: Option<(usize, u64)>,
    pub bars: Vec<Bar>,
    pub hover: Option<Action>,
    /// Action under the cursor while the left button is held (pressed look of buttons).
    pub pressed: Option<Action>,
    /// A button pressed and not released yet (`acts_on_release`).
    armed: Option<Action>,
    /// Where that press was: its window comes to the front at the release, as the button acts (`mouse_down`).
    raise_at: Option<(f32, f32)>,
    /// A toolbar button right-pressed and not released yet: (its hit, what the release over it runs).
    armed_right: Option<(Action, Action)>,
    hover_rect: Option<Rect>,
    damage: Damage,
    /// Typing keyboard (spec: ui/typing-keyboard.md): keys held now as (physical key, rack row,
    /// note key), the octave shift, and whether the transport has the project for live notes.
    pub(crate) typing_held: Vec<(KeyCode, usize, u16)>,
    /// Keys held now that launched Performance mode slots (spec: ui/performance-mode.md §3), as (physical key, note).
    perf_typed: Vec<(KeyCode, u8)>,
    pub typing_octave: i32,
    typing_synced: bool,
    /// The piano-roll key held with the mouse, sounding as a live note until released: (rack row, key).
    mouse_note: Option<(usize, u16)>,
    /// When the mixer's cables were last stepped (`None` while they are still).
    cables_at: Option<std::time::Instant>,
    /// Velocity of the next key the mouse sounds (a keyboard click sets it from the click's x; back to the default
    /// after use).
    key_velocity: u8,
    /// MIDI controller input, and its held notes: (key, rack row they started on).
    pub midi: crate::midi_in::MidiIn,
    pub(crate) midi_play: crate::midi_play::Play,
    pub midi_out: crate::midi_out::MidiOut,
    pub(crate) remote: crate::remote::Remote,
    /// Step edit's chord being collected from the typing keyboard and MIDI notes (`crate::step_edit`).
    pub(crate) step_chord: crate::step_edit::Chord,
    /// Last title-bar press (window, time, position), for double-click maximize / restore.
    pub(crate) title_click: Option<(Win, std::time::Instant, (f32, f32))>,
    /// Last channel-rack press (action, time), for double clicks on slots and mixer boxes (§8, §12).
    rack_click: Option<(Action, std::time::Instant)>,
    /// Last press on the CPU panel, for its double click (the Plugin performance monitor).
    cpu_click: Option<std::time::Instant>,
    pub hint: Cow<'static, str>,
    pub mouse: (f32, f32),
    pub drag: Option<Drag>,
    /// A held click that has not moved yet (Click-and-hold & special gesture functions, `crate::hold`).
    pub hold: Option<crate::hold::Hold>,
    /// Views easing to where the wheel sent them (General ▸ Display ▸ Smoothing, `crate::smooth`).
    pub(crate) smooth: crate::smooth::Eases,
    /// Where the shell should move the cursor to (a wheel step in a menu, `menu_wheel`).
    pub warp: Option<(f32, f32)>,
    /// One-click audio recording under way (spec: ui/one-click-recording.md §2).
    pub(crate) audio_rec: Option<crate::one_click::AudioRec>,
    /// Disk recording into the armed mixer tracks under way (spec: audio/track-io.md §2.5).
    pub(crate) track_rec: Option<crate::track_rec::TrackRec>,
    /// TOOLS > Retro audio capture: the last minutes the device played (spec: ui/audio-logger.md).
    pub(crate) audio_log: crate::audio_log::Log,
    /// Wheel movement in an open menu short of a whole notch (trackpads).
    menu_notch: f32,
    /// A Shift+press grabbed piano-roll notes: the drag's first move picks up copies of them (`App::pick_up_notes`).
    shift_clone: bool,
    /// Audio ▸ Auto close: whether the output is closed while another program is in front (`crate::auto_close`).
    pub(crate) auto_close: crate::auto_close::AutoClose,
    pub want_minimize: bool,
    pub want_maximize: bool,
    pub want_close: bool,
    /// The window may close now (unsaved changes were answered): the shell ends the program.
    pub want_exit: bool,
    /// macOS: the answer to a Quit, for AppKit (`macos::answer_quit`).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub quit_answer: Option<bool>,
    /// Linux: while a dialog is open, detached plugin windows are not kept above the main window (they would cover it).
    #[cfg(all(plugin_windows, target_os = "linux"))]
    unpinned: Option<crate::plugin_window::Unpinned>,
    pub want_drag_window: bool,
    /// Show the horizontal-resize cursor (hovering or dragging a note end).
    pub resize_cursor: bool,
    /// Modifier keys held (set by the shell).
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// The right Shift key is held (the playlist's quick slice, ui/playlist.md §3).
    pub rshift: bool,
    /// The key now passed to [`App::key`] is an auto-repeat of a held key (spec: ui/piano-keys.md §4 rule 1).
    pub repeat: bool,
    /// Playlist regions from the last paint (grid, ruler, marker lane).
    pl_grid: Option<Rect>,
    /// Playlist scrolls since the last paint: how far the content moved (logical px, right and down), see
    /// `playlist_scrolled`.
    scrolled: Option<(f64, f64)>,
    /// Canvas areas the next paint moves before it paints: from `take_damage`.
    shifts: Vec<Moved>,
    /// Areas the last paint moved: the shell moves its window buffer the same way (`take_moved`).
    moved: Vec<Moved>,
    /// The canvas scale at the last paint.
    px_scale: f32,
    /// What covered the playlist at the last paint (`playlist_covers`).
    pl_covers: Vec<Cover>,
    /// Piano roll regions from the last paint (window, grid, velocity panel).
    piano_win: Option<Rect>,
    pub(crate) piano_grid: Option<Rect>,
    piano_panel: Option<Rect>,
    /// Sampler channel rows whose Lemon Sampler instance could not be created (not retried).
    pub(crate) sampler_failed: Vec<usize>,
    /// The control plane's socket (`--control`), when it listens.
    pub rpc_server: Option<crate::control::server::Server>,
    /// Mixer slots (`plugins::fx_key`) whose Lemon Parametric EQ could not start (not retried).
    pub(crate) fx_failed: Vec<usize>,
    /// The running plugin scan, and the scan asked for while it runs (spec: ui/plugin-manager.md §4).
    pub(crate) sweep: Option<crate::plugin_scan::Sweep>,
    pub(crate) scan_queued: Option<crate::plugin_db::Mode>,
    /// Wrapped plugin channels (rows) and slots (`fx_key`) whose plugin was not found or did not start (not retried).
    pub(crate) wrap_failed: Vec<usize>,
    /// When the Lemon Trio instances were last looked at for a sample they loaded (`follow_osc3_samples`).
    pub(crate) osc3_look: Option<std::time::Instant>,
    /// Plugin channels (rows) and slots (`fx_key`) naming a native plugin that is not installed or did not start
    /// (not retried; spec: plugins/fruity-abi.md "Opened projects").
    pub(crate) native_failed: Vec<usize>,
    /// An opened project is still loading its samplers (spec: ui/project-loading.md): progress shows, input waits.
    loading: Option<crate::loading::Loading>,
    /// The opened project's missing samples and plugins (`crate::missing`).
    pub(crate) missing: crate::missing::Missing,
    /// A zipped project being unzipped or written (`crate::zipped`): its panel shows, input waits.
    pub(crate) zipping: Option<crate::zipped::Zipping>,
    /// While a project loads: when the plugin hosts stop for this tick (`host_plugins`), and whether one stopped with
    /// instances left to start.
    host_deadline: Option<std::time::Instant>,
    host_more: bool,
    /// An export is rendering (FILE > Export): its first file's name and progress (0..1); the same panel shows, input
    /// waits.
    pub(crate) export: Option<(String, f32)>,
    /// The running export opens the file manager on its first file when done (Open the folder when done).
    export_reveal: bool,
    /// The running export is PATTERNS > Quick render as audio clip / Render and replace's (spec: ui/pattern-render.md):
    /// what to do with its file.
    pub(crate) pattern_render: Option<crate::pattern_render::Pending>,
    /// `device::opens` when the hosted plugins last followed the output device's rate (`follow_device`).
    device_opens: u64,
    /// The project, tempo and Lemon Studio's own saved state (`lemon_state::saved_form`) as last opened, saved or
    /// created: anything else has unsaved changes.
    pub(crate) saved: (Project, f64, Vec<u8>),
    /// The Autosave timer and the last backup (spec: ui/settings-choices.md "Backup files").
    pub(crate) backup: crate::backup::Backup,
}

/// Window-level requests the shell must carry out after an event.
impl App {
    pub fn new(path: Option<PathBuf>) -> App {
        // Each test's app looks for missing samples only in its own project's folders, never another test's.
        #[cfg(test)]
        os_engine::sample::set_thread_search_folders(Vec::new());
        let transport = Transport::new(crate::factory_folder::dir());
        // Tests keep the old five-channel template; a new session starts with one Lemon Sampler channel.
        #[cfg(test)]
        let project = test_project();
        #[cfg(not(test))]
        let project = default_project();
        let mut app = App {
            plugins: crate::plugins::Plugins::default(),
            state: State {
                project,
                plugdb: Default::default(),
                hosted_rows: Vec::new(),
                pending_plugin: None,
                pending_open: None,
                sample_loads: Vec::new(),
                retimed: false,
                file: None,
                path: None,
                z: vec![Win::Playlist, Win::ChannelRack],
                no_focus: false,
                show_browser: true,
                rack_pos: crate::arrange::RACK_POS,
                rack_size: views::rack::DEFAULT_SIZE,
                rack_scroll_x: 0.0,
                rack: Default::default(),
                rack_roll_view: false,
                plugin_channel: 0,
                plugin_moved: false,
                plugin_pos: (560.0, 260.0),
                selected_channel: 0,
                selected_mixer: 1,
                mixer: Default::default(),
                pattern: 1,
                song_mode: false,
                menu_open: None,
                menu_sub: None,
                lane_menu: false,
                settings: views::settings::SettingsUi { native: os_native::settings_shown(), ..Default::default() },
                fl_text: saved_fl_text(),
                theme_base: saved_fl_text(),
                playlist_scroll: (0.0, 0.0),
                playlist_bar_w: views::playlist::BAR_W,
                mixer_scroll: 0.0,
                rack_scroll: 0.0,
                browser_tab: 0,
                browser_open: views::browser::default_open(),
                browser_folders: crate::browser::load_folders(),
                browser_names: crate::browser::load_names(),
                params: Default::default(),
                value_clip: None,
                browser_dirs: Default::default(),
                cables: std::cell::RefCell::new({
                    let mut c = crate::cable::Cables::with(load_cable_cfg());
                    c.animate(load_animations());
                    c
                }),
                browser_scroll: 0.0,
                browser_sel: None,
                browser_drag: None,
                browser_search: String::new(),
                browser_search_focus: false,
                browser_opts: crate::browser::load_opts(),
                browser_hist: Vec::new(),
                browser_hist_pos: 0,
                browser_w: crate::browser::load_width(),
                browser_split_click: None,
                browser_marquee: Default::default(),
                browser_favs: crate::browser::load_favs(),
                browser_index: Default::default(),
                tempo: 130.0,
                tempo_digits: std::cell::Cell::new([f32::INFINITY; 3]),
                message: None,
                monitor: crate::monitor::Monitor::with_cfg(load_monitor_cfg()),
                cpu: crate::cpu::Cpu::saved(),
                perf: crate::perf::Perf::default(),
                tapper: Default::default(),
                container: Default::default(),
                touch: Default::default(),
                events: Default::default(),
                events_back: None,
                strum: Default::default(),
                arp: Default::default(),
                piano_dlg: Default::default(),
                rec: Default::default(),
                toggles: vec!["typing"],
                clock: crate::clock::Clock::load(),
                toolbar: views::toolbar::Opts::load(),
                bar: crate::toolbar::Bar::load(),
                midi: Default::default(),
                link_box: None,
                multilink: Default::default(),
                maximized: true,
                piano: PianoState::default(),
                playlist: crate::playlist::PlaylistState::default(),
                history: Default::default(),
                control: Default::default(),
                global_snap: Default::default(),
                pat_ui: Default::default(),
                wins: crate::arrange::default_wins(),
                screen: (1920.0, 1080.0),
                display_scale: 1.0,
                ui_scale: 1.0,
                dialogs: Vec::new(),
                credits: Default::default(),
                clip_colors: Default::default(),
                plugin_edited: false,
                plugin_edit_revision: 0,
                recovery_id: crate::backup::new_identity(),
                detached: Vec::new(),
                detached_frames: Vec::new(),
                surface: None,
                overlay_on: None,
                drag_on: None,
            },
            theme: load_theme(),
            transport,
            hits: Vec::new(),
            keep: Keep::default(),
            live: Vec::new(),
            shown: Shown::default(),
            overlays: Vec::new(),
            screens: Vec::new(),
            native: Vec::new(),
            canvas_off: (0, 0),
            popup_lift: None,
            plugin_menu: None,
            bars: Vec::new(),
            hover: None,
            pressed: None,
            armed: None,
            raise_at: None,
            armed_right: None,
            hover_rect: None,
            damage: Damage::Full,
            typing_held: Vec::new(),
            perf_typed: Vec::new(),
            typing_octave: 0,
            typing_synced: false,
            mouse_note: None,
            cables_at: None,
            key_velocity: TYPING_VELOCITY,
            midi: crate::midi_in::MidiIn::none(),
            midi_play: Default::default(),
            midi_out: crate::midi_out::MidiOut::none(),
            remote: Default::default(),
            step_chord: crate::step_edit::Chord::default(),
            title_click: None,
            rack_click: None,
            cpu_click: None,
            hint: Cow::Borrowed(""),
            mouse: (-1.0, -1.0),
            drag: None,
            hold: None,
            smooth: Default::default(),
            warp: None,
            audio_rec: None,
            track_rec: None,
            audio_log: Default::default(),
            menu_notch: 0.0,
            shift_clone: false,
            auto_close: Default::default(),
            want_minimize: false,
            want_maximize: false,
            want_close: false,
            want_exit: false,
            quit_answer: None,
            #[cfg(all(plugin_windows, target_os = "linux"))]
            unpinned: None,
            want_drag_window: false,
            resize_cursor: false,
            ctrl: false,
            shift: false,
            alt: false,
            rshift: false,
            repeat: false,
            pl_grid: None,
            scrolled: None,
            shifts: Vec::new(),
            moved: Vec::new(),
            px_scale: 1.0,
            pl_covers: Vec::new(),
            piano_win: None,
            piano_grid: None,
            piano_panel: None,
            sampler_failed: Vec::new(),
            rpc_server: None,
            fx_failed: Vec::new(),
            sweep: None,
            scan_queued: None,
            wrap_failed: Vec::new(),
            osc3_look: None,
            native_failed: Vec::new(),
            loading: None,
            missing: Default::default(),
            zipping: None,
            host_deadline: None,
            host_more: false,
            export: None,
            export_reveal: false,
            pattern_render: None,
            device_opens: 0,
            saved: (Project::default(), 0.0, Vec::new()),
            backup: Default::default(),
        };
        // The snap settings are program settings, kept in the user folder (spec: ui/global-snap.md §6).
        let snap = crate::snap::load();
        app.state.global_snap = snap.global;
        app.state.piano.snap = snap.piano;
        app.state.playlist.snap = snap.playlist;
        crate::project_info::created(&mut app.state);
        app.mark_saved();
        if let Some(p) = path {
            app.open(p);
        }
        app
    }

    pub fn title(&self) -> String {
        let s = &self.state;
        let name = s.project.info.title.clone().filter(|t| !t.is_empty()).or_else(|| {
            s.path.as_ref().and_then(|p| p.file_stem()).map(|n| n.to_string_lossy().into_owned())
        });
        match name {
            Some(n) => format!("[{n}]"),
            None => "[untitled]".into(),
        }
    }

    pub fn open(&mut self, path: PathBuf) {
        // A MIDI file opened as a project (the command line, say) is imported as a dropped one is (spec:
        // ui/midi-files.md §10.6).
        let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if ext == "mid" || ext == "midi" {
            return self.midi_file_chosen(path, false);
        }
        // A zipped project is unzipped beside itself, then its project file opens (`crate::zipped`).
        if ext == "zip" {
            return crate::zipped::import(self, path);
        }
        // A project in the layout used before version 24.2 is converted to the current one as it opens, and is saved in
        // that layout (spec: flp/layouts.md "Converting to the newer layout"). One the conversion cannot handle gets a
        // dialog that says so, not just a line in the hint bar.
        let mut legacy = None;
        let result = std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|b| os_flp::FlpFile::parse(&b).map_err(|e| e.to_string()))
            .and_then(|f| os_flp::lemon::validate_version(&f).map(|()| f).map_err(|e| e.to_string()))
            .and_then(|mut f| match os_flp::project::upgrade(&mut f).and_then(|_| os_flp::project::load(&f)) {
                Ok(p) => Ok((f, p)),
                Err(os_flp::project::ProjectError::LegacyLayout(v)) => {
                    let e = os_flp::project::ProjectError::LegacyLayout(v.clone()).to_string();
                    legacy = Some(v);
                    Err(e)
                }
                Err(e) => Err(e.to_string()),
            });
        if let Some(v) = legacy {
            let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            let saved = if v.trim().is_empty() { "an older version".to_string() } else { format!("version {}", v.trim()) };
            let rest = format!(
                ".\n\nIt was saved by {saved}, in an older file layout that Lemon Studio could not convert.\n\nOpen the project in \
                 version 24.2 or later, save it again, then open the new copy here."
            );
            self.tell(crate::dialog::Kind::Info, "Project not supported", crate::dialog::spans(&["Lemon Studio cannot open ", &name, &rest]));
        }
        match result {
            Ok((file, project)) => {
                self.transport.stop();
                self.reset_hosted();
                let s = &mut self.state;
                s.tempo = project.tempo.unwrap_or(130.0);
                s.pattern = project.current_pattern.unwrap_or(1).max(1);
                s.selected_channel = 0;
                s.selected_mixer = 1;
                s.mixer = Default::default();
                s.playlist_scroll = (0.0, 0.0);
                s.rack_scroll = 0.0;
                s.rack_scroll_x = 0.0;
                s.rack.reset();
                s.hide(Win::Plugin);
                s.mixer_scroll = 0.0;
                // As saved (event 9); a file without it opens in song mode, as in FL 26 (spec: flp/engine-events.md
                // "Project").
                s.song_mode = project.song_mode.unwrap_or(true);
                s.project = project;
                s.recovery_id = crate::backup::new_identity();
                s.plugin_edit_revision = 0;
                self.backup = Default::default();
                // Opening in song mode sets the initial values, as FL 26 does (spec: flp/engine-events.md "Event
                // 216"); the project is opened with them, not changed.
                crate::init::apply(s);
                crate::project_info::opened(s);
                crate::rack::from_model(s);
                s.file = Some(file);
                s.message = Some(format!("Opened {}", path.display()));
                crate::templates::remember(&path);
                crate::recent::add(&path);
                s.path = Some(path);
                s.piano.reset();
                s.playlist.reset();
                s.history.reset();
                s.control.reset();
                s.control.event(&crate::control::By::User, "opened", s.path.as_ref().map_or(String::new(), |p| p.display().to_string()), Vec::new());
                crate::playlist::load_view(s);
                s.piano.dirty = true; // live notes use the new project
                piano::center_keys(s, self.piano_grid.map_or(700.0, |g| g.h));
                // A .lemon file's own state (icons, slot names and colours, Performance settings); its channel icons
                // name channels by position, which the ids given here then follow.
                self.state.control.settle(&mut self.state.project);
                if let Some(f) = self.state.file.take() {
                    crate::lemon_state::apply(&mut self.state, &mut self.plugins, &f);
                    self.state.file = Some(f);
                }
                self.mark_saved();
                crate::missing::opened(self);
                self.loading = Some(crate::loading::Loading::start(&self.state.project));
                self.state.playlist.wave_wait = true;
                self.invalidate();
            }
            Err(e) => self.state.message = Some(format!("Could not open {}: {e}", path.display())),
        }
    }

    /// FILE > Save (`choose` = false: to the project's file, when it has one) and Save as.
    fn save(&mut self, choose: bool) {
        if choose {
            self.save_dialog(None);
        } else {
            self.save_then(None);
        }
    }

    /// Saves the project to its file, or asks where (a project without one, or one opened from an FLP, which is never
    /// saved over: spec flp/lemon-format.md "Opening and saving"); then `next`, once it is saved.
    fn save_then(&mut self, next: Option<crate::dialog::Next>) {
        let Some(path) = self.state.path.clone().filter(|p| !crate::project_files::is_flp(p)) else { return self.save_dialog(next) };
        self.save_to(path);
        if let Some(n) = next {
            self.go_on_if_saved(n);
        }
    }

    /// Save as: for a project never saved, Lemon Studio's Save as form while Ask for data folder says so (spec:
    /// ui/data-folder.md §7, `crate::save_form`), else the file dialog.
    fn save_dialog(&mut self, next: Option<crate::dialog::Next>) {
        if self.state.path.is_none() && crate::save_form::asks_on_save() {
            return self.open_save_form(next);
        }
        self.save_file_dialog(next);
    }

    /// The Save project as dialog, starting in the project's folder with its name (an FLP's with `.lemon`).
    pub(crate) fn save_file_dialog(&mut self, next: Option<crate::dialog::Next>) {
        use crate::filedlg::{FileDlg, Mode};
        use crate::project_files::{as_lemon, EXT};
        let path = self.state.path.clone();
        let name = path.as_deref().map(as_lemon).and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_else(|| format!("Untitled.{EXT}"));
        let dlg = FileDlg::new(Mode::Save, "Save project as", "project", crate::dialog::Then::SaveProject(next))
            .filter("Lemon Studio project", &[EXT])
            .start(path.as_deref().and_then(|p| p.parent()))
            .name(&name);
        self.file_dialog(dlg);
    }

    pub(crate) fn go_on_if_saved(&mut self, n: crate::dialog::Next) {
        if self.unsaved() { self.held_back(n) } else { self.go_on(n) }
    }

    /// The Export form's job starts (spec: audio/export.md §4): what plays (`song`, else the current pattern) renders on
    /// the audio thread at the output device's rate; playback stops and a progress panel shows until the files are
    /// written.
    // spec: audio/export.md
    pub(crate) fn start_export(&mut self, job: crate::export::Job, song: bool) {
        if self.export.is_some() {
            return;
        }
        let p = crate::rack::play_project(&self.state);
        let rate = Transport::device_rate().unwrap_or(44100);
        let first = job.options.ticked().next().unwrap_or(0);
        let name = job.file(first).file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let mode = if song { PlayMode::Song } else { PlayMode::Pattern(self.state.pattern) };
        self.export_reveal = job.options.reveal;
        self.plugins.set_offline(true);
        // Lemon Sampler voices playing from disk wait for their frames in every export, high quality or not.
        lemon_sampler::stream::export_started();
        self.transport.export(&p, mode, rate, job);
        self.export = Some((name, 0.0));
        self.invalidate();
    }

    /// Follows a running export: moves the progress bar, then reports the result (a pattern render's goes to
    /// `pattern_render_done`).
    pub(crate) fn export_tick(&mut self) {
        let Some((name, _)) = self.export.clone() else { return };
        match self.transport.export_status() {
            crate::transport::Export::Running(p) => {
                self.export = Some((name, p));
                self.damage_rect(crate::loading::panel_rect(self.state.screen).outset(4.0));
            }
            crate::transport::Export::Done(r) => {
                self.export = None;
                self.midi_after_export();
                self.plugins.set_offline(false);
                lemon_sampler::stream::export_finished();
                if let Some(p) = self.pattern_render.take() {
                    return self.pattern_render_done(p, r);
                }
                self.state.message = Some(match r {
                    Ok((secs, files)) => {
                        if self.export_reveal {
                            if let Some(f) = files.first() {
                                crate::theme_library::show_in_folder(f);
                            }
                        }
                        let more = if files.len() > 1 { format!(" + {} more", files.len() - 1) } else { String::new() };
                        format!("Exported {}:{:02} to {name}{more}", (secs / 60.0) as u32, (secs % 60.0) as u32)
                    }
                    Err(e) => format!("Export failed: {e}"),
                });
                self.invalidate();
            }
            crate::transport::Export::Idle => {}
        }
    }

    /// Saves the project to `target`; a file over the Project data size warning is said (`App::size_notice`).
    pub(crate) fn save_to(&mut self, target: PathBuf) {
        let _ = self.save_to_result(target);
    }

    /// The typed save result is also used by the control API; UI wording never determines success.
    pub(crate) fn save_to_result(&mut self, target: PathBuf) -> Result<u64, String> {
        let result = self.write_project(target, &mut |_| {}, false, false);
        if let Ok(bytes) = result {
            self.size_notice(bytes);
        }
        result
    }

    /// Gather a candidate, validate its serialization, and durably publish it. Failed saves preserve the loaded
    /// model, file, identity and dirty state. Copies retain their source identity; Save As starts a new lineage.
    fn write_project(&mut self, target: PathBuf, edit: &mut dyn FnMut(&mut os_model::Project), copy: bool, exclusive: bool) -> Result<u64, String> {
        let before = (self.state.project.clone(), self.state.recovery_id);
        if !copy && self.state.path.as_ref().is_some_and(|p| p != &target) {
            self.state.recovery_id = crate::backup::new_identity();
        }
        let result = self.publish_project(&target, edit, exclusive);
        if result.is_err() {
            (self.state.project, self.state.recovery_id) = before;
        }
        if let Err(e) = &result {
            self.state.message = Some(format!("Save failed: {e}"));
        }
        result
    }

    fn publish_project(&mut self, target: &std::path::Path, edit: &mut dyn FnMut(&mut os_model::Project), exclusive: bool) -> Result<u64, String> {
        // Copy live sampler settings into the candidate. A failed snapshot must never look like an empty state.
        for row in self.state.hosted_rows.clone() {
            if self.state.project.channels.get(row).is_some_and(|c| c.kind.is_sampler()) {
                let lemon = self.plugins.descriptor(row).is_some_and(|d| d.name == crate::plugins::LEMON_SAMPLER);
                if lemon {
                    let st = self.plugins.state(row).ok_or_else(|| format!("Lemon Sampler on channel {} could not save its state", row + 1))?;
                    crate::plugins::store_sampler(&mut self.state.project.channels[row], &st);
                }
                if let Some(p) = self.plugins.sampler_path(row) {
                    let ch = &mut self.state.project.channels[row];
                    if ch.sample_path.as_deref().is_some_and(|old| crate::plugins::resolve_sample(old) == p) {
                        continue;
                    }
                    if !p.is_empty() {
                        ch.sample_path = Some(p);
                    } else if ch.sample_path.is_some() {
                        ch.clear_sample();
                    }
                }
            }
        }
        self.try_store_builtin_states()?;
        let s = &mut self.state;
        s.project.tempo = Some(s.tempo);
        s.project.song_mode = Some(s.song_mode);
        crate::rack::to_model(s);
        crate::playlist::store_view(s);
        let lacks_playlist = s.file.as_ref().and_then(|f| os_flp::project::load(f).ok()).is_some_and(|p| p.arrangements.is_empty())
            && !s.project.arrangements.is_empty();
        let fresh = s.file.is_none() || lacks_playlist;
        if fresh {
            settle_origins(&mut s.project);
            if s.project.arrangements.is_empty() {
                s.project.arrangements.push(os_flp::project::default_arrangement());
            }
        }
        let mut proj = s.project.clone();
        crate::project_info::stamp(s, &mut proj);
        crate::data_folder::stamp(s, &mut proj);
        crate::project_plugins::store_hosted(&mut proj, &self.plugins)?;
        let extra = crate::lemon_state::events(s, &self.plugins);
        edit(&mut proj);
        let mut file = if fresh {
            os_flp::project::create(&proj).map_err(|e| e.to_string())?
        } else {
            s.file.clone().expect("checked")
        };
        os_flp::project::apply(&proj, &mut file).map_err(|e| e.to_string())?;
        lemon_or_flp(&mut file, target, extra)?;
        let bytes = file.to_bytes().map_err(|e| e.to_string())?;
        let checked = os_flp::FlpFile::parse(&bytes).map_err(|e| format!("saved project validation: {e}"))?;
        os_flp::lemon::validate_version(&checked).map_err(|e| format!("saved project validation: {e}"))?;
        let persisted = os_flp::project::load(&checked).map_err(|e| format!("saved project validation: {e}"))?;
        if persisted.main_volume.unwrap_or(os_model::Project::MAIN_VOLUME_DEFAULT)
            != proj.main_volume.unwrap_or(os_model::Project::MAIN_VOLUME_DEFAULT)
        {
            return Err("Save refused: main volume changed during serialization".into());
        }
        // A readable project may still omit supported state if imported layout sections are absent.
        // Reject the write before publication and leave the unsaved live session intact.
        let residual = os_flp::project::unsaved_parts(&proj, &checked);
        if !residual.is_empty() {
            return Err(format!("Save refused: layout cannot store {}", residual.join(", ")));
        }
        if exclusive {
            crate::durable_file::create(target, &bytes)
        } else {
            crate::backup::keep_overwritten(target, crate::data_folder::backup_folder(s, Some(target)), s.recovery_id)?;
            crate::durable_file::replace(target, &bytes)
        }.map_err(|e| format!("{}: {e}", target.display()))?;
        // Only a completed publication can adopt the new saved state.
        settle_origins(&mut s.project);
        crate::project_plugins::adopt_saved(&mut s.project, &proj);
        s.file = Some(file);
        crate::templates::remember(target);
        crate::recent::add(target);
        s.path = Some(target.to_path_buf());
        s.message = Some(format!("Saved {}", target.display()));
        self.mark_saved();
        Ok(bytes.len() as u64)
    }

    /// Writes a copy without changing the live project, its name, dirty state, or recent-project preferences.
    pub(crate) fn save_copy(&mut self, target: PathBuf) -> Option<Result<(), String>> {
        self.save_copy_with(target, &mut |_| {})
    }

    pub(crate) fn save_copy_with(&mut self, target: PathBuf, edit: &mut dyn FnMut(&mut os_model::Project)) -> Option<Result<(), String>> {
        self.save_copy_inner(target, edit, false)
    }

    /// Recovery generations use exclusive publication so a collision can never replace another backup.
    pub(crate) fn save_backup_copy(&mut self, target: PathBuf) -> Option<Result<(), String>> {
        self.save_copy_inner(target, &mut |_| {}, true)
    }

    fn save_copy_inner(&mut self, target: PathBuf, edit: &mut dyn FnMut(&mut os_model::Project), exclusive: bool) -> Option<Result<(), String>> {
        if self.loading.is_some() || self.export.is_some() {
            return None;
        }
        let s = &mut self.state;
        let keep = (s.project.clone(), s.path.clone(), s.file.clone(), s.message.take(), self.saved.clone(), s.plugin_edited, s.recovery_id);
        let (last, recent) = (crate::templates::last_kept(), crate::recent::list());
        let done = self.write_project(target, edit, true, exclusive).map(|_| ());
        crate::templates::restore_last(last);
        crate::recent::restore(&recent);
        let s = &mut self.state;
        (s.project, s.path, s.file, s.message, self.saved, s.plugin_edited, s.recovery_id) = keep;
        Some(done)
    }

    /// The project as it is now counts as saved (opened, saved, new).
    pub(crate) fn mark_saved(&mut self) {
        // New things get their ids before the copy, so ids alone are no unsaved change.
        self.state.control.settle(&mut self.state.project);
        self.saved = (self.state.project.clone(), self.state.tempo, crate::lemon_state::saved_form(&self.state, &self.plugins));
        self.state.plugin_edited = false;
    }

    /// Whether the project changed since it was last opened, saved or created, a hosted plugin's settings included.
    pub fn unsaved(&self) -> bool {
        self.saved.0 != self.state.project
            || self.saved.1 != self.state.tempo
            || self.saved.2 != crate::lemon_state::saved_form(&self.state, &self.plugins)
            || self.state.plugin_edited
    }

    /// The system asks the window to close (its close shortcut, a taskbar or dock). The dialog-like windows drawn
    /// inside the main window close first, one per request, as windows of their own would under that shortcut: the
    /// settings window, the Remote control settings box, then the tool window in front (Tempo tapper, Touch controller,
    /// Plugin performance monitor, the piano roll's tool dialogs; as with their ×). Otherwise the app closes, once
    /// unsaved changes are answered. While a dialog is open the window stays (the dialog is answered first).
    pub fn close_requested(&mut self) {
        if self.modal_open() {
            return;
        }
        if self.state.settings.open {
            self.state.settings.open = false;
            self.invalidate();
            return;
        }
        if self.state.link_box.take().is_some() {
            self.invalidate();
            return;
        }
        let tool = |w: &Win| crate::floating::is_tool(*w);
        if let Some(&w) = self.state.z.last().filter(|w| !self.state.no_focus && tool(w)) {
            self.act(Action::CloseWin(w));
            self.invalidate();
            return;
        }
        self.ask_discard("closing", crate::dialog::Next::Close);
    }

    /// Before the project is replaced or the app closes (`doing`: "closing", ...): with unsaved changes, asks to save
    /// them (spec: ui/dialogs.md §1f), and `next` follows once they are saved or not wanted; otherwise right away.
    pub fn ask_discard(&mut self, doing: &str, next: crate::dialog::Next) {
        use crate::dialog::{spans, Answer, Kind, Msg, Then};
        #[cfg(test)]
        let quiet = !crate::dialog::fake::is_live();
        #[cfg(not(test))]
        let quiet = false;
        if !self.unsaved() || quiet {
            return self.go_on(next);
        }
        let name = self.state.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned());
        let text = match &name {
            Some(n) => spans(&["Save the changes to ", n, &format!(" before {doing}?")]),
            None => spans(&[&format!("Save the changes to the new project before {doing}?")]),
        };
        self.show_msg(Msg::new(Kind::Question, "Confirm", text, &[Answer::Yes, Answer::No, Answer::Cancel], Then::Discard(next)));
    }

    /// The work that waited for unsaved changes goes on.
    fn go_on(&mut self, n: crate::dialog::Next) {
        use crate::dialog::Next;
        match n {
            Next::Close => self.want_exit = true,
            Next::Quit => self.quit_answer = Some(true),
            Next::NewProject => {
                self.new_project();
                self.offer_new_form();
            }
            Next::OpenDialog => self.open_dialog(),
            Next::Open(p) => self.open(p),
            Next::ImportMidi(p) => self.import_pending(p),
            Next::Revert(b) => self.revert_from(b),
            Next::Template(f) => self.new_from_pick(f),
        }
        self.invalidate();
    }

    /// The work that waited for unsaved changes is called off (cancelled, or the save did not happen).
    fn held_back(&mut self, n: crate::dialog::Next) {
        if n == crate::dialog::Next::Quit {
            self.quit_answer = Some(false);
        }
    }

    /// A dialog was answered: what waited for it (spec: ui/dialogs.md §5).
    pub(crate) fn dialog_done(&mut self, then: crate::dialog::Then, reply: crate::dialog::Reply) {
        use crate::dialog::{Answer, Reply, Then};
        let file = match &reply {
            Reply::File(p) => p.clone(),
            Reply::Button(_) => None,
        };
        match then {
            Then::Nothing | Then::Replace(_) => {}
            Then::Discard(next) => match reply {
                Reply::Button(Answer::Yes) => self.save_then(Some(next)),
                Reply::Button(Answer::No) => self.go_on(next),
                _ => self.held_back(next),
            },
            Then::SaveProject(next) => match (file, next) {
                (Some(p), next) => {
                    self.save_to(p);
                    if let Some(n) = next {
                        self.go_on_if_saved(n);
                    }
                }
                (None, Some(n)) => self.held_back(n),
                (None, None) => {}
            },
            Then::ExportFolder => {
                if let Some(p) = file {
                    self.export_folder_chosen(p);
                }
            }
            Then::OpenProject => {
                if let Some(p) = file {
                    self.open(p);
                }
            }
            Then::LoadTheme => {
                if let Some(p) = file {
                    self.load_theme_file(&p);
                }
            }
            Then::Wallpaper => {
                if let Some(p) = file {
                    self.set_wallpaper(&p);
                }
            }
            Then::ArpPattern => {
                if let Some(p) = file {
                    self.arp_command(crate::arpeggiate::ACmd::Pattern(Some(p)));
                }
            }
            Then::QuantTemplate => {
                if let Some(p) = file {
                    self.quant_command(crate::quantize::QCmd::Template(Some(p)));
                }
            }
            Then::ChopPattern => {
                if let Some(p) = file {
                    self.chop_command(Some(p));
                }
            }
            Then::PluginFolder => {
                if let Some(p) = file {
                    self.plugin_folder_chosen(p);
                }
            }
            Then::BrowserFolder(row) => {
                if let Some(p) = file {
                    self.search_folder_chosen(row, p);
                }
            }
            Then::MissingFiles => {
                if reply == Reply::Button(Answer::Yes) {
                    crate::missing::pick_folder(self);
                }
            }
            Then::SampleFolder => {
                if let Some(p) = file {
                    crate::missing::search(self, p);
                }
            }
            Then::ExportZip => {
                if let Some(p) = file {
                    crate::zipped::export(self, p);
                }
            }
            Then::FactoryFolder => {
                if let Some(p) = file {
                    self.set_factory_folder(Some(p));
                }
            }
            Then::SaveTemplate => {
                if let Some(p) = file {
                    self.template_chosen(p);
                }
            }
            Then::NewProjectFolder => {
                if let Some(p) = file {
                    self.new_folder_chosen(p);
                }
            }
            Then::DeleteUnused(files) => self.delete_unused_done(files, &reply),
            Then::ExtInput(t) => {
                if reply == Reply::Button(Answer::Yes) {
                    crate::mixer::external_only(&mut self.state, t);
                }
            }
            Then::SavePreset(key) => {
                if let Some(p) = file {
                    self.state.message = crate::presets::save_chosen(self, key, p);
                }
            }
            Then::SaveChannelState(row) => {
                if let Some(p) = file {
                    self.state.message = crate::channel_states::save_chosen(self, row, p);
                }
            }
            Then::ImportPreset(key) => {
                if let Some(p) = file {
                    self.state.message = crate::presets::import_chosen(self, key, p);
                }
            }
            Then::RenamePreset(key, old) => {
                if let Some(p) = file {
                    self.state.message = crate::presets::rename_chosen(self, key, old, p);
                }
            }
            Then::DeletePreset(key, path) => {
                if reply == Reply::Button(Answer::Yes) {
                    self.state.message = crate::presets::delete_chosen(self, key, path);
                }
            }
            Then::SaveToolbarPreset => {
                if let Some(p) = file {
                    self.toolbar_saved(p);
                }
            }
            Then::DeleteToolbarPreset(path) => {
                if reply == Reply::Button(Answer::Yes) {
                    self.toolbar_deleted(path);
                }
            }
            Then::RackSample(row) => {
                if let Some(p) = file {
                    crate::rack::sample_chosen(self, row, p);
                }
            }
            // Cancel and the caption's × change nothing.
            Then::StretchClips(rows, method) => {
                if reply == Reply::Button(Answer::Ok) {
                    crate::rack::set_stretch(self, &rows, method);
                }
            }
            // spec: ui/pattern-time-signature.md §2-3: No leaves the pattern alone; closing the dialog without Accept
            // leaves it without a start marker.
            Then::ReplaceSignature(n) => {
                if reply == Reply::Button(Answer::Yes) {
                    self.replace_signature(n);
                }
            }
            Then::Signature(n, num, den) => {
                if reply == Reply::Button(Answer::Ok) {
                    crate::timesig::set_start(&mut self.state, n, num, den);
                }
            }
            Then::SongSignature(at, num, den) => {
                if reply == Reply::Button(Answer::Ok) {
                    crate::timesig::set_song(&mut self.state, at, num, den);
                }
            }
            Then::PerfConfirm => {
                if reply == Reply::Button(Answer::Ok) {
                    self.open_marker_settings();
                }
            }
            Then::MidiExportConfirm => {
                if reply == Reply::Button(Answer::Ok) {
                    self.state.message = Some(self.prepare_midi_export());
                }
            }
            Then::PerfMarkers(count, spacing) => {
                if reply == Reply::Button(Answer::Ok) {
                    let m = crate::perform::prepare_with(&mut self.state, crate::perform::MarkerForm { count, spacing });
                    self.state.message = Some(m);
                }
            }
            Then::DataFolder(d) => self.data_folder_done(d, reply),
            Then::ClientSave(p) => {
                if reply == Reply::Button(Answer::Yes) {
                    self.save_to(p);
                }
            }
            Then::Proposal(n) => {
                let s = &mut self.state;
                let r = if reply == Reply::Button(Answer::Yes) { crate::control::accept(s, n).map(|_| ()) } else { crate::control::discard(s, n) };
                if let Err(e) = r {
                    s.message = Some(e);
                }
            }
            Then::MidiImport(piano) => {
                if let Some(p) = file {
                    self.midi_file_chosen(p, piano);
                }
            }
            Then::MidiExport(piano) => {
                if let Some(p) = file {
                    self.midi_export_chosen(p, piano);
                }
            }
        }
        self.invalidate();
    }

    /// A dialog opened or closed: Linux's detached plugin windows step back while one is open.
    pub(crate) fn modal_changed(&mut self) {
        #[cfg(all(plugin_windows, target_os = "linux"))]
        {
            let open = self.modal_open();
            if open != self.unpinned.is_some() {
                self.unpinned = open.then(crate::plugin_window::unpinned);
            }
        }
        self.invalidate();
    }

    /// FILE > New project: the Default template (spec: ui/settings-choices.md "Miscellaneous: seven more checks,
    /// Default template and Startup project"); for Empty, or a template that does not load, the built-in project.
    fn new_project(&mut self) {
        if self.new_from_template() {
            return;
        }
        self.empty_project();
    }

    /// The built-in new project (FILE ▸ New with Empty, New from template ▸ Empty).
    pub(crate) fn empty_project(&mut self) {
        self.transport.stop();
        self.reset_hosted();
        let s = &mut self.state;
        s.project = default_project();
        s.recovery_id = crate::backup::new_identity();
        s.plugin_edit_revision = 0;
        self.backup = Default::default();
        crate::project_info::created(s);
        s.file = None;
        s.path = None;
        s.tempo = 130.0;
        s.pattern = 1;
        // FL's default template is saved in pattern mode.
        s.song_mode = false;
        s.piano.reset();
        s.playlist.reset();
        s.history.reset();
        s.control.reset();
        s.control.event(&crate::control::By::User, "opened", "New project", Vec::new());
        crate::playlist::load_view(s);
        s.rack.reset();
        crate::rack::from_model(s);
        s.piano.dirty = true;
        s.message = Some("New project".into());
        self.mark_saved();
    }

    /// FILE > Open: the open dialog, starting in the project's folder.
    fn open_dialog(&mut self) {
        let start = self.state.path.as_ref().and_then(|p| p.parent().map(std::path::Path::to_path_buf));
        let dlg = crate::filedlg::FileDlg::new(crate::filedlg::Mode::Open, "Open project", "project", crate::dialog::Then::OpenProject)
            .filter("Lemon Studio project", &["lemon", "flp", "zip"])
            .start(start.as_deref());
        self.file_dialog(dlg);
    }

    /// FILE > Save new version (Ctrl+N): saves next to the project as "name_2.lemon", "name_3.lemon", ... (the first
    /// free number; an FLP's versions too), which becomes the project's file. A project without a file asks for a name.
    fn save_new_version(&mut self) {
        let Some(path) = self.state.path.clone() else { return self.save(true) };
        let stem = path.file_stem().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let (base, n) = match stem.rsplit_once('_') {
            Some((b, d)) if !b.is_empty() && !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()) => (b.to_string(), d.parse::<u32>().unwrap_or(1)),
            _ => (stem.clone(), 1),
        };
        let dir = path.parent().map(PathBuf::from).unwrap_or_default();
        let ext = crate::project_files::EXT;
        let target = (n + 1..).map(|k| dir.join(format!("{base}_{k}.{ext}"))).find(|p| !p.exists()).expect("a free name");
        self.save_to(target);
    }

    /// The built-in recreations write their settings into the model as the file's own plugin state: Lemon Parametric
    /// EQ, Tesla (as Edison), Pulp Filter (as Love Philter) and Press (as Maximus) into their slots, Lemon Trio into its
    /// channels.
    pub(crate) fn store_builtin_states(&mut self) {
        if let Err(e) = self.try_store_builtin_states() {
            self.state.message = Some(e);
        }
    }

    pub(crate) fn try_store_builtin_states(&mut self) -> Result<(), String> {
        for key in self.plugins.hosted.keys().copied().filter(|&k| k >= crate::plugins::FX_KEY).collect::<Vec<_>>() {
            let Some((t, k)) = crate::plugins::fx_of(key) else { continue };
            let Some(slot) = self.state.project.mixer.iter_mut().find(|m| m.index == t).and_then(|m| m.slots[usize::from(k)].as_mut()) else { continue };
            let st = self.plugins.capture_recreation_state(key, &slot.name)?;
            if let Some(st) = st {
                slot.state = st;
            }
        }
        for row in self.plugins.hosted.range(..crate::plugins::FX_KEY).map(|(&k, _)| k).collect::<Vec<_>>() {
            let Some(desc) = self.plugins.descriptor(row) else { continue };
            let file = match desc.name.as_str() {
                crate::osc3::LEMON_TRIO => crate::osc3::FILE_NAME,
                crate::citrus::CITRUS => crate::citrus::FILE_NAME,
                crate::slicer::LEMON_SLICER => crate::slicer::FILE_NAME,
                crate::drums::LEMON_DRUMS => crate::drums::FILE_NAME,
                _ => continue,
            };
            let Some(st) = self.plugins.capture_recreation_state(row, file)? else { continue };
            if let Some(ch) = self.state.project.channels.get_mut(row).filter(|c| c.kind == os_model::ChannelKind::Plugin) {
                let keeps_spelling = ch.plugin.as_deref().is_some_and(|p| p == file
                    || (file == crate::slicer::FILE_NAME && crate::slicer::is_file_name(p))
                    || (file == crate::drums::FILE_NAME && crate::drums::is_file_name(p)));
                if !keeps_spelling {
                    ch.plugin = Some(file.into());
                }
                ch.plugin_state = (!(file == crate::citrus::FILE_NAME && st.is_empty())).then_some(st);
                if file == crate::osc3::FILE_NAME {
                    if let Some(path) = self.plugins.osc3_sample(row, true).filter(|p| !p.is_empty()) {
                        if !ch.sample_path.as_deref().is_some_and(|old| crate::plugins::resolve_sample(old) == path) {
                            crate::rack::auto_name(ch, std::path::Path::new(&path));
                            ch.sample_path = Some(path);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Assign stable owners and restore persisted parameter order before a new instance can receive automation.
    pub(crate) fn prepare_plugin_routing(&mut self) {
        self.state.control.settle(&mut self.state.project);
        self.follow_ids(false, false);
        self.restore_plugin_identities();
    }

    /// Drops every hosted instance (a new or opened project); `sync_samplers` hosts the new project's samplers.
    pub(crate) fn reset_hosted(&mut self) {
        self.plugins.clear();
        self.loading = None;
        self.state.playlist.wave_wait = false;
        self.state.hosted_rows.clear();
        self.sampler_failed.clear();
        self.fx_failed.clear();
        self.wrap_failed.clear();
        self.native_failed.clear();
        self.transport.set_instruments(Vec::new());
        self.transport.set_effects(Vec::new());
    }

    /// Every sampler channel is played and edited by Lemon Sampler (owner, 2026-10-10; spec: plugins/lemon-sampler.md §7):
    /// a channel without an instance gets one, started from the channel's sample and envelopes. Without Lemon Sampler
    /// (plugin not found) sampler channels keep the engine's own sample playback.
    /// With a `deadline` it stops after the instance that passes it; returns whether every sampler row is done.
    fn sync_samplers(&mut self, deadline: Option<std::time::Instant>) -> bool {
        let Some(index) = self.plugins.index_of(crate::plugins::LEMON_SAMPLER) else { return true };
        let mut added = false;
        let mut done = true;
        for row in 0..self.state.project.channels.len() {
            let ch = &self.state.project.channels[row];
            if !ch.kind.is_sampler() || self.state.hosted_rows.contains(&row) || self.sampler_failed.contains(&row) {
                continue;
            }
            if deadline.is_some_and(|d| std::time::Instant::now() >= d) {
                done = false;
                break;
            }
            let st = crate::plugins::sampler_state(ch);
            if let Some(l) = &mut self.loading {
                l.hosted += 1;
                l.label = ch.name.clone().unwrap_or_default();
            }
            match self.plugins.create_with(row, index, Some(&st)) {
                Ok(_) => {
                    self.state.hosted_rows.push(row);
                    added = true;
                    // A channel just added (ADD ▸ Lemon Sampler): its window is about to open (spec:
                    // ui/settings-choices.md "Miscellaneous: naming, detached plugins and startup").
                    if self.state.pending_open == Some(row) {
                        self.plugins.added(row);
                    }
                }
                Err(e) => {
                    self.sampler_failed.push(row);
                    self.state.message = Some(format!("Lemon Sampler: {e}"));
                }
            }
        }
        if added {
            self.prepare_plugin_routing();
            self.transport.set_instruments(self.plugins.instruments());
            self.state.piano.dirty = true;
        }
        done
    }

    /// Project loading (spec: ui/project-loading.md): samples decode on worker threads first, then the sampler
    /// instances start a few per tick (`loading::HOST_BUDGET`) so the progress panel keeps moving. Without a loading
    /// project, new sampler rows are hosted at once.
    fn loading_tick(&mut self) {
        let Some(decoded) = self.loading.as_mut().map(|l| l.decode_done()) else {
            self.sync_samplers(None);
            return;
        };
        let deadline = std::time::Instant::now() + crate::loading::HOST_BUDGET;
        // Then the plugin channels and mixer effects, under the same budget, then the samplers' played samples.
        if decoded && self.sync_samplers(Some(deadline)) && self.host_plugins(deadline) && self.loading.as_mut().is_some_and(|l| l.builds_done()) {
            self.loading = None;
            self.state.playlist.wave_wait = false;
            // What was not found is listed now (spec: ui/project-loading.md §6).
            crate::missing::loaded(self);
            // Opening hydrates hosted instruments, stable plugin identities and relative sample paths
            // over several ticks. These are part of opening the saved session, not user edits. Only
            // adopt that hydrated state as clean if no user edit was recorded during the load; a
            // live edit/tempo change must never be silently marked as saved at completion.
            if self.state.history.len() == 0 && !self.state.plugin_edited && self.state.tempo == self.saved.1 {
                self.mark_saved();
            }
            return self.invalidate();
        }
        self.damage_rect(crate::loading::panel_rect(self.state.screen).outset(4.0));
    }

    /// Starts the opened project's plugin channels and mixer effects until `deadline` (`project_plugins::sync_wrapped`
    /// and the other hosts stop after the instance that passes it): true when none is left to start.
    fn host_plugins(&mut self, deadline: std::time::Instant) -> bool {
        self.host_deadline = Some(deadline);
        self.host_more = false;
        self.sync_effects();
        self.sync_wrapped();
        self.sync_osc3();
        self.sync_citrus();
        self.sync_slicer();
        self.sync_drums();
        self.sync_native();
        self.host_deadline = None;
        let samplers = self.state.hosted_rows.iter().filter(|&&r| self.state.project.channels.get(r).is_some_and(|c| c.kind.is_sampler())).count();
        let done = (self.plugins.hosted.len() + self.wrap_failed.len() + self.fx_failed.len() + self.native_failed.len()).saturating_sub(samplers);
        let more = self.host_more;
        if let Some(l) = &mut self.loading {
            l.label.clear();
            l.plugins_done = done;
            // Plugins no host plays (FL's own effects without a recreation) are not waited for.
            if !more {
                l.plugins = done;
            }
        }
        !more
    }

    /// Whether the plugin hosts' time this tick is over (`host_plugins`): the host that asks stops and goes on next tick.
    pub(crate) fn past_deadline(&mut self) -> bool {
        let past = self.host_deadline.is_some_and(|d| std::time::Instant::now() >= d);
        self.host_more |= past;
        past
    }

    /// A project is loading or an export is rendering: the window shows the progress and takes no input (spec:
    /// ui/project-loading.md §2).
    pub fn loading(&self) -> bool {
        self.loading.is_some() || self.export.is_some() || self.missing.search.is_some() || self.zipping.is_some()
    }

    /// The opened project's loading, for the startup splash: its progress (0..=1) and step, while it loads.
    pub fn project_loading(&self) -> Option<(f32, String)> {
        let l = self.loading.as_ref()?;
        Some((l.progress(), l.status().split_whitespace().collect::<Vec<_>>().join(" ")))
    }

    /// Mixer slots that hold the project file's parametric EQ are played and edited by Lemon Parametric EQ, started
    /// from the slot's saved state (spec: plugins/lemon-parametric-eq-2.md §8). Without the plugin the engine's own EQ
    /// plays them. Edison's slots are opened as Tesla the same way, with their sample, loop, markers and recording
    /// settings (spec: plugins/tesla/README.md "Opening Edison slots"), Love Philter's as Pulp Filter and Maximus's as
    /// Press with their state (spec: plugins/lemon-pulp-filter.md §6, plugins/lemon-press.md §6), and the FL effects
    /// another Lemon effect recreates (`plugins::FL_RECREATIONS`: Reeverb 2, Balance, Chorus, ...) by that effect.
    pub(crate) fn sync_effects(&mut self) {
        use crate::plugins::{fx_key, EDISON_FILE_NAME, EQ_FILE_NAME, LEMON_EQ, LOVE_PHILTER_FILE_NAME, MAXIMUS_FILE_NAME, PRESS, PULP_FILTER, TESLA};
        // The installed EQ plays these slots instead (`host_native`).
        if crate::project_plugins::native_first() {
            return;
        }
        // Runs every tick: the slots already hosted are passed over before their states are read, as an Edison slot's
        // can hold tens of megabytes of audio.
        let todo: Vec<(u16, u8, String)> = self
            .state
            .project
            .mixer
            .iter()
            .flat_map(|m| m.slots.iter().enumerate().map(move |(k, s)| (m.index, k as u8, s)))
            .filter(|&(t, k, _)| !self.plugins.hosted.contains_key(&fx_key(t, k)) && !self.fx_failed.contains(&fx_key(t, k)))
            .filter_map(|(t, k, s)| {
                s.as_ref()
                    .filter(|s| {
                        [EQ_FILE_NAME, EDISON_FILE_NAME, LOVE_PHILTER_FILE_NAME, MAXIMUS_FILE_NAME].contains(&s.name.as_str())
                            || crate::plugins::recreation_of(&s.name).is_some()
                    })
                    .map(|s| (t, k, s.name.clone()))
            })
            .collect();
        let mut made = false;
        for (t, k, file) in todo {
            if self.past_deadline() {
                break;
            }
            let recreation = crate::plugins::recreation_of(&file);
            let name = match file.as_str() {
                EQ_FILE_NAME => LEMON_EQ,
                LOVE_PHILTER_FILE_NAME => PULP_FILTER,
                MAXIMUS_FILE_NAME => PRESS,
                _ => recreation.unwrap_or(TESLA),
            };
            let Some(index) = self.plugins.effect_index(name) else { continue };
            let track = self.state.project.mixer.iter().find(|m| m.index == t);
            let Some(fl) = track.and_then(|m| m.slots.get(usize::from(k))?.as_ref()).map(|s| s.state.as_slice()) else { continue };
            let st = if file == EQ_FILE_NAME {
                Some(lemon_parametric_eq_2::params::plugin_state_from_fl(fl))
            } else if file == LOVE_PHILTER_FILE_NAME {
                // Bytes that are not a Love Philter state open Pulp Filter's starting state.
                Some(pulp_filter::plugin_state_from_fl(fl))
            } else if file == MAXIMUS_FILE_NAME {
                // Bytes that are not a Maximus state open Press's starting state.
                Some(press::plugin_state_from_fl(fl))
            } else if let Some(lemon) = recreation {
                // A state shorter than the original's, or of a later version, opens the defaults (spec:
                // plugins/lemon-small-effects.md §5).
                crate::plugins::recreation_state_from_fl(lemon, fl)
            } else {
                // Bytes that are not an Edison state open an empty Tesla.
                tesla::plugin_state_from_edison(fl)
            };
            made = true;
            if let Err(e) = self.plugins.create_effect(t, k, index, st.as_deref()) {
                self.fx_failed.push(fx_key(t, k));
                self.state.message = Some(format!("{name}: {e}"));
            }
        }
        if !made {
            return;
        }
        self.prepare_plugin_routing();
        self.transport.set_effects(self.plugins.effects());
        self.state.piano.dirty = true;
    }

    /// Plugin host work on the UI thread: create / open requested plugins, apply plugin-window edits to the channel.
    pub(crate) fn plugin_tick(&mut self) {
        // Instances follow their channels by id (an undo, a command), before rows are looked at.
        self.follow_ids(true, false);
        if std::mem::take(&mut self.state.retimed) {
            self.transport.stop();
        }
        // Rows gone without the app (an undone drop that made a channel, `rack::remove_last`): their instruments go.
        let len = self.state.project.channels.len();
        if self.plugins.hosted.range(len..crate::plugins::FX_KEY).next().is_some() {
            self.remap_rows(&|r| (r < len).then_some(r));
        }
        self.scan_tick();
        self.loading_tick();
        self.export_tick();
        crate::missing::tick(self);
        crate::zipped::tick(self);
        self.midi_tick();
        self.sync_midi_outs();
        self.mixer_tick();
        self.follow_ids(false, true);
        self.host_commands();
        crate::presets::pending_tick(self);
        self.params_tick();
        // While a project loads, `loading_tick` starts these a few per tick.
        if self.loading.is_none() {
            self.sync_effects();
            self.sync_wrapped();
            self.sync_osc3();
            self.follow_osc3_samples();
            self.sync_citrus();
            self.sync_slicer();
            self.sync_drums();
            self.sync_native();
        }
        for (row, path) in std::mem::take(&mut self.state.sample_loads) {
            if !self.state.hosted_rows.contains(&row) {
                continue;
            }
            // The root note and stretch settings the load may have read from the file (`rack::read_tags`) go with it.
            let set = self.state.project.channels.get(row).map(crate::plugins::tag_params).unwrap_or_default();
            // A row hosted this tick already started from its channel's sample.
            let new = self.plugins.sampler_path(row).as_deref() != Some(path.as_str());
            if !self.plugins.set_sampler(row, new.then_some(path.as_str()), &set) {
                self.state.message = Some(format!("Lemon Sampler did not take {path}"));
                self.invalidate();
            }
        }
        // A native generator waits for its first check in a child process (`Plugins::instrument_ready`).
        let pending = self.state.pending_plugin;
        if let Some((_, index)) = pending.filter(|&(_, index)| !self.plugins.instrument_ready(index)) {
            let name = self.plugins.available.get(index).map(|a| a.desc.name.clone()).unwrap_or_default();
            let msg = format!("Trying {name} before its first load...");
            if self.state.message.as_deref() != Some(msg.as_str()) {
                self.state.message = Some(msg);
                self.invalidate();
            }
        } else if let Some((row, index)) = pending {
            self.state.pending_plugin = None;
            match self.plugins.create(row, index) {
                Ok(name) => {
                    self.plugins.added(row);
                    self.state.hosted_rows.push(row);
                    self.prepare_plugin_routing();
                    self.transport.set_instruments(self.plugins.instruments());
                    self.state.piano.dirty = true;
                    let format = self.plugins.descriptor(row).map_or("", |d| crate::plugins::format_name(d.format));
                    self.state.message = Some(format!("Added {name} (hosted {format} plugin)"));
                    self.state.pending_open = Some(row);
                }
                Err(e) => self.state.message = Some(format!("Plugin: {e}")),
            }
            self.invalidate();
        }
        // Without plugin windows opening one says so.
        #[cfg(not(plugin_windows))]
        if self.state.pending_open.take().is_some_and(|r| self.state.hosted_rows.contains(&r)) {
            self.state.message = Some(off_text(PLUGIN_WINDOWS));
            self.invalidate();
        }
        #[cfg(plugin_windows)]
        if let Some(row) = self.state.pending_open.take().filter(|r| self.state.hosted_rows.contains(r)) {
            if let Some(info) = channel_info(&self.state.project, row) {
                match self.plugins.open_window(row, info) {
                    Ok(()) => {
                        self.state.raise(Win::Hosted(row));
                        self.select_window_track(row);
                    }
                    Err(e) => {
                        self.state.message = Some(format!("Plugin: {e}"));
                        self.invalidate();
                    }
                }
            }
        }
        // Plugin housekeeping on every platform (callbacks, parameter flushes, restarts, rate changes); the plugin
        // windows' requests come with it.
        #[cfg(plugin_windows)]
        let requests = {
            let members = self.member_infos();
            let (project, mixer, icons) = (&self.state.project, &self.state.mixer, &self.state.playlist.icons);
            self.plugins.idle(|row| {
                // A container's member (`crate::container`): its name over its slot's title.
                if row >= crate::plugins::MEMBER_KEY {
                    return members.get(&row).cloned();
                }
                let info = if row >= crate::plugins::FX_KEY { crate::mixer::fx_info(project, mixer, row) } else { channel_info(project, row) };
                info.map(|i| crate::plugin_window::ChannelInfo { icon: crate::playlist::window_icon(project, icons, row), ..i })
            })
        };
        // No plugin windows here, so no requests.
        #[cfg(not(plugin_windows))]
        self.plugins.idle(|_| None);
        // What was hosted on this tick takes the id of its channel or slot now, before anything moves rows.
        self.follow_ids(false, false);
        self.restore_plugin_identities();
        // Open plugin editors are processed while nothing plays: notes from an instrument's own keyboard sound, and an
        // effect's own playback (Tesla) too. So is the mixer while a track monitors its input (spec:
        // audio/track-io.md §1 "Signal": monitoring works while stopped).
        // The system's input device opens with the output only while a track with an input chosen hears it or is
        // armed, so the microphone is not held (and shown in use) the rest of the time.
        crate::device::want_inputs(self.state.project.mixer.iter().any(|m| m.audio_in >= 0 && (m.monitored() || m.armed)));
        let monitoring = self.state.project.mixer.iter().any(|m| m.monitored()) && crate::device::input_source().is_some();
        let listen = self.plugins.editor_open() || monitoring;
        if listen {
            self.sync_live();
        }
        self.transport.listen(listen);
        // Keys pressed while a plugin window has the focus still play the typing keyboard.
        #[cfg(any(windows, target_os = "macos"))]
        for k in crate::plugin_window::take_keys() {
            use winit::platform::scancode::PhysicalKeyExtScancode;
            if let winit::keyboard::PhysicalKey::Code(code) = winit::keyboard::PhysicalKey::from_scancode(k.scancode) {
                // Space keeps playing / stopping the song (the typing keyboard does not use it).
                if !self.typing_key(code, k.pressed, k.ctrl, k.alt) && k.pressed && code == winit::keyboard::KeyCode::Space {
                    self.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::Space), k.ctrl, k.alt);
                }
            }
        }
        // X11 gives a key let go of over a docked editor to the editor: its typed note ends once the key is up.
        #[cfg(all(plugin_windows, target_os = "linux"))]
        if !self.typing_held.is_empty() || !self.perf_typed.is_empty() {
            if let Some(map) = crate::plugin_window::keys_down() {
                self.release_keys_up(|code| crate::plugin_window::in_keymap(&map, code));
            }
        }
        // A plugin's menu closed without a pick since the last tick.
        self.plugin_menu_tick();
        #[cfg(plugin_windows)]
        {
            for r in requests {
                use crate::plugins::Request;
                let chans = &mut self.state.project.channels;
                match r {
                    Request::Close(_) => continue,
                    Request::Raise(row) => {
                        // A click in a plugin window closes an open app popup and the Song info box, as a click elsewhere
                        // does.
                        if self.state.playlist.popup.take().is_some() {
                            self.invalidate();
                        }
                        self.close_song_info();
                        self.state.raise(Win::Hosted(row));
                        self.select_window_track(row);
                        continue;
                    }
                    // A native Wayland main window (Linux): the popups cannot be lifted above a plugin window there.
                    #[cfg(not(windows))]
                    Request::Menu(..) | Request::Presets(..) | Request::MiscMenu(..) | Request::KnobMenu(..) if self.plugins.owner == 0 => {
                        self.state.message = Some(off_text(PLUGIN_POPUPS));
                        self.invalidate();
                        continue;
                    }
                    #[cfg(not(windows))]
                    Request::PluginMenu(row, m, _) if self.plugins.owner == 0 => {
                        self.plugins.menu_done(row, m.id, None);
                        self.state.message = Some(off_text(PLUGIN_POPUPS));
                        self.invalidate();
                        continue;
                    }
                    Request::PluginMenu(row, m, (x, y)) => {
                        // The plugin's own menu (VST 3 context menu) as the app's popup, over its window docked or
                        // detached (spec: plugins/vst3-hosting.md §9).
                        let detached = self.plugins.window_of(row).is_some_and(|(_, d)| d);
                        let at = self.screen_to_canvas(x, y);
                        if self.show_plugin_menu(row, &m, at) {
                            self.popup_lift = detached.then_some(row);
                        }
                        self.invalidate();
                        continue;
                    }
                    Request::Menu(row, x, y) => {
                        let Some((_, detached)) = self.plugins.window_of(row) else { continue };
                        let (lx, ly) = self.screen_to_canvas(x, y);
                        let mut menu = crate::playlist::plugin_window_menu(row, detached, lx, ly);
                        // A container's member has its presets too (plugins/patcher/README.md §4.2).
                        crate::presets::fill_window_menu(self, row, &mut menu);
                        crate::playlist::fill_plugin_window_menu(&self.state, row, &mut menu);
                        // A channel's window: Replace ▸ lists the generators (an effect slot's stays greyed here; the
                        // mixer's slot menu has its own Replace ▸).
                        if row < self.state.project.channels.len() {
                            if let Some(i) = menu.items.iter_mut().find(|i| i.label == "Replace") {
                                *i = crate::playlist::PItem::sub("Replace", crate::plugin_menu::items(&self.state, crate::plugin_menu::Target::Replace(row), true));
                            }
                        }
                        self.state.playlist.popup = Some(menu);
                        self.popup_lift = detached.then_some(row);
                        self.invalidate();
                        continue;
                    }
                    Request::Presets(row, x, y) => {
                        let Some((_, detached)) = self.plugins.window_of(row) else { continue };
                        let (lx, ly) = self.screen_to_canvas(x, y);
                        self.state.playlist.popup = Some(crate::presets::popup(self, row, lx, ly));
                        self.popup_lift = detached.then_some(row);
                        self.invalidate();
                        continue;
                    }
                    Request::PresetStep(row, d) => {
                        self.state.message = crate::presets::run(self, crate::presets::PCmd::Step(row, d));
                        continue;
                    }
                    Request::DropFile(row, path) => {
                        self.file_on_plugin_window(row, path);
                        continue;
                    }
                    Request::Misc(row, m, v) => {
                        crate::misc::set(self, row, m, v);
                        continue;
                    }
                    Request::MiscMenu(row, m, x, y) => {
                        let Some((_, detached)) = self.plugins.window_of(row) else { continue };
                        let (lx, ly) = self.screen_to_canvas(x, y);
                        self.state.playlist.popup = Some(crate::misc::menu(self, row, m, lx, ly));
                        self.popup_lift = detached.then_some(row);
                        self.invalidate();
                        continue;
                    }
                    Request::KnobMenu(row, c, x, y) => {
                        let Some((_, detached)) = self.plugins.window_of(row) else { continue };
                        let (lx, ly) = self.screen_to_canvas(x, y);
                        self.state.playlist.popup = Some(match c {
                            // The channel's volume and pan: the rack's knob menu (spec: ui/channel-rack.md §19).
                            crate::knobs::Control::Knob(k) => crate::rack::knob_menu(&self.state, k, self.automation_target(&Action::Knob(k)), lx, ly),
                            c => crate::knobs::reset_menu(c, lx, ly),
                        });
                        self.popup_lift = detached.then_some(row);
                        self.invalidate();
                        continue;
                    }
                    // The channel's controls in a plugin window's strip or card: undo steps, a drag or a run of wheel
                    // notches of one knob one step (`control::run_edit`).
                    Request::ToggleOn(row) => {
                        let _ = chans;
                        crate::control::knob_edit(&mut self.state, crate::rack::MUTE_SOLO, Region::Mutes, |s| {
                            s.project.channels.get_mut(row).into_iter().for_each(|c| c.enabled = !c.enabled)
                        })
                    }
                    Request::Volume(row, v) => self.channel_control(row, ChannelControl::Volume, |c| c.volume = v),
                    Request::Pan(row, v) => self.channel_control(row, ChannelControl::Pan, |c| c.pan = v),
                    Request::PitchCents(row, v) => self.channel_control(row, ChannelControl::Pitch, |c| c.pitch_cents = v),
                    Request::PitchRange(row, r) => {
                        let _ = chans;
                        crate::control::edit(&mut self.state, "Channel pitch range", Region::Channel(row), |s| {
                            s.project.channels.get_mut(row).into_iter().for_each(|c| {
                                // A narrower range takes the pitch knob with it.
                                c.misc.pitch_range = r.clamp(1, 48) as u8;
                                let lim = 100 * i32::from(c.misc.pitch_range);
                                c.pitch_cents = c.pitch_cents.clamp(-lim, lim);
                            })
                        })
                    }
                    Request::Track(row, t) => {
                        let _ = chans;
                        let max = crate::mixer::max_insert(&self.state.project);
                        self.channel_control(row, ChannelControl::Track, |c| c.mixer_track = t.min(max));
                    }
                    Request::ShowTrack(row) => {
                        // As a double click on the channel rack's mixer box (spec: ui/channel-rack.md §12).
                        if let Some(t) = chans.get(row).map(|c| c.mixer_track) {
                            self.state.selected_mixer = t;
                            self.state.raise(Win::Mixer);
                        }
                    }
                    Request::Note(row, key, vel, on) => {
                        // The plugin's own keyboard plays on the live stream, like the typing keyboard.
                        if on {
                            self.sync_live();
                            self.transport.note_on(row, u16::from(key), vel);
                        } else {
                            self.transport.note_off(row, u16::from(key));
                        }
                        continue;
                    }
                    Request::Automate(row, index) => {
                        self.state.message = self.params_automate(row, index);
                        self.invalidate();
                        continue;
                    }
                    Request::Audio(path, to) => {
                        self.sample_from_plugin(path, to);
                        continue;
                    }
                    Request::Notes(row, notes) => {
                        self.notes_from_plugin(row, &notes);
                        continue;
                    }
                    Request::NotAvailable(name) => {
                        self.state.message = Some(off_text(name));
                        self.invalidate();
                        continue;
                    }
                    Request::Stopped(_, text) => {
                        self.state.message = Some(text);
                        self.invalidate();
                        continue;
                    }
                }
                self.state.piano.dirty = true;
                self.invalidate();
            }
            self.clip_docked();
        }
    }

    /// Control `which` of the channel on row `row` changed by `f` (a plugin window's strip or card, the rack's mixer
    /// box): an undo step, a run of changes of one control one step; volume, pan and pitch as knobs (none with Undo knob
    /// tweaks off).
    pub(crate) fn channel_control(&mut self, row: usize, which: ChannelControl, f: impl FnOnce(&mut os_model::Channel)) {
        let (name, knob) = match which {
            ChannelControl::Volume => ("Channel volume", true),
            ChannelControl::Pan => ("Channel panning", true),
            ChannelControl::Pitch => ("Channel pitch", true),
            ChannelControl::Track => ("Channel mixer track", false),
        };
        let run = crate::history::Run::Channel(row, which);
        crate::control::run_edit(&mut self.state, name, Region::Channel(row), run, knob, |s| {
            if let Some(c) = s.project.channels.get_mut(row) {
                f(c);
            }
        });
    }

    /// Screen point -> canvas logical px (an X11 main window: Linux; macOS).
    #[cfg(all(plugin_windows, not(windows)))]
    fn screen_to_canvas(&self, x: i32, y: i32) -> (f32, f32) {
        let (x, y) = crate::plugin_window::screen_to_main(x, y).unwrap_or((x, y));
        let k = self.plugins.scale;
        ((x - self.canvas_off.0) as f32 / k, (y - self.canvas_off.1) as f32 / k)
    }

    /// Screen point -> canvas logical px.
    #[cfg(windows)]
    fn screen_to_canvas(&self, x: i32, y: i32) -> (f32, f32) {
        let mut p = windows_sys::Win32::Foundation::POINT { x, y };
        // SAFETY: plain coordinate conversion against our own window.
        unsafe { windows_sys::Win32::Graphics::Gdi::ScreenToClient(self.plugins.owner as _, &mut p) };
        let k = self.plugins.scale;
        ((p.x - self.canvas_off.0) as f32 / k, (p.y - self.canvas_off.1) as f32 / k)
    }

    /// Linux and macOS: mouse input on a lifted popup (`sync_lift`) goes to the app as if it happened in the main window.
    #[cfg(all(plugin_windows, not(windows)))]
    fn lift_input(&mut self) {
        let input = crate::plugin_window::take_lift_input();
        let k = self.plugins.scale;
        let (ox, oy) = self.canvas_off;
        for i in &input {
            match *i {
                crate::plugin_window::LiftInput::Move(x, y) => {
                    self.mouse_move((x - ox) as f32 / k, (y - oy) as f32 / k);
                }
                crate::plugin_window::LiftInput::Down(1) => self.mouse_down(),
                crate::plugin_window::LiftInput::Down(2) => self.mouse_down_middle(),
                crate::plugin_window::LiftInput::Down(3) => self.mouse_down_right(),
                crate::plugin_window::LiftInput::Down(_) => {}
                crate::plugin_window::LiftInput::Up => self.mouse_up(),
            }
        }
    }

    /// After a paint: a popup opened from a detached plugin window is shown above that window (it is painted in the
    /// main window, which the detached window covers).
    #[cfg(plugin_windows)]
    pub fn sync_lift(&mut self, canvas: &Canvas) {
        let lifted = self.popup_lift.filter(|_| self.state.playlist.popup.is_some()).and_then(|r| self.plugins.window_of(r));
        let bounds = self.overlays.iter().copied().reduce(|a, b| a.union(&b));
        match (lifted, bounds) {
            (Some((frame, true)), Some(b)) => {
                let (x, y, w, h) = canvas.device_rect(b);
                let stride = canvas.pixmap.width() as usize;
                let px = canvas.pixmap.pixels();
                let mut buf = Vec::with_capacity(w * h);
                for row in y..y + h {
                    for p in &px[row * stride + x..row * stride + x + w] {
                        buf.push(u32::from(p.red()) << 16 | u32::from(p.green()) << 8 | u32::from(p.blue()));
                    }
                }
                let origin = (x as i32 + self.canvas_off.0, y as i32 + self.canvas_off.1);
                crate::plugin_window::lift_show(self.plugins.owner, frame, origin, (w as i32, h as i32), buf);
            }
            _ => {
                self.popup_lift = None;
                crate::plugin_window::lift_hide();
            }
        }
    }

    /// Docked plugin windows (native children) join `State::z` and are clipped to the workspace minus the drawn
    /// windows above them and the overlays (spec: ui/plugin-window.md "Docking"). Detached windows leave `State::z`.
    #[cfg(plugin_windows)]
    fn clip_docked(&mut self) {
        let docked = self.plugins.docked();
        self.state.z.retain(|w| !matches!(w, Win::Hosted(r) if !docked.contains(r)));
        for &r in &docked {
            if !self.state.z.contains(&Win::Hosted(r)) {
                self.state.z.push(Win::Hosted(r));
            }
        }
        let k = self.plugins.scale;
        let (ox, oy) = self.canvas_off;
        let px = |r: Rect| [(r.x * k).floor() as i32 + ox, (r.y * k).floor() as i32 + oy, (r.right() * k).ceil() as i32 + ox, (r.bottom() * k).ceil() as i32 + oy];
        let work = px(self.state.work_rect());
        let over: Vec<[i32; 4]> = self.overlays.iter().map(|&r| px(r.outset(2.0))).collect();
        let z = self.state.z.clone();
        let stacked: Vec<usize> = z.iter().filter_map(|w| if let Win::Hosted(r) = w { Some(*r) } else { None }).collect();
        self.plugins.stack(&stacked);
        let docked_at = z.iter().rposition(|&w| self.state.is_max(w));
        for (i, &w) in z.iter().enumerate() {
            let Win::Hosted(row) = w else { continue };
            let mut covers = over.clone();
            for &a in &z[i + 1..] {
                if !matches!(a, Win::Hosted(_)) && !self.state.is_detached(a) {
                    covers.push(px(self.win_bounds(a)));
                }
            }
            // Below a maximized window: covered entirely.
            if docked_at.is_some_and(|d| i < d) {
                covers.push(work);
            }
            self.plugins.clip(row, work, &covers);
        }
    }

    /// Paint the whole frame.
    /// Mark everything dirty (state changed in a way that can affect any view).
    pub fn invalidate(&mut self) {
        self.damage = Damage::Full;
    }

    pub(crate) fn damage_rect(&mut self, r: Rect) {
        if r.is_empty() {
            return;
        }
        match &mut self.damage {
            Damage::Full => {}
            Damage::None => self.damage = Damage::Rects(vec![r]),
            Damage::Rects(v) => add_damage(v, r),
        }
    }

    /// The mouse in the top band's own px (`views::band_k`), for its controls laid out there (the tempo digits, the song
    /// position slider, the monitor's columns).
    pub(crate) fn band_mouse(&self) -> (f32, f32) {
        let k = views::band_k(&self.state);
        (self.mouse.0 / k, self.mouse.1 / k)
    }

    /// The mouse in the home px of toolbar panel `p` (`toolbar::Panel::home`), wherever the layout puts it: for what
    /// is laid out there (the song position slider, the monitor's columns).
    pub(crate) fn panel_mouse(&self, p: crate::toolbar::Panel) -> (f32, f32) {
        let (x, y) = self.band_mouse();
        let a = views::panel_at(&self.state, p).unwrap_or_default();
        (x - a.dx, y - a.dy)
    }

    /// Damage rect `r` of the top band, given in the home px of its panels (`HINT_RECT`, `TEMPO_RECT`, `CPU_RECT`, ...):
    /// repainted where the layout and Toolbars scaling put those panels (spec: ui/toolbar-menu.md §8, ui/ui-scaling.md
    /// §1). The floating hint bar repeats the hint panel.
    pub(crate) fn damage_band(&mut self, r: Rect) {
        for d in views::band_rects(&self.state, r) {
            self.damage_rect(d);
        }
        if self.state.toolbar.hint_bar && !r.intersect(&HINT_RECT).is_empty() {
            self.damage_rect(views::toolbar::hint_bar_rect(&self.state));
        }
    }

    /// Whether a paint is due: damage, or a playlist scroll waiting to move its pixels (`playlist_scrolled` adds no
    /// damage of its own; without this a scroll-bar drag or middle-drag grab showed nothing until the release).
    pub fn has_damage(&self) -> bool {
        self.damage != Damage::None || self.scrolled.is_some()
    }

    /// Take the pending damage: `None` = nothing to paint, `Some(None)` = whole frame, else the regions to repaint
    /// (all in one `paint_region`).
    /// A playlist scroll is resolved here (`resolve_scroll`): call it right before `paint_region`.
    pub fn take_damage(&mut self) -> Option<Option<Vec<Rect>>> {
        // An input's paint can come before the next tick (Windows paints before `about_to_wait`): what follows the
        // settings is brought up to date first, so the frame shows it.
        self.follow_settings();
        self.resolve_scroll();
        let damage = std::mem::replace(&mut self.damage, Damage::None);
        // A change that repaints the whole main window may show in a detached one too (the colours, a project opened);
        // the detached windows' own changes mark them (`damage_win`, `on_surface`).
        if damage == Damage::Full {
            self.mark_detached(None);
        }
        match damage {
            Damage::None => None,
            Damage::Full => Some(None),
            Damage::Rects(mut v) => {
                // The Toolbars menu (spec: ui/toolbar-menu.md §2): without separators the rows' pixels move after
                // painting, so the toolbar repaints whole. The band's parts damage themselves where they are drawn
                // (`damage_band`).
                let o = self.state.toolbar;
                let k = views::band_k(&self.state);
                let band = views::scale_rect(views::toolbar::band(&self.state, self.state.screen.0 / k), k);
                if !o.separators && !o.hidden && v.iter().any(|r| !r.intersect(&band).is_empty()) {
                    add_damage(&mut v, band);
                }
                Some(Some(v))
            }
        }
    }

    /// The areas the last paint moved instead of repainting (a playlist scroll; `os_ui::shift_area` rounds the area,
    /// `os_ui::keep_area` the kept rects): the shell moves the same pixels of its window buffer and presents them.
    pub fn take_moved(&mut self) -> Vec<Moved> {
        std::mem::take(&mut self.moved)
    }

    /// The smallest playlist scroll step whose drawn pixels can move (`blit_step` at the last paint's scale).
    pub(crate) fn playlist_blit_step(&self) -> Option<f32> {
        blit_step(self.px_scale)
    }

    /// The playlist view scrolled from `before` (its horizontal and vertical scroll). The next paint moves the pixels
    /// already drawn for it instead of repainting the window when that gives exactly what a repaint would
    /// (`resolve_scroll`). Damage waiting inside the moving parts moves along: those pixels are stale wherever they go.
    pub(crate) fn playlist_scrolled(&mut self, before: (f32, f32)) {
        let now = self.state.playlist_scroll;
        let d = (f64::from(before.0) - f64::from(now.0), f64::from(before.1) - f64::from(now.1));
        if d == (0.0, 0.0) {
            return;
        }
        if let Damage::Rects(v) = &self.damage {
            let stale: Vec<Rect> = self
                .scroll_moves(d)
                .into_iter()
                .flat_map(|(r, sx, sy)| v.iter().map(move |o| moved_by(o.intersect(&r), sx, sy).outset(1.0).intersect(&r)))
                .collect();
            for r in stale {
                self.damage_rect(r);
            }
        }
        let s = self.scrolled.get_or_insert((0.0, 0.0));
        *s = (s.0 + d.0, s.1 + d.1);
    }

    /// The playlist parts a scroll by `d` (logical px) moves, with their move: the grid both ways, the ruler and
    /// marker lane sideways, the track headers up and down (as laid out by the last paint).
    fn scroll_moves(&self, d: (f64, f64)) -> Vec<(Rect, f64, f64)> {
        let Some(grid) = self.pl_grid else { return Vec::new() };
        let p = views::playlist::scroll_parts(&self.state, grid);
        let mut v = vec![(p.grid, d.0, d.1)];
        if d.0 != 0.0 {
            v.push((p.ruler, d.0, 0.0));
            v.extend(p.lane.map(|l| (l, d.0, 0.0)));
        }
        if d.1 != 0.0 {
            v.push((p.heads, 0.0, d.1));
        }
        v
    }

    /// What is drawn over the playlist: the drawn windows above it and the toolbar.
    fn playlist_covers(&self) -> Vec<Cover> {
        let s = &self.state;
        let mut v: Vec<_> = s
            .z
            .iter()
            .skip_while(|&&w| w != Win::Playlist)
            .skip(1)
            .filter(|&&w| matches!(w, Win::Mixer | Win::PianoRoll) || crate::floating::placed(w).is_some())
            .map(|&w| Cover { bounds: self.win_bounds(w).outset(1.0), win: Some(w), solid: self.win_solid(w) })
            .collect();
        if !s.toolbar.hidden {
            v.push(Cover { bounds: Rect::new(0.0, 0.0, s.screen.0, views::work_y(s) + 1.0), win: None, solid: None });
        }
        // The toolbar editor's palette (spec: ui/toolbar-menu.md §8).
        if let Some(p) = views::palette_rect(s) {
            v.push(Cover { bounds: p.outset(1.0), win: None, solid: None });
        }
        // The floating hint bar (spec: ui/toolbar-menu.md §4).
        if s.toolbar.hint_bar {
            v.push(Cover { bounds: views::toolbar::hint_bar_rect(s).outset(1.0), win: None, solid: None });
        }
        v
    }

    /// The part of window `w` its frame fills with opaque colour first, when it floats: the pixels there depend
    /// on the window alone, not on what is under it.
    fn win_solid(&self, w: Win) -> Option<Rect> {
        let s = &self.state;
        let r = match w {
            // The channel window without a channel draws no frame.
            Win::Plugin if s.project.channels.get(s.plugin_channel).is_none() => None,
            w if crate::floating::placed(w).is_some() => crate::floating::placed(w).map(|p| (p.rect)(s)),
            w => {
                let ws = &s.wins[w.slot()?];
                let r = s.win_rect(w);
                (!ws.maxed && !ws.mini && r.h >= 2.0 * TITLE_H).then_some(r)
            }
        }?;
        // Rounded frames (Lemon, spec: ui/lemon-design.md §3): the corners show what is under the window. Leaving out
        // a radius-wide band at the left and right leaves out all four corners.
        let st = self.theme.colors.style;
        let k = st.radius_panel;
        Some(if st.lemon() { Rect::new(r.x + k, r.y, (r.w - 2.0 * k).max(0.0), r.h) } else { r })
    }

    /// Before a paint: turn the pending playlist scroll into canvas moves (`shifts`) and the damage they leave: the
    /// strips scrolled into view, the scroll bars, what covers the playlist (where it is and where its pixels moved
    /// to; a window that did not change keeps the pixels inside its frame, so only its edge and the content it
    /// uncovers paint), the hovered control. The window is repainted instead when moving would not equal a repaint: a
    /// distance off the drawing grid (`PGeom::xs64`) or not whole device pixels, a window moved or resized since the
    /// last paint, an open popup or menu, a drag other than scrolling, a progress panel, the monitor debug panel.
    fn resolve_scroll(&mut self) {
        self.shifts.clear();
        let Some(d) = self.scrolled.take() else { return };
        if self.damage == Damage::Full {
            return;
        }
        let k = f64::from(self.px_scale);
        let exact = |v: f64| (v * 256.0).fract() == 0.0 && (v * k).fract() == 0.0;
        let can = exact(d.0)
            && exact(d.1)
            && self.pl_grid.is_some()
            && views::playlist::grid_of(&self.state) == self.pl_grid
            && self.overlays.is_empty()
            && matches!(self.drag, None | Some(Drag::Scroll { .. }))
            && self.loading.is_none()
            && self.export.is_none()
            && self.state.playlist.overlay.is_none()
            && !self.state.monitor.debug;
        if !can {
            return self.damage_win(Win::Playlist);
        }
        let grid = self.pl_grid.expect("checked");
        // A window that is where it was at the last paint keeps the pixels its frame fills; around them it is
        // painted where it draws (`views::paint_area`: its resize zones are hits only) and where those pixels moved.
        // A window that moved or changed is painted where it was and where it is, with all it may paint.
        let now = self.playlist_covers();
        let mut covers: Vec<(Rect, Option<Rect>)> = now
            .iter()
            .map(|c| match (c.win, c.solid) {
                (Some(w), Some(solid)) if self.pl_covers.contains(c) => (views::paint_area(&self.state, w), Some(solid)),
                _ => (c.bounds, None),
            })
            .collect();
        covers.extend(self.pl_covers.iter().filter(|c| !now.contains(c)).map(|c| (c.bounds, None)));
        for (r, sx, sy) in self.scroll_moves(d) {
            if sx.abs() >= f64::from(r.w) || sy.abs() >= f64::from(r.h) {
                self.damage_rect(r);
                continue;
            }
            // What the move leaves to paint, but for the kept pixels: those show the same as before. (Damage
            // waiting from before stays whole: it is what changed.)
            let mut paint = Vec::new();
            // Covers are painted where they are; the pixels of them that moved are content's.
            for &(c, _) in &covers {
                paint.push(c.intersect(&r));
                paint.push(moved_by(c, sx, sy).intersect(&r));
            }
            let keep: Vec<Rect> = covers.iter().filter_map(|c| c.1).map(|s| s.intersect(&r)).filter(|k| !k.is_empty()).collect();
            self.shifts.push(Moved { area: r, dx: (sx * k) as i32, dy: (sy * k) as i32, keep: keep.clone() });
            let (sx, sy) = (sx as f32, sy as f32);
            // The strips scrolled into view (damage reaches out to whole device pixels, past the moved area's rounded edge).
            if sx != 0.0 {
                let x = if sx > 0.0 { r.x } else { r.right() + sx };
                paint.push(Rect::new(x, r.y, sx.abs(), r.h));
            }
            if sy != 0.0 {
                let y = if sy > 0.0 { r.y } else { r.bottom() + sy };
                paint.push(Rect::new(r.x, y, r.w, sy.abs()));
            }
            // The hovered control's look moved off the mouse.
            if let (Some(a), Some(h)) = (&self.hover, self.hover_rect) {
                if hover_is_visible(a) {
                    paint.push(moved_by(h.outset(3.0), f64::from(sx), f64::from(sy)).intersect(&r));
                }
            }
            for k in &keep {
                let kept = os_ui::inner_rect(*k, self.px_scale);
                paint = paint.into_iter().flat_map(|p| rect_minus(p, kept)).collect();
            }
            for p in paint {
                self.damage_rect(p);
            }
        }
        let parts = views::playlist::scroll_parts(&self.state, grid);
        self.damage_rect(parts.overview);
        self.damage_rect(parts.vbar);
    }

    /// Repaint window `w` only (with its resize zones), or everything when it is not shown.
    // ---------------------------------------------------------------- CPU panel and Plugin performance monitor

    /// The CPU panel's right-click menu (spec: ui/main-window.md §13).
    fn cpu_menu(&mut self, x: f32, y: f32) {
        use crate::cpu::{CpuCmd, Refresh};
        use crate::playlist::{Cmd, Mark, PItem, Popup};
        let c = &self.state.cpu;
        let refresh = Refresh::ALL.iter().map(|&r| PItem::new(r.label(), Cmd::Cpu(CpuCmd::Refresh(r))).mark(Mark::Radio(c.refresh == r))).collect();
        let items = vec![
            PItem::sub("Refresh", refresh),
            PItem::sep(),
            PItem::new("View voice count", Cmd::Cpu(CpuCmd::Fps(false))).mark(Mark::Radio(!c.fps_view)),
            PItem::new("View FPS", Cmd::Cpu(CpuCmd::Fps(true))).mark(Mark::Radio(c.fps_view)),
            PItem::new("Misc", Cmd::Cpu(CpuCmd::Misc)).mark(Mark::Check(c.misc)),
            PItem::sep(),
            PItem::new("Plugin performance monitor", Cmd::Cpu(CpuCmd::Monitor)).mark(Mark::Check(self.state.shown(Win::Perf))),
            PItem::new("Audio settings", Cmd::Cpu(CpuCmd::AudioSettings)),
        ];
        self.state.playlist.popup = Some(Popup { x, y, items, sub: None, scroll: [0; 2] });
    }

    /// The metronome button's right-click menu: the sound and the volume (spec: ui/metronome.md §2), and Recording
    /// precount (spec: ui/recording.md §3 "The metronome's menu").
    pub(crate) fn metronome_menu(&mut self, x: f32, y: f32) {
        use crate::playlist::{Cmd, Mark, PItem, Popup};
        use crate::record::{Opt, RecCmd};
        use os_engine::metronome::Sound;
        let now = crate::metronome::sound();
        let mut items = vec![PItem::head("Metronome sound")];
        items.extend(Sound::ALL.iter().map(|&s| PItem::new(s.label(), Cmd::MetronomeSound(s)).mark(Mark::Radio(s == now))));
        let db = crate::metronome::volume();
        items.push(PItem::head("Metronome volume"));
        for v in os_engine::metronome::VOLUMES {
            items.push(PItem::new(os_engine::metronome::volume_label(v), Cmd::MetronomeVolume(v)).mark(Mark::Radio(v == db)));
        }
        let bars = if Opt::Precount.on() { crate::record::precount_bars() } else { 0 };
        items.push(PItem::head("Count-in"));
        for (label, n) in [("Off", 0), ("1 bar", 1), ("2 bars", 2)] {
            items.push(PItem::new(label, Cmd::Rec(RecCmd::Precount(n))).mark(Mark::Radio(bars == n)));
        }
        self.state.playlist.popup = Some(Popup { x, y, items, sub: None, scroll: [0; 2] });
    }

    /// Metronome switch (toolbar, OPTIONS menu, Ctrl+M): heard from the next beat, also while playing.
    pub fn toggle_metronome(&mut self) {
        let on = crate::metronome::toggle();
        self.state.message = Some(if on { "Metronome on" } else { "Metronome off" }.into());
        self.invalidate();
    }

    /// The auto-scrolling button's right-click menu (spec: ui/auto-scrolling.md §2).
    fn autoscroll_menu(&mut self, x: f32, y: f32) {
        use crate::playlist::{Cmd, Mark, PItem, Popup};
        let items = vec![PItem::new("Continuous scrolling", Cmd::ContinuousScrolling).mark(Mark::Check(crate::autoscroll::continuous()))];
        self.state.playlist.popup = Some(Popup { x, y, items, sub: None, scroll: [0; 2] });
    }

    /// Auto scrolling switch (toolbar, OPTIONS menu): followed from the next move of the playhead.
    pub fn toggle_autoscroll(&mut self) {
        let on = crate::autoscroll::toggle();
        self.state.message = Some(if on { "Auto scrolling on" } else { "Auto scrolling off" }.into());
        self.invalidate();
    }

    /// Auto scrolling (spec: ui/auto-scrolling.md §5): the playlist (song) or the piano roll (pattern) scrolls so the
    /// shown playing position stays in view; in song mode the piano roll too, while a clip of its pattern plays. Left
    /// alone while the mouse drags a scroll bar.
    fn auto_scroll(&mut self) {
        let set = crate::autoscroll::get();
        let now = &self.shown;
        if !set.on || !now.playing || now.paused || matches!(self.drag, Some(Drag::Scroll { .. } | Drag::BarZoom { .. } | Drag::BarEdge { .. } | Drag::BarRows { .. })) {
            return;
        }
        let tick = now.tick as f32;
        let ppq = f32::from(self.state.project.ppq.max(4));
        let pattern_tick = if now.song { piano::pattern_tick_in_song(&self.state, now.tick).map(|t| t as f32) } else { Some(tick) };
        if now.song
            && let Some(g) = self.pl_grid.filter(|_| self.state.shown(Win::Playlist))
        {
            let left = self.state.playlist_scroll.0;
            let x = tick * self.state.playlist_bar_w / (ppq * 4.0);
            if let Some(v) = crate::autoscroll::follow(left, g.w, x, set.continuous) {
                // In whole steps the drawn pixels can move by (`playlist_scrolled`).
                let v = blit_step(self.px_scale).map_or(v, |q| left + ((v - left) / q).round() * q);
                let before = self.state.playlist_scroll;
                self.scroll_set(Scroll::PlaylistX, v);
                self.playlist_scrolled(before);
            }
        }
        let Some(g) = self.piano_grid.filter(|_| self.state.shown(Win::PianoRoll)) else { return };
        let Some(tick) = pattern_tick else { return };
        let left = self.scroll_get(Scroll::PianoX);
        let x = tick * self.state.piano.px_per_beat / ppq;
        if let Some(v) = crate::autoscroll::follow(left, g.w, x, set.continuous) {
            self.scroll_set(Scroll::PianoX, v);
            if self.scroll_get(Scroll::PianoX) != left {
                self.damage_win(Win::PianoRoll);
            }
        }
    }

    /// A command of the CPU panel's menu.
    pub(crate) fn cpu_cmd(&mut self, c: crate::cpu::CpuCmd) -> Option<String> {
        use crate::cpu::CpuCmd;
        match c {
            CpuCmd::Monitor => self.toggle_perf(),
            CpuCmd::AudioSettings => self.open_settings(views::settings::AUDIO),
            c => self.state.cpu.set(c),
        }
        self.invalidate();
        None
    }

    /// Opens the Plugin performance monitor (or brings it to the front), in the middle of the workspace the first
    /// time (spec: ui/plugin-performance.md §1).
    fn open_perf(&mut self) {
        let s = &mut self.state;
        if !s.perf.placed {
            let w = s.work_rect();
            let (pw, ph) = views::perf::SIZE;
            s.perf.pos = ((w.x + (w.w - pw) / 2.0).max(0.0).round(), (w.y + (w.h - ph) / 2.0).max(views::work_y(s)).round());
            s.perf.placed = true;
        }
        if !s.shown(Win::Perf) {
            s.perf.reset();
            let src = self.perf_sources();
            self.state.perf.read(std::time::Instant::now(), src);
        }
        self.state.raise(Win::Perf);
        self.invalidate();
    }

    /// VIEW > Plugin performance monitor and the panel menu's entry: open it, or close it when it is in front.
    fn toggle_perf(&mut self) {
        if self.state.shown(Win::Perf) && self.state.in_front(Win::Perf) {
            self.close_perf();
        } else {
            self.open_perf();
        }
    }

    /// Closing the monitor resets its measurements.
    fn close_perf(&mut self) {
        self.state.hide(Win::Perf);
        self.state.perf.reset();
        self.state.perf.finding = false;
        self.invalidate();
    }

    /// VIEW > Close all windows (F12), Close all unfocused windows (Ctrl+F12), Close all plugin windows (Alt+F12)
    /// (spec: ui/close-windows.md). The browser is a panel and stays.
    pub(crate) fn close_windows(&mut self, what: CloseAll) {
        let plugin = |w: Win| matches!(w, Win::Plugin | Win::Hosted(_));
        // The focused window is the one in front, unless the empty workspace took the last click.
        let keep = match what {
            CloseAll::Unfocused if !self.state.no_focus => self.state.z.last().copied(),
            _ => None,
        };
        let closing: Vec<Win> = self.state.z.iter().copied().filter(|&w| Some(w) != keep && (what != CloseAll::Plugins || plugin(w))).collect();
        // each as its × closes it: the monitor resets, a piano roll tool dialog puts the notes back
        for w in closing {
            self.act(Action::CloseWin(w));
        }
        // Hosted plugin windows (rack rows and mixer slots; detached ones are not in the stack).
        let rows: Vec<usize> = self.plugins.hosted.keys().copied().collect();
        for row in rows.into_iter().filter(|&r| keep != Some(Win::Hosted(r))) {
            self.plugins.close_if_open(row);
        }
        self.invalidate();
    }

    /// Every hosted instance for the monitor, rack rows then mixer slots.
    fn perf_sources(&self) -> Vec<crate::perf::Source> {
        let p = &self.state.project;
        self.plugins
            .hosted
            .iter()
            .map(|(&key, h)| {
                let (synth, place) = match crate::plugins::fx_of(key) {
                    None if key >= crate::plugins::MEMBER_KEY => (false, self.member_place(key).unwrap_or_default()),
                    None => (true, p.channels.get(key).map_or_else(|| format!("Channel {}", key + 1), |c| c.name.clone().unwrap_or_else(|| format!("Channel {}", c.index + 1)))),
                    Some((0, slot)) => (false, format!("Master, slot {}", slot + 1)),
                    Some((track, slot)) => (false, format!("Bus {track}, slot {}", slot + 1)),
                };
                crate::perf::Source { key, name: h.name.clone(), patch: self.plugins.preset_name(key), place, synth, stopped: h.stopped(), meter: h.meter.clone() }
            })
            .collect()
    }

    fn perf_act(&mut self, a: crate::perf::PerfAct) {
        use crate::perf::PerfAct;
        let p = &mut self.state.perf;
        match a {
            PerfAct::Sort(c) => p.sort_by(c),
            PerfAct::ResetOnTransport => p.reset_on_transport = !p.reset_on_transport,
            PerfAct::Freeze => p.freeze = !p.freeze,
            PerfAct::Find => p.finding = true,
            PerfAct::List => {}
            PerfAct::Row(i) => {
                let Some(key) = p.shown().get(i).map(|r| r.key) else { return };
                let now = std::time::Instant::now();
                let double = p.last_click.is_some_and(|(k, t)| k == key && now.duration_since(t).as_millis() < 400);
                p.last_click = if double { None } else { Some((key, now)) };
                if double {
                    // The channel's plugin window, or the mixer slot's.
                    match crate::plugins::fx_of(key) {
                        Some((track, slot)) => self.open_fx(track, slot),
                        None if key >= crate::plugins::MEMBER_KEY => self.state.message = self.open_member_key(key),
                        None => self.state.pending_open = Some(key),
                    }
                }
            }
        }
        self.damage_win(Win::Perf);
    }

    fn perf_wheel(&mut self, notches: f32) {
        let fit = views::perf::rows_fit(&self.state);
        let p = &mut self.state.perf;
        let max = p.shown().len().saturating_sub(fit);
        p.scroll = (p.scroll.min(max) as f32 - (notches * 3.0).round()).clamp(0.0, max as f32) as usize;
    }

    /// Typing into the monitor's Find box. True when the key was taken.
    fn perf_key(&mut self, key: &winit::keyboard::Key, ctrl: bool) -> bool {
        use winit::keyboard::{Key, NamedKey};
        let p = &mut self.state.perf;
        match key {
            Key::Named(NamedKey::Escape) => {
                p.find.clear();
                p.finding = false;
            }
            Key::Named(NamedKey::Enter) => p.finding = false,
            Key::Named(NamedKey::Backspace) => {
                p.find.pop();
            }
            Key::Named(NamedKey::Space) => p.find.push(' '),
            Key::Character(c) if !ctrl => p.find.push_str(c),
            _ => return false,
        }
        p.scroll = 0;
        self.damage_win(Win::Perf);
        true
    }

    /// Play and Stop clear the monitor's peaks when Reset on transport is ticked.
    fn perf_transport(&mut self) {
        if self.state.perf.reset_on_transport {
            self.state.perf.reset_peaks();
            self.damage_win(Win::Perf);
        }
    }

    // ---------------------------------------------------------------- Strum dialog (spec: ui/piano-strum.md)

    /// Tools ▸ Strum… and Alt+S: open the dialog in front (centred over the main window the first time, then where it
    /// was left) on the piano roll's notes, previewing the settings last accepted (§1). Open already: to the front.
    pub(crate) fn open_strum(&mut self) {
        let s = &mut self.state;
        if !s.shown(Win::Strum) {
            if !s.strum.placed {
                let (w, h) = views::strum::SIZE;
                s.strum.pos = (((s.screen.0 - w) / 2.0).max(0.0).round(), ((s.screen.1 - h) / 2.0).max(views::work_y(s)).round());
                s.strum.placed = true;
            }
            crate::strum::open(s);
        }
        s.raise(Win::Strum);
        self.invalidate();
    }

    fn strum_act(&mut self, a: crate::strum::Act) {
        let s = &mut self.state;
        if a == crate::strum::Act::Accept {
            s.message = Some(crate::strum::accept(s));
            s.hide(Win::Strum);
        } else {
            crate::strum::act(s, a);
        }
        self.invalidate();
    }

    // ---------------------------------------------------------------- Arpeggiate dialog (spec: ui/piano-arpeggiate.md)

    /// Tools ▸ Arpeggiate… and Alt+A: open the dialog in front (centred over the main window the first time, then where
    /// it was left) on the piano roll's notes, previewing the settings last accepted (§1). Open already: to the front.
    pub(crate) fn open_arp(&mut self) {
        let s = &mut self.state;
        if !s.shown(Win::Arp) {
            if !s.arp.placed {
                let (w, h) = views::arpeggiate::SIZE;
                s.arp.pos = (((s.screen.0 - w) / 2.0).max(0.0).round(), ((s.screen.1 - h) / 2.0).max(views::work_y(s)).round());
                s.arp.placed = true;
            }
            crate::arpeggiate::open(s);
        }
        s.raise(Win::Arp);
        self.invalidate();
    }

    fn arp_act(&mut self, a: crate::arpeggiate::Act) {
        use crate::arpeggiate::{self as arp, Act, Menu};
        let s = &mut self.state;
        match a {
            Act::Accept => {
                s.message = Some(arp::accept(s));
                s.hide(Win::Arp);
            }
            Act::Folder => {
                let dir = s.arp.pattern.path.as_ref().and_then(|p| p.parent().map(std::path::Path::to_path_buf)).unwrap_or_else(arp::folder);
                let dlg = crate::filedlg::FileDlg::new(crate::filedlg::Mode::Open, "Load arpeggiator pattern", "arpeggiator", crate::dialog::Then::ArpPattern)
                    .filter("Score", &["fsc"])
                    .start(Some(&dir));
                self.file_dialog(dlg);
            }
            Act::Menu(m) => {
                let items = match m {
                    Menu::Pattern => arp::pattern_menu(&s.arp.pattern, &arp::folder()),
                    m => arp::option_menu(&s.arp.set, m),
                };
                let (x, y) = self.mouse;
                self.state.playlist.popup = Some(crate::playlist::Popup { x, y, items, sub: None, scroll: [0; 2] });
            }
            a => arp::act(s, a),
        }
        self.invalidate();
    }

    /// A template or duration mode picked for the Quantize dialog (its menus, its file dialog).
    pub(crate) fn quant_command(&mut self, c: crate::quantize::QCmd) {
        if let Some(m) = crate::piano_dlg::quant_cmd(&mut self.state, c) {
            self.state.message = Some(m);
        }
        self.invalidate();
    }

    /// A pattern picked for the Chop dialog (its ▸ menu, its file dialog); None is the built-in Default.
    pub(crate) fn chop_command(&mut self, p: Option<std::path::PathBuf>) {
        if let Some(m) = crate::piano_dlg::chop_cmd(&mut self.state, p) {
            self.state.message = Some(m);
        }
        self.invalidate();
    }

    /// A pattern, direction or sync picked for the Arpeggiate dialog (its menus, its file dialog).
    pub(crate) fn arp_command(&mut self, c: crate::arpeggiate::ACmd) {
        if let Some(m) = crate::arpeggiate::command(&mut self.state, c) {
            self.state.message = Some(m);
        }
        self.invalidate();
    }

    // ---------------------------------------------------------------- Flam, Flip and Scale levels dialogs

    /// Tools ▸ Flam… (Alt+F), Claw machine… (Alt+W), Flip… (Alt+Y), Scale levels… (Alt+X), Limit… (Alt+K), LFO…
    /// (Alt+O), Quantize… (Alt+Q) and Chop… (Alt+U): open the dialog in front (centred over the main
    /// window the first time, then where it was left) on the piano roll's notes, previewing the settings last accepted.
    /// Another of these dialogs open: its preview is put back and the window shows this one. Scale levels does not open
    /// on a channel control lane (spec: ui/piano-level-scaling.md §4).
    pub(crate) fn open_piano_tool(&mut self, tool: crate::piano_dlg::Tool) {
        let s = &mut self.state;
        if tool == crate::piano_dlg::Tool::Levels && s.piano.lane.is_channel() {
            s.message = Some("Scale levels: show a note property in the Control lane first".into());
            self.invalidate();
            return;
        }
        if !(s.shown(Win::PianoDlg) && s.piano_dlg.tool == tool) {
            let i = tool.index();
            if !s.piano_dlg.placed[i] {
                let (w, h) = views::piano_dlg::size(&s.piano_dlg, tool);
                s.piano_dlg.pos[i] = (((s.screen.0 - w) / 2.0).max(0.0).round(), ((s.screen.1 - h) / 2.0).max(views::work_y(s)).round());
                s.piano_dlg.placed[i] = true;
                if tool == crate::piano_dlg::Tool::Limit {
                    s.piano_dlg.kb_scroll = crate::piano_dlg::KB_HOME;
                }
            }
            crate::piano_dlg::open(s, tool);
        }
        s.raise(Win::PianoDlg);
        self.invalidate();
    }

    fn piano_dlg_act(&mut self, a: crate::piano_dlg::Act) {
        use crate::piano_dlg::{self as dlg, Act};
        let s = &mut self.state;
        match a {
            Act::Accept => {
                s.message = Some(dlg::accept(s));
                s.hide(Win::PianoDlg);
            }
            Act::Options | Act::KeyMenu | Act::ScaleMenu | Act::SnapMenu | Act::PeriodMenu | Act::TrashMenu | Act::ShapeMenu | Act::TemplateMenu | Act::DurMenu => {
                let (x, y) = self.mouse;
                let items = match a {
                    Act::KeyMenu => dlg::key_menu(dlg::key_scale(&s.piano_dlg).0),
                    Act::ScaleMenu => dlg::scale_menu(dlg::key_scale(&s.piano_dlg).1),
                    Act::SnapMenu => dlg::snap_menu(&s.piano_dlg.limit),
                    Act::PeriodMenu => dlg::period_menu(&s.piano_dlg.claw),
                    Act::TrashMenu => dlg::trash_menu(&s.piano_dlg.claw),
                    Act::ShapeMenu => dlg::shape_menu(&s.piano_dlg.lfo),
                    Act::TemplateMenu if s.piano_dlg.tool == dlg::Tool::Chop => crate::chop::pattern_menu(&s.piano_dlg.chop_pat.0, &crate::chop::folder()),
                    Act::TemplateMenu => crate::quantize::template_menu(&s.piano_dlg.quant_tpl.0, &crate::quantize::folder()),
                    Act::DurMenu => crate::quantize::mode_menu(&s.piano_dlg.quant),
                    _ => dlg::options_menu(),
                };
                s.playlist.popup = Some(crate::playlist::Popup { x, y, items, sub: None, scroll: [0; 2] });
            }
            Act::TemplateFolder => {
                let chop = s.piano_dlg.tool == dlg::Tool::Chop;
                let (cur, home) = if chop { (&s.piano_dlg.chop_pat.0, crate::chop::folder()) } else { (&s.piano_dlg.quant_tpl.0, crate::quantize::folder()) };
                let dir = cur.path.as_ref().and_then(|p| p.parent().map(std::path::Path::to_path_buf)).unwrap_or(home);
                let (title, key, then) = if chop { ("Load chopping pattern", "chopper", crate::dialog::Then::ChopPattern) } else { ("Load quantize template", "quantizer", crate::dialog::Then::QuantTemplate) };
                let dlg = crate::filedlg::FileDlg::new(crate::filedlg::Mode::Open, title, key, then).filter("Score", &["fsc"]).start(Some(&dir));
                self.file_dialog(dlg);
            }
            Act::Keys => {
                let k = views::piano_dlg::kb_key(s, self.mouse.0, self.mouse.1, false);
                dlg::set_green(s, k);
                self.drag = Some(Drag::LimitKeys);
            }
            Act::Ruler => {
                let k = views::piano_dlg::kb_key(s, self.mouse.0, self.mouse.1, true);
                let high = dlg::nearer_end(s, k);
                dlg::set_end(s, high, k);
                self.drag = Some(Drag::LimitEnd { high });
            }
            // A press beside the handle brings its middle under the mouse first.
            Act::Scroll => {
                let h = views::piano_dlg::kb_handle(s);
                let dx = if h.outset(2.0).contains(self.mouse.0, self.mouse.1) { self.mouse.0 - h.x } else { h.w / 2.0 };
                self.limit_scroll_handle(self.mouse.0 - dx);
                self.drag = Some(Drag::LimitScroll { dx });
            }
            a => dlg::act(s, a),
        }
        self.invalidate();
    }

    /// Scroll the Limit dialog's keyboard so its scroll bar's handle starts at screen `x`.
    fn limit_scroll_handle(&mut self, x: f32) {
        let s = &mut self.state;
        let h = views::piano_dlg::kb_handle(s);
        let to = s.piano_dlg.kb_scroll + (x - h.x) * views::piano_dlg::kb_scroll_per_px();
        s.piano_dlg.kb_scroll = to.clamp(0.0, crate::piano_dlg::kb_max()).round();
        self.invalidate();
    }

    /// A preset picked from the Flam dialog's Options ▸ menu (spec: ui/piano-flam.md §5).
    pub(crate) fn flam_preset(&mut self, p: crate::flam::Preset) {
        crate::piano_dlg::preset(&mut self.state, p);
        self.invalidate();
    }

    // ---------------------------------------------------------------- Tempo tapper (spec: ui/tempo-tapper.md)

    /// VIEW > Tempo tapper and Ctrl+F5: open it (or bring it to the front), centred over the main window the first
    /// time and then where it was left, also in later runs (`layout`); close it when it is in front (§1, §8).
    fn toggle_tapper(&mut self) {
        let s = &mut self.state;
        if s.shown(Win::Tapper) && s.in_front(Win::Tapper) {
            s.hide(Win::Tapper);
        } else {
            if !s.tapper.placed {
                let (w, h) = views::tapper::SIZE;
                s.tapper.pos = (((s.screen.0 - w) / 2.0).max(0.0).round(), ((s.screen.1 - h) / 2.0).max(views::work_y(s)).round());
                s.tapper.placed = true;
            }
            s.raise(Win::Tapper);
        }
        self.invalidate();
    }

    fn tapper_act(&mut self, a: crate::tapper::TapAct) {
        use crate::tapper::TapAct;
        match a {
            TapAct::Pad => {
                let t = self.state.tapper.clock();
                self.tap(t);
            }
            // Turning Apply tempo on writes the tempo on the pad to the project at once (§5).
            TapAct::Apply => {
                if crate::tapper::toggle_apply() {
                    if let Some(bpm) = self.state.tapper.count.tempo() {
                        crate::control::edit_app(self, crate::control::command::TEMPO, crate::control::command::Region::Settings, |app| app.set_tempo(bpm));
                    }
                }
            }
            // Held while playing (§6): the engine runs 2 % slower / faster until the release; stopped, it does nothing.
            TapAct::Nudge(n) => {
                if self.transport.playing() && !self.transport.paused() {
                    crate::tapper::nudge_flag().store(n.signum(), std::sync::atomic::Ordering::Relaxed);
                }
            }
        }
        self.damage_win(Win::Tapper);
    }

    /// A tap at `t` seconds on the tapper's clock: it counts (§2), lights the pad (§4), clicks (the metronome's Tick,
    /// accented on the taps the beat bar shows orange, §8) and, with Apply tempo on, a new tempo becomes the project
    /// tempo (§5).
    fn tap(&mut self, t: f64) {
        let tp = &mut self.state.tapper;
        let bpm = tp.count.tap(t);
        tp.flash_at = Some(std::time::Instant::now());
        tp.flash = 1.0;
        let accent = tp.count.bar().is_some_and(|(_, accent)| accent);
        #[cfg(test)]
        tp.test_clicks.push(accent);
        #[cfg(not(test))]
        self.transport.tap_click(accent);
        // A count's taps make one undo step; a new count, a new one.
        if bpm.is_none() {
            self.state.history.end_run();
        }
        if let Some(bpm) = bpm.filter(|_| crate::tapper::get().apply) {
            crate::control::edit_run(self, crate::control::command::TEMPO, crate::control::command::Region::Settings, crate::history::Run::Tap, |app| app.set_tempo(bpm));
        }
        self.damage_win(Win::Tapper);
    }

    /// A key on a MIDI keyboard taps while the tapper is open (its pad's hint, §1), and so do a typing-keyboard note
    /// and a Touch controller press (§8); the keys of a chord tap once.
    pub(crate) fn key_tap(&mut self) {
        if !self.state.shown(Win::Tapper) {
            return;
        }
        let t = self.state.tapper.clock();
        if !self.state.tapper.count.last().is_some_and(|l| t - l < crate::tapper::MIDI_CHORD) {
            self.tap(t);
        }
    }

    /// Sets the project tempo, kept in the tempo box's range (10-522 BPM): playback follows from the next tick, the
    /// toolbar shows it. Returns the tempo set.
    pub(crate) fn set_tempo(&mut self, bpm: f64) -> f64 {
        let t = bpm.clamp(10.0, 522.0);
        if self.state.tempo != t {
            self.state.tempo = t;
            self.state.piano.dirty = true;
            self.damage_band(TEMPO_RECT);
        }
        t
    }

    /// Repaint what scroll target `t` scrolls.
    pub(crate) fn damage_scroll(&mut self, t: Scroll) {
        match scroll_win(t) {
            Some(w) => self.damage_win(w),
            None => self.invalidate(),
        }
    }

    pub(crate) fn damage_win(&mut self, w: Win) {
        if !self.state.shown(w) {
            return self.invalidate();
        }
        if self.state.is_detached(w) {
            return self.mark_detached(Some(w));
        }
        match w {
            Win::Playlist | Win::Mixer | Win::PianoRoll => self.damage_rect(self.state.win_area(w)),
            w => match crate::floating::placed(w) {
                Some(p) => self.damage_rect((p.bounds)(&self.state)),
                None => self.invalidate(),
            },
        }
    }

    /// Repaint the live regions of `kind` from the last paint.
    fn damage_live(&mut self, kind: Live) {
        let rs: Vec<Rect> = self.live.iter().filter(|(k, _)| *k == kind).map(|(_, r)| *r).collect();
        for r in rs {
            self.damage_rect(r);
        }
    }

    /// The playback state now, as the frame should show it.
    fn play_state(&self) -> Shown {
        let playing = self.transport.playing();
        Shown {
            playing,
            paused: self.transport.paused(),
            tick: self.transport.tick(),
            song: self.transport.song(),
            sounding: if playing { self.transport.sounding() } else { Vec::new() },
            tempo: self.transport.tempo(),
        }
    }

    /// Damage what playback moved since the shown state, then show the new one: the clock, both playheads (old and
    /// new place), the rack's playing step and the lights of sounding notes. Starting, stopping, pausing and switching
    /// between pattern and song repaint everything.
    fn playback_damage(&mut self) {
        let now = self.play_state();
        self.show_playback(now);
    }

    /// `playback_damage` for a given state (tests and the paint benchmark play without a device).
    pub(crate) fn show_playback(&mut self, now: Shown) {
        let old = std::mem::replace(&mut self.shown, now.clone());
        // The song looped back to its start: the initial values again (spec: ui/settings-choices.md "Advanced:
        // Initialize controls automatically"). The engine sets them itself there; the project follows, so the controls
        // show them.
        let looped = now.playing && old.playing && now.song && old.song && !now.paused && now.tick < old.tick;
        if looped && crate::init::apply(&mut self.state) {
            self.invalidate();
        }
        if now.playing != old.playing || now.paused != old.paused || now.song != old.song {
            self.auto_scroll();
            return self.invalidate();
        }
        if now.tempo != old.tempo {
            self.damage_band(TEMPO_RECT);
        }
        if !now.playing {
            return;
        }
        if now.tick != old.tick {
            self.damage_live(Live::Clock);
            let (pl, piano, rack) = (self.state.shown(Win::Playlist), self.state.shown(Win::PianoRoll), self.state.shown(Win::ChannelRack));
            if now.song && pl {
                if let Some(g) = self.pl_grid {
                    let pg = crate::playlist::PGeom::new(&self.state, g);
                    // The playhead line and the ruler's song-position triangle (and the marker lane between them).
                    let top = g.y - 22.0 - crate::playlist::MARKER_H;
                    for t in [old.tick, now.tick] {
                        // The flag's outline reaches x + 10.5 (views/playlist.rs ruler_paint).
                        let x = pg.x_of(t as f32);
                        self.damage_rect(Rect::new(x - 8.0, top, 20.0, g.bottom() - top));
                    }
                }
            }
            if piano {
                if let Some(g) = self.piano_geom() {
                    let grid = g.grid;
                    // In song mode the piano roll's playhead is where its pattern plays, while a clip of it does.
                    let at = |t: f64| if now.song { piano::pattern_tick_in_song(&self.state, t) } else { Some(t) };
                    let xs: Vec<f32> = [at(old.tick), at(now.tick)].into_iter().flatten().map(|t| g.x_of(t as f32)).collect();
                    for &x in &xs {
                        self.damage_rect(Rect::new(x - 2.0, grid.y, 5.0, grid.h));
                    }
                    // A note held while recording grows from the old playhead to the new one (spec: ui/recording.md §2).
                    if let [a, b] = xs[..]
                        && self.state.rec.holding()
                    {
                        self.damage_rect(Rect::new(a.min(b) - 2.0, grid.y, (a - b).abs() + 5.0, grid.h));
                    }
                }
            }
            if !now.song && rack {
                for r in views::rack::step_columns(&self.state, old.tick, now.tick) {
                    self.damage_rect(r);
                }
            }
            self.auto_scroll();
        }
        if now.sounding != old.sounding {
            self.damage_live(Live::Sounding);
        }
    }

    /// Debug panel slider `i` follows mouse x.
    fn set_mon_param(&mut self, i: usize, x: f32) {
        let t = views::mondebug::slider(i);
        let p = &crate::monitor::PARAMS[i];
        let k = ((x - t.x) / t.w).clamp(0.0, 1.0);
        self.state.monitor.cfg.set(i, p.min + (p.max - p.min) * k);
        self.hint = Cow::Owned(format!("{}: {}", p.name, self.state.monitor.cfg.text(i)));
        self.damage_rect(views::mondebug::PANEL);
        self.damage_band(MONITOR_RECT);
        self.damage_band(HINT_RECT);
    }

    /// The cable physics panel's controls (views/cabledebug.rs).
    fn cable_debug(&mut self, d: crate::cable::DebugAct) {
        use crate::cable::DebugAct;
        let c = self.state.cables.get_mut();
        match d {
            DebugAct::Param(i) => {
                self.drag = Some(Drag::CableParam(i));
                let x = self.mouse.0;
                return self.set_cable_param(i, x);
            }
            DebugAct::Reset => {
                for i in 0..crate::cable::PARAMS.len() {
                    c.set(i, crate::cable::PARAMS[i].default);
                }
            }
            DebugAct::Save => {
                self.state.message = Some(match save_cable_cfg(&c.tuning) {
                    Ok(p) => format!("Cable physics saved to {}", p.display()),
                    Err(e) => format!("Cable physics not saved: {e}"),
                });
            }
            DebugAct::Close => c.debug = false,
        }
        self.invalidate();
    }

    /// Cable debug panel slider `i` follows mouse x; the cables take the new value at once.
    fn set_cable_param(&mut self, i: usize, x: f32) {
        let t = views::cabledebug::slider(i);
        let p = &crate::cable::PARAMS[i];
        let k = ((x - t.x) / t.w).clamp(0.0, 1.0);
        let c = self.state.cables.get_mut();
        c.set(i, p.min + (p.max - p.min) * k);
        self.hint = Cow::Owned(format!("{}: {}", p.name, c.tuning.text(i)));
        self.damage_rect(views::cabledebug::PANEL);
        self.damage_band(HINT_RECT);
        self.damage_win(Win::Mixer);
    }

    /// The monitor is moving (a signal or a decay), or the tapper's pad fading, or the CPU panel counting frames (View
    /// FPS): the shell should tick at animation rate.
    pub fn animating(&self) -> bool {
        self.transport.playing()
            || matches!(self.drag, Some(Drag::SongPosHold { .. }))
            || self.state.monitor.active()
            || self.state.mixer.meters.iter().any(|m| m.active())
            || self.loading()
            || self.plugin_windows_busy()
            || self.state.tapper.flash_at.is_some()
            || self.song_info_fading()
            || self.state.cables.borrow().moving()
            || self.hold.is_some()
            || self.smooth.running()
            || self.midi_play.busy()
            || self.remote.busy()
            || self.state.browser_marquee.borrow().is_some()
            || self.state.cpu.fps_view
    }

    /// Linux: a plugin window is open (its X11 events and the editor's run loop are served on the tick).
    fn plugin_windows_busy(&self) -> bool {
        #[cfg(target_os = "linux")]
        return crate::plugin_window::busy();
        #[cfg(not(target_os = "linux"))]
        false
    }

    /// General ▸ Miscellaneous checks that follow a change, on every tick and before a frame: "Select first channel with
    /// notes when the pattern changes" (`App::pattern_tick`), "Auto zoom in piano roll" (`piano::open_tick`). Also the
    /// theme's rule for new clips' channel colours, which the code making them reads from the state.
    /// The output device opened (again, or another one: a fallback, a driver set to another rate): hosted plugins move
    /// to its rate.
    fn follow_device(&mut self) {
        let opens = crate::device::opens();
        if opens == self.device_opens {
            return;
        }
        self.device_opens = opens;
        if let Some(rate) = crate::device::last().map(|i| i.rate) {
            self.plugins.set_rate(rate);
            self.state.settings.rate = Some(rate);
        }
    }

    fn follow_settings(&mut self) {
        self.pattern_tick();
        if crate::piano::open_tick(&mut self.state) {
            self.invalidate();
        }
        let clips = self.clip_colors();
        if self.state.clip_colors != clips {
            self.state.clip_colors = clips;
        }
    }

    /// The current theme's rule for the colours of new audio and automation clips' channels (read-only): the applied
    /// `.flstheme`'s `OverrideClips` ("Audio and automation clips use note colors", which the Theme page toggles; off
    /// for the stock Dark theme used without a file) and its note colours.
    /// Spec: ui/settings-choices.md "Light mode and clip note colours (2026-10-16)".
    pub fn clip_colors(&self) -> crate::clip_colors::ClipColors {
        crate::clip_colors::ClipColors::of(&self.state.fl_theme())
    }

    /// The mixer's cables swing until still: step them and repaint where they were and are.
    fn cables_tick(&mut self, now: std::time::Instant) {
        if !self.state.shown(Win::Mixer) {
            self.state.cables.get_mut().clear();
            self.cables_at = None;
            return;
        }
        let dt = self.cables_at.map_or(0.0, |t| now.duration_since(t).as_secs_f32());
        let cables = self.state.cables.get_mut();
        let moved = cables.advance(dt);
        self.cables_at = cables.moving().then_some(now);
        if let Some(r) = moved {
            self.damage_rect(r);
        }
    }

    /// Periodic housekeeping; returns whether a repaint is needed.
    pub fn tick(&mut self) -> bool {
        // Things the interface added get their ids before anything looks them up (`control::ids`).
        let s = &mut self.state;
        s.control.settle_tick(&mut s.project, s.piano.dirty);
        self.control_tick();
        self.plugin_tick();
        // Playback loops over the time selection, as it is now (spec: ui/recording.md §5).
        self.sync_loop_range();
        #[cfg(all(plugin_windows, not(windows)))]
        self.lift_input();
        {
            // Scope / analyzer / peak bars: from the audio tap; repaint just the monitor when it changed.
            // The monitor advances at its own refresh rate, however often we tick.
            let (tp, rate) = (&self.transport, self.transport.sample_rate());
            if self.state.monitor.update(std::time::Instant::now(), rate, || tp.take_peaks(), |b| tp.tap(b)) {
                self.damage_band(MONITOR_RECT);
            }
        }
        if let Some(e) = self.transport.take_error() {
            self.state.message = Some(format!("Audio: {e}"));
            self.invalidate();
        }
        self.follow_device();
        // Options ▸ Focus selected channel on playback and Auto switch display filter (spec: ui/channel-rack.md §17).
        if crate::rack::focus_tick(&mut self.state, self.transport.playing()) | crate::rack::filter_tick(&mut self.state) {
            self.invalidate();
        }
        self.record_tick();
        self.audio_log.tick();
        // CPU panel (spec: ui/main-window.md §13) and the Plugin performance monitor: a reading at the panel's refresh
        // rate.
        let now = std::time::Instant::now();
        self.song_pos_tick(now);
        self.hold_tick(now);
        self.ease_tick(now);
        self.cables_tick(now);
        self.marquee_tick(now);
        self.browser_index_tick();
        self.backup_tick(now);
        self.work_tick(now);
        self.song_info_tick(now);
        self.auto_close_tick(now);
        // Audio ▸ CPU ▸ Auto-bypass when silent, for every slot whose own switch is on (spec: audio/auto-bypass.md).
        os_engine::engine::auto_bypass::set_enabled(crate::prefs::check(&crate::prefs::SMART_DISABLE));
        self.follow_settings();
        if self.state.cpu.due(now) {
            let tp = &self.transport;
            let load = tp.playing().then(|| tp.load()).filter(|_| !tp.paused());
            // Every stream's engine voices and every hosted instrument's (spec: audio/voices.md §5).
            let voices = tp.engine_voices() + self.plugins.voices();
            let dropouts = tp.underruns();
            if self.state.shown(Win::Perf) {
                let src = self.perf_sources();
                if self.state.perf.read(now, src) {
                    self.damage_win(Win::Perf);
                }
            }
            if self.state.cpu.update(now, load, voices, dropouts) {
                self.damage_band(CPU_RECT);
                let (x, y) = self.mouse;
                if self.top_hit(x, y).is_some_and(|h| h.action == Action::CpuPanel) {
                    self.hint = Cow::Owned(self.state.cpu.hint());
                    self.damage_band(HINT_RECT);
                }
                if self.state.settings.open && self.state.settings.tab == views::settings::AUDIO && self.audio_status() {
                    self.invalidate();
                }
            }
        }
        // View FPS: the panel is painted every frame, so it reads the rate the interface can keep up (the display's
        // refresh while nothing is slow), not just how often something else changed.
        if self.state.cpu.fps_view {
            self.damage_band(CPU_RECT);
        }
        // The tapper's pad fades back after a tap (spec: ui/tempo-tapper.md §4).
        if self.state.tapper.fade(now) && self.state.shown(Win::Tapper) {
            self.damage_rect(views::tapper::pad_rect(&self.state));
        }
        // Mixer meters (spec: ui/mixer.md §9): fall back after playback stops.
        let peaks = self.transport.take_track_peaks();
        if crate::mixer::feed_meters(&mut self.state, &peaks) && self.state.shown(Win::Mixer) {
            self.damage_live(Live::Meter);
        }
        // Audio clips follow their channel's stretch, pitch and the tempo (ui/playlist.md §9c). The lengths are worked
        // out, not edited: the saved copy gets them too, so a fit alone (an opened FL project) is not an unsaved change.
        if let Some(fits) = crate::playlist::audio_fits(&self.state) {
            if crate::playlist::fit_clips(&mut self.state.project.arrangements, &fits) {
                crate::playlist::fit_clips(&mut self.saved.0.arrangements, &fits);
                self.state.piano.dirty = true;
                self.damage_win(Win::Playlist);
            }
        }
        crate::playlist::follow_crossfades(self);
        // Resumed from a Marker pause: playback counts as started on the marker, so Stop returns there (spec:
        // audio/transport.md "Time markers in SONG mode").
        if let Some(t) = self.transport.take_marker_resumed() {
            self.state.playlist.pos = t;
            self.damage_win(Win::Playlist);
        }
        // Edits (notes, steps, mixer, channel volume / pan / mute, tempo) reach playback right away,
        // also during a drag (at most once per tick).
        if self.state.piano.dirty {
            self.state.piano.dirty = false;
            // An edit in the interface (a command's change is its event already; an undo or redo is one too).
            let s = &mut self.state;
            if s.control.changed_since_seen(&s.project, s.tempo) {
                s.control.event(&crate::control::By::User, "edit", "", Vec::new());
            }
            // Also while Play is still starting (not `playing` yet); ignored when stopped.
            {
                self.transport.update(&crate::rack::play_project(&self.state));
            }
        }
        self.playback_damage();
        self.has_damage()
    }

    /// Paint the whole frame (`damage` None) or just the damaged rect.
    pub fn paint(&mut self, canvas: &mut Canvas, damage: Option<Rect>) {
        self.paint_region(canvas, damage.as_ref().map(std::slice::from_ref));
    }

    /// Paint the whole frame (None) or only the given rects, in one pass (see `Canvas::begin_region`).
    /// What a paint shows of playback and the drags in progress.
    fn frame_info(&self) -> views::FrameInfo {
        views::FrameInfo {
            title: self.title(),
            hint: self.hint.clone(),
            playing: self.shown.playing,
            paused: self.shown.paused,
            tick: self.shown.tick,
            song: self.shown.song,
            tempo: self.shown.tempo.unwrap_or(self.state.tempo),
            peaks: self.transport.peaks(),
            sounding: self.shown.sounding.clone(),
            song_pos_drag: matches!(self.drag, Some(Drag::SongPos { .. })),
            bar_drag: match self.drag {
                Some(Drag::Bar(d)) => Some(d),
                _ => None,
            },
        }
    }

    /// Options ▸ Detached on window `w` (the Mixer, the Channel rack; spec: ui/mixer.md §6.2, Lemon Studio's design):
    /// `on` shows it in a native window of its own, at the size it has in the main window, and brings it to the front;
    /// off puts it back in the main window where it was. A maximized or collapsed window detaches floating.
    pub fn set_detached(&mut self, w: Win, on: bool) {
        let s = &mut self.state;
        if on == s.is_detached(w) {
            return;
        }
        if on {
            let r = s.float_rect(w);
            if let Some(i) = w.slot() {
                s.wins[i].rect = r;
                s.wins[i].maxed = false;
                s.wins[i].mini = false;
            }
            s.detached.push((w, (r.w, r.h)));
            self.screen_of(w);
        } else {
            s.detached.retain(|d| d.0 != w);
            // Re-attached from its own window (its menu): `on_surface` drops the surface once done with it.
            if s.surface != Some(w) {
                self.screens.retain(|d| d.0 != w);
            }
            if s.overlay_on == Some(w) {
                s.overlay_on = None;
            }
        }
        self.state.raise(w);
        self.drag = None;
        self.invalidate();
    }

    /// The detached windows that are shown: each needs its native window (the shell opens and closes them to match).
    pub fn detached_windows(&self) -> Vec<Win> {
        self.state.detached.iter().map(|d| d.0).filter(|&w| self.state.shown(w)).collect()
    }

    /// Where detached window `w` was in the main window (logical px), to open its native window over that place.
    pub fn detached_origin(&self, w: Win) -> Option<Rect> {
        let s = &self.state;
        match w {
            Win::ChannelRack => {
                let ((dx, dy), (aw, ah)) = s.rack_area();
                Some(Rect::new(s.rack_pos.0 - dx, s.rack_pos.1 - dy, aw, ah))
            }
            w => w.slot().map(|i| s.wins[i].rect),
        }
    }

    /// The native window that `w` was last in (outer x, y and inner w, h in window-system px), from this run or the
    /// settings.
    pub fn detached_frame(&self, w: Win) -> Option<[i32; 4]> {
        self.state.detached_frames.iter().find(|d| d.0 == w).map(|d| d.1)
    }

    /// The shell's native window for `w` now has this size (logical px): it is painted again at that size. The Channel
    /// rack takes it as its own size, less the graph editor below it.
    pub fn set_detached_size(&mut self, w: Win, size: (f32, f32)) {
        let s = &mut self.state;
        if w == Win::ChannelRack && s.is_detached(w) {
            let (_, (aw, ah)) = s.rack_area();
            let (ew, eh) = (aw - s.rack_size.0, ah - s.rack_size.1);
            let to = ((size.0 - ew).max(views::rack::min_w(s)), (size.1 - eh).max(views::rack::MIN_H));
            if to != s.rack_size {
                s.rack_size = to;
                self.mark_detached(Some(w));
            }
            return;
        }
        if let Some(d) = s.detached.iter_mut().find(|d| d.0 == w) {
            if d.1 != size {
                d.1 = size;
                self.mark_detached(Some(w));
            }
        }
    }

    /// The native window for `w` sits at `frame` (outer x, y and inner w, h in window-system px), kept for `layout`.
    pub fn set_detached_frame(&mut self, w: Win, frame: [i32; 4]) {
        let f = &mut self.state.detached_frames;
        f.retain(|d| d.0 != w);
        f.push((w, frame));
    }

    /// Marks detached window `w` (all of them with `None`) for painting again.
    pub(crate) fn mark_detached(&mut self, w: Option<Win>) {
        for (win, sc) in &mut self.screens {
            if w.is_none_or(|w| w == *win) {
                sc.dirty = true;
            }
        }
    }

    /// Detached window `w`'s surface (`Screen`), made on first use (a window detached by the settings at the start,
    /// `layout::restore`, has none yet).
    fn screen_of(&mut self, w: Win) -> Option<usize> {
        if !self.state.is_detached(w) {
            return None;
        }
        Some(self.screens.iter().position(|d| d.0 == w).unwrap_or_else(|| {
            self.screens.push((w, Screen { mouse: (-1.0, -1.0), dirty: true, ..Screen::default() }));
            self.screens.len() - 1
        }))
    }

    /// A drag that carries something from one window to another: a browser item (a sample, a plugin, a preset, a
    /// pattern or a channel of the project). The shell gives its moves and its release to the surface under the
    /// cursor on screen, the main window's or a detached window's (`State::drag_on`).
    pub fn cross_drag(&self) -> bool {
        matches!(self.drag, Some(Drag::Browser { .. }))
    }

    /// A key pressed while surface `on` has the keyboard focus (`None`: the main window, else that detached window's
    /// native window): it acts on that surface, and a popup or type-in box it opens is drawn there.
    pub fn key_on(&mut self, on: Option<Win>, key: &winit::keyboard::Key, ctrl: bool, alt: bool) {
        let open = |a: &Self| a.state.playlist.popup.is_some() || a.state.playlist.edit.is_some();
        let before = open(self);
        self.on(on, |a| a.key(key, ctrl, alt));
        if !before && open(self) {
            self.state.overlay_on = on.filter(|&w| self.state.is_detached(w));
            self.invalidate();
        }
    }

    /// Runs `f` on surface `on`: a detached window's (`on_surface`), or the main window's (`None`).
    pub fn on<R>(&mut self, on: Option<Win>, f: impl FnOnce(&mut Self) -> R) -> R {
        match on {
            Some(w) => self.on_surface(w, f),
            None => f(self),
        }
    }

    /// The button released in the native window of surface `from` (`None`: the main window's). A drag between windows
    /// (`cross_drag`) ends on the surface it was last moved over, where it drops.
    pub fn mouse_up_from(&mut self, from: Option<Win>) {
        let to = if self.cross_drag() { self.state.drag_on.filter(|&w| self.state.is_detached(w)) } else { from };
        self.on(to, Self::mouse_up);
    }

    /// Whether detached window `w` has something new to show (painting it, `paint_detached`, clears that).
    pub fn detached_dirty(&self, w: Win) -> bool {
        self.state.is_detached(w) && self.screens.iter().find(|d| d.0 == w).is_none_or(|d| d.1.dirty)
    }

    /// Runs `f` (input or a paint) on detached window `w`'s surface: its hits, mouse and hover are swapped in, the state
    /// knows the surface (`State::surface`) and its screen is that window's size. Not detached: `f` runs on the main
    /// window's.
    pub fn on_surface<R>(&mut self, w: Win, f: impl FnOnce(&mut Self) -> R) -> R {
        let (Some(i), Some(size)) = (self.screen_of(w), self.state.detached_size(w)) else {
            return f(self);
        };
        self.swap_screen(i);
        let main_screen = std::mem::replace(&mut self.state.screen, size);
        self.state.surface = Some(w);
        // The Channel rack is laid out from its place (`views::rack::rect`): at its window's top-left meanwhile.
        let rack_pos = (w == Win::ChannelRack).then(|| {
            let (off, _) = self.state.rack_area();
            std::mem::replace(&mut self.state.rack_pos, off)
        });
        let r = f(self);
        if let Some(p) = rack_pos {
            self.state.rack_pos = p;
        }
        self.state.surface = None;
        self.state.screen = main_screen;
        // (`f` may have detached or re-attached other windows.)
        let i = self.screens.iter().position(|d| d.0 == w).unwrap_or(i);
        self.swap_screen(i);
        // Its look changed with its hover or what was done there. Re-attached meanwhile (`set_detached`), its surface
        // goes.
        if self.state.is_detached(w) {
            self.screens[i].1.dirty = true;
        } else {
            self.screens.remove(i);
        }
        r
    }

    fn swap_screen(&mut self, i: usize) {
        let sc = &mut self.screens[i].1;
        std::mem::swap(&mut self.hits, &mut sc.hits);
        std::mem::swap(&mut self.bars, &mut sc.bars);
        std::mem::swap(&mut self.live, &mut sc.live);
        std::mem::swap(&mut self.keep, &mut sc.keep);
        std::mem::swap(&mut self.overlays, &mut sc.overlays);
        std::mem::swap(&mut self.mouse, &mut sc.mouse);
        std::mem::swap(&mut self.hover, &mut sc.hover);
        std::mem::swap(&mut self.hover_rect, &mut sc.hover_rect);
    }

    /// Paints detached window `w` whole into `canvas` (its native window's), with the popups and boxes opened from it.
    pub fn paint_detached(&mut self, w: Win, canvas: &mut Canvas) {
        if w != Win::ChannelRack {
            if let Some(d) = self.state.detached.iter_mut().find(|d| d.0 == w) {
                d.1 = canvas.size();
            }
        }
        let info = self.frame_info();
        self.on_surface(w, |app| {
            canvas.begin_frame(None);
            app.hits.clear();
            app.bars.clear();
            app.live.clear();
            app.keep = Keep { full: true, ..Keep::default() };
            let mut ui = Ui {
                c: canvas,
                t: &app.theme.colors,
                hits: &mut app.hits,
                bars: &mut app.bars,
                hover: &app.hover,
                pressed: &app.pressed,
                mouse: app.mouse,
                win: None,
                keep: &mut app.keep,
                live: &mut app.live,
                flat: app.state.toolbar.flat,
            };
            let overlays = views::paint_detached(&mut ui, &app.state, &info, w);
            app.keep.next.clear();
            app.overlays = app.hits[overlays.min(app.hits.len())..].iter().map(|h| h.rect).collect();
        });
        if let Some(d) = self.screens.iter_mut().find(|d| d.0 == w) {
            d.1.dirty = false;
        }
    }

    pub fn paint_region(&mut self, canvas: &mut Canvas, damage: Option<&[Rect]>) {
        // The CPU panel's View FPS counts painted frames.
        self.state.cpu.frame();
        // A playlist scroll's moves (`take_damage`) go first; the damage they left is painted with the rest.
        debug_assert!(damage.is_none() || self.scrolled.is_none(), "take_damage resolves a scroll before a partial paint");
        self.moved.clear();
        if damage.is_some() && self.px_scale == canvas.scale {
            for m in std::mem::take(&mut self.shifts) {
                canvas.shift(m.area, m.dx, m.dy, &m.keep);
                self.moved.push(m);
            }
        }
        self.shifts.clear();
        if damage.is_none() {
            self.scrolled = None;
        }
        self.px_scale = canvas.scale;
        match damage {
            Some(rects) => canvas.begin_region(rects),
            None => canvas.begin_frame(None),
        }
        self.state.screen = canvas.size();
        self.state.ui_scale = canvas.scale;
        // The main window or the workspace shrank: the windows keep their part in it (`win_clamp::keep_all`). Moved
        // during a partial paint, the next frame is painted whole.
        if crate::win_clamp::keep_all(&mut self.state) && damage.is_some() {
            self.invalidate();
        }
        // Plugin editors in this process colour their waveforms as the views do (spec: ui/settings-choices.md
        // "Display: Colorful waveforms (2026-10-16)").
        crate::waves::publish(&self.theme.colors);
        // The widget kit's scroll bars follow High visibility (spec: ui/settings-choices.md "Display: PPI ruler, Force
        // refreshes, Thick lines, High visibility").
        crate::high_visibility::publish();
        // Last paint's hits stay available to the parts this one skips (see `Ui::part`).
        std::mem::swap(&mut self.hits, &mut self.keep.prev);
        std::mem::swap(&mut self.bars, &mut self.keep.prev_bars);
        std::mem::swap(&mut self.live, &mut self.keep.prev_live);
        self.hits.clear();
        self.bars.clear();
        self.live.clear();
        self.keep.full = damage.is_none();
        let info = self.frame_info();
        let mut ui = Ui {
            c: canvas,
            t: &self.theme.colors,
            hits: &mut self.hits,
            bars: &mut self.bars,
            hover: &self.hover,
            pressed: &self.pressed,
            mouse: self.mouse,
            win: None,
            keep: &mut self.keep,
            live: &mut self.live,
            flat: self.state.toolbar.flat,
        };
        let overlays = views::paint(&mut ui, &self.state, &info);
        if self.state.cables.get_mut().take_slowed() {
            self.state.message = Some("Mixer cables hang still: the mixer is too slow to redraw them swinging (they try again with a new route or another current track)".into());
            self.damage_win(Win::Mixer);
        }
        self.keep.parts = std::mem::take(&mut self.keep.next);
        self.keep.prev.clear();
        self.keep.prev_bars.clear();
        self.keep.prev_live.clear();
        self.overlays = self.hits[overlays.min(self.hits.len())..].iter().map(|h| h.rect).collect();
        // Remember where the piano roll's regions are, for mouse mapping and damage.
        let find = |a: Action| self.hits.iter().find(|h| h.action == a).map(|h| h.rect);
        self.piano_grid = find(Action::PianoGrid);
        self.piano_panel = find(Action::PianoVelocity);
        self.pl_grid = find(Action::PlaylistArea);
        self.pl_covers = self.playlist_covers();
        self.piano_win = match (self.piano_grid, self.piano_panel) {
            // The window below its title bar: overview, ruler, keyboard, grid, velocity panel.
            (Some(g), Some(p)) => Some(Rect::new(g.x - 73.0, g.y - 41.0, g.w + 96.0, p.bottom() - g.y + 41.0)),
            _ => None,
        };
        if let Some(l) = &self.loading {
            crate::loading::paint(canvas, &self.theme.colors, l);
        }
        if let Some(s) = &self.missing.search {
            crate::missing::paint(canvas, &self.theme.colors, s);
        }
        if let Some(z) = &self.zipping {
            crate::zipped::paint(canvas, &self.theme.colors, z);
        }
        if let Some((name, p)) = &self.export {
            let status = format!("Rendering {name}   {:.0} %", p * 100.0);
            crate::loading::panel(canvas, &self.theme.colors, "Exporting", &status, *p);
        }
    }

    /// Repaint the piano roll body (note edits change the grid, overview and velocity panel).
    pub(crate) fn damage_piano(&mut self) {
        match self.piano_win {
            Some(r) => self.damage_rect(r),
            None => self.invalidate(),
        }
        // In PAT mode the song position slider shows the play-start marker within the pattern's length.
        if !self.state.song_mode {
            self.damage_band(crate::song_pos::PANEL);
        }
    }

    pub(crate) fn piano_geom(&self) -> Option<Geom> {
        self.piano_grid.map(|g| Geom::new(&self.state, g))
    }

    /// A press on the piano roll's splitter: drags it; a double click hides / brings back the event panel (spec:
    /// ui/event-panel-splitter.md §2).
    fn piano_split_press(&mut self) {
        let now = std::time::Instant::now();
        let p = &mut self.state.piano;
        let double = p.split_click.is_some_and(|t| now.duration_since(t).as_millis() < 400);
        p.split_click = if double { None } else { Some(now) };
        if double {
            p.split = p.split.toggled();
            crate::piano_split::save(p.split);
            return self.damage_piano();
        }
        let band = self.hits.iter().find(|h| h.action == Action::PianoSplitter).map(|h| h.rect);
        if let (Some(band), Some(g), Some(panel)) = (band, self.piano_grid, self.piano_panel) {
            let h = panel.bottom() - g.y - crate::piano_split::BAND;
            self.drag = Some(Drag::PianoSplit { dy: self.mouse.1 - band.y, top: g.y, h });
        }
    }

    /// The playlist grid from the last paint.
    pub fn pl_grid(&self) -> Option<Rect> {
        self.pl_grid
    }

    /// The playlist takes the editing keys: it is shown and above the piano roll (or the piano roll is hidden),
    /// or its text box is open.
    pub fn pl_focus(&self) -> bool {
        let s = &self.state;
        if s.playlist.edit.is_some() {
            return true;
        }
        let pos = |w: Win| s.z.iter().position(|&x| x == w);
        match (pos(Win::Playlist), pos(Win::PianoRoll)) {
            (Some(p), Some(r)) => p > r,
            (Some(_), None) => true,
            _ => false,
        }
    }

    /// Song playback from `tick` (Song mode; spec: ui/playlist.md §7).
    pub fn play_song_from(&mut self, tick: u32) {
        self.state.song_mode = true;
        if self.state.project.arrangement().is_none_or(|a| a.clips.is_empty()) {
            // Nothing plays in SONG mode, so a pattern that was playing stops too.
            if self.transport.playing() {
                self.transport.stop();
            }
            self.state.message = Some("The arranger is empty: nothing to play as a song".into());
            return self.invalidate();
        }
        // The initial values first (spec: ui/settings-choices.md "Advanced: Initialize controls automatically").
        crate::init::apply(&mut self.state);
        let mut p = self.state.project.clone();
        p.tempo = Some(self.state.tempo);
        self.transport.stop();
        self.transport.set_loop(self.state.playlist.range);
        self.transport.play(&p, PlayMode::Song, f64::from(tick));
        self.invalidate();
    }

    /// Clip menu "Preview": play just this clip once (spec: ui/playlist.md §10).
    pub fn preview_clip(&self, c: &os_model::Clip) {
        let mut p = self.state.project.clone();
        p.tempo = Some(self.state.tempo);
        let mut clip = *c;
        clip.position = 0;
        clip.track = 0;
        clip.muted = false;
        p.arrangements = vec![os_model::Arrangement { clips: vec![clip], ..Default::default() }];
        self.transport.preview_song(p);
    }

    /// A file dropped on the window: audio onto the playlist becomes an audio clip; a project opens.
    pub fn drop_file(&mut self, path: PathBuf) {
        let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if self.modal_open() {
            return self.dialog_drop(path);
        }
        // A drop accepts an Info box or a browser folder cell being edited, as a click does.
        crate::project_info::accept_info(&mut self.state);
        crate::search_folders::commit(&mut self.state);
        if crate::project_files::is_project(&path) || ext == "zip" {
            return self.ask_discard("opening another project", crate::dialog::Next::Open(path));
        }
        // A MIDI file is imported as FILE ▸ Import ▸ MIDI file does (spec: ui/midi-files.md §10.6).
        if ext == "mid" || ext == "midi" {
            return self.midi_file_chosen(path, false);
        }
        // A channel state adds its channel (spec: ui/plugin-window.md "Channel states").
        if crate::channel_states::is_state_file(&path) {
            self.state.message = crate::channel_states::add(self, &path);
            return self.invalidate();
        }
        // A preset goes into the plugin under the mouse, or a new instance of its plugin (spec: plugins/presets.md).
        if crate::presets::is_preset_file(&path) {
            self.state.message = crate::presets::dropped(self, &path);
            return self.invalidate();
        }
        if path.is_dir() {
            return self.add_browser_folder(path);
        }
        if ext == "flstheme" || ext == "txt" && path.file_name().is_some_and(|n| n == "theme.txt") {
            self.load_theme_file(&path);
            return;
        }
        let (x, y) = self.mouse;
        if self.top_hit(x, y).and_then(|h| h.win) == Some(Win::ChannelRack) {
            self.sample_to_rack(path.to_string_lossy().into_owned(), y);
            return;
        }
        let Some(grid) = self.pl_grid.filter(|g| g.contains(x, y)) else {
            self.state.message = Some("Drop audio files on the arranger to add audio clips".into());
            return self.invalidate();
        };
        let s = &mut self.state;
        let g = crate::playlist::PGeom::new(s, grid);
        let track = g.track_at(y);
        let tick = crate::playlist::snap_place(s, &g, g.tick_at(x), self.alt);
        let name = path.to_string_lossy().into_owned();
        s.message = Some(match crate::playlist::add_audio_clip(s, &name, track, tick) {
            Some(_) => format!("Added audio clip {}", path.display()),
            None => format!("Could not read {} as audio", path.display()),
        });
        self.invalidate();
    }

    /// A sample dropped on the channel rack at window y `y` (spec: ui/channel-rack.md, New project / drops): onto a
    /// sampler row it replaces that channel's sample (Lemon Sampler loads it) and renames the channel after the file
    /// while "Name channels after their sample or plugin" is on (`rack::auto_name`); elsewhere it adds a Lemon Sampler channel named after the
    /// file. Either channel reads the file's root note and beats (`rack::read_tags`). One history step, "Load channel
    /// sample(s)" (`history::load_sample`).
    pub(crate) fn sample_to_rack(&mut self, path: String, y: f32) {
        let s = &self.state;
        let row = views::rack::row_at(s, y).filter(|&r| s.project.channels[r].kind.is_sampler());
        self.sample_to_channel(path, row);
    }

    /// Sample `path` into sampler row `row`, or into a new Lemon Sampler channel at the bottom of the rack (`None`), named
    /// after the file; that channel is selected.
    pub(crate) fn sample_to_channel(&mut self, path: String, row: Option<usize>) {
        let s = &mut self.state;
        let stem = std::path::Path::new(&path).file_stem().map(|n| n.to_string_lossy().into_owned());
        // With Copy used samples to the project folder: the sample's copy in the data folder (`data_folder::adopt`).
        let path = crate::data_folder::adopt(s, &path);
        let row = match row {
            Some(r) => {
                crate::history::load_sample(s, r);
                s.sample_loads.push((r, path.clone()));
                crate::rack::auto_name(&mut s.project.channels[r], std::path::Path::new(&path));
                r
            }
            None => {
                // The step covers the channel itself: undone, it goes.
                crate::history::load_sample(s, s.project.channels.len());
                let r = add_channel(&mut s.project);
                s.project.channels[r].name = stem.clone();
                r
            }
        };
        let ch = &mut s.project.channels[row];
        crate::rack::read_tags(ch, std::path::Path::new(&path));
        ch.sample_path = Some(path);
        s.selected_channel = row;
        s.rack.select_only(row);
        s.piano.dirty = true;
        s.message = Some(format!("{} loaded in Lemon Sampler", stem.unwrap_or_default()));
        self.invalidate();
    }

    /// Sample `path` in a new Lemon Slicer channel at the bottom of the rack (the file's Fruity Slicer), named after the
    /// file and selected: sliced at the file's cue points (Tesla's regions), else auto-sliced, as Lemon Slicer's own
    /// Load does (spec: plugins/tesla/README.md "Sending slices to Lemon Slicer"). One undo step, "Add channel", as the
    /// plugin picker's.
    pub(crate) fn sample_to_slicer(&mut self, path: String) {
        let stem = std::path::Path::new(&path).file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let path = crate::data_folder::adopt(&mut self.state, &path);
        let st = match lemon_slicer::file_state(&path, lemon_slicer::state::State::default()) {
            Ok((st, _)) => st,
            Err(e) => {
                self.state.message = Some(format!("{}: {e}", crate::slicer::LEMON_SLICER));
                return self.invalidate();
            }
        };
        let n = st.slices.len();
        crate::control::edit_app(self, crate::history::ADD_CHANNEL, crate::control::command::Region::Project, |app| {
            let s = &mut app.state;
            let row = add_plugin_channel(&mut s.project, crate::slicer::LEMON_SLICER);
            let ch = &mut s.project.channels[row];
            ch.name = Some(stem.clone());
            ch.plugin_state = Some(st.to_bytes());
            s.selected_channel = row;
            s.rack.select_only(row);
            crate::rack::autosize(s);
            s.piano.dirty = true;
        });
        self.state.message = Some(format!("{stem} loaded in {} ({n} slices)", crate::slicer::LEMON_SLICER));
        self.sync_slicer();
        self.invalidate();
    }

    /// A sample a Lemon plugin sent (Tesla's "Send to arranger as audio clip" / "Send to selected channel" / "Send to
    /// new Lemon Slicer channel", host link `Audio`; spec: plugins/tesla/README.md "Version 1", "Sending slices to
    /// Lemon Slicer"): an audio clip at the song position on the first track free over it (one "Arranger edit" step),
    /// the selected channel's sample (a new sampler channel when the selected one isn't a sampler), as a drop on the
    /// rack does, or a new Lemon Slicer channel.
    pub(crate) fn sample_from_plugin(&mut self, path: String, to: os_hostlink::AudioTo) {
        match to {
            os_hostlink::AudioTo::Channel => {
                let s = &self.state;
                let row = Some(s.selected_channel).filter(|&r| s.project.channels.get(r).is_some_and(|c| c.kind.is_sampler()));
                return self.sample_to_channel(path, row);
            }
            os_hostlink::AudioTo::Slicer => return self.sample_to_slicer(path),
            os_hostlink::AudioTo::Playlist => {}
        }
        let s = &mut self.state;
        let start = s.playlist.pos;
        let secs = s.playlist.wave(&path).map_or(0.0, |w| w.secs());
        let p = &s.project;
        let tempo = p.tick_tempo(p.tempo.unwrap_or(140.0).max(1.0));
        let len = (secs * tempo / 60.0 * f64::from(p.ppq.max(1))).ceil() as u32;
        let track = crate::playlist::free_track(s, 0, start, start + len.max(1));
        let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        s.message = Some(match crate::playlist::add_audio_clip(s, &path, track, start) {
            Some(_) => format!("Added audio clip {name}"),
            None => format!("Could not read {name} as audio"),
        });
        s.piano.dirty = true;
        self.invalidate();
    }

    /// The mouse pointer's screen position (physical px).
    #[cfg(any(windows, target_os = "macos"))]
    fn pointer_on_screen() -> Option<(i32, i32)> {
        #[cfg(windows)]
        {
            use windows_sys::Win32::Foundation::POINT;
            let mut pt = POINT { x: 0, y: 0 };
            (unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut pt) } != 0).then_some((pt.x, pt.y))
        }
        #[cfg(target_os = "macos")]
        crate::plugin_window::pointer()
    }

    /// The hosted key whose plugin window is under the mouse pointer (docked or detached).
    #[cfg(any(windows, target_os = "macos"))]
    pub(crate) fn plugin_window_under_cursor(&self) -> Option<usize> {
        let (x, y) = Self::pointer_on_screen()?;
        self.plugins.row_at_screen(x, y)
    }

    /// A browser sample released over a hosted plugin window (it sits above the main window, which keeps the mouse
    /// capture during the drag): the plugin gets the file as if it came from Explorer (on macOS only Lemon Sampler takes
    /// it, through its state). Returns whether it was one.
    #[cfg(any(windows, target_os = "macos"))]
    pub(crate) fn sample_to_plugin_window(&mut self, path: &str) -> bool {
        let Some((x, y)) = Self::pointer_on_screen() else { return false };
        let Some(row) = self.plugins.row_at_screen(x, y) else { return false };
        let sampler = self.state.project.channels.get(row).is_some_and(|c| c.kind.is_sampler());
        let taken = self.plugins.drop_file(row, x, y, path, sampler);
        self.sample_taken(row, path, sampler, taken);
        true
    }

    /// Sample `path` went into row `row`'s plugin (`taken`) or not: a sampler channel takes its name, tags and path.
    fn sample_taken(&mut self, row: usize, path: &str, sampler: bool, taken: bool) {
        let name = std::path::Path::new(path).file_stem().map(|n| n.to_string_lossy().into_owned());
        if taken {
            if sampler && row < self.state.project.channels.len() {
                crate::history::load_sample(&mut self.state, row);
            }
            if let Some(ch) = self.state.project.channels.get_mut(row).filter(|_| sampler) {
                crate::rack::auto_name(ch, std::path::Path::new(path));
                crate::rack::read_tags(ch, std::path::Path::new(path));
                ch.sample_path = Some(path.to_string());
                // The plugin loads the file itself: it takes what the load read alongside.
                let set = crate::plugins::tag_params(ch);
                self.plugins.set_sampler(row, None, &set);
            }
            self.state.message = Some(format!("{} loaded in the plugin", name.unwrap_or_default()));
        } else {
            self.state.message = Some("This plugin does not take dropped files".into());
        }
        self.invalidate();
    }

    /// A file dropped on a plugin window's frame (its title bar or strip, or an editor that takes no files; spec:
    /// plugins/presets.md "Drag and drop"): a preset loads into the plugin, a sample into a sampler channel's.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn file_on_plugin_window(&mut self, row: usize, path: PathBuf) {
        if crate::presets::is_preset_file(&path) {
            self.state.message = crate::presets::load_into(self, row, &path);
            return self.invalidate();
        }
        let sampler = self.state.project.channels.get(row).is_some_and(|c| c.kind.is_sampler());
        let p = path.to_string_lossy().into_owned();
        let taken = sampler && self.plugins.load_sample(row, &p);
        self.sample_taken(row, &p, sampler, taken);
    }

    /// Load a theme: a `.flstheme` (spec ui/flstheme.md) or a Lemon Studio `theme.txt`. A `.flstheme` is also
    /// remembered as `current.flstheme` in the user folder so the next start uses it.
    pub fn load_theme_file(&mut self, path: &std::path::Path) {
        self.load_theme_file_ex(path, true)
    }

    /// `load_theme_file`; `remember` = keep it for the next start. A `.flstheme` colours the skin in use; a `theme.txt`
    /// sets Citrus's tokens, so it switches to Citrus.
    pub fn load_theme_file_ex(&mut self, path: &std::path::Path, remember: bool) {
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let fl = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("flstheme"));
        let loaded = if fl {
            std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).map_err(|e| e.to_string()).map(|text| {
                // A picture named relative to the theme file stays found once the theme is kept elsewhere
                // (`wallpaper::absolute_names`).
                let dir = std::path::absolute(path).ok().and_then(|p| p.parent().map(std::path::Path::to_path_buf));
                let text = match dir {
                    Some(d) => crate::wallpaper::absolute_names(&text, &d),
                    None => text,
                };
                let t = skin_theme(self.design(), &name, Some(&text));
                (t, Some(text))
            })
        } else {
            theme_from_file(path).map(|t| (t, None))
        };
        match loaded {
            Ok((t, text)) => {
                self.theme_mark(crate::theme_library::Step::Other);
                let to_citrus = !fl && self.design() != Design::Citrus;
                self.theme = t;
                // Theme ▸ Options read and rewrite the applied .flstheme (spec: ui/settings-choices.md "Light mode and
                // clip note colours").
                self.state.theme_base = text.clone();
                self.state.fl_text = text;
                os_theme::set_current(&self.theme.colors);
                crate::wallpaper::forget();
                if remember && to_citrus {
                    save_design(Design::Citrus);
                }
                // Kept as the text taken in: its picture names made absolute.
                if remember && fl && let (Some(d), Some(text)) = (os_paths::user_dir(), &self.state.fl_text) {
                    let _ = std::fs::create_dir_all(&d);
                    let keep = d.join("current.flstheme");
                    if keep.as_path() != path {
                        let _ = std::fs::write(keep, text);
                    }
                }
                self.sync_preset();
                self.state.message = Some(if to_citrus { format!("Theme: {name} (Citrus skin)") } else { format!("Theme: {name}") });
            }
            Err(e) => self.state.message = Some(format!("Could not load theme {name}: {e}")),
        }
        self.invalidate();
    }

    /// The design in use (spec: ui/lemon-design.md).
    pub fn design(&self) -> Design {
        self.theme.colors.style.design
    }

    /// Switch to skin `d`, keeping the applied theme (a `.flstheme` colours either skin; with none, Citrus may load the
    /// saved `theme.txt`). `remember` = keep the choice for the next start (`design.txt` in the user folder).
    pub fn set_design(&mut self, d: Design, remember: bool) {
        self.theme = match &self.state.fl_text {
            Some(text) => skin_theme(d, &self.theme.name.clone(), Some(text)),
            None => design_theme(d, remember),
        };
        os_theme::set_current(&self.theme.colors);
        if remember {
            save_design(d);
        }
        self.sync_preset();
        self.state.message = Some(format!("Skin: {}", d.name()));
        self.invalidate();
    }

    /// Theme ▸ Options: flips "Audio and automation clips use note colors" (`OverrideClips`) in the applied theme, the
    /// stock Dark theme's text when no `.flstheme` is applied, and keeps it as `current.flstheme` for the next start
    /// (spec: ui/settings-choices.md "Light mode and clip note colours").
    pub(crate) fn toggle_theme_option(&mut self, label: &str) {
        self.theme_mark(crate::theme_library::Step::Other);
        let on = !self.state.fl_theme().override_clips;
        let text = self.state.fl_text.as_deref().unwrap_or(os_theme::fl::DARK_TEXT);
        let text = os_theme::fl::set_key(text, "OverrideClips", if on { "1" } else { "0" });
        let palette = os_theme::design::palette(self.design(), Some(&os_theme::FlTheme::parse(&text)));
        self.theme = os_theme::Theme::new(&self.theme.name.clone(), palette, self.theme.adjust);
        os_theme::set_current(&self.theme.colors);
        if let Some(d) = theme_dir() {
            let _ = std::fs::create_dir_all(&d);
            let _ = std::fs::write(d.join("current.flstheme"), &text);
        }
        self.state.fl_text = Some(text);
        self.sync_preset();
        self.state.message = Some(format!("{label}: {}", if on { "on" } else { "off" }));
        self.invalidate();
    }

    /// The Themes tile shown as chosen (spec: ui/theme-presets.md "Settings"): Default when no theme file colours the
    /// skin (or the stock Dark theme does, whatever its options), else the tile whose theme has the applied colours; none
    /// when the theme came from elsewhere (Load theme..., a drop, a `theme.txt`). While the window is open the tiles'
    /// previews are redrawn for the skin and theme in use.
    pub(crate) fn sync_preset(&mut self) {
        let applied = self.state.fl_text.as_deref().map(os_theme::FlTheme::parse);
        // The tile the theme was chosen from stays framed while Adjustments change it (`State::theme_base`).
        let chosen = self.state.theme_base.as_deref().map(os_theme::FlTheme::parse);
        let design = self.design();
        let st = &mut self.state.settings;
        let themes: Vec<_> = st.themes.iter().map(|(_, t)| os_theme::FlTheme::parse(t)).collect();
        // With no theme file the skin shows its own colours: its own tile (the Lemon or Citrus theme, listed first in
        // `Design::ALL` order); so does stock Dark without Lemon Studio's key.
        let own = Design::ALL.iter().position(|d| *d == design).unwrap_or(0);
        st.preset = match (&chosen, &applied) {
            (None, None) if design == Design::Lemon || self.theme.colors == os_theme::Palette::default() => own,
            (None, None) => usize::MAX,
            (None, Some(_)) => own,
            (Some(th), _) => themes.iter().position(|t| t.same_colours(th)).or(th.is_stock().then_some(own)).unwrap_or(usize::MAX),
        };
        st.skin = own;
        if st.open {
            st.skin_previews = Design::ALL.iter().map(|d| os_theme::design::palette(*d, applied.as_ref())).collect();
            st.theme_previews = themes.iter().map(|th| os_theme::design::palette(design, Some(th))).collect();
        }
        self.check_readable();
    }

    /// Open the settings window on `tab` (spec: ui/settings.md §1); reads the output devices and theme files.
    pub fn open_settings(&mut self, tab: usize) {
        let st = &mut self.state.settings;
        if st.tab != tab {
            st.scroll = 0.0;
        }
        st.open = true;
        st.picker = None;
        st.tab = tab;
        self.audio_page();
        if tab == views::settings::MIDI {
            self.midi_page_opened();
        }
        // General ▸ System's association check shows what Windows says now (`crate::file_assoc`).
        #[cfg(windows)]
        self.assoc_read();
        // The Lemon and Citrus themes, the built-in themes, then the user's own (`crate::theme_library`).
        self.state.settings.name_edit = None;
        self.theme_list();
        self.sync_preset();
        self.invalidate();
    }

    /// Fills the Audio page: the output devices, the audio settings and the status (spec: ui/settings.md "Audio").
    fn audio_page(&mut self) {
        let cfg = crate::audio_cfg::AudioCfg::current();
        let (devices, default) = crate::audio_cfg::devices(&cfg);
        let drivers = crate::device::asio_drivers();
        let st = &mut self.state.settings;
        let listed = if cfg.asio_driver().is_some() { &drivers } else { &devices };
        st.device = match &cfg.device {
            Some(d) if listed.contains(d) => d.clone(),
            Some(d) => format!("{d} (not found: default device)"),
            None => format!("Default ({})", default.as_deref().unwrap_or("no output device")),
        };
        st.devices = devices;
        st.asio_drivers = drivers;
        let (inputs, default_input) = if cfg.asio_driver().is_some() { (Vec::new(), None) } else { crate::audio_cfg::input_devices(&cfg) };
        st.input = match &cfg.input {
            Some(d) if inputs.contains(d) => d.clone(),
            Some(d) => format!("{d} (not found: default device)"),
            None => format!("Default ({})", default_input.as_deref().unwrap_or("no input device")),
        };
        st.inputs = inputs;
        // What the Windows device offers for short buffers (spec: audio/wasapi-low-latency.md 4.3).
        st.periods = if cfg.asio_driver().is_some() { None } else { crate::device::periods(cfg.device.as_deref()) };
        st.audio = cfg;
        st.rate = Transport::device_rate();
        self.audio_status();
    }

    /// The Audio page's status lines from the stream that is open; returns whether they changed.
    fn audio_status(&mut self) -> bool {
        use crate::device::{ASIO, WASAPI_EXCLUSIVE};
        let tp = &self.transport;
        let opened = tp.opened();
        let asio_open = opened.as_ref().is_some_and(|o| o.host == ASIO);
        let instead = opened.as_ref().and_then(|o| o.instead.clone());
        let chosen = self.state.settings.audio.asio_driver().map(str::to_string);
        let driver = chosen.and_then(|name| crate::device::last().filter(|i| i.host == ASIO && i.name == name));
        let first = match opened {
            // An ASIO driver's blocks are its buffer (spec: audio/devices.md, "Status lines"), as are those of a device in
            // exclusive mode (audio/wasapi-low-latency.md 4.6).
            Some(o) if o.host == ASIO || o.host == WASAPI_EXCLUSIVE => {
                let blocks = o.fixed.map_or(String::new(), |b| format!(", blocks of {}", views::settings::buffer_text(b, o.rate)));
                format!("Status: {} open at {} Hz{blocks} ({})", o.name, o.rate, o.host)
            }
            Some(o) => {
                let rate = o.rate;
                let block = match tp.block() {
                    0 => String::new(),
                    b => format!(", blocks of {}", views::settings::buffer_text(b, rate)),
                };
                let buffer = o.fixed.map_or(String::new(), |b| format!(", buffer {}", views::settings::buffer_text(b, rate)));
                format!("Status: {} open at {rate} Hz{block}{buffer} ({})", o.name, o.host)
            }
            None => {
                let cfg = &self.state.settings.audio;
                let host = if cfg.asio_driver().is_some() {
                    ASIO
                } else if cfg!(windows) && cfg.exclusive() {
                    WASAPI_EXCLUSIVE
                } else {
                    crate::audio_cfg::host_name(cfg)
                };
                format!("Status: opens with the first note or Play ({host})")
            }
        };
        let second = match tp.underruns() {
            0 => "No dropouts so far.".to_string(),
            n => format!("Dropouts so far: {n} (device blocks the engine could not fill in time; a longer buffer helps)"),
        };
        let st = &mut self.state.settings;
        let changed = st.status[0] != first || st.status[1] != second || (&st.driver, st.asio_open, &st.instead) != (&driver, asio_open, &instead);
        st.status = [first, second];
        (st.driver, st.asio_open, st.instead) = (driver, asio_open, instead);
        changed
    }

    /// The menu of an Audio page combo, under its box (`x`, `y`).
    fn audio_menu(&mut self, label: &str, x: f32, y: f32) {
        use crate::audio_cfg::{Set, BUFFERS, RATES, SMALLEST};
        use crate::playlist::{Cmd, Mark, PItem, Popup};
        use views::settings::{ANIMATIONS, BUFFER_LENGTH, INPUT_DEVICE, MODE, MODE_EXCLUSIVE, MODE_SHARED, OUTPUT_DEVICE, SAMPLE_RATE};
        let still = !self.state.cables.borrow().animations();
        let st = &self.state.settings;
        let cfg = &st.audio;
        let rate = st.rate.unwrap_or(44100);
        let items: Vec<PItem> = match label {
            // Project ▸ Time settings (spec: ui/settings-choices.md "Project"): the timebase, FL's list.
            views::settings::TIMEBASE => {
                let now = self.state.project.ppq;
                views::settings::TIMEBASES.iter().map(|&v| PItem::new(v.to_string(), Cmd::Timebase(v)).mark(Mark::Radio(v == now))).collect()
            }
            // The "Set as" mode (event 35), in FL's order.
            views::settings::SET_AS => {
                let sig = self.state.project.bar_is_signature();
                vec![
                    PItem::new(views::settings::SET_AS_DIVISION, Cmd::TimeMode(false)).mark(Mark::Radio(!sig)),
                    PItem::new(views::settings::SET_AS_SIGNATURE, Cmd::TimeMode(true)).mark(Mark::Radio(sig)),
                ]
            }
            // Project ▸ Advanced ▸ Panning law (event 23), FL's two choices.
            views::settings::PANNING_LAW => {
                let now = self.state.project.panning_law.unwrap_or(0);
                views::settings::PANNING_LAWS.iter().map(|&(name, v)| PItem::new(name, Cmd::PanningLaw(v)).mark(Mark::Radio(v == now))).collect()
            }
            // One of the time signature's two numbers: a numerator is the beats per bar (the steps per beat stay), a
            // denominator d makes 16 / d steps per beat (events 17, 18).
            views::settings::NUMERATOR | views::settings::DENOMINATOR => {
                let now = self.state.project.bar();
                let numerator = label == views::settings::NUMERATOR;
                let choices = if numerator { views::settings::NUMERATORS } else { views::settings::DENOMINATORS };
                choices
                    .iter()
                    .map(|&v| {
                        let bar = if numerator { (v, now.1) } else { (now.0, 16 / v.max(1)) };
                        PItem::new(v.to_string(), Cmd::Bar(bar.0, bar.1)).mark(Mark::Radio(bar == now))
                    })
                    .collect()
            }
            // The Windows devices, then the ASIO drivers, each under its heading (spec: audio/devices.md, "The Output
            // device menu"). The ASIO heading has the ® its first use needs (the ASIO SDK's usage guidelines §15).
            OUTPUT_DEVICE => {
                let asio = cfg.asio_driver().is_some();
                let on = |d: &String, driver: bool| Mark::Radio(asio == driver && cfg.device.as_ref() == Some(d));
                cfg!(windows)
                    .then(|| PItem::head("Windows devices"))
                    .into_iter()
                    .chain(std::iter::once(PItem::new("Default device", Cmd::Audio(Set::Device(None))).mark(Mark::Radio(cfg.device.is_none()))))
                    .chain(st.devices.iter().map(|d| PItem::new(d.clone(), Cmd::Audio(Set::Device(Some(d.clone())))).mark(on(d, false))))
                    .chain((!st.asio_drivers.is_empty()).then(|| PItem::head("ASIO® devices")))
                    .chain(st.asio_drivers.iter().map(|d| PItem::new(d.clone(), Cmd::Audio(Set::Asio(d.clone()))).mark(on(d, true))))
                    .collect()
            }
            // The system's default input (followed when it changes), then each input device.
            INPUT_DEVICE => std::iter::once(PItem::new("Default device", Cmd::Audio(Set::Input(None))).mark(Mark::Radio(cfg.input.is_none())))
                .chain(st.inputs.iter().map(|d| PItem::new(d.clone(), Cmd::Audio(Set::Input(Some(d.clone())))).mark(Mark::Radio(cfg.input.as_ref() == Some(d)))))
                .collect(),
            // Windows devices: shared or exclusive mode (spec: audio/wasapi-low-latency.md 4.2). Linux: the sound server,
            // or an ALSA device directly.
            MODE => [
                PItem::new(MODE_SHARED, Cmd::Audio(Set::Mode(false))).mark(Mark::Radio(!cfg.exclusive && !cfg.jack)),
                PItem::new(MODE_EXCLUSIVE, Cmd::Audio(Set::Mode(true))).mark(Mark::Radio(cfg.exclusive && !cfg.jack)),
            ]
            .into_iter()
            // Linux: as a client of the JACK server (spec: audio/linux-output.md "JACK").
            .chain(cfg!(target_os = "linux").then(|| PItem::new(views::settings::MODE_JACK, Cmd::Audio(Set::Jack)).mark(Mark::Radio(cfg.jack))))
            .collect(),
            // Under an ASIO driver: the driver's own, then what it takes, once it has opened (spec: audio/devices.md,
            // "Sample rate and buffer length under ASIO"). In exclusive mode: the rates the device runs at itself
            // (audio/wasapi-low-latency.md 4.4).
            SAMPLE_RATE => {
                let exclusive_rates = st.periods.as_ref().map(|p| p.exclusive_rates.clone()).filter(|r| cfg.exclusive() && !r.is_empty());
                let (own, rates) = match (cfg.asio_driver(), &st.driver) {
                    (Some(_), Some(d)) => (format!("Driver ({rate})"), d.rates.clone()),
                    (Some(_), None) => (format!("Driver ({rate})"), RATES.to_vec()),
                    (None, _) => ("Device rate".to_string(), exclusive_rates.unwrap_or_else(|| RATES.to_vec())),
                };
                std::iter::once(PItem::new(own, Cmd::Audio(Set::Rate(None))).mark(Mark::Radio(cfg.rate.is_none())))
                    .chain(rates.into_iter().map(|r| PItem::new(format!("{r} Hz"), Cmd::Audio(Set::Rate(Some(r)))).mark(Mark::Radio(cfg.rate == Some(r)))))
                    .collect()
            }
            // A Windows device: its own period, in exclusive mode its smallest, then the lengths that change something
            // in the mode (spec: audio/wasapi-low-latency.md 4.3).
            BUFFER_LENGTH => {
                let text = |b| views::settings::buffer_text(b, rate);
                let mut smallest = None;
                let (own, sizes) = match (cfg.asio_driver(), &st.driver, &st.periods) {
                    (Some(_), Some(d), _) => match d.fixed {
                        Some(b) => (format!("Driver ({})", views::settings::buffer_text(b, d.rate)), d.sizes.clone()),
                        None => ("Driver".to_string(), d.sizes.clone()),
                    },
                    (Some(_), None, _) => ("Driver".to_string(), Vec::new()),
                    (None, _, Some(p)) => {
                        let x = cfg.exclusive();
                        if x {
                            let item = PItem::new(format!("Smallest ({})", text(p.exclusive_frames(rate).1)), Cmd::Audio(Set::Buffer(Some(SMALLEST))));
                            smallest = Some(item.mark(Mark::Radio(cfg.buffer == Some(SMALLEST))));
                        }
                        (format!("Device ({})", text(p.own(x, rate))), p.sizes(x, rate))
                    }
                    (None, _, None) => ("Device default".to_string(), BUFFERS.to_vec()),
                };
                std::iter::once(PItem::new(own, Cmd::Audio(Set::Buffer(None))).mark(Mark::Radio(cfg.buffer.is_none())))
                    .chain(smallest)
                    .chain(sizes.into_iter().map(|b| {
                        PItem::new(views::settings::buffer_text(b, rate), Cmd::Audio(Set::Buffer(Some(b)))).mark(Mark::Radio(cfg.buffer == Some(b)))
                    }))
                    .collect()
            }
            ANIMATIONS => {
                let item = |label: &'static str, on: bool| PItem::new(label, Cmd::Mixer(crate::mixer::MCmd::Animations(on))).mark(Mark::Radio(still != on));
                vec![item("Playful", true), item("Off", false)]
            }
            _ => return,
        };
        self.state.playlist.popup = Some(Popup { x, y: y + 1.0, items, sub: None, scroll: [0; 2] });
        self.invalidate();
    }

    /// Changes one audio setting: kept for the next start, hosted plugins move to the new rate, and the open streams
    /// move to the new output (spec: ui/settings.md "Audio").
    pub fn set_audio(&mut self, s: crate::audio_cfg::Set) {
        let input = matches!(s, crate::audio_cfg::Set::Input(_));
        let cfg = crate::audio_cfg::AudioCfg::current().with(s);
        cfg.apply();
        crate::device::system_input::forget();
        if input {
            // The output plays on; the mixer's input stream moves to the new device (`OutputDevice::sync_inputs`).
            self.audio_page();
            self.state.message = Some(format!("Audio input: {}", self.state.settings.input));
            return self.invalidate();
        }
        if let Some(rate) = Transport::device_rate() {
            self.plugins.set_rate(rate);
        }
        self.transport.reopen();
        self.audio_page();
        let st = &self.state.settings;
        self.state.message = Some(format!("Audio: {}, {} Hz", st.device, st.rate.unwrap_or(0)));
        self.invalidate();
    }

    /// Theme import (spec: ui/flstheme.md): pick a .flstheme, starting in FL's theme folder when installed.
    fn theme_dialog(&mut self) {
        let fl = std::path::Path::new(r"C:\Program Files\Image-Line\FL Studio 2026\Artwork\Themes");
        let dlg = crate::filedlg::FileDlg::new(crate::filedlg::Mode::Open, "Load theme", "theme", crate::dialog::Then::LoadTheme)
            .filter("Theme", &["flstheme", "txt"])
            .start(Some(fl));
        self.file_dialog(dlg);
    }

    /// Settings > Project ▸ Time settings: the project's bar, beats per bar and steps per beat (events 17, 18; spec:
    /// ui/settings-choices.md "Project", audio/song-structure.md "Time signatures and a pattern's length"), from the
    /// Numerator / Denominator combos or a time division's value boxes. Playback follows at once (pattern lengths, the
    /// metronome's accents, what hosted plugins are told), as do the pattern lengths the editors draw; the project
    /// counts as changed.
    pub fn set_bar(&mut self, beats: u16, steps: u16) -> Option<String> {
        crate::control::edit_app(self, crate::control::command::TIME_SIGNATURE, crate::control::command::Region::Settings, |app| app.set_bar_now(beats, steps))
    }

    fn set_bar_now(&mut self, beats: u16, steps: u16) -> Option<String> {
        let bar = (beats.max(1), steps.max(1));
        if self.state.project.bar() != bar {
            self.state.project.bar = Some(bar);
            self.state.piano.dirty = true;
        }
        self.invalidate();
        Some(bar_text(&self.state.project))
    }

    /// Settings > Project ▸ Set as time signature / time division (event 35; spec: ui/settings-choices.md "Set as time
    /// division"). Each mode keeps its own last bar while the program runs (4/4 at first): switching keeps the bar of
    /// the mode left and writes the other mode's into the project. A bar is never converted between the modes.
    pub fn set_time_mode(&mut self, signature: bool) -> Option<String> {
        crate::control::edit_app(self, crate::control::command::TIME_SIGNATURE, crate::control::command::Region::Settings, |app| app.set_time_mode_now(signature))
    }

    fn set_time_mode_now(&mut self, signature: bool) -> Option<String> {
        let s = &mut self.state;
        let now = s.project.bar_is_signature();
        if now != signature {
            let memory = &mut s.settings.bar_memory;
            memory[usize::from(now)] = Some(s.project.bar());
            s.project.bar = Some(memory[usize::from(signature)].unwrap_or(Project::BAR_DEFAULT));
            s.project.bar_as_signature = Some(signature);
            s.piano.dirty = true;
        }
        self.invalidate();
        let mode = if signature { views::settings::SET_AS_SIGNATURE } else { views::settings::SET_AS_DIVISION };
        Some(format!("{mode}: {}", bar_text(&self.state.project)))
    }

    /// Settings > Project ▸ a time division's value box (`label`, `settings::LIVE_NUMBERS`) moved by `delta` (the
    /// mouse wheel). It stays in its range; a file's value outside it can only move toward it.
    fn settings_number(&mut self, label: &str, delta: i32) -> Option<String> {
        // The value boxes declared in `crate::prefs` (General ▸ Undo history ▸ Maximum undo levels, File ▸ Backup ▸
        // Maximum).
        if let Some(n) = crate::prefs::find_number(label) {
            // Audio ▸ Browser preview track and Metronome track offer "---" and the open project's insert tracks; one
            // past them comes back in range (spec: ui/settings-choices.md "Mixer: Browser preview track").
            let track = n == &crate::prefs::BROWSER_PREVIEW_TRACK || n == &crate::prefs::METRONOME_TRACK;
            let max = if track { n.max.min(crate::mixer::inserts(&self.state)) } else { n.max };
            let v = (i32::from(crate::prefs::number(n)) + delta).clamp(i32::from(n.min), i32::from(max.max(n.min))) as u16;
            crate::prefs::set_number(n, v);
            if n == &crate::prefs::METRONOME_TRACK {
                // The playing engines hear it from their next tick.
                crate::metronome::switch().set_track(v);
            }
            crate::history::trim(&mut self.state);
            self.invalidate();
            return Some(format!("{}: {}", n.label, crate::prefs::number_text(n)));
        }
        let (lo, hi) = views::settings::number_range(label)?;
        let (beats, steps) = self.state.project.bar();
        let beat_box = label == views::settings::BEATS_PER_BAR;
        let now = if beat_box { beats } else { steps };
        let v = (i32::from(now) + delta).clamp(i32::from(lo.min(now)), i32::from(hi.max(now))) as u16;
        // Wheel notches in a row make one undo step.
        let bar = if beat_box { (v, steps) } else { (beats, v) };
        crate::control::edit_run(self, crate::control::command::TIME_SIGNATURE, crate::control::command::Region::Settings, crate::history::Run::Bar, |app| app.set_bar_now(bar.0, bar.1))
    }

    /// Settings > Project ▸ Timebase (PPQ): the project's ticks per quarter (the file header's PPQ). Every tick count
    /// of the project is rescaled at once and nothing is asked (spec: ui/settings-choices.md "Project"; the rounding is
    /// `os_model::rescale_tick`'s). Playback stops: its position is in ticks of the old timebase. The editors' positions,
    /// selections, clipboards and undo steps are rescaled with the project, so no undo brings back ticks of the old
    /// timebase; choosing the old timebase again rescales back (exactly, unless ticks were rounded on the way).
    pub fn set_timebase(&mut self, ppq: u16) -> Option<String> {
        let (from, to) = (self.state.project.ppq.max(1), ppq.max(1));
        if from != to {
            self.transport.stop();
            // One step, "Project settings", that brings the ticks back exactly (spec: ui/settings-choices.md "Undo
            // history: the four checks").
            crate::history::timebase(&mut self.state);
            retime(&mut self.state, from, to);
        }
        self.invalidate();
        Some(format!("Timebase: {to} PPQ"))
    }

    /// Settings > Project ▸ Advanced ▸ Panning law (event 23: 0 Circular, 2 Triangular; spec: audio/gain-laws.md
    /// "Panning law: Triangular"): the law of the channel and mixer track pans. Playback follows from its next tick, as
    /// for any pan edit; the project counts as changed. A file without the event is Circular.
    pub fn set_panning_law(&mut self, law: u8) -> Option<String> {
        crate::control::edit(&mut self.state, "Panning law", crate::control::command::Region::Settings, |s| {
            if s.project.panning_law.unwrap_or(0) != law {
                s.project.panning_law = Some(law);
                s.piano.dirty = true;
            }
        });
        self.invalidate();
        Some(format!("Panning law: {}", views::settings::panning_text(Some(law))))
    }

    /// Settings > Project ▸ Advanced ▸ Play truncated notes in clips (spec: audio/song-structure.md "Play truncated
    /// notes"): switched for the project, and playback follows at once. Off when the file does not say.
    fn toggle_play_truncated(&mut self) {
        let on = !self.state.project.play_truncated;
        crate::control::edit(&mut self.state, "Play truncated notes in clips", crate::control::command::Region::Settings, |s| s.project.play_truncated = on);
        self.state.piano.dirty = true;
        self.state.message = Some(format!("Play truncated notes in clips: {}", if on { "on" } else { "off" }));
    }

    /// Settings > Project ▸ Playlist ▸ Automatic crossfades: switched for the project (event 44), which counts as
    /// changed; off when the file does not say. Turned on, the crossfades are set from the overlaps (next tick,
    /// `playlist::follow_crossfades`); turned off, their lengths go to 0 (spec: audio/audio-clips.md "Automatic
    /// crossfades").
    fn toggle_auto_crossfades(&mut self) {
        let on = !self.state.project.auto_crossfades.unwrap_or(false);
        // The whole project: switching sets the clips' crossfades.
        crate::control::edit(&mut self.state, "Automatic crossfades", crate::control::command::Region::Project, |s| {
            s.project.auto_crossfades = Some(on);
            crate::playlist::crossfades_switched(s);
        });
        self.state.message = Some(format!("Automatic crossfades: {}", if on { "on" } else { "off" }));
    }

    fn settings_act(&mut self, a: views::settings::SAct) {
        use views::settings::SAct;
        // A click anywhere but the colour picker and the colour circles closes the picker.
        if !matches!(a, SAct::Swatch(..) | SAct::PickBody | SAct::PickPart(_) | SAct::PickSv | SAct::PickHue | SAct::PickRevert) {
            self.picker_close();
        }
        // A click off the name panel cancels it.
        if !matches!(a, SAct::NameBody | SAct::NameOk | SAct::NameCancel) {
            self.state.settings.name_edit = None;
        }
        let st = &mut self.state.settings;
        match a {
            SAct::Body | SAct::PickBody | SAct::PickClose | SAct::NameBody => {}
            // The name panel (`crate::theme_library`).
            SAct::NameOk => return self.name_ok(),
            SAct::NameCancel => return self.name_close(),
            SAct::Button(views::settings::SAVE_PRESET) => return self.theme_save_open(),
            SAct::Button(views::settings::THEME_UNDO) => self.state.message = Some(self.theme_undo(false)),
            SAct::Button(views::settings::THEME_REDO) => self.state.message = Some(self.theme_undo(true)),
            SAct::Button(views::settings::FIX_TEXT) => self.theme_fix_text(),
            // Theme ▸ Adjustments (spec: ui/theme-presets.md §C).
            SAct::ThemeKnob(i) => return self.theme_knob_press(usize::from(i)),
            SAct::Swatch(i, part) => {
                let r = self.hits.iter().rev().find(|h| h.action == Action::Settings(SAct::Swatch(i, part))).map(|h| h.rect);
                let (i, part) = (usize::from(i), usize::from(part));
                let at = r.map_or((self.mouse.0, self.mouse.1, 0.0), |r| {
                    // Steps is two half circles, each its own hit.
                    let cx = if i != crate::theme_adjust::STEPS { r.cx() } else if part == 0 { r.right() } else { r.x };
                    (cx, r.cy(), r.h / 2.0)
                });
                return self.picker_open(i, part, at);
            }
            SAct::PickPart(k) => return self.picker_part(usize::from(k)),
            SAct::PickSv | SAct::PickHue => {
                let r = self.hits.iter().rev().find(|h| h.action == Action::Settings(a.clone())).map(|h| h.rect);
                let Some(r) = r else { return };
                self.theme_mark(crate::theme_library::Step::Other);
                let (x, y) = self.mouse;
                if a == SAct::PickSv {
                    self.picker_sv(r, x, y);
                    self.drag = Some(Drag::PickSv(r));
                } else {
                    self.picker_hue(r, y);
                    self.drag = Some(Drag::PickHue(r));
                }
                return;
            }
            SAct::PickRevert => return self.picker_revert(),
            SAct::Close => st.open = false,
            // The program settings declared in `crate::prefs`.
            SAct::Combo(views::settings::DEFAULT_TEMPLATE, x, y) => return self.template_menu(x, y),
            SAct::Combo(crate::data_folder::COMBO, x, y) => return self.data_folder_menu(x, y),
            SAct::Combo(label, x, y) if crate::prefs::find_combo(label).is_some() => return self.pref_menu(label, x, y),
            SAct::Combo(label, x, y) => return self.audio_menu(label, x, y),
            SAct::Slider(label, x, w) => return self.pref_slider_press(label, x, w),
            SAct::Midi(m) => return self.midi_page_act(m),
            SAct::Button(views::settings::REFRESH_MIDI) => self.midi_refresh(true),
            SAct::Ppi => return self.ppi_press(),
            SAct::Tab(i) => {
                st.tab = i;
                st.scroll = 0.0;
                // Read again whenever the General page opens (spec: ui/settings-choices.md "System: Associate project
                // files with application").
                #[cfg(windows)]
                if i == views::settings::GENERAL {
                    self.assoc_read();
                }
                if i == views::settings::MIDI {
                    self.midi_page_opened();
                }
            }
            // Opt in / out of the experimental native host and scan again so the effect lists follow (spec
            // plugins/fruity-abi.md). Its displayed state comes straight from `os_native::enabled()`.
            SAct::Check(k) if k == views::settings::NATIVE_HOST_CHECK => {
                let on = !os_native::enabled();
                os_native::set_enabled(on);
                self.start_scan(crate::plugin_db::Mode::Changed);
                self.state.message = Some(if on { "Experimental native plugin host on: looking for native plugins".into() } else { "Experimental native plugin host off".into() });
                // The hidden preference applies again (or stops applying) with the host.
                if os_native::prefer_native_set() {
                    self.swap_builtin_overrides();
                }
            }
            // Real native plugins in place of the Lemon recreations (spec plugins/fruity-abi.md "Opened projects").
            SAct::Check(k) if k == views::settings::NATIVE_FIRST_CHECK => {
                let on = !os_native::prefer_native();
                os_native::set_prefer_native(on);
                let n = self.swap_builtin_overrides();
                self.state.message = Some(match (on, n) {
                    (true, 0) => "Opened projects play their own native plugins, not the Lemon recreations".into(),
                    (true, n) => format!("Opened projects play their own native plugins: {n} channels / slots switched from the Lemon recreations"),
                    (false, 0) => "Lemon recreations play their projects' plugins again".into(),
                    (false, n) => format!("Lemon recreations play their projects' plugins again: {n} channels / slots switched back"),
                });
            }
            // The plugin manager (spec: ui/plugin-manager.md §2).
            SAct::Check(k) if views::settings::plugin_check(k) => self.plugin_check(k),
            SAct::Plug(p) => self.plugin_act(p),
            SAct::Check(k) if k == crate::prefs::COPY_USED_SAMPLES.label => self.toggle_copy_samples(),
            SAct::Check(k) if crate::prefs::find_check(k).is_some() => self.pref_toggle(k),
            SAct::Check(k) if k == views::settings::TRUNCATED_CHECK => self.toggle_play_truncated(),
            SAct::Check(k) if k == views::settings::CROSSFADES_CHECK => self.toggle_auto_crossfades(),
            SAct::Check(k) if k == views::settings::SHOW_INFO_CHECK => self.toggle_show_info(),
            SAct::Check(k) if k == views::settings::NOTE_COLORS_CHECK => self.toggle_theme_option(k),
            // The only place that writes the registry (`crate::file_assoc`).
            #[cfg(windows)]
            SAct::Check(k) if k == views::settings::ASSOCIATE_CHECK => self.assoc_click(),
            // A time division's value box: the mouse wheel changes it (`wheel_act`).
            SAct::Number(label) => self.state.message = Some(format!("{label}: the mouse wheel changes it")),
            // Checks and buttons outside `settings::LIVE` are drawn greyed out with an `Action::Placeholder` hit instead.
            SAct::Check(k) => self.state.message = Some(off_text(k)),
            SAct::Group(k) => {
                if !st.folded.remove(k) {
                    st.folded.insert(k);
                }
            }
            // Skins and themes (spec: ui/theme-presets.md §B): a skin keeps the theme; the themes (the skins' own colours,
            // then the built-in and user themes) colour either skin. Reset = no theme: the skin's own colours.
            SAct::Skin(i) => {
                if let Some(d) = Design::ALL.get(i).copied() {
                    self.set_design(d, true);
                    self.state.message = Some(format!("Skin: {}", d.name()));
                }
            }
            SAct::Button("Reset") => {
                self.theme_mark(crate::theme_library::Step::Other);
                self.theme = design_theme(self.design(), false);
                os_theme::set_current(&self.theme.colors);
                if let Some(d) = theme_dir() {
                    let _ = std::fs::remove_file(d.join("current.flstheme"));
                }
                self.state.fl_text = None;
                self.state.theme_base = None;
                self.sync_preset();
                self.state.message = Some(format!("Theme: {} (the skin's own colours)", self.design().name()));
            }
            SAct::Preset(i) => {
                if let Some((name, text)) = st.themes.get(i).cloned() {
                    self.theme_mark(crate::theme_library::Step::Other);
                    self.theme = skin_theme(self.design(), &name, Some(&text));
                    os_theme::set_current(&self.theme.colors);
                    self.state.fl_text = Some(text.clone());
                    self.state.theme_base = Some(text.clone());
                    // Remembered like an imported theme (loaded at the next start).
                    if let Some(d) = theme_dir() {
                        let _ = std::fs::create_dir_all(&d);
                        let _ = std::fs::write(d.join("current.flstheme"), &text);
                    }
                    self.sync_preset();
                    self.state.message = Some(format!("Theme: {name}"));
                }
            }
            SAct::Button("Load theme...") => self.theme_dialog(),
            SAct::Button(views::settings::SHOW_ASIO_PANEL) => self.transport.asio_panel(),
            SAct::Button("Manage plugins") => self.open_settings(views::settings::PLUGINS),
            SAct::Button("Manage messages") => return self.messages_menu(),
            SAct::Button("Scan for new plugins") => self.start_scan(crate::plugin_db::Mode::Changed),
            SAct::Button("Rescan all") => self.start_scan(crate::plugin_db::Mode::All),
            SAct::Button("Add folder...") => self.add_plugin_folder(),
            SAct::Button(views::settings::RESET_WORK_TIME) => self.reset_work_time(),
            SAct::Button(views::settings::MANAGE_SHORTCUTS) => return self.open_shortcuts(),
            SAct::Button(crate::data_folder::RENAME) => self.open_rename(),
            SAct::Button(crate::data_folder::SET_FOLDER) => self.data_folder_pick(),
            SAct::Button(crate::factory_folder::SET_FOLDER) => self.factory_folder_pick(),
            SAct::Button(crate::factory_folder::RESET) => self.set_factory_folder(None),
            SAct::Button(b) => self.state.message = Some(off_text(b)),
            // The Info page's boxes (spec: ui/settings.md "Info").
            SAct::Info(f) => return self.info_act(f),
            // File ▸ Browser extra search folders (`crate::search_folders`).
            SAct::FolderPick(i) => return self.search_folder_pick(i),
            SAct::FolderCell(i, col) => return self.search_folder_cell(i, col),
            SAct::Genres(x, y) => return self.genre_menu(x, y),
        }
        self.invalidate();
    }

    /// The automation target, name and current value of a control, for "Create automation clip" (§8).
    pub(crate) fn automation_target(&self, a: &Action) -> Option<(u32, String, f64)> {
        use os_model::target;
        let s = &self.state;
        let chan = |row: usize| s.project.channels.get(row);
        let chan_name = |row: usize| chan(row).and_then(|c| c.name.clone()).unwrap_or_else(|| format!("Channel {}", row + 1));
        let mix_name = |t: u16| {
            s.project.mixer.iter().find(|m| m.index == t).and_then(|m| m.name.clone()).unwrap_or_else(|| if t == 0 { "Master".into() } else { format!("Bus {t}") })
        };
        Some(match *a {
            Action::Knob(crate::knobs::KnobId::ChanVol(row)) => {
                (target::channel(chan(row)?.index, target::CHANNEL_VOLUME), format!("{} - Channel volume", chan_name(row)), chan(row)?.volume)
            }
            Action::Knob(crate::knobs::KnobId::ChanPan(row)) => {
                (target::channel(chan(row)?.index, target::CHANNEL_PAN), format!("{} - Channel panning", chan_name(row)), 0.5 + chan(row)?.pan / 2.0)
            }
            Action::Knob(crate::knobs::KnobId::MixPan(t)) => {
                let pan = s.project.mixer.iter().find(|m| m.index == t).and_then(|m| m.pan).unwrap_or(0.0);
                (target::mixer(t, target::MIXER_PAN), format!("{} - Pan", mix_name(t)), 0.5 + pan / 2.0)
            }
            Action::Fader(t) => {
                let v = s.project.mixer.iter().find(|m| m.index == t).and_then(|m| m.volume).unwrap_or(0.8);
                (target::mixer(t, target::MIXER_VOLUME), format!("{} - Volume", mix_name(t)), v)
            }
            Action::Tempo => (target::TEMPO, "Tempo".into(), crate::playlist::tempo_value(s.tempo)),
            Action::Knob(k @ crate::knobs::KnobId::Eq(t, b, f)) => {
                s.project.mixer.iter().find(|m| m.index == t)?;
                let param = [target::EQ_GAIN, target::EQ_FREQ, target::EQ_WIDTH][usize::from(f.min(2))] + b.min(2);
                let text = s.knob_text(k);
                let label = text.split_once(':').map_or(text.as_str(), |(a, _)| a);
                (target::mixer(t, param), format!("{} - {label}", mix_name(t)), f64::from(s.knob(k)))
            }
            _ => return None,
        })
    }

    /// Repaints the current track's equaliser: graph, faders and knobs (a change to one moves the others).
    fn damage_eq(&mut self) {
        use crate::{knobs::KnobId, mixer::MHit};
        let eq = |a: &Action| matches!(a, Action::Knob(KnobId::Eq(..)) | Action::Mix(MHit::EqGraph));
        match self.hits.iter().filter(|h| eq(&h.action)).map(|h| h.rect).reduce(|a, b| a.union(&b)) {
            Some(r) => self.damage_rect(r.outset(8.0)),
            None => self.invalidate(),
        }
    }

    pub(crate) fn top_hit(&self, x: f32, y: f32) -> Option<&Hit> {
        self.hits.iter().rev().find(|h| h.rect.contains(x, y))
    }

    /// Mouse moved; returns whether a repaint is needed.
    pub fn mouse_move(&mut self, x: f32, y: f32) -> bool {
        self.mouse = (x, y);
        self.hold_moved(x, y);
        // A drag between windows came onto another surface: its ghost leaves the one it was drawn on.
        if self.cross_drag() && self.state.drag_on != self.state.surface {
            self.state.drag_on = self.state.surface;
            self.invalidate();
            self.mark_detached(None);
        }
        if let Some(d) = self.drag.clone() {
            match d {
                Drag::Browser { item, label, m0, started } => self.browser_drag_move(label, m0, started, item),
                Drag::Pl(d) => {
                    if let Some(g) = self.pl_grid {
                        crate::playlist::drag_move(self, d, g);
                    }
                }
                Drag::BarZoom { t, m0, p0, ratio, z0, center, view } => {
                    let z = z0 * 1.012f32.powf(m0.1 - y);
                    self.zoom_set(t, z);
                    let k = self.zoom_get(t).map_or(1.0, |z| z / z0);
                    // Keep the content that was centred in view, then add the horizontal drag.
                    self.scroll_set(t, (center * k - view / 2.0) + (x - m0.0) * ratio * k);
                    let _ = p0;
                    self.damage_scroll(t);
                }
                Drag::BarEdge { t, right, fixed, units, grab } => {
                    if let Some(b) = self.bar(t) {
                        let along = if b.vertical { y - b.track.y } else { x - b.track.x };
                        let at = ((along - grab) / b.len() * units).clamp(0.0, units);
                        let span = if right { at - fixed } else { fixed - at };
                        self.zoom_set(t, b.view / span.max(f32::MIN_POSITIVE));
                        let z = self.zoom_get(t).unwrap_or(1.0);
                        // The scroll range at the new zoom: the bar itself is only repainted with it next frame.
                        if let Some(bar) = self.bars.iter_mut().find(|bar| bar.target == t) {
                            bar.max = (units * z - bar.view).max(0.0);
                        }
                        self.scroll_set(t, if right { fixed * z } else { fixed * z - b.view });
                        self.damage_scroll(t);
                    }
                }
                Drag::BarRows { t, last, row } => {
                    // The picker's bar resizes its previews either way, down / right = taller, as FL's resizes them
                    // (spec: ui/scroll-bars.md §3); the others scroll up/down.
                    let picker = t == Scroll::PlPick;
                    // Only when it moved up/down: the clamp uses last frame's bar, stale once the rows were resized.
                    if let Some(b) = self.bar(t).filter(|_| y != last.1 && !picker) {
                        self.scroll_set(t, self.scroll_get(t) + (y - last.1) * b.ratio());
                    }
                    let moved = if picker { x - last.0 + y - last.1 } else { x - last.0 };
                    let row = row.map(|r| {
                        let r = r * BAR_ROW_STEP.powf(moved / BAR_ROW_PX);
                        self.set_rows(t, r);
                        r
                    });
                    self.drag = Some(Drag::BarRows { t, last: (x, y), row });
                    self.damage_scroll(t);
                }
                Drag::PianoRowZoom { y0, row0 } => {
                    if let Some(g) = self.piano_geom() {
                        let row = row0 * piano::ROW_ZOOM_STEP.powf((y0 - y) / piano::ROW_ZOOM_PX);
                        piano::set_rows(&mut self.state, g.grid, y0, row);
                        self.damage_win(Win::PianoRoll);
                    }
                }
                Drag::Scroll { m0, axes } => {
                    for (t, vertical, p0, ratio) in axes {
                        let d = if vertical { y - m0.1 } else { x - m0.0 };
                        let v = p0 + d * ratio;
                        if matches!(t, Scroll::PlaylistX | Scroll::PlaylistY) {
                            // In whole steps the drawn pixels can move by (`playlist_scrolled`).
                            let before = self.state.playlist_scroll;
                            let cur = self.scroll_get(t);
                            let v = blit_step(self.px_scale).map_or(v, |q| cur + ((v - cur) / q).round() * q);
                            self.scroll_set(t, v);
                            self.playlist_scrolled(before);
                        } else {
                            self.scroll_set(t, v);
                            self.damage_scroll(t);
                        }
                    }
                }
                Drag::NoteMove { x0, y0, orig } => {
                    if std::mem::take(&mut self.shift_clone) {
                        self.pick_up_notes(false);
                    } else {
                        self.drag_notes(x0, y0, &orig, false);
                    }
                    self.state.piano.dirty = true;
                }
                Drag::NoteResize { x0, orig } => {
                    self.drag_notes(x0, 0.0, &orig, true);
                    self.state.piano.dirty = true;
                }
                Drag::NoteErase => self.erase_at(x, y),
                Drag::NoteMute { on } => self.mute_at(x, y, on),
                Drag::NotePaint { key, start, len } => self.paint_notes(key, start, len),
                Drag::Marquee { x0, y0, base } => {
                    let r = Rect::span(x0.min(x), x0.max(x), y0.min(y), y0.max(y));
                    self.state.piano.overlay = Some(Overlay::Marquee(r));
                    if let Some(g) = self.piano_geom() {
                        let s = &mut self.state;
                        let mut sel = base.clone();
                        sel.extend(piano::notes_in(s, &g, r).into_iter().filter(|i| !base.contains(i)));
                        if let Some(ch) = piano::channel(s) {
                            s.piano.select(s.pattern, ch, sel);
                        }
                    }
                    self.damage_piano();
                }
                Drag::Slice => {
                    if let Some(g) = self.piano_geom() {
                        let t = crate::piano_snap::place(&self.state, &g, g.tick_at(x), self.alt);
                        self.state.piano.overlay = Some(Overlay::Slice(t));
                    }
                    self.damage_piano();
                }
                Drag::ZoomBox { x0, y0 } => {
                    self.state.piano.overlay = Some(Overlay::ZoomBox(Rect::span(x0.min(x), x0.max(x), y0.min(y), y0.max(y))));
                    self.damage_piano();
                }
                Drag::Marker => {
                    if let Some(g) = self.piano_geom() {
                        self.state.piano.marker = crate::piano_snap::place(&self.state, &g, g.tick_at(x), self.alt);
                        self.state.piano.marker_placed = true;
                    }
                    self.damage_piano();
                }
                Drag::SongRuler => {
                    if let Some(g) = self.piano_geom() {
                        let t = crate::piano_snap::place(&self.state, &g, g.tick_at(x), self.alt);
                        self.piano_song_seek(t, false);
                    }
                    self.damage_piano();
                }
                Drag::TimeSel { t0 } => {
                    if let Some(g) = self.piano_geom() {
                        let t = crate::piano_snap::place(&self.state, &g, g.tick_at(x), self.alt);
                        piano::select_range(&mut self.state, t0.min(t), t0.max(t));
                    }
                    self.damage_piano();
                }
                Drag::Scrub { tick } => {
                    if let Some(g) = self.piano_geom() {
                        let t = g.tick_at(x).max(0.0);
                        self.scrub(tick, t);
                        self.drag = Some(Drag::Scrub { tick: t });
                        self.state.piano.overlay = Some(Overlay::Scrub(t));
                    }
                    self.damage_piano();
                }
                Drag::Velocity { last_x, last_y } => {
                    self.state.piano.dirty = true;
                    self.paint_velocity(last_x, last_y, x, y);
                    self.drag = Some(Drag::Velocity { last_x: x, last_y: y });
                }
                Drag::ControlErase { last_x } => {
                    self.erase_control_points(last_x, x);
                    self.drag = Some(Drag::ControlErase { last_x: x });
                }
                Drag::Touch => self.touch_drag(x, y),
                Drag::Events(st) => self.events_drag(st, x, y),
                Drag::PianoKeys { key } => {
                    if let Some(g) = self.piano_geom() {
                        let k = g.key_at(y).clamp(0, views::piano::TOP_KEY);
                        if k != key {
                            self.drag = Some(Drag::PianoKeys { key: k });
                            self.key_velocity = self.key_click_velocity(x);
                            self.hold_key(Some(k), true);
                        }
                    }
                }
                Drag::Bar(_) => self.invalidate(),
                Drag::Window { win, dx, dy } => {
                    let old = self.win_bounds(win);
                    // The window keeps its part in the workspace, whatever "Don't limit windows to screen" says.
                    crate::win_clamp::drag_to(&mut self.state, win, (x - dx, y - dy));
                    let new = self.win_bounds(win);
                    self.damage_rect(old.union(&new));
                }
                Drag::WinResize { win, edges, m0, r0 } => {
                    let old = self.win_bounds(win);
                    let (dx, dy) = (x - m0.0, y - m0.1);
                    let (mw, mh) = win.min_size();
                    let mut r = r0;
                    if edges & EDGE_L != 0 {
                        r.w = (r0.w - dx).max(mw);
                        r.x = r0.right() - r.w;
                    }
                    if edges & EDGE_R != 0 {
                        r.w = (r0.w + dx).max(mw);
                    }
                    if edges & EDGE_T != 0 {
                        r.h = (r0.h - dy).max(mh).min((r0.bottom() - views::work_y(&self.state)).max(mh));
                        r.y = r0.bottom() - r.h;
                    }
                    if edges & EDGE_B != 0 {
                        r.h = (r0.h + dy).max(mh);
                    }
                    // On whole px, as a dragged window's place (`win_clamp::set_pos`).
                    let r = Rect::new(r.x.round(), r.y.round(), r.w.round(), r.h.round());
                    if let Some(i) = win.slot() {
                        self.state.wins[i].rect = r;
                    }
                    let new = self.win_bounds(win);
                    self.damage_rect(old.union(&new));
                }
                Drag::RackResize { edges, m0, s0 } => {
                    let old = views::rack::bounds(&self.state);
                    let s = &mut self.state;
                    let w = if edges & views::rack::EDGE_RIGHT != 0 { s0.0 + x - m0.0 } else { s0.0 };
                    let h = if edges & views::rack::EDGE_BOTTOM != 0 { s0.1 + y - m0.1 } else { s0.1 };
                    s.rack_size = views::rack::snap_size(s, w, h);
                    let new = views::rack::bounds(&self.state);
                    self.damage_rect(old.union(&new));
                }
                Drag::RackMixer { ch, y0, v0 } => {
                    // One insert per 10 px, up = higher (spec: ui/channel-rack.md §12).
                    let v = (i32::from(v0) + ((y0 - y) / crate::rack::MIXER_DRAG).round() as i32).clamp(0, i32::from(crate::mixer::max_insert(&self.state.project))) as u16;
                    if self.state.project.channels.get(ch).is_some_and(|c| c.mixer_track != v) {
                        self.channel_control(ch, ChannelControl::Track, |c| c.mixer_track = v);
                        self.state.piano.dirty = true;
                    }
                    self.hint = Cow::Owned(format!("Target mixer track: {}", views::rack::insert_name(v)));
                    let r = self.hits.iter().rev().find(|h| h.action == Action::RackMixer(ch)).map(|h| h.rect);
                    match r {
                        Some(r) => self.damage_rect(r.outset(2.0)),
                        None => self.invalidate(),
                    }
                    self.damage_band(HINT_RECT);
                }
                Drag::RackPaint { row, on, last } => {
                    if let Some(k) = views::rack::step_at(&self.state, x) {
                        if k != last {
                            for j in last.min(k)..=last.max(k) {
                                crate::rack::set_step(&mut self.state, row, j, on);
                            }
                            self.drag = Some(Drag::RackPaint { row, on, last: k });
                            self.invalidate();
                        }
                    }
                }
                Drag::RackSel { from } => {
                    if let Some(r) = views::rack::row_at(&self.state, y) {
                        let shown = crate::rack::shown(&self.state);
                        if let (Some(a), Some(b)) = (shown.iter().position(|&x| x == from), shown.iter().position(|&x| x == r)) {
                            self.state.rack.sel = shown[a.min(b)..=a.max(b)].iter().copied().collect();
                        }
                    }
                    self.invalidate();
                }
                Drag::RackDots { x0, p0 } => {
                    let w = p0 as f32 / 100.0 * crate::rack::NAME_W + (x - x0);
                    let pct = (w / crate::rack::NAME_W * 100.0).round().clamp(crate::rack::NAME_PCT_MIN as f32, crate::rack::NAME_PCT_MAX as f32) as u32;
                    crate::rack::relayout(&mut self.state, |s| s.rack.name_pct = pct);
                    self.hint = Cow::Owned(format!("Channel button width: {pct}%"));
                    self.invalidate();
                }
                Drag::RackLen { y0, v0 } => {
                    let v = (v0 as i32 + ((y0 - y) / crate::rack::LEN_DRAG).round() as i32).clamp(0, crate::rack::max_steps(&self.state) as i32) as u32;
                    crate::rack::set_len(&mut self.state, (v > 0).then_some(v));
                    self.hint = Cow::Owned(if v == 0 { "Pattern length: Auto".into() } else { format!("Pattern length: {v} steps") });
                    self.invalidate();
                }
                Drag::RackLoopLen { row, y0, v0 } => {
                    let hint = crate::rack::set_loop_box(&mut self.state, row, v0 + ((y0 - y) / crate::rack::LEN_DRAG).round() as i32);
                    self.hint = Cow::Owned(hint);
                    self.invalidate();
                }
                Drag::RackGraph => self.rack_graph_at((x, y)),
                Drag::Dialog { dx, dy } => self.dialog_drag(x - dx, y - dy),
                Drag::DialogBar { y0, s0, ratio } => self.dialog_scroll(s0 + (y - y0) * ratio),
                Drag::PatternBox { y0, p0 } => {
                    let (y0, p0): (f32, u16) = (y0.to_owned(), p0.to_owned());
                    let steps = ((y0 - y) / crate::patterns::DRAG_STEP).trunc() as i32;
                    let n = (i32::from(p0) + steps).clamp(1, i32::from(crate::patterns::MAX)) as u16;
                    crate::patterns::select(&mut self.state, n);
                    self.invalidate();
                }
                Drag::MixSelect => {
                    // The strip whose column the cursor is in, whatever its height.
                    let under = self.hits.iter().rev().find_map(|h| match h.action {
                        Action::Mix(crate::mixer::MHit::Strip(n)) if x >= h.rect.x && x < h.rect.right() => Some(n),
                        _ => None,
                    });
                    if let Some(n) = under.filter(|&n| n != self.state.selected_mixer) {
                        crate::mixer::select(&mut self.state, n, false);
                        self.damage_win(Win::Mixer);
                    }
                }
                Drag::Fader { track, y0, v0 } => {
                    let per_px = views::mixer::FADER_PER_PX / f64::from(views::mixer::scale(self.state.win_rect(Win::Mixer)));
                    let v = (v0 - f64::from(y - y0) * per_px).clamp(0.0, 1.0);
                    if let Some(t) = self.state.project.mixer.iter_mut().find(|t| t.index == track) {
                        t.volume = Some(v);
                        self.state.piano.dirty = true;
                    }
                    self.record_control(&Action::Fader(track));
                    self.hint = Cow::Owned(crate::mixer::level_text("Volume", v));
                    let col = self.hits.iter().rev().find(|h| h.action == Action::Fader(track)).map(|h| h.rect);
                    match col {
                        Some(r) => self.damage_rect(r.outset(10.0)),
                        None => self.invalidate(),
                    }
                    self.damage_band(HINT_RECT);
                }
                Drag::Knob { id, y0, v0 } => {
                    self.state.set_knob(id, v0 - (y - y0) * id.drag_rate());
                    self.record_knob(id);
                    self.multilink_knob(id.target());
                    self.hint = Cow::Owned(self.state.knob_text(id));
                    match self.hits.iter().rev().find(|h| h.action == Action::Knob(id)).map(|h| h.rect) {
                        Some(r) => self.damage_rect(r.outset(6.0)),
                        None => self.invalidate(),
                    }
                    if let crate::knobs::KnobId::Eq(..) = id {
                        self.damage_eq();
                    }
                    self.damage_band(HINT_RECT);
                }
                Drag::LimitKeys => {
                    let k = views::piano_dlg::kb_key(&self.state, x, y, false);
                    crate::piano_dlg::set_green(&mut self.state, k);
                    self.invalidate();
                }
                Drag::LimitEnd { high } => {
                    let k = views::piano_dlg::kb_key(&self.state, x, y, true);
                    crate::piano_dlg::set_end(&mut self.state, high, k);
                    self.invalidate();
                }
                Drag::LimitScroll { dx } => self.limit_scroll_handle(x - dx),
                Drag::EqGraph { track, band, g } => {
                    crate::mixer::eq_graph_drag(&mut self.state, track, band, g, (x, y));
                    self.hint = Cow::Owned(crate::mixer::eq_graph_hint(&self.state, g, x));
                    self.damage_eq();
                    self.damage_band(HINT_RECT);
                }
                Drag::MonParam(i) => {
                    self.set_mon_param(i, x);
                    return true;
                }
                Drag::MidiCurve(i) => {
                    self.midi_curve_drag(i, y);
                    return true;
                }
                Drag::MidiBox { part, y0, steps } => {
                    let now = ((y0 - y) / 8.0) as i32;
                    if now != steps {
                        self.drag = Some(Drag::MidiBox { part, y0, steps: now });
                        self.midi_page_wheel(part, now - steps);
                    }
                    return true;
                }
                Drag::LinkBox { part, y0, steps } => {
                    let now = ((y0 - y) / 8.0) as i32;
                    if now != steps {
                        self.drag = Some(Drag::LinkBox { part, y0, steps: now });
                        self.link_box_step(part, now - steps);
                    }
                    return true;
                }
                Drag::CableParam(i) => {
                    self.set_cable_param(i, x);
                    return true;
                }
                Drag::SongPos { dx } => self.song_pos_drag(dx),
                Drag::PianoSplit { dy, top, h } => {
                    let split = self.state.piano.split.dragged(y - dy - top, h);
                    if split != self.state.piano.split {
                        self.state.piano.split = split;
                        self.damage_piano();
                    }
                }
                Drag::BrowserSplit { dx } => self.browser_split_drag(x - dx),
                Drag::SongPosHold { .. } => {}
                Drag::PrefSlider { label, x: x0, w } => self.pref_slider_drag(label, x0, w),
                Drag::Ppi { x0, n0 } => self.ppi_set(crate::ppi::dragged(n0, x - x0, self.px_scale)),
                Drag::ThemeKnob { i, y0, p0 } => self.theme_knob_drag(i, y0, p0, y),
                Drag::PickSv(r) => self.picker_sv(r, x, y),
                Drag::PickHue(r) => self.picker_hue(r, y),
                Drag::Tempo { y0, t0, step } => {
                    // Rounded to the box's thousandths, so repeated steps don't drift.
                    let t = self.set_tempo(((t0 + f64::from(y0 - y).round() * step) * 1000.0).round() / 1000.0);
                    self.record_control(&Action::Tempo);
                    self.hint = Cow::Owned(format!("Tempo {t:.3} BPM"));
                    self.damage_band(TEMPO_RECT);
                    self.damage_band(HINT_RECT);
                }
            }
            return true;
        }
        let (hover, mut hint, rect) = match self.top_hit(x, y) {
            Some(h) => (Some(h.action.clone()), h.hint.clone(), Some(h.rect)),
            None => (None, Cow::Borrowed(""), None),
        };
        if hover == Some(Action::Monitor) {
            // Reading under the cursor: frequency and level of the analyzer column.
            let x = self.panel_mouse(crate::toolbar::Panel::Monitor).0;
            hint = Cow::Owned(self.state.monitor.hint((x - MONITOR_RECT.x - 1.0).max(0.0) as usize));
        }
        if hover == Some(Action::CpuPanel) {
            hint = Cow::Owned(self.state.cpu.hint());
        }
        if let (Some(Action::PianoKeys), Some(g)) = (&hover, self.piano_geom()) {
            // The key under the cursor: "C5 / 60" (spec: ui/settings-choices.md "Note names and Black notes").
            hint = Cow::Owned(crate::note_names::key_hint(g.key_at(y).clamp(0, views::piano::TOP_KEY)));
        }
        if let (Some(Action::Mix(crate::mixer::MHit::EqGraph)), Some(g)) = (&hover, rect) {
            hint = Cow::Owned(crate::mixer::eq_graph_hint(&self.state, g, x));
        }
        let resize = matches!(hover, Some(Action::PianoNote(_)))
            && matches!(self.state.piano.tool, Tool::Draw | Tool::Paint | Tool::Select)
            && rect.is_some_and(|r| x >= r.right() - resize_zone(r.w));
        // Playlist: the clip under the mouse (automation clips show their points) and the resize cursor on clip edges.
        let mut resize = resize;
        // The ends of the Playlist's and Piano roll's horizontal thumbs zoom (spec: ui/scroll-bars.md §4).
        if let Some(b) = hover.as_ref().and_then(|a| if let Action::Scrollbar(t) = a { self.bar(*t) } else { None }) {
            let along = if b.vertical { y - b.track.y } else { x - b.track.x };
            if b.edge_at(self.scroll_get(b.target), along).is_some() {
                resize = true;
                hint = Cow::Borrowed(BAR_EDGE_HINT);
            }
        }
        if hover == Some(Action::PlaylistArea) {
            if let Some(g) = self.pl_grid {
                let s = &self.state;
                let pg = crate::playlist::PGeom::new(s, g);
                let hit = crate::playlist::clip_at(s, &pg, x, y);
                // Automation clips show their points while hovered, audio clips their fade and gain handles (§9a).
                let auto = hit.filter(|&i| {
                    let c = &crate::playlist::clips(s)[i];
                    matches!(c.source, os_model::ClipSource::Channel(ch) if s.project.automation(ch).is_some()) || crate::playlist::is_audio_clip(s, &pg, c)
                });
                let handle = hit.and_then(|i| crate::playlist::handle_at(s, &pg, i, x, y));
                if auto != self.state.playlist.hover_clip || handle != self.state.playlist.hover_handle {
                    // Repaint only the clips that lost and gained them (or whose lit handle changed).
                    for i in [self.state.playlist.hover_clip, auto].into_iter().flatten() {
                        match crate::playlist::clips(&self.state).get(i) {
                            Some(c) => self.damage_rect(pg.clip_rect(c).outset(6.0)),
                            None => self.invalidate(),
                        }
                    }
                    self.state.playlist.hover_clip = auto;
                    self.state.playlist.hover_handle = handle;
                }
                if let (Some(i), Some(h)) = (hit, handle) {
                    hint = Cow::Owned(crate::playlist::handle_hint(&crate::playlist::clips(&self.state)[i], h));
                } else if let Some(i) = hit {
                    let r = pg.clip_rect(&crate::playlist::clips(&self.state)[i]);
                    let z = crate::playlist::EDGE.min(r.w / 4.0);
                    let tool_resizes = matches!(self.state.playlist.tool, crate::playlist::PlTool::Draw | crate::playlist::PlTool::Paint | crate::playlist::PlTool::Select);
                    resize |= tool_resizes && (x <= r.x + z || x >= r.right() - z);
                    if hint.is_empty() || hint == "Arranger" {
                        hint = Cow::Owned(crate::playlist::source_name(&self.state, crate::playlist::clips(&self.state)[i].source));
                    }
                }
            }
        } else if let Some(i) = self.state.playlist.hover_clip.take() {
            self.state.playlist.hover_handle = None;
            let r = self.pl_grid.zip(crate::playlist::clips(&self.state).get(i)).map(|(g, c)| crate::playlist::PGeom::new(&self.state, g).clip_rect(c));
            match r {
                Some(r) => self.damage_rect(r.outset(6.0)),
                None => self.invalidate(),
            }
        }
        let changed = hover != self.hover || hint != self.hint || resize != self.resize_cursor;
        self.resize_cursor = resize;
        if changed {
            // Repaint the controls that lost / gained hover and the hint panel; large background
            // regions have no hover look.
            let mut dirty = Vec::new();
            for (a, r) in [(&self.hover, self.hover_rect), (&hover, rect)] {
                if let (Some(a), Some(r)) = (a, r) {
                    if hover_is_visible(a) {
                        dirty.push(r.outset(3.0));
                    }
                }
            }
            for r in dirty {
                self.damage_rect(r);
            }
            if hint != self.hint {
                self.damage_band(HINT_RECT);
            }
        }
        self.hover = hover;
        self.hover_rect = rect;
        self.hint = hint;
        changed
    }

    /// Plain buttons, switches and menu entries act on the release over the same control instead of on the press, like
    /// the system's own (owner, 2026-10-15 for the window buttons, 2026-10-18 for the rest: acting on the press made
    /// accidental clicks easy). The press arms one; moving off it before releasing lets it go. What a press starts stays
    /// on the press: drags (knobs, faders, sliders, value boxes, scroll bars, grids, steps, window title bars), menu and
    /// list openers, list rows, text fields, scroll bar arrows and the tempo tapper's pad.
    fn acts_on_release(&self, a: &Action) -> bool {
        use crate::dialog::DHit;
        use crate::mixer::MHit;
        use crate::project_forms::FHit;
        use crate::views::settings::{Plug, SAct};
        match a {
            Action::Settings(sa) => matches!(
                sa,
                SAct::Close
                    | SAct::Tab(_)
                    | SAct::Check(_)
                    | SAct::Group(_)
                    | SAct::Button(_)
                    | SAct::Preset(_)
                    | SAct::FolderPick(_)
                    | SAct::Plug(Plug::Folder(_) | Plug::Remove(_) | Plug::Prefer(_) | Plug::Sort(_) | Plug::Star(_) | Plug::Show(_))
            ),
            // A filled effect slot's name opens its window; an empty one's opens the effect menu.
            Action::Mix(MHit::SlotName(i)) => crate::mixer::slot_name(&self.state, self.state.selected_mixer, *i).is_some(),
            Action::Mix(h) => matches!(
                h,
                MHit::Led(_) | MHit::Polarity(_) | MHit::Swap(_) | MHit::Route(_) | MHit::Unroute(_) | MHit::Add | MHit::AddAfter | MHit::SlotLed(_)
            ),
            Action::Dlg(h) => matches!(
                h,
                DHit::Close
                    | DHit::Button(_)
                    | DHit::Remember
                    | DHit::Back
                    | DHit::Up
                    | DHit::NewFolder
                    | DHit::Place(_)
                    | DHit::Header(_)
                    | DHit::FilterItem(_)
                    | DHit::Accept
                    | DHit::Cancel
                    | DHit::ShortcutChoice(_)
                    | DHit::ResetAll
                    | DHit::Apply
                    | DHit::Form(FHit::Check(_) | FHit::Main | FHit::Skip | FHit::Auto | FHit::Folder | FHit::Item(_) | FHit::Fold)
            ),
            Action::Perf(p) => matches!(p, crate::perf::PerfAct::Sort(_) | crate::perf::PerfAct::ResetOnTransport | crate::perf::PerfAct::Freeze),
            Action::Tapper(t) => *t == crate::tapper::TapAct::Apply,
            Action::Touch(t) => !matches!(t, crate::touch::TouchAct::Note(_)),
            Action::Events(a) => matches!(a, crate::events::EvAct::Tool(_)),
            Action::CableDebug(d) => !matches!(d, crate::cable::DebugAct::Param(_)),
            Action::TypeIn(t) => !matches!(t, crate::typein::TiAct::Field),
            Action::Pat(h) => {
                use crate::patterns::Hit;
                matches!(h, Hit::Accept | Hit::Cancel | Hit::Suggest(..) | Hit::Swatch(_) | Hit::ConfirmOk | Hit::ConfirmCancel | Hit::ConfirmRemember)
            }
            Action::RackFill(h) => !matches!(h, crate::rack::FillHit::Inside),
            Action::Bar(h) => matches!(h, crate::toolbar::BarHit::Rows(_) | crate::toolbar::BarHit::Done),
            _ => matches!(
                a,
                Action::Placeholder(_)
                    | Action::Typing
                    | Action::Metronome
                    | Action::AutoScroll
                    | Action::EnableGroups
                    | Action::StepEdit
                    | Action::Clock
                    | Action::GlobalSnapToggle
                    | Action::Monitor
                    | Action::MonReset
                    | Action::MonSave
                    | Action::MonClose
                    | Action::Minimize
                    | Action::Maximize
                    | Action::Close
                    | Action::Play
                    | Action::Stop
                    | Action::PatSong(_)
                    | Action::MenuItem(..)
                    | Action::ToggleWin(_)
                    | Action::PatternAdd
                    | Action::ChannelSelect(_)
                    | Action::ChannelEnable(_)
                    | Action::Util(_)
                    | Action::RackRollView
                    | Action::RackRoll(_)
                    | Action::CloseWin(_)
                    | Action::MinWin(_)
                    | Action::MaxWin(_)
                    | Action::BrowserTab(_)
                    | Action::BrowserCollapse
                    | Action::BrowserReread
                    | Action::PianoUndo
                    | Action::PianoTool(_)
                    | Action::PianoLanePick(_)
                    | Action::PlTool(_)
                    | Action::PlLed(_)
                    | Action::PlGroupArrow(_)
                    | Action::PlStretch
                    | Action::PlPickTab(_)
                    | Action::PlPickAdd
                    | Action::PlPopup(_)
                    | Action::RackSpeaker
                    | Action::RackLoop
                    | Action::RackGraph
                    | Action::RackGraphTab(_)
                    | Action::SongInfo
            ),
        }
    }

    pub fn mouse_down(&mut self) {
        // The popups and boxes this press opens go on the surface it was made on (a detached window's, `on_surface`).
        self.state.overlay_on = self.state.surface;
        self.invalidate();
        self.hold = None;
        let (x, y) = self.mouse;
        let action = self.top_hit(x, y).map(|h| h.action.clone());
        // A press stops the views easing where they are drawn; a scroll bar's arrow steps on from where its ease is
        // going (`crate::smooth`).
        if !matches!(action, Some(Action::ScrollStep(..))) {
            self.smooth.stop();
        }
        // Any press takes the Song info box away at once; one outside it goes on to what it is over.
        if action != Some(Action::SongInfo) {
            self.close_song_info();
        }
        self.pressed = action.clone();
        self.armed = None;
        self.armed_right = None;
        // A dialog takes every click (spec: ui/dialogs.md §3).
        if self.modal_open() {
            // The time signature dialog's value list (the app's popup) takes its own; a click anywhere else closes it.
            if crate::timesig::list_open(&self.state).is_some() {
                match action {
                    Some(a @ (Action::PlPopup(_) | Action::PlPopupSub(_))) => {
                        self.act(a);
                    }
                    _ => self.state.playlist.popup = None,
                }
                return;
            }
            if let Some(Action::Dlg(h)) = action {
                if self.acts_on_release(&Action::Dlg(h.clone())) {
                    self.armed = Some(Action::Dlg(h));
                } else {
                    self.dialog_click(h);
                }
            }
            return;
        }
        // So is the Remote control settings box; a press on one of its value boxes starts a vertical drag of it.
        if crate::remote::is_open(&self.state) {
            if let Some(Action::Remote(r)) = action {
                if r.is_value() {
                    self.drag = Some(Drag::LinkBox { part: r, y0: self.mouse.1, steps: 0 });
                } else {
                    self.link_box_act(r);
                }
            }
            return;
        }
        // The value box is modal: only its own parts take clicks (spec: ui/type-in-value.md §1).
        if crate::typein::is_open(&self.state) {
            if let Some(a @ Action::TypeIn(_)) = action {
                if self.acts_on_release(&a) {
                    self.armed = Some(a);
                } else {
                    self.act(a);
                }
            }
            return;
        }
        // Playlist popup menus close on any click outside them (their scroll bars are in them); a click elsewhere
        // accepts an open text box.
        if self.state.playlist.popup.is_some() && !matches!(action, Some(Action::PlPopup(_) | Action::PlPopupSub(_) | Action::Scrollbar(Scroll::Popup | Scroll::PopupSub))) {
            self.state.playlist.popup = None;
            // A dropdown's submenu: the click goes on to the dropdown (below), but one on the submenu's own ▸ entry only
            // closes it.
            match self.state.menu_sub.take() {
                Some(i) if self.state.menu_open.is_some() && !matches!(action, Some(Action::MenuSub(_, j)) if j == i) => {}
                _ => return,
            }
        }
        // The pattern name box / delete confirmation are modal: a click outside accepts the box.
        if (self.state.pat_ui.name_box.is_some() || self.state.pat_ui.confirm) && !matches!(action, Some(Action::Pat(_))) {
            if self.state.pat_ui.name_box.is_some() {
                crate::patterns::accept(&mut self.state);
            }
            return;
        }
        // The Advanced fill dialog is modal (spec: ui/channel-rack.md §22).
        if self.state.rack.fill.is_some() && !matches!(action, Some(Action::RackFill(_))) {
            return;
        }
        if self.state.playlist.edit.is_some() {
            crate::playlist::commit_edit(&mut self.state);
        }
        // A click anywhere but the Info box being edited accepts it (spec: ui/settings.md "Info").
        if self.state.settings.info_edit.as_ref().is_some_and(|e| action != Some(Action::Settings(views::settings::SAct::Info(e.field)))) {
            crate::project_info::accept_info(&mut self.state);
        }
        // The same for a cell of File ▸ Browser extra search folders.
        if self.state.settings.folder_edit.as_ref().is_some_and(|e| action != Some(Action::Settings(views::settings::SAct::FolderCell(e.row, e.col)))) {
            crate::search_folders::commit(&mut self.state);
        }
        // Clicking anywhere but the search field leaves it (spec: ui/browser.md §5).
        if action != Some(Action::BrowserSearch) {
            self.state.browser_search_focus = false;
        }
        // The same for the monitor's Find box (spec: ui/plugin-performance.md §1).
        if action != Some(Action::Perf(crate::perf::PerfAct::Find)) {
            self.state.perf.finding = false;
        }
        if self.state.lane_menu && !matches!(action, Some(Action::PianoLanePick(_))) {
            self.state.lane_menu = false;
            return;
        }
        // Any click outside an open menu closes it. VIEW's window entries are window toggles: one clicked in the menu
        // closes the menu and toggles its window. Its open submenu takes its own clicks.
        if self.state.menu_open.is_some()
            && !matches!(action, Some(Action::MenuItem(..) | Action::MenuSub(..) | Action::Menu(_) | Action::PlPopup(_) | Action::PlPopupSub(_) | Action::Scrollbar(Scroll::Popup | Scroll::PopupSub)))
        {
            self.state.menu_open = None;
            if !(matches!(action, Some(Action::ToggleWin(_))) && self.overlays.iter().any(|r| r.contains(x, y))) {
                return;
            }
        }
        // A button that acts on its release brings its window to the front then too: raised at the press, the window
        // showed in front of what the button was about to change for as long as the button was held (the channel
        // name button over an open plugin window hid it only after the rack had come over it).
        if action.as_ref().is_some_and(|a| self.acts_on_release(a)) {
            self.raise_at = Some((x, y));
        } else {
            self.raise_at = None;
            self.raise_under_mouse();
        }
        if let Some(a) = action {
            if self.acts_on_release(&a) {
                self.armed = Some(a);
            } else {
                self.act(a);
            }
        }
        self.rack_pending();
    }

    /// Clicking any part of a child window brings it to the front (spec: ui/channel-rack.md §5).
    fn raise_under_mouse(&mut self) {
        let (x, y) = self.mouse;
        self.raise_at_point(x, y);
    }

    /// The window at (`x`, `y`) to the front, the keyboard to it; a press on the empty workspace takes it from all.
    fn raise_at_point(&mut self, x: f32, y: f32) {
        if let Some(w) = self.top_hit(x, y).and_then(|h| h.win) {
            if self.state.z.last() != Some(&w) {
                self.state.raise(w);
                self.invalidate();
            }
            self.state.no_focus = false;
        } else if self.state.work_rect().contains(x, y) {
            self.state.no_focus = true;
        }
    }

    /// Area a floating window paints (for damage).
    fn win_bounds(&self, w: Win) -> Rect {
        crate::floating::placed(w).map_or_else(|| self.state.win_rect(w).outset(5.0), |p| (p.bounds)(&self.state))
    }

    /// Cursor for the current hover / drag.
    pub fn cursor(&self) -> Cursor {
        let win_edges = match (&self.drag, &self.hover) {
            (Some(Drag::WinResize { edges, .. }), _) => *edges,
            (None, Some(Action::WinResize(_, e))) => *e,
            _ => 0,
        };
        if win_edges != 0 {
            return match win_edges {
                EDGE_L | EDGE_R => Cursor::EwResize,
                EDGE_T | EDGE_B => Cursor::NsResize,
                e if e == EDGE_L | EDGE_T || e == EDGE_R | EDGE_B => Cursor::NwseResize,
                _ => Cursor::NeswResize,
            };
        }
        if matches!((&self.drag, &self.hover), (Some(Drag::PianoSplit { .. }), _) | (None, Some(Action::PianoSplitter))) {
            return Cursor::NsResize;
        }
        if matches!((&self.drag, &self.hover), (Some(Drag::BrowserSplit { .. }), _) | (None, Some(Action::BrowserSplitter))) {
            return Cursor::EwResize;
        }
        let edges = match (&self.drag, &self.hover) {
            (Some(Drag::RackResize { edges, .. }), _) => *edges,
            (None, Some(Action::RackResize(e))) => *e,
            _ => 0,
        };
        match edges {
            0 if self.resize_cursor => Cursor::EwResize,
            0 => Cursor::Default,
            views::rack::EDGE_RIGHT => Cursor::EwResize,
            views::rack::EDGE_BOTTOM => Cursor::NsResize,
            _ => Cursor::NwseResize,
        }
    }

    /// True while a drag moves a value rather than the cursor (knobs, the tempo, scroll-bar right-drags): the shell
    /// hides the cursor and keeps it where the drag started, feeding relative movement.
    pub fn cursor_locked(&self) -> bool {
        matches!(self.drag, Some(Drag::BarZoom { .. } | Drag::BarRows { .. } | Drag::Knob { .. } | Drag::Tempo { .. }))
    }

    pub(crate) fn bar(&self, t: Scroll) -> Option<Bar> {
        self.bars.iter().find(|b| b.target == t).copied()
    }

    /// Scroll value of an axis in content pixels.
    pub fn scroll_get(&self, t: Scroll) -> f32 {
        let s = &self.state;
        match t {
            Scroll::PianoX => s.piano.scroll_x * s.piano.px_per_beat / f32::from(s.project.ppq.max(4)),
            Scroll::PianoY => s.piano.scroll_y,
            Scroll::PitchY => piano::PITCH_RANGE as f32 - (s.piano.pitch_center as f32 + 241.0),
            Scroll::PlaylistX => s.playlist_scroll.0,
            Scroll::PlaylistY => s.playlist_scroll.1,
            Scroll::MixerX => s.mixer_scroll,
            Scroll::RackX => s.rack_scroll_x,
            Scroll::RackY => s.rack_scroll,
            Scroll::PlPick => s.playlist.pick_scroll,
            Scroll::Settings => s.settings.scroll,
            Scroll::PlugTable => s.plugdb.top as f32 * views::settings::TABLE_ROW_H,
            Scroll::Popup | Scroll::PopupSub => s.playlist.popup.as_ref().map_or(0.0, |p| p.scroll[usize::from(t == Scroll::PopupSub)] as f32),
        }
    }

    /// Set an axis, clamped to what its scroll bar allows (last frame).
    pub fn scroll_set(&mut self, t: Scroll, v: f32) {
        let unbounded = if matches!(t, Scroll::RackX | Scroll::RackY) { 0.0 } else { f32::MAX };
        let max = self.bar(t).map_or(unbounded, |b| b.max);
        let v = v.clamp(0.0, max.max(0.0));
        let s = &mut self.state;
        match t {
            Scroll::PianoX => s.piano.scroll_x = v / (s.piano.px_per_beat / f32::from(s.project.ppq.max(4))),
            Scroll::PianoY => s.piano.scroll_y = v.round(),
            Scroll::PitchY => s.piano.pitch_center = piano::clamp_pitch_center((piano::PITCH_RANGE as f32 - 241.0 - v).round() as i32),
            Scroll::PlaylistX => s.playlist_scroll.0 = v,
            Scroll::PlaylistY => s.playlist_scroll.1 = v,
            Scroll::MixerX => s.mixer_scroll = v,
            Scroll::RackX => s.rack_scroll_x = v,
            Scroll::RackY => s.rack_scroll = v,
            Scroll::PlPick => s.playlist.pick_scroll = v,
            Scroll::Settings => s.settings.scroll = v,
            Scroll::PlugTable => s.plugdb.top = (v / views::settings::TABLE_ROW_H).round() as usize,
            Scroll::Popup | Scroll::PopupSub => {
                let Some(p) = s.playlist.popup.as_mut() else { return };
                let (k, col) = (usize::from(t == Scroll::PopupSub), v.round() as usize);
                // Scrolling the list closes its submenu (its row may go out of view).
                if k == 0 && p.scroll[0] != col {
                    p.sub = None;
                    p.scroll[1] = 0;
                }
                p.scroll[k] = col;
            }
        }
    }

    /// Zoom of an axis in pixels per content unit, where it can zoom.
    fn zoom_get(&self, t: Scroll) -> Option<f32> {
        match t {
            Scroll::PianoX => Some(self.state.piano.px_per_beat),
            Scroll::PlaylistX => Some(self.state.playlist_bar_w),
            _ => None,
        }
    }

    fn zoom_set(&mut self, t: Scroll, z: f32) {
        match t {
            Scroll::PianoX => self.state.piano.px_per_beat = z.clamp(6.0, 1600.0),
            Scroll::PlaylistX => self.state.playlist_bar_w = z.clamp(4.0, 600.0),
            _ => {}
        }
    }

    /// Row height of a vertical scroll bar's content, where its rows can be resized (spec: ui/scroll-bars.md).
    fn rows_get(&self, t: Scroll) -> Option<f32> {
        match t {
            Scroll::PianoY => Some(self.state.piano.row_h),
            Scroll::PlaylistY => Some(self.state.playlist.row_h),
            Scroll::PlPick => Some(self.state.playlist.pick_row()),
            _ => None,
        }
    }

    /// Resize a vertical scroll bar's rows to `row` (clamped and rounded by the view), keeping the row at the view
    /// centre in place.
    fn set_rows(&mut self, t: Scroll, row: f32) {
        match t {
            Scroll::PianoY => {
                if let Some(g) = self.piano_geom() {
                    let at = g.grid.cy();
                    piano::set_rows(&mut self.state, g.grid, at, row);
                }
            }
            Scroll::PlaylistY => {
                if let Some(g) = self.pl_grid {
                    crate::playlist::set_row_h(&mut self.state, g, g.cy(), row);
                }
            }
            Scroll::PlPick => {
                if let Some(b) = self.bar(t) {
                    crate::playlist::set_pick_row(&mut self.state, b.view, row);
                }
            }
            _ => {}
        }
    }

    /// Press on a scroll bar. Left: a press on an end of a horizontal thumb that zooms drags that end; a press
    /// off the thumb first centres the thumb on the mouse, then dragging scrolls. Right: the cursor is held; on a horizontal bar sideways scrolls and up/down zooms,
    /// on a vertical bar up/down scrolls and sideways resizes the rows (spec: ui/scroll-bars.md).
    fn scrollbar_press(&mut self, t: Scroll, right: bool) {
        let Some(b) = self.bar(t) else { return };
        let (x, y) = self.mouse;
        let along = if b.vertical { y - b.track.y } else { x - b.track.x };
        let pos = self.scroll_get(t);
        let thumb0 = b.thumb_at(pos);
        if let (false, Some(end), Some(z)) = (right, b.edge_at(pos, along), self.zoom_get(t)) {
            let (units, start, stop) = ((b.max + b.view) / z, pos / z, (pos + b.view) / z);
            let (fixed, moved) = if end { (start, stop) } else { (stop, start) };
            // Measured from the end's time rather than the drawn thumb, so the press itself does not zoom.
            let grab = along - moved / units * b.len();
            if t == Scroll::PianoX {
                self.state.piano.bar_span = Some(units * f32::from(self.state.project.ppq.max(4)));
            }
            self.drag = Some(Drag::BarEdge { t, right: end, fixed, units, grab });
            return self.invalidate();
        }
        if !right && (along < thumb0 || along > thumb0 + b.thumb) {
            self.scroll_set(t, (along - b.thumb / 2.0) * b.ratio());
        }
        let p0 = self.scroll_get(t);
        self.drag = Some(if right && b.vertical {
            Drag::BarRows { t, last: (x, y), row: self.rows_get(t) }
        } else if right {
            let z0 = self.zoom_get(t).unwrap_or(1.0);
            Drag::BarZoom { t, m0: (x, y), p0, ratio: b.ratio(), z0, center: p0 + b.view / 2.0, view: b.view }
        } else {
            Drag::Scroll { m0: (x, y), axes: vec![(t, b.vertical, p0, b.ratio())] }
        });
        self.invalidate();
    }

    /// Middle button: grab-scroll the Piano roll and Playlist (FL manual: middle-drag scrolls
    /// both directions).
    pub fn mouse_down_middle(&mut self) {
        // The popups and boxes this press opens go on the surface it was made on (a detached window's, `on_surface`).
        self.state.overlay_on = self.state.surface;
        self.close_song_info();
        if crate::typein::is_open(&self.state) || crate::remote::is_open(&self.state) || self.modal_open() {
            return;
        }
        self.smooth.stop();
        self.raise_under_mouse();
        let (x, y) = self.mouse;
        let action = self.top_hit(x, y).map(|h| h.action.clone());
        if let Some(Action::Mix(h)) = action {
            if crate::mixer::middle(self, h) {
                return self.invalidate();
            }
        }
        if let Some(Action::BrowserRow(i)) = action {
            if self.browser_row_middle(i) {
                return;
            }
        }
        // Knobs, faders and sliders go back to their default (spec: ui/knobs.md "Owner decisions").
        if let Some(c) = action.as_ref().and_then(crate::knobs::Control::of) {
            return self.reset_control(c);
        }
        if self.ctrl && matches!(action, Some(Action::PianoGrid | Action::PianoNote(_) | Action::PianoKeys)) {
            self.drag = Some(Drag::PianoRowZoom { y0: y, row0: self.state.piano.row_h });
            return;
        }
        let targets: &[Scroll] = match action {
            Some(Action::PianoGrid | Action::PianoNote(_)) => &[Scroll::PianoX, Scroll::PianoY],
            Some(Action::PianoKeys) => &[Scroll::PianoY],
            Some(Action::PianoVelocity) => &[Scroll::PianoX],
            Some(Action::PlaylistArea) => &[Scroll::PlaylistX, Scroll::PlaylistY],
            _ => return,
        };
        let axes = targets.iter().map(|&t| (t, matches!(t, Scroll::PianoY | Scroll::PlaylistY), self.scroll_get(t), -1.0)).collect();
        self.drag = Some(Drag::Scroll { m0: (x, y), axes });
    }

    /// Right button: delete notes in the piano roll (spec: ui/piano-roll.md §8); right-drag on
    /// a scroll bar scrolls.
    pub fn mouse_down_right(&mut self) {
        // The popups and boxes this press opens go on the surface it was made on (a detached window's, `on_surface`).
        self.state.overlay_on = self.state.surface;
        self.close_song_info();
        if crate::typein::is_open(&self.state) || crate::remote::is_open(&self.state) || self.modal_open() {
            return;
        }
        self.smooth.stop();
        let (x, y) = self.mouse;
        let action = self.top_hit(x, y).map(|h| h.action.clone());
        if self.state.menu_open.is_some() {
            self.state.menu_open = None;
            // With its submenu.
            if self.state.menu_sub.take().is_some() {
                self.state.playlist.popup = None;
            }
            self.invalidate();
            return;
        }
        if self.state.pat_ui.name_box.is_some() || self.state.pat_ui.confirm {
            return;
        }
        // Pattern selector (spec: ui/pattern-menu.md §1): box = menu at the mouse, "+" = find next empty.
        match &action {
            Some(Action::Settings(views::settings::SAct::Preset(i))) => return self.theme_menu(*i, x, y),
            // The settings window's sliders and theme knobs: their default is all the menu offers.
            Some(a @ Action::Settings(views::settings::SAct::Slider(..) | views::settings::SAct::ThemeKnob(_))) => {
                if let Some(c) = crate::knobs::Control::of(a) {
                    self.state.playlist.popup = Some(crate::knobs::reset_menu(c, x, y));
                }
                return self.invalidate();
            }
            // Settings > MIDI's Port and offset boxes: right-click resets (spec: ui/midi-settings.md §1).
            Some(Action::Settings(views::settings::SAct::Midi(m))) if m.is_value() => return self.midi_page_reset(*m),
            Some(Action::PatternNext) => {
                self.state.pat_ui.menu_at = Some((x, y));
                self.state.menu_open = Some(views::menus::PATTERNS);
                return self.invalidate();
            }
            Some(Action::PatternAdd) => {
                let at = views::band_rect(&self.state, Rect::new(crate::patterns::PLUS_BOX_AT.0, crate::patterns::PLUS_BOX_AT.1, 0.0, 0.0));
                crate::patterns::run(&mut self.state, crate::patterns::Cmd::FindNext, (at.x, at.y));
                return self.invalidate();
            }
            Some(Action::CpuPanel) => {
                self.cpu_menu(x, y);
                return self.invalidate();
            }
            Some(Action::Metronome) => {
                self.metronome_menu(x, y);
                return self.invalidate();
            }
            Some(Action::Record) => {
                self.record_menu(x, y);
                return self.invalidate();
            }
            Some(Action::RecOpt(o)) => {
                self.record_option_menu(*o, x, y);
                return self.invalidate();
            }
            // The Limit dialog's keys take the right button too (spec: ui/piano-limit.md §1).
            Some(Action::PianoDlg(crate::piano_dlg::Act::Keys)) => return self.piano_dlg_act(crate::piano_dlg::Act::Keys),
            // The event editor's right button draws a line (spec: ui/event-editor.md §3).
            Some(Action::Events(crate::events::EvAct::Graph)) => return self.events_press(true, false),
            Some(Action::PianoTarget) => {
                // spec: ui/piano-target-channel.md §4
                let row = self.state.selected_channel;
                if row < self.state.project.channels.len() {
                    self.state.playlist.popup = None;
                    self.toggle_channel_window(row);
                }
                return self.invalidate();
            }
            Some(Action::PianoSnapMenu | Action::GlobalSnap) => {
                // spec: ui/piano-snap.md §7, ui/global-snap.md §4
                self.state.playlist.popup = Some(crate::piano_snap::lengths_menu(x, y));
                return self.invalidate();
            }
            Some(Action::AutoScroll) => {
                self.autoscroll_menu(x, y);
                return self.invalidate();
            }
            Some(Action::SongPos) => return self.song_pos_jump(),
            Some(Action::Clock) => {
                self.state.playlist.popup = Some(views::menus::clock_popup(&self.state, x, y));
                return self.invalidate();
            }
            // The Toolbars menu: on the hint panel and on bare toolbar background (spec: ui/toolbar-menu.md §1).
            Some(Action::DragWindow | Action::Bar(crate::toolbar::BarHit::Item(..) | crate::toolbar::BarHit::Grip(_))) => {
                self.state.bar.user = crate::toolbar::user_presets();
                self.state.playlist.popup = Some(views::menus::hint_popup(&self.state, x, y));
                return self.invalidate();
            }
            // Toolbar buttons with a right-click action: armed here, run on the release (spec: ui/toolbar-buttons.md §2).
            Some(a) if toolbar_right(a).is_some() => {
                self.state.playlist.popup = None;
                self.armed_right = toolbar_right(a).map(|run| (a.clone(), run));
                return self.invalidate();
            }
            Some(Action::BrowserRow(i)) => {
                if self.params_browser_right(*i, x, y) || self.browser_row_menu(*i, x, y) {
                    return;
                }
            }
            _ => {}
        }
        if self.state.playlist.popup.is_some() && !matches!(action, Some(Action::PlPopup(_) | Action::PlPopupSub(_))) {
            self.state.playlist.popup = None;
            self.invalidate();
            if !matches!(action, Some(Action::PlaylistArea)) {
                return;
            }
        }
        if self.state.playlist.edit.is_some() {
            crate::playlist::commit_edit(&mut self.state);
        }
        self.raise_under_mouse();
        if let Some(Action::Scrollbar(t)) = action {
            return self.scrollbar_press(t, true);
        }
        if let Some(a) = action.as_ref() {
            if self.rack_right(a) || self.container_right(a) {
                return self.invalidate();
            }
        }
        if let Some(Action::Mix(h)) = action {
            if crate::mixer::right(self, h) {
                return self.invalidate();
            }
        }
        // Knobs, faders and the tempo: "Create automation clip" (spec: ui/playlist.md §8) and "Enter value..."
        // (spec: ui/type-in-value.md §8).
        if let Some(a) = action.as_ref() {
            let typein = match *a {
                Action::Knob(k) => Some(crate::typein::Target::Knob(k)),
                Action::Fader(t) => Some(crate::typein::Target::Fader(t)),
                _ => None,
            };
            let auto = self.automation_target(a);
            if auto.is_some() || typein.is_some() {
                self.state.playlist.popup = Some(crate::playlist::knob_menu(&self.state, auto, typein, x, y));
                return self.invalidate();
            }
            if self.playlist_act(a, true) {
                return;
            }
        }
        // Right-click the monitor: open the debug panel; on a debug slider: back to its default.
        // Monitor debug panel (views/mondebug.rs) parked 2026-10-09: its values are now the defaults. Uncomment to tune again.
        // if action == Some(Action::Monitor) {
        //     self.state.monitor.debug = !self.state.monitor.debug;
        //     return self.invalidate();
        // }
        if let Some(Action::MonParam(i)) = action {
            let d = crate::monitor::PARAMS[i].default;
            self.state.monitor.cfg.set(i, d);
            return self.damage_rect(views::mondebug::PANEL);
        }
        if let Some(Action::CableDebug(crate::cable::DebugAct::Param(i))) = action {
            self.state.cables.get_mut().set(i, crate::cable::PARAMS[i].default);
            self.damage_rect(views::cabledebug::PANEL);
            return self.damage_win(Win::Mixer);
        }
        if action == Some(Action::PianoRuler) {
            if let Some(g) = self.piano_geom() {
                let t = crate::piano_snap::place(&self.state, &g, g.tick_at(x), self.alt);
                self.state.piano.clear_selection();
                self.drag = Some(Drag::TimeSel { t0: t });
                self.damage_piano();
            }
            return;
        }
        if action == Some(Action::PianoVelocity) && self.state.piano.lane.is_channel() {
            piano::snapshot(&mut self.state);
            self.drag = Some(Drag::ControlErase { last_x: x });
            self.erase_control_points(x, x);
            return;
        }
        if let Some(a @ (Action::PianoNote(_) | Action::PianoGrid)) = action {
            // Select: same as the left button; Zoom: toggle zoomed out / last zoom; the other
            // tools delete while the right button is held (spec: ui/piano-roll.md §9).
            match self.state.piano.tool {
                Tool::Select => self.piano_click(&a),
                Tool::Zoom => self.zoom_toggle(),
                _ => {
                    piano::snapshot_as(&mut self.state, "Piano roll delete note");
                    self.drag = Some(Drag::NoteErase);
                    self.erase_at(x, y);
                }
            }
        }
    }

    pub fn mouse_up(&mut self) {
        self.hold = None;
        self.shift_clone = false;
        self.release();
        // An undo step whose press changed nothing goes (spec: ui/settings-choices.md "Undo history").
        crate::history::settle(&mut self.state);
    }

    fn release(&mut self) {
        // A held nudge ends with the button (spec: ui/tempo-tapper.md §6).
        if crate::tapper::nudge_flag().swap(0, std::sync::atomic::Ordering::Relaxed) != 0 {
            self.damage_win(Win::Tapper);
        }
        // The pressed button's window comes to the front, before the button acts (it may close the window).
        if let Some((x, y)) = self.raise_at.take() {
            self.raise_at_point(x, y);
        }
        // An armed button acts if the release is over it (`acts_on_release`).
        if let Some(a) = self.armed.take() {
            let (x, y) = self.mouse;
            if self.top_hit(x, y).is_some_and(|h| h.action == a) {
                self.act(a);
                self.rack_pending();
                self.invalidate();
            }
        }
        if let Some((hit, run)) = self.armed_right.take() {
            let (x, y) = self.mouse;
            if self.top_hit(x, y).is_some_and(|h| h.action == hit) {
                self.act(run);
                self.invalidate();
            }
        }
        let pressed = self.pressed.take();
        // A click without a drag did its work on the press (painted then): its release only lifts the pressed look.
        if self.drag.is_none() {
            let rect = pressed.as_ref().and_then(|a| self.hits.iter().rev().find(|h| &h.action == a)).map(|h| h.rect);
            match (pressed, rect) {
                (None, _) => {}
                (Some(_), Some(r)) => self.damage_rect(r.outset(3.0)),
                (Some(_), None) => self.invalidate(),
            }
            self.hold_key(None, false);
            if self.state.piano.overlay.take().is_some() {
                self.damage_piano();
            }
            return;
        }
        if pressed.is_some() {
            self.invalidate();
        }
        self.hold_key(None, false);
        if matches!(self.drag, Some(Drag::Velocity { .. })) {
            self.state.piano.dirty = true;
        }
        if let Some(Drag::Browser { item, started, .. }) = self.drag.clone() {
            self.drag = None;
            return self.browser_drop(item, started);
        }
        if let Some(Drag::Pl(d)) = self.drag.clone() {
            self.drag = None;
            if let Some(g) = self.pl_grid {
                crate::playlist::drag_end(self, d, g);
            }
            return self.invalidate();
        }
        let had_overlay = self.state.piano.overlay.take().is_some();
        match self.drag.clone() {
            Some(Drag::Slice) => {
                if let Some(g) = self.piano_geom() {
                    let t = crate::piano_snap::place(&self.state, &g, g.tick_at(self.mouse.0), self.alt);
                    if piano::slice(&mut self.state, t) > 0 {
                        self.state.piano.dirty = true;
                    }
                }
            }
            Some(Drag::ZoomBox { x0, y0 }) => self.zoom_to(x0, y0),
            Some(Drag::SongPos { .. }) => self.song_pos_release(),
            Some(Drag::Bar(d)) => self.bar_drop(d),
            Some(Drag::PianoSplit { .. }) => crate::piano_split::save(self.state.piano.split),
            Some(Drag::BrowserSplit { .. }) => crate::browser::save_width(self.state.browser_w),
            Some(Drag::Marker) if self.transport.playing() && !self.transport.song() => {
                self.start_playback();
            }
            Some(Drag::SongRuler) => self.song_pos_release(),
            Some(Drag::Touch) => self.touch_hold(None),
            Some(Drag::Events(st)) => self.events_release(st),
            Some(Drag::BarEdge { .. }) => self.state.piano.bar_span = None,
            Some(Drag::ThemeKnob { .. } | Drag::PickSv(_) | Drag::PickHue(_)) => self.theme_store(),
            _ => {}
        }
        if had_overlay {
            self.damage_piano();
        }
        if matches!(self.drag, Some(Drag::NoteMove { .. } | Drag::NoteResize { .. } | Drag::NoteErase | Drag::NoteMute { .. } | Drag::NotePaint { .. })) {
            piano::finish_edit(&mut self.state);
            self.state.piano.dirty = true;
            self.damage_piano();
            self.drag = None;
            return;
        }
        self.invalidate();
        self.drag = None;
    }

    /// Move (`resize == false`) or resize the dragged notes from their originals; the distance follows the snap
    /// (spec: ui/piano-snap.md §5, §8), measured from the grabbed note's start, or its end for a resize.
    pub(crate) fn drag_notes(&mut self, x0: f32, y0: f32, orig: &[(usize, Note)], resize: bool) {
        let Some(g) = self.piano_geom() else { return };
        let (x, y) = self.mouse;
        let unit = crate::piano_snap::unit(&self.state).unwrap_or(1);
        let from = orig.first().map_or(0, |&(_, o)| if resize { o.position + o.length.max(unit) } else { o.position });
        let skip: Vec<usize> = orig.iter().map(|&(i, _)| i).collect();
        let mut dt = crate::piano_snap::delta(&self.state, &g, from, (x - x0) / g.ppt, self.alt, &skip);
        let mut held = None;
        let was_held = self.state.piano.held;
        let rects = |s: &State| -> Vec<Rect> {
            let notes = piano::notes(s);
            orig.iter().filter_map(|&(i, _)| notes.get(i).and_then(|n| g.note_rect(n))).collect()
        };
        let mut dirty = rects(&self.state);
        let s = &mut self.state;
        let p = piano::pattern_mut(s);
        if resize {
            for &(i, o) in orig {
                if let Some(n) = p.notes.get_mut(i) {
                    // The shortest note a resize leaves is one unit (spec: ui/piano-snap.md §8).
                    let len = i64::from(o.length.max(unit)) + dt;
                    n.length = len.max(i64::from(unit)) as u32;
                }
            }
            let first = orig.first().and_then(|&(i, _)| p.notes.get(i)).map(|n| n.length);
            if let Some(len) = first {
                s.piano.last_len = len;
            }
        } else {
            // Shift held keeps the notes on their keys (spec: ui/piano-roll.md §8).
            let dk = if self.shift { 0 } else { (g.key_at(y) - g.key_at(y0)).clamp(-131, 131) };
            // Keep every note at or after tick 0 and inside the key range.
            let min_pos = orig.iter().map(|(_, o)| i64::from(o.position)).min().unwrap_or(0);
            dt = dt.max(-min_pos);
            let lo = orig.iter().map(|(_, o)| i32::from(o.key)).min().unwrap_or(0);
            let hi = orig.iter().map(|(_, o)| i32::from(o.key)).max().unwrap_or(0);
            let dk = dk.clamp(-lo, views::piano::TOP_KEY - hi);
            // The grabbed note (first) is auditioned again whenever its key changes (§8).
            if let Some(&(_, o)) = orig.first() {
                let k = i32::from(o.key) + dk;
                if was_held != Some(k) {
                    held = Some(k);
                }
            }
            for &(i, o) in orig {
                if let Some(n) = p.notes.get_mut(i) {
                    n.position = (i64::from(o.position) + dt) as u32;
                    n.key = (i32::from(o.key) + dk) as u16;
                }
            }
        }
        dirty.extend(rects(&self.state));
        self.damage_notes(&dirty);
        if held.is_some() {
            let play = self.audition();
            self.hold_key(held, play);
        }
    }

    /// Set the key held with the mouse (lit on the keyboard). With `play` it sounds as a live note, like a typing-keyboard
    /// key, until another key is held or the button is released (§8).
    /// Notes clicked or dragged in the piano roll are auditioned, except while playback runs (it is heard already).
    fn audition(&self) -> bool {
        !(self.transport.playing() && !self.transport.paused())
    }

    /// Velocity of a click on the piano roll's keyboard at `x`: soft at the keys' left end, loud at their tips (as FL).
    fn key_click_velocity(&self, x: f32) -> u8 {
        let Some(g) = self.piano_grid else { return TYPING_VELOCITY };
        let (left, w) = (g.x - 73.0, 71.0);
        (1.0 + 126.0 * ((x - left) / w).clamp(0.0, 1.0)).round() as u8
    }

    fn hold_key(&mut self, key: Option<i32>, play: bool) {
        if self.state.piano.held == key {
            return;
        }
        self.state.piano.held = key;
        if let Some((row, k)) = self.mouse_note.take() {
            self.transport.note_off(row, k);
        }
        let row = piano::channel(&self.state).and_then(|ch| self.state.project.channels.iter().position(|c| c.index == ch));
        if let (Some(k), true, Some(row)) = (key, play, row) {
            let k = k.clamp(0, views::piano::TOP_KEY) as u16;
            self.sync_live();
            self.transport.note_on(row, k, std::mem::replace(&mut self.key_velocity, TYPING_VELOCITY));
            self.mouse_note = Some((row, k));
        }
        // The keyboard column.
        match self.piano_grid {
            Some(g) => self.damage_rect(Rect::new(g.x - 73.0, g.y, 73.0, g.h)),
            None => self.invalidate(),
        }
    }

    /// Damage note rects (old and new) and the velocity-panel columns under them. The overview
    /// mini-map is refreshed when the drag ends.
    fn damage_notes(&mut self, rects: &[Rect]) {
        let (Some(grid), Some(panel)) = (self.piano_grid, self.piano_panel) else { return self.invalidate() };
        let Some(first) = rects.first() else { return };
        let u = rects.iter().fold(*first, |a, r| a.union(r)).outset(2.0).intersect(&grid);
        if u.w > 0.0 && u.h > 0.0 {
            self.damage_rect(u);
        }
        let (x0, x1) = rects.iter().fold((f32::MAX, f32::MIN), |(a, b), r| (a.min(r.x), b.max(r.right())));
        self.damage_rect(Rect::new(x0 - 6.0, panel.y, x1 - x0 + 12.0, panel.h).intersect(&panel.outset(1.0)));
    }

    /// Delete this channel's note under (x, y), if any.
    fn erase_at(&mut self, x: f32, y: f32) {
        let Some(g) = self.piano_geom() else { return };
        if !g.grid.contains(x, y) {
            return;
        }
        let ch = piano::channel(&self.state);
        let hit = piano::notes(&self.state)
            .iter()
            .enumerate()
            .rev()
            .find(|(_, n)| Some(n.channel) == ch && g.note_rect(n).is_some_and(|r| r.contains(x, y)))
            .map(|(i, _)| i);
        if let Some(i) = hit {
            piano::delete(&mut self.state, vec![i]);
            self.state.piano.dirty = true;
            self.damage_piano();
        }
    }

    /// Mute tool: give the note under (x, y) the mute state `on`.
    fn mute_at(&mut self, x: f32, y: f32, on: bool) {
        let Some(g) = self.piano_geom() else { return };
        if !g.grid.contains(x, y) {
            return;
        }
        let Some(i) = piano::note_at(&self.state, &g, x, y) else { return };
        let p = piano::pattern_mut(&mut self.state);
        if p.notes[i].muted() != on {
            p.notes[i].set_muted(on);
            let r = g.note_rect(&p.notes[i]);
            self.state.piano.dirty = true;
            match r {
                Some(r) => self.damage_notes(&[r]),
                None => self.damage_piano(),
            }
        }
    }

    /// Paint tool: fill `key`'s row with notes `len` apart from `start` up to the mouse (either
    /// direction), skipping slots that already hold a note of this channel on that key. Each note laid deletes the
    /// other keys' notes under it with "Monophonic painting in piano roll" on (`piano::paint_mono`).
    fn paint_notes(&mut self, key: i32, start: u32, len: u32) {
        let (Some(g), Some(ch)) = (self.piano_geom(), piano::channel(&self.state)) else { return };
        let len = len.max(1);
        let t = g.tick_at(self.mouse.0).max(0.0) as i64;
        let k = (t - i64::from(start)).div_euclid(i64::from(len));
        let (a, b) = (k.min(0), k.max(0));
        let (mut added, mut deleted) = (Vec::new(), false);
        for j in a..=b {
            let pos = i64::from(start) + j * i64::from(len);
            if pos < 0 {
                continue;
            }
            let pos = pos as u32;
            let taken = piano::notes(&self.state).iter().any(|n| n.channel == ch && i32::from(n.key) == key && n.position == pos);
            if !taken {
                let i = piano::add_note(&mut self.state, ch, pos, key);
                piano::pattern_mut(&mut self.state).notes[i].length = len;
                let before = piano::notes(&self.state).len();
                let i = piano::paint_mono(&mut self.state, i);
                deleted |= piano::notes(&self.state).len() < before;
                added.push(piano::notes(&self.state)[i]);
            }
        }
        if deleted {
            self.damage_piano();
        } else if !added.is_empty() {
            self.state.piano.dirty = true;
            let rects: Vec<Rect> = added.iter().filter_map(|n| g.note_rect(n)).collect();
            self.damage_notes(&rects);
        }
    }

    /// Playback tool: sound the notes the scrub line enters between ticks `from` and `to`.
    fn scrub(&mut self, from: f32, to: f32) {
        let Some(ch) = piano::channel(&self.state) else { return };
        let (a, b) = (from.min(to), from.max(to));
        let mut keys: Vec<i32> = piano::notes(&self.state)
            .iter()
            .filter(|n| n.channel == ch && !n.muted())
            .filter(|n| {
                let p = n.position as f32;
                let end = p + piano::shown_len(&self.state, n) as f32;
                if from == to {
                    // Press: every note the line touches sounds.
                    p <= to && to < end
                } else {
                    // Drag: notes whose start the line just passed.
                    p > a && p <= b
                }
            })
            .map(|n| i32::from(n.key))
            .collect();
        keys.sort_unstable();
        keys.dedup();
        for k in keys {
            self.preview(ch, k);
        }
    }

    /// Zoom tool: zoom the time axis to the dragged box (a click zooms in 2x around the mouse).
    fn zoom_to(&mut self, x0: f32, y0: f32) {
        let Some(g) = self.piano_geom() else { return };
        let (x1, y1) = self.mouse;
        let p = &mut self.state.piano;
        let back = (p.px_per_beat, p.scroll_x, p.scroll_y);
        let ppq = f32::from(self.state.project.ppq.max(4));
        if (x1 - x0).abs() < 4.0 {
            let tick = g.tick_at(x1);
            p.px_per_beat = (p.px_per_beat * 2.0).min(1600.0);
            p.scroll_x = (tick - (x1 - g.grid.x) * ppq / p.px_per_beat).max(0.0);
        } else {
            let (t0, t1) = (g.tick_at(x0.min(x1)).max(0.0), g.tick_at(x0.max(x1)).max(0.0));
            p.px_per_beat = (g.grid.w / ((t1 - t0).max(1.0) / ppq)).clamp(6.0, 1600.0);
            p.scroll_x = t0;
            p.scroll_y = (p.scroll_y + y0.min(y1) - g.grid.y).max(0.0);
        }
        p.zoom_back = Some(back);
        piano::clamp_scroll(&mut self.state, g.grid.h);
        self.invalidate();
    }

    /// Zoom tool right-click: fully zoomed out (the normal 96 px/beat view) <-> the last zoom.
    fn zoom_toggle(&mut self) {
        let Some(g) = self.piano_geom() else { return };
        let p = &mut self.state.piano;
        let normal = PianoState::default().px_per_beat;
        if (p.px_per_beat - normal).abs() > 0.01 {
            p.zoom_back = Some((p.px_per_beat, p.scroll_x, p.scroll_y));
            p.px_per_beat = normal;
            p.scroll_x = 0.0;
        } else if let Some((z, sx, sy)) = p.zoom_back {
            p.zoom_back = Some((p.px_per_beat, p.scroll_x, p.scroll_y));
            p.px_per_beat = z;
            p.scroll_x = sx;
            p.scroll_y = sy;
        }
        piano::clamp_scroll(&mut self.state, g.grid.h);
        self.invalidate();
    }

    /// Where Ctrl+V pastes (§9): the playhead when it is in view, else the left edge of the view.
    /// Delete the channel lane's control points between two x positions.
    fn erase_control_points(&mut self, last_x: f32, x: f32) {
        let (Some(g), Some(panel)) = (self.piano_geom(), self.piano_panel) else { return };
        let lane = self.state.piano.lane;
        let (Some(ch), Some(param)) = (piano::channel(&self.state), lane.param()) else { return };
        let (a, b) = (last_x.min(x), last_x.max(x));
        let from = crate::piano_snap::cell(&self.state, g.tick_at(a));
        let to = crate::piano_snap::cell(&self.state, g.tick_at(b));
        if piano::erase_controls(&mut self.state, ch, param, from, to) > 0 {
            self.damage_rect(panel);
        }
    }

    /// Set the event panel's property of this channel's notes whose start lies between the last and
    /// current x; in a channel lane, set a control point at every snap step the cursor path crosses.
    fn paint_velocity(&mut self, last_x: f32, last_y: f32, x: f32, y: f32) {
        let (Some(g), Some(panel)) = (self.piano_geom(), self.piano_panel) else { return };
        let lane = self.state.piano.lane;
        if lane.is_channel() {
            let (Some(ch), Some(param)) = (piano::channel(&self.state), lane.param()) else { return };
            let center = self.state.piano.pitch_center;
            let t0 = crate::piano_snap::cell(&self.state, g.tick_at(last_x));
            let t1 = crate::piano_snap::cell(&self.state, g.tick_at(x));
            // A point is set when the cursor enters a snap unit (its value is the path height at the
            // unit's start, as in FL's captured ramps) and on the press; staying inside a unit keeps it.
            // Without a unit, one point at the pointer (spec: ui/piano-snap.md §8).
            let (lo, hi, step) = match crate::piano_snap::unit(&self.state) {
                Some(u) => (t0.min(t1), t0.max(t1), u),
                None => (t1, t1, 1),
            };
            let press = last_x == x && last_y == y;
            let mut t = lo;
            loop {
                if t != t0 || (t0 == t1 && press) {
                    let px = g.x_of(t as f32);
                    let at = if (x - last_x).abs() < 0.5 { 1.0 } else { ((px - last_x) / (x - last_x)).clamp(0.0, 1.0) };
                    let y_at = last_y + (y - last_y) * at;
                    piano::set_control(&mut self.state, ch, param, t, lane.control_at(panel, y_at, center));
                }
                if t >= hi {
                    break;
                }
                t += step;
            }
            self.damage_rect(panel);
            return;
        }
        let v = lane.value_at(panel, y);
        let (ta, tb) = (g.tick_at(last_x.min(x)), g.tick_at(last_x.max(x)));
        // The notes the cursor's path covers: those starting inside it, and the note already playing where the path
        // begins (the latest start at or before it; before the first note, the first one), so a click anywhere above
        // or below a note's line sets it. A selection limits this to the selected notes (FL's manual: select first to
        // reach one of several notes that start together).
        let cand = piano::targets(&self.state);
        let starts = || cand.iter().filter_map(|&i| piano::notes(&self.state).get(i)).map(|n| n.position);
        let anchor = starts().filter(|&p| p as f32 <= ta).max().or_else(|| starts().min());
        let hit: Vec<usize> = cand
            .iter()
            .copied()
            .filter(|&i| piano::notes(&self.state).get(i).is_some_and(|n| Some(n.position) == anchor || (n.position as f32 >= ta && n.position as f32 <= tb)))
            .collect();
        let p = piano::pattern_mut(&mut self.state);
        let mut span: Option<(f32, f32)> = None;
        let mut edited = None;
        for (i, n) in p.notes.iter_mut().enumerate() {
            let nx = g.x_of(n.position as f32);
            if hit.contains(&i) {
                lane.set(n, v);
                edited = Some(*n);
                let end = g.x_of((n.position + n.length.max(g.step)) as f32);
                span = Some(span.map_or((nx, end), |(l, r)| (l.min(nx), r.max(end))));
            }
        }
        // The next notes placed take the edited note's controls.
        if edited.is_some() {
            self.state.piano.last_controls = edited;
        }
        // Only the panel columns of the changed lollipops (the hint is left alone: a damage
        // union with the hint panel would cover most of the window).
        if let Some((l, r)) = span {
            self.damage_rect(Rect::new(l - 6.0, panel.y, r - l + 12.0, panel.h).intersect(&panel.outset(1.0)));
        }
    }

    pub fn wheel(&mut self, dx: f32, dy: f32, shift: bool, ctrl: bool) {
        if self.modal_open() {
            return self.dialog_wheel(dy);
        }
        if self.menu_wheel(dx, dy, shift) {
            return;
        }
        // Scrolling or zooming one window repaints just that window; scrolling the playlist moves its pixels.
        let before = self.damage.clone();
        let s = &self.state;
        let view = (s.playlist_scroll, s.playlist_bar_w, s.playlist.row_h, s.playlist.pick_scroll);
        self.invalidate();
        // A scroll eases to where the wheel sends it (General ▸ Display ▸ Smoothing, `crate::smooth`).
        if let Some(w) = self.eased(|app| app.wheel_act(dx, dy, shift, ctrl)) {
            self.damage = before;
            let s = &self.state;
            if w == Win::Playlist && (s.playlist_bar_w, s.playlist.row_h, s.playlist.pick_scroll) == (view.1, view.2, view.3) {
                self.playlist_scrolled(view.0);
            } else {
                self.damage_win(w);
            }
        }
    }

    /// The wheel over an open menu, as in FL (spec: ui/menus.md "The wheel in menus"): each notch lights the next row
    /// down (or up) that can be picked, skipping separators, headings and greyed-out rows, on into the next column and
    /// round from the last row to the first; the cursor moves to the row's middle. Returns whether a menu was open (it
    /// takes the wheel).
    ///
    /// A popup list wider than the window (spec: ui/menus.md "Menus wider than the window") scrolls to show the row lit
    /// in a column out of view; sideways (Shift, a sideways wheel) or over its scroll bar, the wheel scrolls it a column
    /// a notch instead.
    fn menu_wheel(&mut self, dx: f32, dy: f32, shift: bool) -> bool {
        if self.state.playlist.popup.is_none() && self.state.menu_open.is_none() {
            self.menu_notch = 0.0;
            return false;
        }
        self.menu_notch += dy + dx;
        let notches = self.menu_notch.trunc();
        self.menu_notch -= notches;
        let (x, y) = self.mouse;
        let bar = match self.top_hit(x, y).map(|h| &h.action) {
            Some(Action::Scrollbar(t @ (Scroll::Popup | Scroll::PopupSub))) => Some(*t),
            _ => None,
        };
        // The menu under the cursor; else the popup, else the dropdown (a submenu only under the cursor).
        let under = self.hits.iter().rev().find(|h| h.menu > 0 && h.rect.contains(x, y)).map(|h| h.menu);
        let Some(menu) = under.or_else(|| self.hits.iter().map(|h| h.menu).filter(|&m| m > 0 && m != MENU_SUB).max()) else { return true };
        // The list's scroll, when it is wider than the window.
        let list = bar
            .or(match menu {
                MENU_POPUP => Some(Scroll::Popup),
                MENU_SUB => Some(Scroll::PopupSub),
                _ => None,
            })
            .filter(|&t| self.bar(t).is_some_and(|b| b.max > 0.0));
        if let Some(t) = list.filter(|_| bar.is_some() || shift || dx.abs() > dy.abs()) {
            if notches != 0.0 {
                self.scroll_set(t, self.scroll_get(t) - notches);
                self.invalidate();
            }
            return true;
        }
        let rows: Vec<(Rect, Option<usize>, &Action)> = self.hits.iter().filter(|h| h.menu == menu).map(|h| (h.rect, h.reveal, &h.action)).collect();
        let n = rows.len();
        let mut at = rows.iter().position(|(r, ..)| r.contains(x, y));
        for _ in 0..notches.abs() as usize {
            // Up is positive; from off the rows the first notch down lights the first row, up the last.
            at = Some(match (at, notches < 0.0) {
                (Some(i), true) => (i + 1) % n,
                (Some(i), false) => (i + n - 1) % n,
                (None, true) => 0,
                (None, false) => n - 1,
            });
        }
        let Some((r, reveal, action)) = at.map(|i| rows[i]).filter(|_| notches != 0.0) else { return true };
        let action = action.clone();
        // The cursor keeps its x over the row's column, else goes three quarters of the way across it.
        let to = (if x > r.x && x < r.right() { x } else { r.x + r.w * 0.75 }, r.y + r.h / 2.0);
        self.warp = Some(to);
        let reveal = reveal.zip(list);
        if let Some((v, t)) = reveal {
            self.scroll_set(t, v as f32);
            self.invalidate();
        }
        self.mouse_move(to.0, to.1);
        if reveal.is_some() {
            // Its column scrolled into view: the row is under the cursor from the next frame on (these hits are stale).
            self.hover = Some(action);
        }
        true
    }

    /// The wheel's work; returns the window when only that window's view moved (else everything is repainted).
    fn wheel_act(&mut self, dx: f32, dy: f32, shift: bool, ctrl: bool) -> Option<Win> {
        let (x, y) = self.mouse;
        let area = self.top_hit(x, y).map(|h| h.action.clone());
        if matches!(area, Some(Action::BrowserList | Action::BrowserRow(_) | Action::BrowserTab(_))) {
            self.browser_wheel(dy + dx);
            return None;
        }
        // The Limit dialog's keyboard: a white key a notch, up toward the high keys (spec: ui/piano-limit.md §5).
        if let Some(Action::PianoDlg(crate::piano_dlg::Act::Keys | crate::piano_dlg::Act::Ruler | crate::piano_dlg::Act::Scroll)) = area {
            let d = &mut self.state.piano_dlg;
            d.kb_scroll = (d.kb_scroll + (dy + dx) * crate::piano_dlg::KEY_W).round().clamp(0.0, crate::piano_dlg::kb_max());
            self.invalidate();
            return None;
        }
        // The event editor: time scrolls, Ctrl zooms (spec: ui/event-editor.md §3).
        if let Some(Action::Events(crate::events::EvAct::Graph | crate::events::EvAct::Ruler)) = area {
            self.events_wheel(dy + dx, ctrl);
            return None;
        }
        if matches!(area, Some(Action::Settings(views::settings::SAct::Plug(views::settings::Plug::Table | views::settings::Plug::Row(_) | views::settings::Plug::Star(_) | views::settings::Plug::Show(_)))) | Some(Action::Scrollbar(Scroll::PlugTable))) {
            let n = self.state.plugdb.rows.len();
            let top = self.state.plugdb.top as f32 - (dy + dx) * 3.0;
            self.state.plugdb.top = (top.max(0.0) as usize).min(n.saturating_sub(views::settings::TABLE_ROWS));
            return None;
        }
        // General ▸ Input's scrolling speeds: a notch moves a slider by its step, up for more (spec:
        // ui/settings-choices.md "Input: Vertical and Horizontal scrolling speed").
        if let Some(Action::Settings(views::settings::SAct::Slider(label, ..))) = area {
            if dy + dx != 0.0 {
                self.pref_slider_wheel(label, if dy + dx > 0.0 { 1 } else { -1 });
            }
            return None;
        }
        // General ▸ Display's PPI ruler: 1 PPI a notch, up for more (spec: ui/settings-choices.md "Display: PPI ruler,
        // Force refreshes, Thick lines, High visibility (2026-10-16)").
        // Theme ▸ Adjustments: a knob steps its value, up for more (`crate::theme_adjust`).
        if let Some(Action::Settings(views::settings::SAct::ThemeKnob(i))) = area {
            if dy + dx != 0.0 {
                self.theme_knob_wheel(usize::from(i), if dy + dx > 0.0 { 1 } else { -1 });
            }
            return None;
        }
        if let Some(Action::Settings(views::settings::SAct::Ppi)) = area {
            if dy + dx != 0.0 {
                self.ppi_wheel(if dy + dx > 0.0 { 1 } else { -1 });
            }
            return None;
        }
        // Project ▸ a time division's value box: one per notch, up for more (spec: ui/settings-choices.md "Set as time
        // division").
        if let Some(Action::Remote(r)) = area {
            if dy + dx != 0.0 && r.is_value() {
                self.link_box_step(r, if dy + dx > 0.0 { 1 } else { -1 });
            }
            return None;
        }
        if let Some(Action::Settings(views::settings::SAct::Midi(m))) = area {
            if dy + dx != 0.0 {
                self.midi_page_wheel(m, if dy + dx > 0.0 { 1 } else { -1 });
            }
            return None;
        }
        if let Some(Action::Settings(views::settings::SAct::Number(label))) = area {
            if dy + dx != 0.0 {
                if let Some(m) = self.settings_number(label, if dy + dx > 0.0 { 1 } else { -1 }) {
                    self.state.message = Some(m);
                }
            }
            return None;
        }
        if matches!(area, Some(Action::Settings(_) | Action::Scrollbar(Scroll::Settings))) {
            let s = &self.state;
            let max = (views::settings::content_h(s) - views::settings::page_h(s)).max(0.0);
            self.state.settings.scroll = (self.state.settings.scroll - (dy + dx) * 63.0).min(max).max(0.0);
            return None;
        }
        if matches!(area, Some(Action::Perf(crate::perf::PerfAct::List | crate::perf::PerfAct::Row(_)))) {
            self.perf_wheel(dy + dx);
            return Some(Win::Perf);
        }
        if matches!(area, Some(Action::SongPos)) {
            self.song_pos_wheel(dy);
            return None;
        }
        if matches!(area, Some(Action::PianoTarget)) && dy != 0.0 {
            // spec: ui/piano-target-channel.md §3
            crate::piano_tools::cycle_channel(self, if dy < 0.0 { 1 } else { -1 }, true);
            return Some(Win::PianoRoll);
        }
        if matches!(area, Some(Action::GlobalSnap)) && dy != 0.0 {
            // One setting per notch, down toward Bar (spec: ui/global-snap.md §1).
            let notches = (-dy).round() as i32;
            let notches = if notches == 0 { -(dy.signum() as i32) } else { notches };
            self.state.message = Some(crate::snap::wheel(&mut self.state, notches));
            return None;
        }
        if matches!(area, Some(Action::PlPick(_) | Action::PlPickList)) {
            self.scroll_set(Scroll::PlPick, self.scroll_get(Scroll::PlPick) - (dy + dx) * self.state.playlist.pick_row());
            return Some(Win::Playlist);
        }
        if matches!(area, Some(Action::PatternNext)) {
            let d = if dy + dx > 0.0 { 1 } else { -1 };
            let n = (i32::from(self.state.pattern.max(1)) + d).clamp(1, i32::from(crate::patterns::MAX)) as u16;
            crate::patterns::select(&mut self.state, n);
            return None;
        }
        if let Some(Action::Scrollbar(t)) = area {
            let step = if t == Scroll::PianoY { self.state.piano.row_h * 3.0 } else { 60.0 };
            // An editor's bar scrolls at the scrolling speeds (`crate::scroll_speed`); the picker list keeps its step.
            let (dx, dy) = if shift || t == Scroll::PlPick { (dx, dy) } else { crate::scroll_speed::scaled(dx, dy) };
            self.scroll_set(t, self.scroll_get(t) - (dy + dx) * step);
            return scroll_win(t);
        }
        // Piano roll (spec: ui/piano-roll.md §3a): Alt+wheel over the ruler zooms time; the keyboard takes the wheel
        // like the grid (scroll, Ctrl = time zoom); the keys zoom with Ctrl+middle-drag.
        if matches!(area, Some(Action::PianoRuler)) && self.alt {
            let Some(g) = self.piano_geom() else { return None };
            piano::zoom_time(&mut self.state, &g, x, dy + dx);
            return Some(Win::PianoRoll);
        }
        if matches!(area, Some(Action::PianoGrid | Action::PianoNote(_) | Action::PianoKeys | Action::PianoVelocity)) {
            let Some(g) = self.piano_geom() else { return None };
            if ctrl {
                // Zoom time around the mouse.
                piano::zoom_time(&mut self.state, &g, x, dy + dx);
                piano::clamp_scroll(&mut self.state, g.grid.h);
                return Some(Win::PianoRoll);
            }
            let row = self.state.piano.row_h;
            let p = &mut self.state.piano;
            if shift {
                p.scroll_x = (p.scroll_x - (dy + dx) * 120.0 / g.ppt).max(0.0);
            } else {
                let (dx, dy) = crate::scroll_speed::scaled(dx, dy);
                p.scroll_y -= dy * row * 3.0;
                p.scroll_x = (p.scroll_x - dx * 120.0 / g.ppt).max(0.0);
            }
            piano::clamp_scroll(&mut self.state, g.grid.h);
            return Some(Win::PianoRoll);
        }
        if let Some(Action::Knob(id)) = area {
            let mut v = self.state.knob(id);
            if id == crate::knobs::KnobId::MasterPitch {
                v = (v * 24.0).round() / 24.0; // wheel steps from the nearest semitone
            }
            let v = v + dy.signum() * id.notch() * if dy == 0.0 { 0.0 } else { 1.0 };
            if v.clamp(0.0, 1.0) != self.state.knob(id) {
                crate::history::knob(&mut self.state, id, true);
            }
            self.state.set_knob(id, v);
            self.record_knob(id);
            self.multilink_knob(id.target());
            self.hint = Cow::Owned(self.state.knob_text(id));
            return None;
        }
        // The wheel over a mixer fader moves it, not the mixer's scroll (spec: ui/mixer.md §4).
        if let Some(Action::Fader(t)) = area
            && dy != 0.0
        {
            let s = &mut self.state;
            let v0 = crate::mixer::track(s, t).map(|m| m.volume.unwrap_or(0.8))?;
            let v = (v0 + f64::from(dy.signum()) * views::mixer::FADER_NOTCH).clamp(0.0, 1.0);
            if v != v0 {
                // One step for a run of notches on the fader (none with Undo knob tweaks off: a fader is a knob here).
                if crate::prefs::check(&crate::prefs::UNDO_KNOB_TWEAKS) && !s.history.joins(crate::history::Run::Fader(t)) {
                    crate::control::edit_press(s, crate::history::MIXER_VOLUME, crate::control::command::Region::Track(t));
                    s.history.set_run(crate::history::Run::Fader(t));
                }
                if let Some(m) = crate::mixer::track_mut(s, t) {
                    m.volume = Some(v);
                }
                s.piano.dirty = true;
                self.record_control(&Action::Fader(t));
            }
            self.hint = Cow::Owned(crate::mixer::level_text("Volume", v));
            return None;
        }
        // The wheel over a mixer effect slot moves its effect up or down the track's chain (spec: ui/mixer.md §7).
        if let Some(Action::Mix(crate::mixer::MHit::SlotName(k) | crate::mixer::MHit::SlotArrow(k))) = area
            && dy + dx != 0.0
        {
            let notches = (dy + dx).round() as i32;
            crate::mixer::slot_wheel(self, k, if notches == 0 { (dy + dx).signum() as i32 } else { notches });
            return None;
        }
        // Channel rack value boxes (spec: ui/channel-rack.md §7, §12, §15): one per notch.
        let d = if dy + dx > 0.0 { 1 } else { -1 };
        match area {
            Some(Action::RackMixer(i)) => {
                let max = crate::mixer::max_insert(&self.state.project);
                if let Some(now) = self.state.project.channels.get(i).map(|c| c.mixer_track) {
                    let v = (i32::from(now) + d).clamp(0, i32::from(max)) as u16;
                    self.channel_control(i, ChannelControl::Track, |c| c.mixer_track = v);
                    self.hint = Cow::Owned(format!("Target mixer track: {}", views::rack::insert_name(v)));
                    self.state.piano.dirty = true;
                }
                return None;
            }
            Some(Action::RackLen) => {
                let v = (crate::rack::len_value(&self.state) as i32 + d).clamp(0, crate::rack::max_steps(&self.state) as i32) as u32;
                crate::rack::set_len(&mut self.state, (v > 0).then_some(v));
                return None;
            }
            Some(Action::RackLoopBox(i)) => {
                let v = crate::rack::loop_box_value(&self.state, i) + d;
                crate::rack::set_loop_box(&mut self.state, i, v);
                return None;
            }
            // Shift+wheel over a channel's name button moves the channel (spec: ui/channel-rack.md §13).
            Some(Action::ChannelSelect(i)) if shift && dy + dx != 0.0 => {
                let notches = (dy + dx).round() as i32;
                crate::rack::wheel_move(self, i, if notches == 0 { d } else { notches });
                return None;
            }
            _ => {}
        }
        let rack_area = self.top_hit(x, y).is_some_and(|h| h.win == Some(Win::ChannelRack));
        if rack_area {
            let (rx, ry) = if shift { (dy + dx, 0.0) } else { crate::scroll_speed::scaled(dx, dy) };
            self.scroll_set(Scroll::RackY, self.scroll_get(Scroll::RackY) - ry * views::rack::ROW_H);
            self.scroll_set(Scroll::RackX, self.scroll_get(Scroll::RackX) - rx * 4.0 * views::rack::STEP_W);
            return Some(Win::ChannelRack);
        }
        let alt = self.alt;
        let pl_grid = self.pl_grid;
        let pl_area = matches!(
            area,
            Some(Action::PlaylistArea | Action::PlRuler | Action::PlMarkers | Action::PlHeader(_) | Action::PlLed(_) | Action::PlGroupArrow(_))
        );
        let s = &mut self.state;
        let (dx, dy) = if shift { (dy, dx) } else { (dx, dy) };
        // What the playlist and the mixer scroll by (`crate::scroll_speed`); zooms take the wheel's own steps.
        let (sx, sy) = if shift { (dx, dy) } else { crate::scroll_speed::scaled(dx, dy) };
        let row_h = s.playlist.row_h;
        match area {
            Some(Action::Fader(_) | Action::Mix(_) | Action::MixerArea) => {
                s.mixer_scroll = (s.mixer_scroll - (sy + sx) * 44.0).max(0.0);
                Some(Win::Mixer)
            }
            // Alt+wheel anywhere in the playlist: track height (spec: ui/playlist.md §3, §6).
            _ if pl_area && alt => {
                let grid = pl_grid.unwrap_or(Rect::new(481.0, 150.0, 1400.0, 800.0));
                crate::playlist::zoom_rows(s, grid, y, dy + dx);
                Some(Win::Playlist)
            }
            // Ctrl+wheel anywhere in the playlist, the plain wheel over the ruler: time zoom around the mouse (§3).
            _ if pl_area && (ctrl || (!shift && matches!(area, Some(Action::PlRuler | Action::PlMarkers)))) => {
                let gx = self.pl_grid.map_or(481.0, |g| g.x);
                let bar = (s.playlist_scroll.0 + x - gx) / s.playlist_bar_w;
                s.playlist_bar_w = (s.playlist_bar_w * 1.25f32.powf(dy + dx)).clamp(4.0, 600.0);
                s.playlist_scroll.0 = (bar * s.playlist_bar_w - (x - gx)).max(0.0);
                Some(Win::Playlist)
            }
            // Shift+wheel over the track headers moves the selected track (spec: ui/playlist.md §6).
            Some(Action::PlHeader(_) | Action::PlLed(_) | Action::PlGroupArrow(_)) if shift => {
                let d = if dx + dy > 0.0 { -1 } else { 1 };
                crate::playlist::snapshot(s);
                crate::playlist::shift_tracks(s, d);
                None
            }
            Some(Action::PlHeader(_) | Action::PlLed(_) | Action::PlGroupArrow(_)) => {
                s.playlist_scroll.1 = (s.playlist_scroll.1 - sy * row_h).max(0.0);
                Some(Win::Playlist)
            }
            Some(Action::PlaylistArea) => {
                s.playlist_scroll.0 = (s.playlist_scroll.0 - sx * 60.0).max(0.0);
                s.playlist_scroll.1 = (s.playlist_scroll.1 - sy * row_h).max(0.0);
                Some(Win::Playlist)
            }
            _ => None,
        }
    }

    pub fn key(&mut self, key: &winit::keyboard::Key, ctrl: bool, alt: bool) {
        self.invalidate();
        // The offline renderer runs on the transport thread. Esc requests cancellation;
        // export_tick still owns reporting the outcome and returning hosted plugins to
        // realtime mode, so don't report success or failure before it acknowledges the stop.
        if self.export.is_some() && !self.modal_open() && !ctrl && !alt
            && matches!(key, winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape))
        {
            self.transport.cancel_export();
            return;
        }
        // A dialog takes every key.
        if self.modal_open() {
            return self.dialog_key(key, ctrl, alt);
        }
        // So does a box of the settings window's Info page while it is edited, and a cell of File ▸ Browser extra
        // search folders.
        if self.info_key(key, ctrl, alt) || self.folder_cell_key(key, ctrl, alt) || self.name_key(key, ctrl, alt) {
            return;
        }
        use winit::keyboard::{Key, NamedKey};
        // The Remote control settings box takes Esc (drops it) and Enter (accepts).
        if crate::remote::is_open(&self.state) {
            match key {
                Key::Named(NamedKey::Escape) => self.state.link_box = None,
                Key::Named(NamedKey::Enter) => self.link_accept(),
                _ => {}
            }
            return;
        }
        // Ctrl+J: Multilink to controllers (spec: ui/remote-control.md §4).
        if let Key::Character(c) = key
            && ctrl
            && !alt
            && !self.shift
            && c.eq_ignore_ascii_case("j")
        {
            return self.toggle_multilink();
        }
        // A held key repeats only the piano roll's moving and copying keys and the browser's Up / Down; every other key
        // acts on its first press (switches would flicker on and off; spec: ui/piano-keys.md §4 rule 1).
        if self.repeat && !(self.piano_keys() && crate::piano_tools::repeats(key, ctrl, alt, self.shift)) && !self.browser_repeats(key, ctrl, alt) {
            return;
        }
        // Shift+Alt+G: Enable groups (spec: ui/enable-groups.md §1), everywhere the main window has the keyboard.
        if let Key::Character(c) = key
            && alt
            && self.shift
            && !ctrl
            && c.eq_ignore_ascii_case("g")
        {
            return self.toggle_groups();
        }
        // Alt+1 .. Alt+9, Alt+0: FILE's recent projects (spec: ui/recent-projects.md §2).
        if let Key::Character(c) = key
            && alt
            && !ctrl
            && !self.shift
            && let Some(n) = crate::recent::alt_digit(c)
        {
            return self.act(Action::MenuItem(0, views::menus::RECENT_PICK + n));
        }
        if self.plugin_find_key(key, ctrl) {
            return;
        }
        if self.state.perf.finding && self.perf_key(key, ctrl) {
            return;
        }
        if crate::patterns::key(&mut self.state, key, ctrl) {
            self.rack_pending();
            return;
        }
        if self.browser_key(key, ctrl, alt) {
            return;
        }
        // While Settings > Theme is open, Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y undo and redo theme changes
        // (`crate::theme_library`).
        if !alt && self.theme_undo_key(key, ctrl) {
            return self.invalidate();
        }
        // Ctrl+Z / Ctrl+Alt+Z: the project's undo history, whichever window is in front; Ctrl+Shift+Z redoes (spec:
        // ui/settings-choices.md "Undo history: the four checks (2026-10-16)").
        if let Key::Character(c) = key
            && ctrl
            && c.eq_ignore_ascii_case("z")
        {
            if self.shift {
                let m = crate::history::ctrl_shift_z(&mut self.state);
                self.say(m);
                return self.invalidate();
            }
            return self.undo_key(alt);
        }
        // The event editor's keys while it is in front (spec: ui/event-editor.md §4).
        let s = &self.state;
        if s.z.last() == Some(&Win::Events) && s.menu_open.is_none() && !s.settings.open && s.playlist.edit.is_none() && s.playlist.popup.is_none() && self.events_key(key, ctrl, alt) {
            return self.invalidate();
        }
        // The channel rack's keys while it is in front (spec: ui/channel-rack.md §20).
        let s = &self.state;
        if (s.z.last() == Some(&Win::ChannelRack) || s.rack.fill.is_some())
            && s.menu_open.is_none()
            && !s.settings.open
            && s.playlist.edit.is_none()
            && crate::rack::key(self, key, ctrl, alt)
        {
            self.rack_pending();
            return;
        }
        // The mixer's keys while it is in front (spec: ui/mixer.md §2).
        let s = &self.state;
        if s.z.last() == Some(&Win::Mixer) && s.menu_open.is_none() && !s.settings.open && s.playlist.edit.is_none() && s.playlist.popup.is_none() && crate::mixer::key(self, key, ctrl, alt) {
            return;
        }
        // The playlist's editing keys when it is in front (spec: ui/playlist.md §5).
        if self.pl_focus() && crate::playlist::key(self, key, ctrl, alt) {
            return;
        }
        if let Some(cmd) = pattern_shortcut(key, ctrl, self.shift) {
            if let Some(m) = self.pattern_cmd(cmd) {
                self.state.message = Some(m);
            }
            return;
        }
        // The piano roll's editing and tool keys (spec: ui/piano-roll.md §11, ui/piano-keys.md).
        let s = &self.state;
        if s.shown(Win::PianoRoll) && s.menu_open.is_none() && !s.settings.open && s.playlist.edit.is_none() && crate::piano_tools::key(self, key, ctrl, alt) {
            return;
        }
        match key {
            // Space starts and stops (rewinding); Ctrl+Space starts and pauses, like the Play button (FL's manual,
            // keyboard shortcuts: "Start/Stop Playback", "Start/Pause Playback").
            Key::Named(NamedKey::Space) if ctrl => self.act(Action::Play),
            Key::Named(NamedKey::Space) => self.act(if self.transport.playing() { Action::Stop } else { Action::Play }),
            // L flips the PAT / SONG switch, as in FL (the typing keyboard, when on, takes L first).
            Key::Character(c) if !ctrl && !alt && !self.shift && c.eq_ignore_ascii_case("l") => self.act(Action::PatSong(!self.state.song_mode)),
            // spec: ui/tempo-tapper.md §1
            Key::Named(NamedKey::F5) if ctrl => self.act(Action::ToggleWin(Win::Tapper)),
            Key::Named(NamedKey::F7) if alt && !ctrl => self.act(Action::ToggleWin(Win::Touch)),
            Key::Named(NamedKey::F5) => self.act(Action::ToggleWin(Win::Playlist)),
            Key::Named(NamedKey::F6) => self.act(Action::ToggleWin(Win::ChannelRack)),
            Key::Named(NamedKey::F7) => self.act(Action::ToggleWin(Win::PianoRoll)),
            // The browser and the pickers: General ▸ Input ▸ Manage shortcuts (spec: ui/settings-choices.md).
            Key::Named(NamedKey::F8) => self.f8_key(ctrl, alt),
            Key::Named(NamedKey::F9) => self.act(Action::ToggleWin(Win::Mixer)),
            Key::Named(NamedKey::F10) => self.open_settings(views::settings::MIDI),
            // Toolbars > Hide (spec: ui/toolbar-menu.md §5).
            Key::Named(NamedKey::F11) if ctrl => self.toolbar_switch(views::toolbar::Switch::Hide),
            Key::Named(NamedKey::F11) => self.open_settings(views::settings::INFO),
            // Monitor debug panel (views/mondebug.rs) parked 2026-10-09: its values are now the defaults. Uncomment to tune again.
            // Key::Named(NamedKey::F12) => self.state.monitor.debug = !self.state.monitor.debug,
            // spec: ui/close-windows.md; Shift+F12 is Align all channel editors (arrange.rs).
            Key::Named(NamedKey::F12) if self.shift => self.align_channel_editors(),
            Key::Named(NamedKey::F12) => self.close_windows(if ctrl {
                CloseAll::Unfocused
            } else if alt {
                CloseAll::Plugins
            } else {
                CloseAll::All
            }),
            Key::Named(NamedKey::Escape) if self.state.settings.open && self.state.settings.picker.is_some() => self.picker_close(),
            Key::Named(NamedKey::Escape) if self.state.settings.open => self.state.settings.open = false,
            Key::Named(NamedKey::Escape) => {
                self.state.menu_open = None;
                self.state.menu_sub = None;
                self.state.playlist.popup = None;
                self.state.piano.clear_selection();
            }
            // Piano roll editing keys (spec: ui/piano-roll.md §8).
            // Ctrl+A also makes the whole pattern the time selection, as FL does (spec: ui/piano-keys.md §4 rule 4).
            Key::Character(c) if ctrl && c.eq_ignore_ascii_case("a") && self.state.shown(Win::PianoRoll) => {
                let s = &mut self.state;
                if let Some(ch) = piano::channel(s) {
                    let all = piano::notes(s).iter().enumerate().filter(|(_, n)| n.channel == ch).map(|(i, _)| i).collect();
                    s.piano.select(s.pattern, ch, all);
                    s.piano.range = Some((0, piano::pattern_end(s)));
                }
            }
            Key::Character(c) if ctrl && c.eq_ignore_ascii_case("t") => self.toggle_typing(),
            Key::Character(c) if ctrl && !alt && !self.shift && c.eq_ignore_ascii_case("m") => self.toggle_metronome(),
            Key::Character(c) if ctrl && !alt && self.shift && c.eq_ignore_ascii_case("m") => self.act(Action::MenuItem(0, views::menus::FILE_EXPORT_MIDI)),
            // The record options (spec: ui/recording.md §3); a focused playlist or the piano roll took Ctrl+B / Ctrl+P
            // before this.
            Key::Character(c) if ctrl && !alt && !self.shift && c.eq_ignore_ascii_case("i") => self.toggle_record_option(crate::record::Opt::Wait),
            Key::Character(c) if ctrl && !alt && !self.shift && c.eq_ignore_ascii_case("p") => self.toggle_record_option(crate::record::Opt::Precount),
            Key::Character(c) if ctrl && !alt && !self.shift && c.eq_ignore_ascii_case("b") => self.toggle_record_option(crate::record::Opt::Blend),
            Key::Character(c) if ctrl && !alt && c.eq_ignore_ascii_case("e") => self.toggle_step_edit(),
            // Record (spec: ui/recording.md §2): R, when the typing keyboard has not taken it as a note.
            Key::Character(c) if !ctrl && !alt && !self.shift && c.eq_ignore_ascii_case("r") => self.act(Action::Record),
            // Piano-roll tools and selection editing (spec: ui/piano-roll.md §9). Letters switch tools; with Shift too,
            // which the typing keyboard leaves alone (Shift+C and Shift+D were taken above; spec: ui/piano-keys.md §4
            // rule 16).
            Key::Character(c) if !ctrl && !alt && self.state.shown(Win::PianoRoll) && Tool::from_letter(c).is_some() => {
                if let Some(t) = Tool::from_letter(c) {
                    self.act(Action::PianoTool(t));
                }
            }
            Key::Character(c) if ctrl && c.eq_ignore_ascii_case("c") && self.state.shown(Win::PianoRoll) => {
                let s = &mut self.state;
                let sel = s.piano.selection(s.pattern, piano::channel(s));
                let notes = piano::notes(s);
                s.piano.clipboard = sel.iter().filter_map(|&i| notes.get(i).copied()).collect();
            }
            Key::Character(c) if ctrl && c.eq_ignore_ascii_case("v") && self.state.shown(Win::PianoRoll) => {
                let clip = self.state.piano.clipboard.clone();
                let at = piano::paste_tick(&self.state, &clip);
                piano::paste(&mut self.state, &clip, at);
                self.state.piano.dirty = true;
            }
            Key::Named(k @ (NamedKey::ArrowLeft | NamedKey::ArrowRight | NamedKey::ArrowUp | NamedKey::ArrowDown))
                if self.state.shown(Win::PianoRoll) =>
            {
                self.piano_arrow(*k, ctrl, alt);
            }
            // Delete: the selection, or with nothing selected every note of the channel (spec: ui/piano-keys.md §4
            // rule 3).
            Key::Named(NamedKey::Delete) if self.state.shown(Win::PianoRoll) => {
                let s = &mut self.state;
                let idx = piano::targets(s);
                if !idx.is_empty() {
                    piano::snapshot_as(s, "Piano roll delete note");
                    piano::delete(s, idx);
                }
            }
            // FILE shortcuts (views/menus.rs). Shift picks another command: Shift+Ctrl+S = Save as; the other Shift
            // variants have none, so they do nothing (not Open, Export or a new file).
            Key::Character(c) if ctrl && !self.shift && c.eq_ignore_ascii_case("o") => self.act(Action::MenuItem(0, views::menus::FILE_OPEN)),
            Key::Character(c) if ctrl && !self.shift && c.eq_ignore_ascii_case("s") => self.act(Action::MenuItem(0, views::menus::FILE_SAVE)),
            Key::Character(c) if ctrl && self.shift && c.eq_ignore_ascii_case("s") => self.act(Action::MenuItem(0, views::menus::FILE_SAVE_AS)),
            // VIEW ▸ Arrange windows ▸ Realign (spec: ui/menu-submenus.md §2).
            Key::Character(c) if ctrl && self.shift && !alt && c.eq_ignore_ascii_case("h") => self.arrange(crate::arrange::Layout::Realign),
            Key::Character(c) if ctrl && !self.shift && c.eq_ignore_ascii_case("r") => self.act(Action::MenuItem(0, views::menus::FILE_EXPORT)),
            Key::Character(c) if ctrl && self.shift && !alt && c.eq_ignore_ascii_case("r") => self.act(Action::MenuItem(0, views::menus::FILE_EXPORT_MP3)),
            Key::Character(c) if ctrl && !self.shift && c.eq_ignore_ascii_case("n") => self.act(Action::MenuItem(0, views::menus::FILE_SAVE_VERSION)),
            // TOOLS > Browser find... (spec: ui/browser-commands.md §3).
            Key::Character(c) if ctrl && !alt && !self.shift && c.eq_ignore_ascii_case("f") => self.browser_jump(crate::browser::Jump::Find),
            _ => {}
        }
    }

    /// SONG mode, a press or drag in the piano roll's ruler at pattern tick `t`: when the playlist plays this pattern
    /// there, the song position moves to that place, as a click in the playlist ruler would (the clip under the
    /// current position first, else the next one, else the first). A playing song plays on from there (on release
    /// when dragging); a paused one stops there, so Play resumes from that point. False: the pattern does not play
    /// that tick anywhere in the song.
    fn piano_song_seek(&mut self, t: u32, press: bool) -> bool {
        let tp = &self.transport;
        let near = if tp.playing() && tp.song() { tp.tick() as u32 } else { self.state.playlist.pos };
        let Some(at) = piano::song_tick_of(&self.state, t, near) else { return false };
        if press && tp.playing() && tp.song() && tp.paused() {
            tp.stop();
        }
        self.set_song_pos(at, press);
        true
    }

    /// Start playback in the mode the PAT / SONG switch shows. Pattern mode loops the current pattern from the piano
    /// roll's play-start marker (§10); song mode plays the playlist from its start and stops at its end.
    pub(crate) fn start_playback(&mut self) {
        self.start_playback_counted(0);
    }

    /// `start_playback` after a recording count-in of `bars` bars (spec: ui/recording.md §3).
    pub(crate) fn start_playback_counted(&mut self, bars: u32) {
        let song = self.state.song_mode;
        if song && !self.one_click_starting() && self.state.project.arrangement().is_none_or(|a| a.clips.is_empty()) {
            self.state.message = Some("The arranger is empty: nothing to play as a song".into());
            self.invalidate();
            return;
        }
        // The song starts with the project's initial values (spec: ui/settings-choices.md "Advanced: Initialize
        // controls automatically").
        if crate::init::apply(&mut self.state) {
            self.invalidate();
        }
        if song {
            crate::perform::starting(&mut self.state);
        }
        let p = crate::rack::play_project(&self.state);
        let start = if song { f64::from(self.state.playlist.pos) } else { f64::from(self.state.piano.marker) };
        // A looping time selection starts at its start unless the position is inside it (spec: ui/recording.md §5).
        let start = match self.selection_loop() {
            Some((a, b)) if start < f64::from(a) || start >= f64::from(b) => f64::from(a),
            _ => start,
        };
        // The engine gets the loop range before its first tick.
        self.sync_loop_range();
        self.transport.play_counted(&p, self.play_mode(), start, bars);
    }

    fn play_mode(&self) -> PlayMode {
        if self.state.song_mode { PlayMode::Song } else { PlayMode::Pattern(self.state.pattern) }
    }

    /// Play one note of channel `channel` (Note::channel) once (also while playing or paused).
    /// Typing keyboard switch (toolbar, Options menu, Ctrl+T); turning it off releases held notes.
    pub fn toggle_typing(&mut self) {
        let s = &mut self.state;
        if let Some(i) = s.toggles.iter().position(|t| *t == "typing") {
            s.toggles.remove(i);
            self.typing_release_all();
        } else {
            s.toggles.push("typing");
        }
        self.invalidate();
    }

    /// Enable groups switch (toolbar, OPTIONS menu, Shift+Alt+G): the playlist's clips are deselected, as in FL.
    pub fn toggle_groups(&mut self) {
        let on = crate::enable_groups::toggle();
        self.state.playlist.sel.clear();
        self.state.message = Some(if on { "Groups enabled" } else { "Groups disabled" }.into());
        self.invalidate();
    }

    pub fn typing_on(&self) -> bool {
        self.state.toggles.contains(&"typing")
    }

    /// A physical key went down or up. Returns true when the typing keyboard took it (a note key or
    /// the octave keys while the switch is on, or the release of a held note); the caller then
    /// skips the normal shortcut handling. Spec: ui/typing-keyboard.md.
    pub fn typing_key(&mut self, code: KeyCode, pressed: bool, ctrl: bool, alt: bool) -> bool {
        // A playlist text box (track / marker name, point value) takes the letters.
        if pressed && (self.modal_open() || self.state.playlist.edit.is_some() || self.state.plugdb.finding || self.state.pat_ui.name_box.is_some() || self.state.pat_ui.confirm || self.browser_takes_typing() || self.state.perf.finding) {
            return false;
        }
        // So does a box of the settings window's Info page, and a browser folder cell.
        if pressed && (self.info_editing() || self.folder_cell_editing() || self.name_editing()) {
            return false;
        }
        if !pressed {
            if let Some(i) = self.perf_typed.iter().position(|h| h.0 == code) {
                let (_, note) = self.perf_typed.remove(i);
                self.perform_note(note, TYPING_VELOCITY, false);
                return true;
            }
            let Some(i) = self.typing_held.iter().position(|h| h.0 == code) else { return false };
            let (_, row, key) = self.typing_held.remove(i);
            self.transport.note_off(row, key);
            self.typing_lit_changed();
            self.step_edit_up(crate::step_edit::Src::Typing(code));
            return true;
        }
        // Ctrl / Alt / Shift combinations stay shortcuts (Shift+D, Shift+Q in the piano roll).
        if !self.typing_on() || ctrl || alt || self.shift {
            return false;
        }
        if matches!(code, KeyCode::NumpadAdd | KeyCode::NumpadSubtract) {
            let d = if code == KeyCode::NumpadAdd { 1 } else { -1 };
            self.typing_octave = (self.typing_octave + d).clamp(-4, 5);
            self.state.message = Some(format!("Computer keyboard octave {:+}", self.typing_octave));
            self.invalidate();
            return true;
        }
        // Performance mode (spec: ui/performance-mode.md §3): its layout's keys launch the zone's slots as the
        // Performance mode MIDI channel's notes do, and play no note; the other note keys do nothing.
        if crate::perform::zone(&self.state).is_some() {
            if let Some(note) = crate::perform::typing_note(code) {
                if !self.perf_typed.iter().any(|h| h.0 == code) {
                    self.perf_typed.push((code, note));
                    self.perform_note(note, TYPING_VELOCITY, true);
                }
                return true;
            }
            if typing_semitone(code).is_some() {
                return true;
            }
        }
        let Some(semi) = typing_semitone(code) else { return false };
        if self.typing_held.iter().any(|h| h.0 == code) {
            return true; // auto-repeat
        }
        let key = 48 + semi + 12 * self.typing_octave;
        // A channel locked to the computer keyboard takes its notes (spec: ui/channel-rack.md §18 Receive notes from).
        let row = self.state.rack.note_row(crate::rack::NoteSrc::Typing, self.state.selected_channel);
        if !(0..=127).contains(&key) || row >= self.state.project.channels.len() {
            return true;
        }
        self.locked_hint(crate::rack::NoteSrc::Typing, row);
        self.sync_live();
        let key = key as u16;
        self.transport.note_on(row, key, TYPING_VELOCITY);
        self.typing_held.push((code, row, key));
        self.step_edit_down(crate::step_edit::Src::Typing(code), key, TYPING_VELOCITY);
        self.typing_lit_changed();
        self.key_tap(); // spec: ui/tempo-tapper.md §8
        true
    }

    /// A note of `src` plays its locked channel `row` while another one is focused: the hint bar says so, as the
    /// original's did ("808 Clap (Locked:Typing keyboard)", measured 2026-10-30).
    pub(crate) fn locked_hint(&mut self, src: crate::rack::NoteSrc, row: usize) {
        if row == self.state.selected_channel {
            return;
        }
        let Some(c) = self.state.project.channels.get(row) else { return };
        let name = c.name.clone().unwrap_or_else(|| format!("Channel {}", c.index + 1));
        self.state.message = Some(format!("{name} (locked to {})", src.label().to_lowercase()));
        self.invalidate();
    }

    /// The live stream needs the project before the first live note.
    pub(crate) fn sync_live(&mut self) {
        if !self.typing_synced {
            self.typing_synced = true;
            self.transport.update(&crate::rack::play_project(&self.state));
        }
    }

    /// Release every held typing-keyboard note and the piano-roll key held with the mouse (focus lost, switch off,
    /// rows renumbered): no hanging notes.
    pub fn typing_release_all(&mut self) {
        self.step_chord.clear();
        if let Some((row, key)) = self.mouse_note.take() {
            self.transport.note_off(row, key);
        }
        for (_, note) in std::mem::take(&mut self.perf_typed) {
            self.perform_note(note, TYPING_VELOCITY, false);
        }
        if self.typing_held.is_empty() {
            return;
        }
        for (_, row, key) in std::mem::take(&mut self.typing_held) {
            self.transport.note_off(row, key);
        }
        self.typing_lit_changed();
    }

    /// Ends the typed notes (and Performance mode keys) whose key `down` says is no longer down, as their release would
    /// have: on Linux the release of a key let go of over a docked plugin editor goes to the editor (issue #274).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn release_keys_up(&mut self, down: impl Fn(KeyCode) -> bool) {
        let up: Vec<KeyCode> = self.typing_held.iter().map(|h| h.0).chain(self.perf_typed.iter().map(|h| h.0)).filter(|&c| !down(c)).collect();
        for code in up {
            self.typing_key(code, false, false, false);
        }
    }

    /// Held typing-keyboard keys and MIDI notes light on the piano roll's keyboard (§4).
    pub(crate) fn typing_lit_changed(&mut self) {
        let channels = &self.state.project.channels;
        let midi = self.midi_play.lit().chain(self.state.touch.held.map(|(key, row)| (row, u16::from(key))));
        self.state.piano.typed = self.typing_held.iter().map(|&(_, row, key)| (row, key)).chain(midi).filter_map(|(row, key)| Some((channels.get(row)?.index, i32::from(key)))).collect();
        match self.piano_grid {
            Some(g) => self.damage_rect(Rect::new(g.x - 73.0, g.y, 73.0, g.h)),
            None => self.invalidate(),
        }
    }

    fn preview(&self, channel: u16, key: i32) {
        let mut p = self.state.project.clone();
        p.tempo = Some(self.state.tempo);
        let note = Note { key: key.clamp(0, views::piano::TOP_KEY) as u16, length: u32::from(p.ppq.max(4)), ..Note::step(channel, 0) };
        p.patterns = vec![Pattern { number: 1, notes: vec![note], ..Pattern::default() }];
        p.arrangements.clear();
        self.transport.preview(p);
    }

    /// Arrow keys in the piano roll (§9): Left/Right scroll; Shift+arrows move the selection (or
    /// every note) one snap unit (a tick without one, spec: ui/piano-snap.md §6) / semitone; Ctrl+Up/Down
    /// move it an octave.
    /// The arrows in the piano roll (spec: ui/piano-roll.md §11, ui/piano-keys.md §2 "Moving notes", §4 rules 8 and 9):
    /// Shift moves the targets a key or a snap unit, Ctrl an octave, Alt a tick, Ctrl+Shift+Left / Right rotates them;
    /// Left / Right alone scroll a bar.
    fn piano_arrow(&mut self, k: winit::keyboard::NamedKey, ctrl: bool, alt: bool) {
        use winit::keyboard::NamedKey::*;
        let Some(g) = self.piano_geom() else { return };
        let step = i64::from(crate::piano_snap::unit(&self.state).unwrap_or(1));
        let left = k == ArrowLeft;
        let (dt, dk) = match (k, ctrl, self.shift, alt) {
            (ArrowLeft | ArrowRight, true, true, false) => {
                let idx = piano::targets(&self.state);
                if piano::rotate(&mut self.state, &idx, if left { -step } else { step }) {
                    self.state.piano.dirty = true;
                }
                return;
            }
            (ArrowLeft | ArrowRight, false, false, true) => (if left { -1 } else { 1 }, 0),
            (_, _, _, true) | (ArrowUp | ArrowDown, true, true, _) | (ArrowLeft | ArrowRight, true, false, _) => return,
            (ArrowUp, true, false, _) => (0, 12),
            (ArrowDown, true, false, _) => (0, -12),
            (ArrowLeft, false, true, _) => (-step, 0),
            (ArrowRight, false, true, _) => (step, 0),
            (ArrowUp, false, true, _) => (0, 1),
            (ArrowDown, false, true, _) => (0, -1),
            (ArrowLeft | ArrowRight, false, false, _) => {
                let d = if left { -1.0 } else { 1.0 };
                let s = &mut self.state;
                s.piano.scroll_x = (s.piano.scroll_x + d * g.bar as f32).max(0.0);
                return;
            }
            _ => return,
        };
        let idx = piano::targets(&self.state);
        if piano::shift(&mut self.state, &idx, dt, dk) {
            self.state.piano.dirty = true;
        }
    }

    /// The piano roll gets the editing keys: it is shown, no menu, Settings or playlist text box is open, and neither
    /// the channel rack, the mixer nor the playlist is in front of it (`App::key` asks those first).
    fn piano_keys(&self) -> bool {
        let s = &self.state;
        s.shown(Win::PianoRoll)
            && s.menu_open.is_none()
            && !s.settings.open
            && s.playlist.edit.is_none()
            && s.rack.fill.is_none()
            && !matches!(s.z.last(), Some(Win::ChannelRack | Win::Mixer))
            && !self.pl_focus()
    }

    /// Left click in the piano roll (spec: ui/piano-roll.md §8 Draw tool, §9 other tools).
    fn piano_click(&mut self, a: &Action) {
        let (x, y) = self.mouse;
        let Some(g) = self.piano_geom() else { return };
        let Some(ch) = piano::channel(&self.state) else {
            self.state.message = Some("Piano roll: no channel selected".into());
            return;
        };
        let tool = self.state.piano.tool;
        if matches!(a, Action::PianoGrid | Action::PianoNote(_)) && tool != Tool::Draw {
            let s = &mut self.state;
            let note = if let Action::PianoNote(i) = *a { Some(i) } else { None };
            match tool {
                Tool::Delete => {
                    piano::snapshot_as(s, "Piano roll delete note");
                    self.drag = Some(Drag::NoteErase);
                    self.erase_at(x, y);
                    return;
                }
                Tool::Mute => {
                    let Some(i) = note else { return };
                    piano::snapshot(s);
                    let on = !piano::notes(s)[i].muted();
                    self.drag = Some(Drag::NoteMute { on });
                    self.mute_at(x, y, on);
                    return;
                }
                Tool::Paint if note.is_none() => {
                    piano::snapshot_as(s, "Piano roll add note");
                    s.piano.clear_selection();
                    let key = g.key_at(y).clamp(0, views::piano::TOP_KEY);
                    let start = crate::piano_snap::place(s, &g, g.tick_at(x), self.alt);
                    let i = piano::add_note(s, ch, start, key);
                    let i = piano::paint_mono(s, i);
                    let len = piano::notes(s)[i].length;
                    s.piano.dirty = true;
                    self.drag = Some(Drag::NotePaint { key, start, len });
                    let play = self.audition();
                    self.hold_key(Some(key), play);
                    self.damage_piano();
                    return;
                }
                Tool::Slice => {
                    self.drag = Some(Drag::Slice);
                    self.state.piano.overlay = Some(Overlay::Slice(crate::piano_snap::place(&self.state, &g, g.tick_at(x), self.alt)));
                    self.damage_piano();
                    return;
                }
                Tool::Zoom => {
                    self.drag = Some(Drag::ZoomBox { x0: x, y0: y });
                    return;
                }
                Tool::Playback => {
                    let t = g.tick_at(x).max(0.0);
                    self.drag = Some(Drag::Scrub { tick: t });
                    self.scrub(t, t);
                    self.state.piano.overlay = Some(Overlay::Scrub(t));
                    self.damage_piano();
                    return;
                }
                Tool::Select if note.is_none() => {
                    let base = if self.ctrl { s.piano.selection(s.pattern, Some(ch)) } else { Vec::new() };
                    s.piano.select(s.pattern, ch, base.clone());
                    self.drag = Some(Drag::Marquee { x0: x, y0: y, base });
                    self.damage_piano();
                    return;
                }
                Tool::Select => {
                    // Clicking an unselected note selects it (Ctrl adds), then drags as Draw does.
                    let i = note.unwrap_or(0);
                    let mut sel = s.piano.selection(s.pattern, Some(ch));
                    if !sel.contains(&i) {
                        if !self.ctrl {
                            sel.clear();
                        }
                        sel.push(i);
                        s.piano.select(s.pattern, ch, sel);
                    }
                }
                // Paint on a note: drags that one note like Draw (no group move).
                _ => s.piano.clear_selection(),
            }
        }
        let s = &mut self.state;
        match *a {
            Action::PianoGrid => {
                piano::snapshot_as(s, "Piano roll add note");
                s.piano.clear_selection();
                let i = piano::add_note(s, ch, crate::piano_snap::place(s, &g, g.tick_at(x), self.alt), g.key_at(y));
                let i = piano::paint_mono(s, i);
                let n = piano::notes(s)[i];
                s.piano.last_len = n.length;
                self.drag = Some(Drag::NoteMove { x0: x, y0: y, orig: vec![(i, n)] });
                let play = self.audition();
                self.hold_key(Some(i32::from(n.key)), play);
                // Held without a move, the new note's type cycles (Click-and-hold & special gesture functions).
                self.hold_start(crate::hold::Kind::NewNote);
            }
            Action::PianoNote(i) => {
                let Some(&n) = piano::notes(s).get(i) else { return };
                piano::snapshot(s);
                let sel = s.piano.selection(s.pattern, Some(ch));
                let idx = if sel.contains(&i) && tool.moves_groups() {
                    sel
                } else {
                    s.piano.clear_selection();
                    vec![i]
                };
                let notes = piano::notes(s);
                // The clicked note first: it is the one auditioned while dragging.
                let orig: Vec<(usize, Note)> =
                    std::iter::once(i).chain(idx.into_iter().filter(|&j| j != i)).filter_map(|j| notes.get(j).map(|&m| (j, m))).collect();
                s.piano.last_len = n.length.max(g.step);
                s.piano.last_controls = Some(n);
                let nr = g.note_rect(&n).unwrap_or_default();
                self.drag = Some(if x >= nr.right() - resize_zone(nr.w) {
                    self.resize_cursor = true;
                    Drag::NoteResize { x0: x, orig }
                } else {
                    let play = self.audition();
                    self.hold_key(Some(i32::from(n.key)), play);
                    Drag::NoteMove { x0: x, y0: y, orig }
                });
                // Shift+drag drags copies of the notes (§8); held without a move, the notes are cloned (Click-and-hold &
                // special gesture functions).
                self.shift_clone = self.shift && matches!(self.drag, Some(Drag::NoteMove { .. }));
                if matches!(self.drag, Some(Drag::NoteMove { .. })) && !self.shift_clone {
                    self.hold_start(crate::hold::Kind::Notes);
                }
            }
            Action::PianoVelocity => {
                piano::snapshot(s);
                self.drag = Some(Drag::Velocity { last_x: x, last_y: y });
                self.paint_velocity(x, y, x, y);
            }
            Action::PianoKeys => {
                let k = g.key_at(y).clamp(0, views::piano::TOP_KEY);
                self.drag = Some(Drag::PianoKeys { key: k });
                self.key_velocity = self.key_click_velocity(x);
                self.hold_key(Some(k), true);
            }
            _ => {}
        }
    }

    /// Playlist actions (spec: ui/playlist.md). `right` = the right button.
    fn playlist_act(&mut self, a: &Action, right: bool) -> bool {
        use crate::playlist as pl;
        let grid = self.pl_grid;
        match *a {
            Action::PlaylistArea => {
                if let Some(g) = grid {
                    if right && self.ctrl {
                        // Ctrl+right-drag zooms to a box (§3).
                        self.drag = Some(Drag::Pl(pl::PlDrag::ZoomBox { x0: self.mouse.0, y0: self.mouse.1 }));
                    } else {
                        pl::press_grid(self, g, right);
                    }
                }
            }
            Action::PlRuler => {
                if let Some(g) = grid {
                    pl::press_ruler(self, g, right);
                }
            }
            Action::PlMarkers => {
                if let Some(g) = grid {
                    pl::press_markers(self, g, right);
                }
            }
            Action::PlHeader(t) => {
                let at = self.top_hit(self.mouse.0, self.mouse.1).map_or(self.mouse, |h| (h.rect.x, h.rect.y));
                pl::press_header(self, t, right, at);
            }
            Action::PlLed(t) => pl::press_led(self, t, right),
            Action::PlGroupArrow(t) => {
                pl::toggle_collapse(&mut self.state, t);
                self.invalidate();
            }
            Action::PlTool(t) if !right => {
                self.state.playlist.tool = t;
                self.state.playlist.overlay = None;
            }
            Action::PlSnap => {
                let (x, y) = self.mouse;
                self.state.playlist.popup = Some(pl::snap_menu(&self.state, x, y + 10.0));
            }
            Action::PlOptions => {
                let (x, y) = self.mouse;
                self.state.playlist.popup = Some(pl::options_menu(&self.state, x - 6.0, y + 10.0));
            }
            Action::PlStretch => {
                self.state.message = pl::run(self, pl::Cmd::StretchMode);
            }
            Action::PlArrangement => {
                let r = self.top_hit(self.mouse.0, self.mouse.1).map(|h| h.rect).unwrap_or(Rect::new(self.mouse.0, self.mouse.1, 1.0, 20.0));
                self.state.playlist.arr_anchor = (r.x - 4.0, r.y);
                self.state.playlist.popup = Some(pl::arrangement_menu(&self.state, r.x, r.bottom() + 2.0));
            }
            Action::PlPickTab(k) => self.state.playlist.picker = k,
            Action::PlPick(src) => {
                self.state.playlist.source = Some(src);
                if let os_model::ClipSource::Pattern(n) = src {
                    self.state.pattern = n;
                }
                if right {
                    let (x, y) = self.mouse;
                    match src {
                        // Right-click on a pattern: the PATTERNS commands for it (spec: ui/pattern-menu.md §3).
                        os_model::ClipSource::Pattern(_) => self.state.playlist.popup = Some(views::menus::pattern_popup(x, y)),
                        // On an audio or automation clip: its channel's menu, the channel selected in the rack (spec:
                        // ui/playlist.md §10a).
                        os_model::ClipSource::Channel(ch) => {
                            if let Some(row) = self.state.project.channels.iter().position(|c| c.index == ch) {
                                self.state.rack.select_only(row);
                            }
                            self.state.playlist.popup = Some(pl::pick_menu(&self.state, ch, x, y));
                        }
                    }
                }
            }
            Action::PlPopup(i) => {
                let Some(p) = self.state.playlist.popup.as_mut() else { return true };
                let Some(item) = p.items.get(i).cloned() else { return true };
                if !item.children.is_empty() {
                    p.sub = if p.sub == Some(i) { None } else { Some(i) };
                    p.scroll[1] = 0;
                } else if item.enabled && !item.head {
                    if let Some(m) = pl::run(self, item.cmd) {
                        self.state.message = Some(m);
                    }
                } else if let pl::Cmd::NotYet(name) = item.cmd {
                    // A greyed-out entry only says so; the menu stays open.
                    self.state.message = Some(off_text(name));
                }
            }
            Action::PlPopupSub(i) => {
                let item = self.state.playlist.popup.as_ref().and_then(|p| p.sub.and_then(|s| p.items.get(s))).and_then(|it| it.children.get(i)).cloned();
                match item.filter(|it| !it.head && !it.sep) {
                    Some(item) if item.enabled => {
                        if let Some(m) = pl::run(self, item.cmd) {
                            self.state.message = Some(m);
                        }
                    }
                    Some(pl::PItem { cmd: pl::Cmd::NotYet(name), .. }) => self.state.message = Some(off_text(name)),
                    _ => {}
                }
            }
            _ => return false,
        }
        // A pick in a dropdown's submenu closes the dropdown with it.
        if matches!(a, Action::PlPopup(_) | Action::PlPopupSub(_)) && self.state.playlist.popup.is_none() && self.state.menu_sub.take().is_some() {
            self.state.menu_open = None;
        }
        self.invalidate();
        true
    }

    pub fn act(&mut self, a: Action) {
        if let Action::Settings(sa) = a {
            return self.settings_act(sa);
        }
        if let Action::Scrollbar(t) = a {
            return self.scrollbar_press(t, false);
        }
        if let Action::ScrollStep(t, dir) = a {
            let step = self.bar(t).map_or(48.0, |b| (b.view / 8.0).max(18.0).round());
            return self.eased(|app| app.scroll_set(t, app.scroll_get(t) + dir as f32 * step));
        }
        self.note_host_tweak(&a);
        if self.playlist_act(&a, false) {
            return;
        }
        if self.browser_act(&a) {
            return;
        }
        if self.rack_act(&a) {
            return;
        }
        if a == Action::PianoSplitter {
            return self.piano_split_press();
        }
        if matches!(a, Action::PianoGrid | Action::PianoNote(_) | Action::PianoVelocity | Action::PianoKeys) {
            self.piano_click(&a);
            return;
        }
        if a == Action::PianoRuler {
            if let Some(g) = self.piano_geom() {
                let t = crate::piano_snap::place(&self.state, &g, g.tick_at(self.mouse.0), self.alt);
                if self.state.song_mode && self.piano_song_seek(t, true) {
                    self.drag = Some(Drag::SongRuler);
                } else {
                    self.state.piano.marker = t;
                    self.state.piano.marker_placed = true;
                    self.drag = Some(Drag::Marker);
                }
            }
            return;
        }
        let s = &mut self.state;
        match a {
            Action::PianoGrid | Action::PianoNote(_) | Action::PianoVelocity | Action::PianoKeys | Action::PianoRuler | Action::PianoSplitter | Action::Scrollbar(_) | Action::ScrollStep(..) => {}
            Action::PianoTool(t) => {
                s.piano.tool = t;
                s.piano.overlay = None;
            }
            // EDIT > Undo, also the toolbar's Undo button (spec: ui/toolbar-buttons.md §2).
            Action::PianoUndo | Action::Util(Util::Undo) => s.message = crate::history::ctrl_z(s),
            Action::Typing => self.toggle_typing(),
            Action::Metronome => self.toggle_metronome(),
            Action::Record => self.toggle_record(),
            Action::RecOpt(o) => self.toggle_record_option(o),
            Action::OneClick => self.one_click_button(),
            Action::AutoScroll => self.toggle_autoscroll(),
            Action::EnableGroups => self.toggle_groups(),
            Action::Multilink => self.toggle_multilink(),
            Action::Remote(r) => self.link_box_act(r),
            Action::StepEdit => self.toggle_step_edit(),
            // spec: ui/main-window.md §13: a double click opens the Plugin performance monitor; one click does nothing.
            Action::CpuPanel => {
                let now = std::time::Instant::now();
                let double = self.cpu_click.is_some_and(|t| now.duration_since(t).as_millis() < 400);
                self.cpu_click = if double { None } else { Some(now) };
                if double {
                    self.open_perf();
                }
            }
            Action::Perf(p) => self.perf_act(p),
            Action::Tapper(a) => self.tapper_act(a),
            Action::Touch(a) => self.touch_act(a),
            Action::Strum(a) => self.strum_act(a),
            Action::Container(a) => self.container_act(a),
            Action::Arp(a) => self.arp_act(a),
            Action::PianoDlg(a) => self.piano_dlg_act(a),
            Action::Events(a) => self.events_act(a),
            Action::Placeholder(name) => s.message = Some(off_text(name)),
            Action::Monitor => {
                s.monitor.toggle();
                self.damage_band(MONITOR_RECT);
                // The hint (frequency read-out) follows the new mode.
                let (x, _) = self.panel_mouse(crate::toolbar::Panel::Monitor);
                self.hint = Cow::Owned(self.state.monitor.hint((x - MONITOR_RECT.x - 1.0).max(0.0) as usize));
                self.damage_band(HINT_RECT);
            }
            Action::Minimize => self.want_minimize = true,
            Action::Maximize => self.want_maximize = true,
            Action::Close => self.want_close = true,
            Action::DragWindow => self.want_drag_window = true,
            Action::Bar(h) => self.bar_hit(h),
            Action::Play => {
                self.perf_transport();
                match self.transport.state() {
                    // Play toggles pause (the position is held in place); Stop rewinds.
                    TransportState::Playing => self.transport.pause(),
                    TransportState::Paused => self.transport.resume(),
                    TransportState::Stopped => self.play_from_stop(),
                }
            }
            Action::Stop => {
                self.perf_transport();
                self.transport.stop();
                self.record_stopped();
                // Stop rewinds to where playback started: the song position (or the piano roll's play-start marker)
                // stays where it was put, so the next start plays from there again; with a time selection, its start.
                let s = &mut self.state;
                if s.song_mode {
                    if let Some((a, _)) = s.playlist.range {
                        s.playlist.pos = a;
                    }
                    // A song position move: the initial values (spec: ui/settings-choices.md "Advanced: Initialize
                    // controls automatically").
                    crate::init::apply(s);
                } else if let Some((a, _)) = s.piano.range.filter(|_| s.shown(Win::PianoRoll)) {
                    // spec: ui/recording.md §5: the piano roll's time selection, as in SONG mode.
                    s.piano.marker = a;
                }
                self.invalidate();
            }
            Action::PatSong(song) => {
                let changed = s.song_mode != song;
                s.song_mode = song;
                // The running playback follows the switch: it restarts in the new mode (a paused one is stopped),
                // so the playheads always show what is heard.
                if changed && self.transport.playing() {
                    let was_playing = !self.transport.paused();
                    self.transport.stop();
                    if was_playing {
                        self.start_playback();
                    }
                }
            }
            Action::MonParam(i) => {
                self.drag = Some(Drag::MonParam(i));
                let x = self.mouse.0;
                self.set_mon_param(i, x);
            }
            Action::MonReset => {
                s.monitor.cfg = Default::default();
                self.invalidate();
            }
            Action::MonSave => {
                s.message = Some(match save_monitor_cfg(&s.monitor.cfg) {
                    Ok(p) => format!("Monitor settings saved to {}", p.display()),
                    Err(e) => format!("Monitor settings not saved: {e}"),
                });
                self.invalidate();
            }
            Action::MonClose => {
                s.monitor.debug = false;
                self.invalidate();
            }
            Action::CableDebug(d) => self.cable_debug(d),
            Action::Tempo => {
                // The digit under the press sets the step: the whole number, else the decimal place it is over.
                let x = self.mouse.0 / views::band_k(s); // the tempo box is laid out in band px
                let n = s.tempo_digits.get().iter().filter(|&&d| x >= d).count();
                let step = [1.0, 0.1, 0.01, 0.001][n];
                // One undo step for the drag (none when it doesn't change the tempo).
                crate::control::edit_press(s, crate::control::command::TEMPO, crate::control::command::Region::Settings);
                self.drag = Some(Drag::Tempo { y0: self.mouse.1, t0: s.tempo, step });
            }
            Action::SongPos => self.song_pos_press(),
            Action::Clock => {
                s.clock.toggle();
                s.clock.save();
                self.invalidate();
            }
            Action::GlobalSnap => {
                // Its top-left at the combo's bottom-left (spec: ui/global-snap.md §2).
                let b = self.hits.iter().rev().find(|h| h.action == Action::GlobalSnap).map_or(Rect::new(self.mouse.0, self.mouse.1, 0.0, 0.0), |h| h.rect);
                s.playlist.popup = Some(crate::snap::menu(s, b.x, b.bottom()));
            }
            Action::GlobalSnapToggle => s.message = Some(crate::snap::toggle(s)),
            Action::Menu(i) => {
                s.pat_ui.menu_at = None;
                s.menu_open = if s.menu_open == Some(i) { None } else { Some(i) };
                s.menu_sub = None;
            }
            Action::MenuItem(m, i) => {
                s.menu_open = None;
                self.menu_item(m, i);
            }
            Action::MenuSub(m, i) => self.open_menu_sub(m, i),
            // Show a hidden window, bring a covered one to the front, hide one already in front
            // (spec: ui/channel-rack.md §5).
            Action::ToggleWin(Win::Browser) => s.show_browser = !s.show_browser,
            Action::ToggleWin(Win::Perf) => self.toggle_perf(),
            Action::ToggleWin(Win::Tapper) => self.toggle_tapper(),
            Action::ToggleWin(Win::Touch) => self.toggle_touch(),
            Action::ToggleWin(w) => {
                if s.shown(w) && s.in_front(w) {
                    s.hide(w);
                } else {
                    s.raise(w);
                }
                if w == Win::PianoRoll && s.shown(Win::PianoRoll) {
                    piano::center_keys(s, self.piano_grid.map_or(700.0, |g| g.h));
                }
            }
            // spec: ui/pattern-menu.md §1
            Action::PatternPrev => {
                let at = views::band_rect(s, Rect::new(837.0, 75.0, 0.0, 0.0));
                s.pat_ui.menu_at = Some((at.x, at.y));
                s.menu_open = Some(views::menus::PATTERNS);
            }
            Action::PatternNext => self.drag = Some(Drag::PatternBox { y0: self.mouse.1, p0: s.pattern.max(1) }),
            Action::PatternAdd => {
                let at = views::band_rect(s, Rect::new(crate::patterns::PLUS_BOX_AT.0, crate::patterns::PLUS_BOX_AT.1, 0.0, 0.0));
                crate::patterns::run(s, crate::patterns::Cmd::FindFirst, (at.x, at.y));
            }
            Action::Pat(h) => crate::patterns::hit(s, h),
            Action::Dlg(h) => self.dialog_click(h),
            Action::SongInfo => self.close_song_info(),
            Action::PlPickList => {}
            Action::PlPickAdd => {
                let at = crate::patterns::box_at_mouse(self.mouse);
                crate::patterns::run(s, crate::patterns::Cmd::FindFirst, at);
                s.playlist.source = None;
            }
            Action::Mix(crate::mixer::MHit::EqGraph) => {
                if let Some(g) = self.top_hit(self.mouse.0, self.mouse.1).map(|h| h.rect) {
                    let (track, band) = (self.state.selected_mixer, crate::mixer::eq_band_at(&self.state, g, self.mouse.0));
                    crate::control::edit_press(&mut self.state, "Mixer track equalizer", crate::control::command::Region::Track(track));
                    self.drag = Some(Drag::EqGraph { track, band, g });
                    crate::mixer::eq_graph_drag(&mut self.state, track, band, g, self.mouse);
                    self.damage_eq();
                }
            }
            Action::Mix(h) => {
                let plain_strip = matches!(h, crate::mixer::MHit::Strip(_)) && !self.ctrl && !self.shift;
                if let Some(m) = crate::mixer::hit(self, h) {
                    self.state.message = Some(m);
                }
                if plain_strip {
                    self.drag = Some(Drag::MixSelect);
                }
            }
            Action::Fader(t) => {
                crate::mixer::select(s, t, self.ctrl);
                // One undo step for the drag (none when it doesn't move the fader, nor with Undo knob tweaks off: a
                // fader is a knob here).
                if crate::prefs::check(&crate::prefs::UNDO_KNOB_TWEAKS) {
                    crate::control::edit_press(s, crate::history::MIXER_VOLUME, crate::control::command::Region::Track(t));
                }
                let v0 = s.project.mixer.iter().find(|m| m.index == t).and_then(|m| m.volume).unwrap_or(0.8);
                self.drag = Some(Drag::Fader { track: t, y0: self.mouse.1, v0 });
            }
            Action::ChannelSelect(_) | Action::RackSelect(_) => {}
            Action::RackRollView => s.rack_roll_view = !s.rack_roll_view,
            Action::RackRoll(i) => {
                s.selected_channel = i;
                s.piano.clear_selection();
                s.raise(Win::PianoRoll);
                piano::center_keys(s, self.piano_grid.map_or(700.0, |g| g.h));
            }
            Action::RackMixer(_) => {}
            Action::RackAdd => {
                let at = self.mouse;
                self.open_plugin_menu(crate::plugin_menu::Target::Gen(None), at);
            }
            // Under the button (spec: ui/plugin-menu.md §1).
            Action::PickerOpen => {
                let (x, y) = self.mouse;
                let at = self.top_hit(x, y).map_or((x, y), |h| (h.rect.x, h.rect.bottom() + 2.0));
                self.open_plugin_menu(crate::plugin_menu::Target::All, at);
            }
            // Under the button (spec: ui/project-picker.md §2).
            Action::ProjectPicker => {
                let (x, y) = self.mouse;
                let at = self.top_hit(x, y).map_or((x, y), |h| (h.rect.x, h.rect.bottom() + 2.0));
                self.open_project_picker(at);
            }
            // An automation clip LFO's switch and mode: a click flips them, one undo step.
            Action::Knob(id @ crate::knobs::KnobId::AutoLfo(_, k)) if k.switch() => {
                crate::history::knob(s, id, false);
                s.set_knob(id, if s.knob(id) >= 0.5 { 0.0 } else { 1.0 });
                self.hint = Cow::Owned(s.knob_text(id));
            }
            // A MIDI Out check: a click flips it, one undo step.
            Action::Knob(id @ crate::knobs::KnobId::MidiOut(_, k)) if k.switch() => {
                crate::history::knob(s, id, false);
                s.set_knob(id, if s.knob(id) >= 0.5 { 0.0 } else { 1.0 });
                self.hint = Cow::Owned(s.knob_text(id));
            }
            Action::Knob(id) => {
                crate::history::knob(s, id, false);
                self.drag = Some(Drag::Knob { id, y0: self.mouse.1, v0: s.knob(id) });
                self.hint = Cow::Owned(s.knob_text(id));
            }
            Action::PianoLaneMenu => s.lane_menu = true,
            Action::PianoToolsMenu => {
                let (x, y) = self.mouse;
                s.playlist.popup = Some(crate::piano_tools::tools_menu(s, x - 6.0, y + 10.0));
            }
            Action::PianoTarget => {
                // Top-left 4 px left of the name, 11 px below the title text's centre (spec: ui/piano-target-channel.md §2).
                let b = self.hits.iter().rev().find(|h| h.action == Action::PianoTarget).map_or(Rect::new(self.mouse.0, self.mouse.1, 0.0, 0.0), |h| h.rect);
                s.playlist.popup = Some(crate::piano_tools::target_menu(s, b.x + 2.0 - 4.0, b.cy() + 11.0));
            }
            Action::PianoSnapMenu => {
                // Top-left 11 px left of and below the button's centre (spec: ui/piano-snap.md §1).
                let b = self.hits.iter().rev().find(|h| h.action == Action::PianoSnapMenu).map_or(Rect::new(self.mouse.0, self.mouse.1, 0.0, 0.0), |h| h.rect);
                s.playlist.popup = Some(crate::piano_snap::snap_menu(s, b.cx() - 11.0, b.cy() + 11.0));
            }
            Action::PitchStep(dir) => {
                s.piano.pitch_center = piano::clamp_pitch_center(s.piano.pitch_center + dir * piano::PITCH_STEP);
            }
            Action::PianoLanePick(i) => {
                s.piano.lane = piano::Lane::ALL[i.min(piano::Lane::ALL.len() - 1)];
                s.lane_menu = false;
            }
            Action::Settings(_) => {}
            // The rack's own edge mask (right, bottom) as a window's.
            Action::RackResize(edges) if s.is_detached(Win::ChannelRack) => {
                let r = if edges & views::rack::EDGE_RIGHT != 0 { EDGE_R } else { 0 };
                let b = if edges & views::rack::EDGE_BOTTOM != 0 { EDGE_B } else { 0 };
                self.native.push((Win::ChannelRack, Native::Resize(r | b)));
            }
            Action::RackResize(edges) => {
                self.drag = Some(Drag::RackResize { edges, m0: self.mouse, s0: s.rack_size });
            }
            Action::ChannelEnable(_)
            | Action::Step(..)
            | Action::RackOptions
            | Action::RackFilter
            | Action::RackDots
            | Action::RackSpeaker
            | Action::RackLen
            | Action::RackLoop
            | Action::RackGraph
            | Action::RackLoopBox(_)
            | Action::RackGraphArea
            | Action::RackGraphTab(_)
            | Action::RackFill(_) => {}
            Action::WinDrag(win) => {
                // Double-click on the title bar toggles maximized / floating (like the max button).
                let now = std::time::Instant::now();
                let m = self.mouse;
                let double = matches!(self.title_click, Some((w, t, p)) if w == win
                    && now.duration_since(t).as_millis() < 500
                    && (p.0 - m.0).abs() < 4.0 && (p.1 - m.1).abs() < 4.0);
                if double && win.slot().is_some() {
                    self.title_click = None;
                    self.act(Action::MaxWin(win));
                    return;
                }
                self.title_click = Some((win, now, m));
                // A detached window's own title bar moves its native window (the window system drags it).
                if self.state.is_detached(win) {
                    self.native.push((win, Native::Move));
                    return;
                }
                let s = &mut self.state;
                if s.is_max(win) {
                    return;
                }
                if win == Win::Plugin {
                    s.plugin_moved = true;
                }
                let pos = match crate::floating::placed(win) {
                    Some(p) => (p.pos)(s),
                    None => {
                        let w = win;
                        // Start from the rect as shown (kept on screen), then move it.
                        let r = s.float_rect(w);
                        if let Some(i) = w.slot() {
                            s.wins[i].rect = r;
                        }
                        (r.x, r.y)
                    }
                };
                self.drag = Some(Drag::Window { win, dx: self.mouse.0 - pos.0, dy: self.mouse.1 - pos.1 });
            }
            // A detached window's frame, buttons and edges work its native window.
            Action::WinResize(win, edges) if s.is_detached(win) => self.native.push((win, Native::Resize(edges))),
            Action::MinWin(w) if s.is_detached(w) => self.native.push((w, Native::Minimize)),
            Action::MaxWin(w) if s.is_detached(w) => self.native.push((w, Native::Maximize)),
            Action::WinResize(win, edges) => {
                let r0 = s.float_rect(win);
                self.drag = Some(Drag::WinResize { win, edges, m0: self.mouse, r0 });
            }
            Action::MinWin(w) => {
                if let Some(i) = w.slot() {
                    s.wins[i].mini = !s.wins[i].mini;
                }
            }
            Action::MaxWin(w) => {
                if let Some(i) = w.slot() {
                    s.wins[i].mini = false;
                    s.wins[i].maxed = !s.wins[i].maxed;
                }
            }
            // A second one leaves it maximized (spec: ui/toolbar-buttons.md §2).
            Action::ShowMax(w) => {
                s.raise(w);
                if let Some(i) = w.slot() {
                    s.wins[i].mini = false;
                    s.wins[i].maxed = true;
                }
                if w == Win::PianoRoll {
                    piano::center_keys(s, self.piano_grid.map_or(700.0, |g| g.h));
                }
            }
            // No help index yet: the left click does nothing.
            Action::Util(Util::Help) => {}
            Action::Util(Util::Save) => self.menu_item(0, views::menus::FILE_SAVE_AS),
            Action::Util(Util::AudioEditor) => self.audio_editor(0, false),
            Action::Util(Util::NewAudioEditor) => self.audio_editor(0, true),
            Action::CloseWin(Win::Perf) => self.close_perf(),
            Action::CloseWin(Win::Strum) => {
                crate::strum::cancel(s);
                s.hide(Win::Strum);
                self.damage_win(Win::PianoRoll);
            }
            Action::CloseWin(Win::Arp) => {
                crate::arpeggiate::cancel(s);
                s.hide(Win::Arp);
                self.damage_win(Win::PianoRoll);
            }
            Action::CloseWin(Win::PianoDlg) => {
                crate::piano_dlg::cancel(s);
                s.hide(Win::PianoDlg);
                self.damage_win(Win::PianoRoll);
            }
            Action::CloseWin(Win::Events) => {
                crate::events::close(s);
                self.invalidate();
            }
            Action::CloseWin(w) => s.hide(w),
            Action::BrowserRow(_) | Action::BrowserTab(_) | Action::BrowserList | Action::BrowserCollapse | Action::BrowserReread | Action::BrowserMenu | Action::BrowserSearch | Action::BrowserClear | Action::BrowserSplitter => {}
            Action::TypeIn(t) => crate::typein::click(s, t),
            Action::PlaylistArea | Action::MixerArea | Action::RackArea | Action::Swallow => {}
            Action::PlTool(_)
            | Action::PlSnap
            | Action::PlOptions
            | Action::PlRuler
            | Action::PlMarkers
            | Action::PlHeader(_)
            | Action::PlLed(_)
            | Action::PlGroupArrow(_)
            | Action::PlArrangement
            | Action::PlStretch
            | Action::PlPickTab(_)
            | Action::PlPick(_)
            | Action::PlPopup(_)
            | Action::PlPopupSub(_) => {}
        }
    }

    /// Make `i` the focused channel (piano roll, typing keyboard, plugin window follow it, spec: ui/channel-rack.md §8).
    pub(crate) fn focus_channel(&mut self, i: usize) {
        let s = &mut self.state;
        if s.selected_channel != i && s.shown(Win::PianoRoll) {
            s.selected_channel = i;
            s.piano.clear_selection();
            piano::center_keys(s, self.piano_grid.map_or(700.0, |g| g.h));
        }
        s.selected_channel = i;
    }

    /// Name button click (spec: ui/channel-rack.md §13): opens the channel's plugin window, or closes it when open.
    fn toggle_channel_window(&mut self, i: usize) {
        // A channel's container: its window (spec: plugins/patcher/README.md §4.4).
        if self.state.project.channels.get(i).is_some_and(crate::container::is_container_channel) {
            let place = (i as u16, crate::container::CHANNEL);
            if self.state.shown(Win::Container) && self.state.container.at == place {
                self.state.hide(Win::Container);
            } else {
                crate::container::show(&mut self.state, place.0, place.1);
            }
            return;
        }
        if self.state.hosted_rows.contains(&i) {
            if !self.plugins.close_if_open(i) {
                self.state.pending_open = Some(i);
            }
            return;
        }
        let s = &mut self.state;
        let sampler = s.project.channels.get(i).is_some_and(|c| c.kind.is_sampler());
        if !sampler && s.shown(Win::Plugin) && s.plugin_channel == i {
            s.hide(Win::Plugin);
        } else {
            self.open_channel_window(i);
        }
    }

    /// Open rack row `i`'s window (not hosted: Lemon Sampler's for a sampler or audio clip, else the channel window).
    pub(crate) fn open_channel_window(&mut self, i: usize) {
        let s = &mut self.state;
        if s.project.channels.get(i).is_some_and(crate::container::is_container_channel) {
            crate::container::show(s, i as u16, crate::container::CHANNEL);
        } else if s.project.channels.get(i).is_some_and(|c| c.kind.is_sampler()) {
            // Not hosted yet (just added): Lemon Sampler starts on the next tick and its window opens then.
            s.pending_open = Some(i);
            self.sampler_failed.retain(|&r| r != i);
        } else {
            if !s.shown(Win::Plugin) && !s.plugin_moved {
                s.plugin_pos = crate::arrange::channel_home(s);
            }
            s.plugin_channel = i;
            s.raise(Win::Plugin);
        }
    }

    /// Ctrl+Z (`alt` = Ctrl+Alt+Z) and EDIT's entries: a move in the project's undo history (`crate::history`; spec:
    /// ui/settings-choices.md "Undo history: the four checks (2026-10-16)").
    pub(crate) fn undo_key(&mut self, alt: bool) {
        let s = &mut self.state;
        if let Some(m) = if alt { crate::history::ctrl_alt_z(s) } else { crate::history::ctrl_z(s) } {
            self.say(m);
        }
        self.invalidate();
    }

    /// A key's result for the hint bar (step edit, undo): it is the message, and it replaces the hint of what the
    /// pointer rests on until the pointer moves, as step edit's song position does (else a hint over the playlist or
    /// the rack hid it).
    pub(crate) fn say(&mut self, m: String) {
        self.hint = Cow::Owned(m.clone());
        self.state.message = Some(m);
        self.damage_band(HINT_RECT);
    }

    /// ADD > Audio clip / Automation clip (spec: ui/add-clip-channels.md §3): the channel goes last in the rack (kept
    /// in view) and its window opens: an audio clip's settings (Lemon Sampler's window), an automation clip's channel
    /// window.
    pub(crate) fn add_clip_channel(&mut self, kind: ChannelKind) {
        let row = crate::playlist::add_clip_channel(&mut self.state, kind);
        let s = &mut self.state;
        let view_h = views::rack::panel(views::rack::rect(s)).h;
        s.rack_scroll = (views::rack::content_h(s) - view_h).max(0.0);
        self.open_channel_window(row);
        self.invalidate();
    }

    /// Press on a step with the left button, or the `right` one (spec: ui/channel-rack.md §9): selects the row, sets
    /// the step and starts painting. "Right button clears steps" (spec: ui/settings-choices.md "Input:
    /// Click-and-hold and both mouse buttons (2026-10-16)"): off, either button toggles the step; on, the left one sets
    /// it and the right one clears it. Either way the drag that follows sets (left) or clears (right) each step it
    /// crosses.
    fn rack_step(&mut self, i: usize, k: u32, right: bool) {
        self.state.rack.select_only(i);
        self.focus_channel(i);
        let s = &mut self.state;
        s.piano.clear_selection();
        // One undo step for the press and the painting that follows.
        crate::rack::snapshot(s);
        // A step past an explicit length grows the pattern to the end of the pattern's bar holding it (§5, §23).
        if let Some(l) = crate::rack::explicit_len(s) {
            if k >= l {
                let ps = crate::rack::per_step(s);
                crate::rack::set_len(s, Some(s.project.pattern_bar_end(s.pattern, (k + 1) * ps).div_ceil(ps)));
            }
        }
        if crate::prefs::check(&crate::prefs::BOTH_MOUSE_BUTTONS) {
            crate::rack::set_step(s, i, k, !right);
        } else {
            crate::rack::toggle_step(s, i, k);
        }
        self.drag = Some(Drag::RackPaint { row: i, on: !right, last: k });
    }

    /// Speaker (spec: ui/channel-rack.md §7): plays the current pattern; while it plays, pauses / resumes.
    fn rack_speaker(&mut self) {
        match self.transport.state() {
            TransportState::Playing if !self.state.song_mode => self.transport.pause(),
            TransportState::Paused if !self.state.song_mode => self.transport.resume(),
            _ => {
                if self.transport.playing() {
                    self.transport.stop();
                }
                self.state.song_mode = false;
                self.start_playback();
            }
        }
    }

    /// Graph editor click / drag at a screen point (spec: ui/channel-rack.md §14).
    fn rack_graph_at(&mut self, (x, y): (f32, f32)) {
        let Some(b) = views::rack::graph_bars(&self.state) else { return };
        let Some(k) = views::rack::step_at(&self.state, x) else { return };
        let v = ((b.bottom() - y) / b.h).clamp(0.0, 1.0);
        if let Some(t) = crate::rack::graph_set(&mut self.state, k, v) {
            self.hint = Cow::Owned(t);
        }
        self.invalidate();
    }

    /// Channel rack left clicks (spec: ui/channel-rack.md §7-15, §22). Returns whether `a` was one.
    fn rack_act(&mut self, a: &Action) -> bool {
        use crate::rack as rk;
        let (shift, ctrl, alt) = (self.shift, self.ctrl, self.alt);
        let now = std::time::Instant::now();
        let double = matches!(&self.rack_click, Some((p, t)) if p == a && now.duration_since(*t).as_millis() < 400);
        if matches!(a, Action::RackSelect(_) | Action::RackMixer(_)) {
            self.rack_click = if double { None } else { Some((a.clone(), now)) };
        }
        let mouse = self.mouse;
        match *a {
            Action::RackSelect(i) => {
                if double {
                    self.state.rack.sel = rk::shown(&self.state).into_iter().collect();
                } else if shift {
                    let sel = &mut self.state.rack.sel;
                    if !sel.remove(&i) {
                        sel.insert(i);
                    }
                } else {
                    self.state.rack.select_only(i);
                    self.focus_channel(i);
                    self.drag = Some(Drag::RackSel { from: i });
                }
            }
            Action::ChannelSelect(i) => {
                if shift {
                    let at = views::rack::name_rect(&self.state, i).map_or(mouse, |r| (r.x, r.y));
                    rk::open_box(&mut self.state, rk::RBox::Channel(i), at);
                } else if !ctrl {
                    self.state.rack.select_only(i);
                    self.focus_channel(i);
                    self.toggle_channel_window(i);
                }
            }
            Action::ChannelEnable(i) => {
                let s = &mut self.state;
                if ctrl || alt {
                    rk::solo(s, i);
                } else if shift {
                    rk::toggle_lock(s, i);
                } else {
                    rk::toggle_mute(s, i);
                }
            }
            Action::Step(i, k) => self.rack_step(i, k, false),
            Action::RackMixer(i) => {
                let s = &mut self.state;
                if double {
                    if let Some(t) = s.project.channels.get(i).map(|c| c.mixer_track) {
                        s.selected_mixer = t;
                        s.raise(Win::Mixer);
                    }
                } else {
                    let v0 = s.project.channels.get(i).map_or(0, |c| c.mixer_track);
                    self.drag = Some(Drag::RackMixer { ch: i, y0: mouse.1, v0 });
                }
            }
            Action::RackOptions => {
                let p = rk::options_menu(&self.state, self.state.rack_pos.0, self.state.rack_pos.1 + 28.0);
                self.state.playlist.popup = Some(p);
            }
            Action::RackFilter => {
                let p = rk::filter_menu(&self.state, mouse.0, mouse.1 + 8.0);
                self.state.playlist.popup = Some(p);
            }
            Action::RackDots => self.drag = Some(Drag::RackDots { x0: mouse.0, p0: self.state.rack.name_pct }),
            Action::RackSpeaker => self.rack_speaker(),
            Action::RackLen => self.drag = Some(Drag::RackLen { y0: mouse.1, v0: rk::len_value(&self.state) }),
            Action::RackLoop => {
                let s = &mut self.state;
                rk::relayout(s, |s| s.rack.loop_mode = !s.rack.loop_mode);
                s.piano.dirty = true;
            }
            Action::RackGraph => self.state.rack.graph_open = !self.state.rack.graph_open,
            Action::RackLoopBox(i) => self.drag = Some(Drag::RackLoopLen { row: i, y0: mouse.1, v0: rk::loop_box_value(&self.state, i) }),
            Action::RackGraphArea => {
                self.rack_graph_at(mouse);
                self.drag = Some(Drag::RackGraph);
            }
            Action::RackGraphTab(i) => self.state.rack.graph_tab = rk::Tab::ALL[i.min(rk::Tab::ALL.len() - 1)],
            Action::RackFill(h) => rk::fill_hit(&mut self.state, h),
            _ => return false,
        }
        self.invalidate();
        true
    }

    /// Channel rack right clicks (spec: ui/channel-rack.md §8-19). Returns whether `a` was one.
    fn rack_right(&mut self, a: &Action) -> bool {
        use crate::knobs::KnobId;
        use crate::rack as rk;
        let (x, y) = self.mouse;
        let s = &self.state;
        let popup = match *a {
            Action::ChannelSelect(i) => rk::channel_menu(s, i, x, y),
            Action::ChannelEnable(i) => rk::led_menu(s, i, x, y),
            Action::RackMixer(i) => rk::mixer_menu(s, i, x, y),
            Action::Knob(k @ (KnobId::ChanVol(_) | KnobId::ChanPan(_) | KnobId::Swing)) => rk::knob_menu(s, k, self.automation_target(a), x, y),
            Action::RackFilter => rk::filter_edit_menu(s, x, y),
            Action::RackLen => rk::length_menu(s, x, y),
            Action::RackLoop => rk::loop_menu(s, x, y),
            Action::RackSelect(i) => {
                let sel = &mut self.state.rack.sel;
                if !sel.remove(&i) {
                    sel.insert(i);
                }
                return true;
            }
            Action::Step(i, k) => {
                self.rack_step(i, k, true);
                return true;
            }
            Action::RackGraphArea => {
                self.rack_graph_at((x, y));
                self.drag = Some(Drag::RackGraph);
                return true;
            }
            _ => return false,
        };
        self.state.playlist.popup = Some(popup);
        true
    }

    /// Channels whose deletion was confirmed go now (spec: ui/channel-rack.md §18).
    pub(crate) fn rack_pending(&mut self) {
        if let Some(rows) = self.state.rack.delete_now.take() {
            crate::rack::delete_step(self, &rows);
        }
    }

    /// Rack rows were renumbered (`m(old) = new`, `None` = deleted): hosted plugins, their windows and playback follow.
    pub(crate) fn remap_rows(&mut self, m: &dyn Fn(usize) -> Option<usize>) {
        self.typing_release_all();
        self.plugins.remap(m);
        // The rows hosted are the instances' rows.
        self.state.hosted_rows = self.plugins.hosted.range(..crate::plugins::FX_KEY).map(|(&r, _)| r).collect();
        self.sampler_failed = self.sampler_failed.iter().filter_map(|&r| m(r)).collect();
        self.wrap_failed = self.wrap_failed.iter().filter_map(|&k| if k >= crate::plugins::FX_KEY { Some(k) } else { m(k) }).collect();
        self.native_failed = self.native_failed.iter().filter_map(|&k| if k >= crate::plugins::FX_KEY { Some(k) } else { m(k) }).collect();
        self.prepare_plugin_routing();
        self.transport.set_instruments(self.plugins.instruments());
        self.state.piano.dirty = true;
    }

    /// A row's hosted instrument for cloning: (available plugin index, saved state).
    pub(crate) fn plugin_copy(&self, r: usize) -> Option<(usize, Vec<u8>)> {
        let ch = self.state.project.channels.get(r)?;
        let name = match ch.kind {
            ChannelKind::Sampler | ChannelKind::AudioClip => crate::plugins::LEMON_SAMPLER.to_string(),
            // A hosted plugin is found by its identity (a wrapped channel's plugin name is the wrapper's).
            ChannelKind::Plugin => {
                let d = self.plugins.descriptor(r)?;
                let index = self.plugins.index_of_uid(&d.uid())?;
                return Some((index, self.plugins.state(r)?));
            }
            _ => return None,
        };
        let index = self.plugins.index_of(&name)?;
        let blob = self.plugins.hosted.get(&r)?.plugin.lock().ok()?.save_state()?;
        Some((index, blob))
    }

    /// Host a copy of an instrument (`plugin_copy`) on row `row`.
    pub(crate) fn plugin_paste(&mut self, row: usize, index: usize, blob: &[u8]) {
        if self.plugins.create_with(row, index, Some(blob)).is_ok() {
            if !self.state.hosted_rows.contains(&row) {
                self.state.hosted_rows.push(row);
            }
            self.prepare_plugin_routing();
            self.transport.set_instruments(self.plugins.instruments());
        }
    }

    /// A PATTERNS command from the menu bar, the pattern menu or its key: the ones that open dialogs here, the others
    /// in `patterns::run` with a name box at the mouse. Returns a status message.
    pub(crate) fn pattern_cmd(&mut self, cmd: crate::patterns::Cmd) -> Option<String> {
        match cmd {
            crate::patterns::Cmd::TimeSignature => {
                self.ask_time_signature();
                None
            }
            crate::patterns::Cmd::OpenInBrowser => {
                self.browser_jump(crate::browser::Jump::Patterns);
                None
            }
            crate::patterns::Cmd::QuickRender | crate::patterns::Cmd::RenderReplace => {
                self.render_pattern(cmd == crate::patterns::Cmd::RenderReplace);
                None
            }
            crate::patterns::Cmd::RenderAsClip => {
                self.open_render_form();
                None
            }
            _ => {
                let at = crate::patterns::box_at_mouse(self.mouse);
                crate::patterns::run(&mut self.state, cmd, at)
            }
        }
    }

    pub(crate) fn menu_item(&mut self, menu: usize, item: usize) {
        use views::menus::*;
        // FILE's first entries and About go by their place; every other entry by its command (`MenuCmd`), never its
        // label.
        match (menu, item) {
            (0, FILE_NEW) => return self.ask_discard("starting a new project", crate::dialog::Next::NewProject),
            (0, FILE_OPEN) => return self.ask_discard("opening another project", crate::dialog::Next::OpenDialog),
            (0, FILE_SAVE) => return self.save(false),
            (0, FILE_SAVE_AS) => return self.save(true),
            (0, FILE_SAVE_VERSION) => return self.save_new_version(),
            (0, FILE_EXPORT) => return self.open_export(false),
            (0, FILE_EXPORT_MP3) => return self.open_export(true),
            (0, FILE_EXPORT_MIDI) => return self.export_midi_dialog(false),
            (0, FILE_EXIT) => {
                self.want_close = true;
                return;
            }
            (HELP_MENU, HELP_ABOUT) => {
                // The Windows build includes the ASIO SDK, which is under version 3 only (THIRD-PARTY-NOTICES.txt §9).
                let licence = if cfg!(windows) { "the GNU GPL v3" } else { "the GNU GPL v3 or later" };
                self.state.message = Some(format!("Lemon Studio {} (pre-alpha), free software under {licence}", env!("CARGO_PKG_VERSION")));
                return;
            }
            (PATTERNS, i) if i >= PATTERN_PICK => return crate::patterns::select(&mut self.state, (i - PATTERN_PICK) as u16),
            (0, i) if i >= RECENT_PICK => return self.open_recent(i - RECENT_PICK),
            _ => {}
        }
        let Some(entry) = item_at(menu, item) else { return };
        match entry.cmd {
            // Theme import (spec: ui/flstheme.md): pick a .flstheme, starting in FL's theme folder when installed.
            MenuCmd::Settings(tab) => self.open_settings(tab),
            MenuCmd::ExportZip => crate::zipped::export_dialog(self),
            // ADD > Sampler: a new Lemon Sampler channel at the bottom of the rack (spec: ui/plugin-menu.md §1).
            MenuCmd::Sampler => {
                if let Some(index) = self.plugins.index_of(crate::plugins::LEMON_SAMPLER) {
                    self.state.message = crate::plugin_menu::add_generator(self, index, None);
                }
            }
            // ADD > Audio clip / Automation clip (spec: ui/add-clip-channels.md §3).
            MenuCmd::AudioClip => self.add_clip_channel(ChannelKind::AudioClip),
            MenuCmd::AutomationClip => self.add_clip_channel(ChannelKind::Automation),
            // ADD > Layer (spec: ui/add-clip-channels.md §4): the rack keeps the new row in view.
            MenuCmd::Layer => {
                crate::playlist::add_layer_channel(&mut self.state);
                let s = &mut self.state;
                let view_h = views::rack::panel(views::rack::rect(s)).h;
                s.rack_scroll = (views::rack::content_h(s) - view_h).max(0.0);
                self.invalidate();
            }
            MenuCmd::MorePlugins => {
                let at = self.mouse;
                self.open_plugin_menu(crate::plugin_menu::Target::Gen(None), at);
            }
            MenuCmd::Rescan => self.start_scan(crate::plugin_db::Mode::Changed),
            // The plugin menus' display mode, as their own radios set it (spec: ui/plugin-menu.md §2-3).
            MenuCmd::View(v) => self.state.message = crate::plugin_menu::run(self, crate::plugin_menu::PlugCmd::View(v)),
            MenuCmd::Web(url) => {
                crate::help_links::open(url);
                self.state.message = Some(format!("Opening {url}"));
            }
            MenuCmd::SaveMidiClip => self.save_midi_clipboard(),
            MenuCmd::SaveAudioClip => self.save_audio_clipboard(),
            MenuCmd::Diagnostics => self.save_diagnostics(),
            MenuCmd::NewPattern => {
                let at = crate::patterns::box_at_mouse(self.mouse);
                if let Some(m) = crate::patterns::run(&mut self.state, crate::patterns::Cmd::FindFirst, at) {
                    self.state.message = Some(m);
                }
            }
            // spec: ui/cancel-recording.md
            MenuCmd::CancelRecording => self.state.message = Some(self.cancel_recording()),
            MenuCmd::StopSound => {
                self.transport.stop();
                self.state.message = Some("Sound stopped".into());
            }
            // spec: ui/unused-channels.md
            MenuCmd::SelectUnused => self.state.message = crate::rack::run(self, crate::rack::RCmd::SelectUnused),
            MenuCmd::PurgeClips => self.state.message = crate::rack::run(self, crate::rack::RCmd::Purge),
            MenuCmd::AutoBypassAll => self.state.message = crate::mixer::run(self, crate::mixer::MCmd::AutoBypassAll),
            MenuCmd::RemoveTeslas => {
                if let Some(m) = self.ask_remove_teslas() {
                    self.state.message = Some(m);
                }
            }
            MenuCmd::MidiOut => self.state.message = self.add_midi_out(),
            MenuCmd::PrepMidiExport => self.ask_midi_export(),
            MenuCmd::PrepPerformance => {
                if let Some(m) = crate::playlist::run(self, crate::playlist::Cmd::PerfPrepare) {
                    self.state.message = Some(m);
                }
            }
            // spec: ui/piano-tools.md §4 "Lemon Studio"
            MenuCmd::Chords => {
                let at = self.mouse;
                crate::playlist::start_edit(&mut self.state, crate::playlist::EditTarget::Chords, at);
                if let Some(e) = self.state.playlist.edit.as_mut() {
                    e.sel = true;
                }
            }
            // spec: ui/playlist.md "Locking tracks to their clips"
            MenuCmd::LockClips(on) => {
                if let Some(m) = crate::playlist::run(self, crate::playlist::Cmd::LockClips(on)) {
                    self.state.message = Some(m);
                }
            }
            MenuCmd::ResetEmptyTracks => {
                if let Some(m) = crate::playlist::run(self, crate::playlist::Cmd::ResetEmptyTracks) {
                    self.state.message = Some(m);
                }
            }
            // spec: ui/project-templates.md
            MenuCmd::RestoreBackup => self.revert_to_backup(),
            MenuCmd::SaveAsTemplate => self.save_template_dialog(),
            MenuCmd::SaveToNewFolder => self.new_folder_dialog(),
            MenuCmd::DeleteUnused => self.delete_unused_ask(),
            MenuCmd::ClipPalette => {
                let at = self.mouse;
                self.open_project_picker(at);
            }
            MenuCmd::PluginPalette => {
                let at = self.mouse;
                self.open_plugin_menu(crate::plugin_menu::Target::All, at);
            }
            MenuCmd::Pattern(cmd) => {
                if let Some(m) = self.pattern_cmd(cmd) {
                    self.state.message = Some(m);
                }
            }
            MenuCmd::CloseAll(what) => self.close_windows(what),
            MenuCmd::IntoWorkspace => self.move_into_workspace(),
            MenuCmd::AlignEditors => self.align_channel_editors(),
            MenuCmd::Typing => self.toggle_typing(),
            MenuCmd::Metronome => self.toggle_metronome(),
            MenuCmd::AudioLog => self.toggle_audio_log(),
            MenuCmd::DumpAudioLog => {
                let m = self.dump_audio_log();
                self.state.message = Some(m);
            }
            MenuCmd::ClearLog => {
                if let Some(m) = self.rec_cmd(crate::record::RecCmd::ClearLog) {
                    self.state.message = Some(m);
                }
            }
            MenuCmd::Record(o) => self.toggle_record_option(o),
            MenuCmd::Autoscroll => self.toggle_autoscroll(),
            MenuCmd::Groups => self.toggle_groups(),
            MenuCmd::StepEdit => self.toggle_step_edit(),
            MenuCmd::Multilink => self.toggle_multilink(),
            MenuCmd::Midi(sw) => self.toggle_midi_switch(sw),
            MenuCmd::Params(entry) => {
                if let Some(msg) = self.params_menu(entry) {
                    self.state.message = Some(msg);
                }
            }
            // EDIT > Undo = Ctrl+Z, Redo = Ctrl+Alt+Z: the project's undo history (`undo_key`).
            MenuCmd::Undo => self.undo_key(false),
            MenuCmd::Redo => self.undo_key(true),
            // spec: ui/browser-commands.md
            MenuCmd::Jump(j) if j.available() => self.browser_jump(j),
            // VIEW's window entries are window toggles (`Action::ToggleWin`); ▸ entries open their submenu.
            MenuCmd::Jump(_) | MenuCmd::Window(_) | MenuCmd::Sub(_) | MenuCmd::None => {
                self.state.message = Some(format!("{}: not available yet", entry.label));
            }
        }
    }

    /// A ▸ entry of the open dropdown (spec: ui/menus.md "Look"): its submenu, the app's own popup, opens to the right
    /// of the row as the popups' submenus do (kept on screen); the dropdown stays open under it.
    fn open_menu_sub(&mut self, menu: usize, i: usize) {
        let Some(fill) = views::menus::sub_at(menu, i) else { return };
        let at = self.hits.iter().rev().find(|h| h.action == Action::MenuSub(menu, i)).map_or(Rect::new(self.mouse.0, self.mouse.1, 0.0, 0.0), |h| h.rect);
        let items = fill(&self.state);
        self.state.playlist.popup = Some(crate::playlist::Popup { x: at.right(), y: at.y - 2.0, items, sub: None, scroll: [0; 2] });
        self.state.menu_sub = Some(i);
    }
}

impl App {
    /// A Toolbars menu switch (spec: ui/toolbar-menu.md §2-5, §8). Hide (Ctrl+F11) takes the toolbar and menu bar away
    /// and moves the browser and the workspace up to the window's top, the floating windows with them; a second Hide
    /// brings everything back.
    pub(crate) fn toolbar_switch(&mut self, sw: views::toolbar::Switch) {
        use views::toolbar::Switch;
        let s = &mut self.state;
        match sw {
            Switch::Edit => {
                let on = !s.bar.editing;
                return self.toolbar_edit(on);
            }
            Switch::Separators => s.toolbar.separators = !s.toolbar.separators,
            Switch::Flat => s.toolbar.flat = !s.toolbar.flat,
            Switch::HintBar => s.toolbar.hint_bar = !s.toolbar.hint_bar,
            Switch::Hide => {
                s.toolbar.hidden = !s.toolbar.hidden;
                s.bar.editing = false;
                s.menu_open = None;
                s.menu_sub = None;
                let h = views::band_h(s);
                lift_windows(s, if s.toolbar.hidden { -h } else { h });
            }
        }
        if sw != Switch::Hide {
            s.toolbar.save();
        }
        self.invalidate();
    }

    /// The toolbar editor on or off (spec: ui/toolbar-menu.md §8): Toolbars ▸ Edit, the palette's Done. While it is on,
    /// each row starts with a grip, each panel is one thing to drag, and the palette lies under the band, over the
    /// windows. A hidden toolbar comes back for it.
    pub(crate) fn toolbar_edit(&mut self, on: bool) {
        if on && self.state.toolbar.hidden {
            self.toolbar_switch(views::toolbar::Switch::Hide);
        }
        let s = &mut self.state;
        s.bar.editing = on;
        if on {
            s.bar.user = crate::toolbar::user_presets();
        }
        s.menu_open = None;
        s.menu_sub = None;
        self.invalidate();
    }

    /// Puts layout `l` on the toolbar and keeps it (`settings.txt`). When the band's height changes (one row or two),
    /// the workspace's top moves with it, the floating windows too.
    fn set_toolbar_layout(&mut self, l: crate::toolbar::Layout) {
        let s = &mut self.state;
        let before = views::band_h(s);
        s.bar.layout = l;
        s.bar.layout.save();
        let dy = views::band_h(s) - before;
        if !s.toolbar.hidden && dy != 0.0 {
            lift_windows(s, dy);
        }
        self.invalidate();
    }

    /// A press on the toolbar editor (a click for the rows and Done, `acts_on_release`): a panel, a tile, the logo or a
    /// grip starts a drag (`bar_drop` lands it); the preset name opens the preset menu.
    fn bar_hit(&mut self, h: crate::toolbar::BarHit) {
        use crate::toolbar::{BarDrag, BarHit, Moving};
        let (x, y) = self.mouse;
        let r = self.top_hit(x, y).map_or(Rect::new(x, y, 0.0, 0.0), |h| h.rect);
        let k = views::band_k(&self.state);
        let moving = match h {
            BarHit::Item(row, i) => Moving::Item(row, i),
            BarHit::Tile(t) => Moving::Tile(t),
            BarHit::Logo => Moving::Logo,
            BarHit::Grip(row) => Moving::Row(row),
            BarHit::Presets => {
                self.state.bar.user = crate::toolbar::user_presets();
                self.state.playlist.popup = Some(views::menus::editor_presets_popup(&self.state, r.x, r.bottom()));
                return self.invalidate();
            }
            BarHit::Rows(n) => {
                let mut l = self.state.bar.layout.clone();
                l.set_rows(n);
                return self.set_toolbar_layout(l);
            }
            BarHit::Done => return self.toolbar_edit(false),
            BarHit::Palette => return,
        };
        self.drag = Some(Drag::Bar(BarDrag { moving, grab: ((x - r.x) / k, (y - r.y) / k) }));
        self.invalidate();
    }

    /// An editor drag let go: the panel moves, comes off onto the palette or goes onto the toolbar; a row swaps places
    /// with the other (spec: ui/toolbar-menu.md §8).
    fn bar_drop(&mut self, d: crate::toolbar::BarDrag) {
        let width = self.state.screen.0 / views::band_k(&self.state);
        let p = views::toolbar::palette(&self.state, width).rect;
        let to = crate::toolbar::drop_at(&self.state.bar, width, &d, self.band_mouse(), (p.y, p.bottom()));
        let mut l = self.state.bar.layout.clone();
        if crate::toolbar::apply(&mut l, &d, to) {
            self.set_toolbar_layout(l);
        }
        self.invalidate();
    }

    /// A toolbar preset picked in the Toolbars menu or the editor's preset menu: its layout goes on the toolbar.
    pub(crate) fn toolbar_preset(&mut self, id: crate::toolbar::PresetId) {
        use crate::toolbar::{Layout, PresetId, PRESETS};
        let l = match &id {
            PresetId::Factory(i) => PRESETS.get(*i).map(|p| p.layout()),
            PresetId::User(path) => std::fs::read_to_string(path).ok().and_then(|t| Layout::parse(&t)),
        };
        match l {
            Some(l) => {
                self.set_toolbar_layout(l);
                self.state.message = Some(format!("Toolbar preset: {}", self.state.bar.name()));
            }
            None => self.state.message = Some("The toolbar preset could not be read".into()),
        }
        self.invalidate();
    }

    /// The editor's Save preset as...: the save dialog in the user folder's `Toolbar presets` (`toolbar_saved` writes
    /// the file).
    pub(crate) fn toolbar_save_as(&mut self) {
        use crate::filedlg::{FileDlg, Mode};
        use crate::toolbar::EXT;
        let Some(dir) = crate::toolbar::user_dir() else {
            self.state.message = Some("Toolbar presets need a user folder".into());
            return;
        };
        let _ = std::fs::create_dir_all(&dir);
        let start = self.state.bar.user_current().map_or_else(|| "My toolbar".to_string(), |p| p.name.clone());
        let dlg = FileDlg::new(Mode::Save, "Save toolbar preset as", "toolbar preset", crate::dialog::Then::SaveToolbarPreset)
            .filter("Lemon Studio toolbar preset", &[EXT])
            .start(Some(&dir))
            .name(&format!("{start}.{EXT}"));
        self.file_dialog(dlg);
    }

    /// The toolbar's layout written to `path`, chosen in the save dialog; it is then listed with the user's presets.
    fn toolbar_saved(&mut self, path: PathBuf) {
        self.state.message = Some(match crate::toolbar::write_preset(&path, &self.state.bar.layout) {
            Ok(p) => format!("Toolbar preset saved: {}", crate::toolbar::preset_name(&p)),
            Err(e) => format!("Toolbar preset not saved: {e}"),
        });
        self.state.bar.user = crate::toolbar::user_presets();
        self.invalidate();
    }

    /// The editor's Delete preset: asks first. Only a user preset with the current layout can go.
    pub(crate) fn toolbar_delete(&mut self) {
        use crate::dialog::{Answer, Kind, Msg, Then};
        if self.state.bar.layout.factory().is_some() {
            return;
        }
        let Some(p) = self.state.bar.user_current().cloned() else { return };
        let text = vec![("Delete the toolbar preset ".to_string(), false), (p.name, true), ("?".to_string(), false)];
        self.show_msg(Msg::new(Kind::Question, "Delete preset", text, &[Answer::Yes, Answer::No], Then::DeleteToolbarPreset(p.path)));
    }

    /// The delete question answered Yes: the preset's file goes. The layout stays on the toolbar.
    fn toolbar_deleted(&mut self, path: PathBuf) {
        self.state.message = Some(match std::fs::remove_file(&path) {
            Ok(()) => format!("Toolbar preset deleted: {}", crate::toolbar::preset_name(&path)),
            Err(e) => format!("Toolbar preset not deleted: {e}"),
        });
        self.state.bar.user = crate::toolbar::user_presets();
        self.invalidate();
    }
}

/// Moves the floating windows `dy` down with the workspace's top, keeping them below it: the toolbar hidden or shown
/// (spec: ui/toolbar-menu.md §5), its band grown or shrunk by Toolbars scaling (spec: ui/ui-scaling.md §1).
pub(crate) fn lift_windows(s: &mut State, dy: f32) {
    let top = views::work_y(s);
    let lift = |y: &mut f32| *y = (*y + dy).max(top);
    for w in &mut s.wins {
        lift(&mut w.rect.y);
    }
    for pos in [&mut s.rack_pos, &mut s.plugin_pos, &mut s.perf.pos, &mut s.tapper.pos, &mut s.touch.pos, &mut s.strum.pos, &mut s.arp.pos, &mut s.events.pos] {
        lift(&mut pos.1);
    }
    for pos in &mut s.piano_dlg.pos {
        lift(&mut pos.1);
    }
}

/// What a right click on toolbar button `hit` runs on its release (spec: ui/toolbar-buttons.md §2). The Undo button has
/// none until the browser has a History section.
fn toolbar_right(hit: &Action) -> Option<Action> {
    use views::menus::{FILE_SAVE_VERSION, HELP_ABOUT, HELP_MENU};
    Some(match *hit {
        Action::Util(Util::Save) => Action::MenuItem(0, FILE_SAVE_VERSION),
        Action::Util(Util::Help) => Action::MenuItem(HELP_MENU, HELP_ABOUT),
        Action::Util(Util::AudioEditor) => Action::Util(Util::NewAudioEditor),
        Action::ToggleWin(w @ (Win::Playlist | Win::PianoRoll)) => Action::ShowMax(w),
        Action::ToggleWin(Win::Browser) | Action::ProjectPicker => Action::PickerOpen,
        _ => return None,
    })
}

/// PATTERNS menu shortcuts (spec: ui/pattern-menu.md §3).
fn pattern_shortcut(key: &winit::keyboard::Key, ctrl: bool, shift: bool) -> Option<crate::patterns::Cmd> {
    use crate::patterns::Cmd;
    use winit::keyboard::{Key, NamedKey};
    let k = match key {
        // spec: ui/browser-commands.md §4
        Key::Character(c) if ctrl && shift && c.eq_ignore_ascii_case("p") => return Some(Cmd::OpenInBrowser),
        Key::Named(k) => k,
        _ => return None,
    };
    Some(match (k, ctrl, shift) {
        (NamedKey::F4, false, true) => Cmd::FindFirst,
        (NamedKey::F4, false, false) => Cmd::FindNext,
        (NamedKey::F4, true, false) => Cmd::FindNextNoName,
        (NamedKey::F2, false, false) => Cmd::Rename,
        (NamedKey::Insert, true, true) => Cmd::InsertOne,
        (NamedKey::Delete, true, true) => Cmd::Delete,
        (NamedKey::ArrowUp, true, true) => Cmd::MoveUp,
        (NamedKey::ArrowDown, true, true) => Cmd::MoveDown,
        _ => return None,
    })
}

/// Typing keyboard velocity (fixed, spec: ui/typing-keyboard.md).
const TYPING_VELOCITY: u8 = 100;

/// Semitone above the lower tier's first C (C4) for a physical key (spec: ui/typing-keyboard.md).
fn typing_semitone(code: KeyCode) -> Option<i32> {
    use KeyCode::*;
    Some(match code {
        // Lower tier: white keys on the bottom row, black keys on the row above.
        KeyZ => 0, KeyS => 1, KeyX => 2, KeyD => 3, KeyC => 4, KeyV => 5, KeyG => 6, KeyB => 7,
        KeyH => 8, KeyN => 9, KeyJ => 10, KeyM => 11, Comma => 12, KeyL => 13, Period => 14,
        Semicolon => 15, Slash => 16,
        // Upper tier, one octave up.
        KeyQ => 12, Digit2 => 13, KeyW => 14, Digit3 => 15, KeyE => 16, KeyR => 17, Digit5 => 18,
        KeyT => 19, Digit6 => 20, KeyY => 21, Digit7 => 22, KeyU => 23, KeyI => 24, Digit9 => 25,
        KeyO => 26, Digit0 => 27, KeyP => 28, BracketLeft => 29, Equal => 30, BracketRight => 31,
        _ => return None,
    })
}

/// The file just written holds the model's channels and mixer tracks in order: each one's file block is its position
/// (os-flp `apply` follows moved channels and the effect slots of moved mixer tracks by these).
/// Makes the file written to `target` a `.lemon` file (Lemon Studio's chunks around the FLP's, saved by this version,
/// with `extra`: Lemon Studio's own state, `crate::lemon_state`), or a plain FLP for a target named `.flp`, which has no
/// place for `extra` (spec: flp/lemon-format.md).
fn lemon_or_flp(file: &mut os_flp::FlpFile, target: &std::path::Path, mut extra: Vec<os_flp::Event>) -> Result<(), String> {
    if crate::project_files::is_flp(target) {
        os_flp::lemon::to_flp(file);
        return Ok(());
    }
    extra.insert(0, os_flp::lemon::saved_by(&format!("Lemon Studio {}", env!("CARGO_PKG_VERSION"))));
    os_flp::lemon::to_lemon(file, &extra).map_err(|e| e.to_string())
}

fn settle_origins(p: &mut Project) {
    for (i, c) in p.channels.iter_mut().enumerate() {
        c.rack.origin = Some(i as u16);
    }
    for (i, m) in p.mixer.iter_mut().enumerate() {
        m.origin = Some(i as u16);
    }
}

/// An empty sampler channel at rack position `i`.
fn sampler_channel(i: u16, name: &str, mixer_track: u16) -> Channel {
    Channel {
        id: os_model::Id::NONE,
        index: i,
        kind: ChannelKind::Sampler,
        name: Some(name.into()),
        plugin: None,
        plugin_state: None,
        color: None,
        enabled: true,
        volume: 10000.0 / 12800.0,
        pan: 0.0,
        pitch_cents: 0,
        mixer_track,
        sample_path: None,
        layer_children: Vec::new(),
        envelopes: Default::default(),
        rack: Default::default(),
        misc: Default::default(), sound: Default::default(),
    }
}

/// What a plugin window's strip shows for rack row `row` (spec: ui/plugin-window.md).
#[cfg(plugin_windows)]
pub(crate) fn channel_info(p: &Project, row: usize) -> Option<crate::plugin_window::ChannelInfo> {
    let ch = p.channels.get(row)?;
    Some(crate::plugin_window::ChannelInfo {
        name: ch.name.clone().unwrap_or_else(|| format!("Channel {}", ch.index + 1)),
        // The mixer track's own name when it has one, else "Master" / "Insert N".
        target: p
            .mixer
            .iter()
            .find(|t| t.index == ch.mixer_track)
            .and_then(|t| t.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| views::rack::insert_name(ch.mixer_track)),
        enabled: ch.enabled,
        volume: ch.volume,
        pan: ch.pan,
        pitch_cents: ch.pitch_cents,
        range: i32::from(ch.misc.pitch_range),
        track: ch.mixer_track,
        max_track: crate::mixer::max_insert(p),
        misc: Some(crate::misc::vals_of(ch)),
        icon: None,
    })
}

/// The timebase goes from `from` to `to` ticks per quarter (`App::set_timebase`, an undone or redone change): every tick
/// count follows, the project's, the views', the history's and the clipboards'.
pub(crate) fn retime(s: &mut State, from: u16, to: u16) {
    if from == to {
        return;
    }
    s.project.set_ppq(to);
    crate::piano::rescale(s, from, to);
    crate::playlist::rescale(s, from, to);
    crate::history::rescale(s, from, to);
    if let Some(rows) = &mut s.rack.clipboard {
        rows.iter_mut().flatten().for_each(|n| n.rescale(from, to));
    }
    s.piano.dirty = true;
}

/// A generator-plugin channel played by a hosted instrument, routed like `add_channel`. Returns its rack row.
pub(crate) fn add_plugin_channel(p: &mut Project, name: &str) -> usize {
    let row = add_channel(p);
    let ch = &mut p.channels[row];
    ch.kind = ChannelKind::Plugin;
    // The built-in Lemon Trio is stored as the file's own 3x Osc, Citrus as its Sytrus.
    ch.plugin = Some(crate::osc3::file_name(name).to_string());
    ch.name = Some(name.to_string());
    row
}

/// Add-channel button (spec: ui/channel-rack.md §5): an empty sampler routed to the next insert
/// after the highest one in use. Returns its rack row.
pub(crate) fn add_channel(p: &mut Project) -> usize {
    let index = p.channels.iter().map(|c| c.index + 1).max().unwrap_or(0);
    let track = (p.channels.iter().map(|c| c.mixer_track).max().unwrap_or(0) + 1).min(crate::mixer::max_insert(p));
    p.channels.push(sampler_channel(index, crate::plugins::LEMON_SAMPLER, track));
    p.channels.len() - 1
}

/// A new project (owner's choice, 2026-10-10): one empty sampler channel, played and edited by Lemon Sampler, on
/// insert 1, and pattern 1.
pub fn default_project() -> Project {
    template_project(&[crate::plugins::LEMON_SAMPLER])
}

/// The project's bar as the Project page names it: "Time signature: 3/8" or "Time division: 5 beats of 3 steps".
fn bar_text(p: &Project) -> String {
    let (beats, steps) = p.bar();
    if p.bar_is_signature() {
        format!("Time signature: {beats}/{}", views::settings::denominator_text(steps))
    } else {
        format!("Time division: {beats} beats of {steps} steps")
    }
}

/// A one-channel project that plays `path` once from the start (browser sample preview, spec: ui/browser.md §7).
pub(crate) fn preview_project(path: &str) -> Project {
    let mut p = template_project(&["Preview"]);
    p.channels[0].sample_path = Some(path.to_string());
    p.patterns = vec![Pattern { number: 1, notes: vec![Note { length: u32::from(p.ppq.max(4)), ..Note::step(0, 0) }], ..Pattern::default() }];
    p.arrangements.clear();
    p
}

/// A project with no channels: the mixer and pattern 1 of a new one (FILE ▸ Import ▸ MIDI file adds its channels).
pub(crate) fn empty_project() -> Project {
    template_project(&[])
}

/// The old five-channel template (tests that need several channels).
#[cfg(test)]
pub fn test_project() -> Project {
    template_project(&["Kick", "Clap", "Hat", "Snare", "Pluck"])
}

/// Sampler channels named `names` (channel i on insert i + 1), 18 mixer tracks (master, Insert 1-16 and the hidden
/// last one, as FL's default template stores them; spec: ui/mixer.md §1) routed to the master, pattern 1.
fn template_project(names: &[&str]) -> Project {
    let channel = |i: u16, name: &str| sampler_channel(i, name, i + 1);
    let mut mixer: Vec<MixerTrack> = (0..18u16).map(MixerTrack::new).collect();
    // The hidden last track is routed nowhere, as FL stores it (spec: flp/events/mixer.md).
    mixer[17].routes.clear();
    Project {
        ppq: 96,
        tempo: Some(130.0),
        current_pattern: Some(1),
        channels: names.iter().enumerate().map(|(i, n)| channel(i as u16, n)).collect(),
        patterns: vec![Pattern { number: 1, ..Pattern::default() }],
        mixer,
        // FL's default template holds event 30 = 1: "Play truncated notes in clips" is on in a new project (spec:
        // flp/engine-events.md "Project", "Event 30").
        play_truncated: true,
        // Automatic crossfades (event 44) as the program setting says, whatever the template holds (spec:
        // ui/settings-choices.md "Project").
        auto_crossfades: Some(crate::prefs::check(&crate::prefs::AUTO_CROSSFADES_NEW)),
        ..Project::default()
    }
}

fn monitor_cfg_path() -> Option<PathBuf> {
    os_paths::user_file("monitor.txt")
}

/// Monitor settings from `monitor.txt` in the user folder (written by the debug panel's Save), else defaults.
fn load_monitor_cfg() -> crate::monitor::Settings {
    let mut cfg = crate::monitor::Settings::default();
    if let Some(text) = monitor_cfg_path().and_then(|p| std::fs::read_to_string(p).ok()) {
        cfg.load(&text);
    }
    cfg
}

fn cable_cfg_path() -> Option<PathBuf> {
    os_paths::user_file("cables.txt")
}

/// Cable physics factors from `cables.txt` in the user folder (written by the cable debug panel's Save), else
/// defaults.
fn load_cable_cfg() -> crate::cable::Tuning {
    let mut t = crate::cable::Tuning::default();
    if let Some(text) = cable_cfg_path().and_then(|p| std::fs::read_to_string(p).ok()) {
        t.load(&text);
    }
    t
}

/// General settings ▸ Animations from `animations.txt` in the user folder: on unless it says "Off". Tests always
/// animate.
fn load_animations() -> bool {
    if cfg!(test) {
        return true;
    }
    os_paths::user_file("animations.txt").and_then(|f| std::fs::read_to_string(f).ok()).is_none_or(|t| t.trim() != "Off")
}

pub fn save_animations(on: bool) {
    if cfg!(test) {
        return;
    }
    if let Some(dir) = os_paths::user_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("animations.txt"), if on { "Playful" } else { "Off" });
    }
}

fn save_cable_cfg(t: &crate::cable::Tuning) -> Result<PathBuf, String> {
    let p = cable_cfg_path().ok_or("no user folder")?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, t.dump()).map_err(|e| e.to_string())?;
    Ok(p)
}

fn save_monitor_cfg(cfg: &crate::monitor::Settings) -> Result<PathBuf, String> {
    let p = monitor_cfg_path().ok_or("no user folder")?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, cfg.dump()).map_err(|e| e.to_string())?;
    Ok(p)
}

/// Read a `.flstheme` (spec ui/flstheme.md) or a Lemon Studio theme text file.
pub fn theme_from_file(path: &std::path::Path) -> Result<Theme, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&bytes);
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("flstheme")) {
        Ok(skin_theme(Design::Citrus, &name, Some(&text)))
    } else {
        Theme::parse(&name, &text)
    }
}

/// The theme: the saved design's (spec: ui/lemon-design.md); for Classic `theme.txt` in the user folder
/// (`os_paths::user_dir`), else the last imported `current.flstheme`, else the default.
fn load_theme() -> Theme {
    let t = design_theme(saved_design(), true);
    os_theme::set_current(&t.colors);
    t
}

/// The settings folder from before the rename to Lemon Studio (`OpenStudio`, e.g. `%APPDATA%/OpenStudio`) moves to the
/// user folder (`os_paths::user_dir`, e.g. `%APPDATA%/Lemon Studio`) once, at start, so dropped browser folders, presets,
/// the design and the theme carry over (spec: ui/lemon-design.md §12).
pub fn move_old_settings() {
    if cfg!(test) {
        return;
    }
    let (Some(old), Some(new)) = (os_paths::legacy_dir(), os_paths::user_dir()) else { return };
    if old.is_dir() && !new.exists() {
        let _ = std::fs::rename(&old, &new);
    }
}

/// The design named in `design.txt` in the user folder; Lemon when there is none. Tests always use Classic.
pub fn saved_design() -> Design {
    if cfg!(test) {
        return Design::Citrus;
    }
    os_paths::user_file("design.txt")
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| Design::parse(&t))
        .unwrap_or(Design::Lemon)
}

fn save_design(d: Design) {
    if cfg!(test) {
        return;
    }
    if let Some(dir) = os_paths::user_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("design.txt"), d.name());
    }
}

/// The theme of skin `d` (`saved` = coloured by the saved theme files, else the skin's own colours): Lemon takes the
/// saved `current.flstheme`; Citrus `theme.txt` (Lemon Studio's token file, Citrus only), else `current.flstheme`.
pub fn design_theme(d: Design, saved: bool) -> Theme {
    match d {
        Design::Lemon => skin_theme(Design::Lemon, "Lemon", saved.then(saved_fl_text).flatten().as_deref()),
        Design::Citrus if saved => load_theme_inner(),
        Design::Citrus => Theme::default(),
    }
}

/// Skin `d` coloured by the `.flstheme` text `fl` (`None`: the skin's own colours; spec: ui/theme-presets.md "Skins").
pub fn skin_theme(d: Design, name: &str, fl: Option<&str>) -> Theme {
    let th = fl.map(os_theme::FlTheme::parse);
    Theme::new(name, os_theme::design::palette(d, th.as_ref()), os_theme::Adjust::default())
}

/// The text of the `current.flstheme` the Classic design loads at start (`load_theme_inner`; `None` when `theme.txt`
/// comes first, or there is none). Tests have none.
fn saved_fl_text() -> Option<String> {
    let dir = theme_dir()?;
    if dir.join("theme.txt").exists() {
        return None;
    }
    std::fs::read(dir.join("current.flstheme")).ok().map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// The folder `current.flstheme` is kept in: the user folder; in tests this thread's folder, none unless one is set
/// (`THEME_DIR`).
pub(crate) fn theme_dir() -> Option<PathBuf> {
    #[cfg(test)]
    return THEME_DIR.with(|d| d.borrow().clone());
    #[cfg(not(test))]
    os_paths::user_dir()
}

#[cfg(test)]
thread_local! {
    /// Tests: the folder `theme_dir` gives.
    static THEME_DIR: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

fn load_theme_inner() -> Theme {
    if cfg!(test) {
        return Theme::default();
    }
    let Some(dir) = os_paths::user_dir() else { return Theme::default() };
    for f in ["theme.txt", "current.flstheme"] {
        let p = dir.join(f);
        if p.exists() {
            match theme_from_file(&p) {
                Ok(t) => return t,
                Err(e) => eprintln!("theme {f}: {e}; using the default"),
            }
        }
    }
    Theme::default()
}

#[cfg(test)]
mod tests {
    include!("app_tests_1.rs");
    include!("app_tests_2.rs");
}
