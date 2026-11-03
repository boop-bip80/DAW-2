
    use super::*;

    fn frame(app: &mut App, c: &mut Canvas, damage: Option<Rect>) {
        app.paint(c, damage);
    }

    /// Paint what `App::take_damage` returned, as the shell does: each rect, or the whole frame.
    fn frame_damage(app: &mut App, c: &mut Canvas, damage: Option<Option<Vec<Rect>>>) {
        match damage {
            Some(Some(rects)) => app.paint_region(c, Some(&rects)),
            Some(None) => app.paint(c, None),
            None => {}
        }
    }

    /// Slots holding the file's parametric EQ get a Lemon Parametric EQ instance and show its name; adding the EQ
    /// stores the file's EQ (spec: plugins/lemon-parametric-eq-2.md §8).
    #[test]
    fn parametric_eq_slots_are_hosted() {
        use crate::plugins::{fx_key, EQ_FILE_NAME, LEMON_EQ};
        let mut a = App::new(None);
        a.state.project = test_project();
        a.reset_hosted();
        a.plugins.effects = crate::plugins::builtin_effects();
        let mut fl = lemon_parametric_eq_2::params::default_state();
        lemon_parametric_eq_2::params::set_field(&mut fl, 1, -600);
        a.state.project.mixer[2].slots[1] = Some(os_model::SlotPlugin { id: os_model::Id::NONE, name: EQ_FILE_NAME.into(), state: fl.clone() });
        a.sync_effects();
        assert!(a.plugins.hosted.contains_key(&fx_key(2, 1)));
        assert_eq!(crate::mixer::slot_name(&a.state, 2, 1).as_deref(), Some(LEMON_EQ));
        assert_eq!(a.plugins.eq_file_state(fx_key(2, 1)), Some(fl));
        // A second pass changes nothing.
        let before = a.plugins.hosted.len();
        a.sync_effects();
        assert_eq!(a.plugins.hosted.len(), before);
        assert_eq!(crate::plugins::new_slot(LEMON_EQ).state, lemon_parametric_eq_2::params::default_state());
    }

    /// A project's Edison slots are opened as Tesla with their sample and recording settings, show Tesla's name and
    /// keep the file's Edison, whose state Tesla writes back (spec: plugins/tesla/README.md "Opening Edison slots").
    #[test]
    fn edison_slots_open_as_tesla() {
        use crate::plugins::{fx_key, EDISON_FILE_NAME, TESLA};
        use tesla::sample::{Format, Sample};
        let mut a = App::new(None);
        a.state.project = test_project();
        a.reset_hosted();
        a.plugins.effects = crate::plugins::builtin_effects();
        let saved = tesla::state::Saved {
            sample: Sample::from_channels(44100, vec![vec![0.25; 4410], vec![-0.25; 4410]], Format::Int16),
            path: "take.wav".into(),
            rec: tesla::state::Rec { armed: true, ..Default::default() },
            ..Default::default()
        };
        let fl = tesla::state::to_edison(&saved);
        a.state.project.mixer[0].slots[3] = Some(os_model::SlotPlugin { id: os_model::Id::NONE, name: EDISON_FILE_NAME.into(), state: fl });
        a.sync_effects();
        let key = fx_key(0, 3);
        assert!(a.plugins.hosted.contains_key(&key));
        assert_eq!(crate::mixer::slot_name(&a.state, 0, 3).as_deref(), Some(TESLA));
        let back = tesla::state::from_edison(&a.plugins.edison_file_state(key).expect("an Edison state")).unwrap();
        assert_eq!((back.sample.frames(), back.sample.channels(), back.path.as_str(), back.rec.armed), (4410, 2, "take.wav", true));
        a.store_builtin_states();
        let slot = a.state.project.mixer[0].slots[3].clone().unwrap();
        assert_eq!(slot.name, EDISON_FILE_NAME);
        assert_eq!(tesla::state::from_edison(&slot.state).map(|s| s.sample.frames()), Some(4410));
        // A second pass changes nothing.
        let before = a.plugins.hosted.len();
        a.sync_effects();
        assert_eq!(a.plugins.hosted.len(), before);
    }

    /// A project's Love Philter slots are opened as Pulp Filter with their state, show Pulp Filter's name and keep the
    /// file's Love Philter, whose state Pulp Filter writes back (spec: plugins/lemon-pulp-filter.md §6).
    #[test]
    fn love_philter_slots_open_as_pulp_filter() {
        use crate::plugins::{LOVE_PHILTER_FILE_NAME, PULP_FILTER, fx_key};
        use os_engine::effects::love_philter::state::{MASTER, State};
        let mut a = App::new(None);
        a.state.project = test_project();
        a.reset_hosted();
        a.plugins.effects = crate::plugins::builtin_effects();
        let mut st = State::new_default();
        st.globals[MASTER] = 77;
        let fl = st.to_bytes();
        a.state.project.mixer[1].slots[2] = Some(os_model::SlotPlugin {
            id: os_model::Id::NONE,
            name: LOVE_PHILTER_FILE_NAME.into(),
            state: fl.clone(),
        });
        a.sync_effects();
        let key = fx_key(1, 2);
        assert!(a.plugins.hosted.contains_key(&key));
        assert_eq!(
            crate::mixer::slot_name(&a.state, 1, 2).as_deref(),
            Some(PULP_FILTER)
        );
        assert_eq!(a.plugins.love_philter_file_state(key), Some(fl.clone()));
        a.store_builtin_states();
        let slot = a.state.project.mixer[1].slots[2].clone().unwrap();
        assert_eq!(
            (slot.name.as_str(), slot.state),
            (LOVE_PHILTER_FILE_NAME, fl)
        );
        let before = a.plugins.hosted.len();
        a.sync_effects();
        assert_eq!(a.plugins.hosted.len(), before);
        assert_eq!(
            crate::plugins::new_slot(PULP_FILTER).name,
            LOVE_PHILTER_FILE_NAME
        );
    }

    /// A project's Maximus slots are opened as Press with their state, show Press's name and keep the file's Maximus,
    /// whose state Press writes back (spec: plugins/lemon-press.md §6).
    #[test]
    fn maximus_slots_open_as_press() {
        use crate::plugins::{MAXIMUS_FILE_NAME, PRESS, fx_key};
        use os_engine::effects::maximus::state::{P_LMH_MIX, State};
        let mut a = App::new(None);
        a.state.project = test_project();
        a.reset_hosted();
        a.plugins.effects = crate::plugins::builtin_effects();
        let mut st = State::new_default();
        st.params[P_LMH_MIX] = 500;
        let fl = st.to_bytes();
        a.state.project.mixer[1].slots[3] = Some(os_model::SlotPlugin { id: os_model::Id::NONE, name: MAXIMUS_FILE_NAME.into(), state: fl.clone() });
        a.sync_effects();
        let key = fx_key(1, 3);
        assert!(a.plugins.hosted.contains_key(&key));
        assert_eq!(crate::mixer::slot_name(&a.state, 1, 3).as_deref(), Some(PRESS));
        assert_eq!(a.plugins.maximus_file_state(key), Some(fl.clone()));
        a.store_builtin_states();
        let slot = a.state.project.mixer[1].slots[3].clone().unwrap();
        assert_eq!((slot.name.as_str(), slot.state), (MAXIMUS_FILE_NAME, fl));
        assert_eq!(crate::plugins::new_slot(PRESS).name, MAXIMUS_FILE_NAME);
        // A factory preset (a state preset) gives the slot its Maximus state, saved back under the file's name
        // (spec: plugins/factory-presets.md "Press (Maximus) and Pulp Filter").
        let p = crate::presets::factory(PRESS).into_iter().find(|p| p.name == "Mastering/Max loudness").expect("the preset");
        let Ok((_, crate::presets::Body::State(blob))) = crate::presets::load(&p.path, "clap:org.openstudio.press") else { panic!("a state preset") };
        assert!(a.plugins.set_state(key, &blob));
        let max = press::fl_from_plugin_state(&blob).unwrap();
        assert_eq!(a.plugins.maximus_file_state(key), Some(max.clone()));
        a.store_builtin_states();
        assert_eq!(a.state.project.mixer[1].slots[3].clone().unwrap().state, max);
    }

    /// A project's Fruity Reeverb 2, Balance and Chorus slots (user report 2026-10-26: "This slot's plugin is not
    /// hosted") are opened as Lemon Reverb, Lemon Balance and Lemon Chorus with their state, show the Lemon names, and
    /// a save keeps the FL effects, not wrapped, with the state the Lemon effects write back. Stereo Shaper stays the
    /// engine's (its difference output is the slot's alone).
    // spec: plugins/lemon-small-effects.md "Opened projects' slots"
    #[test]
    fn fl_effect_slots_open_as_their_lemon_recreations() {
        use crate::plugins::fx_key;
        let mut reverb = lemon_reverb::params::default_state();
        lemon_reverb::params::set_field(&mut reverb, 4, 70);
        let mut balance = lemon_small_fx::params::default_state(lemon_small_fx::balance::SPEC);
        lemon_small_fx::params::set_field(&mut balance, 0, -64);
        let mut chorus = lemon_small_fx::params::default_state(lemon_small_fx::chorus::SPEC);
        let d = lemon_small_fx::params::field(&chorus, 1);
        lemon_small_fx::params::set_field(&mut chorus, 1, d + 1);
        let cases = [(1u16, 0u8, "Fruity Reeverb 2", "Lemon Reverb", reverb), (2, 1, "Fruity Balance", "Lemon Balance", balance), (3, 2, "Fruity Chorus", "Lemon Chorus", chorus)];
        let mut a = App::new(None);
        a.state.project = test_project();
        a.state.file = None;
        a.state.path = None;
        a.reset_hosted();
        a.plugins.effects = crate::plugins::builtin_effects();
        for (t, k, file, _, fl) in &cases {
            a.state.project.mixer[usize::from(*t)].slots[usize::from(*k)] = Some(os_model::SlotPlugin { id: os_model::Id::NONE, name: (*file).into(), state: fl.clone() });
        }
        let shaper = os_model::SlotPlugin { id: os_model::Id::NONE, name: "Fruity Stereo Shaper".into(), state: Vec::new() };
        a.state.project.mixer[4].slots[0] = Some(shaper.clone());
        a.sync_effects();
        for (t, k, file, lemon, fl) in &cases {
            let key = fx_key(*t, *k);
            assert_eq!(a.plugins.descriptor(key).map(|d| d.name), Some(lemon.to_string()), "{file}");
            assert_eq!(crate::mixer::slot_name(&a.state, *t, *k).as_deref(), Some(*lemon));
            assert_eq!(a.plugins.recreation_file_state(key, file).as_ref(), Some(fl), "{file}: the state as it was opened");
        }
        assert!(!a.plugins.hosted.contains_key(&fx_key(4, 0)), "Stereo Shaper: the engine's");
        let path = std::env::temp_dir().join(format!("lemon-fl-recreations-{}.flp", std::process::id()));
        a.save_to(path.clone());
        assert!(a.state.message.as_deref().is_some_and(|m| m.starts_with("Saved")), "{:?}", a.state.message);
        let saved = os_flp::project::load(&os_flp::FlpFile::parse(&std::fs::read(&path).unwrap()).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        for (t, k, file, _, fl) in &cases {
            let slot = saved.mixer[usize::from(*t)].slots[usize::from(*k)].clone().unwrap();
            assert_eq!((slot.name.as_str(), &slot.state), (*file, fl));
        }
        assert_eq!(saved.mixer[4].slots[0], Some(shaper));
    }

    /// A project's Fruity Flanger and Fruity Phaser slots open as Lemon Flanger and Lemon Phaser with their state, show
    /// the Lemon names, and a save keeps the FL effects with the state the Lemon effects write back.
    // spec: plugins/lemon-small-effects.md "Opened projects' slots", plugins/lemon-flanger.md §5, plugins/lemon-phaser.md §5
    #[test]
    fn fl_flanger_and_phaser_slots_open_as_their_lemon_recreations() {
        use crate::plugins::fx_key;
        let mut flanger = lemon_small_fx::params::default_state(lemon_small_fx::flanger::SPEC);
        lemon_small_fx::params::set_field(&mut flanger, 1, 700);
        let mut phaser = lemon_small_fx::params::default_state(lemon_small_fx::phaser::SPEC);
        lemon_small_fx::params::set_field(&mut phaser, 6, 15);
        let cases = [(1u16, 0u8, "Fruity Flanger", "Lemon Flanger", flanger), (2, 1, "Fruity Phaser", "Lemon Phaser", phaser)];
        let mut a = App::new(None);
        a.state.project = test_project();
        a.state.file = None;
        a.state.path = None;
        a.reset_hosted();
        a.plugins.effects = crate::plugins::builtin_effects();
        for (t, k, file, _, fl) in &cases {
            a.state.project.mixer[usize::from(*t)].slots[usize::from(*k)] = Some(os_model::SlotPlugin { id: os_model::Id::NONE, name: (*file).into(), state: fl.clone() });
        }
        a.sync_effects();
        for (t, k, file, lemon, fl) in &cases {
            let key = fx_key(*t, *k);
            assert_eq!(a.plugins.descriptor(key).map(|d| d.name), Some(lemon.to_string()), "{file}");
            assert_eq!(crate::mixer::slot_name(&a.state, *t, *k).as_deref(), Some(*lemon));
            assert_eq!(a.plugins.recreation_file_state(key, file).as_ref(), Some(fl), "{file}: the state as it was opened");
        }
        let path = std::env::temp_dir().join(format!("lemon-fl-modulation-{}.flp", std::process::id()));
        a.save_to(path.clone());
        assert!(a.state.message.as_deref().is_some_and(|m| m.starts_with("Saved")), "{:?}", a.state.message);
        let saved = os_flp::project::load(&os_flp::FlpFile::parse(&std::fs::read(&path).unwrap()).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        for (t, k, file, _, fl) in &cases {
            let slot = saved.mixer[usize::from(*t)].slots[usize::from(*k)].clone().unwrap();
            assert_eq!((slot.name.as_str(), &slot.state), (*file, fl));
        }
    }

    /// A project's Fruity Peak Controller slot opens as Lemon Peak Controller with its state, and a save keeps the FL
    /// effect with that state and the project's controller links.
    // spec: plugins/lemon-peak-controller.md "Opened projects"
    #[test]
    fn fl_peak_controller_slots_open_as_lemon_peak_controller() {
        use crate::plugins::{fx_key, LEMON_PEAK_CONTROLLER};
        let mut fl = lemon_small_fx::params::default_state(lemon_small_fx::peak_controller::SPEC);
        lemon_small_fx::params::set_field(&mut fl, 3, 40);
        let mut a = App::new(None);
        a.state.project = test_project();
        a.state.file = None;
        a.state.path = None;
        a.reset_hosted();
        a.plugins.effects = crate::plugins::builtin_effects();
        a.state.project.mixer[1].slots[0] = Some(os_model::SlotPlugin { id: os_model::Id::NONE, name: "Fruity Peak Controller".into(), state: fl.clone() });
        let link = os_model::ControllerLink { formula: Some("1-Input".into()), ..os_model::ControllerLink::new(0x7000 + 64, 1, 0x40_0000) };
        a.state.project.controller_links = vec![link.clone()];
        a.sync_effects();
        let key = fx_key(1, 0);
        assert_eq!(a.plugins.descriptor(key).map(|d| d.name), Some(LEMON_PEAK_CONTROLLER.to_string()));
        assert_eq!(a.plugins.recreation_file_state(key, "Fruity Peak Controller").as_ref(), Some(&fl), "the state as it was opened");
        let path = std::env::temp_dir().join(format!("lemon-fl-peak-controller-{}.flp", std::process::id()));
        a.save_to(path.clone());
        assert!(a.state.message.as_deref().is_some_and(|m| m.starts_with("Saved")), "{:?}", a.state.message);
        let saved = os_flp::project::load(&os_flp::FlpFile::parse(&std::fs::read(&path).unwrap()).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        let slot = saved.mixer[1].slots[0].clone().unwrap();
        assert_eq!((slot.name.as_str(), &slot.state), ("Fruity Peak Controller", &fl));
        assert_eq!(saved.controller_links, vec![link]);
    }

    /// A project's Vocodex slot opens as Lemon Vocoder with its state (the parts Lemon Vocoder does not play
    /// included), its sidechain bus takes the input the carrier selector names, and a save writes the same bytes back.
    // spec: plugins/lemon-vocoder.md "Opened projects"
    #[test]
    fn vocodex_slots_open_as_lemon_vocoder() {
        use crate::plugins::{fx_key, LEMON_VOCODER, VOCODEX_FILE_NAME};
        use os_engine::effects::vocoder as dsp;
        let mut fl = dsp::default_state();
        let at = dsp::settings_at(&fl).unwrap();
        // The bands at 20, the carrier on the track's second input, the modulator on its own audio; one byte of a
        // curve Lemon Vocoder does not play changed so the save must keep it.
        fl[at + 4 * dsp::BANDS..at + 4 * dsp::BANDS + 4].copy_from_slice(&20i32.to_le_bytes());
        fl[at + 4 * dsp::MOD_INPUT..at + 4 * dsp::MOD_INPUT + 4].copy_from_slice(&0i32.to_le_bytes());
        fl[at + 4 * dsp::CAR_INPUT..at + 4 * dsp::CAR_INPUT + 4].copy_from_slice(&2i32.to_le_bytes());
        fl[40] ^= 0x10;
        let mut a = App::new(None);
        a.state.project = test_project();
        a.state.file = None;
        a.state.path = None;
        a.reset_hosted();
        a.plugins.effects = crate::plugins::builtin_effects();
        a.state.project.mixer[1].slots[0] = Some(os_model::SlotPlugin { id: os_model::Id::NONE, name: VOCODEX_FILE_NAME.into(), state: fl.clone() });
        assert_eq!(crate::project_plugins::slot_ports(&a.state.project, 1, 0), (0, 2), "the sidechain bus takes the carrier's input");
        a.sync_effects();
        let key = fx_key(1, 0);
        assert_eq!(a.plugins.descriptor(key).map(|d| d.name), Some(LEMON_VOCODER.to_string()));
        assert_eq!(a.plugins.recreation_file_state(key, VOCODEX_FILE_NAME).as_ref(), Some(&fl), "the state as it was opened");
        let path = std::env::temp_dir().join(format!("lemon-vocodex-{}.flp", std::process::id()));
        a.save_to(path.clone());
        assert!(a.state.message.as_deref().is_some_and(|m| m.starts_with("Saved")), "{:?}", a.state.message);
        let saved = os_flp::project::load(&os_flp::FlpFile::parse(&std::fs::read(&path).unwrap()).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        let slot = saved.mixer[1].slots[0].clone().unwrap();
        assert_eq!((slot.name.as_str(), &slot.state), (VOCODEX_FILE_NAME, &fl));
    }

    /// A project's Fruity Formula Controller slot opens as Lemon Formula Controller with its state (the formula and the
    /// comments too), and a save keeps the FL effect with that state and the project's controller links.
    // spec: plugins/lemon-formula-controller.md "Opened projects"
    #[test]
    fn fl_formula_controller_slots_open_as_lemon_formula_controller() {
        use crate::plugins::{fx_key, LEMON_FORMULA_CONTROLLER};
        use os_engine::effects::formula_ctrl::Settings;
        let comment: Vec<u8> = "Sine".encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut tail = vec![4];
        tail.extend(comment);
        tail.push(0);
        let fl = Settings { knobs: [46592, 65536, 32768], formula: "a+Sin(SongTime*Pi)*b*0.5".into(), tail }.to_state();
        let mut a = App::new(None);
        a.state.project = test_project();
        a.state.file = None;
        a.state.path = None;
        a.reset_hosted();
        a.plugins.effects = crate::plugins::builtin_effects();
        a.state.project.mixer[1].slots[0] = Some(os_model::SlotPlugin { id: os_model::Id::NONE, name: "Fruity Formula Controller".into(), state: fl.clone() });
        let link = os_model::ControllerLink::new(0x7000 + 64, 0, 0x40_0000);
        a.state.project.controller_links = vec![link.clone()];
        a.sync_effects();
        let key = fx_key(1, 0);
        assert_eq!(a.plugins.descriptor(key).map(|d| d.name), Some(LEMON_FORMULA_CONTROLLER.to_string()));
        assert_eq!(a.plugins.recreation_file_state(key, "Fruity Formula Controller").as_ref(), Some(&fl), "the state as it was opened");
        let path = std::env::temp_dir().join(format!("lemon-fl-formula-controller-{}.flp", std::process::id()));
        a.save_to(path.clone());
        assert!(a.state.message.as_deref().is_some_and(|m| m.starts_with("Saved")), "{:?}", a.state.message);
        let saved = os_flp::project::load(&os_flp::FlpFile::parse(&std::fs::read(&path).unwrap()).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        let slot = saved.mixer[1].slots[0].clone().unwrap();
        assert_eq!((slot.name.as_str(), &slot.state), ("Fruity Formula Controller", &fl));
        assert_eq!(saved.controller_links, vec![link]);
    }

    /// The toolbar's audio editor button puts Tesla on the master track and shows it; another click shows the same
    /// one, the right click adds another (spec: plugins/tesla/requests.md #5).
    #[test]
    fn the_audio_editor_button_opens_tesla_on_the_master() {
        use crate::mixer::{slot_name, FxReq};
        use crate::plugins::TESLA;
        let mut a = App::new(None);
        a.state.project = test_project();
        a.reset_hosted();
        a.plugins.effects = crate::plugins::builtin_effects();
        let first = a.state.project.mixer[0].slots.iter().position(Option::is_none).unwrap() as u8;
        a.audio_editor(0, false);
        assert_eq!(slot_name(&a.state, 0, first).as_deref(), Some(TESLA));
        assert!(a.state.mixer.pending.iter().any(|r| matches!(r, FxReq::Create { track: 0, slot, .. } if *slot == first)));
        a.state.mixer.pending.clear();
        a.audio_editor(0, false);
        assert_eq!(a.state.mixer.pending, vec![FxReq::Show { track: 0, slot: first }]);
        a.state.mixer.pending.clear();
        a.audio_editor(0, true);
        let teslas = (0..10).filter(|&k| slot_name(&a.state, 0, k).as_deref() == Some(TESLA)).count();
        assert_eq!(teslas, 2);
        assert_eq!(toolbar_right(&Action::Util(Util::AudioEditor)), Some(Action::Util(Util::NewAudioEditor)));
    }

    /// A sample Tesla sends becomes an audio clip at the song position on the first track free there, or the
    /// selected sampler channel's sample (spec: plugins/tesla/README.md "Version 1").
    #[test]
    fn a_sent_sample_lands_on_the_playlist_or_the_channel() {
        let path = std::env::temp_dir().join(format!("openstudio-sent-{}.wav", std::process::id()));
        let s = tesla::sample::Sample::from_channels(44100, vec![vec![0.5; 44100]], tesla::sample::Format::Int16);
        std::fs::write(&path, tesla::wav::write(&s, tesla::wav::Depth::Own)).unwrap();
        let p = path.to_string_lossy().into_owned();
        let mut a = App::new(None);
        a.state.project = test_project();
        a.state.playlist.pos = 384;
        let before = a.state.project.channels.len();
        a.sample_from_plugin(p.clone(), os_hostlink::AudioTo::Playlist);
        assert_eq!(a.state.project.channels.len(), before + 1);
        let clips = crate::playlist::clips(&a.state);
        let i = *a.state.playlist.sel.first().expect("the new clip, selected");
        let clip = clips[i];
        assert_eq!(clip.position, 384);
        assert!(clip.end() > clip.position + 100, "a second long");
        let overlaps = |c: &os_model::Clip| c.track == clip.track && c.position < clip.end() && c.end() > clip.position;
        assert_eq!(clips.iter().filter(|c| overlaps(c)).count(), 1, "alone on its track");
        // To the selected channel: a sampler takes it.
        let row = a.state.project.channels.iter().position(|c| c.kind.is_sampler()).unwrap();
        a.state.selected_channel = row;
        a.sample_from_plugin(p.clone(), os_hostlink::AudioTo::Channel);
        assert!(a.state.project.channels[row].sample_path.as_deref().is_some_and(|s| s.ends_with(".wav")));
        let _ = std::fs::remove_file(&path);
    }

    /// Writes the toolbar with a live-looking signal to OS_MONITOR_SHOT (a look-and-feel check, not an assertion).
    #[test]
    fn new_project_is_one_empty_lemon_sampler() {
        let p = default_project();
        assert_eq!(p.channels.len(), 1);
        let c = &p.channels[0];
        assert_eq!((c.kind, c.name.as_deref(), c.sample_path.as_deref(), c.mixer_track), (ChannelKind::Sampler, Some("Lemon Sampler"), None, 1));
        // Its starting Lemon Sampler state names no sample and carries the channel's envelope records.
        let st = crate::plugins::sampler_state(c);
        let json = String::from_utf8_lossy(&st[8..]).to_string();
        assert!(json.contains("\"envvol_on\":{\"bool\":false}") && json.contains("\"sample-path\":\"\\\"\\\"\""), "{json}");
    }

    /// The whole window at one scale, for looking at text: `OS_TEXT_SHOT=<file.png> OS_TEXT_SHOT_SCALE=1.25`.
    #[test]
    fn text_shot() {
        let Some(out) = std::env::var_os("OS_TEXT_SHOT") else { return };
        let scale: f32 = std::env::var("OS_TEXT_SHOT_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
        let Ok(mut c) = Canvas::new((1600.0 * scale) as u32, (900.0 * scale) as u32, scale) else { return };
        let mut app = App::new(None);
        app.invalidate();
        frame(&mut app, &mut c, None);
        c.pixmap.save_png(std::path::Path::new(&out)).unwrap();
    }

    #[test]
    fn monitor_shot() {
        let Some(out) = std::env::var_os("OS_MONITOR_SHOT") else { return };
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        for mode in [crate::monitor::Mode::Spectrum, crate::monitor::Mode::Scope] {
            app.state.monitor = crate::monitor::Monitor::default();
            app.state.monitor.mode = mode;
            app.state.monitor.debug = true;
            let mut seed = 12345u32;
            let mut phase = 0.0f32;
            app.state.monitor.update(std::time::Instant::now(), 48000, || (0.5, 0.42), |b| {
                for v in b.iter_mut() {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    let noise = (seed >> 8) as f32 / 8388608.0 - 1.0;
                    phase += 0.05;
                    *v = 0.25 * phase.sin() + 0.1 * (phase * 7.3).sin() + 0.05 * noise;
                }
                9 + mode as u64
            });
            app.invalidate();
            frame(&mut app, &mut c, None);
            let path = std::path::Path::new(&out).join(format!("{mode:?}.png"));
            c.pixmap.save_png(path).unwrap();
        }
    }

    /// The toolbar's analyzer columns and peak bars are solid at 150 %: no device column of the panel shows
    /// through between two 1 px columns (spec: ui/ui-scaling.md §4).
    #[test]
    fn monitor_columns_tile_at_fractional_scales() {
        use crate::monitor::{k, Mode, COLS};
        for scale in [1.0, 1.25, 1.5, 1.75] {
            let Ok(mut c) = Canvas::new((1920.0 * scale) as u32, (1080.0 * scale) as u32, scale) else { return };
            let mut app = App::new(None);
            app.state.monitor.mode = Mode::Spectrum;
            app.state.monitor.bars_db = [app.state.monitor.cfg.v[k::DB_TOP]; COLS];
            app.state.monitor.meter_db = [0.0; 2];
            app.invalidate();
            frame(&mut app, &mut c, None);
            if let Some(out) = std::env::var_os("OS_MONITOR_SHOT") {
                c.pixmap.save_png(std::path::Path::new(&out).join(format!("tiles-{scale}.png"))).unwrap();
            }
            let w = c.pixmap.width() as usize;
            let px = c.pixmap.pixels();
            let row = |y: f32, x0: f32, x1: f32| -> Vec<u8> {
                let (dy, d0, d1) = ((y * scale) as usize, (x0 * scale).round() as usize, (x1 * scale).round() as usize);
                px[dy * w + d0..dy * w + d1].iter().map(|p| p.green()).collect()
            };
            // Analyzer: 84 full-height columns from x 881; peak bars: 75 px from x 969 at y 5 and 16 (body rows).
            for (what, y, x0, x1) in [("analyzer", 30.0, 881.0, 965.0), ("peak L", 9.0, 969.0, 1043.0), ("peak R", 20.0, 969.0, 1043.0)] {
                let got = row(y, x0, x1);
                assert!(got.iter().all(|&g| g > 0x80), "{what} at {scale}: gaps in {got:?}");
            }
            // The tip column is shaded on purpose (0.58): stay clear of it above. Rows tile too.
            let (d0, d1) = ((5.0 * scale).round() as usize, (15.0 * scale).round() as usize);
            let col: Vec<u8> = (d0..d1).map(|y| px[y * w + (1000.0 * scale) as usize].green()).collect();
            assert!(col.iter().all(|&g| g > 0x80), "peak L rows at {scale}: {col:?}");
            let (d0, d1) = ((16.0 * scale).round() as usize, (26.0 * scale).round() as usize);
            let col: Vec<u8> = (d0..d1).map(|y| px[y * w + (1000.0 * scale) as usize].green()).collect();
            assert!(col.iter().all(|&g| g > 0x80), "peak R rows at {scale}: {col:?}");
        }
    }

    /// Knobs drag vertically (1/1024 per px), step with the wheel, and model-backed ones write the project.
    /// A project that was never opened from a file saves as a new FLP that loads back (spec: flp/container.md).
    #[test]
    fn new_project_saves_and_reloads() {
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.file = None;
        app.state.path = None;
        app.state.tempo = 111.0;
        app.state.project.patterns[0].notes.push(Note::step(2, 48));
        app.state.project.patterns.push(Pattern { number: 2, name: Some("Two".into()), notes: vec![Note::step(0, 0)], ..Pattern::default() });
        // A changed mixer is kept: the save gives the new file a playlist, which the mixer is stored after.
        app.state.project.mixer[3].name = Some("Drums".into());
        app.state.project.mixer[3].volume = Some(0.5);
        let path = std::env::temp_dir().join(format!("openstudio-new-{}.flp", std::process::id()));
        app.save_to(path.clone());
        assert!(app.state.message.as_deref().is_some_and(|m| m.starts_with("Saved") && !m.contains("not stored")), "{:?}", app.state.message);
        let file = os_flp::FlpFile::parse(&std::fs::read(&path).unwrap()).unwrap();
        if let Some(keep) = std::env::var_os("OS_KEEP_FLP") {
            let _ = std::fs::copy(&path, keep); // for checking the file in FL by hand
        }
        let back = os_flp::project::load(&file).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(back.tempo, Some(111.0));
        assert_eq!(back.channels.len(), 5);
        assert_eq!(back.patterns[0].notes.len(), 1);
        assert_eq!(back.patterns[1].name.as_deref(), Some("Two"));
        assert_eq!(back.mixer.len(), 18);
        assert_eq!((back.mixer[3].name.as_deref(), back.mixer[3].volume), (Some("Drums"), Some(0.5)));
        // Saving again goes through apply on the same file.
        app.state.project.patterns[0].notes.push(Note::step(1, 96));
        app.save_to(path.clone());
        let back = os_flp::project::load(&os_flp::FlpFile::parse(&std::fs::read(&path).unwrap()).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(back.patterns[0].notes.len(), 2);
        assert_eq!(back.mixer[3].name.as_deref(), Some("Drums"));
    }

    /// Settings > Project (spec: ui/settings-choices.md "Project", audio/song-structure.md): the Numerator and
    /// Denominator combos list their choices with the project's signature marked, and a pick changes the project's bar
    /// at once (events 17, 18 = beats per bar, 16 / denominator steps per beat; the pattern lengths the editors use
    /// follow; the project counts as changed); "Play truncated notes in clips" shows and switches the project's
    /// setting, on in a new project. Both survive a save and reopen, of a new project and of an opened one.
    #[test]
    fn project_settings_time_signature_and_truncated_notes() {
        use crate::playlist::Mark;
        use crate::views::settings::{SAct, DENOMINATOR, NUMERATOR, PROJECT, TRUNCATED_CHECK};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        (app.state.file, app.state.path) = (None, None);
        // A note ending at tick 300 of the current pattern: one 4/4 bar.
        app.state.project.patterns[0].notes = vec![Note { length: 276, ..Note::step(0, 24) }];
        assert_eq!(crate::piano::pattern_length(&app.state, app.state.pattern), 384);
        app.mark_saved();
        app.open_settings(PROJECT);
        frame(&mut app, &mut c, None);
        let shown = |app: &App, label: &str| {
            views::settings::page(PROJECT, &app.state).into_iter().flat_map(|g| g.rows).find_map(|r| match r {
                views::settings::Row::Combo(v, _, l) if l == label => Some(v),
                _ => None,
            })
        };
        assert_eq!((shown(&app, NUMERATOR).as_deref(), shown(&app, DENOMINATOR).as_deref()), (Some("4"), Some("4")), "4/4 when the file has none");
        let pick = |app: &mut App, c: &mut Canvas, label: &str, choice: &str, count: usize| {
            let r = app.hits.iter().rev().find(|h| matches!(&h.action, Action::Settings(SAct::Combo(l, ..)) if *l == label)).map(|h| h.rect).unwrap_or_else(|| panic!("{label} works"));
            click(app, c, r);
            let p = app.state.playlist.popup.as_ref().unwrap_or_else(|| panic!("{label} opens a menu"));
            assert_eq!(p.items.len(), count, "{label}");
            let now = shown(app, label).unwrap();
            assert!(p.items.iter().all(|it| it.mark == Mark::Radio(it.label == now)), "{label}: the current value is marked");
            let i = p.items.iter().position(|it| it.label == choice).unwrap_or_else(|| panic!("{choice} offered"));
            let r = hit_rect(app, &Action::PlPopup(i));
            click(app, c, r);
        };
        pick(&mut app, &mut c, NUMERATOR, "3", 16);
        assert_eq!(app.state.project.bar, Some((3, 4)), "three beats of four sixteenths");
        assert_eq!(app.state.message.as_deref(), Some("Time signature: 3/4"));
        assert!(app.unsaved(), "a change to the project");
        // Patterns are whole bars of the signature: 288 ticks in 3/4.
        assert_eq!(crate::piano::pattern_length(&app.state, app.state.pattern), 576);
        assert_eq!(crate::playlist::source_len(&app.state, os_model::ClipSource::Pattern(app.state.pattern)), 576);
        pick(&mut app, &mut c, DENOMINATOR, "8", 4);
        assert_eq!(app.state.project.bar, Some((3, 2)), "an eighth is two sixteenths");
        assert_eq!((shown(&app, NUMERATOR).as_deref(), shown(&app, DENOMINATOR).as_deref()), (Some("3"), Some("8")));
        assert_eq!(crate::piano::pattern_length(&app.state, app.state.pattern), 432);
        // Play truncated notes: on in a new project, as in FL's default template (spec: flp/engine-events.md "Event
        // 30"), on File > New too; a click switches it.
        assert!(default_project().play_truncated);
        let r = hit_rect(&app, &Action::Settings(SAct::Group("Advanced")));
        click(&mut app, &mut c, r);
        assert!(!app.hits.iter().any(|h| h.action == Action::Placeholder(TRUNCATED_CHECK)), "it works");
        let r = hit_rect(&app, &Action::Settings(SAct::Check(TRUNCATED_CHECK)));
        let on = |app: &App| views::settings::page(PROJECT, &app.state).into_iter().flat_map(|g| g.rows).find_map(|r| match r {
            views::settings::Row::Check(k, v) if k == TRUNCATED_CHECK => Some(v),
            _ => None,
        });
        assert_eq!((app.state.project.play_truncated, on(&app)), (true, Some(true)));
        for want in [false, true] {
            click(&mut app, &mut c, r);
            assert_eq!((app.state.project.play_truncated, on(&app)), (want, Some(want)));
        }
        // Saved and opened again.
        let path = std::env::temp_dir().join(format!("lemon-project-settings-{}.flp", std::process::id()));
        app.save_to(path.clone());
        assert!(app.state.message.as_deref().is_some_and(|m| m.starts_with("Saved")), "{:?}", app.state.message);
        assert!(!app.unsaved());
        let mut opened = App::new(None);
        opened.open(path.clone());
        assert_eq!((opened.state.project.bar, opened.state.project.play_truncated), (Some((3, 2)), true));
        // Changed in the opened project and saved over it (the file's events patched), then opened again.
        crate::playlist::run(&mut opened, crate::playlist::Cmd::Bar(7, 1));
        opened.settings_act(SAct::Check(TRUNCATED_CHECK));
        assert!(opened.unsaved());
        opened.save_to(path.clone());
        let mut again = App::new(None);
        again.open(path.clone());
        let _ = std::fs::remove_file(&path);
        assert_eq!((again.state.project.bar, again.state.project.play_truncated), (Some((7, 1)), false));
        assert_eq!(again.state.project.signature(), Some((7, 16)));
    }

    /// A theme's workspace picture (spec: ui/flstheme.md §3, BackPicFilename): named relative to the theme file, it is
    /// made absolute when the theme is taken in and covers the workspace right of the browser, without the watermark
    /// or the version line; the solid colour mode (BackMode=1) leaves the plain workspace. A temp user folder, never
    /// the real one.
    #[test]
    fn theme_picture_covers_the_workspace() {
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let dir = std::env::temp_dir().join(format!("lemon-theme-picture-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("pics")).unwrap();
        THEME_DIR.with(|d| *d.borrow_mut() = Some(dir.clone()));
        let mut pm = tiny_skia::Pixmap::new(4, 2).unwrap();
        pm.fill(tiny_skia::Color::from_rgba8(0x7a, 0x3c, 0x91, 0xff));
        std::fs::write(dir.join("pics").join("wall.png"), pm.encode_png().unwrap()).unwrap();
        let file = dir.join("Wall.flstheme");
        std::fs::write(&file, format!("{}BackPicFilename=pics\\wall.png\n", os_theme::fl::DARK_TEXT)).unwrap();
        let mut app = App::new(None);
        // No windows: the whole workspace shows.
        app.state.z.clear();
        // The workspace's pixels, right of the browser and below the toolbar.
        let work = |app: &mut App, c: &mut Canvas| -> Vec<u32> {
            frame(app, c, None);
            let (left, top) = (views::browser_w(&app.state) as usize, views::work_y(&app.state) as usize);
            let mut out = vec![0u32; 1920 * 1080];
            c.copy_to_xrgb(&mut out);
            (top..1080).flat_map(|y| out[y * 1920 + left..(y + 1) * 1920].to_vec()).collect()
        };
        let plain = work(&mut app, &mut c);
        let ws = os_theme::Palette::default().workspace;
        let ws = u32::from(ws.r) << 16 | u32::from(ws.g) << 8 | u32::from(ws.b);
        assert!(plain.iter().any(|&p| p != ws), "the logo and the version line on the plain workspace");
        app.load_theme_file_ex(&file, false);
        let kept = app.state.fl_text.as_deref().and_then(|t| os_theme::fl::get_key(t, "BackPicFilename")).map(std::path::PathBuf::from);
        assert_eq!(kept, Some(dir.join("pics").join("wall.png")), "made absolute against the theme file's folder");
        let pic = work(&mut app, &mut c);
        let off = pic.iter().filter(|&&p| p != 0x7a3c91).count();
        assert_eq!(off, 0, "the picture everywhere, no logo or version line");
        // The solid colour mode: the plain workspace again.
        std::fs::write(&file, format!("{}BackPicFilename=pics\\wall.png\nBackMode=1\n", os_theme::fl::DARK_TEXT)).unwrap();
        app.load_theme_file_ex(&file, false);
        assert!(work(&mut app, &mut c) == plain);
        THEME_DIR.with(|d| *d.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Theme ▸ Options (spec: ui/settings-choices.md "Light mode and clip note colours"): the clip note colours check
    /// shows the applied theme's value (the stock Dark theme's when none is applied); a click flips it in that theme,
    /// keeps it as `current.flstheme` rewriting only its own key, and leaves the palette alone. There is no Light mode
    /// check (removed 2026-10-18). A temp user folder, never the real one.
    #[test]
    fn theme_options_toggle_the_applied_theme() {
        use crate::views::settings::{SAct, NOTE_COLORS_CHECK, THEME};
        use os_theme::{FlTheme, Palette};
        let dir = std::env::temp_dir().join(format!("lemon-theme-options-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        THEME_DIR.with(|d| *d.borrow_mut() = Some(dir.clone()));
        let saved = || std::fs::read_to_string(dir.join("current.flstheme")).unwrap_or_default();
        let checks = |app: &App| -> Vec<(&'static str, bool)> {
            let rows = views::settings::page(THEME, &app.state).into_iter().flat_map(|g| g.rows);
            rows.filter_map(|r| match r {
                views::settings::Row::Check(k, v) => Some((k, v)),
                _ => None,
            })
            .collect()
        };
        let mut app = App::new(None);
        assert_eq!((app.design(), app.state.fl_text.as_deref()), (Design::Citrus, None));
        assert_eq!(checks(&app), [(NOTE_COLORS_CHECK, false)], "only the clip note colours check");
        // Clip note colours: stored and saved; new clips' channels follow it (`App::clip_colors`); the palette stays.
        app.settings_act(SAct::Check(NOTE_COLORS_CHECK));
        assert!(app.state.fl_theme().override_clips && saved().contains("OverrideClips=1"));
        app.tick();
        assert!(app.clip_colors().note_colors && app.state.clip_colors.note_colors);
        assert_eq!((checks(&app), &app.theme.colors), (vec![(NOTE_COLORS_CHECK, true)], &Palette::default()));
        app.settings_act(SAct::Check(NOTE_COLORS_CHECK));
        assert!(!app.state.fl_theme().override_clips && saved().contains("OverrideClips=0"));
        // An imported theme with Lightmode=0 (a BOM, CRLF lines) draws as with Light mode on; a click on the page
        // rewrites only its own key.
        let file = dir.join("Night.flstheme");
        let night = "\u{feff}Hue=-18\r\nSaturation=6\r\nLightness=-57\r\nContrast=40\r\nLightmode=0\r\nSelected=-4546441\r\n";
        std::fs::write(&file, night).unwrap();
        app.load_theme_file_ex(&file, false);
        let lit = night.replace("Lightmode=0", "Lightmode=1");
        assert_eq!(app.theme.base, os_theme::fl::palette(&FlTheme::parse(&lit)));
        if let Ok(mut c) = Canvas::new(1920, 1080, 1.0) {
            app.open_settings(THEME);
            frame(&mut app, &mut c, None);
            assert!(!app.hits.iter().any(|h| h.action == Action::Settings(SAct::Check("Light mode"))));
            let r = hit_rect(&app, &Action::Settings(SAct::Check(NOTE_COLORS_CHECK)));
            click(&mut app, &mut c, r);
        } else {
            app.settings_act(SAct::Check(NOTE_COLORS_CHECK));
        }
        let want = format!("{night}OverrideClips=1\r\n");
        assert_eq!(saved(), want);
        // The next start reads it back.
        assert_eq!(App::new(None).state.fl_text.as_deref(), Some(want.as_str()));
        THEME_DIR.with(|d| *d.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Settings > Theme's Skins and Themes (spec: ui/theme-presets.md "Settings"): a theme colours either skin and
    /// stays when the skin changes; Default gives the skin its own colours; the chosen tiles follow; every tile has a
    /// preview. A temp user folder, never the real one.
    #[test]
    fn skins_and_themes_are_chosen_apart() {
        use crate::views::settings::{SAct, THEME};
        use os_theme::{presets::PRESETS, FlTheme, Palette};
        let dir = std::env::temp_dir().join(format!("lemon-skins-themes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        THEME_DIR.with(|d| *d.borrow_mut() = Some(dir.clone()));
        let mut app = App::new(None);
        app.open_settings(THEME);
        let st = &app.state.settings;
        assert_eq!((st.skin_previews.len(), st.theme_previews.len()), (2, PRESETS.len() + 2));
        assert_eq!(st.themes[..2].iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["Lemon", "Citrus"]);
        assert_eq!((st.skin, st.preset), (1, 1), "Citrus skin, no theme: the Citrus theme's tile");
        // A theme in Citrus, then the Lemon skin keeps it.
        let (name, text) = PRESETS[2];
        app.settings_act(SAct::Preset(4));
        assert_eq!(app.theme.colors, os_theme::fl::palette(&FlTheme::parse(text)));
        assert_eq!(std::fs::read_to_string(dir.join("current.flstheme")).unwrap(), text);
        app.settings_act(SAct::Skin(0));
        assert_eq!(app.design(), Design::Lemon);
        assert_eq!(app.theme.colors, os_theme::design::palette(Design::Lemon, Some(&FlTheme::parse(text))));
        assert_eq!((app.state.settings.skin, app.state.settings.preset), (0, 4), "{name}");
        assert_eq!(app.state.settings.theme_previews[4], app.theme.colors, "the tile previews what is applied");
        // The Citrus theme on the Lemon skin: Lemon's shapes, the stock look's colours; kept across a skin switch.
        let lemon_own = os_theme::design::palette(Design::Lemon, None);
        app.settings_act(SAct::Preset(1));
        assert!(app.theme.colors.style.lemon() && app.theme.colors != lemon_own);
        assert_eq!(app.theme.colors.a(0xa8e44a), os_theme::Color::rgb(0xa8e44a), "the stock look's green selection");
        assert_eq!(app.state.settings.preset, 1);
        app.settings_act(SAct::Skin(1));
        assert_eq!((&app.theme.colors, app.state.settings.preset), (&Palette::default(), 1));
        // The Lemon theme on the Citrus skin: Citrus's shapes, Lemon's colours.
        app.settings_act(SAct::Preset(0));
        assert!(!app.theme.colors.style.lemon());
        assert_eq!(app.theme.colors.toolbar, lemon_own.toolbar);
        assert_eq!(app.theme.colors.a(0xa8e44a), lemon_own.a(0xa8e44a), "Lemon's Main as the selection");
        assert_eq!(app.state.settings.preset, 0);
        app.settings_act(SAct::Skin(0));
        assert_eq!((&app.theme.colors, app.state.settings.preset), (&lemon_own, 0));
        // Reset: no theme, the skin's own colours; the theme file is dropped and the skin's own tile is framed.
        app.settings_act(SAct::Button("Reset"));
        assert_eq!(app.theme.colors, lemon_own);
        assert!(app.state.fl_text.is_none() && !dir.join("current.flstheme").exists());
        assert_eq!(app.state.settings.preset, 0);
        app.settings_act(SAct::Skin(1));
        assert_eq!((&app.theme.colors, app.state.settings.preset), (&Palette::default(), 1));
        // Ctrl+Shift+D no longer switches the skin (owner, 2026-10-17).
        app.shift = true;
        app.key(&winit::keyboard::Key::Character("d".into()), true, false);
        app.shift = false;
        assert_eq!(app.design(), Design::Citrus);
        // The page draws both lists of tiles with their hits.
        if let Ok(mut c) = Canvas::new(1920, 1080, 1.0) {
            frame(&mut app, &mut c, None);
            for a in [SAct::Skin(0), SAct::Skin(1), SAct::Preset(0), SAct::Preset(PRESETS.len() + 1)] {
                hit_rect(&app, &Action::Settings(a));
            }
        }
        THEME_DIR.with(|d| *d.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Save preset... writes the applied theme into the user `themes` folder under a typed name and frames its new tile;
    /// a tile's right-click menu duplicates any theme, and renames or deletes the user's own (spec:
    /// ui/theme-presets.md §D).
    #[test]
    fn user_themes_are_saved_renamed_and_deleted() {
        use crate::playlist::ThemeCmd;
        use crate::views::settings::{SAct, SAVE_PRESET, THEME};
        use winit::keyboard::{Key, NamedKey};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let dir = std::env::temp_dir().join(format!("lemon-theme-library-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        THEME_DIR.with(|d| *d.borrow_mut() = Some(dir.clone()));
        let themes = dir.join("themes");
        let mut app = App::new(None);
        app.open_settings(THEME);
        app.settings_act(SAct::Preset(3));
        app.theme_knob_wheel(0, 2);
        frame(&mut app, &mut c, None);
        // The buttons are below the tiles: scroll to the end.
        let t0 = hit_rect(&app, &Action::Settings(SAct::Preset(0)));
        app.mouse_move(t0.cx(), t0.cy());
        app.wheel(0.0, -40.0, false, false);
        frame(&mut app, &mut c, None);
        let r = hit_rect(&app, &Action::Settings(SAct::Button(SAVE_PRESET)));
        click(&mut app, &mut c, r);
        let name = |app: &App| app.state.settings.name_edit.as_ref().map(|e| e.text.text.clone());
        assert_eq!(name(&app), Some(format!("{} custom", app.state.settings.themes[3].0)));
        // A built-in theme's name is refused; the panel stays.
        app.state.settings.name_edit.as_mut().unwrap().text.set("frost");
        app.key(&Key::Named(NamedKey::Enter), false, false);
        assert!(app.state.settings.name_edit.is_some() && !themes.exists());
        app.state.settings.name_edit.as_mut().unwrap().text.set("Mine");
        app.key(&Key::Named(NamedKey::Enter), false, false);
        assert!(app.state.settings.name_edit.is_none());
        assert_eq!(std::fs::read_to_string(themes.join("Mine.flstheme")).ok(), app.state.fl_text.clone());
        let st = &app.state.settings;
        let mine = st.themes.iter().position(|(n, _)| n == "Mine").expect("its tile");
        assert_eq!((st.user_from, st.preset), (mine, mine), "after the built-in tiles, and framed");
        // The menu: Duplicate only on a built-in tile; all four on the user's.
        app.wheel(0.0, 40.0, false, false);
        frame(&mut app, &mut c, None);
        let t0 = hit_rect(&app, &Action::Settings(SAct::Preset(0)));
        app.mouse_move(t0.cx(), t0.cy());
        app.mouse_down_right();
        assert_eq!(app.state.playlist.popup.as_ref().map(|p| p.items.len()), Some(1));
        app.state.playlist.popup = None;
        app.theme_menu(mine, 100.0, 100.0);
        assert_eq!(app.state.playlist.popup.as_ref().map(|p| p.items.len()), Some(4));
        app.state.playlist.popup = None;
        // Duplicate, rename, delete.
        app.theme_cmd(ThemeCmd::Duplicate(0));
        assert_eq!(std::fs::read_to_string(themes.join("Lemon copy.flstheme")).ok().as_deref(), Some(app.state.settings.themes[0].1.as_str()));
        let mine = app.state.settings.themes.iter().position(|(n, _)| n == "Mine").unwrap();
        app.theme_cmd(ThemeCmd::Rename(mine));
        assert_eq!(name(&app).as_deref(), Some("Mine"));
        app.state.settings.name_edit.as_mut().unwrap().text.set("Lemon copy");
        app.key(&Key::Named(NamedKey::Enter), false, false);
        assert!(app.state.settings.name_edit.is_some(), "a clash with another of the user's themes is refused");
        app.key(&Key::Named(NamedKey::Escape), false, false);
        assert!(app.state.settings.name_edit.is_none() && app.state.settings.open, "Esc closes the panel, not the window");
        app.theme_cmd(ThemeCmd::Rename(mine));
        app.state.settings.name_edit.as_mut().unwrap().text.set("Ours");
        app.key(&Key::Named(NamedKey::Enter), false, false);
        assert!(themes.join("Ours.flstheme").exists() && !themes.join("Mine.flstheme").exists());
        let ours = app.state.settings.themes.iter().position(|(n, _)| n == "Ours").unwrap();
        assert_eq!(app.state.settings.preset, ours, "still framed");
        app.theme_cmd(ThemeCmd::Delete(ours));
        app.name_ok();
        assert!(!themes.join("Ours.flstheme").exists());
        assert!(app.state.settings.themes.iter().all(|(n, _)| n != "Ours"));
        THEME_DIR.with(|d| *d.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y on the Theme page step back and forward through theme changes; a run of wheel
    /// notches on one knob is one step (spec: ui/theme-presets.md §D).
    #[test]
    fn theme_changes_undo_and_redo() {
        use crate::views::settings::{SAct, THEME};
        use winit::keyboard::Key;
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let dir = std::env::temp_dir().join(format!("lemon-theme-undo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        THEME_DIR.with(|d| *d.borrow_mut() = Some(dir.clone()));
        let mut app = App::new(None);
        app.open_settings(THEME);
        let none = app.theme.colors.clone();
        app.settings_act(SAct::Preset(2));
        app.settings_act(SAct::Preset(5));
        let five = app.state.fl_text.clone();
        frame(&mut app, &mut c, None);
        let hue = hit_rect(&app, &Action::Settings(SAct::ThemeKnob(0)));
        app.mouse_move(hue.cx(), hue.cy());
        for _ in 0..3 {
            app.wheel(0.0, 1.0, false, false);
        }
        let wheeled = app.state.fl_text.clone();
        assert_ne!(wheeled, five);
        let z = Key::Character("z".into());
        app.key(&z, true, false);
        assert_eq!(app.state.fl_text, five, "the three notches are one step");
        assert_eq!(app.state.settings.preset, 5);
        app.key(&z, true, false);
        assert_eq!(app.state.settings.preset, 2);
        app.key(&z, true, false);
        assert!(app.state.fl_text.is_none() && !dir.join("current.flstheme").exists());
        assert_eq!(app.theme.colors, none, "the skin's own colours");
        app.shift = true;
        app.key(&z, true, false);
        app.shift = false;
        assert_eq!(app.state.settings.preset, 2);
        app.key(&Key::Character("y".into()), true, false);
        app.key(&Key::Character("y".into()), true, false);
        assert_eq!(app.state.fl_text, wheeled);
        assert_eq!(std::fs::read_to_string(dir.join("current.flstheme")).ok(), wheeled, "kept");
        // A new change empties the redo list.
        app.key(&z, true, false);
        app.settings_act(SAct::Preset(7));
        app.key(&Key::Character("y".into()), true, false);
        assert_eq!(app.state.settings.preset, 7);
        THEME_DIR.with(|d| *d.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A theme whose text is too faint on its surfaces gets a warning on the Theme page; Fix text changes only its text
    /// colour so the text reads (spec: ui/theme-presets.md §D).
    #[test]
    fn faint_text_is_warned_and_fixed() {
        use crate::views::settings::{SAct, FIX_TEXT, THEME};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let dir = std::env::temp_dir().join(format!("lemon-theme-faint-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        THEME_DIR.with(|d| *d.borrow_mut() = Some(dir.clone()));
        let mut app = App::new(None);
        // On Citrus, which draws a theme's text colour as given.
        app.set_design(Design::Citrus, false);
        app.open_settings(THEME);
        app.settings_act(SAct::Preset(3));
        assert!(app.state.settings.unreadable.is_none());
        // Text the grey of the surfaces (with no Text adjustment moving it).
        let faint = os_theme::fl::set_key(os_theme::fl::DARK_TEXT, "TextColor", &os_theme::fl::colour_value("TextColor", os_theme::Color::rgb(0x2a2e30)).to_string());
        app.theme_edit(faint.clone());
        assert!(app.state.settings.unreadable.is_some());
        frame(&mut app, &mut c, None);
        let r = hit_rect(&app, &Action::Settings(SAct::Button(FIX_TEXT)));
        click(&mut app, &mut c, r);
        assert!(app.state.settings.unreadable.is_none());
        let now = app.state.fl_text.clone().unwrap();
        assert_eq!(os_theme::fl::remove_key(&now, "TextColor"), os_theme::fl::remove_key(&faint, "TextColor"));
        frame(&mut app, &mut c, None);
        assert!(!app.hits.iter().any(|h| h.action == Action::Settings(SAct::Button(FIX_TEXT))), "the warning is gone");
        THEME_DIR.with(|d| *d.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Theme ▸ Adjustments (spec: ui/theme-presets.md §C): the knobs show the applied theme's values and edit them live
    /// (drag, wheel, middle click back to the chosen theme's value), its tile stays framed, and the theme is kept for the
    /// next start; a colour circle opens the picker, whose square sets the colour live, Revert goes back, Esc closes it
    /// before the window, and a click elsewhere closes it.
    #[test]
    fn theme_adjustments_edit_the_applied_theme() {
        use crate::theme_adjust::{knob_value, KNOBS, METERS};
        use crate::views::settings::{SAct, THEME};
        use os_theme::{presets::PRESETS, FlTheme};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let dir = std::env::temp_dir().join(format!("lemon-theme-adjust-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        THEME_DIR.with(|d| *d.borrow_mut() = Some(dir.clone()));
        let mut app = App::new(None);
        app.open_settings(THEME);
        app.settings_act(SAct::Preset(2));
        let base = PRESETS[0].1;
        frame(&mut app, &mut c, None);
        // Brightness: dragged up 30 px = a tenth of its 510 range.
        let knob = hit_rect(&app, &Action::Settings(SAct::ThemeKnob(2)));
        let v0 = knob_value(&FlTheme::parse(base), 2);
        app.mouse_move(knob.cx(), knob.cy());
        app.mouse_down();
        app.mouse_move(knob.cx(), knob.cy() - 30.0);
        let v = knob_value(&app.state.fl_theme(), 2);
        assert_eq!(v, KNOBS[2].at(KNOBS[2].pos(v0) + 0.1));
        assert_eq!(app.theme.colors, os_theme::design::palette(app.design(), Some(&app.state.fl_theme())), "live");
        assert_eq!(app.state.settings.preset, 2, "the tile it came from stays framed");
        app.mouse_up();
        assert_eq!(std::fs::read_to_string(dir.join("current.flstheme")).unwrap(), app.state.fl_text.clone().unwrap(), "kept");
        // The wheel steps Hue by 5 degrees; a middle click restores the theme's value.
        frame(&mut app, &mut c, None);
        let hue = hit_rect(&app, &Action::Settings(SAct::ThemeKnob(0)));
        app.mouse_move(hue.cx(), hue.cy());
        app.wheel(0.0, 1.0, false, false);
        assert_eq!(app.state.fl_theme().surface.hue, knob_value(&FlTheme::parse(base), 0) + 5.0);
        app.mouse_down_middle();
        assert_eq!(app.state.fl_theme().surface.hue, knob_value(&FlTheme::parse(base), 0));
        app.mouse_move(knob.cx(), knob.cy());
        app.mouse_down_middle();
        assert!(app.state.fl_theme().same_colours(&FlTheme::parse(base)), "back to the theme as chosen");
        // Meters: the picker opens on its first colour; the square's top right corner is full saturation and brightness.
        frame(&mut app, &mut c, None);
        let r = hit_rect(&app, &Action::Settings(SAct::Swatch(METERS as u8, 0)));
        click(&mut app, &mut c, r);
        assert_eq!(app.state.settings.picker.as_ref().map(|p| (p.swatch, p.part)), Some((METERS, 0)));
        let r = hit_rect(&app, &Action::Settings(SAct::PickPart(2)));
        click(&mut app, &mut c, r);
        let sv = hit_rect(&app, &Action::Settings(SAct::PickSv));
        let h = app.state.settings.picker.as_ref().unwrap().hsv[0];
        app.mouse_move(sv.right() - 0.01, sv.y);
        app.mouse_down();
        app.mouse_up();
        assert_eq!(app.state.fl_theme().meters[2], crate::theme_adjust::from_hsv([h, 1.0, 1.0]));
        // Revert, then Esc closes the picker and leaves the window open.
        frame(&mut app, &mut c, None);
        let r = hit_rect(&app, &Action::Settings(SAct::PickRevert));
        click(&mut app, &mut c, r);
        assert!(app.state.fl_theme().same_colours(&FlTheme::parse(base)));
        app.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape), false, false);
        assert!(app.state.settings.picker.is_none() && app.state.settings.open);
        // A click elsewhere on the page closes it too.
        frame(&mut app, &mut c, None);
        let r = hit_rect(&app, &Action::Settings(SAct::Swatch(0, 0)));
        click(&mut app, &mut c, r);
        assert!(app.state.settings.picker.is_some());
        let r = hit_rect(&app, &Action::Settings(SAct::ThemeKnob(4)));
        click(&mut app, &mut c, r);
        assert!(app.state.settings.picker.is_none());
        THEME_DIR.with(|d| *d.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The rows of the Project page, flattened.
    fn project_rows(app: &App) -> Vec<views::settings::Row> {
        views::settings::page(views::settings::PROJECT, &app.state).into_iter().flat_map(|g| g.rows).collect()
    }

    /// Picks `choice` from the menu of the live Project combo `label` (it must offer `count` choices).
    fn project_pick(app: &mut App, c: &mut Canvas, label: &str, choice: &str, count: usize) {
        use crate::views::settings::SAct;
        let r = app.hits.iter().rev().find(|h| matches!(&h.action, Action::Settings(SAct::Combo(l, ..)) if *l == label)).map(|h| h.rect).unwrap_or_else(|| panic!("{label} works"));
        click(app, c, r);
        let p = app.state.playlist.popup.as_ref().unwrap_or_else(|| panic!("{label} opens a menu"));
        assert_eq!(p.items.len(), count, "{label}");
        let i = p.items.iter().position(|it| it.label == choice).unwrap_or_else(|| panic!("{choice} offered"));
        let r = hit_rect(app, &Action::PlPopup(i));
        click(app, c, r);
    }

    /// Settings > Project ▸ Set as (spec: ui/settings-choices.md "Set as time division", flp/engine-events.md
    /// "Project"): time division swaps the Numerator / Denominator combos for the Beats per bar (1..32) and Steps per
    /// beat (1..16) boxes the mouse wheel changes; each mode keeps its own last bar while the program runs (4/4 at
    /// first), switching writes it into the project (event 35 with it), and a bar is never converted. Saved and opened
    /// again, the page shows the mode and the bar the file holds.
    #[test]
    fn project_settings_set_as_time_division() {
        use crate::playlist::Mark;
        use crate::views::settings::{Row, SAct, BEATS_PER_BAR, DENOMINATOR, NUMERATOR, PROJECT, SET_AS, SET_AS_DIVISION, SET_AS_SIGNATURE, STEPS_PER_BEAT};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        (app.state.file, app.state.path) = (None, None);
        app.mark_saved();
        app.open_settings(PROJECT);
        frame(&mut app, &mut c, None);
        let combo = |app: &App, label: &str| project_rows(app).into_iter().find_map(|r| match r {
            Row::Combo(v, _, l) if l == label => Some(v),
            _ => None,
        });
        let value = |app: &App, label: &str| project_rows(app).into_iter().find_map(|r| match r {
            Row::Value(v, l) if l == label => Some(v),
            _ => None,
        });
        let shown = |app: &App| (combo(app, SET_AS), combo(app, NUMERATOR), combo(app, DENOMINATOR), value(app, BEATS_PER_BAR), value(app, STEPS_PER_BEAT));
        let some = |v: &str| Some(v.to_string());
        assert_eq!(shown(&app), (some(SET_AS_SIGNATURE), some("4"), some("4"), None, None), "a file without event 35 is a time signature");
        // The menu: FL's two choices, the current one marked.
        let r = app.hits.iter().rev().find(|h| matches!(&h.action, Action::Settings(SAct::Combo(l, ..)) if *l == SET_AS)).map(|h| h.rect).expect("Set as works");
        click(&mut app, &mut c, r);
        let items: Vec<_> = app.state.playlist.popup.as_ref().expect("a menu").items.iter().map(|it| (it.label.to_string(), it.mark.clone())).collect();
        assert_eq!(items, vec![(SET_AS_DIVISION.to_string(), Mark::Radio(false)), (SET_AS_SIGNATURE.to_string(), Mark::Radio(true))]);
        app.state.playlist.popup = None;
        frame(&mut app, &mut c, None);
        // 3/8 as a time signature, then time division: 4 and 4 at first.
        project_pick(&mut app, &mut c, NUMERATOR, "3", 16);
        project_pick(&mut app, &mut c, DENOMINATOR, "8", 4);
        project_pick(&mut app, &mut c, SET_AS, SET_AS_DIVISION, 2);
        assert_eq!((app.state.project.bar, app.state.project.bar_as_signature), (Some((4, 4)), Some(false)));
        assert_eq!(shown(&app), (some(SET_AS_DIVISION), None, None, some("4"), some("4")));
        assert!(!app.hits.iter().any(|h| matches!(&h.action, Action::Settings(SAct::Combo(l, ..)) if *l == NUMERATOR)), "no combos");
        // The wheel: up adds one, down takes one, inside the box's range.
        let wheel = |app: &mut App, c: &mut Canvas, label: &'static str, dy: f32, times: usize| {
            let r = hit_rect(app, &Action::Settings(SAct::Number(label)));
            app.mouse_move(r.cx(), r.cy());
            for _ in 0..times {
                app.wheel(0.0, dy, false, false);
            }
            frame(app, c, None);
        };
        wheel(&mut app, &mut c, BEATS_PER_BAR, 1.0, 1);
        wheel(&mut app, &mut c, STEPS_PER_BEAT, -1.0, 1);
        assert_eq!(app.state.project.bar, Some((5, 3)));
        assert_eq!(app.state.message.as_deref(), Some("Time division: 5 beats of 3 steps"));
        assert_eq!((value(&app, BEATS_PER_BAR), value(&app, STEPS_PER_BEAT)), (some("5"), some("3")));
        // Five beats of three sixteenths: 360 ticks, what the editors' pattern lengths use.
        assert_eq!(app.state.project.bar_ticks(), 360);
        wheel(&mut app, &mut c, STEPS_PER_BEAT, -1.0, 5);
        wheel(&mut app, &mut c, BEATS_PER_BAR, 1.0, 40);
        assert_eq!(app.state.project.bar, Some((32, 1)), "1..32 and 1..16");
        wheel(&mut app, &mut c, STEPS_PER_BEAT, 1.0, 20);
        assert_eq!(app.state.project.bar, Some((32, 16)));
        wheel(&mut app, &mut c, BEATS_PER_BAR, -1.0, 27);
        wheel(&mut app, &mut c, STEPS_PER_BEAT, -1.0, 13);
        assert_eq!(app.state.project.bar, Some((5, 3)));
        // Each mode comes back with its own bar; none is converted.
        project_pick(&mut app, &mut c, SET_AS, SET_AS_SIGNATURE, 2);
        assert_eq!((app.state.project.bar, app.state.project.bar_as_signature), (Some((3, 2)), Some(true)));
        assert_eq!(shown(&app), (some(SET_AS_SIGNATURE), some("3"), some("8"), None, None));
        project_pick(&mut app, &mut c, SET_AS, SET_AS_DIVISION, 2);
        assert_eq!(app.state.project.bar, Some((5, 3)));
        assert!(app.unsaved());
        // Saved and opened again: time division, five beats of three steps.
        let path = std::env::temp_dir().join(format!("lemon-project-division-{}.flp", std::process::id()));
        app.save_to(path.clone());
        let mut opened = App::new(None);
        opened.open(path.clone());
        let _ = std::fs::remove_file(&path);
        assert_eq!((opened.state.project.bar, opened.state.project.bar_as_signature), (Some((5, 3)), Some(false)));
        opened.open_settings(PROJECT);
        assert_eq!(shown(&opened), (some(SET_AS_DIVISION), None, None, some("5"), some("3")));
        // A time-signature file whose steps per beat make no signature still shows (and plays) its bar.
        opened.state.project.bar_as_signature = Some(true);
        assert_eq!((combo(&opened, NUMERATOR), combo(&opened, DENOMINATOR)), (some("5"), some("5.33")));
        assert_eq!(opened.state.project.bar_ticks(), 360);
    }

    /// Settings > Project ▸ Timebase (PPQ) (spec: ui/settings-choices.md "Project"): FL's eleven choices, the
    /// project's marked; a pick rescales every tick count of the project at once (notes, pattern controls, clips,
    /// markers; automation data stays) and asks nothing. The editors follow (marker, song position, selections,
    /// clipboards, undo steps), so an undo after it restores the new timebase's ticks. Saved, the file's header holds
    /// the new PPQ; choosing the old one again gives back the same project.
    #[test]
    fn project_settings_timebase_rescales() {
        use crate::playlist::Mark;
        use crate::views::settings::{SAct, PROJECT, TIMEBASE, TIMEBASES};
        use os_model::{Arrangement, Clip, ClipSource, TimeMarker};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        (app.state.file, app.state.path) = (None, None);
        let note = Note { length: 276, ..Note::step(0, 24) };
        app.state.project.patterns[0].notes = vec![note];
        app.state.project.arrangements = vec![Arrangement {
            clips: vec![Clip::new(192, 384, 0, ClipSource::Pattern(1))],
            markers: vec![TimeMarker::new(96, "Verse")],
            ..Default::default()
        }];
        // Editor state in ticks: the piano roll's marker, clipboard and an undo step; the song position and selection.
        app.state.piano.marker = 48;
        app.state.piano.clipboard = vec![note];
        crate::piano::snapshot(&mut app.state);
        app.state.project.patterns[0].notes[0].position = 72;
        app.state.playlist.pos = 96;
        app.state.playlist.range = Some((96, 192));
        app.mark_saved();
        let before = app.state.project.clone();
        app.open_settings(PROJECT);
        frame(&mut app, &mut c, None);
        let r = app.hits.iter().rev().find(|h| matches!(&h.action, Action::Settings(SAct::Combo(l, ..)) if *l == TIMEBASE)).map(|h| h.rect).expect("Timebase works");
        click(&mut app, &mut c, r);
        let items = &app.state.playlist.popup.as_ref().expect("a menu").items;
        assert_eq!(items.iter().map(|it| it.label.to_string()).collect::<Vec<_>>(), TIMEBASES.iter().map(u16::to_string).collect::<Vec<_>>());
        assert!(items.iter().all(|it| it.mark == Mark::Radio(it.label == "96")));
        assert_eq!(TIMEBASES, &[24, 48, 72, 96, 120, 144, 168, 192, 384, 768, 960]);
        app.state.playlist.popup = None;
        frame(&mut app, &mut c, None);
        project_pick(&mut app, &mut c, TIMEBASE, "192", 11);
        let p = &app.state.project;
        assert_eq!(p.ppq, 192);
        assert_eq!((p.patterns[0].notes[0].position, p.patterns[0].notes[0].length), (144, 552));
        let a = &p.arrangements[0];
        assert_eq!((a.clips[0].position, a.clips[0].length, a.markers[0].position), (384, 768, 192));
        assert_eq!(app.state.message.as_deref(), Some("Timebase: 192 PPQ"));
        assert!(app.unsaved());
        assert_eq!((app.state.piano.marker, app.state.piano.clipboard[0].position), (96, 48));
        assert_eq!((app.state.playlist.pos, app.state.playlist.range), (192, Some((192, 384))));
        // The pattern is still one 4/4 bar long, in the new ticks.
        assert_eq!(crate::piano::pattern_length(&app.state, app.state.pattern), 768);
        // The change is one step, "Project settings": undone, the old timebase and its ticks, and the undo step made
        // before it then works in those; redone, both come back in the new ticks.
        assert_eq!(crate::history::undo(&mut app.state).as_deref(), Some(crate::history::PROJECT_SETTINGS));
        assert_eq!((app.state.project.ppq, app.state.project.patterns[0].notes[0].position), (96, 72));
        assert_eq!((app.state.piano.marker, app.state.playlist.pos), (48, 96));
        assert!(crate::piano::undo(&mut app.state));
        assert_eq!(app.state.project.patterns[0].notes[0].position, 24);
        crate::history::redo(&mut app.state);
        crate::history::redo(&mut app.state);
        assert_eq!((app.state.project.ppq, app.state.project.patterns[0].notes[0].position), (192, 144));
        assert!(std::mem::take(&mut app.state.retimed), "playback stops");
        // Back to 96: the same project as before (every tick was doubled, none rounded).
        project_pick(&mut app, &mut c, TIMEBASE, "96", 11);
        assert!(app.state.project == before && !app.unsaved());
        // Saved at 72: the header holds the new PPQ, the ticks are the new ones (rounded to the nearest).
        project_pick(&mut app, &mut c, TIMEBASE, "72", 11);
        let path = std::env::temp_dir().join(format!("lemon-project-timebase-{}.flp", std::process::id()));
        app.save_to(path.clone());
        let back = os_flp::project::load(&os_flp::FlpFile::parse(&std::fs::read(&path).unwrap()).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!((back.ppq, back.patterns[0].notes[0].position, back.patterns[0].notes[0].length), (72, 54, 207));
        assert_eq!((back.arrangements[0].clips[0].position, back.arrangements[0].markers[0].position), (144, 72));
    }

    /// Settings > Project ▸ Panning law, Fast declick for cut groups, Automatic crossfades (events 23, 40, 44; spec:
    /// flp/engine-events.md "Project") show the project's values; fast declick is not played yet, so greyed.
    #[test]
    fn project_settings_show_the_unplayed_settings() {
        use crate::views::settings::Row;
        let mut app = App::new(None);
        app.state.project.auto_crossfades = None;
        let get = |app: &App| {
            let rows = project_rows(app);
            let pan = rows.iter().find_map(|r| match r {
                Row::Combo(v, _, "Panning law") => Some(v.clone()),
                _ => None,
            });
            let declick = rows.iter().find_map(|r| match r {
                Row::Check("Fast fade for choke groups", v) => Some(*v),
                _ => None,
            });
            let fades = rows.iter().find_map(|r| match r {
                Row::Check2("Automatic crossfades", v, ..) => Some(*v),
                _ => None,
            });
            (pan, declick, fades)
        };
        assert_eq!(get(&app), (Some("Circular".into()), Some(false), Some(false)), "a project without the events");
        let p = &mut app.state.project;
        (p.panning_law, p.fast_declick, p.auto_crossfades) = (Some(2), Some(true), Some(true));
        assert_eq!(get(&app), (Some("Triangular".into()), Some(true), Some(true)));
        assert!(views::settings::PROJECT_COMBOS.contains(&"Panning law"), "the panning law works");
    }

    /// Settings > Project ▸ Playlist (spec: ui/settings-choices.md "Project", the storage table): "Enable automatic
    /// crossfades in new projects" is a program setting, on by default, and a new project's Automatic crossfades (event
    /// 44) takes it, both ways; opening a file keeps the file's 44 (or its lack of one) whatever the setting.
    /// "Automatic crossfades" switches the project's 44, which counts as a change and is saved.
    #[test]
    fn project_settings_automatic_crossfades() {
        use crate::prefs::{self, AUTO_CROSSFADES_NEW};
        use crate::views::settings::{Row, SAct, CROSSFADES_CHECK, PROJECT};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        assert!(prefs::check(&AUTO_CROSSFADES_NEW), "on by default");
        assert_eq!(default_project().auto_crossfades, Some(true));
        let mut app = App::new(None);
        app.open_settings(PROJECT);
        frame(&mut app, &mut c, None);
        // The program setting: a click switches it, and File > New follows it both ways.
        let r = hit_rect(&app, &Action::Settings(SAct::Check(AUTO_CROSSFADES_NEW.label)));
        click(&mut app, &mut c, r);
        assert!(!prefs::check(&AUTO_CROSSFADES_NEW));
        assert_eq!(app.state.message.as_deref(), Some("Enable automatic crossfades in new projects: off"));
        app.new_project();
        assert_eq!(app.state.project.auto_crossfades, Some(false));
        click(&mut app, &mut c, r);
        assert!(prefs::check(&AUTO_CROSSFADES_NEW));
        app.new_project();
        assert_eq!(app.state.project.auto_crossfades, Some(true));
        assert!(!app.unsaved());
        // The project's check shows and switches event 44.
        frame(&mut app, &mut c, None);
        let shown = |app: &App| {
            project_rows(app).into_iter().find_map(|r| match r {
                Row::Check2(CROSSFADES_CHECK, v, ..) => Some(v),
                _ => None,
            })
        };
        assert_eq!(shown(&app), Some(true));
        assert!(!app.hits.iter().any(|h| h.action == Action::Placeholder(CROSSFADES_CHECK)), "it works");
        let r = hit_rect(&app, &Action::Settings(SAct::Check(CROSSFADES_CHECK)));
        click(&mut app, &mut c, r);
        assert_eq!((app.state.project.auto_crossfades, shown(&app)), (Some(false), Some(false)));
        assert_eq!(app.state.message.as_deref(), Some("Automatic crossfades: off"));
        assert!(app.unsaved());
        // Saved (44 = 0) and opened with the setting on: the file's value. Switched on there and saved over it, then
        // opened with the setting off: on.
        let path = std::env::temp_dir().join(format!("lemon-project-crossfades-{}.flp", std::process::id()));
        app.save_to(path.clone());
        let mut opened = App::new(None);
        opened.open(path.clone());
        assert_eq!(opened.state.project.auto_crossfades, Some(false));
        opened.settings_act(SAct::Check(CROSSFADES_CHECK));
        assert!(opened.unsaved());
        opened.save_to(path.clone());
        prefs::set_check(&AUTO_CROSSFADES_NEW, false);
        let mut again = App::new(None);
        again.open(path.clone());
        assert_eq!(again.state.project.auto_crossfades, Some(true));
        // A file without event 44 opens without one (shown off), whatever the setting.
        let mut p = test_project();
        p.auto_crossfades = None;
        std::fs::write(&path, os_flp::project::create(&p).unwrap().to_bytes().unwrap()).unwrap();
        for on in [false, true] {
            prefs::set_check(&AUTO_CROSSFADES_NEW, on);
            let mut app = App::new(None);
            app.open(path.clone());
            assert_eq!((app.state.project.auto_crossfades, shown(&app)), (None, Some(false)), "setting {on}");
        }
        let _ = std::fs::remove_file(&path);
    }

    /// Settings > Audio ▸ Mixer (spec: ui/settings-choices.md "Transport: Play truncated notes and Reset plugins"):
    /// "Play truncated notes on transport" is on and "Reset plugins on transport" off by default; a click switches
    /// each (kept as a program setting), and the next start of playback goes by them.
    #[test]
    fn audio_settings_transport_checks() {
        use crate::prefs::{self, PLAY_TRUNCATED_ON_TRANSPORT as TRUNCATED, RESET_PLUGINS_ON_TRANSPORT as RESET};
        use crate::transport::Starts;
        use crate::views::settings::{SAct, AUDIO};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        assert_eq!((prefs::check(&TRUNCATED), prefs::check(&RESET)), (true, false), "the defaults");
        assert_eq!(Starts::from_prefs(), Starts { truncated: true, reset: false, count_in: 0 });
        let mut app = App::new(None);
        app.open_settings(AUDIO);
        frame(&mut app, &mut c, None);
        for (check, said) in [(&TRUNCATED, "Play truncated notes on transport: off"), (&RESET, "Reset plugins on transport: on")] {
            assert!(!app.hits.iter().any(|h| h.action == Action::Placeholder(check.label)), "{} works", check.label);
            let r = hit_rect(&app, &Action::Settings(SAct::Check(check.label)));
            click(&mut app, &mut c, r);
            assert_eq!(app.state.message.as_deref(), Some(said));
        }
        assert_eq!((prefs::check(&TRUNCATED), prefs::check(&RESET)), (false, true));
        assert_eq!(Starts::from_prefs(), Starts { truncated: false, reset: true, count_in: 0 }, "the next start goes by them");
        // And back.
        for check in [&TRUNCATED, &RESET] {
            let r = hit_rect(&app, &Action::Settings(SAct::Check(check.label)));
            click(&mut app, &mut c, r);
        }
        assert_eq!(Starts::from_prefs(), Starts { truncated: true, reset: false, count_in: 0 });
    }

    // spec: ui/settings-choices.md "Miscellaneous: seven more checks, Default template and Startup project"
    /// Settings > General ▸ Miscellaneous: "Alternate meter scale" (off by default) and "Create automation clips at
    /// the playback position" (on) switch with a click. The alternate scale is live on the mixer: the "C" column's
    /// read-out at the height where the default scale has −45 dB reads −30 dB.
    #[test]
    fn misc_settings_meter_scale_and_automation_clips() {
        use crate::mixer::MHit;
        use crate::prefs::{self, ALTERNATE_METER_SCALE as ALT, AUTOMATION_AT_PLAYBACK_POSITION as AT_POS};
        use crate::views::settings::{SAct, GENERAL};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        assert_eq!((prefs::check(&ALT), prefs::check(&AT_POS)), (false, true), "the defaults");
        let mut app = App::new(None);
        app.open_settings(GENERAL);
        let above = ["Display", "Language", "Input", "Undo history", "Hints and warnings", "System", "Updates"];
        app.state.settings.folded.extend(above);
        frame(&mut app, &mut c, None);
        for (check, said) in [(&ALT, "Alternate meter scale: on"), (&AT_POS, "Create automation clips at the playback position: off")] {
            assert!(!app.hits.iter().any(|h| h.action == Action::Placeholder(check.label)), "{} works", check.label);
            let r = hit_rect(&app, &Action::Settings(SAct::Check(check.label)));
            click(&mut app, &mut c, r);
            assert_eq!(app.state.message.as_deref(), Some(said));
        }
        assert_eq!((prefs::check(&ALT), prefs::check(&AT_POS)), (true, false));
        // The mixer's level scale reads levels through the alternate scale while it is on.
        app.state.settings.open = false;
        app.state.z = vec![Win::Mixer];
        let floor = app.state.mixer.meter_cfg.v[crate::meters::k::FLOOR_DB];
        let read = |app: &mut App, c: &mut Canvas| -> f32 {
            frame(app, c, None);
            let r = hit_rect(app, &Action::Mix(MHit::Scale));
            app.mouse_move(r.cx(), r.bottom() - crate::meters::frac(-45.0, floor, false) * r.h);
            frame(app, c, None);
            let h = app.hits.iter().rev().find(|h| h.action == Action::Mix(MHit::Scale)).map(|h| h.hint.to_string()).unwrap();
            h.split_whitespace().nth(1).and_then(|v| v.trim_end_matches("dB").parse().ok()).unwrap_or_else(|| panic!("{h}"))
        };
        assert!((read(&mut app, &mut c) + 30.0).abs() < 0.05, "alternate");
        prefs::set_check(&ALT, false);
        assert!((read(&mut app, &mut c) + 45.0).abs() < 0.05, "default");
    }

    // spec: ui/settings-choices.md "Miscellaneous: seven more checks, Default template and Startup project"
    /// The mixer's "C" column: the default scale's +3, 0, 3 … 45; the alternate scale's 0, 3 … 30 with no +3, 0 at
    /// the same height, each mark where the default scale has the level the measured table gives (30 where 45 is).
    #[test]
    fn alternate_meter_scale_marks() {
        use crate::views::mixer::scale_marks;
        let floor = crate::meters::Settings::default().v[crate::meters::k::FLOOR_DB];
        let off = scale_marks(false, floor);
        assert_eq!((off.len(), off[0], off[1], off[16]), (17, ("3", 181.0), ("0", 241.0), ("45", 880.0)));
        let on = scale_marks(true, floor);
        let labels: Vec<&str> = on.iter().map(|m| m.0).collect();
        assert_eq!(labels, ["0", "3", "6", "9", "12", "15", "18", "21", "24", "27", "30"]);
        assert_eq!((on[0].1, on[10].1), (241.0, 880.0), "0 stays, 30 where 45 is");
        // "3" where the default scale has −6.6 dB: a fifth of the way from its "6" (360) to its "9" (419).
        assert!((on[1].1 - 371.8).abs() < 1e-3, "{}", on[1].1);
    }

    // spec: ui/settings-choices.md "Miscellaneous: seven more checks, Default template and Startup project"
    /// "Create automation clips at the playback position", the measured run on an empty playlist: on at bar 5 the clip
    /// covers bars 5–6 (the song is empty: the pattern's length, 1 bar); off, bars 1–6 on the next track; on at bar
    /// 2, bars 2–6 on the track after that. A muted clip does not count for the song's end; a time selection sets
    /// the clip either way.
    #[test]
    fn automation_clips_start_at_the_playback_position() {
        use crate::playlist as pl;
        use crate::prefs::{self, AUTOMATION_AT_PLAYBACK_POSITION as AT_POS};
        let mut app = App::new(None);
        let s = &mut app.state;
        pl::arr_mut(s).clips.clear();
        let current = s.pattern;
        if let Some(p) = s.project.patterns.iter_mut().find(|p| p.number == current) {
            (p.notes, p.length) = (Vec::new(), None);
        }
        let bar = s.project.bar_ticks();
        let last = |s: &State| pl::clips(s).last().map(|c| (c.position / bar, c.end() / bar, c.track)).unwrap();
        s.playlist.pos = 4 * bar;
        pl::create_automation(s, 0, "Auto", 0.5);
        assert_eq!(last(s), (4, 5, 0), "on, at bar 5");
        prefs::set_check(&AT_POS, false);
        pl::create_automation(s, 0, "Auto", 0.5);
        assert_eq!(last(s), (0, 5, 1), "off: from bar 1 to the song's end");
        prefs::set_check(&AT_POS, true);
        s.playlist.pos = bar;
        pl::create_automation(s, 0, "Auto", 0.5);
        assert_eq!(last(s), (1, 5, 2), "on, at bar 2");
        // A muted clip ending at bar 11 does not count; from bar 7 the song has ended: one pattern length.
        pl::arr_mut(s).clips.push(os_model::Clip { muted: true, ..os_model::Clip::new(0, 10 * bar, 6, os_model::ClipSource::Pattern(1)) });
        s.playlist.pos = 6 * bar;
        pl::create_automation(s, 0, "Auto", 0.5);
        assert_eq!(last(s), (6, 7, 7));
        // A time selection sets the clip, whatever the check.
        for on in [true, false] {
            prefs::set_check(&AT_POS, on);
            s.playlist.range = Some((bar, 3 * bar));
            pl::create_automation(s, 0, "Auto", 0.5);
            assert_eq!(last(s).0..last(s).1, 1..3, "time selection, check {on}");
        }
    }

    // spec: ui/settings-choices.md "Miscellaneous: naming, detached plugins and startup"
    /// Settings > General ▸ Miscellaneous: "Name channels after their sample or plugin" (on by default) and "Name effect slots after their plugin" (off)
    /// switch with a click.
    #[test]
    fn misc_settings_auto_name_checks() {
        use crate::prefs::{self, AUTO_NAME_CHANNELS as CHANNELS, AUTO_NAME_EFFECT_SLOTS as SLOTS};
        use crate::views::settings::{SAct, GENERAL};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        assert_eq!((prefs::check(&CHANNELS), prefs::check(&SLOTS)), (true, false), "the defaults");
        let mut app = App::new(None);
        app.open_settings(GENERAL);
        let above = ["Display", "Language", "Input", "Undo history", "Hints and warnings", "System", "Updates"];
        app.state.settings.folded.extend(above);
        frame(&mut app, &mut c, None);
        for (check, said) in [(&CHANNELS, "Name channels after their sample or plugin: off"), (&SLOTS, "Name effect slots after their plugin: on")] {
            assert!(!app.hits.iter().any(|h| h.action == Action::Placeholder(check.label)), "{} works", check.label);
            let r = hit_rect(&app, &Action::Settings(SAct::Check(check.label)));
            click(&mut app, &mut c, r);
            assert_eq!(app.state.message.as_deref(), Some(said));
        }
        assert_eq!((prefs::check(&CHANNELS), prefs::check(&SLOTS)), (false, true));
    }

    // spec: ui/settings-choices.md "Miscellaneous: naming, detached plugins and startup"
    /// "Name channels after their sample or plugin": a sample dropped on a sampler row, chosen in Load sample... or sent from the browser to
    /// the selected channel renames that channel after the file (without its extension) while the check is on; off,
    /// the sample loads and the channel keeps its name. A channel made new for a dropped sample takes its name either
    /// way.
    #[test]
    fn auto_name_channels_on_every_sample_path() {
        use crate::browser::BCmd;
        use crate::prefs::{self, AUTO_NAME_CHANNELS};
        let dir = std::env::temp_dir().join(format!("os_auto_name_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let wav = |n: &str| {
            let p = dir.join(format!("{n}.wav"));
            os_engine::wav::write_f32_stereo(&mut std::fs::File::create(&p).unwrap(), 44100, &[0.0; 8]).unwrap();
            p.to_string_lossy().into_owned()
        };
        let (kick6, kick12) = (wav("Grv Kick 06"), wav("Grv Kick 12"));
        let mut app = App::new(None);
        let r = (0..app.state.project.channels.len()).find(|&r| app.state.project.channels[r].kind.is_sampler()).unwrap();
        let y = views::rack::name_rect(&app.state, r).unwrap().cy();
        // The browser lists the folder's two samples; row `r` is the selected channel.
        app.state.browser_folders.push(dir.clone());
        app.state.browser_open.insert(crate::views::browser::disk_key(&dir));
        app.state.selected_channel = r;
        let now = |app: &App| {
            let ch = &app.state.project.channels[r];
            (ch.name.clone().unwrap_or_default(), ch.sample_path.clone().unwrap_or_default())
        };
        let had = |name: &str, path: &str| (name.to_string(), path.to_string());
        app.state.project.channels[r].name = Some("808 Snare".into());
        app.sample_to_rack(kick6.clone(), y);
        assert_eq!(now(&app), had("Grv Kick 06", &kick6), "on: a drop on the row");
        crate::rack::sample_chosen(&mut app, r, kick12.clone().into());
        assert_eq!(now(&app), had("Grv Kick 12", &kick12), "on: Load sample...");
        app.browser_cmd(BCmd::Send(1));
        assert_eq!(now(&app), had("Grv Kick 06", &kick6), "on: the browser's send");
        prefs::set_check(&AUTO_NAME_CHANNELS, false);
        app.state.project.channels[r].name = Some("808 HiHat".into());
        app.browser_cmd(BCmd::Send(1));
        assert_eq!(now(&app), had("808 HiHat", &kick12), "off: the browser's send");
        app.sample_to_rack(kick6.clone(), y);
        assert_eq!(now(&app), had("808 HiHat", &kick6), "off: a drop on the row");
        crate::rack::sample_chosen(&mut app, r, kick12.clone().into());
        assert_eq!(now(&app), had("808 HiHat", &kick12), "off: Load sample...");
        // Below the rows the drop makes a new channel, named after the file.
        let n = app.state.project.channels.len();
        app.sample_to_rack(kick12.clone(), views::rack::rect(&app.state).bottom() + 40.0);
        assert_eq!(app.state.project.channels.len(), n + 1);
        assert_eq!(app.state.project.channels[app.state.selected_channel].name.as_deref(), Some("Grv Kick 12"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A middle click on a browser sample selects its row and loads the sample into the selected channel, as "Send to
    /// selected channel" does (spec: ui/browser.md §7); undo puts the old sample back.
    #[test]
    fn browser_middle_click_sends_a_sample() {
        use crate::views::browser::{rows, Item};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let dir = std::env::temp_dir().join(format!("os_middle_send_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("Grv Clap 03.wav");
        os_engine::wav::write_f32_stereo(&mut std::fs::File::create(&p).unwrap(), 44100, &[0.0; 8]).unwrap();
        let path = p.to_string_lossy().into_owned();
        let mut app = App::new(None);
        app.state.browser_folders.push(dir.clone());
        app.state.browser_open.insert(crate::views::browser::disk_key(&dir));
        let r = (0..app.state.project.channels.len()).find(|&r| app.state.project.channels[r].kind.is_sampler()).unwrap();
        app.state.selected_channel = r;
        let before = app.state.project.channels[r].sample_path.clone();
        frame(&mut app, &mut c, None);
        let i = rows(&app.state).iter().position(|row| row.item == Item::Sample(path.clone())).expect("sample row");
        let at = hit_rect(&app, &Action::BrowserRow(i));
        app.mouse_move(at.cx(), at.cy());
        app.mouse_down_middle();
        assert_eq!(app.state.project.channels[r].sample_path.as_deref(), Some(path.as_str()));
        assert_eq!(app.state.browser_sel.as_ref(), Some(&rows(&app.state)[i].key), "the row is selected");
        app.key(&winit::keyboard::Key::Character("z".into()), true, false);
        assert_eq!(app.state.project.channels[r].sample_path, before, "one undo step");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Settings > Project ▸ Advanced ▸ Panning law (spec: ui/settings-choices.md "Project", audio/gain-laws.md
    /// "Panning law: Triangular"): FL's two choices with the project's marked; a pick changes event 23 at once (the
    /// engine gets the project again, the project counts as changed), saved and opened again, also patched in an
    /// opened file.
    #[test]
    fn project_settings_panning_law() {
        use crate::playlist::Mark;
        use crate::views::settings::{SAct, PANNING_LAW, PROJECT};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        (app.state.file, app.state.path) = (None, None);
        app.mark_saved();
        app.open_settings(PROJECT);
        frame(&mut app, &mut c, None);
        let r = hit_rect(&app, &Action::Settings(SAct::Group("Advanced")));
        click(&mut app, &mut c, r);
        assert!(!app.hits.iter().any(|h| h.action == Action::Placeholder(PANNING_LAW)), "not greyed");
        let r = app.hits.iter().rev().find(|h| matches!(&h.action, Action::Settings(SAct::Combo(l, ..)) if *l == PANNING_LAW)).map(|h| h.rect).expect("Panning law works");
        click(&mut app, &mut c, r);
        let items: Vec<_> = app.state.playlist.popup.as_ref().expect("a menu").items.iter().map(|it| (it.label.to_string(), it.mark.clone())).collect();
        assert_eq!(items, vec![("Circular".to_string(), Mark::Radio(true)), ("Triangular".to_string(), Mark::Radio(false))], "a project without event 23");
        app.state.playlist.popup = None;
        frame(&mut app, &mut c, None);
        app.state.piano.dirty = false;
        project_pick(&mut app, &mut c, PANNING_LAW, "Triangular", 2);
        assert_eq!(app.state.project.panning_law, Some(2));
        assert_eq!(app.state.message.as_deref(), Some("Panning law: Triangular"));
        assert!(app.state.piano.dirty && app.unsaved(), "playback and the title follow");
        let shown = |app: &App| project_rows(app).into_iter().find_map(|r| match r {
            views::settings::Row::Combo(v, _, l) if l == PANNING_LAW => Some(v),
            _ => None,
        });
        assert_eq!(shown(&app).as_deref(), Some("Triangular"));
        // Saved and opened again; back to Circular in the opened project (23 = 0, patched), saved over it.
        let path = std::env::temp_dir().join(format!("lemon-project-panning-{}.flp", std::process::id()));
        app.save_to(path.clone());
        let mut opened = App::new(None);
        opened.open(path.clone());
        assert_eq!(opened.state.project.panning_law, Some(2));
        crate::playlist::run(&mut opened, crate::playlist::Cmd::PanningLaw(0));
        assert!(opened.unsaved());
        opened.save_to(path.clone());
        let back = os_flp::project::load(&os_flp::FlpFile::parse(&std::fs::read(&path).unwrap()).unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(back.panning_law, Some(0));
    }

    #[test]
    fn tempo_drag_locks_the_cursor() {
        let Ok(mut a) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        frame(&mut app, &mut a, None);
        let r = hit_rect(&app, &Action::Tempo);
        let t0 = app.state.tempo;
        // On the whole number.
        let x = app.state.tempo_digits.get()[0] - 4.0;
        app.mouse_move(x, r.cy());
        frame(&mut app, &mut a, None);
        app.mouse_down();
        assert!(app.cursor_locked(), "tempo drags lock the cursor");
        app.mouse_move(x, r.cy() - 10.0);
        assert_eq!(app.state.tempo, t0 + 10.0);
        app.mouse_up();
        assert!(!app.cursor_locked());
    }

    /// The tempo box's drag, Settings > Project's time settings: undo steps named as the control plane's commands, one
    /// per drag or run of wheel notches, none for a press that changes nothing.
    #[test]
    fn tempo_and_time_settings_are_undo_steps() {
        use crate::control::command::{TEMPO, TIME_SIGNATURE};
        let Ok(mut a) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        frame(&mut app, &mut a, None);
        let r = hit_rect(&app, &Action::Tempo);
        let (t0, steps) = (app.state.tempo, app.state.history.at());
        let x = app.state.tempo_digits.get()[0] - 4.0;
        app.mouse_move(x, r.cy());
        app.mouse_down();
        app.mouse_up();
        assert_eq!(app.state.history.at(), steps, "a click on the tempo");
        app.mouse_down();
        app.mouse_move(x, r.cy() - 10.0);
        app.mouse_move(x, r.cy() - 20.0);
        app.mouse_up();
        assert_eq!((app.state.tempo, app.state.history.at()), (t0 + 20.0, steps + 1));
        assert_eq!(crate::history::undo(&mut app.state).as_deref(), Some(TEMPO));
        assert_eq!(app.state.tempo, t0);
        assert_eq!(crate::history::redo(&mut app.state).as_deref(), Some(TEMPO));
        assert_eq!(app.state.tempo, t0 + 20.0);

        let bar = app.state.project.bar();
        app.set_bar(3, 4);
        assert_eq!(crate::history::undo(&mut app.state).as_deref(), Some(TIME_SIGNATURE));
        assert_eq!(app.state.project.bar(), bar);
        crate::history::redo(&mut app.state);
        assert_eq!(app.state.project.bar(), (3, 4));
        let at = app.state.history.at();
        app.set_bar(3, 4);
        assert_eq!(app.state.history.at(), at, "the bar it is already");
        for _ in 0..3 {
            app.settings_number(views::settings::BEATS_PER_BAR, 1);
        }
        assert_eq!((app.state.project.bar(), app.state.history.at()), ((6, 4), at + 1), "wheel notches in a row: one step");
        crate::history::undo(&mut app.state);
        assert_eq!(app.state.project.bar(), (3, 4));
        let signature = app.state.project.bar_is_signature();
        app.set_time_mode(!signature);
        assert_eq!(crate::history::undo(&mut app.state).as_deref(), Some(TIME_SIGNATURE));
        assert_eq!((app.state.project.bar_is_signature(), app.state.project.bar()), (signature, (3, 4)));
        // The Project page's switches.
        let before = app.state.project.clone();
        app.set_panning_law(2);
        app.toggle_play_truncated();
        app.toggle_auto_crossfades();
        let p = &app.state.project;
        assert!(p.panning_law == Some(2) && p.play_truncated != before.play_truncated && p.auto_crossfades != before.auto_crossfades);
        for name in ["Automatic crossfades", "Play truncated notes in clips", "Panning law"] {
            assert_eq!(crate::history::undo(&mut app.state).as_deref(), Some(name));
        }
        assert!(app.state.project == before);
    }

    #[test]
    fn tempo_drag_steps_by_the_digit_it_starts_on() {
        let Ok(mut a) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        frame(&mut app, &mut a, None);
        let y = hit_rect(&app, &Action::Tempo).cy();
        let d = app.state.tempo_digits.get();
        assert!(d[0] < d[1] && d[1] < d[2], "{d:?}");
        let whole = d[0] - 4.0;
        for (x, step) in [(whole, 1.0), ((d[0] + d[1]) / 2.0, 0.1), ((d[1] + d[2]) / 2.0, 0.01), (d[2] + 2.0, 0.001)] {
            app.state.tempo = 130.0;
            frame(&mut app, &mut a, None);
            app.mouse_move(x, y);
            app.mouse_down();
            app.mouse_move(x, y - 3.0);
            app.mouse_up();
            let want = 130.0 + 3.0 * step;
            assert!((app.state.tempo - want).abs() < 1e-9, "from x {x}: {} vs {want}", app.state.tempo);
        }
    }

    #[test]
    fn a_held_press_slides_the_mixer_selection_across_strips() {
        use crate::mixer::MHit;
        let Ok(mut a) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.wins.iter_mut().for_each(|w| w.maxed = false);
        app.state.z = vec![Win::Mixer];
        frame(&mut app, &mut a, None);
        let (r2, r3) = (hit_rect(&app, &Action::Mix(MHit::Strip(2))), hit_rect(&app, &Action::Mix(MHit::Strip(3))));
        app.mouse_move(r2.cx(), r2.cy());
        app.mouse_down();
        assert_eq!(app.state.selected_mixer, 2);
        // Into the next strip's column, above the strips: still follows the column.
        app.mouse_move(r3.cx(), r3.y - 30.0);
        assert_eq!(app.state.selected_mixer, 3);
        app.mouse_move(r2.cx(), r2.cy());
        assert_eq!(app.state.selected_mixer, 2);
        app.mouse_up();
        app.mouse_move(r3.cx(), r3.cy());
        assert_eq!(app.state.selected_mixer, 2, "released: hovering selects nothing");
        assert_eq!(app.state.mixer.sel, Vec::<u16>::new());
    }

    #[test]
    fn mixer_eq_fader_and_knobs_edit_the_track() {
        use crate::knobs::KnobId;
        let Ok(mut a) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.wins.iter_mut().for_each(|w| w.maxed = false);
        app.state.z = vec![Win::Mixer];
        app.state.selected_mixer = 1;
        frame(&mut app, &mut a, None);
        let i = app.state.project.mixer.iter().position(|m| m.index == 1).unwrap();
        // Band 2's fader: up 40 px is a quarter of the range, +9 dB.
        let r = hit_rect(&app, &Action::Knob(KnobId::Eq(1, 1, 0)));
        app.mouse_move(r.cx(), r.cy());
        app.mouse_down();
        app.mouse_move(r.cx(), r.cy() - 40.0);
        app.mouse_up();
        assert_eq!(app.state.project.mixer[i].eq[1][0], 900);
        frame(&mut app, &mut a, None);
        let hint = app.hits.iter().find(|h| h.action == Action::Knob(KnobId::Eq(1, 1, 0))).map(|h| h.hint.to_string());
        assert_eq!(hint.as_deref(), Some("EQ band 2 (peaking) level: 9.0dB"));
        let (target, name, v) = app.automation_target(&Action::Knob(KnobId::Eq(1, 1, 0))).unwrap();
        assert_eq!((target, name.as_str()), (os_model::target::mixer(1, os_model::target::EQ_GAIN + 1), "Bus 1 - EQ band 2 (peaking) level"));
        assert!((v - 0.75).abs() < 1e-6);
        // Band 3's frequency knob, when the panel is tall enough to show it.
        if app.hits.iter().any(|h| h.action == Action::Knob(KnobId::Eq(1, 2, 1))) {
            let r = hit_rect(&app, &Action::Knob(KnobId::Eq(1, 2, 1)));
            app.mouse_move(r.cx(), r.cy());
            app.mouse_down();
            app.mouse_move(r.cx(), r.cy() - 20.0);
            app.mouse_up();
            assert!(app.state.project.mixer[i].eq[2][1] > os_model::EQ_DEFAULT[2][1]);
        }
    }

    #[test]
    fn mixer_eq_graph_drags_the_nearest_band() {
        use crate::knobs::KnobId;
        use crate::mixer::MHit;
        let Ok(mut a) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut b = Canvas::new(1920, 1080, 1.0).unwrap();
        let mut app = App::new(None);
        app.state.wins.iter_mut().for_each(|w| w.maxed = false);
        app.state.z = vec![Win::Mixer];
        app.state.selected_mixer = 1;
        frame(&mut app, &mut a, None);
        let _ = app.take_damage();
        let i = app.state.project.mixer.iter().position(|m| m.index == 1).unwrap();
        let g = hit_rect(&app, &Action::Mix(MHit::EqGraph));
        // Each change repaints the graph with the faders and knobs, and that matches a full repaint.
        let check = |app: &mut App, a: &mut Canvas, b: &mut Canvas, what: &str| {
            let d = app.take_damage();
            let Some(Some(rects)) = &d else { panic!("{what}: repaints regions, not the frame") };
            assert!(rects.iter().any(|r| r.contains(g.cx(), g.cy())), "{what}: the graph is repainted");
            frame_damage(app, a, d);
            frame(app, b, None);
            assert_same(a, b, what);
        };
        // A knob: the graph follows.
        let k = hit_rect(&app, &Action::Knob(KnobId::Eq(1, 1, 0)));
        app.mouse_move(k.cx(), k.cy());
        frame(&mut app, &mut a, None);
        let _ = app.take_damage();
        app.mouse_down();
        frame(&mut app, &mut a, None);
        let _ = app.take_damage();
        app.mouse_move(k.cx(), k.cy() - 30.0);
        check(&mut app, &mut a, &mut b, "fader drag");
        app.mouse_up();
        app.state.project.mixer[i].eq = os_model::EQ_DEFAULT;
        frame(&mut app, &mut a, None);
        let _ = app.take_damage();
        // The graph's hint reads the frequency under the cursor and the band a press would take: here band 2 (1500 Hz).
        let x = g.x + 1.0 + (g.w - 2.0) * 0.55;
        app.mouse_move(x, g.cy());
        assert!(app.hint.ends_with("Hz  Tweak band 2"), "{}", app.hint);
        // Pressing there and dragging up and left: band 2 follows the cursor, the others stay.
        app.mouse_down();
        frame(&mut app, &mut a, None);
        let _ = app.take_damage();
        app.mouse_move(x - 10.0, g.cy() - (g.h / 2.0 - 4.0) / 2.0);
        check(&mut app, &mut a, &mut b, "graph drag");
        app.mouse_up();
        let eq = app.state.project.mixer[i].eq;
        assert_eq!(eq[1][0], 900, "half way up is +9 dB");
        let want = (crate::mixer::eq_graph_pos(g, x - 10.0) * 65536.0).round() as i32;
        assert_eq!(eq[1][1], want);
        assert_eq!((eq[0], eq[2]), (os_model::EQ_DEFAULT[0], os_model::EQ_DEFAULT[2]));
        // At the left edge the low shelf is the nearest.
        app.mouse_move(g.x + 3.0, g.cy());
        assert!(app.hint.ends_with("Tweak band 1"), "{}", app.hint);
    }

    #[test]
    fn knobs_drag_and_wheel() {
        use crate::knobs::KnobId;
        let Ok(mut a) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut b = Canvas::new(1920, 1080, 1.0).unwrap();
        let mut app = App::new(None);
        app.state.wins.iter_mut().for_each(|w| w.maxed = false);
        app.state.z = vec![Win::Mixer, Win::ChannelRack];
        frame(&mut app, &mut a, None);
        let _ = app.take_damage();
        for id in [KnobId::ChanVol(0), KnobId::MixPan(1), KnobId::ChanPan(0)] {
            let r = hit_rect(&app, &Action::Knob(id));
            let v0 = app.state.knob(id);
            app.mouse_move(r.cx(), r.cy());
            frame(&mut app, &mut a, None);
            app.mouse_down();
            assert!(app.cursor_locked(), "knob drags lock the cursor");
            app.mouse_move(r.cx(), r.cy() - 100.0);
            let want = (v0 + 100.0 * id.drag_rate()).min(1.0);
            assert!((app.state.knob(id) - want).abs() < 1e-4, "{id:?}: {} vs {want}", app.state.knob(id));
            frame(&mut app, &mut a, None);
            app.mouse_up();
            // Damage repaint of the drag equals a full repaint.
            let d = app.take_damage();
            frame_damage(&mut app, &mut a, d);
            frame(&mut app, &mut b, None);
            assert_same(&a, &b, "after knob drag");
            let before = app.state.knob(id);
            app.mouse_move(r.cx(), r.cy());
            frame(&mut app, &mut a, None);
            app.mouse_down_middle();
            assert_eq!(app.state.knob(id), id.default_value(), "middle click resets {id:?}");
            app.state.set_knob(id, before);
            frame(&mut app, &mut a, None);
            app.wheel(0.0, 1.0, false, false);
            assert!((app.state.knob(id) - (before + id.notch()).min(1.0)).abs() < 1e-4, "wheel {id:?}");
        }
        assert!(app.state.project.mixer.iter().find(|m| m.index == 1).unwrap().pan.unwrap() > 0.0, "mixer pan written to the model");
    }

    /// The wheel over a mixer fader moves it instead of scrolling the mixer: three notches up from unity read
    /// `103%  0.8dB  1.09`, and a run of notches is one undo step (spec: ui/mixer.md §4).
    #[test]
    fn wheel_moves_a_mixer_fader() {
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.z = vec![Win::Mixer];
        frame(&mut app, &mut c, None);
        let vol = |app: &App| crate::mixer::track(&app.state, 1).unwrap().volume.unwrap();
        assert_eq!(vol(&app), 0.8);
        let r = hit_rect(&app, &Action::Fader(1));
        app.mouse_move(r.cx(), r.cy());
        let scroll = app.state.mixer_scroll;
        for _ in 0..3 {
            app.wheel(0.0, 1.0, false, false);
        }
        assert_eq!(app.hint, "Volume: 103%  0.8dB  1.09");
        assert_eq!(app.state.mixer_scroll, scroll, "the mixer does not scroll");
        assert!(crate::history::undo(&mut app.state).is_some());
        assert_eq!(vol(&app), 0.8, "one undo step for the three notches");
        for _ in 0..200 {
            app.wheel(0.0, -1.0, false, false);
        }
        assert_eq!(vol(&app), 0.0, "stops at the bottom");
    }

    /// Every knob and fader the main window draws (toolbar, mixer, its equaliser and sends, channel rack) goes back to
    /// its default with a middle click and with Reset, the first entry of its right-click menu (spec: ui/knobs.md,
    /// ui/mixer.md §6.3, ui/channel-rack.md §19).
    #[test]
    fn every_knob_and_fader_resets() {
        use crate::knobs::{Control, KnobId};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.wins.iter_mut().for_each(|w| w.maxed = false);
        app.state.z = vec![Win::ChannelRack, Win::Mixer];
        frame(&mut app, &mut c, None);
        let value = |app: &App, c: Control| match c {
            Control::Knob(id) => app.state.knob(id),
            Control::Fader(t) => crate::mixer::track(&app.state, t).and_then(|m| m.volume).unwrap_or(0.8) as f32,
            _ => unreachable!(),
        };
        let default = |c: Control| match c {
            Control::Knob(id) => id.default_value(),
            _ => 0.8,
        };
        // Away from the default, by a step the stored value can hold.
        let disturb = |app: &mut App, c: Control| {
            let v = if default(c) < 0.5 { 0.875 } else { 0.125 };
            match c {
                Control::Knob(id) => app.state.set_knob(id, v),
                Control::Fader(t) => crate::mixer::track_mut(&mut app.state, t).unwrap().volume = Some(f64::from(v)),
                _ => unreachable!(),
            }
            assert!((value(app, c) - default(c)).abs() > 0.1, "{c:?} moved");
        };
        // The controls the mouse can reach (not under another window), once each.
        let mut seen = Vec::new();
        let controls: Vec<(Control, f32, f32)> = app
            .hits
            .iter()
            .filter_map(|h| Some((Control::of(&h.action)?, h.rect.cx(), h.rect.cy())))
            .filter(|&(c, x, y)| app.top_hit(x, y).and_then(|h| Control::of(&h.action)) == Some(c))
            .filter(|&(c, ..)| !seen.contains(&c) && {
                seen.push(c);
                true
            })
            .collect();
        for want in [
            Control::Knob(KnobId::MasterVol),
            Control::Knob(KnobId::MasterPitch),
            Control::Knob(KnobId::Swing),
            Control::Knob(KnobId::ChanVol(0)),
            Control::Knob(KnobId::ChanPan(0)),
            Control::Knob(KnobId::MixPan(1)),
            Control::Fader(1),
        ] {
            assert!(seen.contains(&want), "{want:?} is drawn");
        }
        assert!(seen.iter().any(|c| matches!(c, Control::Knob(KnobId::Eq(..)))), "the equaliser's knobs are drawn");
        for (ctl, x, y) in controls {
            app.mouse_move(x, y);
            disturb(&mut app, ctl);
            app.mouse_down_middle();
            app.mouse_up();
            assert!((value(&app, ctl) - default(ctl)).abs() < 1e-4, "middle click resets {ctl:?}");
            disturb(&mut app, ctl);
            app.mouse_down_right();
            app.mouse_up();
            let menu = app.state.playlist.popup.clone().unwrap_or_else(|| panic!("{ctl:?} has a menu"));
            assert_eq!(menu.items[0].label, "Reset", "{ctl:?}");
            app.act(Action::PlPopup(0));
            assert!((value(&app, ctl) - default(ctl)).abs() < 1e-4, "Reset resets {ctl:?}");
            assert!(app.state.playlist.popup.is_none());
            frame(&mut app, &mut c, None);
        }
    }

    /// Holding the mouse on a toolbar window button lights it orange; releasing restores it.
    #[test]
    fn pressed_button_look() {
        let Ok(mut a) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        frame(&mut app, &mut a, None);
        // A window button whose press changes nothing in the toolbar (the plugin picker would cover the screen).
        let act = Action::ToggleWin(Win::Browser);
        let r = hit_rect(&app, &act);
        let px = |c: &Canvas| {
            let mut out = vec![0u32; 1920 * 1080];
            c.copy_to_xrgb(&mut out);
            out[(r.cy() as usize + 9) * 1920 + r.cx() as usize]
        };
        let idle = px(&a);
        app.mouse_move(r.cx(), r.cy());
        frame(&mut app, &mut a, None);
        app.mouse_down();
        frame(&mut app, &mut a, None);
        let held = px(&a);
        let (red, blue) = ((held >> 16) & 0xff, held & 0xff);
        assert!(red > 0xe0 && blue < 0xa8, "orange while held: {held:06x} (idle {idle:06x})");
        app.mouse_up();
        app.mouse_move(900.0, 900.0);
        frame(&mut app, &mut a, None);
        assert_eq!(px(&a), idle, "back to idle after release and moving away");
    }

    /// The plugin window menu names its docking entry for what it does: Detach while docked, Attach while detached.
    #[test]
    fn plugin_menu_says_detach_or_attach() {
        for (detached, label) in [(false, "Detach"), (true, "Attach")] {
            let m = crate::playlist::plugin_window_menu(0, detached, 0.0, 0.0);
            let it = m.items.iter().find(|i| i.cmd == crate::playlist::Cmd::PluginDetach(0, !detached)).expect("the entry");
            assert_eq!(it.label, label);
            assert_eq!(it.mark, crate::playlist::Mark::None, "no check mark");
        }
    }

    #[test]
    fn buttons_act_on_release() {
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        frame(&mut app, &mut c, None);
        // A button: nothing on the press, its work on the release over it.
        let r = hit_rect(&app, &Action::ToggleWin(Win::Browser));
        let shown = app.state.show_browser;
        app.mouse_move(r.cx(), r.cy());
        app.mouse_down();
        assert_eq!(app.state.show_browser, shown, "the press does nothing yet");
        app.mouse_up();
        assert_ne!(app.state.show_browser, shown, "the release over the button toggles it");
        frame(&mut app, &mut c, None);
        // Moving off before the release lets it go.
        let r = hit_rect(&app, &Action::ToggleWin(Win::Browser));
        let shown = app.state.show_browser;
        app.mouse_move(r.cx(), r.cy());
        app.mouse_down();
        app.mouse_move(900.0, 600.0);
        app.mouse_up();
        assert_eq!(app.state.show_browser, shown, "a release off the button does nothing");
        frame(&mut app, &mut c, None);
        // A menu opens on the press; its entry acts on the release.
        let m = hit_rect(&app, &Action::Menu(0));
        app.mouse_move(m.cx(), m.cy());
        app.mouse_down();
        assert_eq!(app.state.menu_open, Some(0), "the menu opens on the press");
        app.mouse_up();
        frame(&mut app, &mut c, None);
        let item = app.hits.iter().find(|h| matches!(h.action, Action::MenuItem(0, _))).map(|h| (h.action.clone(), h.rect)).unwrap();
        app.mouse_move(item.1.cx(), item.1.cy());
        app.mouse_down();
        assert_eq!(app.state.menu_open, Some(0), "the entry's press keeps the menu");
        app.mouse_up();
        assert_eq!(app.state.menu_open, None, "the entry's release runs it and closes the menu");
    }

    /// A damage-region repaint must give the pixels of a full repaint (anti-aliased edges cut by
    /// the damage rect may round by a level or two in tiny-skia's masked pipeline).
    #[test]
    fn partial_repaint_matches_full() {
        let Ok(mut a) = Canvas::new(1920, 1080, 1.0) else { return }; // no system font: skip
        let mut b = Canvas::new(1920, 1080, 1.0).unwrap();
        let mut app = App::new(None);
        app.state.z = vec![Win::Mixer, Win::ChannelRack];
        frame(&mut app, &mut a, None);

        // Move the rack and fade a fader, repainting only the damaged regions.
        let old = views::rack::bounds(&app.state);
        app.state.rack_pos = (700.0, 400.0);
        let new = views::rack::bounds(&app.state);
        if let Some(t) = app.state.project.mixer.iter_mut().find(|t| t.index == 3) {
            t.volume = Some(0.35);
        }
        let col = app.hits.iter().find(|h| h.action == Action::Fader(3)).map(|h| h.rect).unwrap();
        frame(&mut app, &mut a, Some(old.union(&new).union(&col.outset(10.0))));

        frame(&mut app, &mut b, None);
        let diffs: Vec<usize> = a.pixmap.data().iter().zip(b.pixmap.data()).enumerate().filter(|(_, (x, y))| x.abs_diff(**y) > 3).map(|(i, _)| i / 4).collect();
        let w = 1920;
        let bbox = diffs.iter().fold((usize::MAX, usize::MAX, 0, 0), |m, &i| (m.0.min(i % w), m.1.min(i / w), m.2.max(i % w), m.3.max(i / w)));
        if !diffs.is_empty() {
            dump(&a, &b);
        }
        assert!(diffs.is_empty(), "{} differing pixels in x {}..{} y {}..{}", diffs.len(), bbox.0, bbox.2, bbox.1, bbox.3);
    }

    /// While playing, only what playback moves is repainted (clock, playheads, the rack's playing step, sounding
    /// lights, meters); each such frame must equal a full repaint of the same state.
    #[test]
    fn playback_repaint_matches_full() {
        let Ok(mut a) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut b = Canvas::new(1920, 1080, 1.0).unwrap();
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.project.patterns[0].notes = (0..16).step_by(2).map(|k| Note::step(0, k * 24)).collect();
        app.state.project.patterns[0].notes.extend((0..16).step_by(4).map(|k| Note::step(1, k * 24)));
        crate::playlist::place(&mut app.state, 0, 0);
        crate::playlist::place(&mut app.state, 1, 384);
        let play = |tick: f64, song: bool, sounding: Vec<(u16, u16)>| Shown { playing: true, paused: false, tick, song, sounding, tempo: None };
        // Floating windows side by side, so every playback region is on screen.
        for (w, r) in [(Win::Playlist, Rect::new(220.0, 90.0, 1000.0, 480.0)), (Win::PianoRoll, Rect::new(220.0, 90.0, 1000.0, 480.0)), (Win::Mixer, Rect::new(230.0, 590.0, 1500.0, 480.0))] {
            let ws = &mut app.state.wins[w.slot().unwrap()];
            ws.maxed = false;
            ws.rect = r;
        }
        for (z, song) in [(vec![Win::Playlist, Win::Mixer, Win::ChannelRack], true), (vec![Win::Mixer, Win::PianoRoll, Win::ChannelRack], false)] {
            app.state.z = z;
            app.state.rack_pos = (1240.0, 100.0);
            frame(&mut app, &mut a, None);
            let grid = if song { app.pl_grid } else { app.piano_grid }.expect("grid shown");
            // Starting playback repaints everything; then each tick only damages.
            app.show_playback(play(0.0, song, Vec::new()));
            let d = app.take_damage();
            assert_eq!(d, Some(None), "starting repaints the whole frame");
            frame_damage(&mut app, &mut a, d);
            for (i, tick) in [5.0, 23.0, 25.0, 50.0, 120.0, 383.0, 390.0, 700.0, 10.0].into_iter().enumerate() {
                let sounding = if i % 3 == 0 { vec![(0, 60)] } else if i % 3 == 1 { vec![(0, 60), (1, 60)] } else { Vec::new() };
                app.show_playback(play(tick, song, sounding));
                // Meters move too.
                app.state.mixer.meters.resize(18, Default::default());
                app.state.mixer.meters[1].db = [-6.0 - i as f32, -12.0];
                app.state.mixer.meters[2].db = [-3.0 * i as f32, -1.0];
                app.damage_live(Live::Meter);
                let d = app.take_damage();
                let Some(Some(rects)) = &d else { panic!("a tick repaints regions, not the frame") };
                assert!(rects.iter().any(|r| !r.intersect(&grid).is_empty()), "the playhead is repainted");
                frame_damage(&mut app, &mut a, d);
                frame(&mut app, &mut b, None);
                assert_same(&a, &b, &format!("song {song}, tick {tick}"));
            }
            app.show_playback(Shown::default());
            let d = app.take_damage();
            frame_damage(&mut app, &mut a, d);
        }
    }

    /// Wheel scrolls and zooms, scroll-bar drags and clip drags repaint only their window, and match a full repaint.
    #[test]
    fn scroll_repaint_matches_full() {
        for design in Design::ALL {
            scroll_repaint_in(design);
        }
    }

    fn scroll_repaint_in(design: Design) {
        let Ok(mut a) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut b = Canvas::new(1920, 1080, 1.0).unwrap();
        let mut app = App::new(None);
        // Rounded (Lemon) windows: their corners are not part of what scrolling keeps (spec: ui/lemon-design.md §3).
        app.theme = design_theme(design, false);
        app.state.project = test_project();
        app.state.project.patterns[0].notes = (0..16).step_by(2).map(|k| Note::step(0, k * 24)).collect();
        for (track, tick) in [(0, 0), (1, 384), (2, 768), (0, 1536)] {
            crate::playlist::place(&mut app.state, track, tick);
        }
        for (w, r) in [(Win::Playlist, Rect::new(220.0, 90.0, 1000.0, 480.0)), (Win::PianoRoll, Rect::new(300.0, 300.0, 900.0, 450.0))] {
            let ws = &mut app.state.wins[w.slot().unwrap()];
            ws.maxed = false;
            ws.rect = r;
        }
        app.state.z = vec![Win::PianoRoll, Win::Playlist, Win::ChannelRack];
        app.state.rack_pos = (1240.0, 100.0);
        frame(&mut app, &mut a, None);
        let _ = app.take_damage();
        let pl = app.state.win_area(Win::Playlist);
        let check = |app: &mut App, a: &mut Canvas, b: &mut Canvas, what: &str| {
            let d = app.take_damage();
            let Some(Some(rects)) = &d else { panic!("{what}: repaints regions, not the frame") };
            // The hint panel follows the hovered thing; everything else stays inside the playlist.
            let inside = rects.iter().filter(|r| r.intersect(&HINT_RECT) != **r);
            let bound = inside.fold(Rect::default(), |u, r| if u.is_empty() { *r } else { u.union(r) });
            assert!(bound.intersect(&pl) == bound, "{what}: only the playlist is repainted ({bound:?})");
            frame_damage(app, a, d);
            frame(app, b, None);
            assert_same(a, b, what);
        };
        let g = app.pl_grid.unwrap();
        // Hovering an automation clip shows its points: onto it and off it again repaints just that clip.
        crate::playlist::create_automation(&mut app.state, 0, "Auto", 0.5);
        app.state.playlist.sel.clear();
        frame(&mut app, &mut a, None);
        let _ = app.take_damage();
        let pg = crate::playlist::PGeom::new(&app.state, g);
        let auto = *crate::playlist::clips(&app.state).last().unwrap();
        let ar = pg.clip_rect(&auto);
        app.mouse_move(ar.x + 20.0, ar.bottom() - 10.0);
        assert!(app.state.playlist.hover_clip.is_some(), "the automation clip is hovered");
        check(&mut app, &mut a, &mut b, "automation clip hovered");
        app.mouse_move(ar.x + 20.0, ar.bottom() + 60.0);
        check(&mut app, &mut a, &mut b, "automation clip left");
        // A plain click on a tool button: the press only shows it pressed, the release picks the tool.
        let tool = hit_rect(&app, &Action::PlTool(crate::playlist::PlTool::Paint));
        app.mouse_move(tool.cx(), tool.cy());
        app.mouse_down();
        assert_eq!(app.state.playlist.tool, crate::playlist::PlTool::Draw, "the press doesn't pick the tool");
        let d = app.take_damage();
        frame_damage(&mut app, &mut a, d);
        app.mouse_up();
        let d = app.take_damage();
        frame_damage(&mut app, &mut a, d);
        frame(&mut app, &mut b, None);
        assert_same(&a, &b, "tool button released");
        assert_eq!(app.state.playlist.tool, crate::playlist::PlTool::Paint);
        app.state.playlist.tool = crate::playlist::PlTool::Draw;
        app.invalidate();
        frame_damage(&mut app, &mut a, Some(None));
        let _ = app.take_damage();
        // Wheel over the grid: scroll down, then right, each easing over 10 ms frames (General ▸ Display ▸ Smoothing at
        // its default); Ctrl zooms at once.
        app.mouse_move(g.x + 300.0, g.y + 100.0);
        frame(&mut app, &mut a, None);
        let _ = app.take_damage();
        let eases = |app: &mut App, a: &mut Canvas, b: &mut Canvas, what: &str| {
            assert!(app.smooth.running(), "{what}: eases");
            for i in 0..100 {
                app.ease_step(10.0);
                if app.has_damage() {
                    check(app, a, b, &format!("{what}, frame {i}"));
                }
                if !app.smooth.running() {
                    return;
                }
            }
            panic!("{what}: the ease ends");
        };
        app.wheel(0.0, -2.0, false, false);
        eases(&mut app, &mut a, &mut b, "wheel scroll");
        app.wheel(-3.0, 0.0, false, false);
        eases(&mut app, &mut a, &mut b, "wheel sideways");
        app.wheel(0.0, 1.0, false, true);
        assert!(!app.smooth.running(), "a zoom doesn't ease");
        check(&mut app, &mut a, &mut b, "ctrl+wheel zoom");
        // Drag the horizontal scroll bar.
        let bar = app.bar(Scroll::PlaylistX).unwrap();
        app.mouse_move(bar.track.x + 3.0, bar.track.cy());
        app.mouse_down();
        let _ = app.take_damage();
        frame(&mut app, &mut a, None);
        app.mouse_move(bar.track.x + 60.0, bar.track.cy());
        check(&mut app, &mut a, &mut b, "scroll bar drag");
        app.mouse_up();
        frame(&mut app, &mut a, None);
        let _ = app.take_damage();
        // Drag the first clip on screen right and down.
        app.state.playlist_scroll = (0.0, 0.0);
        frame(&mut app, &mut a, None);
        let _ = app.take_damage();
        let pg = crate::playlist::PGeom::new(&app.state, app.pl_grid.unwrap());
        let c0 = pg.clip_rect(&crate::playlist::clips(&app.state)[0]);
        app.mouse_move(c0.x + 10.0, c0.y + 25.0);
        app.mouse_down();
        frame(&mut app, &mut a, None);
        let _ = app.take_damage();
        for (dx, dy) in [(40.0, 0.0), (90.0, 48.0), (150.0, 96.0)] {
            app.mouse_move(c0.x + 10.0 + dx, c0.y + 25.0 + dy);
            if let Damage::Rects(rs) = &app.damage {
                let area: f32 = rs.iter().map(|r| r.w * r.h).sum();
                assert!(area < pl.w * pl.h / 4.0, "a clip drag repaints the clip, not the window ({area})");
            }
            check(&mut app, &mut a, &mut b, "clip drag");
        }
        let moved = crate::playlist::clips(&app.state)[0];
        assert!(moved.position > 0 && moved.track == 2, "the clip was dragged: {} on track {}", moved.position, moved.track);
        app.mouse_up();
        frame(&mut app, &mut a, None);
        let _ = app.take_damage();

        // An audio clip (spec: ui/playlist.md §9a): hovering it shows its handles, a handle under the mouse lights,
        // dragging the fade-in triangle repaints that clip and the hint bar only.
        let wav = std::env::temp_dir().join(format!("os-repaint-clip-{}.wav", std::process::id()));
        os_engine::wav::write_f32_stereo(&mut std::fs::File::create(&wav).unwrap(), 48000, &[0.4; 96000]).unwrap();
        crate::playlist::add_audio_clip(&mut app.state, &wav.to_string_lossy(), 4, 384).expect("audio clip");
        app.state.playlist.sel.clear();
        app.state.playlist_bar_w = 240.0;
        frame(&mut app, &mut a, None);
        let _ = app.take_damage();
        let pg = crate::playlist::PGeom::new(&app.state, app.pl_grid.unwrap());
        let ci = crate::playlist::clips(&app.state).len() - 1;
        let body = pg.body(&crate::playlist::clips(&app.state)[ci]);
        app.mouse_move(body.cx() + 30.0, body.cy());
        assert_eq!(app.state.playlist.hover_clip, Some(ci), "the audio clip is hovered");
        check(&mut app, &mut a, &mut b, "audio clip hovered");
        app.mouse_move(body.x + 2.0, body.y + 2.0);
        assert_eq!(app.state.playlist.hover_handle, Some(crate::playlist::Handle::FadeIn));
        check(&mut app, &mut a, &mut b, "fade handle hovered");
        app.mouse_down();
        frame(&mut app, &mut a, None);
        let _ = app.take_damage();
        for dx in [30.0, 70.0] {
            app.mouse_move(body.x + 2.0 + dx, body.y + 2.0);
            if let Damage::Rects(rs) = &app.damage {
                let area: f32 = rs.iter().map(|r| r.w * r.h).sum();
                assert!(area < pl.w * pl.h / 4.0, "a fade drag repaints the clip, not the window ({area})");
            }
            check(&mut app, &mut a, &mut b, "fade drag");
        }
        assert!(crate::playlist::clips(&app.state)[ci].fade_in().is_some(), "the fade was set");
        app.mouse_up();
        frame(&mut app, &mut a, None);
        let _ = app.take_damage();
        // Stretch mode: resizing the audio clip repaints the playlist (every clip of its channel follows).
        crate::playlist::run(&mut app, crate::playlist::Cmd::StretchMode);
        app.invalidate();
        frame_damage(&mut app, &mut a, Some(None));
        let _ = app.take_damage();
        let r = pg.clip_rect(&crate::playlist::clips(&app.state)[ci]);
        app.mouse_move(r.right() - 1.0, r.bottom() - 6.0);
        app.mouse_down();
        frame(&mut app, &mut a, None);
        let _ = app.take_damage();
        app.mouse_move(r.right() + 60.0, r.bottom() - 6.0);
        check(&mut app, &mut a, &mut b, "stretch resize");
        app.mouse_up();
        let _ = std::fs::remove_file(&wav);
    }

    /// Scrolling the playlist moves the pixels drawn for it and paints only what scrolled in (`App::playlist_scrolled`).
    /// Every step equals a full repaint, at 100 % and 150 %: with notes, an audio clip (fades, a waveform changing
    /// every column), an automation clip, markers, a time selection, a track group, a zoom off the pixel grid, song
    /// playback, windows over the playlist, drags, a popup (no move: the window repaints) and random steps.
    #[test]
    fn playlist_scroll_moves_pixels() {
        let wav = std::env::temp_dir().join(format!("os-scroll-move-{}.wav", std::process::id()));
        let audio: Vec<f32> = (0..480_000)
            .map(|i| {
                let t = (i / 2) as f32 / 48000.0;
                (t * 2.0).sin() * (t * 900.0 * (1.0 + t)).sin()
            })
            .collect();
        os_engine::wav::write_f32_stereo(&mut std::fs::File::create(&wav).unwrap(), 48000, &audio).unwrap();
        for scale in [1.0, 1.5] {
            for design in Design::ALL {
                scroll_moves_at(&wav, scale, design);
            }
        }
        let _ = std::fs::remove_file(&wav);
    }

    fn scroll_moves_at(wav: &std::path::Path, scale: f32, design: Design) {
        use crate::playlist as pl;
        let (w, h) = ((1920.0 * scale).round() as u32, (1080.0 * scale).round() as u32);
        let Ok(mut a) = Canvas::new(w, h, scale) else { return };
        let mut b = Canvas::new(w, h, scale).unwrap();
        let mut app = App::new(None);
        app.theme = design_theme(design, false);
        app.state.project = test_project();
        app.state.project.patterns[0].notes = (0..32).map(|i| Note { length: 30, key: 60 + (i % 7) as u16, ..Note::step(0, i * 24) }).collect();
        let bar = 4 * u32::from(app.state.project.ppq);
        for t in 0..14u16 {
            for k in 0..8u32 {
                if (u32::from(t) + k) % 3 != 0 {
                    pl::place(&mut app.state, t, (k * 3 + u32::from(t % 2)) * bar);
                }
            }
        }
        let ci = pl::add_audio_clip(&mut app.state, &wav.to_string_lossy(), 4, bar / 2).expect("audio clip");
        let ch = pl::create_automation(&mut app.state, 0, "Auto", 0.5);
        pl::add_point(&mut app.state, ch, 3.0, 0.9);
        pl::add_point(&mut app.state, ch, 7.5, 0.1);
        {
            let arr = pl::arr_mut(&mut app.state);
            arr.clips[ci].set_fade_in(Some((300.0, 0.3)));
            arr.clips[ci].set_fade_out(Some((900.0, -0.4)));
            arr.markers.push(os_model::TimeMarker::new(2 * bar, "Drop"));
            arr.markers.push(os_model::TimeMarker::new(9 * bar + 7, "Break"));
            arr.tracks[2].color = Some(os_model::Rgb::new(0x90, 0x58, 0x87));
            arr.tracks[3].grouped = true;
            arr.tracks[4].grouped = true;
        }
        app.state.playlist.sel = vec![3, 7];
        app.state.playlist.range = Some((bar + 5, 5 * bar));
        // A zoom off the drawing grid, a start between device pixels (on the 1/256 px lattice: whole steps from it stay
        // exact in f32; a step that is not exact repaints, see "off-grid step"), a track height as a user leaves them.
        app.state.playlist_bar_w = 37.37;
        app.state.playlist_scroll = (123.457_031_25, 0.0);
        app.state.playlist.row_h = 46.0;
        let ws = &mut app.state.wins[Win::Playlist.slot().unwrap()];
        ws.maxed = false;
        ws.rect = Rect::new(150.3, 90.0, 1600.0, 900.0);
        app.state.z = vec![Win::Playlist];
        frame(&mut app, &mut a, None);
        let _ = app.take_damage();
        let pl = app.state.win_area(Win::Playlist);
        let g = app.pl_grid.unwrap();
        let what_at = |what: &str| format!("{what} at {scale}");
        // Paint what is pending; `moves`: pixels were moved (else the window repainted); `small`: under a quarter of the
        // playlist painted. Then the result must equal a full repaint.
        let check = |app: &mut App, a: &mut Canvas, b: &mut Canvas, what: &str, moves: Option<bool>, small: bool| -> Vec<Rect> {
            let what = what_at(what);
            let d = app.take_damage();
            let Some(Some(_)) = &d else { panic!("{what}: repaints regions, not the frame") };
            frame_damage(app, a, d);
            let region = a.region();
            let area: f32 = region.iter().map(|r| r.w * r.h).sum();
            let moved = app.take_moved();
            if let Some(m) = moves {
                assert_eq!(!moved.is_empty(), m, "{what}: the drawn pixels moved");
            }
            if small {
                assert!(area < pl.w * pl.h / 4.0, "{what}: painted {area} of {} px²: {region:?}", pl.w * pl.h);
            }
            frame(app, b, None);
            assert_same(a, b, &what);
            region
        };
        let full = |app: &mut App, a: &mut Canvas| {
            app.invalidate();
            let d = app.take_damage();
            frame_damage(app, a, d);
        };
        let over_grid = (g.x + 300.0, g.y + 200.0);
        app.mouse_move(over_grid.0, over_grid.1);
        full(&mut app, &mut a);
        // Wheel steps at once (General ▸ Display ▸ Smoothing 0; eased steps below): down, up, right, left, Shift =
        // sideways, several before one paint.
        crate::prefs::set_slider(&crate::prefs::SMOOTHING, 0);
        app.wheel(0.0, -1.0, false, false);
        check(&mut app, &mut a, &mut b, "wheel down", Some(true), true);
        app.wheel(0.0, -2.0, false, false);
        check(&mut app, &mut a, &mut b, "wheel down two", Some(true), true);
        app.wheel(0.0, 1.0, false, false);
        check(&mut app, &mut a, &mut b, "wheel up", Some(true), true);
        app.wheel(-1.0, 0.0, false, false);
        check(&mut app, &mut a, &mut b, "wheel right", Some(true), true);
        app.wheel(1.0, 0.0, false, false);
        check(&mut app, &mut a, &mut b, "wheel left", Some(true), true);
        app.wheel(0.0, -1.0, true, false);
        check(&mut app, &mut a, &mut b, "shift+wheel", Some(true), true);
        app.wheel(-1.0, 0.0, false, false);
        app.wheel(0.0, -1.0, false, false);
        app.wheel(-1.0, 0.0, false, false);
        check(&mut app, &mut a, &mut b, "three steps", Some(true), true);
        // Eased steps (Smoothing 128, 10 ms frames): every frame of the ease moves the drawn pixels, in whole steps of
        // the drawing grid at 150 %; a notch sideways during the ease too.
        crate::prefs::set_slider(&crate::prefs::SMOOTHING, 128);
        app.wheel(0.0, -2.0, false, false);
        let mut frames = 0;
        while app.smooth.running() {
            if frames == 6 {
                app.wheel(-1.0, 0.0, false, false);
            }
            app.ease_step(10.0);
            frames += 1;
            if app.has_damage() {
                check(&mut app, &mut a, &mut b, &format!("eased frame {frames}"), Some(true), true);
            }
            assert!(frames < 100, "{}: the ease ends", what_at("eased"));
        }
        assert!(frames > 10, "{}: the steps eased over {frames} frames", what_at("eased"));
        crate::prefs::set_slider(&crate::prefs::SMOOTHING, 0);
        // Over the track headers: rows move, the time line stays; the hovered group arrow (lit) moves off the mouse.
        app.state.playlist_scroll.1 = 0.0;
        full(&mut app, &mut a);
        let arrow = hit_rect(&app, &Action::PlGroupArrow(2));
        app.mouse_move(arrow.cx(), arrow.cy());
        full(&mut app, &mut a);
        app.wheel(0.0, -1.0, false, false);
        check(&mut app, &mut a, &mut b, "wheel over the headers", Some(true), true);
        app.mouse_move(over_grid.0, over_grid.1);
        // At the start: a part of a step moves; a step off the drawing grid repaints the window.
        app.state.playlist_scroll.0 = 30.0;
        full(&mut app, &mut a);
        app.wheel(1.0, 0.0, false, false);
        assert_eq!(app.state.playlist_scroll.0, 0.0);
        check(&mut app, &mut a, &mut b, "wheel to the start", Some(true), true);
        app.state.playlist_scroll.0 = 25.3;
        full(&mut app, &mut a);
        app.wheel(1.0, 0.0, false, false);
        check(&mut app, &mut a, &mut b, "off-grid step", Some(false), false);
        app.state.playlist_scroll.0 = 123.457_031_25;
        full(&mut app, &mut a);
        // Song playback: the playhead moves between steps.
        let pg = pl::PGeom::new(&app.state, g);
        let tick = f64::from(pg.tick_at(g.x + 400.0));
        let shown = |tick: f64| Shown { playing: true, paused: false, tick, song: true, sounding: Vec::new(), tempo: None };
        app.show_playback(shown(tick));
        full(&mut app, &mut a);
        // The playhead's damage before a step was taken where it was drawn; its pixels move with the step.
        app.show_playback(shown(tick + 400.0));
        app.wheel(-1.0, 0.0, false, false);
        app.show_playback(shown(tick + 900.0));
        check(&mut app, &mut a, &mut b, "scroll while playing", Some(true), true);
        app.show_playback(shown(tick + 1300.0));
        app.wheel(0.0, -1.0, false, false);
        app.show_playback(shown(tick + 1700.0));
        check(&mut app, &mut a, &mut b, "scroll down while playing", Some(true), true);
        app.show_playback(Shown::default());
        full(&mut app, &mut a);
        // Dragging the overview's thumb, then grabbing the grid with the middle button.
        let bar_x = app.bar(Scroll::PlaylistX).unwrap();
        let pos = app.scroll_get(Scroll::PlaylistX);
        let thumb_x = bar_x.track.x + pos / bar_x.max * (bar_x.track.w - bar_x.thumb) + bar_x.thumb / 2.0;
        app.mouse_move(thumb_x, bar_x.track.cy());
        app.mouse_down();
        full(&mut app, &mut a);
        // The shell paints only when `has_damage` says so: every move of these drags must ask for a frame.
        for dx in [37.0, 81.0, 60.0] {
            app.mouse_move(thumb_x + dx, bar_x.track.cy());
            assert!(app.has_damage(), "scroll bar drag at {scale}: the move asks for a frame");
            check(&mut app, &mut a, &mut b, "scroll bar drag", Some(true), true);
        }
        app.mouse_up();
        full(&mut app, &mut a);
        app.mouse_move(over_grid.0, over_grid.1);
        app.mouse_down_middle();
        full(&mut app, &mut a);
        for (dx, dy) in [(23.0, 31.0), (-50.0, 7.0), (-90.0, -60.0)] {
            app.mouse_move(over_grid.0 + dx, over_grid.1 + dy);
            assert!(app.has_damage(), "middle-drag grab at {scale}: the move asks for a frame");
            check(&mut app, &mut a, &mut b, "middle-drag grab", Some(true), true);
        }
        app.mouse_up();
        full(&mut app, &mut a);
        // An open popup takes the wheel (spec: ui/menus.md "The wheel in menus"): the playlist stays put.
        app.mouse_move(over_grid.0, over_grid.1);
        app.state.playlist.popup = Some(pl::options_menu(&app.state, g.x + 900.0, g.y + 100.0));
        full(&mut app, &mut a);
        let view = app.state.playlist_scroll;
        app.wheel(0.0, -1.0, false, false);
        assert_eq!(app.state.playlist_scroll, view, "popup open at {scale}: the playlist did not scroll");
        assert!(app.warp.take().is_some());
        check(&mut app, &mut a, &mut b, "popup open", Some(false), false);
        app.state.playlist.popup = None;
        full(&mut app, &mut a);

        // Windows over the playlist: a piano roll and the channel rack.
        let ws = &mut app.state.wins[Win::PianoRoll.slot().unwrap()];
        ws.maxed = false;
        ws.rect = Rect::new(g.x - 60.0, g.bottom() - 220.0, 700.0, 400.0);
        app.state.z = vec![Win::Playlist, Win::PianoRoll, Win::ChannelRack];
        app.state.rack_pos = (g.right() - 380.0, g.y + 120.0);
        full(&mut app, &mut a);
        assert_eq!(app.top_hit(over_grid.0, over_grid.1).map(|h| h.action.clone()), Some(Action::PlaylistArea), "the mouse is over the grid");
        // They did not change: the insides of their frames keep their pixels (their edges and what they uncover paint).
        let insides = [views::rack::rect(&app.state).inset(3.0), app.state.win_rect(Win::PianoRoll).inset(3.0)];
        for (what, dx, dy) in [("covered: down", 0.0, -1.0), ("covered: right", -2.0, 0.0), ("covered: up", 0.0, 2.0), ("covered: left", 1.0, 0.0)] {
            app.wheel(dx, dy, false, false);
            let region = check(&mut app, &mut a, &mut b, what, Some(true), false);
            for inside in insides {
                let painted: f32 = region.iter().map(|r| r.intersect(&inside)).map(|r| r.w * r.h).sum();
                assert!(painted < inside.w * inside.h / 10.0, "{what} at {scale}: painted {painted} px² inside {inside:?}: {region:?}");
            }
        }
        // (The press raises the playlist; put the windows back over it.)
        let covered = vec![Win::Playlist, Win::PianoRoll, Win::ChannelRack];
        app.mouse_down_middle();
        app.state.z = covered.clone();
        full(&mut app, &mut a);
        app.mouse_move(over_grid.0 + 41.0, over_grid.1 - 17.0);
        check(&mut app, &mut a, &mut b, "covered: grab", Some(true), false);
        app.mouse_up();
        app.state.z = covered.clone();
        full(&mut app, &mut a);
        // A window over the playlist moving after a step, before the paint: its old pixels moved with the step.
        assert!(app.playlist_covers().len() > 1, "windows over the playlist");
        app.wheel(0.0, -1.0, false, false);
        let old = app.win_bounds(Win::ChannelRack);
        app.state.rack_pos.1 += 90.0;
        app.damage_rect(old);
        app.damage_rect(app.win_bounds(Win::ChannelRack));
        check(&mut app, &mut a, &mut b, "covered: a window moved after a step", Some(true), false);

        // Random steps: one to three wheel steps a frame, the mouse wandering over the grid (hover changes), playback.
        let mut seed = 0x2545_f491_u32;
        let mut rnd = |n: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed % n
        };
        for i in 0..40 {
            if rnd(4) == 0 {
                let (x, y) = (g.x + 20.0 + rnd(700) as f32 * 0.75, g.y + 10.0 + rnd(500) as f32 * 0.5);
                if app.top_hit(x, y).is_some_and(|h| h.action == Action::PlaylistArea) {
                    app.mouse_move(x, y);
                }
            }
            if rnd(5) == 0 {
                app.show_playback(shown(tick + f64::from(i) * 13.0));
                if rnd(2) == 0 {
                    full(&mut app, &mut a);
                }
            }
            for _ in 0..=rnd(3) {
                let step = [-2.0, -1.0, 1.0, 2.0][rnd(4) as usize];
                if rnd(2) == 0 {
                    app.wheel(step, 0.0, false, false);
                } else {
                    app.wheel(0.0, step, false, false);
                }
            }
            if app.has_damage() {
                check(&mut app, &mut a, &mut b, &format!("random step {i}"), None, false);
            }
        }
    }

    /// Saves a failed comparison's two frames in the temp folder, named after the process: suites running side by side
    /// would otherwise overwrite each other's.
    fn dump(a: &Canvas, b: &Canvas) {
        let id = std::process::id();
        let _ = a.pixmap.save_png(std::env::temp_dir().join(format!("os_partial_{id}.png")));
        let _ = b.pixmap.save_png(std::env::temp_dir().join(format!("os_full_{id}.png")));
    }

    fn assert_same(a: &Canvas, b: &Canvas, what: &str) {
        let width = a.pixmap.width() as usize;
        let diffs = a.pixmap.data().iter().zip(b.pixmap.data()).enumerate().filter(|(_, (x, y))| x.abs_diff(**y) > 3).map(|(i, _)| i / 4);
        let (n, bbox) = diffs.fold((0, (usize::MAX, usize::MAX, 0, 0)), |(n, m), i| (n + 1, (m.0.min(i % width), m.1.min(i / width), m.2.max(i % width), m.3.max(i / width))));
        if n > 0 {
            dump(a, b);
        }
        assert_eq!(n, 0, "{what}: differing pixels in x {}..{} y {}..{}", bbox.0, bbox.2, bbox.1, bbox.3);
    }

    /// The ends of the Playlist's and Piano roll's horizontal thumbs zoom (spec: ui/scroll-bars.md §4): the dragged end
    /// follows the mouse, the other end keeps its time, and the ends show the resize cursor.
    #[test]
    fn bar_ends_zoom() {
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.z = vec![Win::Playlist];
        frame(&mut app, &mut c, None);
        let t = Scroll::PlaylistX;
        let ends = |app: &App, t: Scroll| {
            let b = app.bar(t).unwrap();
            let a = b.track.x + b.thumb_at(app.scroll_get(t));
            (a, a + b.thumb)
        };
        // Right end: dragged right it zooms out; the left end stays at the start.
        let (a0, b0) = ends(&app, t);
        let (cy, z0) = (app.bar(t).unwrap().track.cy(), app.state.playlist_bar_w);
        app.mouse_move((a0 + b0) / 2.0, cy);
        assert_eq!(app.cursor(), Cursor::Default, "the middle of the thumb scrolls");
        app.mouse_move(b0 - 1.0, cy);
        assert_eq!(app.cursor(), Cursor::EwResize);
        assert_eq!(app.hint, BAR_EDGE_HINT);
        app.mouse_down();
        app.mouse_move(b0 + 59.0, cy);
        assert_eq!(app.cursor(), Cursor::EwResize, "kept while dragging");
        app.mouse_up();
        frame(&mut app, &mut c, None);
        let (a1, b1) = ends(&app, t);
        assert!(app.state.playlist_bar_w < z0, "zoomed out: {} -> {}", z0, app.state.playlist_bar_w);
        assert_eq!(app.scroll_get(t), 0.0);
        assert!((a1 - a0).abs() < 0.5 && (b1 - (b0 + 60.0)).abs() < 1.0, "{a0}..{b0} -> {a1}..{b1}");
        // Left end, scrolled to the middle: dragged right it zooms in; the right end keeps its time.
        let b = app.bar(t).unwrap();
        app.scroll_set(t, b.max / 2.0);
        frame(&mut app, &mut c, None);
        let (z1, (a2, b2)) = (app.state.playlist_bar_w, ends(&app, t));
        let stop = (app.scroll_get(t) + b.view) / z1;
        app.mouse_move(a2 + 1.0, cy);
        app.mouse_down();
        app.mouse_move(a2 + 41.0, cy);
        app.mouse_up();
        frame(&mut app, &mut c, None);
        let z2 = app.state.playlist_bar_w;
        let (a3, b3) = ends(&app, t);
        assert!(z2 > z1, "zoomed in: {z1} -> {z2}");
        assert!(((app.scroll_get(t) + b.view) / z2 - stop).abs() < 0.01, "the right end keeps its time");
        assert!((a3 - (a2 + 40.0)).abs() < 1.0 && (b3 - b2).abs() < 1.0, "{a2}..{b2} -> {a3}..{b3}");
        // The Piano roll's bar: while an end is dragged it keeps its length, so the end follows the mouse (with the
        // pattern inside the view the thumb is two thirds of the bar again after the release).
        app.state.z = vec![Win::PianoRoll];
        frame(&mut app, &mut c, None);
        let t = Scroll::PianoX;
        let (a4, b4) = ends(&app, t);
        let cy = app.bar(t).unwrap().track.cy();
        let z4 = app.state.piano.px_per_beat;
        app.mouse_move(b4 - 1.0, cy);
        assert_eq!(app.cursor(), Cursor::EwResize);
        app.mouse_down();
        app.mouse_move(b4 - 41.0, cy);
        frame(&mut app, &mut c, None);
        let (a5, b5) = ends(&app, t);
        assert!((a5 - a4).abs() < 0.5 && (b5 - (b4 - 40.0)).abs() < 1.0, "{a4}..{b4} -> {a5}..{b5}");
        app.mouse_up();
        assert_eq!(app.state.piano.bar_span, None);
        frame(&mut app, &mut c, None);
        assert!(app.state.piano.px_per_beat > z4, "zoomed in: {z4} -> {}", app.state.piano.px_per_beat);
        assert_eq!(app.scroll_get(t), 0.0, "the start stays");
        // Its left end, scrolled in: the end of the view keeps its time.
        let b = app.bar(t).unwrap();
        app.scroll_set(t, b.max / 2.0);
        frame(&mut app, &mut c, None);
        let ppb = |app: &App| app.state.piano.px_per_beat;
        let (z6, (a6, _)) = (ppb(&app), ends(&app, t));
        let stop = (app.scroll_get(t) + b.view) / z6;
        app.mouse_move(a6 + 1.0, cy);
        app.mouse_down();
        app.mouse_move(a6 - 29.0, cy);
        app.mouse_up();
        frame(&mut app, &mut c, None);
        assert!(ppb(&app) < z6, "zoomed out: {z6} -> {}", ppb(&app));
        assert!(((app.scroll_get(t) + b.view) / ppb(&app) - stop).abs() < 0.01, "the right end keeps its time");
    }

    /// Scroll bars: left-drag scrolls one axis, right-drag both (piano roll), middle-drag grabs.
    #[test]
    fn scrollbar_drags() {
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.z = vec![Win::PianoRoll];
        frame(&mut app, &mut c, None);
        let v = app.bar(Scroll::PianoY).unwrap();
        let h = app.bar(Scroll::PianoX).unwrap();
        // Left-drag the vertical thumb down.
        let y0 = app.scroll_get(Scroll::PianoY);
        let thumb_y = v.track.y + y0 / v.max * (v.track.h - v.thumb) + v.thumb / 2.0;
        app.mouse_move(v.track.cx(), thumb_y);
        app.mouse_down();
        app.mouse_move(v.track.cx(), thumb_y + 50.0);
        app.mouse_up();
        assert!(app.scroll_get(Scroll::PianoY) > y0 + 100.0, "scrolled down");
        assert_eq!(app.scroll_get(Scroll::PianoX), 0.0);
        frame(&mut app, &mut c, None);
        // Right-drag the horizontal bar: sideways scrolls, upward zooms in; vertical scroll stays.
        let (x1, y1, z1) = (app.scroll_get(Scroll::PianoX), app.scroll_get(Scroll::PianoY), app.state.piano.px_per_beat);
        app.mouse_move(h.track.x + 5.0, h.track.cy());
        app.mouse_down_right();
        app.mouse_move(h.track.x + 105.0, h.track.cy());
        app.mouse_up();
        assert!(app.scroll_get(Scroll::PianoX) > x1, "sideways scroll");
        assert_eq!(app.state.piano.px_per_beat, z1);
        frame(&mut app, &mut c, None);
        app.mouse_move(h.track.x + 5.0, h.track.cy());
        app.mouse_down_right();
        app.mouse_move(h.track.x + 5.0, h.track.cy() - 60.0);
        app.mouse_up();
        assert!(app.state.piano.px_per_beat > z1 * 1.5, "zoomed in: {}", app.state.piano.px_per_beat);
        assert_eq!(app.scroll_get(Scroll::PianoY), y1);
        frame(&mut app, &mut c, None);
        // Middle-drag the grid: the content follows the mouse.
        let (x2, y2) = (app.scroll_get(Scroll::PianoX), app.scroll_get(Scroll::PianoY));
        let g = app.piano_grid.unwrap();
        app.mouse_move(g.cx(), g.cy());
        app.mouse_down_middle();
        app.mouse_move(g.cx() + 30.0, g.cy() + 36.0);
        app.mouse_up();
        assert!((app.scroll_get(Scroll::PianoX) - (x2 - 30.0)).abs() < 0.5);
        assert!((app.scroll_get(Scroll::PianoY) - (y2 - 36.0)).abs() < 0.5);
        frame(&mut app, &mut c, None);
        // Right-drag the vertical bar (spec: ui/scroll-bars.md): up/down scrolls with the cursor held, sideways
        // does not scroll.
        let (x3, y3, row) = (app.scroll_get(Scroll::PianoX), app.scroll_get(Scroll::PianoY), app.state.piano.row_h);
        let thumb = |app: &App| {
            let v = app.bar(Scroll::PianoY).unwrap();
            (v.track.cx(), v.track.y + app.scroll_get(Scroll::PianoY) / v.max * (v.track.h - v.thumb) + v.thumb / 2.0)
        };
        let (tx, ty) = thumb(&app);
        app.mouse_move(tx, ty);
        app.mouse_down_right();
        assert!(app.cursor_locked());
        app.mouse_move(tx, ty + 20.0);
        app.mouse_up();
        assert!(app.scroll_get(Scroll::PianoY) > y3 + 20.0, "down scrolls");
        assert_eq!((app.scroll_get(Scroll::PianoX), app.state.piano.row_h), (x3, row));
        frame(&mut app, &mut c, None);
        // Sideways resizes the key rows around the view centre: right = taller, left = shorter.
        let g = app.piano_geom().unwrap().grid;
        let centre_key = |app: &App| (g.cy() - g.y + app.state.piano.scroll_y) / app.state.piano.row_h;
        let k0 = centre_key(&app);
        let (tx, ty) = thumb(&app);
        app.mouse_move(tx, ty);
        app.mouse_down_right();
        for i in 1..=6 {
            app.mouse_move(tx + 4.0 * i as f32, ty);
        }
        app.mouse_up();
        assert_eq!(app.state.piano.row_h, (row * 1.15f32.powi(2)).round(), "24 px right = two steps taller");
        assert!((centre_key(&app) - k0).abs() < 0.5, "centre key kept: {} vs {k0}", centre_key(&app));
        frame(&mut app, &mut c, None);
        let (tx, ty) = thumb(&app);
        app.mouse_move(tx, ty);
        app.mouse_down_right();
        app.mouse_move(tx - 400.0, ty);
        app.mouse_up();
        assert_eq!(app.state.piano.row_h, piano::MIN_ROW);
        // Playlist: the same drag sets the track height in 2 px steps between the limits.
        app.state.z = vec![Win::Playlist];
        frame(&mut app, &mut c, None);
        let v = app.bar(Scroll::PlaylistY).unwrap();
        let (tx, ty) = (v.track.cx(), v.track.y + v.thumb / 2.0);
        let h0 = app.state.playlist.row_h;
        app.mouse_move(tx, ty);
        app.mouse_down_right();
        app.mouse_move(tx + 12.0, ty);
        app.mouse_up();
        let h1 = app.state.playlist.row_h;
        assert!(h1 > h0 && h1 % 2.0 == 0.0, "taller in 2 px steps: {h0} -> {h1}");
        for (dx, want) in [(1000.0, crate::playlist::ROW_MAX), (-1000.0, crate::playlist::ROW_MIN)] {
            frame(&mut app, &mut c, None);
            app.mouse_move(tx, ty);
            app.mouse_down_right();
            app.mouse_move(tx + dx, ty);
            app.mouse_up();
            assert_eq!(app.state.playlist.row_h, want);
        }
        // Bars without rows (channel rack, picker) only scroll.
        assert_eq!(app.rows_get(Scroll::RackY), None);
    }

    fn hit_rect(app: &App, a: &Action) -> Rect {
        app.hits.iter().rev().find(|h| &h.action == a).map(|h| h.rect).unwrap_or_else(|| panic!("no hit for {a:?}"))
    }

    fn click(app: &mut App, c: &mut Canvas, r: Rect) {
        app.mouse_move(r.cx(), r.cy());
        app.mouse_down();
        app.mouse_up();
        frame(app, c, None);
    }

    /// Settings window (spec: ui/settings.md): OPTIONS entries open their tab, tabs switch, checks and
    /// group folds toggle, the General page scrolls, Esc closes; every tab paints.
    /// Every scroll bar drags (spec: ui/scroll-bars.md): the settings page's and the Plugins table's too, by the thumb
    /// or by a press on the track, which jumps the thumb there first.
    #[test]
    fn settings_scroll_bars_drag() {
        use crate::views::settings::{GENERAL, PLUGINS};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        let drag = |app: &mut App, c: &mut Canvas, t: Scroll, dy: f32| {
            frame(app, c, None);
            let b = app.bar(t).unwrap_or_else(|| panic!("{t:?} has a bar"));
            let (x, y) = (b.track.cx(), b.track.y + b.thumb / 2.0);
            app.mouse_move(x, y);
            app.mouse_down();
            app.mouse_move(x, y + dy);
            app.mouse_up();
            frame(app, c, None);
        };
        app.open_settings(GENERAL);
        drag(&mut app, &mut c, Scroll::Settings, 60.0);
        let after = app.state.settings.scroll;
        assert!(after > 0.0, "the page scrolled");
        drag(&mut app, &mut c, Scroll::Settings, -1000.0);
        assert_eq!(app.state.settings.scroll, 0.0, "back to the top, no further");
        // A press low on the track jumps there.
        let b = app.bar(Scroll::Settings).unwrap();
        click(&mut app, &mut c, Rect::new(b.track.x, b.track.bottom() - 4.0, b.track.w, 1.0));
        assert!(app.state.settings.scroll > after);

        app.open_settings(PLUGINS);
        let row = |i: usize| crate::plugin_db::TableRow {
            uid: format!("u{i}"),
            name: format!("Plugin {i}"),
            format: "CLAP",
            kind: "Effect",
            vendor: String::new(),
            status: String::new(),
            file: String::new(),
            path: String::new(),
            note: String::new(),
            favorite: false,
            shown: true,
            dim: false,
            toggles: true,
        };
        app.state.plugdb.rows = (0..80).map(row).collect();
        app.state.plugdb.all_rows = app.state.plugdb.rows.clone();
        drag(&mut app, &mut c, Scroll::PlugTable, 40.0);
        let top = app.state.plugdb.top;
        assert!(top > 0, "the table scrolled by whole rows");
        assert!(top + crate::views::settings::TABLE_ROWS <= 80);
        drag(&mut app, &mut c, Scroll::PlugTable, 2000.0);
        assert_eq!(app.state.plugdb.top, 80 - crate::views::settings::TABLE_ROWS, "to the last row, no further");
    }

    /// The system's close shortcut closes the settings window, then the tool window in front, one per press; a
    /// workspace window in front (the mixer) leaves the shortcut to the app, which then closes.
    #[test]
    fn close_shortcut_closes_dialog_windows_first() {
        let mut app = App::new(None);
        app.state.raise(Win::Mixer);
        app.state.raise(Win::Tapper);
        app.open_settings(0);
        app.close_requested();
        assert!(!app.state.settings.open && app.state.z.contains(&Win::Tapper) && !app.want_exit, "only the settings window closed");
        app.close_requested();
        assert!(!app.state.z.contains(&Win::Tapper) && !app.want_exit, "then the tapper");
        assert_eq!(app.state.z.last(), Some(&Win::Mixer));
        app.close_requested();
        assert!(app.want_exit, "then the app");
    }

    #[test]
    fn settings_window() {
        use crate::views::menus::{item_at, OPTIONS_MENU};
        use crate::views::settings::{SAct, FILE, GENERAL, MIDI, NATIVE_HOST_CHECK, PLUGINS, PROJECT, TABS, THEME};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        frame(&mut app, &mut c, None);
        let audio = (0..40).find(|&i| item_at(OPTIONS_MENU, i).is_some_and(|it| it.label == "Audio settings")).unwrap();
        app.menu_item(OPTIONS_MENU, audio);
        assert!(app.state.settings.open);
        assert_eq!(app.state.settings.tab, 1);
        for i in 0..TABS.len() {
            frame(&mut app, &mut c, None);
            let r = hit_rect(&app, &Action::Settings(SAct::Tab(i)));
            click(&mut app, &mut c, r);
            assert_eq!(app.state.settings.tab, i);
        }
        let r = hit_rect(&app, &Action::Settings(SAct::Tab(MIDI)));
        click(&mut app, &mut c, r);
        // The MIDI page works (spec: ui/midi-settings.md §1): its checks toggle; what Lemon Studio has no use for is
        // greyed with its reason.
        let r = hit_rect(&app, &Action::Settings(SAct::Check("Soft takeover")));
        click(&mut app, &mut c, r);
        assert_eq!(app.state.message.as_deref(), Some("Soft takeover: on"));
        assert!(app.hits.iter().any(|h| h.action == Action::Settings(SAct::Body) && h.hint == "Lemon Studio has no MIDI scripts"));
        assert!(app.hits.iter().any(|h| h.action == Action::Settings(SAct::Number("Performance mode MIDI channel"))));
        let r = hit_rect(&app, &Action::Settings(SAct::Tab(PROJECT)));
        click(&mut app, &mut c, r);
        assert!(app.hits.iter().any(|h| matches!(h.action, Action::Settings(SAct::Combo("Ask for data folder", ..)))), "Ask for data folder works");
        assert!(app.hits.iter().any(|h| matches!(h.action, Action::Settings(SAct::Combo("Timebase (PPQ)", ..)))), "the PPQ combo works");
        let adv = Action::Settings(SAct::Group("Advanced"));
        let r = hit_rect(&app, &adv);
        click(&mut app, &mut c, r);
        assert!(app.hits.iter().any(|h| h.action == Action::Placeholder("Fast fade for choke groups")), "Advanced unfolded");
        // The native host toggle shows only with `OPENSTUDIO_NATIVE_SETTINGS` (`SettingsUi::native`); the
        // real-native-plugins toggle only while the native host is on too (off in tests).
        let shown = |app: &App, k| app.hits.iter().any(|h| h.action == Action::Settings(SAct::Check(k)));
        let r = hit_rect(&app, &Action::Settings(SAct::Tab(PLUGINS)));
        for native in [false, true] {
            app.state.settings.native = native;
            click(&mut app, &mut c, r);
            assert_eq!(shown(&app, NATIVE_HOST_CHECK), native, "native host toggle shown: {native}");
            if !os_native::enabled() {
                assert!(!shown(&app, crate::views::settings::NATIVE_FIRST_CHECK), "hidden");
            }
        }
        // The working controls keep their actions.
        for (tab, live) in [
            (FILE, SAct::Button("Manage plugins")),
            (PLUGINS, SAct::Button("Add folder...")),
            (PLUGINS, SAct::Button("Scan for new plugins")),
            (PLUGINS, SAct::Check(crate::views::settings::COMBINE_CHECK)),
            (THEME, SAct::Preset(0)),
            (THEME, SAct::Button("Load theme...")),
            (THEME, SAct::Button("Reset")),
            (THEME, SAct::Button(crate::views::settings::SAVE_PRESET)),
            (THEME, SAct::Button(crate::views::settings::THEME_UNDO)),
            (THEME, SAct::Button(crate::views::settings::THEME_REDO)),
        ] {
            let r = hit_rect(&app, &Action::Settings(SAct::Tab(tab)));
            click(&mut app, &mut c, r);
            if !app.hits.iter().any(|h| h.action == Action::Settings(live.clone())) {
                // Below the fold (the Theme page's tiles fill more than a 1080 px window): scroll to the end.
                app.wheel(0.0, -40.0, false, false);
                frame(&mut app, &mut c, None);
            }
            assert!(app.hits.iter().any(|h| h.action == Action::Settings(live.clone())), "{live:?} works");
        }
        let r = hit_rect(&app, &Action::Settings(SAct::Tab(GENERAL)));
        click(&mut app, &mut c, r);
        app.wheel(0.0, -3.0, false, false);
        assert!(app.state.settings.scroll > 0.0);
        frame(&mut app, &mut c, None);
        app.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape), false, false);
        assert!(!app.state.settings.open);
        app.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::F10), false, false);
        assert!(app.state.settings.open && app.state.settings.tab == MIDI);
    }

    /// A project in the layout used before version 24.2 opens, converted to the current layout, and is saved in it.
    #[test]
    fn older_projects_open_converted() {
        let mut file = os_flp::project::create(&test_project()).expect("create");
        // Channel blocks without their end event: the older layout.
        for c in &mut file.chunks {
            if let os_flp::ChunkBody::Events(v) = &mut c.body {
                v.retain(|e| e.id != 51);
            }
        }
        let path = std::env::temp_dir().join(format!("lemon-legacy-{}.flp", std::process::id()));
        std::fs::write(&path, file.to_bytes().expect("bytes")).expect("write");
        let mut app = App::new(None);
        app.open(path.clone());
        let _ = std::fs::remove_file(&path);
        assert!(app.state.dialogs.is_empty() && !app.modal_open());
        assert_eq!(app.state.project.channels.len(), test_project().channels.len());
        // The file kept for saving is the converted one: every channel block has its end event again.
        let open = app.state.file.as_ref().expect("the project's file");
        let events: Vec<os_flp::Event> = open.events().cloned().collect();
        assert_eq!(events.iter().filter(|e| e.id == 51).count(), test_project().channels.len());
        assert!(!os_flp::project::is_legacy(&events));
    }

    /// Press `k` (a character or a named key) with the modifiers given.
    fn press(app: &mut App, k: winit::keyboard::Key, ctrl: bool, alt: bool) {
        app.key(&k, ctrl, alt);
    }

    fn typed(app: &mut App, s: &str) {
        press(app, winit::keyboard::Key::Character(s.into()), false, false);
    }

    fn named(app: &mut App, k: winit::keyboard::NamedKey) {
        press(app, winit::keyboard::Key::Named(k), false, false);
    }

    fn top_file(app: &App) -> &crate::filedlg::FileDlg {
        match &app.state.dialogs.last().expect("a dialog").body {
            crate::dialog::Body::File(f) => f,
            crate::dialog::Body::Msg(m) => panic!("a message box is on top: {}", m.plain()),
            crate::dialog::Body::Shortcuts(_) => panic!("Manage shortcuts is on top"),
            crate::dialog::Body::TimeSig(_) => panic!("the time signature dialog is on top"),
            crate::dialog::Body::Markers(_) => panic!("Marker settings is on top"),
            crate::dialog::Body::Project(_) => panic!("a project form is on top"),
        }
    }

    /// Unsaved changes are asked about in a message box drawn in the window (spec: ui/dialogs.md §1f, §3): Cancel
    /// stays, No goes on, Yes saves first (a project without a file through the save dialog) and then goes on; the
    /// typing keyboard does not play while it is open; both designs paint it and its buttons take clicks and keys.
    #[test]
    fn unsaved_changes_are_asked_in_the_window() {
        use crate::dialog::{fake, DHit, Next};
        use crate::views::menus::FILE_NEW;
        use winit::keyboard::NamedKey;
        let Ok(mut c) = Canvas::new(1280, 800, 1.0) else { return };
        fake::live(true);
        // Saving a project never saved goes to the file dialog without Lemon Studio's Save as form (`crate::save_form`).
        crate::prefs::set_choice(&crate::prefs::ASK_DATA_FOLDER, crate::save_form::DONT_ASK);
        let dir = std::env::temp_dir().join(format!("os_dialog_unsaved_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for design in [Design::Citrus, Design::Lemon] {
            let mut app = App::new(None);
            app.set_design(design, false);
            app.state.tempo = 99.0;
            assert!(app.unsaved());
            app.menu_item(0, FILE_NEW);
            let (title, text) = app.top_message().expect("asked");
            assert_eq!(title, "Confirm");
            assert_eq!(text, "Save the changes to the new project before starting a new project?");
            assert!(!app.typing_key(KeyCode::KeyQ, true, false, false), "the typing keyboard waits");
            frame(&mut app, &mut c, None);
            // Cancel (a click) stays.
            let r = hit_rect(&app, &Action::Dlg(DHit::Button(2)));
            click(&mut app, &mut c, r);
            assert!(!app.modal_open() && app.unsaved() && app.state.tempo == 99.0, "Cancel keeps the project");
            // Clicks behind a dialog do nothing.
            app.menu_item(0, FILE_NEW);
            frame(&mut app, &mut c, None);
            app.mouse_move(5.0, 5.0);
            app.mouse_down();
            app.mouse_up();
            assert!(app.modal_open());
            // N (the accelerator) = No: the new project replaces the changes.
            typed(&mut app, "n");
            assert!(!app.modal_open() && !app.unsaved() && app.state.tempo == 130.0, "No goes on");
            // Yes on a project without a file: the save dialog, then the window may close.
            app.state.tempo = 98.0;
            app.ask_discard("closing", Next::Close);
            frame(&mut app, &mut c, None);
            named(&mut app, NamedKey::Tab);
            named(&mut app, NamedKey::Tab);
            named(&mut app, NamedKey::Tab);
            named(&mut app, NamedKey::Enter);
            assert_eq!(app.state.dialogs.len(), 1, "Tab went round to Yes");
            let f = top_file(&app);
            assert_eq!((f.mode, f.name.text.as_str(), f.name.all), (crate::filedlg::Mode::Save, "Untitled.lemon", true));
            frame(&mut app, &mut c, None);
            let target = dir.join(format!("Tune {design:?}"));
            typed(&mut app, &target.display().to_string());
            assert!(!app.want_exit);
            named(&mut app, NamedKey::Enter);
            let saved = dir.join(format!("Tune {design:?}.lemon"));
            assert!(saved.exists() && !app.unsaved(), "saved with the type's extension: {:?}", app.state.message);
            assert!(app.want_exit && !app.modal_open(), "then the window closes");
            // Save as onto that file: the replace question starts on No; Yes replaces.
            app.want_exit = false;
            app.state.tempo = 97.0;
            app.save(true);
            frame(&mut app, &mut c, None);
            assert_eq!(top_file(&app).dir, dir, "the dialog starts in the project's folder");
            named(&mut app, NamedKey::Enter);
            assert_eq!(app.state.dialogs.len(), 2);
            assert_eq!(app.top_message().unwrap().1, format!("Tune {design:?}.lemon already exists.\nDo you want to replace it?"));
            frame(&mut app, &mut c, None);
            named(&mut app, NamedKey::Enter);
            assert_eq!(app.state.dialogs.len(), 1, "No goes back to the dialog");
            assert!(app.unsaved());
            named(&mut app, NamedKey::Enter);
            typed(&mut app, "y");
            assert!(!app.modal_open() && !app.unsaved(), "replaced");
        }
        fake::live(false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The file dialog (spec: ui/dialogs.md §4): rows, double clicks, Up / Back, the type combo, the wheel, the caption
    /// drag, typing a name, Esc; in both designs.
    #[test]
    fn file_dialog_browses_and_answers() {
        use crate::dialog::{fake, DHit};
        use crate::filedlg::{Mode, ROW_H};
        use crate::views::menus::FILE_OPEN;
        use winit::keyboard::NamedKey;
        let Ok(mut c) = Canvas::new(1280, 800, 1.0) else { return };
        fake::live(true);
        let dir = std::env::temp_dir().join(format!("os_dialog_browse_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Beats")).unwrap();
        std::fs::write(dir.join("Beats").join("Loop.flp"), b"not a project").unwrap();
        for i in 0..40 {
            std::fs::write(dir.join(format!("Song {i:02}.flp")), b"x").unwrap();
        }
        std::fs::write(dir.join("Zebra.txt"), b"x").unwrap();
        for design in [Design::Citrus, Design::Lemon] {
            let mut app = App::new(None);
            app.set_design(design, false);
            app.state.path = Some(dir.join("Song 00.flp"));
            app.menu_item(0, FILE_OPEN);
            frame(&mut app, &mut c, None);
            let f = top_file(&app);
            assert_eq!((f.mode, &f.dir, f.entries.len()), (Mode::Open, &dir, 41), "FLP files and the folder");
            assert_eq!(f.entries[0].name, "Beats");
            // The wheel scrolls the list; the caption drags the dialog.
            app.mouse_move(640.0, 400.0);
            app.wheel(0.0, -2.0, false, false);
            assert_eq!(top_file(&app).scroll, 6.0 * ROW_H);
            app.wheel(0.0, 10.0, false, false);
            assert_eq!(top_file(&app).scroll, 0.0);
            frame(&mut app, &mut c, None);
            let cap = hit_rect(&app, &Action::Dlg(DHit::Caption));
            let at = app.state.dialogs[0].rect.get();
            app.mouse_move(cap.x + 40.0, cap.cy());
            app.mouse_down();
            app.mouse_move(cap.x + 10.0, cap.cy() + 30.0);
            app.mouse_up();
            assert_eq!(app.state.dialogs[0].at, Some((at.x - 30.0, at.y + 30.0)));
            frame(&mut app, &mut c, None);
            // A double click opens the folder; Alt+Up goes back up with it selected.
            let row = hit_rect(&app, &Action::Dlg(DHit::Row(0)));
            click(&mut app, &mut c, row);
            click(&mut app, &mut c, row);
            assert_eq!(top_file(&app).dir, dir.join("Beats"));
            press(&mut app, winit::keyboard::Key::Named(NamedKey::ArrowUp), false, true);
            assert_eq!(top_file(&app).dir, dir);
            assert_eq!(top_file(&app).sel, Some(0));
            // All files lists the text file too.
            frame(&mut app, &mut c, None);
            let r = hit_rect(&app, &Action::Dlg(DHit::Filter));
            click(&mut app, &mut c, r);
            assert!(top_file(&app).filter_open);
            let r = hit_rect(&app, &Action::Dlg(DHit::FilterItem(1)));
            click(&mut app, &mut c, r);
            assert_eq!(top_file(&app).entries.len(), 42);
            // Typing a name that is not there is refused in the status; Backspace edits.
            typed(&mut app, "Nothing.flp");
            named(&mut app, NamedKey::Enter);
            assert!(app.modal_open());
            assert_eq!(top_file(&app).status_text(), "Nothing.flp was not found");
            frame(&mut app, &mut c, None);
            for _ in 0.."Nothing.flp".len() {
                named(&mut app, NamedKey::Backspace);
            }
            assert_eq!(top_file(&app).name.text, "");
            // Down picks a row; the main button opens the file (not a real project: the hint bar says so).
            named(&mut app, NamedKey::ArrowDown);
            assert_eq!(top_file(&app).name.text, "Song 00.flp");
            let r = hit_rect(&app, &Action::Dlg(DHit::Accept));
            click(&mut app, &mut c, r);
            assert!(!app.modal_open());
            assert!(app.state.message.as_deref().is_some_and(|m| m.starts_with("Could not open")), "{:?}", app.state.message);
            // Esc cancels.
            app.menu_item(0, FILE_OPEN);
            named(&mut app, NamedKey::Escape);
            assert!(!app.modal_open());
        }
        fake::live(false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Look check for the dialogs in both designs: `OS_SHOT_DIR=dir cargo test -p os-app dialog_shots -- --ignored`.
    #[test]
    #[ignore]
    fn dialog_shots() {
        use crate::dialog::{fake, spans, Answer, Kind, Msg, Then};
        let Some(out) = std::env::var_os("OS_SHOT_DIR") else { return };
        let out = std::path::PathBuf::from(out);
        let Ok(mut c) = Canvas::new(1280, 800, 1.0) else { return };
        fake::live(true);
        let dir = std::env::temp_dir().join(format!("os_dialog_shots_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Drums")).unwrap();
        std::fs::create_dir_all(dir.join("Vocals")).unwrap();
        for (n, len) in [("Club mix.flp", 48_213), ("Demo.flp", 9_120), ("Night drive.flp", 131_072), ("Sketch 3.flp", 2_048)] {
            std::fs::write(dir.join(n), vec![0u8; len]).unwrap();
        }
        for design in [Design::Citrus, Design::Lemon] {
            let name = format!("{design:?}").to_lowercase();
            let mut app = App::new(None);
            app.set_design(design, false);
            app.state.path = Some(dir.join("Demo.flp"));
            for (kind, title, text, buttons, tag) in [
                (Kind::Question, "Confirm", spans(&["Save the changes to ", "Demo.flp", " before closing?"]), &[Answer::Yes, Answer::No, Answer::Cancel][..], "question"),
                (Kind::Warning, "Confirm save as", spans(&["", "Demo.flp", " already exists.\nDo you want to replace it?"]), &[Answer::Yes, Answer::No][..], "warning"),
                (Kind::Error, "Error", spans(&["The file doesn't exist or has been moved."]), &[Answer::Ok][..], "error"),
                (Kind::Info, "Project not supported", spans(&["Lemon Studio cannot open ", "Old.flp", ".\n\nIt was saved by version 12.3, in an older file layout that Lemon Studio could not convert."]), &[Answer::Ok][..], "info"),
            ] {
                let mut m = Msg::new(kind, title, text, buttons, Then::Nothing);
                // As in the app, the replace question starts on No.
                m.focus = usize::from(tag == "warning");
                app.show_msg(m);
                frame(&mut app, &mut c, None);
                app.mouse_move(-1.0, -1.0);
                frame(&mut app, &mut c, None);
                c.pixmap.save_png(out.join(format!("dialog_{name}_{tag}.png"))).unwrap();
                app.state.dialogs.clear();
                app.modal_changed();
            }
            app.save(true);
            frame(&mut app, &mut c, None);
            let row = hit_rect(&app, &Action::Dlg(crate::dialog::DHit::Row(3)));
            app.mouse_move(row.cx(), row.cy());
            frame(&mut app, &mut c, None);
            c.pixmap.save_png(out.join(format!("dialog_{name}_save.png"))).unwrap();
            app.state.dialogs.clear();
            app.file_dialog(crate::filedlg::FileDlg::new(crate::filedlg::Mode::Folder, "Add a plugin folder", "plugins", Then::PluginFolder).start(Some(&dir)));
            frame(&mut app, &mut c, None);
            c.pixmap.save_png(out.join(format!("dialog_{name}_folder.png"))).unwrap();
            app.state.dialogs.clear();
            app.modal_changed();
        }
        fake::live(false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The wheel in an open menu (spec: ui/menus.md "The wheel in menus"): a notch lights the next row that can be
    /// picked (past separators, headings and greyed-out rows, on into the next column, round at the ends) and puts the
    /// cursor in its middle; the menu bar's dropdowns step the same way.
    #[test]
    fn wheel_steps_through_menu_rows() {
        use crate::playlist::{Cmd, PItem, Popup};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        let items = vec![
            PItem::new("A", Cmd::Nothing),
            PItem::sep(),
            PItem::new("Off", Cmd::Nothing).off(),
            PItem::head("Head"),
            PItem::new("B", Cmd::Nothing),
            PItem::col(),
            PItem::new("C", Cmd::Nothing),
        ];
        app.state.playlist.popup = Some(Popup { x: 300.0, y: 200.0, items, sub: None, scroll: [0; 2] });
        frame(&mut app, &mut c, None);
        let (a, b, cc) = (hit_rect(&app, &Action::PlPopup(0)), hit_rect(&app, &Action::PlPopup(4)), hit_rect(&app, &Action::PlPopup(6)));
        let lit = |app: &App| app.top_hit(app.mouse.0, app.mouse.1).map(|h| h.action.clone());
        // From off the menu: down lights the first row, three quarters across; the shell is asked to move the cursor.
        app.mouse_move(10.0, 10.0);
        app.wheel(0.0, -1.0, false, false);
        assert_eq!(app.warp.take(), Some((a.x + a.w * 0.75, a.cy())));
        assert_eq!(lit(&app), Some(Action::PlPopup(0)));
        // Down skips the separator, the greyed-out row and the heading; x stays over the column.
        app.mouse_move(a.x + 5.0, a.cy());
        app.wheel(0.0, -1.0, false, false);
        assert_eq!(app.mouse, (a.x + 5.0, b.cy()));
        // On into the next column, then round to the first row; up goes back round to the last.
        app.wheel(0.0, -1.0, false, false);
        assert_eq!((lit(&app), app.mouse), (Some(Action::PlPopup(6)), (cc.x + cc.w * 0.75, cc.cy())));
        app.wheel(0.0, -1.0, false, false);
        assert_eq!(lit(&app), Some(Action::PlPopup(0)));
        app.wheel(0.0, 1.0, false, false);
        assert_eq!(lit(&app), Some(Action::PlPopup(6)));
        // From off the menu, up lights the last row; part notches add up.
        app.mouse_move(10.0, 10.0);
        app.wheel(0.0, 0.5, false, false);
        assert_eq!(app.mouse, (10.0, 10.0));
        app.wheel(0.0, 0.5, false, false);
        assert_eq!(lit(&app), Some(Action::PlPopup(6)));
        // Nothing was picked and nothing behind the menu scrolled.
        assert!(app.state.playlist.popup.is_some());
        app.state.playlist.popup = None;
        // A menu bar dropdown.
        app.state.menu_open = Some(views::menus::TOOLS_MENU);
        frame(&mut app, &mut c, None);
        let first = app.hits.iter().find(|h| h.menu == MENU_DROPDOWN).map(|h| (h.rect, h.action.clone())).expect("dropdown rows");
        app.mouse_move(10.0, 10.0);
        app.wheel(0.0, -1.0, false, false);
        assert_eq!((app.mouse.1, lit(&app)), (first.0.cy(), Some(first.1)));
        assert!(app.warp.take().is_some());
        // With no menu open the wheel does what it did.
        app.state.menu_open = None;
        frame(&mut app, &mut c, None);
        app.wheel(0.0, -1.0, false, false);
        assert_eq!(app.warp, None);
    }

    /// A popup wider than the window (spec: ui/menus.md "Menus wider than the window"): the columns that fit and a scroll
    /// bar; the wheel steps on into the columns out of view, scrolling them in, and round to either end; sideways or over
    /// the bar it scrolls a column a notch; a press on the bar keeps the menu open; scrolling closes the open submenu. A
    /// menu that fits has no bar.
    #[test]
    fn wide_menus_scroll_sideways() {
        use crate::playlist::{Cmd, PItem, Popup};
        let Ok(mut c) = Canvas::new(800, 600, 1.0) else { return };
        let mut app = App::new(None);
        let mut items: Vec<PItem> = (0..300).map(|i| PItem::new(format!("Entry {i}"), Cmd::Nothing)).collect();
        items[0] = PItem::sub("More", vec![PItem::new("Inner", Cmd::Nothing)]);
        app.state.playlist.popup = Some(Popup { x: 300.0, y: 200.0, items, sub: None, scroll: [0; 2] });
        frame(&mut app, &mut c, None);
        let max = app.bar(Scroll::Popup).expect("a scroll bar").max as usize;
        assert!(max > 0);
        // The rows that can be clicked are all in the window; the others are out of view.
        let shown = |app: &App| -> Vec<usize> {
            let rows = app.hits.iter().filter(|h| h.rect.h > 0.0);
            rows.filter_map(|h| match h.action {
                Action::PlPopup(i) => {
                    assert!(h.rect.x >= 0.0 && h.rect.right() <= 800.0 && h.rect.bottom() <= 600.0, "row {i} in the window");
                    Some(i)
                }
                _ => None,
            })
            .collect()
        };
        let scroll = |app: &App| app.state.playlist.popup.as_ref().map(|p| p.scroll[0]);
        let k = *shown(&app).iter().max().unwrap();
        assert!(k < 299 && !shown(&app).contains(&299));
        // Down from the last row shown: one column on, the first row of the column that came into view under the cursor.
        let r = hit_rect(&app, &Action::PlPopup(k));
        app.mouse_move(r.cx(), r.cy());
        app.wheel(0.0, -1.0, false, false);
        let to = app.warp.take().expect("the cursor moves");
        assert_eq!((scroll(&app), app.hover.clone()), (Some(1), Some(Action::PlPopup(k + 1))));
        frame(&mut app, &mut c, None);
        assert_eq!(app.top_hit(to.0, to.1).map(|h| h.action.clone()), Some(Action::PlPopup(k + 1)));
        assert!(shown(&app).contains(&(k + 1)) && !shown(&app).contains(&1));
        // Back up to a row in view: no scroll.
        app.wheel(0.0, 1.0, false, false);
        assert_eq!((scroll(&app), app.hover.clone()), (Some(1), Some(Action::PlPopup(k))));
        // From off the menu, up lights the last row: scrolled to the end; down from it goes round to the start.
        app.mouse_move(10.0, 10.0);
        app.wheel(0.0, 1.0, false, false);
        assert_eq!((scroll(&app), app.hover.clone()), (Some(max), Some(Action::PlPopup(299))));
        frame(&mut app, &mut c, None);
        assert!(shown(&app).contains(&299));
        app.wheel(0.0, -1.0, false, false);
        assert_eq!((scroll(&app), app.hover.clone()), (Some(0), Some(Action::PlPopup(0))));
        frame(&mut app, &mut c, None);
        // Shift+wheel, a sideways wheel, and the wheel over the bar scroll a column a notch; nothing lights.
        app.warp = None;
        app.wheel(0.0, -2.0, true, false);
        assert_eq!(scroll(&app), Some(2));
        app.wheel(1.0, 0.0, false, false);
        assert_eq!(scroll(&app), Some(1));
        frame(&mut app, &mut c, None);
        let track = app.bar(Scroll::Popup).unwrap().track;
        app.mouse_move(track.cx(), track.cy());
        app.wheel(0.0, -1.0, false, false);
        assert_eq!((scroll(&app), app.warp), (Some(2), None));
        // A press on the bar's track jumps there and keeps the menu open.
        frame(&mut app, &mut c, None);
        click(&mut app, &mut c, Rect::new(track.right() - 2.0, track.y, 1.0, track.h));
        assert_eq!(scroll(&app), Some(max));
        click(&mut app, &mut c, Rect::new(track.x, track.y, 1.0, track.h));
        assert_eq!(scroll(&app), Some(0));
        // Scrolling closes the open submenu (it fits: no bar of its own).
        let more = hit_rect(&app, &Action::PlPopup(0));
        click(&mut app, &mut c, more);
        assert_eq!(app.state.playlist.popup.as_ref().and_then(|p| p.sub), Some(0));
        assert!(app.bar(Scroll::PopupSub).is_none());
        app.mouse_move(more.cx(), more.cy());
        app.wheel(0.0, -1.0, true, false);
        assert_eq!(app.state.playlist.popup.as_ref().map(|p| (p.sub, p.scroll)), Some((None, [1, 0])));
        // A menu that fits is drawn as before.
        app.state.playlist.popup = Some(Popup { x: 300.0, y: 200.0, items: vec![PItem::new("A", Cmd::Nothing)], sub: None, scroll: [0; 2] });
        frame(&mut app, &mut c, None);
        assert!(app.bar(Scroll::Popup).is_none());
        // A wide submenu scrolls on its own: the wheel steps on into its next column, and over its bar scrolls it.
        let inner = (0..300).map(|i| PItem::new(format!("Inner {i}"), Cmd::Nothing)).collect();
        app.state.playlist.popup = Some(Popup { x: 0.0, y: 0.0, items: vec![PItem::sub("Big", inner)], sub: Some(0), scroll: [0; 2] });
        frame(&mut app, &mut c, None);
        assert!(app.bar(Scroll::Popup).is_none());
        let track = app.bar(Scroll::PopupSub).expect("the submenu's own bar").track;
        let k = (0..300).rev().find(|&i| app.hits.iter().any(|h| h.action == Action::PlPopupSub(i) && h.rect.h > 0.0)).unwrap();
        let r = hit_rect(&app, &Action::PlPopupSub(k));
        app.mouse_move(r.cx(), r.cy());
        app.wheel(0.0, -1.0, false, false);
        let sub = |app: &App| app.state.playlist.popup.as_ref().map(|p| (p.sub, p.scroll));
        assert_eq!((sub(&app), app.hover.clone()), (Some((Some(0), [0, 1])), Some(Action::PlPopupSub(k + 1))));
        frame(&mut app, &mut c, None);
        app.mouse_move(track.cx(), track.cy());
        app.wheel(0.0, -1.0, false, false);
        assert_eq!(sub(&app), Some((Some(0), [0, 2])));
    }

    /// With hundreds of plugins (issue #134) the F8 menu is wider than the window: its footer is out of view at first;
    /// the scroll bar brings it in, and Manage plugins... works there (spec: ui/menus.md "Menus wider than the window").
    #[test]
    fn plugin_menu_with_many_plugins_scrolls_to_its_footer() {
        use winit::keyboard::{Key, NamedKey};
        let Ok(mut c) = Canvas::new(1366, 768, 1.0) else { return };
        let mut app = App::new(None);
        app.plugins = crate::plugins::Plugins::builtins();
        for i in 0..300 {
            app.plugins.available.push(crate::plugin_db::fake(&format!("Test Synth {i}"), os_host::Kind::Instrument));
            app.plugins.effects.push(crate::plugin_db::fake(&format!("Test FX {i}"), os_host::Kind::Effect));
        }
        app.state.plugdb.rebuild(&app.plugins);
        frame(&mut app, &mut c, None);
        app.key(&Key::Named(NamedKey::F8), false, false);
        frame(&mut app, &mut c, None);
        let manage = app.state.playlist.popup.as_ref().and_then(|p| p.items.iter().position(|i| i.label == "Manage plugins...")).expect("the F8 menu");
        let row = |app: &App| app.hits.iter().find(|h| h.action == Action::PlPopup(manage) && h.rect.h > 0.0).map(|h| h.rect);
        assert_eq!(row(&app), None, "out of view at first");
        let track = app.bar(Scroll::Popup).expect("a scroll bar").track;
        click(&mut app, &mut c, Rect::new(track.right() - 2.0, track.y, 1.0, track.h));
        assert!(crate::plugin_menu::is_open(&app.state), "the bar keeps the menu open");
        let r = row(&app).expect("in view at the end");
        assert!(r.x >= 0.0 && r.right() <= 1366.0);
        click(&mut app, &mut c, r);
        assert!(app.state.settings.open && app.state.settings.tab == views::settings::PLUGINS);
    }

    /// The automation point menu's Copy value / Paste value (spec: ui/playlist.md §8 "Point menu"): one clipboard with
    /// the knobs and parameters, Paste value naming the copied value as a percentage and greyed out until something
    /// was copied; a paste is one undo step.
    #[test]
    fn point_menu_copies_and_pastes_values() {
        use crate::playlist::{self as pl, Cmd};
        use crate::knobs::KnobId;
        let mut app = App::new(None);
        let ch = pl::create_automation(&mut app.state, 0, "Auto", 0.25);
        let s = &mut app.state;
        let labels = |s: &State| pl::point_menu(s, ch, 1, 0.0, 0.0).items.iter().map(|it| (it.label.to_string(), it.enabled)).collect::<Vec<_>>();
        let l = labels(s);
        let at = l.iter().position(|(n, _)| n == "Copy value").expect("Copy value");
        assert_eq!(l[at..], [("Copy value".into(), true), ("Paste value".into(), false), ("Enter value...".into(), true)]);
        // Copy a point, paste onto the main volume knob; its menu names the value.
        pl::run(&mut app, Cmd::PointCopy(ch, 1));
        assert_eq!(app.state.value_clip, Some(0.25));
        let knob_menu = crate::rack::knob_menu(&app.state, KnobId::MasterVol, None, 0.0, 0.0);
        assert!(knob_menu.items.iter().any(|it| it.label == "Paste value (25%)" && it.enabled));
        crate::rack::run(&mut app, crate::rack::RCmd::PasteValue(KnobId::MasterVol));
        assert!((app.state.knob(KnobId::MasterVol) - 0.25).abs() < 1e-3);
        // Copy a knob, paste onto the first point: it moves to that value and stays at its time; undo puts it back.
        app.state.set_knob(KnobId::MasterVol, 0.8);
        crate::rack::run(&mut app, crate::rack::RCmd::CopyValue(KnobId::MasterVol));
        assert_eq!(labels(&app.state)[at + 1], ("Paste value (80%)".into(), true));
        pl::run(&mut app, Cmd::PointPaste(ch, 0));
        let p0 = app.state.project.automation(ch).unwrap().points[0];
        assert!((p0.value - 0.8).abs() < 1e-3 && p0.beat == 0.0, "{p0:?}");
        crate::history::undo(&mut app.state);
        assert_eq!(app.state.project.automation(ch).unwrap().points[0].value, 0.25);
    }

    /// The CPU panel opens Settings > Audio; its combos open menus over the window and a choice becomes the audio
    /// settings (spec: ui/main-window.md "CPU panel", ui/settings.md "Audio").
    #[test]
    fn cpu_panel_and_audio_settings() {
        use crate::audio_cfg::AudioCfg;
        use crate::views::settings::{SAct, AUDIO, BUFFER_LENGTH, OUTPUT_DEVICE, SAMPLE_RATE};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        frame(&mut app, &mut c, None);
        let r = hit_rect(&app, &Action::CpuPanel);
        assert!(app.state.cpu.update(std::time::Instant::now(), Some(0.25), 3, 0));
        app.mouse_move(r.cx(), r.cy());
        assert!(app.hint.contains("Audio load 25%") && app.hint.contains("3 voices"), "{}", app.hint);
        // One click does nothing; the menu's Audio settings opens the Audio tab.
        click(&mut app, &mut c, r);
        assert!(!app.state.settings.open);
        app.mouse_move(r.cx(), r.cy());
        app.mouse_down_right();
        frame(&mut app, &mut c, None);
        let i = app.state.playlist.popup.as_ref().expect("the panel's menu").items.iter().position(|it| it.label == "Audio settings").unwrap();
        let item = hit_rect(&app, &Action::PlPopup(i));
        click(&mut app, &mut c, item);
        assert!(app.state.settings.open && app.state.settings.tab == AUDIO);
        assert!(!app.state.settings.status[0].is_empty());
        let combo = |app: &App, label: &str| {
            app.hits.iter().rev().find(|h| matches!(&h.action, Action::Settings(SAct::Combo(l, ..)) if *l == label)).map(|h| h.rect).unwrap_or_else(|| panic!("{label} works"))
        };
        let devices = if cfg!(windows) { "Windows devices" } else { "Default device" };
        // The buffer lengths start with the device's own ("Device default", or its period where Windows says it).
        let mut menus = vec![(OUTPUT_DEVICE, devices), (SAMPLE_RATE, "Device rate"), (BUFFER_LENGTH, "Device")];
        if cfg!(windows) || cfg!(target_os = "linux") {
            menus.push((views::settings::MODE, views::settings::MODE_SHARED));
        }
        for (label, first) in menus {
            let r = combo(&app, label);
            click(&mut app, &mut c, r);
            let p = app.state.playlist.popup.as_ref().unwrap_or_else(|| panic!("{label} opens a menu"));
            assert!(p.items[0].label.starts_with(first), "{label}: {}", p.items[0].label);
            assert!((p.y - r.bottom()).abs() < 2.0, "under its box");
            // The menu is painted over the settings window: its rows take the clicks (the first row under a heading).
            let row = if p.items[0].head { 30.0 } else { 10.0 };
            assert!(app.top_hit(r.x + 10.0, r.bottom() + row).is_some_and(|h| matches!(h.action, Action::PlPopup(_))));
            app.state.playlist.popup = None;
            frame(&mut app, &mut c, None);
        }
        let r = combo(&app, BUFFER_LENGTH);
        click(&mut app, &mut c, r);
        // (Windows shared mode offers only lengths that change something: here, longer than the buffer it grants.)
        let i = app.state.playlist.popup.as_ref().unwrap().items.iter().position(|it| it.label.starts_with("2048 frames")).expect("2048 offered");
        let r = hit_rect(&app, &Action::PlPopup(i));
        click(&mut app, &mut c, r);
        assert!(app.state.playlist.popup.is_none());
        assert_eq!(AudioCfg::current().buffer, Some(2048));
        assert_eq!(app.state.settings.audio.buffer, Some(2048));
        // Back to the defaults for the other tests in this process.
        AudioCfg::default().apply();
    }

    /// Metronome (spec: ui/metronome.md §1-2): the toolbar button and OPTIONS > Metronome toggle the shared switch
    /// (the button lit while on); right-click on the button lists the five sounds with the current one marked.
    #[test]
    fn metronome_button_menu_and_sound() {
        use os_engine::metronome::Sound;
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        frame(&mut app, &mut c, None);
        let was = crate::metronome::on();
        let r = hit_rect(&app, &Action::Metronome);
        assert_eq!((r.x, r.y), (580.0, 4.0));
        click(&mut app, &mut c, r);
        assert_eq!(crate::metronome::on(), !was);
        let opt = (0..60).find(|&i| views::menus::item_at(views::menus::OPTIONS_MENU, i).is_some_and(|it| it.label == "Metronome")).unwrap();
        app.menu_item(views::menus::OPTIONS_MENU, opt);
        assert_eq!(crate::metronome::on(), was);
        // The sound menu: a header, then the sounds; picking one keeps the switch.
        app.mouse_move(r.cx(), r.cy());
        app.mouse_down_right();
        frame(&mut app, &mut c, None);
        let labels: Vec<String> = app.state.playlist.popup.as_ref().unwrap().items.iter().map(|it| it.label.to_string()).collect();
        let volumes = ["Metronome volume", "-12 dB", "-6 dB", "0 dB", "+3 dB", "+6 dB", "+9 dB", "+12 dB"];
        let sounds = ["Metronome sound", "Hi-hat", "Tick", "Beep", "Cowbell", "Pulse"];
        assert_eq!(labels, [&sounds[..], &volumes[..], &["Count-in", "Off", "1 bar", "2 bars"][..]].concat());
        let before = crate::metronome::sound();
        let i = labels.iter().position(|x| x == "Cowbell").unwrap();
        let item = hit_rect(&app, &Action::PlPopup(i));
        click(&mut app, &mut c, item);
        assert_eq!((crate::metronome::sound(), crate::metronome::on()), (Sound::Cowbell, was));
        crate::metronome::set_sound(before);
        // The volume (spec §2): 0 dB marked at first; a pick keeps the switch and the sound and says so.
        let marked = |app: &App| app.state.playlist.popup.as_ref().unwrap().items.iter().filter(|it| it.mark == crate::playlist::Mark::Radio(true)).map(|it| it.label.to_string()).collect::<Vec<_>>();
        app.mouse_move(r.cx(), r.cy());
        app.mouse_down_right();
        frame(&mut app, &mut c, None);
        assert_eq!(marked(&app), [before.label(), "0 dB", "Off"]);
        let i = labels.iter().position(|x| x == "+6 dB").unwrap();
        let item = hit_rect(&app, &Action::PlPopup(i));
        click(&mut app, &mut c, item);
        assert_eq!((crate::metronome::volume(), crate::metronome::sound(), crate::metronome::on()), (6, before, was));
        assert_eq!(app.state.message.as_deref(), Some("Metronome volume: +6 dB"));
        app.mouse_move(r.cx(), r.cy());
        app.mouse_down_right();
        frame(&mut app, &mut c, None);
        assert_eq!(marked(&app), [before.label(), "+6 dB", "Off"]);
        crate::metronome::set_volume(0);
    }

    /// Auto scrolling (spec: ui/auto-scrolling.md §1-2): the toolbar button and OPTIONS > Auto scrolling toggle the
    /// switch (the button lit while on); right-click on the button toggles continuous scrolling.
    #[test]
    fn auto_scrolling_button_and_menu() {
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        frame(&mut app, &mut c, None);
        assert!(!crate::autoscroll::on());
        let r = hit_rect(&app, &Action::AutoScroll);
        assert_eq!((r.x, r.y), (613.0, 45.0));
        click(&mut app, &mut c, r);
        assert!(crate::autoscroll::on());
        let opt = (0..60).find(|&i| views::menus::item_at(views::menus::OPTIONS_MENU, i).is_some_and(|it| it.label == "Auto scrolling")).unwrap();
        app.menu_item(views::menus::OPTIONS_MENU, opt);
        assert!(!crate::autoscroll::on());
        app.mouse_move(r.cx(), r.cy());
        app.mouse_down_right();
        frame(&mut app, &mut c, None);
        let labels: Vec<String> = app.state.playlist.popup.as_ref().unwrap().items.iter().map(|it| it.label.to_string()).collect();
        assert_eq!(labels, ["Continuous scrolling"]);
        let item = hit_rect(&app, &Action::PlPopup(0));
        click(&mut app, &mut c, item);
        assert_eq!(crate::autoscroll::get(), crate::autoscroll::Settings { on: false, continuous: false });
    }

    /// Auto scrolling follows the playhead (spec: ui/auto-scrolling.md §5): the playlist in song mode, the piano roll in
    /// pattern mode; continuous at a third of the width, paged by a quarter page at 7/8; not while paused or off. The
    /// playlist's scroll is drawn by moving pixels: each frame must equal a full repaint.
    #[test]
    fn auto_scrolling_follows_the_playhead() {
        let Ok(mut a) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut b = Canvas::new(1920, 1080, 1.0).unwrap();
        let mut app = App::new(None);
        app.state.project = test_project();
        crate::playlist::place(&mut app.state, 0, 0);
        crate::playlist::place(&mut app.state, 1, 1536);
        for w in [Win::Playlist, Win::PianoRoll] {
            let ws = &mut app.state.wins[w.slot().unwrap()];
            ws.maxed = false;
            ws.rect = Rect::new(220.0, 90.0, 1000.0, 480.0);
        }
        let play = |tick: f64, song: bool| Shown { playing: true, paused: false, tick, song, sounding: Vec::new(), tempo: None };
        crate::autoscroll::toggle();

        // Song mode, playlist in front.
        app.state.z = vec![Win::Playlist];
        frame(&mut app, &mut a, None);
        let g = app.pl_grid.expect("playlist shown");
        let ppt = |app: &App| app.state.playlist_bar_w / (f32::from(app.state.project.ppq.max(4)) * 4.0);
        let tick_at = |app: &App, x: f32| f64::from(x / ppt(app));
        app.show_playback(play(0.0, true));
        let d = app.take_damage();
        frame_damage(&mut app, &mut a, d);
        assert_eq!(app.state.playlist_scroll.0, 0.0);
        // Continuous: still up to a third, then held there.
        app.show_playback(play(tick_at(&app, g.w / 3.0 - 2.0), true));
        assert_eq!(app.state.playlist_scroll.0, 0.0);
        for k in [0.5, 0.52, 0.9] {
            let x = g.w * k;
            app.show_playback(play(tick_at(&app, x), true));
            assert!((app.state.playlist_scroll.0 - (x - g.w / 3.0)).abs() <= 0.5, "continuous at {k}");
            let d = app.take_damage();
            frame_damage(&mut app, &mut a, d);
            frame(&mut app, &mut b, None);
            assert_same(&a, &b, &format!("continuous at {k}"));
        }
        // Paged: nothing until 7/8 of the width, then a quarter page.
        crate::autoscroll::toggle_continuous();
        app.state.playlist_scroll.0 = 0.0;
        frame(&mut app, &mut a, None);
        app.show_playback(play(tick_at(&app, g.w * 0.8), true));
        assert_eq!(app.state.playlist_scroll.0, 0.0);
        app.show_playback(play(tick_at(&app, g.w * 0.9), true));
        assert!((app.state.playlist_scroll.0 - g.w * 0.275).abs() <= 0.5, "paged: {}", app.state.playlist_scroll.0);
        let d = app.take_damage();
        frame_damage(&mut app, &mut a, d);
        frame(&mut app, &mut b, None);
        assert_same(&a, &b, "paged step");
        // Back to the start: the view goes with it.
        app.show_playback(play(0.0, true));
        assert_eq!(app.state.playlist_scroll.0, 0.0);
        // Paused: the view stays.
        app.show_playback(Shown { paused: true, ..play(tick_at(&app, g.w * 3.0), true) });
        assert_eq!(app.state.playlist_scroll.0, 0.0);
        // Off: the view stays.
        app.show_playback(play(0.0, true));
        crate::autoscroll::toggle();
        app.show_playback(play(tick_at(&app, g.w * 3.0), true));
        assert_eq!(app.state.playlist_scroll.0, 0.0);
        app.show_playback(Shown::default());

        // Pattern mode, piano roll in front: its own width and zoom.
        crate::autoscroll::toggle();
        app.state.z = vec![Win::PianoRoll];
        frame(&mut app, &mut a, None);
        let g = app.piano_grid.expect("piano roll shown");
        let ppq = f64::from(app.state.project.ppq.max(4));
        let tick = f64::from(g.w) * 0.9 / f64::from(app.state.piano.px_per_beat) * ppq;
        app.show_playback(play(0.0, false));
        app.show_playback(play(tick, false));
        assert!((app.scroll_get(Scroll::PianoX) - g.w * 0.275).abs() <= 0.5, "piano roll paged: {}", app.scroll_get(Scroll::PianoX));
        let d = app.take_damage();
        frame_damage(&mut app, &mut a, d);
        frame(&mut app, &mut b, None);
        assert_same(&a, &b, "piano roll step");
    }

    /// Song mode: the piano roll follows too, by its pattern's position, while a clip of the pattern plays there; past
    /// the clips it stays (spec: ui/auto-scrolling.md §5, issue 192).
    #[test]
    fn piano_roll_follows_in_song_mode() {
        let Ok(mut a) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        crate::playlist::arr_mut(&mut app.state).clips.clear();
        let bar = app.state.project.bar_ticks();
        let c = crate::playlist::place(&mut app.state, 0, 2 * bar);
        crate::playlist::arr_mut(&mut app.state).clips[c].length = 8 * bar;
        app.state.project.patterns.iter_mut().filter(|p| p.number == app.state.pattern).for_each(|p| p.length = Some(8 * bar));
        let ws = &mut app.state.wins[Win::PianoRoll.slot().unwrap()];
        ws.maxed = false;
        ws.rect = Rect::new(220.0, 90.0, 1000.0, 480.0);
        app.state.z = vec![Win::PianoRoll];
        crate::autoscroll::toggle();
        frame(&mut app, &mut a, None);
        let g = app.piano_grid.expect("piano roll shown");
        let ppq = f64::from(app.state.project.ppq.max(4));
        let per_px = ppq / f64::from(app.state.piano.px_per_beat);
        let play = |tick: f64| Shown { playing: true, paused: false, tick, song: true, sounding: Vec::new(), tempo: None };
        app.show_playback(play(0.0));
        // In the clip: the pattern position, 0.9 of the width in, is held at a third.
        let p = f64::from(g.w) * 0.9 * per_px;
        for k in 1..=9 {
            app.show_playback(play(f64::from(2 * bar) + p * f64::from(k) / 9.0));
            frame(&mut app, &mut a, None);
        }
        let want = g.w * 0.9 - g.w / 3.0;
        assert!((app.scroll_get(Scroll::PianoX) - want).abs() <= 1.0, "{} != {want}", app.scroll_get(Scroll::PianoX));
        // Past the clip: the view stays.
        app.show_playback(play(f64::from(20 * bar)));
        assert!((app.scroll_get(Scroll::PianoX) - want).abs() <= 1.0);
    }

    /// CPU panel (spec: ui/main-window.md §13): the menu's View FPS / Misc / Refresh change what it shows; a double
    /// click opens the Plugin performance monitor (spec: ui/plugin-performance.md), which lists the hosted
    /// instruments, sorts, finds and closes from the VIEW menu.
    #[test]
    fn cpu_panel_menu_and_performance_monitor() {
        use crate::cpu::{CpuCmd, Refresh};
        use crate::perf::{Col, PerfAct};
        use std::time::{Duration, Instant};
        use winit::keyboard::{Key, NamedKey, SmolStr};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        frame(&mut app, &mut c, None);
        let r = hit_rect(&app, &Action::CpuPanel);
        // The menu: Refresh radio items in a submenu, the view pair, Misc.
        app.mouse_move(r.cx(), r.cy());
        app.mouse_down_right();
        frame(&mut app, &mut c, None);
        let labels: Vec<String> = app.state.playlist.popup.as_ref().unwrap().items.iter().map(|it| it.label.to_string()).collect();
        for l in ["Refresh", "View voice count", "View FPS", "Misc", "Plugin performance monitor", "Audio settings"] {
            assert!(labels.iter().any(|x| x == l), "{l} in {labels:?}");
        }
        let i = labels.iter().position(|x| x == "View FPS").unwrap();
        let item = hit_rect(&app, &Action::PlPopup(i));
        click(&mut app, &mut c, item);
        assert!(app.state.cpu.fps_view);
        app.state.cpu.frame();
        app.state.cpu.frame();
        let t0 = Instant::now();
        assert!(app.state.cpu.update(t0, None, 4, 0));
        assert!(app.state.cpu.hint().contains("frames per second"), "{}", app.state.cpu.hint());
        app.cpu_cmd(CpuCmd::Fps(false));
        app.cpu_cmd(CpuCmd::Misc);
        assert!(app.state.cpu.update(t0, None, 4, 0), "a reading comes at once after a change");
        let m = app.state.cpu.now.misc.expect("Misc reads the counts");
        assert!(m.threads > 0 && m.fonts > 0, "{m:?}");
        #[cfg(windows)]
        assert!(m.handles > 0);
        assert!(app.state.cpu.hint().contains("4 voices") && app.state.cpu.hint().contains(" threads"), "{}", app.state.cpu.hint());
        frame(&mut app, &mut c, None);
        app.cpu_cmd(CpuCmd::Misc);
        app.cpu_cmd(CpuCmd::Refresh(Refresh::Slow));
        assert!(app.state.cpu.update(t0, None, 0, 0));
        assert!(!app.state.cpu.due(t0 + Duration::from_millis(500)) && app.state.cpu.due(t0 + Duration::from_millis(1000)));
        // Double click: the monitor, with a row for each hosted instance.
        app.plugins = crate::plugins::Plugins::default();
        app.plugins.available = crate::plugins::builtin();
        let osc = app.plugins.index_of(crate::osc3::LEMON_TRIO).unwrap();
        app.plugins.create_with(0, osc, None).unwrap();
        click(&mut app, &mut c, r);
        assert!(!app.state.shown(Win::Perf), "one click does nothing");
        app.mouse_move(r.cx(), r.cy());
        app.mouse_down();
        app.mouse_up();
        frame(&mut app, &mut c, None);
        assert!(app.state.shown(Win::Perf) && app.state.in_front(Win::Perf));
        let p = &app.state.perf;
        assert_eq!(p.rows.len(), 1);
        assert_eq!((p.rows[0].name.as_str(), p.rows[0].synth, p.rows[0].voices), ("Lemon Trio", true, Some(0)));
        assert!(app.hits.iter().any(|h| h.action == Action::Perf(PerfAct::Row(0))));
        // Sorting by a header; a second click reverses.
        let h = hit_rect(&app, &Action::Perf(PerfAct::Sort(Col::Plugin)));
        click(&mut app, &mut c, h);
        assert_eq!((app.state.perf.sort, app.state.perf.desc), (Col::Plugin, false));
        click(&mut app, &mut c, h);
        assert!(app.state.perf.desc);
        // Find takes the typing (the typing keyboard does not play), Esc clears it.
        let f = hit_rect(&app, &Action::Perf(PerfAct::Find));
        click(&mut app, &mut c, f);
        assert!(app.state.perf.finding);
        for ch in ["z", "z"] {
            app.key(&Key::Character(SmolStr::new(ch)), false, false);
        }
        assert_eq!(app.state.perf.find, "zz");
        assert!(app.state.perf.shown().is_empty());
        frame(&mut app, &mut c, None);
        app.key(&Key::Named(NamedKey::Backspace), false, false);
        app.key(&Key::Named(NamedKey::Escape), false, false);
        assert!(app.state.perf.find.is_empty() && !app.state.perf.finding);
        // Reset on transport and Freeze are toggles.
        let t = hit_rect(&app, &Action::Perf(PerfAct::Freeze));
        click(&mut app, &mut c, t);
        assert!(app.state.perf.freeze);
        // VIEW > Plugin performance monitor closes it while it is in front; closing resets the readings.
        app.act(Action::ToggleWin(Win::Perf));
        assert!(!app.state.shown(Win::Perf) && app.state.perf.rows.is_empty());
        app.act(Action::ToggleWin(Win::Perf));
        assert!(app.state.shown(Win::Perf));
        frame(&mut app, &mut c, None);
        let x = hit_rect(&app, &Action::CloseWin(Win::Perf));
        click(&mut app, &mut c, x);
        assert!(!app.state.shown(Win::Perf));
    }

    /// Tempo tapper (spec: ui/tempo-tapper.md): VIEW > Tempo tapper opens it centred over the main window, ticked in
    /// the menu; clicks on the pad set the project tempo (whole BPM, clamped to the tempo box) while Apply tempo is on,
    /// only show it while off, and turning it on applies the shown tempo; a long pause starts a new count; MIDI keys
    /// tap (a chord once); the title bar moves it, Ctrl+F5 and its close button close it, and it reopens where it was.
    #[test]
    fn tempo_tapper_window() {
        use crate::tapper::TapAct;
        use views::menus::{bar_model, Mark, VIEW};
        use winit::keyboard::{Key, NamedKey};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        frame(&mut app, &mut c, None);
        let view = hit_rect(&app, &Action::Menu(VIEW));
        click(&mut app, &mut c, view);
        let item = hit_rect(&app, &Action::ToggleWin(Win::Tapper));
        click(&mut app, &mut c, item);
        assert!(app.state.shown(Win::Tapper) && app.state.in_front(Win::Tapper));
        assert_eq!(views::tapper::rect(&app.state), Rect::new(882.0, 393.0, 156.0, 294.0), "156 x 294, centred over the main window");
        let row = bar_model(&app.state)[VIEW].rows.iter().find(|r| r.label == "Tap tempo").cloned().unwrap();
        assert_eq!((row.enabled, row.mark, row.shortcut), (true, Mark::Check(true), "Ctrl+F5"));
        // The hints; the controls that do nothing yet only say so.
        let hint = |app: &App, a: &Action| app.hits.iter().rev().find(|h| &h.action == a).map(|h| h.hint.to_string()).unwrap();
        assert_eq!(hint(&app, &Action::Tapper(TapAct::Pad)), "Tap me, or press any key on your MIDI keyboard");
        assert_eq!(hint(&app, &Action::Tapper(TapAct::Nudge(-1))), "Nudge tempo to align time (Left / pitch bend)");
        assert_eq!(hint(&app, &Action::Tapper(TapAct::Nudge(1))), "Nudge tempo to align time (Right / pitch bend)");
        // A nudge is held while playing only, and any release lets it go (spec §6).
        app.act(Action::Tapper(TapAct::Nudge(1)));
        assert_eq!(crate::tapper::nudging(), 0, "stopped: nothing");
        app.transport.pretend_at(0.0, false);
        app.act(Action::Tapper(TapAct::Nudge(-1)));
        assert_eq!(crate::tapper::nudging(), -1);
        app.release();
        assert_eq!(crate::tapper::nudging(), 0);
        app.transport.stop();
        for name in [
            "Resynchronize to taps",
            "Mute all audio during first taps (during playback)",
            "Mute now, for a few bars (during playback)",
        ] {
            assert_eq!(hint(&app, &Action::Placeholder(name)), format!("{name}: not available yet"));
        }

        // Apply tempo on (the default): from the second tap, every tempo is the project's.
        let pad = hit_rect(&app, &Action::Tapper(TapAct::Pad));
        let tap = |app: &mut App, c: &mut Canvas, t: f64| {
            app.state.tapper.test_time = Some(t);
            click(app, c, pad);
        };
        assert!(crate::tapper::get().apply);
        assert_eq!(app.state.tempo, 130.0);
        let steps = app.state.history.at();
        tap(&mut app, &mut c, 10.0);
        assert_eq!((app.state.tapper.count.tempo(), app.state.tempo), (None, 130.0), "one tap: TAP stays");
        assert!(app.state.tapper.flash == 1.0 && app.animating(), "a tap lights the pad, which then fades");
        tap(&mut app, &mut c, 10.5);
        assert_eq!(app.state.tempo, 120.0);
        tap(&mut app, &mut c, 11.0);
        tap(&mut app, &mut c, 11.4);
        assert_eq!(app.state.tempo, 129.0, "the average of every interval, rounded (128.57)");
        assert!(app.unsaved(), "a tapped tempo is a change to the project");
        assert_eq!(app.state.history.at(), steps + 1, "a count's taps are one undo step");
        // Off: the pad shows the tempo, the project keeps its own; turning it on applies the pad's.
        let apply = hit_rect(&app, &Action::Tapper(TapAct::Apply));
        click(&mut app, &mut c, apply);
        assert!(!crate::tapper::get().apply);
        tap(&mut app, &mut c, 11.8);
        assert_eq!((app.state.tapper.count.tempo(), app.state.tempo), (Some(133.0), 129.0));
        click(&mut app, &mut c, apply);
        assert!(crate::tapper::get().apply);
        assert_eq!(app.state.tempo, 133.0);
        // A long pause starts a new count (the tempo stays); the project tempo stays in 10-522.
        tap(&mut app, &mut c, 20.0);
        assert_eq!((app.state.tapper.count.tempo(), app.state.tempo), (None, 133.0));
        tap(&mut app, &mut c, 20.05);
        assert_eq!((app.state.tapper.count.tempo(), app.state.tempo), (Some(1200.0), 522.0));
        // Steps: the first count, turning Apply tempo on, the new count; each undoes to the tempo before it.
        assert_eq!(app.state.history.at(), steps + 3);
        for want in [133.0, 129.0, 130.0] {
            assert_eq!(crate::history::undo(&mut app.state).as_deref(), Some(crate::control::command::TEMPO));
            assert_eq!(app.state.tempo, want);
        }
        for want in [129.0, 133.0, 522.0] {
            crate::history::redo(&mut app.state);
            assert_eq!(app.state.tempo, want);
        }
        // MIDI keys tap while the window is open; a chord's keys are one tap.
        for t in [30.0, 30.03, 30.5] {
            app.state.tapper.test_time = Some(t);
            app.key_tap();
        }
        assert_eq!(app.state.tempo, 120.0);
        // Every tap clicked, accented on the first of each four of a count (§8); the chord's second key did not.
        let (t, f) = (true, false);
        assert_eq!(app.state.tapper.test_clicks, [t, f, f, f, t, t, f, t, f]);
        frame(&mut app, &mut c, None);

        // The title bar moves it; Ctrl+F5 closes it while it is in front and reopens it where it was left.
        let title = hit_rect(&app, &Action::WinDrag(Win::Tapper));
        app.mouse_move(title.x + 40.0, title.cy());
        app.mouse_down();
        app.mouse_move(title.x + 40.0 - 300.0, title.cy() + 100.0);
        app.mouse_up();
        frame(&mut app, &mut c, None);
        assert_eq!(app.state.tapper.pos, (582.0, 493.0));
        app.key(&Key::Named(NamedKey::F5), true, false);
        assert!(!app.state.shown(Win::Tapper));
        assert!(app.state.shown(Win::Playlist), "Ctrl+F5 is not F5");
        app.key(&Key::Named(NamedKey::F5), true, false);
        assert!(app.state.shown(Win::Tapper) && app.state.tapper.pos == (582.0, 493.0));
        assert_eq!(app.state.tapper.count.tempo(), Some(120.0), "the last tempo stays on the pad");
        frame(&mut app, &mut c, None);
        let x = hit_rect(&app, &Action::CloseWin(Win::Tapper));
        click(&mut app, &mut c, x);
        assert!(!app.state.shown(Win::Tapper));
        let row = bar_model(&app.state)[VIEW].rows.iter().find(|r| r.label == "Tap tempo").cloned().unwrap();
        assert_eq!(row.mark, Mark::Check(false));
        // The toolbar's kick drum button (the 8th window button) opens and closes it too.
        let b = hit_rect(&app, &Action::ToggleWin(Win::Tapper));
        assert_eq!((b.x, hint(&app, &Action::ToggleWin(Win::Tapper)).as_str()), (973.0 + 33.0 * 7.0, "Tap tempo (Ctrl+F5)"));
        click(&mut app, &mut c, b);
        assert!(app.state.shown(Win::Tapper) && app.state.in_front(Win::Tapper));
        click(&mut app, &mut c, b);
        assert!(!app.state.shown(Win::Tapper));
    }

    /// While the tapper is open, typing-keyboard notes and Touch controller presses tap like MIDI keys, a chord once
    /// (spec: ui/tempo-tapper.md §8); auto-repeat, the other keys and a glissando do not, and a closed tapper ignores them.
    #[test]
    fn tempo_tapper_typing_and_touch_keys_tap() {
        use crate::touch::TouchAct;
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.selected_channel = 1;
        app.state.toggles.push("typing");
        let key = |app: &mut App, t: f64, code: KeyCode| {
            app.state.tapper.test_time = Some(t);
            app.typing_key(code, true, false, false)
        };
        assert!(key(&mut app, 1.0, KeyCode::KeyZ));
        app.typing_key(KeyCode::KeyZ, false, false, false);
        assert!(app.state.tapper.test_clicks.is_empty(), "the tapper is closed");

        app.act(Action::ToggleWin(Win::Tapper));
        assert!(key(&mut app, 10.0, KeyCode::KeyZ));
        assert!(key(&mut app, 10.03, KeyCode::KeyX), "a chord's second key plays");
        assert!(key(&mut app, 10.2, KeyCode::KeyZ), "auto-repeat");
        assert_eq!(app.typing_held.len(), 2);
        app.typing_key(KeyCode::KeyZ, false, false, false);
        app.typing_key(KeyCode::KeyX, false, false, false);
        assert!(!key(&mut app, 10.3, KeyCode::F1), "not a note key");
        assert!(key(&mut app, 10.5, KeyCode::KeyC));
        app.typing_key(KeyCode::KeyC, false, false, false);
        assert_eq!((app.state.tapper.count.taps(), app.state.tempo), (2, 120.0));

        app.state.tapper.test_time = Some(11.0);
        app.act(Action::Touch(TouchAct::Note(56)));
        assert_eq!(app.state.touch.held, Some((56, 1)), "the pad plays");
        app.state.tapper.test_time = Some(11.2);
        app.touch_hold(Some(57));
        app.touch_hold(None);
        let (t, f) = (true, false);
        assert_eq!((app.state.tapper.test_clicks.as_slice(), app.state.tempo), ([t, f, f].as_slice(), 120.0));
    }

    /// The tapper's flash fades on the ticks (spec: ui/tempo-tapper.md §4, §8): each repaints just the pad, which shows
    /// the same as a full paint, in both designs; once out, ticking stops.
    #[test]
    fn tempo_tapper_flash_repaints_the_pad() {
        use std::time::{Duration, Instant};
        for design in Design::ALL {
            let Ok(mut a) = Canvas::new(1920, 1080, 1.0) else { return };
            let mut b = Canvas::new(1920, 1080, 1.0).unwrap();
            let mut app = App::new(None);
            app.theme = design_theme(design, false);
            app.act(Action::ToggleWin(Win::Tapper));
            app.state.tapper.test_time = Some(1.0);
            app.act(Action::Tapper(crate::tapper::TapAct::Pad));
            app.state.tapper.test_time = Some(1.5);
            app.act(Action::Tapper(crate::tapper::TapAct::Pad));
            frame(&mut app, &mut a, None);
            let _ = app.take_damage();
            let pad = views::tapper::pad_rect(&app.state);
            for (ms, what) in [(100, "fading"), (300, "gone")] {
                app.state.tapper.flash_at = Some(Instant::now() - Duration::from_millis(ms));
                app.tick();
                let d = app.take_damage();
                let Some(Some(rects)) = &d else { panic!("{what}: repaints regions, not the frame") };
                assert!(rects.iter().any(|r| r.intersect(&pad) == pad), "{what}: the pad is repainted ({rects:?})");
                frame_damage(&mut app, &mut a, d);
                frame(&mut app, &mut b, None);
                assert_same(&a, &b, &format!("{design:?}: flash {what}"));
            }
            assert!(!app.animating() && app.state.tapper.flash == 0.0);
        }
    }

    /// How far ahead of an ASIO driver playback renders (spec: audio/devices.md "Render-ahead"): plays a song, muted, at
    /// each buffer length for a while with an edit every second, and prints the frames queued when the driver asked for
    /// a block (least, mean, most), the driver's own latency, the longest time between two blocks (a driver that
    /// stalled), the audio load and the dropouts. Needs the driver installed and free. LEMON_ASIO: the driver
    /// ("ASIO4ALL v2"); LEMON_ASIO_BUFFERS: the lengths ("64,128,256");
    /// LEMON_SONG: an .flp to play, or "trio:<n>" for a pattern of n Lemon Trio channels playing chords on every step;
    /// LEMON_PLUGINS: host the song's plugins the app's last scan found; LEMON_SECONDS: how long at each length (30);
    /// LEMON_RUNS: how many times each length is played (1).
    #[test]
    #[ignore]
    fn render_ahead_on_asio() {
        use crate::audio_cfg::AudioCfg;
        use std::time::{Duration, Instant};
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let driver = var("LEMON_ASIO").unwrap_or_else(|| "ASIO4ALL v2".into());
        let buffers: Vec<u32> = var("LEMON_ASIO_BUFFERS").unwrap_or_else(|| "64,128,256".into()).split(',').filter_map(|b| b.trim().parse().ok()).collect();
        let seconds: u64 = var("LEMON_SECONDS").and_then(|s| s.parse().ok()).unwrap_or(30);
        let runs: usize = var("LEMON_RUNS").and_then(|s| s.parse().ok()).unwrap_or(1);
        let Some(song) = var("LEMON_SONG") else { return eprintln!("LEMON_SONG names no song") };
        let mut app = App::new(None);
        app.plugins = crate::plugins::Plugins::builtins();
        // LEMON_PLUGINS: the plugins the app's last scan found too (the song's FL plugins, hosted as the app hosts them).
        if var("LEMON_PLUGINS").is_some() {
            app.load_plugins();
        }
        let run = |app: &mut App, ms: u64| {
            let t0 = Instant::now();
            while t0.elapsed() < Duration::from_millis(ms) {
                app.tick();
                std::thread::sleep(Duration::from_millis(4));
            }
        };
        if let Some(n) = song.strip_prefix("trio:") {
            // A heavy pattern instead of a song: `n` Lemon Trio channels, each playing four-note chords on every step,
            // a beat long.
            let index = app.plugins.index_of(crate::osc3::LEMON_TRIO).expect("Lemon Trio");
            let mut notes = Vec::new();
            for c in 0..n.parse::<u16>().unwrap_or(8) {
                let row = add_plugin_channel(&mut app.state.project, crate::osc3::LEMON_TRIO);
                app.plugins.create(row, index).expect("a Lemon Trio instance");
                app.state.hosted_rows.push(row);
                let ch = app.state.project.channels[row].index;
                for i in 0..16 {
                    for k in [0, 4, 7, 11] {
                        notes.push(os_model::Note { length: 96, key: 48 + c % 12 + k, ..os_model::Note::step(ch, i * 24) });
                    }
                }
            }
            app.state.project.patterns[0].notes = notes;
            app.state.song_mode = false;
            app.transport.set_instruments(app.plugins.instruments());
        } else {
            app.open(song.clone().into());
        }
        let t0 = Instant::now();
        while app.loading() {
            run(&mut app, 50);
            assert!(t0.elapsed() < Duration::from_secs(180), "the song loads");
        }
        run(&mut app, 500);
        // Muted: the master track's volume at 0 (the engine renders the same).
        if let Some(m) = app.state.project.mixer.iter_mut().find(|m| m.index == 0) {
            m.volume = Some(0.0);
        }
        let (tempo, ppq) = (app.state.project.tempo.unwrap_or(140.0), app.state.project.ppq.max(1));
        eprintln!("{song}: {tempo} BPM, PPQ {ppq}, {} hosted rows", app.state.hosted_rows.len());
        eprintln!("driver | block | rate | tick | ahead min / mean / max (frames) | mean ms | driver latency | longest gap ms | load | dropouts");
        for &b in &buffers {
            for _ in 0..runs {
                AudioCfg { asio: true, device: Some(driver.clone()), rate: None, buffer: Some(b), exclusive: false, input: None, jack: false }.apply();
                app.transport.reopen();
                let t0 = Instant::now();
                let info = loop {
                    run(&mut app, 50);
                    // The stream before the reopen may still be the one open for a moment: wait for the new length (a
                    // driver that refuses it runs at its own).
                    let settled = |i: &crate::device::DeviceInfo| i.fixed == Some(b) || t0.elapsed() > Duration::from_secs(3);
                    if let Some(i) = app.transport.opened().filter(|i| i.host == crate::device::ASIO && i.name == driver && settled(i)) {
                        break Some(i);
                    }
                    if t0.elapsed() > Duration::from_secs(15) {
                        eprintln!("{driver} did not open: {:?}", app.transport.take_error());
                        break None;
                    }
                };
                let Some(info) = info else { continue };
                app.act(Action::Play);
                run(&mut app, 2000);
                eprintln!("{} instruments and {} effects hosted", app.state.hosted_rows.len(), app.plugins.hosted.len() - app.state.hosted_rows.len());
                let _ = app.transport.take_ahead();
                let u0 = app.transport.underruns();
                let (mut min, mut max, mut sum, mut blocks, mut gap, mut load) = (u64::MAX, 0u64, 0.0f64, 0u64, 0u64, 0.0f32);
                for _ in 0..seconds {
                    // An edit: the engine is rebuilt and swapped in at the position playing (LEMON_EDITS=0: none).
                    if var("LEMON_EDITS").is_none_or(|v| v != "0") {
                        app.transport.update(&app.state.project);
                    }
                    run(&mut app, 1000);
                    load = load.max(app.transport.load());
                    if let Some(a) = app.transport.take_ahead() {
                        (min, max, gap) = (min.min(a.min), max.max(a.max), gap.max(a.gap));
                        sum += a.mean * a.blocks as f64;
                        blocks += a.blocks;
                    }
                }
                let du = app.transport.underruns() - u0;
                let rate = f64::from(info.rate);
                let mean = sum / blocks.max(1) as f64;
                let tick = rate * 60.0 / (tempo * f64::from(ppq));
                eprintln!(
                    "{driver} | {} | {} | {tick:.0} | {min} / {mean:.0} / {max} | {:.2} | {:?} | {:.1} | {load:.2} | {du}",
                    app.transport.block(),
                    info.rate,
                    mean * 1000.0 / rate,
                    info.latency,
                    gap as f64 / 1000.0,
                );
                app.act(Action::Stop);
                run(&mut app, 500);
            }
        }
    }

    /// An empty project with a Lemon Trio played from the typing keyboard while the UI ticks, then in a playing
    /// pattern: the streams keep up (no run of dropouts) and the voices end.
    #[test]
    #[ignore]
    fn playing_lemon_3x_osc_keeps_the_audio_going() {
        use std::time::{Duration, Instant};
        use winit::keyboard::KeyCode;
        let mut app = App::new(None);
        app.plugins = crate::plugins::Plugins::default();
        app.plugins.available = crate::plugins::builtin();
        let row = add_plugin_channel(&mut app.state.project, crate::osc3::LEMON_TRIO);
        let index = app.plugins.index_of(crate::osc3::LEMON_TRIO).unwrap();
        app.plugins.create(row, index).unwrap();
        app.state.hosted_rows.push(row);
        app.transport.set_instruments(app.plugins.instruments());
        app.state.selected_channel = row;
        app.state.toggles.push("typing");
        let run = |app: &mut App, ms: u64| {
            let t0 = Instant::now();
            while t0.elapsed() < Duration::from_millis(ms) {
                app.tick();
                std::thread::sleep(Duration::from_millis(4));
            }
        };
        run(&mut app, 300);
        // Past the live stream's first 4095 ticks (about 18 s at 140 BPM): it used to go silent there.
        let u0 = app.transport.underruns();
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(25) {
            app.typing_key(KeyCode::KeyZ, true, false, false);
            run(&mut app, 200);
            app.typing_key(KeyCode::KeyZ, false, false, false);
            run(&mut app, 300);
        }
        let du = app.transport.underruns() - u0;
        eprintln!("long live: {du} dropouts");
        assert!(du < 5, "long live run: {du} dropouts");
        let keys = [KeyCode::KeyZ, KeyCode::KeyX, KeyCode::KeyC, KeyCode::KeyV, KeyCode::KeyB, KeyCode::KeyN, KeyCode::KeyM, KeyCode::KeyQ, KeyCode::KeyW, KeyCode::KeyE];
        let phase = |app: &mut App, name: &str| {
            let u0 = app.transport.underruns();
            for round in 0..30 {
                for (i, &k) in keys.iter().enumerate() {
                    if (round + i) % 3 != 0 {
                        app.typing_key(k, true, false, false);
                    }
                }
                run(app, 30);
                for &k in &keys {
                    app.typing_key(k, false, false, false);
                }
                run(app, 15);
            }
            run(app, 1500);
            let du = app.transport.underruns() - u0;
            let voices = app.transport.engine_voices() + app.plugins.voices();
            eprintln!("{name}: {du} dropouts, {voices} voices after");
            (du, voices)
        };
        let (du, v) = phase(&mut app, "live");
        assert!(du < 5 && v == 0, "live notes: {du} dropouts, {v} voices");
        // A pattern of the synth playing, with live notes on top, then Stop.
        app.state.project.patterns[0].notes = (0..16).map(|i| os_model::Note { length: 24, key: 48 + (i % 5) as u16 * 3, ..os_model::Note::step(app.state.project.channels[row].index, i * 24) }).collect();
        app.act(Action::Play);
        run(&mut app, 300);
        let (du, _) = phase(&mut app, "playing");
        app.act(Action::Stop);
        run(&mut app, 500);
        let v = app.transport.engine_voices() + app.plugins.voices();
        assert!(du < 5 && v == 0, "while playing: {du} dropouts; {v} voices after Stop");
    }

    /// Browser (spec: ui/browser.md): tabs, opening nodes, dragging a pattern and a generator onto the playlist.
    #[test]
    fn browser_tree_and_drag() {
        use crate::views::browser::{rows, Item};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.file = None;
        app.state.path = None;
        app.state.playlist.reset();
        app.state.z = vec![Win::Playlist];
        frame(&mut app, &mut c, None);
        // The all tab lists the roots with content, the preset store and the channel states, all closed.
        let labels: Vec<String> = rows(&app.state).into_iter().map(|r| r.label).collect();
        assert_eq!(labels, ["Current project", "Plugin library", "Plugin presets", "Channel presets"]);
        // Project tab: Current project opened with its patterns (default like the screenshot), History above them.
        let r0 = hit_rect(&app, &Action::BrowserTab(1));
        click(&mut app, &mut c, r0);
        let r = rows(&app.state);
        assert_eq!(r[0].label, "Current project");
        assert_eq!((r[1].label.as_str(), r[1].open), ("History", Some(false)));
        assert_eq!(r[2].label, "Patterns");
        let pi = r.iter().position(|r| r.item == Item::Pattern(1)).expect("pattern row");
        // Drag pattern 1 onto the playlist grid: a pattern clip appears.
        let n0 = crate::playlist::clips(&app.state).len();
        let src = hit_rect(&app, &Action::BrowserRow(pi));
        let grid = app.pl_grid().expect("grid");
        app.mouse_move(src.cx(), src.cy());
        app.mouse_down();
        app.mouse_move(src.cx() + 30.0, src.cy());
        assert!(app.state.browser_drag.is_some(), "drag label shown");
        app.mouse_move(grid.x + 100.0, grid.y + 10.0);
        app.mouse_up();
        assert!(app.state.browser_drag.is_none());
        let clips = crate::playlist::clips(&app.state);
        assert_eq!(clips.len(), n0 + 1);
        assert_eq!(clips.last().unwrap().source, os_model::ClipSource::Pattern(1));
        frame(&mut app, &mut c, None);
        // Collapsing Patterns hides its rows; a plain click is not a drop.
        let pat_hdr = rows(&app.state).iter().position(|r| r.label == "Patterns").unwrap();
        let r0 = hit_rect(&app, &Action::BrowserRow(pat_hdr));
        click(&mut app, &mut c, r0);
        assert!(rows(&app.state).iter().all(|r| r.item != Item::Pattern(1)));
        assert_eq!(crate::playlist::clips(&app.state).len(), n0 + 1);
        // Plugin database tab: open Generators and its Sampler category, drag Lemon Sampler onto the playlist -> a new
        // channel.
        app.plugins = crate::plugins::Plugins::builtins();
        app.state.plugdb.rebuild(&app.plugins);
        let r0 = hit_rect(&app, &Action::BrowserTab(2));
        click(&mut app, &mut c, r0);
        for label in ["Generators", "Sampler"] {
            let gi = rows(&app.state).iter().position(|r| r.label == label).unwrap();
            let r0 = hit_rect(&app, &Action::BrowserRow(gi));
            click(&mut app, &mut c, r0);
        }
        let si = rows(&app.state).iter().position(|r| r.item == Item::Generator(0)).expect("sampler row");
        let ch0 = app.state.project.channels.len();
        let src = hit_rect(&app, &Action::BrowserRow(si));
        app.mouse_move(src.cx(), src.cy());
        app.mouse_down();
        app.mouse_move(grid.x + 200.0, grid.y + 60.0);
        app.mouse_move(grid.x + 210.0, grid.y + 60.0);
        app.mouse_up();
        assert_eq!(app.state.project.channels.len(), ch0 + 1);
        // The favorites tab works (spec §10), empty without favorites; wheel never scrolls past the content. There is no
        // online content tab.
        assert!(app.hits.iter().any(|h| h.action == Action::BrowserTab(3)));
        assert!(!app.hits.iter().any(|h| matches!(h.action, Action::Placeholder("Favorites" | "Online content"))));
        app.act(Action::BrowserTab(3));
        assert!(rows(&app.state).is_empty());
        let r0 = hit_rect(&app, &Action::BrowserTab(0));
        click(&mut app, &mut c, r0);
        app.mouse_move(100.0, 300.0);
        app.wheel(0.0, -5.0, false, false);
        assert_eq!(app.state.browser_scroll, 0.0);
    }

    /// Browser options menu and search (spec: ui/browser.md §4, §5).
    /// A clicked browser sample plays from its file (the 5 s cut-off is the transport's job).
    #[test]
    fn browser_sample_preview_project() {
        let path = std::env::temp_dir().join(format!("os_preview_test_{}.wav", std::process::id()));
        let (rate, n) = (44100u32, 44100usize);
        let mut wav = Vec::new();
        wav.extend(b"RIFF");
        wav.extend(&(36 + n as u32 * 2).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(&16u32.to_le_bytes());
        wav.extend(&1u16.to_le_bytes());
        wav.extend(&1u16.to_le_bytes());
        wav.extend(&rate.to_le_bytes());
        wav.extend(&(rate * 2).to_le_bytes());
        wav.extend(&2u16.to_le_bytes());
        wav.extend(&16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(&(n as u32 * 2).to_le_bytes());
        for i in 0..n {
            let v = ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 16000.0) as i16;
            wav.extend(&v.to_le_bytes());
        }
        std::fs::write(&path, wav).unwrap();
        let p = preview_project(&path.to_string_lossy());
        let mut library = os_engine::SampleLibrary::default();
        let mut e = os_engine::Engine::new(&p, &mut library, 48000, os_engine::PlayMode::Pattern(1));
        let mut buf = vec![0.0f32; 48000];
        e.process(&mut buf);
        let _ = std::fs::remove_file(&path);
        assert!(buf.iter().any(|v| v.abs() > 0.05), "preview is silent");
    }

    /// Audio-clip channels (dropped audio, FL's audio clips) are hosted and opened by Lemon Sampler, never the info
    /// window (spec: plugins/lemon-sampler.md §7).
    #[test]
    fn audio_clip_channels_use_lemon_sampler() {
        let mut app = App::new(None);
        app.plugins = crate::plugins::Plugins::default();
        app.plugins.available = crate::plugins::builtin();
        let wav = std::env::temp_dir().join(format!("os-app-audio-clip-channel-{}.wav", std::process::id()));
        os_engine::wav::write_f32_stereo(&mut std::fs::File::create(&wav).unwrap(), 48000, &[0.5; 9600]).unwrap();
        let path = wav.to_string_lossy().into_owned();
        crate::playlist::add_audio_clip(&mut app.state, &path, 0, 0).expect("audio clip added");
        let row = app.state.project.channels.len() - 1;
        assert_eq!(app.state.project.channels[row].kind, ChannelKind::AudioClip);
        app.plugin_tick();
        assert!(app.state.hosted_rows.contains(&row), "Lemon Sampler hosts the audio clip channel");
        assert_eq!(app.plugins.sampler_path(row).as_deref(), Some(path.as_str()));
        app.toggle_channel_window(row);
        assert!(!app.state.shown(Win::Plugin), "no information window");
        assert_eq!(app.state.pending_open, Some(row), "Lemon Sampler's window opens");
    }

    /// A minute-long audio clip whose file has a `smpl` loop (as FL's exports write), exported as the whole song: the
    /// clip plays the file through once, frame for frame to its end, without jumping back at the loop's end (FL's
    /// clip channels never use loop points: sampler flags 258, spec: audio/audio-clips.md). Also for a clip channel
    /// without sampler flags (made before they were set).
    #[test]
    fn a_long_audio_clip_exports_once() {
        use std::time::Duration;
        let mut a = App::new(None);
        a.plugins = crate::plugins::Plugins::default();
        a.plugins.available = crate::plugins::builtin();
        a.plugins.set_rate(48000);
        let n = 60 * 48000;
        let level = |f: usize| 0.1 + 0.8 * f as f32 / n as f32;
        let wav = std::env::temp_dir().join(format!("os-app-long-clip-{}.wav", std::process::id()));
        let src: Vec<f32> = (0..n).flat_map(|f| [level(f), level(f)]).collect();
        let mut bytes = Vec::new();
        os_engine::wav::write_f32_stereo(&mut bytes, 48000, &src).unwrap();
        // A `smpl` chunk with one loop over the first 50 seconds.
        let words: [u32; 15] = [0, 0, 20833, 60, 0, 0, 0, 1, 0, 0, 0, 0, 50 * 48000, 0, 0];
        bytes.extend_from_slice(b"smpl");
        bytes.extend_from_slice(&60u32.to_le_bytes());
        bytes.extend(words.iter().flat_map(|w| w.to_le_bytes()));
        let riff = (bytes.len() - 8) as u32;
        bytes[4..8].copy_from_slice(&riff.to_le_bytes());
        std::fs::write(&wav, &bytes).unwrap();
        crate::playlist::add_audio_clip(&mut a.state, &wav.to_string_lossy(), 0, 0).expect("audio clip added");
        let ch = a.state.project.channels.last_mut().unwrap();
        assert_eq!(ch.sound.sampler_flags, Some(os_model::ChannelSound::AUDIO_CLIP_FLAGS), "FL's clip channel flags");
        ch.sound.sampler_flags = None;
        a.plugin_tick();
        a.transport.set_instruments(a.plugins.instruments());
        let p = crate::rack::play_project(&a.state);
        let out = std::env::temp_dir().join(format!("os-app-long-clip-{}-export.wav", std::process::id()));
        a.plugins.set_offline(true);
        a.transport.export(&p, PlayMode::Song, 48000, crate::export::Job::wav(&out));
        let t0 = std::time::Instant::now();
        loop {
            match a.transport.export_status() {
                crate::transport::Export::Done(r) => break assert!(r.is_ok(), "{r:?}"),
                _ if t0.elapsed() > Duration::from_secs(120) => panic!("export did not finish"),
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        let bytes = std::fs::read(&out).expect("file written");
        let _ = std::fs::remove_file(&out);
        let _ = std::fs::remove_file(&wav);
        let left: Vec<f32> = bytes[44..].chunks_exact(8).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        // Every quarter second: the sample's level while it plays, silence after it.
        let wrong: Vec<(f32, f32, f32)> = (0..left.len())
            .step_by(12000)
            .filter_map(|f| {
                let want = if f < n - 100 { level(f) } else if f > n + 100 { 0.0 } else { return None };
                ((left[f] - want).abs() > 0.01).then_some((f as f32 / 48000.0, left[f], want))
            })
            .collect();
        assert!(wrong.is_empty(), "{} of {} frames: {:?}", wrong.len(), left.len(), &wrong[..wrong.len().min(8)]);
    }

    /// Lemon Sampler's Keep on disk (spec: plugins/lemon-sampler.md "Keep on disk (Lemon Studio)"): an export, high
    /// quality off (the instances stay in realtime mode), of an audio clip playing from disk and a copy trimmed to start
    /// half way into the file is the export with the sample in memory, to the bit, and no decoded copy of the file is
    /// held while it plays from disk (the host's engine keeps only its length).
    #[test]
    fn an_export_playing_from_disk_is_the_one_from_memory() {
        use serde_json::json;
        use std::time::Duration;
        // 20 seconds of two tones and their overtones at 44100 Hz (the sampler resamples it to 48 kHz).
        let wav = std::env::temp_dir().join(format!("os-app-keep-on-disk-{}.wav", std::process::id()));
        let src: Vec<f32> = (0..20 * 44100)
            .flat_map(|f| {
                let t = f as f32 / 44100.0;
                let tone = |hz: f32| (std::f32::consts::TAU * hz * t).sin();
                [0.3 * tone(220.0) + 0.1 * tone(661.0), 0.3 * tone(331.0) + 0.1 * tone(997.0)]
            })
            .collect();
        os_engine::wav::write_f32_stereo(&mut std::fs::File::create(&wav).unwrap(), 44100, &src).unwrap();
        let export = |keep: bool| -> Vec<u8> {
            let mut a = App::new(None);
            a.plugins = crate::plugins::Plugins::default();
            a.plugins.available = crate::plugins::builtin();
            a.plugins.set_rate(48000);
            crate::playlist::add_audio_clip(&mut a.state, &wav.to_string_lossy(), 0, 0).expect("audio clip added");
            let row = a.state.project.channels.len() - 1;
            a.plugin_tick();
            assert!(a.state.hosted_rows.contains(&row), "Lemon Sampler hosts the audio clip channel");
            let st = crate::plugins::with_params(&a.plugins.state(row).unwrap(), &[("keep_disk", json!({ "bool": keep }))]).unwrap();
            assert!(a.plugins.set_state(row, &st));
            if keep {
                assert!(lemon_sampler::stream::open_files() > 0, "the sample plays from disk");
            }
            a.transport.set_instruments(a.plugins.instruments());
            let first = *crate::playlist::arr_mut(&mut a.state).clips.last().unwrap();
            let second = os_model::Clip { position: first.position + first.length, length: first.length / 2, window: Some((10_000.0, 20_000.0)), ..first };
            crate::playlist::arr_mut(&mut a.state).clips.push(second);
            let p = crate::rack::play_project(&a.state);
            let out = std::env::temp_dir().join(format!("os-app-keep-on-disk-{}-{keep}.wav", std::process::id()));
            // As `start_export` does with high quality off.
            a.plugins.set_offline(false);
            lemon_sampler::stream::export_started();
            a.transport.export(&p, PlayMode::Song, 48000, crate::export::Job::wav(&out));
            let t0 = std::time::Instant::now();
            loop {
                match a.transport.export_status() {
                    crate::transport::Export::Done(r) => break assert!(r.is_ok(), "{r:?}"),
                    _ if t0.elapsed() > Duration::from_secs(120) => panic!("export did not finish"),
                    _ => std::thread::sleep(Duration::from_millis(10)),
                }
            }
            lemon_sampler::stream::export_finished();
            // Playing from disk, nothing holds the decoded file: the engine that rendered the export (and its sample
            // library, which plays on) kept only the length. From memory, the instance holds it.
            assert_eq!(os_engine::sample::held(&wav), !keep, "the decoded file is held: keep on disk {keep}");
            let bytes = std::fs::read(&out).expect("file written");
            let _ = std::fs::remove_file(&out);
            bytes
        };
        let (ram, disk) = (export(false), export(true));
        let _ = std::fs::remove_file(&wav);
        assert_eq!(ram.len(), disk.len());
        let frames = |b: &[u8]| b[44..].chunks_exact(4).map(|v| f32::from_le_bytes([v[0], v[1], v[2], v[3]])).collect::<Vec<f32>>();
        let (a, b) = (frames(&ram), frames(&disk));
        // Sound all the way through both clips (30 seconds).
        assert!(a.len() >= 2 * 29 * 48000 && a[..2 * 29 * 48000].chunks(2 * 4800).all(|c| c.iter().any(|v| v.abs() > 0.05)));
        let first = a.iter().zip(&b).position(|(x, y)| x.to_bits() != y.to_bits());
        assert_eq!(first, None, "{:?}", first.map(|i| (i / 2, a[i], b[i])));
    }

    /// One sample on one playlist track twice, the copy starting half way through the first, with Project ▸ Automatic
    /// crossfades on, exported as FILE ▸ Export audio does it (one audio clip channel, played by Lemon Sampler): the
    /// level stays flat through the overlap and the second clip plays to its own end (spec: audio/audio-clips.md
    /// "Automatic crossfades").
    #[test]
    fn overlapping_clips_of_one_sample_export_flat() {
        use std::time::Duration;
        let mut a = App::new(None);
        a.plugins = crate::plugins::Plugins::default();
        a.plugins.available = crate::plugins::builtin();
        a.plugins.set_rate(48000);
        let wav = std::env::temp_dir().join(format!("os-app-crossfade-{}.wav", std::process::id()));
        os_engine::wav::write_f32_stereo(&mut std::fs::File::create(&wav).unwrap(), 48000, &vec![0.25; 2 * 96000]).unwrap();
        crate::playlist::add_audio_clip(&mut a.state, &wav.to_string_lossy(), 0, 0).expect("audio clip added");
        let row = a.state.project.channels.len() - 1;
        a.plugin_tick();
        assert!(a.state.hosted_rows.contains(&row), "Lemon Sampler hosts the audio clip channel");
        a.transport.set_instruments(a.plugins.instruments());
        let first = *crate::playlist::arr_mut(&mut a.state).clips.last().unwrap();
        let second = os_model::Clip { position: first.position + first.length / 2, ..first };
        crate::playlist::arr_mut(&mut a.state).clips.push(second);
        // On (a new project takes the switch from the preferences); the next tick sets the crossfades from the overlap.
        if !os_model::crossfade::on(&a.state.project) {
            a.toggle_auto_crossfades();
        }
        assert!(os_model::crossfade::derive(&mut a.state.project, a.state.tempo), "the overlap makes a crossfade");
        let p = crate::rack::play_project(&a.state);
        let out = std::env::temp_dir().join(format!("os-app-crossfade-{}-export.wav", std::process::id()));
        a.plugins.set_offline(true);
        a.transport.export(&p, PlayMode::Song, 48000, crate::export::Job::wav(&out));
        let t0 = std::time::Instant::now();
        loop {
            match a.transport.export_status() {
                crate::transport::Export::Done(r) => break assert!(r.is_ok(), "{r:?}"),
                _ if t0.elapsed() > Duration::from_secs(60) => panic!("export did not finish"),
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        let bytes = std::fs::read(&out).expect("file written");
        let _ = std::fs::remove_file(&out);
        let _ = std::fs::remove_file(&wav);
        let left: Vec<f32> = bytes[44..].chunks_exact(8).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        let at = |t: u32| os_engine::Clock::frame_of_tick(48000, p.ppq, f64::from(a.state.tempo), u64::from(t)) as usize;
        let level = left[at(first.length / 4)];
        assert!(level > 0.1, "the first clip sounds: {level}");
        // From the first clip's start to the second's end.
        let end = at(second.position + second.length);
        let (worst, f) = (0..end).map(|f| ((left[f] - level).abs(), f)).fold((0.0f32, 0), |m, x| if x.0 > m.0 { x } else { m });
        assert!(worst < 1e-3 * level, "flat: off by {worst} at frame {f} of {end} (level {level})");
    }

    /// ADD ▸ Lemon Sampler with "Open new plugin windows floating" on: the new channel's instance opens detached; channels added
    /// before and the project's own keep docked (spec: ui/settings-choices.md "Miscellaneous: naming, detached plugins
    /// and startup").
    #[test]
    fn added_sampler_channels_take_the_detach_check() {
        let mut a = App::new(None);
        a.plugins = crate::plugins::Plugins::default();
        a.plugins.available = crate::plugins::builtin();
        let sampler = a.plugins.index_of(crate::plugins::LEMON_SAMPLER).unwrap();
        crate::plugin_menu::add_generator(&mut a, sampler, None);
        let off = a.state.pending_open.expect("its window opens once hosted");
        a.sync_samplers(None);
        a.state.pending_open = None;
        crate::prefs::set_check(&crate::prefs::DETACH_NEW, true);
        crate::plugin_menu::add_generator(&mut a, sampler, None);
        let on = a.state.pending_open.unwrap();
        a.sync_samplers(None);
        let detached = |r: usize| a.plugins.hosted[&r].detached;
        assert!(detached(on));
        assert!(!detached(off), "added before: keeps its mode");
        assert!(!detached(0), "the project's channel");
    }

    /// Lemon Sampler's settings are saved with the project and come back when it is reopened (spec:
    /// plugins/lemon-sampler.md §7): envelopes and LFOs, the channel filter, declick, the sampler flags, the start
    /// offset, the precomputed settings and the root note page. Untouched settings keep the channel's own values.
    #[test]
    fn sampler_settings_save_and_reopen() {
        use serde_json::json;
        let flp = std::env::temp_dir().join(format!("os_sampler_settings_{}.flp", std::process::id()));
        let mut a = App::new(None);
        a.plugins = crate::plugins::Plugins::default();
        a.plugins.available = crate::plugins::builtin();
        a.sync_samplers(None);
        let row = (0..a.state.project.channels.len()).find(|&r| a.state.project.channels[r].kind.is_sampler()).expect("a sampler channel");
        assert!(a.state.hosted_rows.contains(&row));
        let untouched = a.state.project.channels[row].clone();
        let edits = [
            ("envvol_on", json!({ "bool": true })),
            ("envvol_att", json!({ "f32": 0.5 })),
            ("envvol_tempo", json!({ "bool": true })),
            ("envvol_lfo_amt", json!({ "f32": -0.5 })),
            ("envvol_lfo_shape", json!({ "i32": 2 })),
            ("envpitch_amt", json!({ "f32": 0.25 })),
            ("f_cut", json!({ "f32": 0.25 })),
            ("f_type", json!({ "i32": 3 })),
            ("declick", json!({ "i32": 2 })),
            ("loop_use", json!({ "bool": false })),
            ("fx_reverse", json!({ "bool": true })),
            ("fx_boost", json!({ "f32": 0.5 })),
            ("fx_start", json!({ "f32": 0.25 })),
            ("fx_sdelay", json!({ "f32": -0.5 })),
            ("envpan_lfo_global", json!({ "bool": true })),
            ("keep_disk", json!({ "bool": true })),
            ("resample", json!({ "bool": true })),
            ("start_offset", json!({ "f32": 0.125 })),
            ("root", json!({ "i32": 48 })),
            ("fine", json!({ "i32": -20 })),
            ("main_pitch", json!({ "bool": false })),
            ("add_key", json!({ "bool": true })),
            ("zone_lo", json!({ "i32": 37 })),
            ("zone_hi", json!({ "i32": 60 })),
        ];
        let st = crate::plugins::with_params(&a.plugins.state(row).unwrap(), &edits).unwrap();
        assert!(a.plugins.set_state(row, &st));

        // Saving copies them into the channel, in the file's units.
        a.save_to(flp.clone());
        assert!(a.state.message.as_deref().is_some_and(|m| m.starts_with("Saved")), "{:?}", a.state.message);
        let ch = &a.state.project.channels[row];
        let vol = &ch.envelopes[os_model::Envelope::VOLUME];
        assert!(vol.enabled && vol.attack == 32768 && vol.flags & 1 == 1 && vol.lfo_amount == -64 && vol.lfo_shape == 2);
        // The pitch record's amount is in cents (±2400).
        assert_eq!(ch.envelopes[os_model::Envelope::PITCH].amount, 600);
        assert_eq!(ch.envelopes[os_model::Envelope::PAN].flags & 0x20, 0x20, "global LFO");
        // FL's new channel's flags (Load regions, Use loop points), loop points off, Keep on disk and Resample on.
        assert_eq!((ch.sound.cutoff, ch.sound.filter_type, ch.sound.declick, ch.sound.sampler_flags), (64, 3, 2, Some(0x103)));
        assert_eq!(ch.sound.start_offset(), 0.125);
        let m = ch.misc;
        assert_eq!((m.root, m.fine, m.main_pitch, m.add_key, m.key_range), (48, -20, false, true, [37, 60]));
        let fx = ch.sound.fx;
        assert_eq!((fx.flags, fx.boost, fx.stereo_delay, fx.start()), (os_model::SampleFx::REVERSE, 128, 1024, 0.25));
        // Everything not edited is as it was.
        assert_eq!(ch.envelopes[os_model::Envelope::PAN].attack, untouched.envelopes[os_model::Envelope::PAN].attack);
        assert_eq!((fx.fade_in, fx.pogo, fx.length_bits), (untouched.sound.fx.fade_in, untouched.sound.fx.pogo, untouched.sound.fx.length_bits));
        assert_eq!(ch.sound.resonance, untouched.sound.resonance);

        // Reopened: the file holds them, and the new Lemon Sampler starts with them.
        let saved = ch.clone();
        let mut b = App::new(None);
        b.plugins = crate::plugins::Plugins::default();
        b.plugins.available = crate::plugins::builtin();
        b.open(flp.clone());
        let _ = std::fs::remove_file(&flp);
        let t0 = std::time::Instant::now();
        while !b.state.hosted_rows.contains(&row) && t0.elapsed() < std::time::Duration::from_secs(20) {
            b.plugin_tick();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let reopened = &b.state.project.channels[row];
        assert_eq!((reopened.envelopes, reopened.sound), (saved.envelopes, saved.sound));
        let (m, n) = (reopened.misc, saved.misc);
        assert_eq!((m.root, m.fine, m.main_pitch, m.add_key, m.key_range), (n.root, n.fine, n.main_pitch, n.add_key, n.key_range));
        let st = b.plugins.state(row).expect("Lemon Sampler hosted again");
        let v: serde_json::Value = serde_json::from_slice(&st[8..]).unwrap();
        for (id, want) in &edits {
            let got = &v["params"][*id];
            match (got.get("f32"), want.get("f32")) {
                (Some(g), Some(w)) => assert!((g.as_f64().unwrap() - w.as_f64().unwrap()).abs() < 1e-6, "{id}: {got} != {want}"),
                _ => assert_eq!(got, want, "{id}"),
            }
        }
    }

    /// A Lemon Sampler state read back into the channel it started from changes nothing: every value the file holds,
    /// even one a knob cannot show exactly, is kept while its knob is untouched.
    #[test]
    fn untouched_sampler_settings_store_as_they_were() {
        let mut pl = crate::plugins::Plugins::default();
        pl.available = crate::plugins::builtin();
        let index = pl.index_of(crate::plugins::LEMON_SAMPLER).unwrap();
        let mut ch = test_project().channels.remove(0);
        let vol = &mut ch.envelopes[os_model::Envelope::VOLUME];
        (vol.enabled, vol.attack, vol.sustain, vol.lfo_amount, vol.lfo_shape, vol.flags) = (true, 50, 77, -3, 1, 0x20 | 4 | 2);
        (ch.envelopes[os_model::Envelope::PITCH].amount, ch.envelopes[os_model::Envelope::PITCH].lfo_amount) = (1201, -7);
        (ch.sound.cutoff, ch.sound.resonance, ch.sound.filter_type, ch.sound.declick, ch.sound.sampler_flags) = (300, 64, 5, 6, Some(0x10b));
        (ch.sound.start_offset_bits, ch.sound.loop_points) = (0.3f64.to_bits(), Some([12, 34]));
        (ch.misc.root, ch.misc.fine, ch.misc.main_pitch, ch.misc.add_key, ch.misc.key_range) = (48, -35, false, true, [37, 300]);
        ch.sound.fx = os_model::SampleFx {
            flags: os_model::SampleFx::SWAP,
            boost: 77,
            trim: 32768,
            start_bits: 0.1f64.to_bits(),
            crossfade: 3,
            ..Default::default()
        };
        pl.create_with(0, index, Some(&crate::plugins::sampler_state(&ch))).unwrap();
        let mut back = ch.clone();
        crate::plugins::store_sampler(&mut back, &pl.state(0).unwrap());
        assert_eq!(back, ch);
    }

    /// Loop points are frames of the sample file and Lemon Sampler's loop knobs fractions of its last frame's index
    /// (2000 for this 2001-frame file; spec: plugins/lemon-sampler.md §7): FL's values for the sample's ends show the
    /// whole sample, stored points come back while the knobs are untouched, and a moved knob stores its frame.
    #[test]
    fn sampler_loop_points_are_frames_of_the_file() {
        use serde_json::json;
        let wav = std::env::temp_dir().join(format!("os_loop_points_{}.wav", std::process::id()));
        os_engine::wav::write_f32_stereo(&mut std::fs::File::create(&wav).unwrap(), 44100, &[0.25; 4002]).unwrap();
        let mut pl = crate::plugins::Plugins::default();
        pl.available = crate::plugins::builtin();
        let index = pl.index_of(crate::plugins::LEMON_SAMPLER).unwrap();
        let mut ch = test_project().channels.remove(0);
        ch.sample_path = Some(wav.to_string_lossy().into_owned());
        let knobs = |pl: &crate::plugins::Plugins| {
            let v: serde_json::Value = serde_json::from_slice(&pl.state(0).unwrap()[8..]).unwrap();
            (v["params"]["loop_start"]["f32"].as_f64().unwrap(), v["params"]["loop_end"]["f32"].as_f64().unwrap())
        };
        for (points, want) in [([-1, i64::MAX], (0.0, 1.0)), ([500, 1500], (0.25, 0.75))] {
            ch.sound.loop_points = Some(points);
            pl.create_with(0, index, Some(&crate::plugins::sampler_state(&ch))).unwrap();
            assert_eq!(knobs(&pl), want);
            let mut back = ch.clone();
            crate::plugins::store_sampler(&mut back, &pl.state(0).unwrap());
            assert_eq!(back.sound.loop_points, Some(points), "untouched");
        }
        let st = crate::plugins::with_params(&pl.state(0).unwrap(), &[("loop_start", json!({ "f32": 0.5 }))]).unwrap();
        crate::plugins::store_sampler(&mut ch, &st);
        assert_eq!(ch.sound.loop_points, Some([1000, 1500]));
        // A channel without loop points gets them once a knob moves.
        ch.sound.loop_points = None;
        pl.create_with(0, index, Some(&crate::plugins::sampler_state(&ch))).unwrap();
        let st = crate::plugins::with_params(&pl.state(0).unwrap(), &[("loop_end", json!({ "f32": 0.5 }))]).unwrap();
        crate::plugins::store_sampler(&mut ch, &st);
        let _ = std::fs::remove_file(&wav);
        assert_eq!(ch.sound.loop_points, Some([0, 1000]));
    }

    /// An opened project decodes its samples on worker threads, then starts its samplers behind the progress panel;
    /// loading ends with every sampler channel hosted on the shared decode (spec: ui/project-loading.md).
    #[test]
    fn opened_projects_load_their_samplers_first() {
        let flp = std::env::temp_dir().join(format!("os_loading_{}.flp", std::process::id()));
        let wav = std::env::temp_dir().join(format!("os_loading_{}.wav", std::process::id()));
        os_engine::wav::write_f32_stereo(&mut std::fs::File::create(&wav).unwrap(), 48000, &[0.25; 96000]).unwrap();
        let wav = wav.to_string_lossy().into_owned();
        let mut app = App::new(None);
        for c in app.state.project.channels.iter_mut().filter(|c| c.kind.is_sampler()) {
            c.sample_path = Some(wav.clone());
        }
        let rows: Vec<usize> = (0..app.state.project.channels.len()).filter(|&r| app.state.project.channels[r].kind.is_sampler()).collect();
        assert!(rows.len() > 1);
        app.save_to(flp.clone());
        app.plugins = crate::plugins::Plugins::default();
        app.plugins.available = crate::plugins::builtin();
        app.open(flp.clone());
        let _ = std::fs::remove_file(&flp);
        assert!(app.loading(), "opening starts loading");
        assert!(app.animating(), "the panel keeps ticking");
        let mut canvas = Canvas::new(1920, 1080, 1.0).unwrap();
        app.paint(&mut canvas, None);
        let t0 = std::time::Instant::now();
        while app.loading() && t0.elapsed() < std::time::Duration::from_secs(20) {
            app.plugin_tick();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!app.loading(), "loading ends");
        for &r in &rows {
            assert!(app.state.hosted_rows.contains(&r), "row {r} has its Lemon Sampler");
            assert_eq!(app.plugins.sampler_path(r).as_deref(), Some(wav.as_str()));
        }
        let _ = std::fs::remove_file(&wav);
    }

    /// The Misc tab's values are saved with the channel and come back on open (spec: audio/channel-misc.md
    /// "Settings", audio/voice-levels.md, audio/swing.md, audio/note-rules.md), tracking, the arpeggiator and the echo
    /// delay included: the first save adds their events, a later one patches them. (Full portamento is not saved yet:
    /// OpenStudio-specs REQUESTS.md #5.)
    #[test]
    fn channel_misc_save_and_reopen() {
        use crate::misc_page::M;
        let flp = std::env::temp_dir().join(format!("os_channel_misc_{}.flp", std::process::id()));
        let reopen = |a: &mut App| {
            a.save_to(flp.clone());
            assert!(a.state.message.as_deref().is_some_and(|m| m.starts_with("Saved")), "{:?}", a.state.message);
            let mut b = App::new(None);
            b.plugins = crate::plugins::Plugins::default();
            b.open(flp.clone());
            b
        };
        let mut a = App::new(None);
        a.plugins = crate::plugins::Plugins::default();
        let row = 0;
        let untouched = a.state.project.channels[row].misc;
        // Values the file's units hold exactly.
        let first = [
            (M::LevelPan, 0.25),
            (M::LevelVol, 0.75),
            (M::LevelModX, 0.625),
            (M::LevelModY, 0.375),
            (M::PolyMax, 3.0),
            (M::PolySlide, 820.0 / 1024.0),
            (M::Porta, 1.0),
            (M::Mono, 1.0),
            (M::TimeGate, 499.0 / 997.0),
            (M::TimeShift, 0.25),
            (M::TimeSwing, 0.5),
            (M::Cut, 2.0),
            (M::CutBy, 3.0),
            (M::Root, 48.0),
            (M::Fine, -35.0),
            (M::MainPitch, 0.0),
            (M::AddKey, 1.0),
            (M::VolMid, 80.0),
            (M::VolPan, 0.75),
            (M::VolModX, 0.25),
            (M::VolModY, 1.0),
            (M::KeyMid, 72.0),
            (M::KeyPan, 0.0),
            (M::KeyModY, 0.625),
            (M::ArpDir, 4.0),
            (M::ArpTime, 600.0 / 997.0),
            (M::ArpGate, 0.625),
            (M::ArpRange, 0.0),
            (M::ArpRepeat, 3.0),
            (M::ArpSlide, 1.0),
            (M::ArpChord, 7.0),
            (M::EchoFeed, 0.375),
            (M::EchoPan, 0.625),
            (M::EchoModX, 0.25),
            (M::EchoModY, 0.625),
            (M::EchoPitch, 0.625),
            (M::EchoTime, 0.125),
            (M::Echoes, 7.0),
            (M::PingPong, 1.0),
            (M::FatMode, 1.0),
        ];
        for (m, v) in first {
            crate::misc::set(&mut a, row, m, v);
        }
        let want = a.state.project.channels[row].misc;
        assert_ne!(want, untouched);
        let mut b = reopen(&mut a);
        assert_eq!(b.state.project.channels[row].misc, want);

        let second = [
            (M::LevelPan, 0.5),
            (M::LevelVol, 0.25),
            (M::PolyMax, 0.0),
            (M::Mono, 0.0),
            (M::TimeGate, 1.0),
            (M::TimeShift, 0.5),
            (M::TimeSwing, 1.0),
            (M::CutBy, 0.0),
            (M::Root, 72.0),
            (M::Fine, 20.0),
            (M::MainPitch, 1.0),
            (M::VolMid, 100.0),
            (M::KeyPan, 0.625),
            (M::ArpDir, 3.0),
            (M::ArpChord, -1.0),
            (M::ArpRepeat, 1.0),
            (M::EchoFeed, 0.5),
            (M::EchoModY, 0.5),
            (M::FatMode, 0.0),
        ];
        for (m, v) in second {
            crate::misc::set(&mut b, row, m, v);
        }
        let want = b.state.project.channels[row].misc;
        let c = reopen(&mut b);
        let _ = std::fs::remove_file(&flp);
        assert_eq!(c.state.project.channels[row].misc, want);
    }

    /// Opening, loading and saving leave no unsaved changes; an edit does; "Save new version" numbers the file.
    #[test]
    fn unsaved_changes_and_new_versions() {
        let dir = std::env::temp_dir().join(format!("os_versions_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = App::new(None);
        assert!(!app.unsaved(), "a fresh project");
        app.save_to(dir.join("Song.lemon"));
        app.plugins = crate::plugins::Plugins::default();
        app.plugins.available = crate::plugins::builtin();
        app.open(dir.join("Song.lemon"));
        let mut canvas = Canvas::new(1920, 1080, 1.0).unwrap();
        let t0 = std::time::Instant::now();
        while app.loading() && t0.elapsed() < std::time::Duration::from_secs(20) {
            app.plugin_tick();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        for _ in 0..5 {
            app.tick();
            app.paint(&mut canvas, None);
        }
        assert!(!app.unsaved(), "opening and loading change nothing");
        app.state.tempo = 99.0;
        assert!(app.unsaved(), "an edit is unsaved");
        app.act(Action::MenuItem(0, views::menus::FILE_SAVE));
        assert!(!app.unsaved(), "saved");
        app.act(Action::MenuItem(0, views::menus::FILE_SAVE_VERSION));
        assert_eq!(app.state.path.as_deref(), Some(dir.join("Song_2.lemon").as_path()));
        app.act(Action::MenuItem(0, views::menus::FILE_SAVE_VERSION));
        assert_eq!(app.state.path.as_deref(), Some(dir.join("Song_3.lemon").as_path()));
        assert!(dir.join("Song_2.lemon").exists() && dir.join("Song_3.lemon").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Projects save as .lemon files that open again; one opened from an FLP is never saved over: Save asks for a name,
    /// offering the FLP's with .lemon, and the project then lives there (spec: flp/lemon-format.md).
    #[test]
    fn flps_migrate_to_lemon_files() {
        use crate::dialog::fake;
        let dir = std::env::temp_dir().join(format!("os_lemon_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = App::new(None);
        app.state.tempo = 123.0;
        let flp = dir.join("Old.flp");
        app.save_to(flp.clone());
        let plain = os_flp::FlpFile::parse(&std::fs::read(&flp).unwrap()).unwrap();
        assert!(!os_flp::lemon::is_lemon(&plain), "a target named .flp is still written as an FLP");
        let before = std::fs::read(&flp).unwrap();
        let mut app = App::new(None);
        app.open(flp.clone());
        assert_eq!(app.state.tempo, 123.0);
        app.state.tempo = 124.0;
        let lemon = dir.join("Old.lemon");
        let asked = fake::shown().1;
        fake::answer(None);
        app.act(Action::MenuItem(0, views::menus::FILE_SAVE));
        assert_eq!(fake::shown().1, asked + 1, "Save on an FLP asks for a name");
        assert!(!lemon.exists(), "cancelled: nothing written");
        assert_eq!(std::fs::read(&flp).unwrap(), before, "the FLP is not written");
        assert!(app.unsaved());
        fake::answer(Some(lemon.clone()));
        app.act(Action::MenuItem(0, views::menus::FILE_SAVE));
        assert_eq!(app.state.path.as_deref(), Some(lemon.as_path()), "the project now lives in the .lemon file");
        assert!(!app.unsaved());
        assert_eq!(std::fs::read(&flp).unwrap(), before, "the FLP is still not written");
        let file = os_flp::FlpFile::parse(&std::fs::read(&lemon).unwrap()).unwrap();
        assert_eq!(os_flp::lemon::version(&file), Some(os_flp::lemon::VERSION));
        assert_eq!(os_flp::lemon::saved_by_text(&file), Some(format!("Lemon Studio {}", env!("CARGO_PKG_VERSION"))));
        // Save again: in place, no dialog.
        app.state.tempo = 125.0;
        app.act(Action::MenuItem(0, views::menus::FILE_SAVE));
        assert_eq!(fake::shown().1, asked + 2);
        let mut back = App::new(None);
        back.open(lemon.clone());
        assert_eq!((back.state.tempo, back.state.path.as_deref()), (125.0, Some(lemon.as_path())));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Shift+Ctrl+S is Save as: it asks where and leaves the open file as it was. Ctrl+S saves in place without asking.
    /// Shift+Ctrl+O / R / N have no command: they neither open, export nor write a new version.
    #[test]
    fn shift_ctrl_s_is_save_as() {
        use crate::dialog::fake;
        use winit::keyboard::Key;
        let dir = std::env::temp_dir().join(format!("os_save_as_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = App::new(None);
        app.state.project = test_project();
        // Something to export, so Ctrl+R gets as far as its file dialog.
        app.state.project.patterns[0].notes.push(Note::step(0, 0));
        let song = dir.join("Song.lemon");
        app.save_to(song.clone());
        let before = std::fs::read(&song).unwrap();
        let key = |app: &mut App, k: &str, shift: bool| {
            app.shift = shift;
            app.key(&Key::Character(k.into()), true, false);
            app.shift = false;
        };

        app.state.tempo = 99.0;
        key(&mut app, "S", true);
        assert_eq!(fake::shown(), (0, 1), "Save as asks where");
        assert_eq!(std::fs::read(&song).unwrap(), before, "cancelled: the open file is not written");
        assert!(app.unsaved());
        let other = dir.join("Other.lemon");
        fake::answer(Some(other.clone()));
        key(&mut app, "S", true);
        assert_eq!(app.state.path.as_deref(), Some(other.as_path()));
        assert!(other.exists() && !app.unsaved(), "saved to the chosen file");
        assert_eq!(std::fs::read(&song).unwrap(), before, "the old file is left as it was");

        let export_form = |app: &App| {
            matches!(app.state.dialogs.last().map(|d| &d.body), Some(crate::dialog::Body::Project(crate::project_forms::Form::Export(_))))
        };
        let escape = |app: &mut App| app.key(&Key::Named(winit::keyboard::NamedKey::Escape), false, false);
        for k in ["O", "N"] {
            key(&mut app, k, true);
        }
        key(&mut app, "R", true);
        assert!(export_form(&app), "Shift+Ctrl+R opens the Export form (MP3)");
        escape(&mut app);
        assert_eq!(fake::shown(), (0, 2), "no open or file dialog");
        assert!(app.export.is_none());
        assert!(!dir.join("Other_2.lemon").exists(), "no new version");

        app.state.tempo = 100.0;
        key(&mut app, "s", false);
        assert_eq!(fake::shown(), (0, 2), "Ctrl+S does not ask");
        assert!(!app.unsaved(), "Ctrl+S saved in place");
        key(&mut app, "n", false);
        assert!(dir.join("Other_2.lemon").exists(), "Ctrl+N still saves a new version");
        key(&mut app, "o", false);
        assert_eq!(fake::shown(), (1, 2), "Ctrl+O still asks for a file");
        key(&mut app, "r", false);
        assert!(export_form(&app), "Ctrl+R opens the Export form");
        escape(&mut app);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An effect dragged from the plugin database onto the mixer goes into the current track's first free slot
    /// (spec: ui/browser.md §3).
    #[test]
    fn browser_effect_drops_into_a_mixer_slot() {
        use crate::views::browser::{rows, Item};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.plugins.effects = vec![crate::plugin_db::fake("Test FX", os_host::Kind::Effect)];
        app.state.plugdb.rebuild(&app.plugins);
        app.state.z = vec![Win::Mixer];
        app.state.browser_tab = 2;
        app.state.browser_open.extend(["2:Plugin library/Effects".into(), "2:Plugin library/Effects/Other".into()]);
        frame(&mut app, &mut c, None);
        let i = rows(&app.state).iter().position(|r| r.item == Item::Effect(0)).expect("effect row");
        let src = hit_rect(&app, &Action::BrowserRow(i));
        let dst = hit_rect(&app, &Action::MixerArea);
        app.mouse_move(src.cx(), src.cy());
        app.mouse_down();
        app.mouse_move(src.cx() + 30.0, src.cy());
        app.mouse_move(dst.cx(), dst.cy());
        app.mouse_up();
        let t = app.state.selected_mixer;
        assert_eq!(crate::mixer::slot_name(&app.state, t, 0).as_deref(), Some("Test FX"));
    }

    /// The wheel over a mixer effect slot moves the effect a slot a notch, up for wheel up, trading places with its
    /// neighbour (spec: ui/mixer.md §7). With the mouse still, more notches move the same effect, which has left the
    /// cursor; the first and last slots are the ends.
    #[test]
    fn wheel_moves_a_mixer_effect() {
        use crate::mixer::{run, slot_name, MCmd, MHit};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.plugins.effects = vec![crate::plugin_db::fake("FX A", os_host::Kind::Effect), crate::plugin_db::fake("FX B", os_host::Kind::Effect)];
        app.state.z = vec![Win::Mixer];
        run(&mut app, MCmd::AddEffect(0, 0));
        run(&mut app, MCmd::AddEffect(3, 1));
        frame(&mut app, &mut c, None);
        let t = app.state.selected_mixer;
        let slots = |app: &App| (0..10).map(|k| slot_name(&app.state, t, k).unwrap_or_default()).collect::<Vec<_>>();
        let at = |app: &App, k: u8| hit_rect(app, &Action::Mix(MHit::SlotName(k)));
        let r = at(&app, 0);
        app.mouse_move(r.cx(), r.cy());
        app.wheel(0.0, -1.0, false, false);
        assert_eq!(slots(&app)[..4], ["", "FX A", "", "FX B"], "down a slot");
        app.wheel(0.0, -1.0, false, false);
        app.wheel(0.0, -1.0, false, false);
        assert_eq!(slots(&app)[..4], ["", "", "FX B", "FX A"], "the same effect, past the other one");
        frame(&mut app, &mut c, None);
        let r = at(&app, 2);
        app.mouse_move(r.cx(), r.cy());
        app.wheel(0.0, 3.0, false, false);
        assert_eq!(slots(&app)[..4], ["FX B", "", "", "FX A"], "up, and no further than the first slot");
    }

    /// The piano roll's scroll-bar arrows step their axis (spec: ui/piano-roll.md §1a).
    #[test]
    fn piano_scroll_arrows_step() {
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.z = vec![Win::PianoRoll];
        frame(&mut app, &mut c, None);
        for t in [Scroll::PianoY, Scroll::PianoX] {
            let v0 = app.scroll_get(t);
            let r = hit_rect(&app, &Action::ScrollStep(t, 1));
            click(&mut app, &mut c, r);
            // The step eases (General ▸ Display ▸ Smoothing).
            assert!(app.smooth.running(), "{t:?} eases");
            app.ease_to_end();
            assert!(app.scroll_get(t) > v0, "{t:?} steps forward");
            let r = hit_rect(&app, &Action::ScrollStep(t, -1));
            click(&mut app, &mut c, r);
            app.ease_to_end();
            assert!((app.scroll_get(t) - v0).abs() < 0.01, "{t:?} steps back: {v0} -> {}", app.scroll_get(t));
        }
    }

    #[test]
    fn browser_dropped_folder() {
        use crate::views::browser::{rows, Item};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let dir = std::env::temp_dir().join(format!("os_browser_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Kicks")).unwrap();
        std::fs::write(dir.join("Kicks").join("Kick 01.wav"), b"x").unwrap();
        std::fs::write(dir.join("readme.txt"), b"hello").unwrap();
        std::fs::write(dir.join(".hidden"), b"").unwrap();
        let mut app = App::new(None);
        frame(&mut app, &mut c, None);
        app.drop_file(dir.clone());
        let r = rows(&app.state);
        let i = r.iter().position(|r| r.path.as_deref() == Some(dir.as_path())).expect("root row");
        assert_eq!(r[i].open, Some(true));
        // Folders first, then files; hidden files left out.
        assert_eq!(r[i + 1].label, "Kicks");
        assert_eq!(r[i + 2].label, "readme.txt");
        assert!(matches!(r[i + 2].item, Item::File(_)));
        assert_eq!(r.len(), i + 3, "the dropped folder is the last root");
        // Open the subfolder: the sample shows without extension and drags as a sample.
        frame(&mut app, &mut c, None);
        let sub = hit_rect(&app, &Action::BrowserRow(i + 1));
        click(&mut app, &mut c, sub);
        let r = rows(&app.state);
        assert_eq!(r[i + 2].label, "Kick 01");
        assert!(matches!(r[i + 2].item, Item::Sample(_)));
        assert_eq!(r[i + 2].depth, 2);
        // The root's right-click menu removes it (§8).
        frame(&mut app, &mut c, None);
        let root = hit_rect(&app, &Action::BrowserRow(i));
        app.mouse_move(root.cx(), root.cy());
        app.mouse_down_right();
        pick(&mut app, &mut c, "Remove from browser");
        assert!(app.state.browser_folders.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The arrows over the browser walk the tree (issue #169): Up / Down a row, Right opens a node and steps into it,
    /// Left closes it and goes to the parent. Held, Up / Down repeat; with a modifier or away from the browser the
    /// arrows are not the browser's.
    #[test]
    fn browser_arrow_keys_walk_the_tree() {
        use crate::views::browser::{rows, Item};
        use winit::keyboard::{Key, NamedKey};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let dir = std::env::temp_dir().join(format!("os_browser_arrows_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Kicks")).unwrap();
        std::fs::write(dir.join("Kicks").join("Kick 01.wav"), b"x").unwrap();
        std::fs::write(dir.join("Kicks").join("Kick 02.wav"), b"x").unwrap();
        std::fs::write(dir.join("readme.txt"), b"hello").unwrap();
        let mut app = App::new(None);
        frame(&mut app, &mut c, None);
        app.drop_file(dir.clone());
        frame(&mut app, &mut c, None);
        let i = rows(&app.state).iter().position(|r| r.path.as_deref() == Some(dir.as_path())).expect("root row");
        let over = hit_rect(&app, &Action::BrowserRow(0));
        app.mouse_move(over.cx(), over.cy());
        assert!(app.over_browser());
        let arrow = |app: &mut App, k: NamedKey| app.key(&Key::Named(k), false, false);
        let sel = |app: &App| {
            let r = rows(&app.state);
            let k = r.iter().position(|r| Some(&r.key) == app.state.browser_sel.as_ref()).expect("a row selected");
            (k, r[k].label.clone())
        };
        // Nothing selected: Up selects the last row.
        app.state.browser_sel = None;
        arrow(&mut app, NamedKey::ArrowUp);
        assert_eq!(sel(&app), (i + 2, "readme.txt".to_string()));
        arrow(&mut app, NamedKey::ArrowUp);
        assert_eq!(sel(&app), (i + 1, "Kicks".to_string()));
        // Right opens the closed folder (the selection stays), then steps onto its first row.
        arrow(&mut app, NamedKey::ArrowRight);
        assert_eq!(rows(&app.state)[i + 1].open, Some(true));
        assert_eq!(sel(&app).0, i + 1);
        arrow(&mut app, NamedKey::ArrowRight);
        assert_eq!(sel(&app), (i + 2, "Kick 01".to_string()));
        assert!(matches!(rows(&app.state)[i + 2].item, Item::Sample(_)));
        arrow(&mut app, NamedKey::ArrowDown);
        assert_eq!(sel(&app), (i + 3, "Kick 02".to_string()));
        // Held, Down repeats.
        app.repeat = true;
        arrow(&mut app, NamedKey::ArrowDown);
        app.repeat = false;
        assert_eq!(sel(&app), (i + 4, "readme.txt".to_string()));
        // The last row stays selected.
        arrow(&mut app, NamedKey::ArrowDown);
        assert_eq!(sel(&app).0, i + 4);
        // Left on a file goes to its parent; on an open node it closes it.
        arrow(&mut app, NamedKey::ArrowLeft);
        assert_eq!(sel(&app).0, i);
        arrow(&mut app, NamedKey::ArrowLeft);
        assert_eq!(rows(&app.state).len(), i + 1, "the root closed");
        assert_eq!(sel(&app).0, i);
        // Opened again, Kicks is still open.
        arrow(&mut app, NamedKey::ArrowRight);
        assert_eq!(rows(&app.state)[i + 1].open, Some(true));
        // A modifier keeps the arrows' other meanings: Ctrl+Up collapses the structure.
        app.key(&Key::Named(NamedKey::ArrowUp), true, false);
        assert!(app.state.browser_open.iter().all(|k| !k.starts_with(&format!("{}:", app.state.browser_tab))));
        // Away from the browser the arrows leave it alone.
        let before = app.state.browser_sel.clone();
        app.mouse_move(1500.0, 600.0);
        assert!(!app.over_browser());
        arrow(&mut app, NamedKey::ArrowUp);
        assert_eq!(app.state.browser_sel, before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Clicks entry `label` of the open popup.
    fn pick(app: &mut App, c: &mut Canvas, label: &str) {
        frame(app, c, None);
        let items = &app.state.playlist.popup.as_ref().expect("a menu").items;
        let k = items.iter().position(|i| i.label == label).unwrap_or_else(|| panic!("no \"{label}\" in the menu"));
        let r = hit_rect(app, &Action::PlPopup(k));
        click(app, c, r);
    }

    /// A right click on a browser row selects it and opens its menu (spec: ui/browser.md §8): Open / Close for a node,
    /// what the row's kind offers, the file manager for disk entries, the structure commands last.
    #[test]
    fn browser_row_menu() {
        use crate::views::browser::{disk_key, rows, Item, Row};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let dir = std::env::temp_dir().join(format!("os_browser_menu_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        os_engine::wav::write_f32_stereo(&mut std::fs::File::create(dir.join("Snare 01.wav")).unwrap(), 44100, &[0.0; 8]).unwrap();
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.project.patterns.push(os_model::Pattern { number: 2, name: Some("Verse".into()), ..Default::default() });
        app.state.z = vec![Win::Playlist];
        app.state.browser_folders.push(dir.clone());
        app.state.browser_open.insert(disk_key(&dir));
        /// Right-clicks the first row `find` accepts: the row is selected; the menu's labels.
        fn right(app: &mut App, c: &mut Canvas, find: impl Fn(&Row) -> bool) -> Vec<String> {
            frame(app, c, None);
            let i = rows(&app.state).iter().position(find).expect("the row");
            let r = hit_rect(app, &Action::BrowserRow(i));
            app.mouse_move(r.cx(), r.cy());
            app.mouse_down_right();
            assert_eq!(app.state.browser_sel.as_ref(), Some(&rows(&app.state)[i].key), "the right click selects the row");
            app.state.playlist.popup.as_ref().expect("a menu").items.iter().map(|i| i.label.to_string()).collect()
        }
        // A sample in a dropped folder: preview, send, a new channel, its folder; the structure commands last.
        let labels = right(&mut app, &mut c, |r| r.label == "Snare 01");
        assert_eq!(labels[..3], ["Preview", "Send to selected channel", "Add to instrument rack"]);
        assert!(labels.iter().any(|l| l.starts_with("Show in ")));
        assert_eq!(labels[labels.len() - 2..], ["Collapse structure", "Reread structure"]);
        let n = app.state.project.channels.len();
        pick(&mut app, &mut c, "Add to instrument rack");
        assert_eq!(app.state.project.channels.len(), n + 1);
        assert_eq!(app.state.project.channels[app.state.selected_channel].name.as_deref(), Some("Snare 01"));
        // The dropped root: Close / Open, then Remove from browser.
        let labels = right(&mut app, &mut c, |r| r.key == disk_key(&dir));
        assert_eq!(labels[0], "Close");
        assert!(labels.iter().any(|l| l == "Rename...") && labels.iter().any(|l| l.starts_with("Open in ")));
        pick(&mut app, &mut c, "Close");
        assert!(!app.state.browser_open.contains(&disk_key(&dir)));
        assert_eq!(right(&mut app, &mut c, |r| r.key == disk_key(&dir))[0], "Open");
        pick(&mut app, &mut c, "Remove from browser");
        assert!(app.state.browser_folders.is_empty());
        // A pattern: its commands act on that pattern.
        app.state.browser_tab = crate::views::browser::PROJECT_TAB;
        let labels = right(&mut app, &mut c, |r| r.item == Item::Pattern(2));
        assert!(labels.iter().any(|l| l == "Rename and colour..."));
        pick(&mut app, &mut c, "Select pattern");
        assert_eq!(app.state.pattern, 2);
        app.state.pattern = 1;
        let n = app.state.project.patterns.len();
        right(&mut app, &mut c, |r| r.item == Item::Pattern(2));
        pick(&mut app, &mut c, "Clone");
        assert_eq!(app.state.project.patterns.len(), n + 1);
        let copy = app.state.project.pattern(3).and_then(|p| p.name.clone()).unwrap_or_default();
        assert!(app.state.pattern == 3 && copy.starts_with("Verse"), "pattern 2 cloned, not the current one: {copy}");
        // A channel under Generators: selected from its menu.
        app.state.browser_open.insert(crate::views::browser::section_key("Generators").unwrap());
        right(&mut app, &mut c, |r| r.item == Item::Channel(1));
        pick(&mut app, &mut c, "Select channel");
        assert_eq!(app.state.selected_channel, 1);
        // Collapse structure closes every node of the tab.
        right(&mut app, &mut c, |r| r.item == Item::Channel(1));
        pick(&mut app, &mut c, "Collapse structure");
        assert!(!app.state.browser_open.iter().any(|k| k.starts_with("1:")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The browser's splitter (spec: ui/browser.md §9): a drag widens the browser and the workspace starts right of it,
    /// from the default width to half the window; a double click gives the default width back.
    #[test]
    fn browser_splitter_resizes_the_browser() {
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        frame(&mut app, &mut c, None);
        let split = hit_rect(&app, &Action::BrowserSplitter);
        assert_eq!((split.x, split.right()), (201.0, 209.0), "the measured splitter");
        let drag = |app: &mut App, c: &mut Canvas, dx: f32| {
            let s = hit_rect(app, &Action::BrowserSplitter);
            app.mouse_move(s.cx(), s.cy());
            assert_eq!(app.cursor(), Cursor::EwResize);
            app.mouse_down();
            app.mouse_move(s.cx() + dx, s.cy());
            app.mouse_up();
            frame(app, c, None);
        };
        drag(&mut app, &mut c, 150.0);
        assert_eq!(app.state.browser_w, 359.0);
        assert_eq!(app.state.work_rect().x, 359.0);
        assert_eq!(hit_rect(&app, &Action::BrowserSplitter).x, 351.0);
        assert_eq!(hit_rect(&app, &Action::BrowserRow(0)).right(), 351.0, "rows span the wider list");
        drag(&mut app, &mut c, -500.0);
        assert_eq!(app.state.browser_w, 209.0, "not narrower than the default");
        drag(&mut app, &mut c, 2000.0);
        assert_eq!(app.state.browser_w, 960.0, "at most half the window");
        let s = hit_rect(&app, &Action::BrowserSplitter);
        app.mouse_move(s.cx(), s.cy());
        for _ in 0..2 {
            app.mouse_down();
            app.mouse_up();
        }
        assert_eq!(app.state.browser_w, 209.0, "a double click: the default width");
    }

    /// A hovered row whose label does not fit scrolls it (spec: ui/browser.md §9): still at first, then moving while the
    /// mouse stays; a label that fits does not.
    #[test]
    fn browser_long_labels_scroll_while_hovered() {
        use crate::views::browser::{disk_key, rows};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let dir = std::env::temp_dir().join(format!("os_browser_marquee_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("A very long file name that does not fit in the browser.txt"), b"x").unwrap();
        std::fs::write(dir.join("Short.txt"), b"x").unwrap();
        let mut app = App::new(None);
        app.state.browser_folders.push(dir.clone());
        app.state.browser_open.insert(disk_key(&dir));
        frame(&mut app, &mut c, None);
        let at = |app: &App, label: &str| {
            let i = rows(&app.state).iter().position(|r| r.label.starts_with(label)).expect("the row");
            hit_rect(app, &Action::BrowserRow(i))
        };
        let long = at(&app, "A very long");
        app.mouse_move(long.cx(), long.cy());
        frame(&mut app, &mut c, None);
        let m = app.state.browser_marquee.borrow().clone().expect("scrolling");
        assert!(m.over > 0.0 && m.off == 0.0, "still at first: {m:?}");
        assert!(app.animating());
        // Two seconds on, its rect is repainted and the label has moved.
        app.state.browser_marquee.get_mut().as_mut().unwrap().since -= std::time::Duration::from_secs(2);
        app.take_damage();
        app.marquee_tick(std::time::Instant::now());
        assert!(app.has_damage());
        frame(&mut app, &mut c, None);
        assert!(app.state.browser_marquee.borrow().as_ref().unwrap().off > 0.0);
        let short = at(&app, "Short");
        app.mouse_move(short.cx(), short.cy());
        frame(&mut app, &mut c, None);
        assert!(app.state.browser_marquee.borrow().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn browser_menu_and_search() {
        use crate::views::browser::{rows, Item};
        use winit::keyboard::{Key, NamedKey, SmolStr};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.project.patterns.push(os_model::Pattern { number: 2, name: Some("Kick + Clap".into()), ..Default::default() });
        app.state.z = vec![Win::Playlist];
        frame(&mut app, &mut c, None);
        // The ▸ opens the menu; its items follow the screenshot.
        let r0 = hit_rect(&app, &Action::BrowserMenu);
        click(&mut app, &mut c, r0);
        let labels: Vec<String> = app.state.playlist.popup.as_ref().expect("menu").items.iter().map(|i| i.label.to_string()).collect();
        assert_eq!(labels[0], "Search");
        assert!(labels.iter().any(|l| l == "Rescan plugins (fast)"));
        // Toggle "Show parent folders in search results".
        let i = labels.iter().position(|l| l == "Show parent folders in search results").unwrap();
        let r0 = hit_rect(&app, &Action::PlPopup(i));
        click(&mut app, &mut c, r0);
        assert!(app.state.playlist.popup.is_none());
        assert!(crate::views::browser::opt(&app.state, "parents"));
        // Click the search box and type: the whole tree is searched, closed nodes included.
        let r0 = hit_rect(&app, &Action::BrowserSearch);
        click(&mut app, &mut c, r0);
        assert!(app.state.browser_search_focus);
        for ch in ["k", "i", "c", "k"] {
            app.key(&Key::Character(SmolStr::new(ch)), false, false);
        }
        assert_eq!(app.state.browser_search, "kick");
        let r = rows(&app.state);
        // Parents kept: Current project > Patterns > Kick + Clap, and Generators > Kick.
        assert_eq!(r[0].label, "Current project");
        assert!(r.iter().any(|r| r.item == Item::Pattern(2)));
        assert!(r.iter().any(|r| r.label == "Kick" && r.depth == 2));
        // Find next selects the first match, then the second.
        app.key(&Key::Named(NamedKey::F3), false, false);
        let first = app.state.browser_sel.clone().unwrap();
        app.key(&Key::Named(NamedKey::F3), false, false);
        let second = app.state.browser_sel.clone().unwrap();
        assert_ne!(second, first);
        app.key(&Key::Named(NamedKey::F2), false, false);
        assert_eq!(app.state.browser_sel.clone().unwrap(), first);
        // Parents off: matches listed flat.
        app.state.browser_opts.remove("parents");
        assert!(rows(&app.state).iter().all(|r| r.depth == 0 && r.label.to_lowercase().contains("kick")));
        // Navigation history: back = the second match (first, second, first), forward = first again.
        app.browser_cmd(crate::browser::BCmd::History(-1));
        assert_eq!(app.state.browser_sel.clone().unwrap(), second);
        app.browser_cmd(crate::browser::BCmd::History(1));
        assert_eq!(app.state.browser_sel.clone().unwrap(), first);
        // Esc clears the search and leaves the box.
        app.key(&Key::Named(NamedKey::Escape), false, false);
        assert!(app.state.browser_search.is_empty() && !app.state.browser_search_focus);
        assert_eq!(rows(&app.state).len(), 4);
        // Search box on top moves the list down.
        app.browser_cmd(crate::browser::BCmd::Toggle("top"));
        frame(&mut app, &mut c, None);
        assert!(app.hits.iter().any(|h| h.action == Action::BrowserSearch && h.rect.y < 200.0));
    }

    /// Shift+wheel over a channel's name button (spec: ui/channel-rack.md §13): the channel moves a row a notch, up for
    /// wheel up, and is the only one selected. With the mouse still, more notches move the same channel, which has left
    /// the cursor. Without Shift, or over the steps, no channel moves.
    #[test]
    fn shift_wheel_moves_a_channel() {
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        frame(&mut app, &mut c, None);
        let order = |app: &App| (0..app.state.project.channels.len()).map(|r| crate::rack::channel_name(&app.state, r)).collect::<Vec<_>>();
        let b = order(&app);
        assert!(b.len() >= 5);
        let rows = |ix: [usize; 5]| ix.map(|i| b[i].clone());
        let name = hit_rect(&app, &Action::ChannelSelect(2));
        app.mouse_move(name.cx(), name.cy());
        app.wheel(0.0, -1.0, true, false);
        assert_eq!(order(&app)[..5], rows([0, 1, 3, 2, 4]), "down a row");
        assert_eq!(app.state.rack.sel, std::collections::BTreeSet::from([3]));
        frame(&mut app, &mut c, None);
        app.wheel(0.0, 1.0, true, false);
        app.wheel(0.0, 1.0, true, false);
        assert_eq!(order(&app)[..5], rows([0, 2, 1, 3, 4]), "the same channel, two rows up");
        assert_eq!(app.state.rack.sel, std::collections::BTreeSet::from([1]));
        frame(&mut app, &mut c, None);
        let moved = order(&app);
        let name = hit_rect(&app, &Action::ChannelSelect(4));
        app.mouse_move(name.cx(), name.cy());
        app.wheel(0.0, 1.0, false, false);
        let step = hit_rect(&app, &Action::Step(4, 2));
        app.mouse_move(step.cx(), step.cy());
        app.wheel(0.0, 1.0, true, false);
        assert_eq!(order(&app), moved, "no Shift, or over the steps");
    }

    /// Channel rack (spec: ui/channel-rack.md §5): stacking with other windows, name button opens
    /// the plugin window, select slot, mixer-track drag, add, resize; drags repaint like full frames.
    #[test]
    fn channel_rack_window() {
        let Ok(mut a) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut b = Canvas::new(1920, 1080, 1.0).unwrap();
        let mut app = App::new(None);
        frame(&mut app, &mut a, None);
        // Clicking the playlist next to the rack puts the rack behind it; F6 brings it back.
        let rack = views::rack::rect(&app.state);
        click(&mut app, &mut a, Rect::new(rack.right() + 50.0, rack.y + 100.0, 2.0, 2.0));
        assert_eq!(app.state.z.last(), Some(&Win::Playlist));
        let over = (rack.right() - 60.0, rack.y + 100.0); // inside both windows
        assert_eq!(app.top_hit(over.0, over.1).and_then(|h| h.win), Some(Win::Playlist), "the playlist covers the rack there");
        app.act(Action::ToggleWin(Win::ChannelRack));
        frame(&mut app, &mut a, None);
        assert_eq!(app.state.z.last(), Some(&Win::ChannelRack));
        app.act(Action::ToggleWin(Win::ChannelRack));
        assert!(!app.state.shown(Win::ChannelRack), "F6 on the front rack hides it");
        app.act(Action::ToggleWin(Win::ChannelRack));
        frame(&mut app, &mut a, None);

        // Name button opens the channel's Lemon Sampler (a sampler row never shows the info window); the slot selects.
        let hr = hit_rect(&app, &Action::ChannelSelect(2));
        click(&mut app, &mut a, hr);
        assert_eq!((app.state.pending_open, app.state.selected_channel), (Some(2), 2));
        assert!(!app.state.shown(Win::Plugin));
        let hr = hit_rect(&app, &Action::RackSelect(3));
        click(&mut app, &mut a, hr);
        assert_eq!(app.state.rack.sel, std::collections::BTreeSet::from([3]));
        assert_eq!(app.state.z.last(), Some(&Win::ChannelRack), "clicking the rack raised it");

        // Drag the mixer box up 20 px: +5 inserts, with damage repaint matching a full frame.
        let _ = app.take_damage();
        let mb = hit_rect(&app, &Action::RackMixer(0));
        let t0 = app.state.project.channels[0].mixer_track;
        let step = |app: &mut App, c: &mut Canvas, f: &dyn Fn(&mut App)| {
            f(app);
            let d = app.take_damage();
            frame_damage(app, c, d);
        };
        step(&mut app, &mut a, &|ap| { ap.mouse_move(mb.cx(), mb.cy()); });
        step(&mut app, &mut a, &|ap| ap.mouse_down());
        step(&mut app, &mut a, &|ap| { ap.mouse_move(mb.cx(), mb.cy() - 20.0); });
        assert_eq!(app.state.project.channels[0].mixer_track, t0 + 2, "10 px per insert (§12)");
        step(&mut app, &mut a, &|ap| ap.mouse_up());

        // Resize from the corner: wider shows more steps, taller more room.
        let r = views::rack::rect(&app.state);
        step(&mut app, &mut a, &|ap| { ap.mouse_move(r.right() - 2.0, r.bottom() - 2.0); });
        assert_eq!(app.cursor(), Cursor::NwseResize);
        step(&mut app, &mut a, &|ap| ap.mouse_down());
        step(&mut app, &mut a, &|ap| { ap.mouse_move(r.right() + 318.0, r.bottom() + 58.0); });
        step(&mut app, &mut a, &|ap| ap.mouse_up());
        assert_eq!(app.state.rack_size, (views::rack::DEFAULT_SIZE.0 + 320.0, 306.0));
        assert_eq!(views::rack::visible_steps(&app.state), 36);
        frame(&mut app, &mut b, None);
        assert_same(&a, &b, "after mixer drag and resize");

        // A dimmed step (past the one-bar pattern) extends the pattern to two bars.
        assert_eq!(crate::rack::pattern_steps(&app.state), 16);
        let hr = hit_rect(&app, &Action::Step(0, 20));
        click(&mut app, &mut a, hr);
        assert_eq!(crate::rack::pattern_steps(&app.state), 32);

        // Sizes snap to whole steps / rows (FL's widths plus the wider name button), down to four beats of the
        // project's signature but never under 16 steps (the title bar's buttons would collide), and one row.
        let d = crate::rack::NAME_W - crate::rack::FL_NAME_W;
        assert_eq!(views::rack::snap_size(&app.state, 822.0 + d, 300.0), (819.0 + d, 306.0));
        assert_eq!(views::rack::snap_size(&app.state, 10.0, 10.0), (499.0 + d, 126.0));
        let keep = app.state.rack_size;
        app.state.rack_size = (499.0 + d, 126.0);
        assert_eq!(views::rack::visible_steps(&app.state), 16);
        app.state.rack_size = keep;
        let bar = app.state.project.bar;
        app.state.project.bar = Some((2, 8));
        assert_eq!(views::rack::snap_size(&app.state, 10.0, 10.0), (499.0 + d + 16.0 * views::rack::STEP_W, 126.0));
        app.state.project.bar = Some((6, 2));
        assert_eq!(views::rack::snap_size(&app.state, 10.0, 10.0), (499.0 + d, 126.0));
        app.state.project.bar = bar;

        // Piano-roll view toggle: rows become mini rolls; clicking one opens the Piano roll.
        assert!(!app.hits.iter().any(|h| matches!(h.action, Action::RackRoll(_))));
        let hr = hit_rect(&app, &Action::RackRollView);
        click(&mut app, &mut a, hr);
        assert!(app.state.rack_roll_view);
        assert!(!app.hits.iter().any(|h| matches!(h.action, Action::Step(..))), "no steps in roll view");
        let hr = hit_rect(&app, &Action::RackRoll(1));
        click(&mut app, &mut a, hr);
        assert_eq!((app.state.z.last(), app.state.selected_channel), (Some(&Win::PianoRoll), 1));
        app.state.rack_roll_view = false;
        app.act(Action::ToggleWin(Win::ChannelRack));
        frame(&mut app, &mut a, None);

        // Add channels until the rows overflow: a vertical scroll bar appears. "+" opens the generator menu
        // (spec: ui/plugin-menu.md §1); Lemon Sampler adds a sampler channel at the bottom.
        app.plugins = crate::plugins::Plugins::builtins();
        app.state.plugdb.rebuild(&app.plugins);
        let n = app.state.project.channels.len();
        for _ in 0..6 {
            let hr = hit_rect(&app, &Action::RackAdd);
            click(&mut app, &mut a, hr);
            let pick = app.state.playlist.popup.as_ref().and_then(|p| p.items.iter().find(|i| i.label == crate::plugins::LEMON_SAMPLER)).map(|i| i.cmd.clone());
            crate::playlist::run(&mut app, pick.expect("the generator menu lists Lemon Sampler"));
            assert!(app.state.playlist.popup.is_none());
            frame(&mut app, &mut a, None);
        }
        assert_eq!(app.state.project.channels.len(), n + 6);
        assert_eq!(app.state.project.channels.last().unwrap().mixer_track, 11, "after the highest insert in use (5)");
        // Auto resize (minimum height, §7) grows the rack with its rows; with it off the rows overflow into a scroll bar.
        assert!(app.bar(Scroll::RackY).is_none_or(|b| b.max == 0.0), "auto resize fits the rows");
        app.state.rack.auto_resize = crate::rack::AutoResize::Off;
        app.state.rack_size.1 = 200.0;
        frame(&mut app, &mut a, None);
        assert!(app.bar(Scroll::RackY).is_some_and(|b| b.max > 0.0));
    }

    /// Playlist / Mixer / Piano roll windows: drag the title bar, resize from the edges (down to the
    /// minimum size), minimize and maximize; damage repaints match full frames.
    #[test]
    fn window_move_resize_min_max() {
        for design in Design::ALL {
            window_move_resize_in(design);
        }
    }

    fn window_move_resize_in(design: Design) {
        let Ok(mut a) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut b = Canvas::new(1920, 1080, 1.0).unwrap();
        let mut app = App::new(None);
        app.theme = design_theme(design, false);
        app.state.wins.iter_mut().for_each(|w| w.maxed = false);
        app.state.z = vec![Win::Playlist, Win::Mixer];
        frame(&mut app, &mut a, None);
        let _ = app.take_damage();
        let step = |app: &mut App, c: &mut Canvas, f: &dyn Fn(&mut App)| {
            f(app);
            let d = app.take_damage();
            frame_damage(app, c, d);
        };
        let r0 = app.state.win_rect(Win::Mixer);
        assert_eq!(r0, Rect::new(231.0, 565.0, 1552.0, 435.0));

        // Move by the title bar.
        let bar = hit_rect(&app, &Action::WinDrag(Win::Mixer));
        let (bx, by) = (bar.cx(), bar.cy());
        step(&mut app, &mut a, &|ap| { ap.mouse_move(bx, by); });
        step(&mut app, &mut a, &|ap| ap.mouse_down());
        step(&mut app, &mut a, &|ap| { ap.mouse_move(bx - 100.0, by - 50.0); });
        step(&mut app, &mut a, &|ap| ap.mouse_up());
        let r1 = app.state.win_rect(Win::Mixer);
        assert_eq!((r1.x, r1.y, r1.w, r1.h), (r0.x - 100.0, r0.y - 50.0, r0.w, r0.h));

        // Double-click the title bar: maximize, then double-click again: back to r1.
        for want_max in [true, false] {
            let bar = hit_rect(&app, &Action::WinDrag(Win::Mixer));
            let (bx, by) = (bar.x + 150.0, bar.cy());
            step(&mut app, &mut a, &|ap| { ap.mouse_move(bx, by); });
            for _ in 0..2 {
                step(&mut app, &mut a, &|ap| ap.mouse_down());
                step(&mut app, &mut a, &|ap| ap.mouse_up());
            }
            assert_eq!(app.state.is_max(Win::Mixer), want_max);
        }
        assert_eq!(app.state.win_rect(Win::Mixer), r1);

        // Resize from the bottom-right corner: bigger, then clamped to the minimum size.
        let corner = hit_rect(&app, &Action::WinResize(Win::Mixer, EDGE_R | EDGE_B));
        let (cx, cy) = (corner.cx(), corner.cy());
        step(&mut app, &mut a, &|ap| { ap.mouse_move(cx, cy); });
        assert_eq!(app.cursor(), Cursor::NwseResize);
        step(&mut app, &mut a, &|ap| ap.mouse_down());
        step(&mut app, &mut a, &|ap| { ap.mouse_move(cx - 40.0, cy + 30.0); });
        let r2 = app.state.win_rect(Win::Mixer);
        assert_eq!((r2.w, r2.h), (r1.w - 40.0, r1.h + 30.0));
        step(&mut app, &mut a, &|ap| { ap.mouse_move(cx - 3000.0, cy - 3000.0); });
        let r3 = app.state.win_rect(Win::Mixer);
        assert_eq!((r3.x, r3.y, r3.w, r3.h), (r1.x, r1.y, Win::Mixer.min_size().0, Win::Mixer.min_size().1));
        step(&mut app, &mut a, &|ap| ap.mouse_up());
        frame(&mut app, &mut b, None);
        assert_same(&a, &b, "after window move and resize");

        // Top-left resize keeps the opposite corner fixed.
        frame(&mut app, &mut a, None);
        let tl = hit_rect(&app, &Action::WinResize(Win::Mixer, EDGE_L | EDGE_T));
        step(&mut app, &mut a, &|ap| { ap.mouse_move(tl.cx(), tl.cy()); });
        step(&mut app, &mut a, &|ap| ap.mouse_down());
        step(&mut app, &mut a, &|ap| { ap.mouse_move(tl.cx() - 60.0, tl.cy() - 40.0); });
        step(&mut app, &mut a, &|ap| ap.mouse_up());
        let r4 = app.state.win_rect(Win::Mixer);
        assert_eq!((r4.right(), r4.bottom()), (r3.right(), r3.bottom()));
        assert_eq!((r4.w, r4.h), (r3.w + 60.0, r3.h + 40.0));

        // Minimize collapses to the title bar and restores; maximize fills the workspace.
        app.act(Action::MinWin(Win::Mixer));
        frame(&mut app, &mut a, None);
        assert_eq!(app.state.win_rect(Win::Mixer).h, TITLE_H);
        assert!(app.hits.iter().all(|h| !matches!(h.action, Action::WinResize(Win::Mixer, _))), "no resize edges when collapsed");
        app.act(Action::MinWin(Win::Mixer));
        assert_eq!(app.state.win_rect(Win::Mixer), r4);
        app.act(Action::MaxWin(Win::Mixer));
        frame(&mut app, &mut a, None);
        assert_eq!(app.state.win_rect(Win::Mixer), app.state.work_rect());
        assert_eq!(app.state.docked(), Some(Win::Mixer));
        app.act(Action::MaxWin(Win::Mixer));
        assert_eq!(app.state.win_rect(Win::Mixer), r4);

        // The window buttons act on the release over them: not on the press, and not when the mouse slides off
        // before the release.
        frame(&mut app, &mut a, None);
        let close = hit_rect(&app, &Action::CloseWin(Win::Mixer));
        step(&mut app, &mut a, &|ap| { ap.mouse_move(close.cx(), close.cy()); });
        step(&mut app, &mut a, &|ap| ap.mouse_down());
        assert!(app.state.shown(Win::Mixer), "the press does not close");
        step(&mut app, &mut a, &|ap| { ap.mouse_move(close.cx() - 200.0, close.cy() + 200.0); });
        step(&mut app, &mut a, &|ap| ap.mouse_up());
        assert!(app.state.shown(Win::Mixer), "released off the button: nothing");
        step(&mut app, &mut a, &|ap| { ap.mouse_move(close.cx(), close.cy()); });
        step(&mut app, &mut a, &|ap| ap.mouse_down());
        step(&mut app, &mut a, &|ap| ap.mouse_up());
        assert!(!app.state.shown(Win::Mixer), "released over the button: closed");

        // The mixer layout scales with its height only (1 at the measured size).
        assert_eq!(views::mixer::scale(Rect::new(209.0, 83.0, 1711.0, 997.0)), 1.0);
        assert!(views::mixer::scale(Rect::new(0.0, 0.0, 600.0, 435.0)) < 0.3);

        // Dragging a fader works at a squished size: 0 dB stays under the handle's start.
        let mut app = App::new(None);
        app.state.z = vec![Win::Mixer];
        frame(&mut app, &mut a, None);
        let f = hit_rect(&app, &Action::Fader(1));
        app.mouse_move(f.cx(), f.cy());
        app.mouse_down();
        app.mouse_move(f.cx(), f.cy() - 10.0);
        app.mouse_up();
        let v = app.state.project.mixer.iter().find(|m| m.index == 1).and_then(|m| m.volume).unwrap();
        assert!(v > 0.8, "fader moved up: {v}");
    }

    /// Tiny windows (e.g. while restoring after Alt+Tab) must not panic in any view.
    #[test]
    fn tiny_windows_do_not_panic() {
        for (w, h) in [(1, 1), (120, 28), (400, 120), (800, 300), (1920, 140)] {
            let Ok(mut c) = Canvas::new(w, h, 1.0) else { return };
            for z in [vec![Win::Mixer, Win::ChannelRack], vec![Win::Playlist], vec![Win::PianoRoll, Win::Plugin]] {
                let mut app = App::new(None);
                app.state.z = z;
                frame(&mut app, &mut c, None);
                app.mouse_move(w as f32 / 2.0, h as f32 / 2.0);
                app.wheel(0.0, -1.0, false, false);
                frame(&mut app, &mut c, None);
            }
        }
    }

    /// Needs an audio device: `cargo test -p os-app -- --ignored app_deleted_notes`.
    #[test]
    #[ignore]
    fn app_deleted_notes_stop_playing() {
        use winit::keyboard::{Key, NamedKey};
        let mut app = App::new(None);
        app.state.z = vec![Win::PianoRoll];
        app.state.selected_channel = 0;
        let ch = piano::channel(&app.state).unwrap();
        for i in 0..8 {
            piano::add_note(&mut app.state, ch, i * 48, 60);
        }
        app.act(Action::Play);
        for _ in 0..30 {
            std::thread::sleep(std::time::Duration::from_millis(16));
            app.tick();
        }
        assert!(app.transport.playing());
        app.key(&Key::Character("a".into()), true, false);
        app.key(&Key::Named(NamedKey::Delete), false, false);
        assert!(piano::notes(&app.state).is_empty());
        let mut loud = 0;
        for i in 0..120 {
            std::thread::sleep(std::time::Duration::from_millis(16));
            app.tick();
            if i > 20 && !app.transport.sounding().is_empty() {
                loud += 1;
            }
        }
        assert_eq!(loud, 0, "deleted notes still sounding");
    }

    /// Play / pause / stop and the PAT / SONG switch drive one consistent state: the playheads follow the playing
    /// mode and tick, pause holds the tick, stop rewinds (needs an audio device).
    #[test]
    #[ignore]
    fn transport_states_and_modes() {
        use crate::transport::State as T;
        let step = || std::thread::sleep(std::time::Duration::from_millis(250));
        let mut app = App::new(None);
        let ch = app.state.project.channels[0].index;
        app.state.project.patterns = vec![Pattern {
            number: 1,
            notes: (0..8).map(|i| Note { length: 24, key: 60, ..Note::step(ch, i * 48) }).collect(),
            ..Pattern::default()
        }];
        app.state.project.arrangements = vec![os_model::Arrangement {
            number: 1,
            name: None,
            clips: vec![os_model::Clip::new(0, 384 * 4, 0, os_model::ClipSource::Pattern(1))],
            ..Default::default()
        }];
        // Song mode. A device opening for the first time may take a while.
        app.act(Action::PatSong(true));
        app.act(Action::Play);
        let start = std::time::Instant::now();
        while app.transport.tick() <= 10.0 && start.elapsed() < std::time::Duration::from_secs(3) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(app.transport.state(), T::Playing);
        assert!(app.transport.song() && app.transport.tick() > 10.0, "song tick {}", app.transport.tick());
        // Pause holds the tick in place; Play resumes from it (no restart).
        app.act(Action::Play);
        step();
        let held = app.transport.tick();
        assert_eq!(app.transport.state(), T::Paused);
        step();
        assert_eq!(app.transport.tick(), held, "paused tick holds");
        app.act(Action::Play);
        step();
        assert!(app.transport.tick() > held, "resumed from {held}: {}", app.transport.tick());
        // Switching to PAT restarts in pattern mode, which loops inside one bar.
        app.act(Action::PatSong(false));
        step();
        assert!(app.transport.playing() && !app.transport.song());
        assert!(app.transport.tick() < 384.0 + 1.0);
        // Stop rewinds and stays stopped.
        app.act(Action::Stop);
        step();
        assert_eq!((app.transport.state(), app.transport.tick()), (T::Stopped, 0.0));
        // An empty playlist does not start SONG mode.
        app.state.project.arrangements.clear();
        app.act(Action::PatSong(true));
        app.act(Action::Play);
        step();
        assert_eq!(app.transport.state(), T::Stopped);
    }

    /// Stop goes back to where playback started: the song position and the piano roll's marker stay where they were
    /// put (user report: Space played the song from its beginning); with a time selection, to its start.
    #[test]
    fn stop_keeps_the_start_position() {
        let mut app = App::new(None);
        app.state.song_mode = true;
        app.state.playlist.pos = 768;
        app.act(Action::Stop);
        assert_eq!(app.state.playlist.pos, 768);
        app.state.playlist.range = Some((384, 1152));
        app.act(Action::Stop);
        assert_eq!(app.state.playlist.pos, 384, "the time selection's start");
        app.state.song_mode = false;
        app.state.piano.marker = 48;
        app.act(Action::Stop);
        assert_eq!(app.state.piano.marker, 48);
    }

    /// L flips the PAT / SONG switch (FL's key); with the typing keyboard on, L is a note instead.
    #[test]
    fn l_flips_pat_and_song() {
        use winit::keyboard::{Key, KeyCode};
        let mut app = App::new(None);
        if app.typing_on() {
            app.toggle_typing();
        }
        let song = app.state.song_mode;
        app.key(&Key::Character("l".into()), false, false);
        assert_eq!(app.state.song_mode, !song);
        app.key(&Key::Character("L".into()), false, false);
        assert_eq!(app.state.song_mode, song);
        app.key(&Key::Character("l".into()), true, false);
        assert_eq!(app.state.song_mode, song, "Ctrl+L is not the switch");
        app.toggle_typing();
        assert!(app.typing_key(KeyCode::KeyL, true, false, false), "the typing keyboard takes L");
    }

    /// A click in the event panel sets the note already playing where it lands (the latest start at or before it),
    /// above or below its line, in a gap after it too; left of the first note, the first note (spec: ui/piano-roll.md
    /// §7, "Drag in the event panel").
    #[test]
    fn event_panel_click_sets_the_note_already_playing() {
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.z = vec![Win::PianoRoll];
        app.state.selected_channel = 0;
        let ch = piano::channel(&app.state).unwrap();
        let p = piano::pattern_mut(&mut app.state);
        p.notes.retain(|n| n.channel != ch);
        p.notes.extend([96, 192].map(|pos| os_model::Note { length: 48, key: 60, velocity: 64, ..os_model::Note::step(ch, pos) }));
        frame(&mut app, &mut c, None);
        let (g, panel) = (app.piano_geom().unwrap(), app.piano_panel.unwrap());
        let vel = |app: &App| -> Vec<u8> {
            piano::notes(&app.state).iter().filter(|n| n.channel == ch).map(|n| n.velocity).collect()
        };
        let (low, high) = (panel.bottom() - panel.h * 0.1, panel.y + panel.h * 0.1);
        let click = |app: &mut App, tick: f32, y: f32| {
            let x = g.x_of(tick);
            app.paint_velocity(x, y, x, y);
        };
        click(&mut app, 10.0, low);
        let first = vel(&app);
        assert!(first[0] < 20 && first[1] == 64, "left of the first note: {first:?}");
        click(&mut app, 160.0, high);
        let gap = vel(&app);
        assert!(gap[0] > 100 && gap[1] == 64, "a gap: the note before it: {gap:?}");
        click(&mut app, 200.0, low);
        let inside = vel(&app);
        assert!(inside[0] > 100 && inside[1] < 20, "inside the second note: {inside:?}");
    }

    /// SONG mode: a click in the piano roll's ruler moves the song position to where that tick of the pattern plays
    /// (the clip under the current position first, else the next, else the first); the piano roll's playhead maps
    /// the song position back into the pattern while a clip of it plays. A tick no clip shows keeps the old
    /// behaviour (the play-start marker).
    #[test]
    fn piano_ruler_in_song_mode_seeks_the_song() {
        use crate::playlist as pl;
        use os_model::{Clip, ClipSource};
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.pattern = 1;
        app.state.song_mode = true;
        // Pattern 1 at bar 2 (window 0..384) and at bar 5 showing its second half only (window 192..384).
        let mut cut = Clip::new(4 * 384, 192, 0, ClipSource::Pattern(1));
        cut.window = Some((192.0, 384.0));
        let mut whole = Clip::new(384, 384, 0, ClipSource::Pattern(1));
        whole.window = Some((0.0, 384.0));
        pl::arr_mut(&mut app.state).clips = vec![whole, cut];
        assert_eq!(piano::song_tick_of(&app.state, 96, 0), Some(480), "before both clips: the first that shows it");
        assert_eq!(piano::song_tick_of(&app.state, 96, 1000), Some(480), "only the first clip shows tick 96");
        assert_eq!(piano::song_tick_of(&app.state, 240, 1600), Some(1584), "the clip under the position");
        assert_eq!(piano::song_tick_of(&app.state, 240, 800), Some(1584), "the next clip after the position");
        assert_eq!(piano::pattern_tick_in_song(&app.state, 1584.0), Some(240.0));
        assert_eq!(piano::pattern_tick_in_song(&app.state, 1000.0), None, "between the clips");
        // Stopped: the press moves the song position.
        assert!(app.piano_song_seek(96, true));
        assert_eq!(app.state.playlist.pos, 480);
        // A tick no clip shows: nothing moves.
        pl::arr_mut(&mut app.state).clips.truncate(1);
        assert!(!app.piano_song_seek(500, true));
        assert_eq!(app.state.playlist.pos, 480);
    }

    /// Shortening a pattern clip that plays its whole pattern cuts it: it gets a window and plays only that.
    #[test]
    fn a_resized_pattern_clip_plays_only_its_window() {
        use crate::playlist as pl;
        use os_model::{Clip, ClipSource};
        let mut app = App::new(None);
        app.state.project = test_project();
        pl::arr_mut(&mut app.state).clips = vec![Clip::new(0, 384, 0, ClipSource::Pattern(1))];
        let i = 0;
        let orig = pl::clips(&app.state)[i];
        assert_eq!(orig.window, None, "a new pattern clip plays its whole pattern");
        pl::resize_right(&mut app.state, i, &orig, 48);
        let c = pl::clips(&app.state)[i];
        assert_eq!((c.length, c.window), (48, Some((0.0, 48.0))));
    }

    /// Same with right-drag erasing and note moves while playing (needs an audio device).
    #[test]
    #[ignore]
    fn app_erased_notes_stop_playing() {
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.z = vec![Win::PianoRoll];
        app.state.selected_channel = 0;
        frame(&mut app, &mut c, None);
        let g = app.piano_geom().unwrap();
        let y = g.y_of(64) + 8.0;
        // Draw notes by clicking, while playing.
        app.act(Action::Play);
        for i in 0..8 {
            app.mouse_move(g.x_of((i * 48) as f32) + 2.0, y);
            app.mouse_down();
            app.mouse_move(g.x_of((i * 48) as f32) + 2.0, y + 18.0 * (i % 3) as f32);
            app.mouse_up();
            frame(&mut app, &mut c, None);
            app.tick();
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        for _ in 0..60 {
            std::thread::sleep(std::time::Duration::from_millis(16));
            app.tick();
        }
        assert!(!app.transport.sounding().is_empty() || app.transport.peaks().0 > 0.0, "playing notes");
        // Right-drag across all rows to erase.
        frame(&mut app, &mut c, None);
        let g = app.piano_geom().unwrap();
        for key in (55..=70).rev() {
            let yy = g.y_of(key) + 8.0;
            if !g.grid.contains(g.grid.x + 2.0, yy) {
                continue;
            }
            app.mouse_move(g.grid.x + 2.0, yy);
            app.mouse_down_right();
            for i in 0..120 {
                app.mouse_move(g.grid.x + 2.0 + i as f32 * 4.0, yy);
                if i % 8 == 0 {
                    app.tick();
                }
            }
            app.mouse_up();
            frame(&mut app, &mut c, None);
        }
        let ch = piano::channel(&app.state);
        assert!(piano::notes(&app.state).iter().all(|n| Some(n.channel) != ch), "{:?}", piano::notes(&app.state));
        let mut loud = 0;
        for i in 0..150 {
            std::thread::sleep(std::time::Duration::from_millis(16));
            app.tick();
            if i > 20 && !app.transport.sounding().is_empty() {
                loud += 1;
            }
        }
        assert_eq!(loud, 0, "erased notes still sounding: {:?}", app.transport.sounding());
    }

    /// The grid / event panel splitter (spec: ui/event-panel-splitter.md): drag with the minimums and collapsing,
    /// double click to hide and bring back the panel, the vertical resize cursor.
    #[test]
    fn event_panel_splitter() {
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.z = vec![Win::PianoRoll];
        frame(&mut app, &mut c, None);
        let band = hit_rect(&app, &Action::PianoSplitter);
        let (grid, panel) = (app.piano_grid.unwrap(), app.piano_panel.unwrap());
        assert_eq!((panel.h, band.y), (177.0, panel.y - 8.0), "the default layout");
        let h = panel.bottom() - grid.y - 8.0;
        app.mouse_move(band.cx(), band.y + 3.0);
        frame(&mut app, &mut c, None);
        assert_eq!(app.cursor(), Cursor::NsResize);

        // Dragging up 100 px gives the panel 100 px more, keeping the grab offset.
        app.mouse_down();
        app.mouse_move(band.cx(), band.y - 97.0);
        app.mouse_up();
        frame(&mut app, &mut c, None);
        assert_eq!(app.piano_panel.unwrap().h, 277.0);
        assert_eq!(app.piano_grid.unwrap().h + app.piano_panel.unwrap().h, h);
        // Down to 30 px of panel: held at 45; past half of that, collapsed.
        let drag_to = |app: &mut App, c: &mut Canvas, panel_h: f32| {
            let b = hit_rect(app, &Action::PianoSplitter);
            app.state.piano.split_click = None;
            app.mouse_move(b.cx(), b.y + 3.0);
            app.mouse_down();
            app.mouse_move(b.cx(), panel.bottom() - panel_h - 8.0 + 3.0);
            app.mouse_up();
            frame(app, c, None);
            app.piano_panel.unwrap().h
        };
        assert_eq!(drag_to(&mut app, &mut c, 30.0), 45.0);
        assert_eq!(drag_to(&mut app, &mut c, 20.0), 0.0);
        assert!(app.state.piano.split.panel_collapsed());
        assert_eq!(hit_rect(&app, &Action::PianoSplitter).bottom(), panel.bottom(), "the band at the bottom");
        // Open again at 200 px, then a double click collapses it and another brings back 200 px.
        assert_eq!(drag_to(&mut app, &mut c, 200.0), 200.0);
        let b = hit_rect(&app, &Action::PianoSplitter);
        app.state.piano.split_click = None;
        app.mouse_move(b.cx(), b.y + 3.0);
        for _ in 0..2 {
            app.mouse_down();
            app.mouse_up();
        }
        frame(&mut app, &mut c, None);
        assert_eq!(app.piano_panel.unwrap().h, 0.0);
        let b = hit_rect(&app, &Action::PianoSplitter);
        app.mouse_move(b.cx(), b.y + 3.0);
        for _ in 0..2 {
            app.mouse_down();
            app.mouse_up();
        }
        frame(&mut app, &mut c, None);
        assert_eq!(app.piano_panel.unwrap().h, 200.0);
        // Up to under the ruler: the grid collapses and the panel takes the whole height.
        assert_eq!(drag_to(&mut app, &mut c, h - 10.0), h);
        assert_eq!(app.piano_grid.unwrap().h, 0.0);
    }

    /// Event panel: the Repeat lane and the channel control lanes (spec: ui/piano-roll.md §7a, §7b).
    #[test]
    fn event_panel_repeat_and_channel_lanes() {
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.z = vec![Win::PianoRoll];
        app.state.selected_channel = 0;
        frame(&mut app, &mut c, None);
        let ch = piano::channel(&app.state).unwrap();
        let g = app.piano_geom().unwrap();
        let panel = app.piano_panel.unwrap();

        // A note at tick 0, then the Repeat lane: pressing at its start at 6/14 of the height sets repeat 6.
        app.mouse_move(g.x_of(0.0) + 2.0, g.y_of(60) + 8.0);
        app.mouse_down();
        app.mouse_up();
        app.state.piano.lane = piano::Lane::Repeat;
        frame(&mut app, &mut c, None);
        app.mouse_move(g.x_of(0.0) + 2.0, panel.bottom() - panel.h * 6.0 / 14.0);
        app.mouse_down();
        app.mouse_up();
        let repeat = piano::notes(&app.state).iter().find(|n| n.channel == ch).map(|n| n.repeat);
        assert_eq!(repeat, Some(6));

        // Channel volume: a drag lays one point per snap step along the cursor path.
        app.state.piano.lane = piano::Lane::ChanVolume;
        frame(&mut app, &mut c, None);
        let (y0, y1) = (panel.bottom() - panel.h * 0.25, panel.bottom() - panel.h * 0.75);
        app.mouse_move(g.x_of(24.0) + 1.0, y0);
        app.mouse_down();
        for i in 1..=8 {
            let k = i as f32 / 8.0;
            app.mouse_move(g.x_of(24.0) + 1.0 + (g.x_of(96.0) - g.x_of(24.0)) * k, y0 + (y1 - y0) * k);
        }
        app.mouse_up();
        frame(&mut app, &mut c, None);
        let pts: Vec<(u32, i32)> = piano::controls(&app.state).iter().filter(|p| p.channel() == ch).map(|p| (p.position, p.value)).collect();
        let at: Vec<u32> = pts.iter().map(|p| p.0).collect();
        assert_eq!(at, vec![24, 48, 72, 96], "{pts:?}");
        assert!(pts.windows(2).all(|w| w[0].1 < w[1].1), "values rise with the cursor: {pts:?}");
        assert!((pts[0].1 - 3200).abs() < 400 && (pts[3].1 - 9600).abs() < 400, "{pts:?}");

        // The same drag in the pan lane adds separate points (other target), right-drag erases only this lane.
        app.state.piano.lane = piano::Lane::ChanPan;
        frame(&mut app, &mut c, None);
        app.mouse_move(g.x_of(48.0) + 1.0, y0);
        app.mouse_down();
        app.mouse_up();
        assert_eq!(piano::controls(&app.state).len(), 5);
        app.state.piano.lane = piano::Lane::ChanVolume;
        frame(&mut app, &mut c, None);
        app.mouse_move(g.x_of(48.0) + 1.0, y0);
        app.mouse_down_right();
        app.mouse_move(g.x_of(72.0) + 1.0, y0);
        app.mouse_up();
        let left: Vec<(u16, u32)> = piano::controls(&app.state).iter().map(|p| (p.param(), p.position)).collect();
        assert_eq!(left.len(), 3, "{left:?}");
        assert!(left.contains(&(os_model::ControlPoint::PAN, 48)), "pan point kept: {left:?}");
        assert!(piano::undo(&mut app.state));
        assert_eq!(piano::controls(&app.state).len(), 5, "undo restores the erased points");
    }

    /// The pitch lane's own scroll bar (spec: ui/piano-roll.md §7b): arrows move the window 455 cents, it stops at
    /// ±4800 cents, the bar and the arrows agree, and points drawn after scrolling take the scrolled value.
    #[test]
    fn pitch_lane_scrolls() {
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.z = vec![Win::PianoRoll];
        app.state.selected_channel = 0;
        app.state.piano.lane = piano::Lane::ChanPitch;
        frame(&mut app, &mut c, None);
        assert_eq!(app.state.piano.pitch_center, 0);
        for _ in 0..3 {
            app.act(Action::PitchStep(1));
        }
        assert_eq!(app.state.piano.pitch_center, 1365);
        for _ in 0..40 {
            app.act(Action::PitchStep(1));
        }
        assert_eq!(app.state.piano.pitch_center, piano::PITCH_RANGE - 241, "stops at the top of the range");
        for _ in 0..40 {
            app.act(Action::PitchStep(-1));
        }
        assert_eq!(app.state.piano.pitch_center, -(piano::PITCH_RANGE - 241));

        // The bar maps to the same window: the thumb at the top is the top of the range.
        app.state.piano.pitch_center = 0;
        frame(&mut app, &mut c, None);
        assert!(app.bar(Scroll::PitchY).is_some(), "the lane has a scroll bar");
        app.scroll_set(Scroll::PitchY, 0.0);
        assert_eq!(app.state.piano.pitch_center, piano::PITCH_RANGE - 241);
        assert_eq!(app.scroll_get(Scroll::PitchY), 0.0);
        app.state.piano.pitch_center = 1365;

        // A press at the middle of the panel sets a point at about the window's centre value.
        frame(&mut app, &mut c, None);
        let g = app.piano_geom().unwrap();
        let panel = app.piano_panel.unwrap();
        app.mouse_move(g.x_of(48.0) + 1.0, panel.y + panel.h / 2.0);
        app.mouse_down();
        app.mouse_up();
        let v = piano::controls(&app.state).iter().find(|p| p.param() == os_model::ControlPoint::PITCH).map(|p| p.value);
        assert!(v.is_some_and(|v| (v - 1365).abs() <= 2), "{v:?}");
    }

    /// Piano-roll tools driven with the mouse and keys (spec: ui/piano-roll.md §9).
    #[test]
    fn piano_tools() {
        use winit::keyboard::{Key, NamedKey};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.z = vec![Win::PianoRoll];
        app.state.selected_channel = 0;
        frame(&mut app, &mut c, None);
        let ch = piano::channel(&app.state);
        let mine = |app: &App| -> Vec<Note> { piano::notes(&app.state).iter().filter(|n| Some(n.channel) == ch).copied().collect() };
        let g = app.piano_geom().unwrap();
        let row = |k: i32| g.y_of(k) + 8.0;
        let click = |app: &mut App, c: &mut Canvas, x: f32, y: f32| {
            app.mouse_move(x, y);
            app.mouse_down();
            app.mouse_up();
            frame(app, c, None);
        };
        let swipe = |app: &mut App, c: &mut Canvas, (x0, y0): (f32, f32), (x1, y1): (f32, f32), right: bool| {
            app.mouse_move(x0, y0);
            if right { app.mouse_down_right() } else { app.mouse_down() }
            for i in 1..=20 {
                let k = i as f32 / 20.0;
                app.mouse_move(x0 + (x1 - x0) * k, y0 + (y1 - y0) * k);
            }
            app.mouse_up();
            frame(app, c, None);
        };
        let n0 = mine(&app).len();

        // Draw three notes on C5 (48 ticks each, a beat apart).
        for t in [0.0, 96.0, 192.0] {
            click(&mut app, &mut c, g.x_of(t) + 2.0, row(60));
        }
        assert_eq!(mine(&app).len(), n0 + 3);

        // Letters switch tools; Select marquee over the first two notes.
        app.key(&Key::Character("e".into()), false, false);
        assert_eq!(app.state.piano.tool, Tool::Select);
        swipe(&mut app, &mut c, (g.x_of(0.0) + 1.0, row(61)), (g.x_of(120.0), row(59)), false);
        assert_eq!(app.state.piano.selection(app.state.pattern, ch).len(), 2);
        assert!(app.state.piano.overlay.is_none(), "marquee gone after release");

        // Shift+Right moves the selection one step; Ctrl+Up an octave; plain Right scrolls.
        app.shift = true;
        app.key(&Key::Named(NamedKey::ArrowRight), false, false);
        app.shift = false;
        app.key(&Key::Named(NamedKey::ArrowUp), true, false);
        let moved: Vec<(u32, u16)> = mine(&app).iter().map(|n| (n.position, n.key)).collect();
        assert!(moved.contains(&(24, 72)) && moved.contains(&(120, 72)) && moved.contains(&(192, 60)), "{moved:?}");
        app.key(&Key::Named(NamedKey::ArrowRight), false, false);
        assert!(app.state.piano.scroll_x > 0.0);
        app.state.piano.scroll_x = 0.0;
        frame(&mut app, &mut c, None);

        // Right button in Select acts like the left button (no delete).
        let before = mine(&app).len();
        app.mouse_move(g.x_of(200.0), row(60));
        app.mouse_down_right();
        app.mouse_up();
        frame(&mut app, &mut c, None);
        assert_eq!(mine(&app).len(), before);

        // Mute: click toggles; a swipe follows the first note's new state.
        app.key(&Key::Character("t".into()), false, false);
        click(&mut app, &mut c, g.x_of(200.0), row(60));
        assert!(mine(&app).iter().any(|n| n.position == 192 && n.muted()));
        swipe(&mut app, &mut c, (g.x_of(30.0), row(72)), (g.x_of(130.0), row(72)), false);
        assert_eq!(mine(&app).iter().filter(|n| n.key == 72 && n.muted()).count(), 2);

        // Paint: drag along D5 from tick 0 to past 288 -> notes 48 apart, key locked.
        app.key(&Key::Character("b".into()), false, false);
        swipe(&mut app, &mut c, (g.x_of(0.0) + 2.0, row(62)), (g.x_of(300.0), row(66)), false);
        let painted: Vec<u32> = mine(&app).iter().filter(|n| n.key == 62).map(|n| n.position).collect();
        assert_eq!(painted, vec![0, 48, 96, 144, 192, 240, 288]);

        // Slice: a line at tick 24 cuts the D5 note at 0 (48 long) in two.
        app.key(&Key::Character("c".into()), false, false);
        swipe(&mut app, &mut c, (g.x_of(24.0), row(70)), (g.x_of(24.0), row(55)), false);
        let d5: Vec<(u32, u32)> = mine(&app).iter().filter(|n| n.key == 62).map(|n| (n.position, n.length)).take(2).collect();
        assert_eq!(d5, vec![(0, 24), (24, 24)]);

        // Delete: swipe across the D5 row removes every note touched.
        app.key(&Key::Character("d".into()), false, false);
        swipe(&mut app, &mut c, (g.x_of(1.0), row(62)), (g.x_of(330.0), row(62)), false);
        assert_eq!(mine(&app).iter().filter(|n| n.key == 62).count(), 0);

        // Right button in another tool deletes while held.
        app.key(&Key::Character("p".into()), false, false);
        app.mouse_move(g.x_of(200.0), row(60));
        app.mouse_down_right();
        app.mouse_up();
        frame(&mut app, &mut c, None);
        assert!(!mine(&app).iter().any(|n| n.position == 192 && n.key == 60));

        // Ctrl+A, Ctrl+C, Ctrl+V: the marker was never placed, so the copy keeps the first note's place in its bar
        // (tick 0, onto the selected originals), and it is selected.
        let count = mine(&app).len();
        app.key(&Key::Character("a".into()), true, false);
        app.key(&Key::Character("c".into()), true, false);
        app.key(&Key::Character("v".into()), true, false);
        assert_eq!(mine(&app).len(), count * 2);
        assert_eq!(app.state.piano.selection(app.state.pattern, ch).len(), count);
        // Ctrl+B duplicates right after the selection's end.
        app.key(&Key::Character("b".into()), true, false);
        assert_eq!(mine(&app).len(), count * 3);
        assert_eq!(app.state.piano.tool, Tool::Draw, "Ctrl+letters don't switch tools");

        // Ruler (§10): left-drag moves the play-start marker (snapped to steps); right-drag
        // selects the notes starting in a time range, which is the only shaded selection.
        let ruler = app.hits.iter().find(|h| h.action == Action::PianoRuler).map(|h| h.rect).unwrap();
        let ry = ruler.cy();
        swipe(&mut app, &mut c, (g.x_of(0.0) + 1.0, ry), (g.x_of(190.0), ry), false);
        assert_eq!(app.state.piano.marker, 192);
        assert!(app.state.piano.marker_placed);
        assert_eq!(piano::paste_tick(&app.state, &[Note::step(0, 500)]), 192, "placed: paste at the marker");
        swipe(&mut app, &mut c, (g.x_of(0.0) + 1.0, ry), (g.x_of(100.0), ry), true);
        assert_eq!(app.state.piano.range, Some((0, 96)));
        let sel = app.state.piano.selection(app.state.pattern, ch);
        let notes = piano::notes(&app.state);
        assert!(!sel.is_empty() && sel.iter().all(|&i| notes[i].position < 96));
        app.key(&Key::Character("a".into()), true, false);
        assert_eq!(app.state.piano.range, Some((0, piano::pattern_end(&app.state))), "Ctrl+A: the whole pattern");
    }

    /// Ctrl+B with a time selection (spec: ui/piano-roll.md §9, §10): the copies move by the range's length, keeping
    /// the space before the notes, and the range moves on over them; without one, right after the notes' end.
    #[test]
    fn piano_duplicate_follows_the_time_selection() {
        let mut app = App::new(None);
        app.state.project = test_project();
        let ch = piano::channel(&app.state).expect("a channel");
        piano::pattern_mut(&mut app.state).notes = vec![Note { length: 24, ..Note::step(ch, 48) }];
        let starts = |app: &App| {
            let mut v: Vec<u32> = piano::notes(&app.state).iter().filter(|n| n.channel == ch).map(|n| n.position).collect();
            v.sort_unstable();
            v
        };
        // A bar-long range (0..384) with the note 48 ticks in: the copy lands 48 ticks into the next bar.
        piano::select_range(&mut app.state, 0, 384);
        crate::piano_tools::duplicate(&mut app.state);
        assert_eq!(starts(&app), [48, 432]);
        assert_eq!(app.state.piano.range, Some((384, 768)), "the range moves on");
        assert_eq!(app.state.piano.selection(app.state.pattern, Some(ch)).len(), 1, "the copy is selected");
        // Again: from the moved range.
        crate::piano_tools::duplicate(&mut app.state);
        assert_eq!(starts(&app), [48, 432, 816]);
        // An empty range only moves on.
        piano::select_range(&mut app.state, 1000, 1100);
        crate::piano_tools::duplicate(&mut app.state);
        assert_eq!(starts(&app).len(), 3);
        assert_eq!(app.state.piano.range, Some((1100, 1200)));
        // No range: right after the selection's end.
        app.state.piano.select(app.state.pattern, ch, vec![0]);
        crate::piano_tools::duplicate(&mut app.state);
        assert_eq!(starts(&app), [48, 72, 432, 816]);
    }

    /// The piano roll's keys as FL 26 does them (spec: ui/piano-keys.md §2, §4), sent the way main.rs sends a key (the
    /// typing keyboard first, then `App::key` with the repeat flag) to the project those measurements used.
    mod piano_keys {
        use super::*;
        use winit::keyboard::{Key, KeyCode, NamedKey};

        const CTRL: u8 = 1;
        const SHIFT: u8 = 2;
        const ALT: u8 = 4;

        /// A C5 E5 G5 chord at 0, two touching C6, two overlapping D6, an off-grid F5, D5 and C5 (pos, len, key).
        const PLAIN: [(u32, u32, u16); 10] =
            [(0, 96, 60), (0, 96, 64), (0, 96, 67), (384, 48, 72), (432, 48, 72), (480, 72, 74), (520, 72, 74), (581, 30, 65), (672, 24, 62), (800, 48, 60)];

        /// The piano roll alone with the project on channel 0, Line snap (24 ticks), the typing keyboard off and the mouse
        /// over the grid; painted once, as the arrows need the grid. None where no canvas can be made.
        fn setup() -> Option<(App, Canvas)> {
            let mut c = Canvas::new(1920, 1080, 1.0).ok()?;
            let mut app = App::new(None);
            app.state.project = test_project();
            app.state.z = vec![Win::PianoRoll];
            app.state.selected_channel = 0;
            app.state.piano.snap = crate::snap::EditorSnap::Grid(crate::playlist::Snap::Line);
            piano::pattern_mut(&mut app.state).notes = PLAIN.iter().map(|&(p, l, k)| Note { length: l, key: k, ..Note::step(0, p) }).collect();
            if app.typing_on() {
                app.toggle_typing();
            }
            frame(&mut app, &mut c, None);
            let grid = app.piano_geom().expect("the grid").grid;
            app.mouse_move(grid.cx(), grid.cy());
            assert_eq!(crate::piano_snap::unit(&app.state), Some(24));
            Some((app, c))
        }

        /// One key press (and its release) the way main.rs delivers it; `repeat` = an auto-repeat of a held key.
        fn press(app: &mut App, code: KeyCode, key: Key, mods: u8, repeat: bool) {
            let (ctrl, alt) = (mods & CTRL != 0, mods & ALT != 0);
            app.shift = mods & SHIFT != 0;
            if !app.typing_key(code, true, ctrl, alt) {
                app.repeat = repeat;
                app.key(&key, ctrl, alt);
                app.repeat = false;
            }
            app.typing_key(code, false, ctrl, alt);
            app.shift = false;
        }

        /// A letter key held for `n` key-downs (the first press and `n - 1` repeats).
        fn letter(app: &mut App, l: char, mods: u8, n: usize) {
            let code = match l {
                'a' => KeyCode::KeyA,
                'b' => KeyCode::KeyB,
                'c' => KeyCode::KeyC,
                'd' => KeyCode::KeyD,
                'e' => KeyCode::KeyE,
                'h' => KeyCode::KeyH,
                'i' => KeyCode::KeyI,
                'k' => KeyCode::KeyK,
                'l' => KeyCode::KeyL,
                'm' => KeyCode::KeyM,
                'p' => KeyCode::KeyP,
                'r' => KeyCode::KeyR,
                'u' => KeyCode::KeyU,
                'v' => KeyCode::KeyV,
                'z' => KeyCode::KeyZ,
                _ => unreachable!("add the key"),
            };
            let text = if mods & SHIFT != 0 { l.to_ascii_uppercase() } else { l };
            for i in 0..n {
                press(app, code, Key::Character(text.to_string().into()), mods, i > 0);
            }
        }

        fn named(app: &mut App, k: NamedKey, mods: u8, n: usize) {
            let code = match k {
                NamedKey::ArrowUp => KeyCode::ArrowUp,
                NamedKey::ArrowDown => KeyCode::ArrowDown,
                NamedKey::ArrowLeft => KeyCode::ArrowLeft,
                NamedKey::ArrowRight => KeyCode::ArrowRight,
                NamedKey::Delete => KeyCode::Delete,
                NamedKey::Enter => KeyCode::Enter,
                _ => unreachable!("add the key"),
            };
            for i in 0..n {
                press(app, code, Key::Named(k), mods, i > 0);
            }
        }

        /// Channel 0's notes as (pos, len, key), sorted.
        fn spans(app: &App) -> Vec<(u32, u32, u16)> {
            let mut v: Vec<(u32, u32, u16)> = piano::notes(&app.state).iter().filter(|n| n.channel == 0).map(|n| (n.position, n.length, n.key)).collect();
            v.sort_unstable();
            v
        }

        fn select(app: &mut App, pos: u32, key: u16) {
            let i = piano::notes(&app.state).iter().position(|n| n.position == pos && n.key == key).expect("the note");
            app.state.piano.select(app.state.pattern, 0, vec![i]);
        }

        fn selected(app: &App) -> Vec<(u32, u16)> {
            let all = piano::notes(&app.state);
            let mut v: Vec<(u32, u16)> = app.state.piano.selection(app.state.pattern, Some(0)).iter().map(|&i| (all[i].position, all[i].key)).collect();
            v.sort_unstable();
            v
        }

        fn moved(dt: i64, dk: i32) -> Vec<(u32, u32, u16)> {
            let mut v: Vec<(u32, u32, u16)> = PLAIN.iter().map(|&(p, l, k)| ((i64::from(p) + dt) as u32, l, (i32::from(k) + dk) as u16)).collect();
            v.sort_unstable();
            v
        }

        /// Held keys (§4 rule 1): FL's 5 key-downs of Shift+Up = 5 semitones, 4 of Shift+Right = 4 steps, 3 of Ctrl+B =
        /// 3 copies; a held switch acts once; nothing repeats into the channel rack.
        #[test]
        fn held_keys_repeat_the_moving_and_copying_keys() {
            let Some((mut app, _c)) = setup() else { return };
            named(&mut app, NamedKey::ArrowUp, SHIFT, 5);
            assert_eq!(spans(&app), moved(0, 5));
            named(&mut app, NamedKey::ArrowRight, SHIFT, 4);
            assert_eq!(spans(&app), moved(96, 5));
            named(&mut app, NamedKey::ArrowDown, CTRL, 2);
            assert_eq!(spans(&app), moved(96, -19));
            select(&mut app, 896, 41);
            letter(&mut app, 'b', CTRL, 3);
            let copies: Vec<u32> = spans(&app).iter().filter(|n| n.2 == 41 && n.0 >= 800).map(|n| n.0).collect();
            assert_eq!(copies, [896, 944, 992, 1040]);
            // L (PAT / SONG) held: one flip; Ctrl+L held: one legato.
            let song = app.state.song_mode;
            letter(&mut app, 'l', 0, 3);
            assert_eq!(app.state.song_mode, !song);
            letter(&mut app, 'l', 0, 1);
            app.state.piano.clear_selection();
            letter(&mut app, 'l', CTRL, 1);
            let once = spans(&app);
            app.repeat = true;
            app.shift = false;
            app.key(&Key::Character("l".into()), true, false);
            app.repeat = false;
            assert_eq!(spans(&app), once, "a repeat of Ctrl+L does nothing");
            // The channel rack in front takes the keys: no repeats reach the notes.
            app.state.z = vec![Win::PianoRoll, Win::ChannelRack];
            app.repeat = true;
            app.shift = true;
            app.key(&Key::Named(NamedKey::ArrowUp), false, false);
            app.repeat = false;
            app.shift = false;
            assert_eq!(spans(&app), once);
        }

        /// Delete with nothing selected, Ctrl+A's time selection with Ctrl+B and Ctrl+D, Ctrl+Enter (§4 rules 3, 4, 12).
        #[test]
        fn delete_select_all_and_ctrl_enter() {
            let Some((mut app, _c)) = setup() else { return };
            named(&mut app, NamedKey::Delete, 0, 1);
            assert!(spans(&app).is_empty(), "nothing selected: every note goes");
            letter(&mut app, 'z', CTRL, 1);
            assert_eq!(spans(&app), moved(0, 0), "one undo step");
            letter(&mut app, 'a', CTRL, 1);
            assert_eq!(selected(&app).len(), 10);
            assert_eq!(app.state.piano.range, Some((0, 1152)), "bars 1 to 3: the notes end at 848");
            letter(&mut app, 'b', CTRL, 1);
            let mut both = moved(0, 0);
            both.extend(moved(1152, 0));
            both.sort_unstable();
            assert_eq!(spans(&app), both, "the copies move by the pattern's length in bars");
            letter(&mut app, 'd', CTRL, 1);
            assert!(selected(&app).is_empty());
            assert_eq!(app.state.piano.range, None);
            select(&mut app, 800, 60);
            named(&mut app, NamedKey::Enter, CTRL, 1);
            assert_eq!((app.state.piano.range, app.state.piano.marker), (Some((800, 848)), 800));
            assert_eq!(selected(&app), [(800, 60)]);
        }

        /// Ctrl+L, Ctrl+U and Ctrl+V as measured (§2 "Editing", "Paste"; §4 rules 5 to 7).
        #[test]
        fn legato_chop_and_paste() {
            let Some((mut app, _c)) = setup() else { return };
            letter(&mut app, 'l', CTRL, 1);
            let s = spans(&app);
            assert!(s.contains(&(0, 384, 60)) && s.contains(&(581, 91, 65)), "{s:?}");
            assert!(s.contains(&(800, 352, 60)), "the last note: to the end of bar 3");
            letter(&mut app, 'z', CTRL, 1);
            letter(&mut app, 'u', CTRL, 1);
            let s = spans(&app);
            assert!(s.contains(&(581, 24, 65)) && s.contains(&(605, 6, 65)), "{s:?}");
            assert!([520, 544, 568].iter().all(|&t| s.contains(&(t, 24, 74))), "{s:?}");
            letter(&mut app, 'z', CTRL, 1);
            assert_eq!(spans(&app), moved(0, 0));

            // The marker never placed: the note's place in its bar, in bar 1; a second paste onto the first moves a unit.
            select(&mut app, 432, 72);
            letter(&mut app, 'c', CTRL, 1);
            letter(&mut app, 'd', CTRL, 1);
            letter(&mut app, 'v', CTRL, 1);
            assert_eq!(selected(&app), [(48, 72)]);
            letter(&mut app, 'd', CTRL, 1);
            letter(&mut app, 'v', CTRL, 1);
            assert_eq!(selected(&app), [(72, 72)]);
            // C5 from 800 lands at 32, inside the chord's C5: one unit later.
            select(&mut app, 800, 60);
            letter(&mut app, 'c', CTRL, 1);
            letter(&mut app, 'd', CTRL, 1);
            letter(&mut app, 'v', CTRL, 1);
            assert_eq!(selected(&app), [(56, 60)]);
            // Pasting again while that copy is still selected lands on it.
            letter(&mut app, 'v', CTRL, 1);
            assert_eq!(selected(&app), [(56, 60)]);
            assert_eq!(spans(&app).iter().filter(|n| n.0 == 56).count(), 2);
            // A placed marker: there.
            app.state.piano.marker = 480;
            app.state.piano.marker_placed = true;
            letter(&mut app, 'v', CTRL, 1);
            assert_eq!(selected(&app), [(480, 60)]);
        }

        /// Shift+E / Shift+P and the other tool letters switch tools with or without the typing keyboard, which takes the
        /// plain letters as notes; Shift+C and Shift+D keep their editing commands (§4 rule 16).
        #[test]
        fn shift_letters_switch_tools() {
            let Some((mut app, _c)) = setup() else { return };
            letter(&mut app, 'e', 0, 1);
            assert_eq!(app.state.piano.tool, Tool::Select, "typing keyboard off: E");
            letter(&mut app, 'p', 0, 1);
            assert_eq!(app.state.piano.tool, Tool::Draw, "typing keyboard off: P");
            letter(&mut app, 'e', SHIFT, 1);
            assert_eq!(app.state.piano.tool, Tool::Select, "typing keyboard off: Shift+E");
            letter(&mut app, 'p', SHIFT, 1);
            assert_eq!(app.state.piano.tool, Tool::Draw, "typing keyboard off: Shift+P");

            app.toggle_typing();
            assert!(app.typing_on());
            letter(&mut app, 'e', 0, 1);
            assert_eq!(app.state.piano.tool, Tool::Draw, "typing keyboard on: E plays a note");
            letter(&mut app, 'e', SHIFT, 1);
            assert_eq!(app.state.piano.tool, Tool::Select, "typing keyboard on: Shift+E");
            letter(&mut app, 'p', SHIFT, 1);
            assert_eq!(app.state.piano.tool, Tool::Draw, "typing keyboard on: Shift+P");
            letter(&mut app, 'b', SHIFT, 1);
            assert_eq!(app.state.piano.tool, Tool::Paint);
            letter(&mut app, 'z', SHIFT, 1);
            assert_eq!(app.state.piano.tool, Tool::Zoom);

            letter(&mut app, 'p', SHIFT, 1);
            letter(&mut app, 'd', SHIFT, 1);
            assert_eq!(app.state.piano.tool, Tool::Draw, "Shift+D discards lengths");
            assert!(spans(&app).iter().all(|&(_, l, _)| l == 24), "one snap unit long: {:?}", spans(&app));
            letter(&mut app, 'c', SHIFT, 1);
            assert_eq!(app.state.piano.tool, Tool::Draw, "Shift+C selects a colour");
            assert_eq!(selected(&app).len(), 10);
        }

        /// Alt+arrows, rotation, mute, the selection keys and Shift+K / Shift+H (§4 rules 8 to 11, 13, 14).
        #[test]
        fn alt_arrows_rotate_mute_and_select() {
            let Some((mut app, _c)) = setup() else { return };
            named(&mut app, NamedKey::ArrowRight, ALT, 2);
            assert_eq!(spans(&app), moved(2, 0));
            named(&mut app, NamedKey::ArrowUp, ALT, 1);
            assert_eq!(spans(&app), moved(2, 0), "Alt+Up does nothing");
            named(&mut app, NamedKey::ArrowLeft, ALT, 3);
            assert_eq!(spans(&app), moved(0, 0), "Alt+Left stops at 0");
            named(&mut app, NamedKey::ArrowUp, CTRL | SHIFT, 1);
            assert_eq!(spans(&app), moved(0, 0), "Ctrl+Shift+Up moves no notes");
            named(&mut app, NamedKey::ArrowLeft, CTRL | SHIFT, 1);
            let s = spans(&app);
            assert!(s.contains(&(1128, 96, 60)) && s.contains(&(776, 48, 60)), "the chord wraps to 1152 - 24: {s:?}");
            letter(&mut app, 'z', CTRL, 1);
            assert_eq!(spans(&app), moved(0, 0));

            letter(&mut app, 'm', ALT, 1);
            assert!(piano::notes(&app.state).iter().all(|n| n.muted()), "nothing selected: all");
            select(&mut app, 800, 60);
            letter(&mut app, 'm', ALT | SHIFT, 1);
            assert_eq!(piano::notes(&app.state).iter().filter(|n| n.muted()).count(), 9);

            letter(&mut app, 'i', SHIFT, 1);
            assert_eq!(selected(&app).len(), 9);
            assert!(!selected(&app).contains(&(800, 60)));
            letter(&mut app, 'r', SHIFT, 1);
            assert_eq!(selected(&app).len(), 1);
            letter(&mut app, 'm', SHIFT, 1);
            assert!((1..=10).contains(&selected(&app).len()));
            letter(&mut app, 'c', SHIFT, 1);
            assert_eq!(selected(&app).len(), 10, "one colour: all");
            assert_eq!(app.state.piano.tool, Tool::Draw, "Shift+C is no tool letter");

            // Shift+K / Shift+H work with the typing keyboard on.
            app.toggle_typing();
            assert!(app.typing_on());
            letter(&mut app, 'k', SHIFT, 1);
            assert_eq!(app.state.selected_channel, 1);
            letter(&mut app, 'h', SHIFT, 1);
            assert_eq!(app.state.selected_channel, 0, "the next channel with notes, wrapping");
        }
    }

    /// The piano roll title's target channel (spec: ui/piano-target-channel.md): hint, the channel menu and a pick,
    /// the wheel through the channels with notes, right-click for the channel's window.
    /// "Enter value..." (spec: ui/type-in-value.md): the knob menu's item opens the value box at the mouse; typed text
    /// sets the knob on Enter, Esc leaves it; the box is modal; the mode buttons switch and the accepted mode is kept.
    #[test]
    fn type_in_value() {
        use crate::knobs::KnobId;
        use crate::playlist::EditTarget;
        use crate::typein::{Target, TiAct};
        use winit::keyboard::{Key, NamedKey, SmolStr};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.z = vec![Win::ChannelRack];
        frame(&mut app, &mut c, None);
        let typ = |app: &mut App, text: &str| {
            for ch in text.chars() {
                let k = if ch == ' ' { Key::Named(NamedKey::Space) } else { Key::Character(SmolStr::new(ch.to_string())) };
                app.key(&k, false, false);
            }
        };
        let enter = |app: &mut App| app.key(&Key::Named(NamedKey::Enter), false, false);
        // Right-click the first row's volume knob, pick "Enter value...".
        let open = |app: &mut App, c: &mut Canvas, k: KnobId| {
            let r = hit_rect(app, &Action::Knob(k));
            app.mouse_move(r.cx(), r.cy());
            app.mouse_down_right();
            app.mouse_up();
            let menu = app.state.playlist.popup.clone().expect("knob menu");
            let i = menu.items.iter().position(|i| i.label == "Enter value...").expect("item");
            assert!(menu.items[i].enabled);
            app.mouse_move(r.cx() + 40.0, r.cy() + 300.0);
            app.act(Action::PlPopup(i));
            frame(app, c, None);
        };
        let vol = KnobId::ChanVol(0);
        open(&mut app, &mut c, vol);
        let e = app.state.playlist.edit.clone().expect("value box");
        assert_eq!(e.target, EditTarget::TypeIn(Target::Knob(vol), false));
        assert!(e.sel, "the field opens selected");
        assert_eq!(e.text, crate::typein::text(&app.state, Target::Knob(vol), false));
        assert!(e.text.ends_with("dB"), "{}", e.text);
        // Bottom-right corner 10 px right of / below the mouse (§1).
        let (mx, my) = app.mouse;
        assert_eq!((e.at.0 + crate::typein::W, e.at.1 + crate::typein::H), (mx + 10.0, my + 10.0));
        assert_eq!(crate::typein::caption(&app.state, Target::Knob(vol)), "Value for Channel volume");
        // Typing replaces the selected text; Enter sets -6 dB.
        typ(&mut app, "-6 dB");
        assert_eq!(app.state.playlist.edit.as_ref().unwrap().text, "-6 dB");
        enter(&mut app);
        assert!(app.state.playlist.edit.is_none());
        assert!(app.state.knob_text(vol).contains("-6.0dB"), "{}", app.state.knob_text(vol));
        // Esc leaves the value; unreadable text too (OS).
        frame(&mut app, &mut c, None);
        let before = app.state.knob(vol);
        open(&mut app, &mut c, vol);
        typ(&mut app, "-20");
        app.key(&Key::Named(NamedKey::Escape), false, false);
        assert!(app.state.playlist.edit.is_none());
        assert_eq!(app.state.knob(vol), before);
        frame(&mut app, &mut c, None);
        open(&mut app, &mut c, vol);
        typ(&mut app, "abc");
        enter(&mut app);
        assert_eq!(app.state.knob(vol), before);
        // Modal: a click elsewhere keeps the box; the A button switches to normalized, ✓ accepts and the mode stays.
        frame(&mut app, &mut c, None);
        open(&mut app, &mut c, vol);
        app.mouse_move(5.0, 1000.0);
        app.mouse_down();
        app.mouse_up();
        assert!(app.state.playlist.edit.is_some(), "modal");
        frame(&mut app, &mut c, None);
        let a = hit_rect(&app, &Action::TypeIn(TiAct::Mode(true)));
        click(&mut app, &mut c, a);
        let e = app.state.playlist.edit.clone().unwrap();
        assert_eq!(e.target, EditTarget::TypeIn(Target::Knob(vol), true));
        assert!(e.sel);
        typ(&mut app, "0.5");
        let ok = hit_rect(&app, &Action::TypeIn(TiAct::Accept));
        click(&mut app, &mut c, ok);
        assert!(app.state.playlist.edit.is_none());
        assert!((app.state.knob(vol) - 0.5).abs() < 1e-6);
        assert!(app.state.playlist.typein_norm, "the accepted mode is kept");
        // The pan box then opens in A; switch back to the value mode and type a pan.
        let pan = KnobId::ChanPan(0);
        open(&mut app, &mut c, pan);
        assert_eq!(app.state.playlist.edit.as_ref().unwrap().text, "0.5");
        let v = hit_rect(&app, &Action::TypeIn(TiAct::Mode(false)));
        click(&mut app, &mut c, v);
        assert_eq!(app.state.playlist.edit.as_ref().unwrap().text, "Centered");
        typ(&mut app, "25% left");
        enter(&mut app);
        assert_eq!(app.state.knob_text(pan), "Channel panning: 25% left");
        assert!(!app.state.playlist.typein_norm);
        // ✗ closes.
        frame(&mut app, &mut c, None);
        open(&mut app, &mut c, pan);
        typ(&mut app, "50");
        let x = hit_rect(&app, &Action::TypeIn(TiAct::Close));
        click(&mut app, &mut c, x);
        assert!(app.state.playlist.edit.is_none());
        assert_eq!(app.state.knob_text(pan), "Channel panning: 25% left");
        // Accepting the field as it opened keeps the exact value.
        app.state.set_knob(pan, 0.6234);
        frame(&mut app, &mut c, None);
        open(&mut app, &mut c, pan);
        enter(&mut app);
        assert_eq!(app.state.knob(pan), 0.6234);
    }
    /// "Enter value..." from the toolbar's and the mixer's short menus (spec: ui/type-in-value.md §6, §8): master
    /// pitch's Hz box, a mixer fader in dB, the mixer pan's menu keeps its automation part.
    #[test]
    fn type_in_value_toolbar_and_mixer() {
        use crate::knobs::KnobId;
        use crate::playlist::EditTarget;
        use crate::typein::Target;
        use winit::keyboard::{Key, NamedKey, SmolStr};
        let Ok(mut c) = Canvas::new(1920, 1080, 1.0) else { return };
        let mut app = App::new(None);
        app.state.project = test_project();
        app.state.z = vec![Win::Mixer];
        frame(&mut app, &mut c, None);
        let typ = |app: &mut App, text: &str| {
            for ch in text.chars() {
                let k = if ch == ' ' { Key::Named(NamedKey::Space) } else { Key::Character(SmolStr::new(ch.to_string())) };
                app.key(&k, false, false);
            }
            app.key(&Key::Named(NamedKey::Enter), false, false);
        };
        // Right-click `a`, check the menu's labels, pick "Enter value...".
        let open = |app: &mut App, c: &mut Canvas, a: Action, auto: bool| {
            let r = hit_rect(app, &a);
            app.mouse_move(r.cx(), r.cy());
            app.mouse_down_right();
            app.mouse_up();
            let menu = app.state.playlist.popup.clone().expect("knob menu");
            assert_eq!(menu.items.iter().any(|i| i.label == "Create automation clip"), auto, "{a:?}");
            let i = menu.items.iter().position(|i| i.label == "Enter value...").expect("item");
            assert!(menu.items[i].enabled);
            app.act(Action::PlPopup(i));
            frame(app, c, None);
            app.state.playlist.edit.clone().expect("value box")
        };
        let pitch = Target::Knob(KnobId::MasterPitch);
        let e = open(&mut app, &mut c, Action::Knob(KnobId::MasterPitch), false);
        assert_eq!(e.target, EditTarget::TypeIn(pitch, false));
        assert_eq!(e.text, "440");
        assert_eq!(crate::typein::width(pitch), crate::typein::W_PLAIN);
        assert_eq!(crate::typein::caption(&app.state, pitch), "Master pitch (Hz)");
        typ(&mut app, "466.16");
        assert_eq!(app.state.knob_text(KnobId::MasterPitch), "Master pitch: +100 cents");
        frame(&mut app, &mut c, None);
        open(&mut app, &mut c, Action::Knob(KnobId::MasterVol), false);
        typ(&mut app, "-6");
        assert!(app.state.knob_text(KnobId::MasterVol).contains("-6.0dB"), "{}", app.state.knob_text(KnobId::MasterVol));
        // A mixer fader: dB through the fader law, clamped to its top.
        frame(&mut app, &mut c, None);
        let e = open(&mut app, &mut c, Action::Fader(1), true);
        assert_eq!(e.target, EditTarget::TypeIn(Target::Fader(1), false));
        assert_eq!(e.text, "0.0dB");
        assert_eq!(crate::typein::caption(&app.state, Target::Fader(1)), "Value for Bus 1 - Volume");
        typ(&mut app, "-6");
        let vol = |app: &App| crate::mixer::track(&app.state, 1).unwrap().volume.unwrap();
        assert!(crate::mixer::level_text("Volume", vol(&app)).contains("-6.0dB"));
        frame(&mut app, &mut c, None);
        open(&mut app, &mut c, Action::Fader(1), true);
        typ(&mut app, "10");
        assert_eq!(crate::mixer::level_text("Volume", vol(&app)), "Volume: 125%  5.6dB  1.90");
        // The mixer pan and stereo separation.
        frame(&mut app, &mut c, None);
        open(&mut app, &mut c, Action::Knob(KnobId::MixPan(2)), true);
        typ(&mut app, "40 left");
        assert_eq!(app.state.knob_text(KnobId::MixPan(2)), "Panning: 40% left");
        frame(&mut app, &mut c, None);
        open(&mut app, &mut c, Action::Knob(KnobId::MixSep(2)), false);
        typ(&mut app, "50");
        assert!(app.state.knob_text(KnobId::MixSep(2)).ends_with("50% merged"), "{}", app.state.knob_text(KnobId::MixSep(2)));
    }

