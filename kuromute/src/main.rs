#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;
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
mod settings;
mod sound;
mod talk;
mod tray;
mod update;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};

fn attach_console() {
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

fn list_devices() {
    attach_console();
    unsafe {
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
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let adopted = config::adopt_legacy(&config::legacy_dir(), &config::data_dir());
    log::init(&config::data_dir().join("logs"));
    if adopted {
        log!("config: adopted config.json from the MuteMic folder");
    }
    log::install_panic_hook();
    let code = match std::env::args().nth(1).as_deref() {
        Some("--list-devices") => {
            list_devices();
            0
        }
        Some("--toggle") => {
            attach_console();
            if app::post_to_core(msg::MSG_TOGGLE) {
                0
            } else {
                println!("not running");
                1
            }
        }
        Some("--settings") => match settings::single::acquire() {
            Some(_guard) => settings::ui::run(),
            None => 0,
        },
        _ => app::run(),
    };
    std::process::exit(code);
}
