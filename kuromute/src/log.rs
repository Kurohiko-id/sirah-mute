use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;
use windows::Win32::System::SystemInformation::GetLocalTime;

const MAX_BYTES: u64 = 1_000_000;
const KEEP: usize = 3;

enum Msg {
    Line(String),
    /// Acked once every line queued before it is on disk.
    Flush(Sender<()>),
}

static TX: OnceLock<Sender<Msg>> = OnceLock::new();
static DIR: OnceLock<PathBuf> = OnceLock::new();

#[macro_export]
macro_rules! log {
    ($($t:tt)*) => { $crate::log::write(format!($($t)*)) };
}

fn format_ts(y: u16, mo: u16, d: u16, h: u16, mi: u16, s: u16, ms: u16) -> String {
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{ms:03}")
}

fn now() -> String {
    let t = unsafe { GetLocalTime() };
    format_ts(
        t.wYear,
        t.wMonth,
        t.wDay,
        t.wHour,
        t.wMinute,
        t.wSecond,
        t.wMilliseconds,
    )
}

fn numbered(dir: &Path, n: usize) -> PathBuf {
    dir.join(format!("sirahmute.log.{n}"))
}

/// Shifts log -> .1 -> .2 ... when the log is >= max_bytes. Never panics:
/// rename/remove failures are ignored because logging must never crash the app.
fn rotate(dir: &Path, max_bytes: u64, keep: usize) {
    let log = dir.join("sirahmute.log");
    if fs::metadata(&log).map(|m| m.len()).unwrap_or(0) < max_bytes {
        return;
    }
    let _ = fs::remove_file(numbered(dir, keep));
    for n in (1..keep).rev() {
        let _ = fs::rename(numbered(dir, n), numbered(dir, n + 1));
    }
    let _ = fs::rename(&log, numbered(dir, 1));
}

fn append(dir: &Path, msg: &str) {
    rotate(dir, MAX_BYTES, KEEP);
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("sirahmute.log"))
    {
        let _ = writeln!(f, "{} {}", now(), msg);
    }
}

pub fn init(dir: &Path) {
    let _ = fs::create_dir_all(dir);
    let dir = dir.to_path_buf();
    let _ = DIR.set(dir.clone());
    let (tx, rx) = channel::<Msg>();
    if TX.set(tx).is_err() {
        return;
    }
    std::thread::spawn(move || {
        for msg in rx {
            match msg {
                Msg::Line(m) => append(&dir, &m),
                Msg::Flush(ack) => {
                    let _ = ack.send(());
                }
            }
        }
    });
}

pub fn write(msg: impl Into<String>) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(Msg::Line(msg.into()));
    }
}

/// Waits (up to 200 ms) until every line queued so far is written. Call before
/// the process exits, or the logger thread dies with the tail of the queue.
pub fn flush() {
    if let Some(tx) = TX.get() {
        let (ack, done) = channel();
        if tx.send(Msg::Flush(ack)).is_ok() {
            let _ = done.recv_timeout(Duration::from_millis(200));
        }
    }
}

pub fn write_sync(msg: &str) {
    if let Some(dir) = DIR.get() {
        append(dir, msg);
    }
}

pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write_sync(&format!(
            "PANIC: {info}\n{}",
            std::backtrace::Backtrace::force_capture()
        ));
        default(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("sirahmute-log-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn ts_format() {
        assert_eq!(
            format_ts(2026, 11, 9, 5, 15, 2, 123),
            "2026-11-09T05:15:02.123"
        );
    }

    #[test]
    fn rotate_shifts_files() {
        let d = tmp("shift");
        fs::write(d.join("sirahmute.log"), "0123456789").unwrap();
        for (n, c) in [(1, "a"), (2, "b"), (3, "c")] {
            fs::write(d.join(format!("sirahmute.log.{n}")), c).unwrap();
        }
        rotate(&d, 5, 3);
        assert!(!d.join("sirahmute.log").exists());
        assert_eq!(
            fs::read_to_string(d.join("sirahmute.log.1")).unwrap(),
            "0123456789"
        );
        assert_eq!(fs::read_to_string(d.join("sirahmute.log.2")).unwrap(), "a");
        assert_eq!(fs::read_to_string(d.join("sirahmute.log.3")).unwrap(), "b");
        assert!(!d.join("sirahmute.log.4").exists());
    }

    #[test]
    fn flush_writes_queued_lines() {
        let d = tmp("flush");
        init(&d);
        write("queued line");
        flush();
        let log = fs::read_to_string(d.join("sirahmute.log")).unwrap();
        assert!(log.contains("queued line"));
    }

    #[test]
    fn rotate_noop_when_small() {
        let d = tmp("small");
        fs::write(d.join("sirahmute.log"), "01234").unwrap();
        rotate(&d, 100, 3);
        assert!(d.join("sirahmute.log").exists());
        assert!(!d.join("sirahmute.log.1").exists());
    }
}
