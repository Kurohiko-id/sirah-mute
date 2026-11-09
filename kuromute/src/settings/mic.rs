//! Mic tab glue: mic dropdown, hotkey record/pick, test mute.
use super::model::{hotkey_choice, hotkey_warning};
use super::record::{self, RecordEvent, Recorder};
use super::ui::{MainWindow, State, commit, send_to_core};
use crate::audio::{Audio, Flow};
use crate::config::Hotkey;
use crate::hotkey::{self, HotkeyError, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, SELECTABLE_KEYS};
use crate::i18n::{self, Key};
use crate::msg::{MSG_PAUSE_HOTKEY, MSG_TOGGLE};
use slint::{ComponentHandle, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::Threading::GetCurrentProcessId;

/// Recording session; dropping the `Recorder` removes the Raw Input registration.
pub struct Mic {
    recorder: Rc<RefCell<Option<Recorder>>>,
}

impl Mic {
    /// Called after the window closes: never leave the core's hotkey paused.
    pub fn finish(self) {
        if self.recorder.borrow_mut().take().is_some() {
            resume();
        }
    }
}

fn model(items: Vec<String>) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        items
            .into_iter()
            .map(SharedString::from)
            .collect::<Vec<_>>(),
    ))
}

fn resume() {
    send_to_core(MSG_PAUSE_HOTKEY, 0, 0);
}

/// Dropdown entries for `vk`: SELECTABLE_KEYS, plus the current key if it is not among them.
fn key_list(vk: u32) -> Vec<u32> {
    let mut v = SELECTABLE_KEYS.to_vec();
    if !v.contains(&vk) {
        v.push(vk);
    }
    v
}

fn show_hotkey(w: &MainWindow, h: &Hotkey) {
    let keys = key_list(h.vk);
    let names = keys
        .iter()
        .map(|&vk| hotkey::display(&Hotkey { mods: 0, vk }))
        .collect();
    w.set_key_names(model(names));
    w.set_key_index(
        keys.iter()
            .position(|&k| k == h.vk)
            .map_or(-1, |i| i as i32),
    );
    w.set_hotkey_text(hotkey::display(h).into());
    w.set_mod_ctrl(h.mods & MOD_CONTROL != 0);
    w.set_mod_alt(h.mods & MOD_ALT != 0);
    w.set_mod_shift(h.mods & MOD_SHIFT != 0);
    w.set_mod_win(h.mods & MOD_WIN != 0);
    w.set_hotkey_warn(hotkey_warning(h));
}

/// Validates locally; saves only a valid, changed hotkey.
fn choose(w: &MainWindow, state: &Rc<RefCell<State>>, mods: u32, vk: u32) {
    let lang = i18n::resolve(state.borrow().cfg.language);
    match hotkey_choice(mods, vk) {
        Ok(h) => {
            w.set_hotkey_error("".into());
            let mut st = state.borrow_mut();
            if st.cfg.hotkey != h {
                st.cfg.hotkey = h;
                commit(&st, w);
            }
            show_hotkey(w, &st.cfg.hotkey);
        }
        Err(e) => {
            let k = match e {
                HotkeyError::BareF12 => Key::HotkeyBareF12,
                HotkeyError::ModifierOnly | HotkeyError::Empty => Key::HotkeyModOnly,
            };
            w.set_hotkey_error(i18n::t(lang, k).into());
        }
    }
}

