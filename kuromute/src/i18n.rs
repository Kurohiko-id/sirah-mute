use crate::config::Lang;
use windows::Win32::Globalization::GetUserDefaultUILanguage;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResolvedLang {
    En,
    Id,
}

pub fn from_langid(id: u16) -> ResolvedLang {
    if id & 0x3FF == 0x21 {
        ResolvedLang::Id
    } else {
        ResolvedLang::En
    }
}

pub fn resolve(lang: Lang) -> ResolvedLang {
    match lang {
        Lang::En => ResolvedLang::En,
        Lang::Id => ResolvedLang::Id,
        Lang::Auto => from_langid(unsafe { GetUserDefaultUILanguage() }),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Muted,
    Unmuted,
    MicDisconnected,
    NoMicChosen,
    MenuSettings,
    MenuGuard,
    MenuCheckUpdate,
    MenuReportBug,
    MenuExit,
    DriftNotify,
    GuardPaused,
    UpdateAvailable,
    UpToDate,
    HotkeyFailed,
    HotkeyInvalid,
    ConfigCorrupt,
    AppName,
    TabMic,
    TabIndicator,
    TabDevice,
    TabGeneral,
    TabPlaceholder,
}

impl Key {
    #[cfg(test)]
    pub const ALL: [Key; 22] = [
        Key::Muted,
        Key::Unmuted,
        Key::MicDisconnected,
        Key::NoMicChosen,
        Key::MenuSettings,
        Key::MenuGuard,
        Key::MenuCheckUpdate,
        Key::MenuReportBug,
        Key::MenuExit,
        Key::DriftNotify,
        Key::GuardPaused,
        Key::UpdateAvailable,
        Key::UpToDate,
        Key::HotkeyFailed,
        Key::HotkeyInvalid,
        Key::ConfigCorrupt,
        Key::AppName,
        Key::TabMic,
        Key::TabIndicator,
        Key::TabDevice,
        Key::TabGeneral,
        Key::TabPlaceholder,
    ];
}

/// `{ver}` / `{key}` placeholders are replaced by callers via `str::replace`.
pub fn t(l: ResolvedLang, k: Key) -> &'static str {
    use Key::*;
    use ResolvedLang::{En, Id};
    match (k, l) {
        (Muted, En) => "Microphone Muted",
        (Muted, Id) => "Mikrofon Dimatikan",
        (Unmuted, En) => "Microphone Unmuted",
        (Unmuted, Id) => "Mikrofon Aktif",
        (MicDisconnected, En) => "Microphone not connected",
        (MicDisconnected, Id) => "Mikrofon tidak terhubung",
        (NoMicChosen, En) => "No microphone chosen — open Settings",
        (NoMicChosen, Id) => "Belum pilih mikrofon — buka Settings",
        (MenuSettings, En) => "Settings…",
        (MenuSettings, Id) => "Pengaturan…",
        (MenuGuard, En) => "Guard devices",
        (MenuGuard, Id) => "Jaga device",
        (MenuCheckUpdate, En) => "Check for Updates",
        (MenuCheckUpdate, Id) => "Cek Update",
        (MenuReportBug, En) => "Report Bug…",
        (MenuReportBug, Id) => "Laporkan Bug…",
        (MenuExit, En) => "Exit",
        (MenuExit, Id) => "Keluar",
        (DriftNotify, En) => "Default audio device changed from your pick. Click to set it back.",
        (DriftNotify, Id) => "Device audio default berubah dari pilihanmu. Klik untuk balikin.",
        (GuardPaused, En) => {
            "Another app keeps changing your audio device. Guard paused for 5 minutes."
        }
        (GuardPaused, Id) => {
            "Ada app lain yang terus ganti device audio. Penjagaan dijeda 5 menit."
        }
        (UpdateAvailable, En) => "Update available: {ver}. Click to open.",
        (UpdateAvailable, Id) => "Ada update: {ver}. Klik untuk buka.",
        (UpToDate, En) => "You're up to date.",
        (UpToDate, Id) => "Sudah versi terbaru.",
        (HotkeyFailed, En) => "Shortcut {key} is in use by another app. Pick another in Settings.",
        (HotkeyFailed, Id) => "Tombol {key} sudah dipakai app lain. Ganti di Pengaturan.",
        (HotkeyInvalid, En) => "Shortcut {key} can't be used. Pick another in Settings.",
        (HotkeyInvalid, Id) => "Tombol {key} tidak bisa dipakai. Ganti di Pengaturan.",
        (ConfigCorrupt, En) => {
            "Settings file was damaged and has been reset. Backup: config.json.bad"
        }
        (ConfigCorrupt, Id) => "File pengaturan rusak dan direset. Cadangan: config.json.bad",
        (TabMic, _) => "Mic",
        (TabIndicator, En) => "Indicator",
        (TabIndicator, Id) => "Penanda",
        (TabDevice, _) => "Device",
        (TabGeneral, En) => "General",
        (TabGeneral, Id) => "Umum",
        (TabPlaceholder, En) => "Coming soon",
        (TabPlaceholder, Id) => "Segera hadir",
        (AppName, _) => "Kuro Mute & Force Mic",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn langid_mapping() {
        assert_eq!(from_langid(0x0421), ResolvedLang::Id);
        assert_eq!(from_langid(0x0409), ResolvedLang::En);
    }

    #[test]
    fn id_muted() {
        assert_eq!(t(ResolvedLang::Id, Key::Muted), "Mikrofon Dimatikan");
    }

    #[test]
    fn all_keys_non_empty() {
        for k in Key::ALL {
            assert!(!t(ResolvedLang::En, k).is_empty());
            assert!(!t(ResolvedLang::Id, k).is_empty());
        }
    }
}
