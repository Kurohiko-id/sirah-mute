//! Penanda tab glue: popup, border, monitors, sound.
use super::model::{Debouncer, retain_known};
use super::ui::{MainWindow, MonitorItem, State, commit};
use crate::config::Position;
use crate::i18n::{self, Key};
use crate::monitors;
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

const POSITIONS: [Position; 6] = [
    Position::TopLeft,
    Position::TopCenter,
    Position::TopRight,
    Position::BottomLeft,
    Position::BottomCenter,
    Position::BottomRight,
];

/// Saves once, 300 ms after the last `poke`.
struct Delayed {
    timer: Timer,
    deb: RefCell<Debouncer>,
    t0: Instant,
}

impl Delayed {
    fn new() -> Rc<Self> {
        Rc::new(Delayed {
            timer: Timer::default(),
            deb: RefCell::new(Debouncer::new(300)),
            t0: Instant::now(),
        })
    }

    fn now(&self) -> u64 {
        self.t0.elapsed().as_millis() as u64
    }

    fn poke(self: &Rc<Self>, save: impl Fn() + 'static) {
        self.deb.borrow_mut().bump(self.now());
        let me = self.clone();
        self.timer
            .start(TimerMode::Repeated, Duration::from_millis(50), move || {
                if me.deb.borrow().ready(me.now()) {
                    me.timer.stop();
                    save();
                }
            });
    }
}

/// Connected monitors plus saved IDs that are not connected: (id, label).
fn monitor_rows(saved: &[String], lang: i18n::ResolvedLang) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = monitors::enumerate()
        .iter()
        .map(|m| {
            let id = if m.stable_id.is_empty() {
                m.gdi_name.clone()
            } else {
                m.stable_id.clone()
            };
            let name = m.gdi_name.trim_start_matches(r"\\.\");
            let label = format!(
                "{name} ({}x{})",
                m.rect.right - m.rect.left,
                m.rect.bottom - m.rect.top
            );
            (id, label)
        })
        .collect();
    for id in saved {
        if !rows.iter().any(|(i, _)| i == id) {
            let label = format!("{id} {}", i18n::t(lang, Key::MicNotConnected));
            rows.push((id.clone(), label));
        }
    }
    rows
}

fn show(w: &MainWindow, st: &State) {
    let c = &st.cfg;
    w.set_pen_popup_enabled(c.popup_enabled);
    w.set_pen_position_index(
        POSITIONS
            .iter()
            .position(|p| *p == c.popup_position)
            .map_or(-1, |i| i as i32),
    );
    w.set_pen_scale(c.popup_scale);
    w.set_pen_persist(c.popup_persist_while_muted);
    w.set_pen_border_enabled(c.border_enabled);
    w.set_pen_thickness(c.border_thickness as f32);
    w.set_pen_all_monitors(c.all_monitors);
    w.set_pen_sound(c.sound_enabled);
}

/// Binds the tab; the returned closure re-enumerates monitors (tab opened).
pub fn setup(w: &MainWindow, state: &Rc<RefCell<State>>) -> Rc<dyn Fn()> {
    show(w, &state.borrow());
    let ids: Rc<RefCell<Vec<String>>> = Rc::default();

    let (ww, st, ids2) = (w.as_weak(), state.clone(), ids.clone());
    let refresh: Rc<dyn Fn()> = Rc::new(move || {
        let Some(w) = ww.upgrade() else { return };
        let (saved, lang) = {
            let s = st.borrow();
            (s.cfg.monitor_ids.clone(), i18n::resolve(s.cfg.language))
        };
        let rows = monitor_rows(&saved, lang);
        let items: Vec<MonitorItem> = rows
            .iter()
            .map(|(id, label)| MonitorItem {
                label: label.as_str().into(),
                checked: saved.contains(id),
            })
            .collect();
        w.set_pen_monitors(ModelRc::new(VecModel::from(items)));
        *ids2.borrow_mut() = rows.into_iter().map(|(id, _)| id).collect();
    });
    refresh();

    // Toggles and dropdown: read every control, save if anything changed.
    let (ww, st) = (w.as_weak(), state.clone());
    w.on_pen_changed(move || {
        let Some(w) = ww.upgrade() else { return };
        let changed = {
            let mut s = st.borrow_mut();
            let before = s.cfg.clone();
            let c = &mut s.cfg;
            c.popup_enabled = w.get_pen_popup_enabled();
            if let Some(p) = POSITIONS.get(w.get_pen_position_index() as usize) {
                c.popup_position = *p;
            }
            c.popup_persist_while_muted = w.get_pen_persist();
            c.border_enabled = w.get_pen_border_enabled();
            c.all_monitors = w.get_pen_all_monitors();
            c.sound_enabled = w.get_pen_sound();
            before != *c
        };
        if changed {
            commit(&st, &w);
        }
    });

    let delayed = Delayed::new();
    let (ww, st) = (w.as_weak(), state.clone());
    w.on_pen_scale_changed(move |v| {
        let v = (v * 10.0).round() / 10.0;
        if (v - st.borrow().cfg.popup_scale).abs() < 1e-6 {
            return;
        }
        st.borrow_mut().cfg.popup_scale = v;
        let (ww, st) = (ww.clone(), st.clone());
        delayed.poke(move || {
            if let Some(w) = ww.upgrade() {
                commit(&st, &w);
            }
        });
    });

    let delayed = Delayed::new();
    let (ww, st) = (w.as_weak(), state.clone());
    w.on_pen_thickness_changed(move |v| {
        let v = v.round() as u32;
        if v == st.borrow().cfg.border_thickness {
            return;
        }
        st.borrow_mut().cfg.border_thickness = v;
        let (ww, st) = (ww.clone(), st.clone());
        delayed.poke(move || {
            if let Some(w) = ww.upgrade() {
                commit(&st, &w);
            }
        });
    });

    let (ww, st) = (w.as_weak(), state.clone());
    w.on_monitor_toggled(move |i, on| {
        let Some(w) = ww.upgrade() else { return };
        let Some(id) = ids.borrow().get(i as usize).cloned() else {
            return;
        };
        {
            let mut s = st.borrow_mut();
            let mut v = retain_known(&s.cfg.monitor_ids);
            v.retain(|x| *x != id);
            if on {
                v.push(id);
            }
            s.cfg.monitor_ids = v;
        }
        commit(&st, &w);
    });

    refresh
}
