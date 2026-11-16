// wired up in Task 2 (run-as-admin plan)
#![allow(dead_code)]

use std::ffi::c_void;
use std::os::windows::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elev = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elev as *mut _ as *mut c_void),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elev.TokenIsElevated != 0
    }
}

fn lower_components(p: &Path) -> Vec<String> {
    p.components()
        .map(|c| match c {
            Component::Prefix(x) => x.as_os_str().to_string_lossy().to_lowercase(),
            c => c.as_os_str().to_string_lossy().to_lowercase(),
        })
        .collect()
}

/// Case-insensitive, component-wise prefix match.
pub fn in_program_files(exe: &Path, pf_dirs: &[PathBuf]) -> bool {
    let e = lower_components(exe);
    pf_dirs.iter().any(|d| {
        let d = lower_components(d);
        !d.is_empty() && e.starts_with(&d)
    })
}

pub fn program_files_dirs() -> Vec<PathBuf> {
    ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"]
        .iter()
        .filter_map(std::env::var_os)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .collect()
}

pub fn task_name(user: &str) -> String {
    format!("SirahMute-{user}")
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn task_xml(exe: &Path, user_id: &str, logon: bool) -> String {
    let user = esc(user_id);
    let trigger = if logon {
        format!("<LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger>")
    } else {
        String::new()
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers>{trigger}</Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>4</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{cmd}</Command>
      <Arguments>--from-task</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
        cmd = esc(&exe.to_string_lossy())
    )
}

fn user_name() -> Result<String, String> {
    match std::env::var("USERNAME") {
        Ok(u) if !u.is_empty() => Ok(u),
        _ => Err("USERNAME not set".into()),
    }
}

/// `DOMAIN\user`, falling back to COMPUTERNAME, then the bare user name.
fn user_id(domain: &str, computer: &str, user: &str) -> String {
    let d = if domain.is_empty() { computer } else { domain };
    if d.is_empty() {
        user.to_string()
    } else {
        format!("{d}\\{user}")
    }
}

fn schtasks(args: &[&str]) -> Result<(), String> {
    let out = Command::new("schtasks")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("schtasks: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    ))
}

pub fn create_task(logon: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if !in_program_files(&exe, &program_files_dirs()) {
        return Err("exe is not under Program Files".into());
    }
    let user = user_name()?;
    let env = |k| std::env::var(k).unwrap_or_default();
    let xml = task_xml(
        &exe,
        &user_id(&env("USERDOMAIN"), &env("COMPUTERNAME"), &user),
        logon,
    );
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
    // Admin-only-writable dir (Program Files), so an unelevated process cannot swap the XML.
    let dir = exe.parent().ok_or("exe has no parent dir")?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let file = dir.join(format!("task-{}-{nanos}.xml", std::process::id()));
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file)
            .map_err(|e| e.to_string())?;
        f.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    let r = schtasks(&[
        "/Create",
        "/TN",
        &task_name(&user),
        "/XML",
        &file.to_string_lossy(),
        "/F",
    ]);
    let _ = std::fs::remove_file(&file);
    r
}

pub fn delete_task() -> Result<(), String> {
    let name = task_name(&user_name()?);
    // Exit codes only: schtasks text is localized. Query fails = task absent.
    if schtasks(&["/Query", "/TN", &name]).is_err() {
        return Ok(());
    }
    schtasks(&["/Delete", "/TN", &name, "/F"])
}

pub fn run_task() -> Result<(), String> {
    schtasks(&["/Run", "/TN", &task_name(&user_name()?)])
}

/// Hands `target` to the already-running unelevated shell via explorer.
pub fn open_unelevated(target: &str) {
    let _ = Command::new("explorer.exe")
        .arg(target)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_program_files_rules() {
        let pf = vec![
            PathBuf::from(r"C:\Program Files"),
            PathBuf::from(r"C:\Program Files (x86)"),
        ];
        assert!(in_program_files(
            Path::new(r"C:\Program Files\SirahMute\sirahmute.exe"),
            &pf
        ));
        assert!(in_program_files(
            Path::new(r"c:\program files\SirahMute\sirahmute.exe"),
            &pf
        ));
        assert!(in_program_files(
            Path::new(r"C:\Program Files (x86)\SirahMute\sirahmute.exe"),
            &pf
        ));
        assert!(!in_program_files(
            Path::new(r"C:\Program Files Evil\sirahmute.exe"),
            &pf
        ));
        assert!(!in_program_files(
            Path::new(r"C:\ProgramFiles\sirahmute.exe"),
            &pf
        ));
        assert!(!in_program_files(
            Path::new(r"D:\Program Files\SirahMute\sirahmute.exe"),
            &pf
        ));
        assert!(!in_program_files(
            Path::new(r"C:\Users\a\AppData\Local\Programs\SirahMute\sirahmute.exe"),
            &pf
        ));
        assert!(!in_program_files(
            Path::new(r"C:\Program Files\SirahMute\sirahmute.exe"),
            &[]
        ));
    }

    #[test]
    fn task_xml_settings() {
        let x = task_xml(
            Path::new(r"C:\Program Files\Sirah & Co\sirahmute.exe"),
            r"PC\Budi",
            true,
        );
        for want in [
            "<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>",
            "<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>",
            "<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>",
            "<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>",
            "<Priority>4</Priority>",
            "<RunLevel>HighestAvailable</RunLevel>",
            "<LogonType>InteractiveToken</LogonType>",
            "<LogonTrigger>",
            r"<Command>C:\Program Files\Sirah &amp; Co\sirahmute.exe</Command>",
            "<Arguments>--from-task</Arguments>",
            r"<UserId>PC\Budi</UserId>",
        ] {
            assert!(x.contains(want), "missing {want}");
        }
        assert!(!task_xml(Path::new(r"C:\x.exe"), "u", false).contains("<LogonTrigger>"));
    }

    #[test]
    fn user_id_fallbacks() {
        assert_eq!(user_id("PC", "X", "Budi"), r"PC\Budi");
        assert_eq!(user_id("", "X", "Budi"), r"X\Budi");
        assert_eq!(user_id("", "", "Budi"), "Budi");
    }

    #[test]
    fn task_name_per_user() {
        assert_eq!(task_name("Budi"), "SirahMute-Budi");
    }
}
