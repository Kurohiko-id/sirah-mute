//! Device tab glue: default input/output, startup force, guard.
use super::mic::{device_choices, model};
use super::model::default_from_index;
use super::ui::{MainWindow, State, commit};
use crate::audio::Flow;
use crate::config::GuardMode;
use crate::i18n::{self, Key};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;
use windows::Win32::Foundation::HWND;

const MODES: [GuardMode; 3] = [GuardMode::Off, GuardMode::Notify, GuardMode::AutoRevert];

type Ids = Rc<RefCell<Vec<String>>>;

/// (ids, dropdown names, selected index); entry 0 is the "do not change" item.
fn entries(
    sink: HWND,
    flow: Flow,
    cur: Option<&String>,
    lang: i18n::ResolvedLang,
) -> (Vec<String>, Vec<String>, i32) {
    let (ids, names) = device_choices(sink, flow, cur, lang);
    let idx = cur
        .and_then(|c| ids.iter().position(|i| i == c))
        .map_or(0, |i| i as i32 + 1);
    let mut all = vec![i18n::t(lang, Key::DontChange).to_string()];
    all.extend(names);
    (ids, all, idx)
}

/// Binds the tab; the returned closure re-reads the device lists and re-shows the toggles
/// (tab opened, or the tray changed "Guard devices" meanwhile).
pub fn setup(w: &MainWindow, state: &Rc<RefCell<State>>, sink: HWND) -> Rc<dyn Fn()> {
    let (out_ids, in_ids): (Ids, Ids) = (Rc::default(), Rc::default());

    let (ww, st, oi, ii) = (w.as_weak(), state.clone(), out_ids.clone(), in_ids.clone());
    let refresh: Rc<dyn Fn()> = Rc::new(move || {
        let Some(w) = ww.upgrade() else { return };
        let (inp, out, lang) = {
            let s = st.borrow();
            w.set_dev_force(s.cfg.force_on_startup);
            w.set_dev_guard(s.cfg.guard_enabled);
            w.set_dev_guard_mode(
                MODES
                    .iter()
                    .position(|m| *m == s.cfg.guard_mode)
                    .map_or(-1, |i| i as i32),
            );
            (
                s.cfg.default_input_id.clone(),
                s.cfg.default_output_id.clone(),
                i18n::resolve(s.cfg.language),
            )
        };
        let (ids, names, idx) = entries(sink, Flow::Render, out.as_ref(), lang);
        w.set_dev_output_names(model(names));
        w.set_dev_output_index(idx);
        *oi.borrow_mut() = ids;
        let (ids, names, idx) = entries(sink, Flow::Capture, inp.as_ref(), lang);
        w.set_dev_input_names(model(names));
        w.set_dev_input_index(idx);
        *ii.borrow_mut() = ids;
    });
    refresh();

    let (ww, st) = (w.as_weak(), state.clone());
    w.on_dev_changed(move || {
        let Some(w) = ww.upgrade() else { return };
        let changed = {
            let mut s = st.borrow_mut();
            let before = s.cfg.clone();
            let c = &mut s.cfg;
            c.default_output_id =
                default_from_index(w.get_dev_output_index().max(0) as usize, &out_ids.borrow());
            c.default_input_id =
                default_from_index(w.get_dev_input_index().max(0) as usize, &in_ids.borrow());
            c.force_on_startup = w.get_dev_force();
            c.guard_enabled = w.get_dev_guard();
            if let Some(m) = MODES.get(w.get_dev_guard_mode() as usize) {
                c.guard_mode = *m;
            }
            before != *c
        };
        if changed {
            commit(&st, &w);
        }
    });

    refresh
}
