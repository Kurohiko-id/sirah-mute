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
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::Threading::GetCurrentProcessId;

/// Recording session; dropping the `Recorder` removes the Raw Input registration.
pub struct Mic {
    recorder: Rc<RefCell<Option<Recorder>>>,
    pub sink: HWND,
    /// Re-reads the mic list (tab opened).
    pub refresh: Rc<dyn Fn()>,
}

impl Mic {
    /// Called after the window closes: never leave the core's hotkey paused.
    pub fn finish(self) {
        if self.recorder.borrow_mut().take().is_some() {
            resume();
        }
    }
}

/// Slider value to the nearest 50 ms step, within the config's 200..=2000 range.
fn snap_cough_ms(v: f32) -> u32 {
    ((v / 50.0).round() as u32 * 50).clamp(200, 2000)
}

fn cough_secs_text(ms: u32, lang: i18n::ResolvedLang) -> String {
    let s = format!("{:.1}", ms as f32 / 1000.0);
    match lang {
        i18n::ResolvedLang::Id => format!("{} dtk", s.replace('.', ",")),
        _ => format!("{s} s"),
    }
}

pub fn model(items: Vec<String>) -> ModelRc<SharedString> {
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
            let changed = std::mem::replace(&mut state.borrow_mut().cfg.hotkey, h) != h;
            if changed {
                commit(state, w);
            }
            show_hotkey(w, &h);
        }
        Err(e) => {
            let k = match e {
                HotkeyError::BareF12 => Key::HotkeyBareF12,
                HotkeyError::ModifierOnly | HotkeyError::Empty => Key::HotkeyModOnly,
            };
            w.set_hotkey_error(i18n::t(lang, k).into());
            show_hotkey(w, &state.borrow().cfg.hotkey); // resync controls to the saved hotkey
        }
    }
}

