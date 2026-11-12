use crate::config::{Config, GuardMode};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(60);
const MAX_REVERTS: usize = 5;
pub(crate) const PAUSE: Duration = Duration::from_secs(300);

/// Arrays indexed console, multimedia, communications.
/// `*_present` = the locked device is active.
#[derive(Default, Clone)]
pub struct Defaults {
    pub capture: [Option<String>; 3],
    pub render: [Option<String>; 3],
    pub input_present: bool,
    pub output_present: bool,
}

#[derive(PartialEq, Debug)]
pub enum Action {
    None,
    Revert { input: bool, output: bool },
    Notify { input: bool, output: bool },
    GiveUp,
}

#[derive(Default)]
pub struct Enforcer {
    history: VecDeque<Instant>,
    paused_until: Option<Instant>,
}

fn drifted(locked: &Option<String>, present: bool, roles: &[Option<String>; 3]) -> bool {
    match locked {
        Some(id) if present => roles.iter().any(|r| r.as_deref() != Some(id.as_str())),
        _ => false,
    }
}

impl Enforcer {
    pub fn new() -> Self {
        Enforcer {
            history: VecDeque::new(),
            paused_until: None,
        }
    }

    pub fn evaluate(&mut self, cfg: &Config, cur: &Defaults, now: Instant) -> Action {
        if !cfg.guard_enabled || cfg.guard_mode == GuardMode::Off {
            return Action::None;
        }
        let input = drifted(&cfg.default_input_id, cur.input_present, &cur.capture);
        let output = drifted(&cfg.default_output_id, cur.output_present, &cur.render);
        if !input && !output {
            return Action::None;
        }
        if cfg.guard_mode == GuardMode::Notify {
            return Action::Notify { input, output };
        }
        if self.paused_until.is_some_and(|p| now < p) {
            return Action::None;
        }
        while self
            .history
            .front()
            .is_some_and(|&t| now.duration_since(t) > WINDOW)
        {
            self.history.pop_front();
        }
        if self.history.len() >= MAX_REVERTS {
            self.paused_until = Some(now + PAUSE);
            self.history.clear();
            return Action::GiveUp;
        }
        self.history.push_back(now);
        Action::Revert { input, output }
    }

    pub fn reset(&mut self) {
        self.history.clear();
        self.paused_until = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: &str) -> Option<String> {
        Some(x.to_string())
    }
    fn cfg(mode: GuardMode) -> Config {
        Config {
            default_input_id: s("mic"),
            default_output_id: s("spk"),
            guard_mode: mode,
            guard_enabled: true,
            ..Config::default()
        }
    }
    fn ok() -> Defaults {
        Defaults {
            capture: [s("mic"), s("mic"), s("mic")],
            render: [s("spk"), s("spk"), s("spk")],
            input_present: true,
            output_present: true,
        }
    }
    fn drift() -> Defaults {
        let mut d = ok();
        d.capture[0] = s("other");
        d
    }
    fn at(t0: Instant, n: u64) -> Instant {
        t0 + Duration::from_secs(n)
    }
    const REV: Action = Action::Revert {
        input: true,
        output: false,
    };

    #[test]
    fn no_drift_none() {
        let mut e = Enforcer::new();
        assert_eq!(
            e.evaluate(&cfg(GuardMode::AutoRevert), &ok(), Instant::now()),
            Action::None
        );
    }
    #[test]
    fn console_drift_reverts_input() {
        let mut e = Enforcer::new();
        assert_eq!(
            e.evaluate(&cfg(GuardMode::AutoRevert), &drift(), Instant::now()),
            REV
        );
    }
    #[test]
    fn only_communications_drift_still_reverts() {
        let mut d = ok();
        d.render[2] = None;
        let mut e = Enforcer::new();
        assert_eq!(
            e.evaluate(&cfg(GuardMode::AutoRevert), &d, Instant::now()),
            Action::Revert {
                input: false,
                output: true
            }
        );
    }
    #[test]
    fn absent_locked_device_is_not_drift() {
        let mut d = drift();
        d.input_present = false;
        let mut e = Enforcer::new();
        assert_eq!(
            e.evaluate(&cfg(GuardMode::AutoRevert), &d, Instant::now()),
            Action::None
        );
    }
    #[test]
    fn notify_mode_notifies() {
        let mut e = Enforcer::new();
        assert_eq!(
            e.evaluate(&cfg(GuardMode::Notify), &drift(), Instant::now()),
            Action::Notify {
                input: true,
                output: false
            }
        );
    }
    #[test]
    fn guard_disabled_none() {
        let mut c = cfg(GuardMode::AutoRevert);
        c.guard_enabled = false;
        let mut e = Enforcer::new();
        assert_eq!(e.evaluate(&c, &drift(), Instant::now()), Action::None);
        c.guard_enabled = true;
        c.guard_mode = GuardMode::Off;
        assert_eq!(e.evaluate(&c, &drift(), Instant::now()), Action::None);
    }
    #[test]
    fn sixth_revert_in_60s_gives_up_then_resumes_after_5min() {
        let (c, d, t0) = (cfg(GuardMode::AutoRevert), drift(), Instant::now());
        let mut e = Enforcer::new();
        for n in 0..5 {
            assert_eq!(e.evaluate(&c, &d, at(t0, n)), REV);
        }
        assert_eq!(e.evaluate(&c, &d, at(t0, 5)), Action::GiveUp);
        assert_eq!(e.evaluate(&c, &d, at(t0, 200)), Action::None);
        assert_eq!(e.evaluate(&c, &d, at(t0, 306)), REV);
    }
    #[test]
    fn old_reverts_expire() {
        let (c, d, t0) = (cfg(GuardMode::AutoRevert), drift(), Instant::now());
        let mut e = Enforcer::new();
        for n in [0, 20, 40, 61, 70] {
            assert_eq!(e.evaluate(&c, &d, at(t0, n)), REV);
        }
    }
    #[test]
    fn reset_clears_pause() {
        let (c, d, t0) = (cfg(GuardMode::AutoRevert), drift(), Instant::now());
        let mut e = Enforcer::new();
        for n in 0..5 {
            e.evaluate(&c, &d, at(t0, n));
        }
        assert_eq!(e.evaluate(&c, &d, at(t0, 5)), Action::GiveUp);
        e.reset();
        assert_eq!(e.evaluate(&c, &d, at(t0, 6)), REV);
    }
}
