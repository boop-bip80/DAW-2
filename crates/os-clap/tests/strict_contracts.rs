// Copyright (C) 2026 Lemon Studio contributors
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Strict in-memory CLAP protocol regression; no installed plugins or audio hardware.
use std::ffi::{c_char, c_void, CStr};
use std::path::Path;
use std::ptr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::{clap_input_events, clap_output_events, clap_event_param_value, CLAP_EVENT_PARAM_VALUE};
use clap_sys::ext::params::{clap_host_params, clap_plugin_params, clap_param_info, CLAP_EXT_PARAMS, CLAP_PARAM_RESCAN_ALL};
use clap_sys::ext::thread_check::{clap_host_thread_check, CLAP_EXT_THREAD_CHECK};
use clap_sys::factory::plugin_factory::{clap_plugin_factory, CLAP_PLUGIN_FACTORY_ID};
use clap_sys::host::clap_host;
use clap_sys::plugin::{clap_plugin, clap_plugin_descriptor};
use clap_sys::process::{clap_process, clap_process_status, CLAP_PROCESS_CONTINUE};
use clap_sys::version::CLAP_VERSION;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static REVERSED: AtomicBool = AtomicBool::new(false);
static LAST_HOST: AtomicUsize = AtomicUsize::new(0);
static PARAMS_SEEN: Mutex<Vec<u32>> = Mutex::new(Vec::new());
static BAD_FLUSH: AtomicU32 = AtomicU32::new(0);
static BAD_STOP: AtomicU32 = AtomicU32::new(0);
static GOOD_START: AtomicU32 = AtomicU32::new(0);
static GOOD_PROCESS: AtomicU32 = AtomicU32::new(0);

struct Static<T>(T);
unsafe impl<T> Sync for Static<T> {}
static FEATURES: Static<[*const c_char; 2]> = Static([c"instrument".as_ptr(), ptr::null()]);
static DESC: Static<clap_plugin_descriptor> = Static(clap_plugin_descriptor {
    clap_version: CLAP_VERSION,
    id: c"org.audit.strict-thread".as_ptr(),
    name: c"Audit Strict Thread".as_ptr(),
    vendor: c"Audit".as_ptr(),
    url: c"".as_ptr(),
    manual_url: c"".as_ptr(),
    support_url: c"".as_ptr(),
    version: c"1".as_ptr(),
    description: c"Records CLAP thread contract violations without crashing".as_ptr(),
    features: FEATURES.0.as_ptr(),
});

unsafe fn on_audio(p: *const clap_plugin) -> bool {
    let h = unsafe { (*p).plugin_data.cast::<clap_host>() };
    let ext = unsafe { ((*h).get_extension.unwrap())(h, CLAP_EXT_THREAD_CHECK.as_ptr()) }.cast::<clap_host_thread_check>();
    unsafe { ((*ext).is_audio_thread.unwrap())(h) }
}
unsafe extern "C" fn init(_: *const clap_plugin) -> bool { true }
unsafe extern "C" fn destroy(p: *const clap_plugin) { drop(unsafe { Box::from_raw(p as *mut clap_plugin) }); }
unsafe extern "C" fn activate(_: *const clap_plugin, _: f64, _: u32, _: u32) -> bool { ACTIVE.store(true, Ordering::Release); true }
unsafe extern "C" fn deactivate(_: *const clap_plugin) { ACTIVE.store(false, Ordering::Release); }
unsafe extern "C" fn nothing(_: *const clap_plugin) {}
unsafe extern "C" fn start(p: *const clap_plugin) -> bool {
    if unsafe { on_audio(p) } { GOOD_START.fetch_add(1, Ordering::Relaxed); }
    true
}
unsafe extern "C" fn stop(p: *const clap_plugin) {
    if !unsafe { on_audio(p) } { BAD_STOP.fetch_add(1, Ordering::Relaxed); }
}
unsafe extern "C" fn process(p: *const clap_plugin, process: *const clap_process) -> clap_process_status {
    if unsafe { on_audio(p) } { GOOD_PROCESS.fetch_add(1, Ordering::Relaxed); }
    let ins = unsafe { (*process).in_events };
    for i in 0..unsafe { ((*ins).size.unwrap())(ins) } {
        let event = unsafe { ((*ins).get.unwrap())(ins, i) };
        if unsafe { (*event).type_ } == CLAP_EVENT_PARAM_VALUE {
            PARAMS_SEEN.lock().unwrap().push(unsafe { (*(event.cast::<clap_event_param_value>())).param_id });
        }
    }
    CLAP_PROCESS_CONTINUE
}
unsafe extern "C" fn param_count(_: *const clap_plugin) -> u32 { 2 }
unsafe extern "C" fn param_info(_: *const clap_plugin, i: u32, out: *mut clap_param_info) -> bool {
    if i > 1 { return false; }
    let out = unsafe { &mut *out };
    out.id = if (i == 0) != REVERSED.load(Ordering::Acquire) { 42 } else { 73 };
    out.min_value = 0.0;
    out.max_value = 1.0;
    out.default_value = 0.0;
    out.name[0] = b'P' as c_char;
    true
}
unsafe extern "C" fn param_value(_: *const clap_plugin, _: u32, out: *mut f64) -> bool { unsafe { *out = 0.0 }; true }
unsafe extern "C" fn flush(p: *const clap_plugin, _: *const clap_input_events, _: *const clap_output_events) {
    if unsafe { on_audio(p) } != ACTIVE.load(Ordering::Acquire) { BAD_FLUSH.fetch_add(1, Ordering::Relaxed); }
}
static PARAMS: clap_plugin_params = clap_plugin_params {
    count: Some(param_count), get_info: Some(param_info), get_value: Some(param_value),
    value_to_text: None, text_to_value: None, flush: Some(flush),
};
unsafe extern "C" fn extension(_: *const clap_plugin, id: *const c_char) -> *const c_void {
    if unsafe { CStr::from_ptr(id) } == CLAP_EXT_PARAMS { (&PARAMS as *const clap_plugin_params).cast() } else { ptr::null() }
}
unsafe extern "C" fn count(_: *const clap_plugin_factory) -> u32 { 1 }
unsafe extern "C" fn descriptor(_: *const clap_plugin_factory, _: u32) -> *const clap_plugin_descriptor { &DESC.0 }
unsafe extern "C" fn create(_: *const clap_plugin_factory, host: *const clap_host, _: *const c_char) -> *const clap_plugin {
    LAST_HOST.store(host as usize, Ordering::Release);
    Box::into_raw(Box::new(clap_plugin {
        desc: &DESC.0, plugin_data: host as *mut c_void, init: Some(init), destroy: Some(destroy),
        activate: Some(activate), deactivate: Some(deactivate), start_processing: Some(start), stop_processing: Some(stop),
        reset: Some(nothing), process: Some(process), get_extension: Some(extension), on_main_thread: Some(nothing),
    }))
}
static FACTORY: clap_plugin_factory = clap_plugin_factory {
    get_plugin_count: Some(count), get_plugin_descriptor: Some(descriptor), create_plugin: Some(create),
};
unsafe extern "C" fn entry_init(_: *const c_char) -> bool { true }
unsafe extern "C" fn entry_deinit() {}
unsafe extern "C" fn factory(id: *const c_char) -> *const c_void {
    if unsafe { CStr::from_ptr(id) } == CLAP_PLUGIN_FACTORY_ID { (&FACTORY as *const clap_plugin_factory).cast() } else { ptr::null() }
}
static ENTRY: clap_plugin_entry = clap_plugin_entry {
    clap_version: CLAP_VERSION, init: Some(entry_init), deinit: Some(entry_deinit), get_factory: Some(factory),
};

