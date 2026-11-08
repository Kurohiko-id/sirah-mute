//! Run-at-login via HKCU\...\Run value `MuteMic` (same value name as v1).

use std::path::Path;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW,
    RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::core::{Result, w};

const RUN_KEY: windows::core::PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE: windows::core::PCWSTR = w!("MuteMic");

/// Registry data for the Run value: the exe path in double quotes.
pub fn desired_value(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

pub fn apply(enabled: bool) -> Result<()> {
    let mut key = HKEY::default();
    unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            None,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            &mut key,
        )
        .ok()?;
    }
    let r = if enabled {
        std::env::current_exe()
            .map_err(|e| windows::core::Error::new(windows::core::HRESULT(-1), e.to_string()))
            .and_then(|exe| write_if_different(key, &desired_value(&exe)))
    } else {
        match unsafe { RegDeleteValueW(key, VALUE) } {
            ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
            e => e.ok(),
        }
    };
    unsafe {
        let _ = RegCloseKey(key);
    }
    r
}

fn write_if_different(key: HKEY, want: &str) -> Result<()> {
    if read(key).as_deref() == Some(want) {
        return Ok(());
    }
    let data: Vec<u8> = want
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    unsafe { RegSetValueExW(key, VALUE, None, REG_SZ, Some(&data)).ok() }
}

fn read(key: HKEY) -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut len = std::mem::size_of_val(&buf) as u32;
    unsafe {
        RegQueryValueExW(
            key,
            VALUE,
            None,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
    }
    .ok()
    .ok()?;
    let s = &buf[..(len as usize / 2).min(buf.len())];
    let s = s.strip_suffix(&[0]).unwrap_or(s);
    Some(String::from_utf16_lossy(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desired_value_is_quoted_path() {
        assert_eq!(
            desired_value(Path::new(r"C:\Program Files\Kuro\kuromute.exe")),
            r#""C:\Program Files\Kuro\kuromute.exe""#
        );
    }
}
