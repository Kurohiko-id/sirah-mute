# Run as Admin Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An opt-in "Run as administrator" setting so the hotkey works while an elevated window (OBS as admin, League of Legends, Task Manager) has focus.

**Architecture:** Admin mode runs the core from a per-user Task Scheduler task with highest run level, so it starts elevated with no UAC prompt at logon or on a manual launch. A non-elevated launch with admin mode on asks the task to start and exits. Turning the setting on or off runs a short elevated helper (`--admin-on` / `--admin-off`, one UAC prompt) that creates or deletes the task and restarts the core. The setting is only offered when the exe lives under Program Files, because an exe in a user-writable folder must never run elevated automatically.

**Tech Stack:** Rust, `windows` 0.62.2, Slint 1.18, Inno Setup 6, `schtasks.exe`.

**Spec:** Design approved in chat on 2026-11-16 (no separate spec file). Its decisions are summarized in "Design" below; treat that section as the spec.

## Design (approved 2026-11-16)

- Settings, General tab: checkbox "Run as administrator", off by default, with the hint "So the hotkey works in games and OBS running as admin." It is disabled outside Program Files, and then a hint says to reinstall with "Install for all users".
- Installer: Inno dialog asks "Install for me only" (current behaviour, `%LOCALAPPDATA%\Programs\SirahMute`) or "Install for all users" (`{autopf}\SirahMute`, needs admin). An all-users install silently uninstalls this user's per-user copy. Config stays in `%APPDATA%\SirahMute`.
- Turning it on: UAC once. Turning it off: no prompt when settings is already elevated, else UAC once. Never at logon, never on a manual launch.
- Autostart in admin mode = the task's logon trigger (present only when "Start with Windows" is on); the HKCU Run value is removed. Admin mode off = today's Run value.
- While elevated: `--mute/--unmute/--status/--toggle` and settings→core messages keep working (UIPI filter), the tray icon comes back after an Explorer restart, and every link opens in a non-elevated browser.
- Release as v2.2.0 together with the Kuro Stats fix already on main (`bf45dbb`, `a1ab78c`).

## Global Constraints

- No new crates. Use `windows` features already enabled; add a feature to Cargo.toml only if a needed API is missing (`Win32_Security` is already on).
- Task name: `SirahMute-<USERNAME>` (one task per Windows user).
- The task's action passes `--from-task` so a task-started instance never tries to relaunch itself.
- Task XML must set `ExecutionTimeLimit` to `PT0S`, `DisallowStartIfOnBatteries` false, `StopIfGoingOnBatteries` false, `MultipleInstancesPolicy` `IgnoreNew`, `Priority` 4, principal `LogonType` `InteractiveToken`, `RunLevel` `HighestAvailable`. (Defaults would kill the app after 72 h, skip it on battery and run it below normal priority.)
- Every user-visible string goes through `i18n.rs` with EN and ID text.
- Commits without AI attribution, split per topic. Branch `v2.2` from `main`.
- Never run the no-arg core while the user's installed instance is running (tests only; manual testing is done by the user).

## Review Focus