fn setup_mics(w: &MainWindow, state: &Rc<RefCell<State>>, sink: HWND) {
    let lang = i18n::resolve(state.borrow().cfg.language);
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let all = match Audio::new(sink) {
        Ok(a) => a.list(Flow::Capture),
        Err(e) => {
            crate::log!("settings: audio init failed: {e}");
            Vec::new()
        }
    };
    let mut ids = Vec::new();
    let mut names = Vec::new();
    for d in all.iter().filter(|d| d.present) {
        ids.push(d.id.clone());
        names.push(d.name.clone());
    }
    let saved = state.borrow().cfg.mic_id.clone();
    if let Some(id) = saved.filter(|id| !ids.contains(id)) {
        let name = all
            .iter()
            .find(|d| d.id == id)
            .map_or(id.as_str(), |d| d.name.as_str());
        names.push(format!("{name} {}", i18n::t(lang, Key::MicNotConnected)));
        ids.push(id.clone());
    }
    let cur = state.borrow().cfg.mic_id.clone();
    w.set_mic_names(model(names));
    w.set_mic_index(
        cur.and_then(|c| ids.iter().position(|i| *i == c))
            .map_or(-1, |i| i as i32),
    );

    let (ww, st) = (w.as_weak(), state.clone());
    w.on_mic_selected(move |i| {
        let (Some(w), Some(id)) = (ww.upgrade(), ids.get(i as usize)) else {
            return;
        };
        let mut s = st.borrow_mut();
        if s.cfg.mic_id.as_ref() != Some(id) {
            s.cfg.mic_id = Some(id.clone());
            commit(&s, &w);
        }
    });
}

pub fn setup(w: &MainWindow, state: &Rc<RefCell<State>>) -> Mic {
    let sink = match record::create_hidden_window() {
        Ok(h) => h,
        Err(e) => {
            crate::log!("settings: sink window failed: {e}");
            HWND::default()
        }
    };
    setup_mics(w, state, sink);
    show_hotkey(w, &state.borrow().cfg.hotkey);

    let recorder: Rc<RefCell<Option<Recorder>>> = Rc::new(RefCell::new(None));
    let timer = Rc::new(Timer::default());

    let (ww, st) = (w.as_weak(), state.clone());
    w.on_hotkey_picked(move || {
        let Some(w) = ww.upgrade() else { return };
        let Some(&vk) = key_list(st.borrow().cfg.hotkey.vk).get(w.get_key_index() as usize) else {
            return;
        };
        let mods = [
            (w.get_mod_ctrl(), MOD_CONTROL),
            (w.get_mod_alt(), MOD_ALT),
            (w.get_mod_shift(), MOD_SHIFT),
            (w.get_mod_win(), MOD_WIN),
        ]
        .iter()
        .filter(|(on, _)| *on)
        .fold(0, |m, (_, b)| m | b);
        choose(&w, &st, mods, vk);
    });

    let (ww, st, rec, tm) = (w.as_weak(), state.clone(), recorder.clone(), timer.clone());
    w.on_record_clicked(move || {
        let Some(w) = ww.upgrade() else { return };
        if rec.borrow().is_some() {
            return;
        }
        send_to_core(MSG_PAUSE_HOTKEY, 1, unsafe { GetCurrentProcessId() }
            as isize);
        match Recorder::start(sink) {
            Ok(r) => *rec.borrow_mut() = Some(r),
            Err(e) => {
                crate::log!("settings: raw input register failed: {e}");
                resume();
                let lang = i18n::resolve(st.borrow().cfg.language);
                w.set_hotkey_error(i18n::t(lang, Key::RecordFailed).into());
                return;
            }
        }
        w.set_recording(true);
        let (ww, st, rec, tm2) = (ww.clone(), st.clone(), rec.clone(), tm.clone());
        tm.start(TimerMode::Repeated, Duration::from_millis(30), move || {
            let Some(ev) = rec.borrow_mut().as_mut().and_then(|r| r.poll()) else {
                return;
            };
            tm2.stop();
            rec.borrow_mut().take(); // unregister Raw Input
            resume(); // before commit, so the core's reload registers the new hotkey
            let Some(w) = ww.upgrade() else { return };
            w.set_recording(false);
            if let RecordEvent::Key { vk, mods } = ev {
                choose(&w, &st, mods, vk);
            }
        });
    });

    w.on_test_mute(|| {
        crate::app::post_to_core(MSG_TOGGLE);
    });

    Mic { recorder }
}