/// Active devices of `flow`, plus the saved `cur` as an extra "(not connected)" entry when absent.
/// Returns (ids, names) in dropdown order.
pub fn device_choices(
    sink: HWND,
    flow: Flow,
    cur: Option<&String>,
    lang: i18n::ResolvedLang,
) -> (Vec<String>, Vec<String>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let all = match Audio::new(sink) {
        Ok(a) => a.list(flow),
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
    if let Some(id) = cur.filter(|id| !ids.contains(id)) {
        let name = all
            .iter()
            .find(|d| d.id == *id)
            .map_or(id.as_str(), |d| d.name.as_str());
        names.push(format!("{name} {}", i18n::t(lang, Key::MicNotConnected)));
        ids.push(id.clone());
    }
    (ids, names)
}

/// (ids, names, selected index) for a dropdown whose saved device is `cur`.
fn fill_list(
    sink: HWND,
    cur: Option<&String>,
    lang: i18n::ResolvedLang,
) -> (Vec<String>, ModelRc<SharedString>, i32) {
    let (ids, names) = device_choices(sink, Flow::Capture, cur, lang);
    let idx = cur
        .and_then(|c| ids.iter().position(|i| i == c))
        .map_or(-1, |i| i as i32);
    (ids, model(names), idx)
}

/// Stores the device at dropdown row `i` into `slot`; true if it changed.
fn pick(ids: &RefCell<Vec<String>>, i: i32, slot: &mut Option<String>) -> bool {
    let Some(id) = ids.borrow().get(i as usize).cloned() else {
        return false;
    };
    slot.replace(id.clone()).as_ref() != Some(&id)
}

/// Fills the mic and talk-source dropdowns now; the returned closure re-reads the devices (tab opened).
fn setup_mics(w: &MainWindow, state: &Rc<RefCell<State>>, sink: HWND) -> Rc<dyn Fn()> {
    let ids: Rc<RefCell<Vec<String>>> = Rc::default();
    let talk_ids: Rc<RefCell<Vec<String>>> = Rc::default();
    let (ww, st, ids2, talk2) = (w.as_weak(), state.clone(), ids.clone(), talk_ids.clone());
    let refresh: Rc<dyn Fn()> = Rc::new(move || {
        let Some(w) = ww.upgrade() else { return };
        let (cur, talk_cur, lang) = {
            let s = st.borrow();
            (
                s.cfg.mic_id.clone(),
                s.cfg.talk_warn_mic_id.clone(),
                i18n::resolve(s.cfg.language),
            )
        };
        let (new_ids, names, idx) = fill_list(sink, cur.as_ref(), lang);
        w.set_mic_names(names);
        w.set_mic_index(idx);
        *ids2.borrow_mut() = new_ids;

        let (new_ids, names, idx) = fill_list(sink, talk_cur.as_ref(), lang);
        w.set_talk_mic_names(names);
        w.set_talk_mic_index(idx);
        *talk2.borrow_mut() = new_ids;
    });
    refresh();

    let (ww, st) = (w.as_weak(), state.clone());
    w.on_mic_selected(move |i| {
        let Some(w) = ww.upgrade() else { return };
        if pick(&ids, i, &mut st.borrow_mut().cfg.mic_id) {
            commit(&st, &w);
        }
    });

    let (ww, st) = (w.as_weak(), state.clone());
    w.on_talk_mic_selected(move |i| {
        let Some(w) = ww.upgrade() else { return };
        if pick(&talk_ids, i, &mut st.borrow_mut().cfg.talk_warn_mic_id) {
            commit(&st, &w);
        }
    });

    let (ww, st) = (w.as_weak(), state.clone());
    w.on_talk_changed(move || {
        let Some(w) = ww.upgrade() else { return };
        st.borrow_mut().cfg.talk_warn_enabled = w.get_talk_enabled();
        commit(&st, &w);
    });
    w.set_talk_enabled(state.borrow().cfg.talk_warn_enabled);

    let (ww, st) = (w.as_weak(), state.clone());
    w.on_cough_changed(move || {
        let Some(w) = ww.upgrade() else { return };
        st.borrow_mut().cfg.cough_hold = w.get_cough_enabled();
        commit(&st, &w);
    });
    let (ww, st) = (w.as_weak(), state.clone());
    w.on_cough_ms_changed(move |v| {
        let Some(w) = ww.upgrade() else { return };
        let ms = snap_cough_ms(v);
        w.set_cough_ms(ms as f32);
        w.set_cough_secs(cough_secs_text(ms, i18n::resolve(st.borrow().cfg.language)).into());
        if ms != std::mem::replace(&mut st.borrow_mut().cfg.cough_hold_ms, ms) {
            commit(&st, &w);
        }
    });
    {
        let s = state.borrow();
        w.set_cough_enabled(s.cfg.cough_hold);
        w.set_cough_ms(s.cfg.cough_hold_ms as f32);
        w.set_cough_secs(cough_secs_text(s.cfg.cough_hold_ms, i18n::resolve(s.cfg.language)).into());
    }
    refresh
}

pub fn setup(w: &MainWindow, state: &Rc<RefCell<State>>) -> Mic {
    let sink = match record::create_hidden_window() {
        Ok(h) => h,
        Err(e) => {
            crate::log!("settings: sink window failed: {e}");
            HWND::default()
        }
    };
    let refresh = setup_mics(w, state, sink);
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
        let started = Instant::now();
        let (ww, st, rec, tm2) = (ww.clone(), st.clone(), rec.clone(), tm.clone());
        tm.start(TimerMode::Repeated, Duration::from_millis(30), move || {
            let polled = rec.borrow_mut().as_mut().and_then(|r| r.poll());
            let ev = match polled {
                Some(ev) => ev,
                None if record::should_cancel(
                    started.elapsed().as_millis() as u64,
                    record::foreground_is_ours(),
                ) =>
                {
                    RecordEvent::Cancel
                }
                None => return,
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

    Mic {
        recorder,
        sink,
        refresh,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cough_slider_snaps_and_formats() {
        assert_eq!(snap_cough_ms(424.0), 400);
        assert_eq!(snap_cough_ms(426.0), 450);
        assert_eq!(snap_cough_ms(5000.0), 2000);
        assert_eq!(cough_secs_text(400, i18n::ResolvedLang::En), "0.4 s");
        assert_eq!(cough_secs_text(1350, i18n::ResolvedLang::Id), "1,4 dtk");
    }
}
