//! "Other mics" tab glue: watched-mic rows, add/remove, pill position.
use super::mic::{device_choices, model};
use super::penanda::POSITIONS;
use super::ui::{MainWindow, OtherMicRow, State, commit};
use crate::audio::Flow;
use crate::config::{MAX_OTHER_MICS, OTHER_NAME_MAX, OtherMic};
use crate::i18n::{self, Key};
use slint::{ComponentHandle, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use windows::Win32::Foundation::HWND;

/// Capture ids a row may pick: everything except the main mic.
pub fn choices(capture_ids: &[String], main: Option<&str>) -> Vec<String> {
    capture_ids
        .iter()
        .filter(|id| Some(id.as_str()) != main)
        .cloned()
        .collect()
}

/// First capture id that is neither the main mic nor already a row.
pub fn next_free(capture_ids: &[String], main: Option<&str>, used: &[String]) -> Option<String> {
    choices(capture_ids, main)
        .into_iter()
        .find(|id| !used.contains(id))
}

fn short_name(s: &str) -> String {
    s.trim().chars().take(OTHER_NAME_MAX).collect()
}

/// Dropdown contents shared by every row; re-read when the tab opens.
#[derive(Default)]
struct Devices {
    /// Selectable ids: present capture devices minus the main mic.
    ids: Vec<String>,
    names: Vec<String>,
    /// Present capture ids, main mic included (for `next_free`).
    present: Vec<String>,
}

pub fn setup(w: &MainWindow, state: &Rc<RefCell<State>>, sink: HWND) -> Rc<dyn Fn()> {
    let devs: Rc<RefCell<Devices>> = Rc::default();
    let rows: Rc<VecModel<OtherMicRow>> = Rc::default();
    w.set_other_rows(rows.clone().into());

    // Pushes cfg -> rows model, device dropdown and the add button.
    let show: Rc<dyn Fn(&MainWindow)> = {
        let (st, devs, rows) = (state.clone(), devs.clone(), rows.clone());
        Rc::new(move |w| {
            let s = st.borrow();
            let d = devs.borrow();
            let mut names = d.names.clone();
            let mut ids = d.ids.clone();
            let lang = i18n::resolve(s.cfg.language);
            for m in &s.cfg.other_mics {
                if !ids.contains(&m.id) {
                    // unplugged: stays listed, like the main-mic dropdown
                    let name = match m.name.trim() {
                        "" => "Mic",
                        n => n,
                    };
                    names.push(format!(
                        "{name} {}",
                        i18n::t(lang, Key::MicNotConnected)
                    ));
                    ids.push(m.id.clone());
                }
            }
            w.set_other_device_names(model(names));
            rows.set_vec(
                s.cfg
                    .other_mics
                    .iter()
                    .map(|m| OtherMicRow {
                        device_index: ids.iter().position(|i| *i == m.id).map_or(-1, |i| i as i32),
                        name: m.name.as_str().into(),
                        enabled: m.enabled,
                    })
                    .collect::<Vec<_>>(),
            );
            let used: Vec<String> = s.cfg.other_mics.iter().map(|m| m.id.clone()).collect();
            w.set_other_can_add(
                used.len() < MAX_OTHER_MICS
                    && next_free(&d.present, s.cfg.mic_id.as_deref(), &used).is_some(),
            );
            w.set_other_position_index(
                POSITIONS
                    .iter()
                    .position(|p| *p == s.cfg.other_mics_position)
                    .map_or(-1, |i| i as i32),
            );
        })
    };

    let refresh: Rc<dyn Fn()> = {
        let (ww, st, devs, show) = (w.as_weak(), state.clone(), devs.clone(), show.clone());
        Rc::new(move || {
            let Some(w) = ww.upgrade() else { return };
            let (main, lang) = {
                let s = st.borrow();
                (s.cfg.mic_id.clone(), i18n::resolve(s.cfg.language))
            };
            let (present, all_names) = device_choices(sink, Flow::Capture, None, lang);
            let ids = choices(&present, main.as_deref());
            let names = present
                .iter()
                .zip(all_names)
                .filter(|(id, _)| ids.contains(id))
                .map(|(_, n)| n)
                .collect();
            *devs.borrow_mut() = Devices {
                ids,
                names,
                present,
            };
            show(&w);
        })
    };
    refresh();

    let (ww, st, devs2, show2) = (w.as_weak(), state.clone(), devs.clone(), show.clone());
    w.on_other_add(move || {
        let Some(w) = ww.upgrade() else { return };
        {
            let mut s = st.borrow_mut();
            let used: Vec<String> = s.cfg.other_mics.iter().map(|m| m.id.clone()).collect();
            let d = devs2.borrow();
            let Some(id) = next_free(&d.present, s.cfg.mic_id.as_deref(), &used)
                .filter(|_| used.len() < MAX_OTHER_MICS)
            else {
                return;
            };
            let name = d
                .ids
                .iter()
                .position(|i| *i == id)
                .map(|i| short_name(&d.names[i]))
                .unwrap_or_default();
            s.cfg.other_mics.push(OtherMic {
                id,
                name,
                enabled: true,
            });
        }
        show2(&w);
        commit(&st, &w);
    });

    let (ww, st, show2) = (w.as_weak(), state.clone(), show.clone());
    w.on_other_remove(move |i| {
        let Some(w) = ww.upgrade() else { return };
        {
            let mut s = st.borrow_mut();
            if i < 0 || i as usize >= s.cfg.other_mics.len() {
                return;
            }
            s.cfg.other_mics.remove(i as usize);
        }
        show2(&w);
        commit(&st, &w);
    });

    // A device already used by another row is refused (the dropdown snaps back).
    let (ww, st, devs2, show2) = (w.as_weak(), state.clone(), devs.clone(), show.clone());
    w.on_other_device(move |i, d| {
        let Some(w) = ww.upgrade() else { return };
        let id = devs2.borrow().ids.get(d as usize).cloned();
        let changed = {
            let mut s = st.borrow_mut();
            let taken = id.as_ref().is_some_and(|id| {
                s.cfg
                    .other_mics
                    .iter()
                    .enumerate()
                    .any(|(n, m)| n as i32 != i && m.id == *id)
            });
            match (id, s.cfg.other_mics.get_mut(i as usize)) {
                (Some(id), Some(m)) if !taken && m.id != id => {
                    m.id = id;
                    true
                }
                _ => false,
            }
        };
        if changed {
            commit(&st, &w);
        } else {
            show2(&w);
        }
    });

    let (ww, st, show2) = (w.as_weak(), state.clone(), show.clone());
    w.on_other_name(move |i, text| {
        let Some(w) = ww.upgrade() else { return };
        let name = short_name(&text);
        let changed = st
            .borrow_mut()
            .cfg
            .other_mics
            .get_mut(i as usize)
            .is_some_and(|m| std::mem::replace(&mut m.name, name.clone()) != name);
        if changed {
            commit(&st, &w);
        }
        if name != text.as_str() {
            show2(&w); // show the trimmed/truncated name
        }
    });

    let (ww, st) = (w.as_weak(), state.clone());
    w.on_other_enabled(move |i, on| {
        let Some(w) = ww.upgrade() else { return };
        let changed = st
            .borrow_mut()
            .cfg
            .other_mics
            .get_mut(i as usize)
            .is_some_and(|m| std::mem::replace(&mut m.enabled, on) != on);
        if changed {
            commit(&st, &w);
        }
    });

    let (ww, st) = (w.as_weak(), state.clone());
    w.on_other_position_changed(move || {
        let Some(w) = ww.upgrade() else { return };
        let Some(p) = POSITIONS.get(w.get_other_position_index() as usize) else {
            return;
        };
        if std::mem::replace(&mut st.borrow_mut().cfg.other_mics_position, *p) != *p {
            commit(&st, &w);
        }
    });

    refresh
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn next_free_skips_main_and_used() {
        let all = ids(&["a", "b", "c"]);
        assert_eq!(next_free(&all, Some("b"), &ids(&["a"])), Some("c".into()));
        assert_eq!(next_free(&all, Some("b"), &ids(&["a", "c"])), None);
    }

    #[test]
    fn choices_exclude_main() {
        assert_eq!(choices(&ids(&["a", "b", "c"]), Some("b")), ids(&["a", "c"]));
    }
}
