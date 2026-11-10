//! Penanda tab glue: popup, border, monitors, sound.
use super::model::{Debouncer, retain_known};
use super::ui::{MainWindow, MonitorItem, State, commit};
use crate::config::{BorderStyle, PopupStyle, Position};
use crate::i18n::{self, Key};
use crate::monitors;
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

type Action = Rc<dyn Fn()>;

const POSITIONS: [Position; 6] = [
    Position::TopLeft,
    Position::TopCenter,
    Position::TopRight,
    Position::BottomLeft,
    Position::BottomCenter,
    Position::BottomRight,
];

const STYLES: [BorderStyle; 2] = [BorderStyle::Solid, BorderStyle::Glow];
const POPUP_STYLES: [PopupStyle; 2] = [PopupStyle::Card, PopupStyle::Pill];

/// Saves once, 300 ms after the last `poke`.
struct Delayed {
    timer: Timer,
    deb: RefCell<Debouncer>,
    t0: Instant,
    pending: RefCell<Option<Rc<dyn Fn()>>>,
}

impl Delayed {
    fn new() -> Rc<Self> {
        Rc::new(Delayed {
            timer: Timer::default(),
            deb: RefCell::new(Debouncer::new(300)),
            t0: Instant::now(),
            pending: RefCell::new(None),
        })
    }

    fn now(&self) -> u64 {
        self.t0.elapsed().as_millis() as u64
    }

    /// Runs a save that is still waiting out its quiet period (window closing).
    fn flush(&self) {
        self.timer.stop();
        if let Some(save) = self.pending.borrow_mut().take() {
            save();
        }
    }

    fn poke(self: &Rc<Self>, save: impl Fn() + 'static) {
        self.deb.borrow_mut().bump(self.now());
        *self.pending.borrow_mut() = Some(Rc::new(save));
        let me = self.clone();
        self.timer
            .start(TimerMode::Repeated, Duration::from_millis(50), move || {
                if me.deb.borrow().ready(me.now()) {
                    me.flush();
                }
            });
    }
}

/// Connected monitors plus saved IDs that match none: (id, gdi name, label).
fn monitor_rows(saved: &[String], lang: i18n::ResolvedLang) -> Vec<(String, String, String)> {
    let mut rows: Vec<(String, String, String)> = monitors::enumerate()
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
            (id, m.gdi_name.clone(), label)
        })
        .collect();
    for id in saved {
        if !rows.iter().any(|(i, g, _)| monitors::id_matches(id, i, g)) {
            let label = format!("{id} {}", i18n::t(lang, Key::MicNotConnected));
            rows.push((id.clone(), id.clone(), label));
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
    w.set_pen_popup_style_index(
        POPUP_STYLES
            .iter()
            .position(|s| *s == c.popup_style)
            .map_or(-1, |i| i as i32),
    );
    w.set_pen_border_enabled(c.border_enabled);
    w.set_pen_thickness(c.border_thickness as f32);
    w.set_pen_border_style_index(
        STYLES
            .iter()
            .position(|s| *s == c.border_style)
            .map_or(-1, |i| i as i32),
    );
    w.set_pen_all_monitors(c.all_monitors);
    w.set_pen_sound(c.sound_enabled);
}

/// Binds the tab; the returned closure re-enumerates monitors (tab opened).
/// Returns (refresh, flush): re-enumerate monitors on tab open; flush pending debounced saves.
pub fn setup(w: &MainWindow, state: &Rc<RefCell<State>>) -> (Action, Action) {
    show(w, &state.borrow());
    let ids: Rc<RefCell<Vec<(String, String)>>> = Rc::default();

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
            .map(|(id, gdi, label)| MonitorItem {
                label: label.as_str().into(),
                checked: saved.iter().any(|s| monitors::id_matches(s, id, gdi)),
            })
            .collect();
        w.set_pen_monitors(ModelRc::new(VecModel::from(items)));
        *ids2.borrow_mut() = rows.into_iter().map(|(id, gdi, _)| (id, gdi)).collect();
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
            if let Some(s) = POPUP_STYLES.get(w.get_pen_popup_style_index() as usize) {
                c.popup_style = *s;
            }
            c.border_enabled = w.get_pen_border_enabled();
            if let Some(s) = STYLES.get(w.get_pen_border_style_index() as usize) {
                c.border_style = *s;
            }
            c.all_monitors = w.get_pen_all_monitors();
            c.sound_enabled = w.get_pen_sound();
            before != *c
        };
        if changed {
            commit(&st, &w);
        }
    });

    let d_scale = Delayed::new();
    let d_thick = Delayed::new();
    let (ww, st) = (w.as_weak(), state.clone());
    let delayed = d_scale.clone();
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

    let (ww, st) = (w.as_weak(), state.clone());
    let delayed = d_thick.clone();
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
        let Some((id, gdi)) = ids.borrow().get(i as usize).cloned() else {
            return;
        };
        {
            let mut s = st.borrow_mut();
            let mut v = retain_known(&s.cfg.monitor_ids);
            v.retain(|x| !monitors::id_matches(x, &id, &gdi));
            if on {
                v.push(id);
            }
            s.cfg.monitor_ids = v;
        }
        commit(&st, &w);
    });

    let flush: Rc<dyn Fn()> = Rc::new(move || {
        d_scale.flush();
        d_thick.flush();
    });
    (refresh, flush)
}

#[cfg(test)]
mod tests {
    use crate::monitors::id_matches;

    #[test]
    fn migrated_gdi_name_matches_connected_monitor() {
        assert!(id_matches(r"\\.\display1", "STABLE", r"\\.\DISPLAY1"));
        assert!(id_matches("STABLE", "STABLE", r"\\.\DISPLAY1"));
        assert!(!id_matches("", "", r"\\.\DISPLAY1"));
    }
}