1. UAC cancelled when turning it on: the checkbox goes back off, config `run_as_admin` is false, no task exists, the core keeps running unelevated. (Task 3, `admin_toggle_reverts_on_failure`.)
2. Task present but broken (exe moved, task deleted by hand, schtasks fails): a non-elevated launch logs the failure and continues as a normal unelevated core, no exit loop. (Task 2, `relaunch_decision_table`.)
3. A task-started core never relaunches itself, even if elevation is somehow missing (`--from-task`). (Task 2, `relaunch_decision_table`.)
4. Settings opened from the Start-menu shortcut (unelevated) while the core is elevated: saving reloads the core and the hotkey change applies. (Task 2 UIPI filter; manual test M4.)
5. A path that only looks like Program Files (`C:\Program Files Evil\`, `C:\ProgramFiles\`, a different drive) is not accepted. (Task 1, `in_program_files_rules`.)

---

### Task 1: `admin` module (pure helpers + system calls)

**Files:**
- Create: `kuromute/src/admin.rs`
- Modify: `kuromute/src/main.rs` (add `mod admin;`)

**Interfaces:**
- Produces:
  - `pub fn is_elevated() -> bool`: `OpenProcessToken` + `GetTokenInformation(TokenElevation)`; false on any error.
  - `pub fn in_program_files(exe: &Path, pf_dirs: &[PathBuf]) -> bool`: case-insensitive, component-wise prefix match (not a string prefix).
  - `pub fn program_files_dirs() -> Vec<PathBuf>`: from env `ProgramFiles`, `ProgramW6432`, `ProgramFiles(x86)` (skip unset/empty).
  - `pub fn task_name(user: &str) -> String` → `format!("SirahMute-{user}")`.
  - `pub fn task_xml(exe: &Path, user_id: &str, logon: bool) -> String`: Task Scheduler 1.2 XML with the Global Constraints settings; `<LogonTrigger><UserId>` present only when `logon`; `<Exec><Command>` = exe, `<Arguments>--from-task</Arguments>`. XML-escape `& < > " '` in the path and user.
  - `pub fn create_task(logon: bool) -> Result<(), String>`: writes `task_xml(current_exe, "<USERDOMAIN>\\<USERNAME>", logon)` to a temp file as UTF-16LE with BOM, runs `schtasks /Create /TN <name> /XML <file> /F` hidden (`CREATE_NO_WINDOW`), deletes the temp file, maps a non-zero exit to `Err` with stdout+stderr.
  - `pub fn delete_task() -> Result<(), String>`: `schtasks /Delete /TN <name> /F`; "cannot find" counts as Ok.
  - `pub fn run_task() -> Result<(), String>`: `schtasks /Run /TN <name>`.
  - `pub fn open_unelevated(target: &str)`: `explorer.exe <target>` (explorer hands it to the already-running unelevated shell). Used for URLs and for relaunching the exe.

- [ ] **Step 1: Write the failing tests** in `admin.rs`:

```rust
#[test]
fn in_program_files_rules() {
    let pf = vec![PathBuf::from(r"C:\Program Files"), PathBuf::from(r"C:\Program Files (x86)")];
    assert!(in_program_files(Path::new(r"C:\Program Files\SirahMute\sirahmute.exe"), &pf));
    assert!(in_program_files(Path::new(r"c:\program files\SirahMute\sirahmute.exe"), &pf));
    assert!(in_program_files(Path::new(r"C:\Program Files (x86)\SirahMute\sirahmute.exe"), &pf));
    assert!(!in_program_files(Path::new(r"C:\Program Files Evil\sirahmute.exe"), &pf));
    assert!(!in_program_files(Path::new(r"C:\ProgramFiles\sirahmute.exe"), &pf));
    assert!(!in_program_files(Path::new(r"D:\Program Files\SirahMute\sirahmute.exe"), &pf));
    assert!(!in_program_files(Path::new(r"C:\Users\a\AppData\Local\Programs\SirahMute\sirahmute.exe"), &pf));
    assert!(!in_program_files(Path::new(r"C:\Program Files\SirahMute\sirahmute.exe"), &[]));
}

#[test]
fn task_xml_settings() {
    let x = task_xml(Path::new(r"C:\Program Files\Sirah & Co\sirahmute.exe"), r"PC\Budi", true);
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
fn task_name_per_user() {
    assert_eq!(task_name("Budi"), "SirahMute-Budi");
}
```

- [ ] **Step 2:** `cargo test admin` → FAIL (module missing).
- [ ] **Step 3:** Implement the functions above. `is_elevated`, `create_task`, `delete_task`, `run_task`, `open_unelevated` are not unit-tested (system side effects); keep each a few lines.
- [ ] **Step 4:** `cargo test admin` → PASS; `cargo clippy --all-targets -- -D warnings` clean.
- [ ] **Step 5:** Commit `feat(admin): elevation check, Program Files rule, scheduled task helpers`.

### Task 2: Core wiring (relaunch, helper commands, autostart, UIPI, links)

**Files:**
- Modify: `kuromute/src/config.rs` (field `run_as_admin: bool`, default false, same per-key tolerant parse as the other bools)
- Modify: `kuromute/src/main.rs` (no-arg path + `--from-task`, `--admin-on`, `--admin-off`)
- Modify: `kuromute/src/app.rs` (`open_url`, `apply_autostart`, UIPI filter after window creation)
- Modify: `kuromute/src/admin.rs`

**Interfaces:**
- Consumes: Task 1 functions.
- Produces:
  - `pub fn relaunch_decision(run_as_admin: bool, elevated: bool, from_task: bool, in_pf: bool) -> bool` in `admin.rs`: true = call `run_task()` and exit 0 if it succeeds. True only when `run_as_admin && !elevated && !from_task && in_pf`.
  - `pub fn helper(on: bool) -> i32` in `admin.rs` (the `--admin-on` / `--admin-off` entry): on = `create_task(cfg.autostart)` → close the running core → `run_task()`; off = `delete_task()` → close the core → `open_unelevated(current_exe)`. "Close the core" = `PostMessageW(WM_CLOSE)` to `FindWindowW(WINDOW_CLASS)`, then poll `FindWindowW` every 100 ms for up to 5 s. Returns 0 on success, 1 on failure (logged). Refuses (returns 1) when `!is_elevated()` or the exe is not in Program Files.

- [ ] **Step 1: Failing tests.** In `admin.rs`:

```rust
#[test]
fn relaunch_decision_table() {
    assert!(relaunch_decision(true, false, false, true));
    assert!(!relaunch_decision(false, false, false, true)); // mode off
    assert!(!relaunch_decision(true, true, false, true));   // already elevated
    assert!(!relaunch_decision(true, false, true, true));   // started by the task: never loop
    assert!(!relaunch_decision(true, false, false, false)); // not in Program Files
}
```

In `config.rs`: `run_as_admin` defaults to false, survives a round trip, and a bad value (`"run_as_admin": "yes"`) resets only that key.

- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3: Implement.**
  - `main.rs` no-arg / `--from-task` path: read config with the existing read function. If `relaunch_decision(...)` and `run_task()` is Ok, return 0. Do this before the core takes its single-instance mutex. A failed `run_task` is logged and startup continues unelevated.
  - `--admin-on` / `--admin-off` → `std::process::exit(admin::helper(..))`.
  - `apply_autostart`: if `cfg.run_as_admin && admin::is_elevated()` → `autostart::apply(false)` then `admin::create_task(cfg.autostart)` (log errors). If `cfg.run_as_admin && !elevated` → log once and leave both alone. Otherwise today's behaviour.
  - UIPI: right after the core window is created and when `admin::is_elevated()`, call `ChangeWindowMessageFilterEx(hwnd, m, MSGFLT_ALLOW, None)` for `reg().reload, show, toggle, pause, remote, taskbar`.
  - `open_url`: when `admin::is_elevated()` → `admin::open_unelevated(url)`, else the current `ShellExecuteW`. All callers (tray, balloon, settings) already go through it.
- [ ] **Step 4:** `cargo test` all green, clippy clean.
- [ ] **Step 5:** Commit `feat(admin): relaunch through the task, admin-on/off helper, UIPI and unelevated links`.

### Task 3: Settings toggle

**Files:**
- Modify: `kuromute/ui/settings.slint` (General section, next to the autostart row)
- Modify: `kuromute/src/settings/umum.rs`, `kuromute/src/settings/ui.rs` (labels), `kuromute/src/i18n.rs`, `kuromute/src/stats.rs` (feature `run_as_admin`)

**Interfaces:**
- Consumes: `admin::in_program_files`, `admin::program_files_dirs`, `admin::is_elevated`; the `--admin-on/--admin-off` helper from Task 2.
- Produces: Slint properties `gen-admin` (in-out bool), `gen-admin-enabled` (in bool), `gen-admin-busy` (in bool), `gen-admin-error` (in string); callback `gen-admin-toggled()`. i18n keys `AdminLabel`, `AdminHint`, `AdminNeedsAllUsers`, `AdminFailed`.

Copy:

| Key | EN | ID |
|---|---|---|
| AdminLabel | Run as administrator | Jalankan sebagai admin |
| AdminHint | So the hotkey works in games and OBS running as admin. Asks for permission once. | Supaya hotkey jalan di game dan OBS yang jalan sebagai admin. Minta izin sekali. |
| AdminNeedsAllUsers | Needs Sirah Mute installed for all users. Run the installer again and choose "Install for all users". | Perlu Sirah Mute yang di-install untuk semua pengguna. Jalankan installer lagi dan pilih "Install for all users". |
| AdminFailed | Couldn't change it. Permission was declined or Task Scheduler failed; see the log. | Gagal mengubah. Izin ditolak atau Task Scheduler gagal; cek log. |

- [ ] **Step 1: Failing tests** in `umum.rs` (pure parts only):
  - `fn admin_row(in_pf: bool) -> (bool, Option<Key>)` (enabled, hint): `(true, None)` in Program Files, `(false, Some(Key::AdminNeedsAllUsers))` outside. Test `admin_row_state`.
  - `fn after_helper(want: bool, exit_ok: bool) -> (bool, Option<Key>)` (checkbox + config value, error): `(want, None)` on success, `(!want, Some(Key::AdminFailed))` on failure. Test `admin_toggle_reverts_on_failure`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3: Implement.** On toggle: set busy, save config with the new value through the existing `commit`, then on a worker thread run the helper with `ShellExecuteExW` verb `runas`, `SEE_MASK_NOCLOSEPROCESS`, `SW_HIDE`, wait for the process and read its exit code (a launch error such as `ERROR_CANCELLED` counts as failure). Back on the UI thread apply `after_helper`; on failure commit the reverted value. Settings itself stays open. Add `on(c.run_as_admin, "run_as_admin")` in `stats::features` and set it in `features_never_carry_user_data`.
- [ ] **Step 4:** `cargo test` green, clippy clean; the i18n completeness test passes.
- [ ] **Step 5:** Commit `feat(settings): Run as administrator toggle`.

### Task 4: Installer, README, version

**Files:**
- Modify: `installer/sirah-mute.iss`, `README.md`, `README.id.md`, `kuromute/Cargo.toml` (version `2.2.0`)

- [ ] **Step 1: Installer.**
  - `[Setup]`: keep `PrivilegesRequired=lowest`; add `PrivilegesRequiredOverridesAllowed=dialog`, `UsePreviousPrivileges=no`; `DefaultDirName={autopf}\SirahMute`. In lowest mode `{autopf}` resolves to `{localappdata}\Programs`, so per-user installs keep today's path.
  - Admin mode, `CurStepChanged(ssInstall)`: if `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{B131B4B2-6950-40F6-AD60-8683270943B1}_is1` exists, run its `QuietUninstallString` (fallback: `UninstallString` + ` /VERYSILENT /SUPPRESSMSGBOXES /NORESTART`) and wait.
  - Admin mode, `CurUninstallStepChanged(usPostUninstall)`: `schtasks /Delete /TN "SirahMute-<USERNAME>" /F`, hidden.
- [ ] **Step 2: README EN + ID.**
  - New short section "Hotkey not working in games or OBS?": admin windows, the setting, the one-time UAC prompt, "Install for all users".
  - Add `run_as_admin` to the Privacy feature list.
- [ ] **Step 3:** Bump `Cargo.toml` to `2.2.0`, `cargo build --release` with the usual RUSTFLAGS, verify the exe has no `C:\Users\` text.
- [ ] **Step 4:** Commits: `feat(installer): optional install for all users`, `docs(readme): run as administrator`, `chore: 2.2.0`.

### Manual tests (user, after Task 4)

- M1 Per-user install (current): the checkbox is disabled with the "Install for all users" hint.
- M2 Install for all users: the old per-user copy is gone, settings and stats are kept, autostart works.
- M3 Turn it on: UAC once, the tray icon blinks off and on, F13 works with Task Manager focused and in LoL/OBS.
- M4 Open "Sirah Mute Settings" from the Start menu (unelevated), change the hotkey: it applies. `--status` from PowerShell/HASS works.
- M5 Reboot: no UAC, the app runs elevated (Task Manager "Elevated" column = Yes).
- M6 Cancel the UAC prompt: the checkbox goes back off with the error text.
- M7 Turn it off: the app restarts unelevated, the task is gone from Task Scheduler, autostart uses the Run value again.
- M8 Restart Explorer while elevated: the tray icon returns. Report bug / Saweria open in the normal browser.
