#![allow(dead_code)]
mod audio;
mod autostart;
mod config;
mod enforcer;
mod geometry;
mod hotkey;
mod i18n;
mod log;
mod migrate;
mod monitors;
mod msg;
mod overlay;
mod policy_config;
mod sound;
mod tray;
mod update;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};

fn list_devices() {
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let a = match audio::Audio::new(HWND::default()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Core Audio init failed: {e}");
            return;
        }
    };
    for (label, flow) in [
        ("CAPTURE", audio::Flow::Capture),
        ("RENDER ", audio::Flow::Render),
    ] {
        for d in a.list(flow) {
            let present = if d.present { "yes" } else { "no " };
            println!("{label}  {present}  {}  {}", d.id, d.name);
        }
    }
}

fn main() {
    if std::env::args().any(|a| a == "--list-devices") {
        list_devices();
    }
}
