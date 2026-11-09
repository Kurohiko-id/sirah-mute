//! Talk-while-muted detector: pure logic, fed one meter peak per tick.
use std::time::{Duration, Instant};

const THRESHOLD: f32 = 0.015;
const HOLD: Duration = Duration::from_millis(300);
const COOLDOWN: Duration = Duration::from_millis(10_000);

#[derive(Default)]
pub struct TalkDetector {
    above_since: Option<Instant>,
    last_fire: Option<Instant>,
}

impl TalkDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// True = warn now: level held above the threshold for `HOLD`, outside the cooldown.
    pub fn feed(&mut self, peak: f32, now: Instant) -> bool {
        if peak < THRESHOLD {
            self.above_since = None;
            return false;
        }
        let since = *self.above_since.get_or_insert(now);
        let cooled = self
            .last_fire
            .is_none_or(|t| now.duration_since(t) >= COOLDOWN);
        if now.duration_since(since) >= HOLD && cooled {
            self.above_since = None;
            self.last_fire = Some(now);
            return true;
        }
        false
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds `peak` every 100 ms over [from_ms, to_ms); returns the ms values where feed() fired.
    fn run(d: &mut TalkDetector, t0: Instant, from_ms: u64, to_ms: u64, peak: f32) -> Vec<u64> {
        (from_ms..to_ms)
            .step_by(100)
            .filter(|&ms| d.feed(peak, t0 + Duration::from_millis(ms)))
            .collect()
    }

    #[test]
    fn silence_never_fires() {
        let (mut d, t0) = (TalkDetector::new(), Instant::now());
        assert!(run(&mut d, t0, 0, 60_000, 0.0).is_empty());
        assert!(run(&mut d, t0, 60_000, 120_000, 0.005).is_empty());
    }

    #[test]
    fn short_blip_ignored() {
        let (mut d, t0) = (TalkDetector::new(), Instant::now());
        let mut f = run(&mut d, t0, 0, 200, 0.2);
        f.extend(run(&mut d, t0, 200, 5_000, 0.0));
        assert!(f.is_empty());
    }

    #[test]
    fn sustained_talk_fires_after_hold() {
        let (mut d, t0) = (TalkDetector::new(), Instant::now());
        assert_eq!(run(&mut d, t0, 0, 1_000, 0.05), vec![300]);
    }

    #[test]
    fn talk_fires_once_per_cooldown() {
        let (mut d, t0) = (TalkDetector::new(), Instant::now());
        assert_eq!(run(&mut d, t0, 0, 25_000, 0.05), vec![300, 10_300, 20_300]);
    }

    #[test]
    fn reset_clears_hold_and_cooldown() {
        let (mut d, t0) = (TalkDetector::new(), Instant::now());
        assert_eq!(run(&mut d, t0, 0, 1_000, 0.05), vec![300]);
        d.reset();
        assert_eq!(run(&mut d, t0, 1_000, 2_000, 0.05), vec![1_300]);
    }
}
