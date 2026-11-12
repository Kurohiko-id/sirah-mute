#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;
mod audio;
mod autostart;
mod config;
mod enforcer;
mod geometry;
mod hotkey;
mod http;
mod i18n;
mod log;
mod migrate;
mod monitors;
mod msg;
mod others;
mod overlay;
mod policy_config;
mod prompt;
mod settings;
mod sound;
mod stats;
mod talk;
mod tray;
mod update;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::Storage::FileSystem::WriteFile;
use windows::Win32::System::Console::{
    ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};

fn attach_console() {
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

/// One line to stdout: the inherited handle first (pipe from PowerShell/HASS Agent), parent console
/// only if there is none. Never creates a console window.
fn print_stdout(word: &str) {
    unsafe {
        let ok = GetStdHandle(STD_OUTPUT_HANDLE)
            .ok()
            .filter(|h| !h.is_invalid() && !h.0.is_null())
            .is_some_and(|h| {
                let line = format!("{word}
");
                let mut n = 0u32;
                WriteFile(h, Some(line.as_bytes()), Some(&mut n), None).is_ok()
            });
        if !ok {
            attach_console();
            println!("{word}");
        }
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
    stats::init();
    stats::install_panic_hook();
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
        Some("--status") => app::remote(0),
        Some("--mute") => app::remote(1),
        Some("--unmute") => app::remote(2),
        Some("--settings") => match settings::single::acquire() {
            Some(_guard) => settings::ui::run(),
            None => 0,
        },
        _ => app::run(),
    };
    std::process::exit(code);
}