#[test]
fn active_callbacks_use_the_audio_role_and_parameter_order_survives_rescans() {
    os_clap::set_main_thread();
    let lib = unsafe { os_clap::Library::from_entry((&ENTRY as *const clap_plugin_entry).cast(), Path::new("audit-thread.clap")) }.unwrap();
    let plugin = os_host::Library::create(lib, "org.audit.strict-thread").unwrap();
    {
        let mut p = plugin.lock().unwrap();
        assert!(p.set_param(42, 0.25));
        p.idle(); // Inactive flush must remain a main-thread callback.
        assert!(!os_engine::parallel::on_audio_thread());
        assert!(p.activate(48000));
    }
    let audio_plugin = plugin.clone();
    std::thread::spawn(move || {
        let _audio = os_engine::parallel::AudioScope::enter();
        let mut p = audio_plugin.lock().unwrap();
        let (mut l, mut r) = ([0.0; 64], [0.0; 64]);
        p.note_on(0, 60, 1.0);
        p.process(48000, &mut l, &mut r);
    }).join().unwrap();
    {
        let mut p = plugin.lock().unwrap();
        assert!(p.set_param(42, 0.5));
        p.idle();
        assert!(p.activate(44100));
    }
    {
        let mut p = plugin.lock().unwrap();
        assert!(p.restore_parameter_ids(&[73, 42]));
        assert_eq!(p.param_info(0).unwrap().id, 73);
        let revision = p.param_revision();
        REVERSED.store(true, Ordering::Release);
        let host = LAST_HOST.load(Ordering::Acquire) as *const clap_host;
        let params = unsafe { ((*host).get_extension.unwrap())(host, CLAP_EXT_PARAMS.as_ptr()) }.cast::<clap_host_params>();
        unsafe { ((*params).rescan.unwrap())(host, CLAP_PARAM_RESCAN_ALL) };
        p.idle();
        assert_eq!(p.parameter_ids(), [73, 42]);
        assert_eq!(p.param_info(0).unwrap().id, 73);
        assert_ne!(revision, p.param_revision());
        p.param(0, 0, 0.75);
        PARAMS_SEEN.lock().unwrap().clear();
        let (mut l, mut r) = ([0.0; 64], [0.0; 64]);
        {
            let _audio = os_engine::parallel::AudioScope::enter();
            p.process(44100, &mut l, &mut r);
        }
        assert_eq!(*PARAMS_SEEN.lock().unwrap(), [73]);
        assert!(!os_engine::parallel::on_audio_thread());
    }
    drop(plugin); // stop_processing during destruction is also an audio callback.
    let start = GOOD_START.load(Ordering::Relaxed);
    let process = GOOD_PROCESS.load(Ordering::Relaxed);
    let flush = BAD_FLUSH.load(Ordering::Relaxed);
    let stop = BAD_STOP.load(Ordering::Relaxed);
    println!("valid audio-thread start_processing={start}; valid audio-thread process={process}");
    println!("CONTRACT VIOLATIONS: active params.flush on non-audio thread={flush}; stop_processing on non-audio thread={stop}");
    assert!(start > 0 && process > 0, "control path must actually process on audio thread");
    assert_eq!((flush, stop), (0, 0), "CLAP callback thread contracts must hold");
}
