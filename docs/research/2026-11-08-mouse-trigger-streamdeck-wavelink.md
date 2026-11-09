# Research: mouse-button mute trigger, Stream Deck, Wave Link (2026-11-08)

Research for the Kuro Mute & Force Mic rewrite. Read-only investigation on the user's PC (Windows 11 Pro 26100) plus primary web sources.

Status labels used below:

- **Verified**: confirmed from a primary source (official docs, first-party code, or a file/command output on this PC).
- **Inferred**: reasoned from verified facts, or taken from a non-primary (community) source.
- **Not found**: looked for it, could not establish it.

## TL;DR (santai)

- Mouse NOIR S1 itu brand Noir Gear (tokonya Indo), jeroannya platform OEM Cina "YJX" (yjx2012.com). Configurator-nya web (WebHID, Chrome/Edge) di https://noirgear.yjx2012.com/, dan setting tombol disimpen di mouse (onboard), jadi abis save software gak perlu jalan.
- Tombol bisa di-set ke: klik/back/forward/scroll, tombol keyboard (F1–F12 doang, F13–F24 gak ada di picker), preset combo Ctrl/Alt/Win, tombol multimedia (Mute, Vol, Play, Calculator, My Computer, dll), macro, fire key, DPI.
- Ctrl bocor ke game itu emang sifat RegisterHotKey: Ctrl tetep keystroke biasa. Game juga bisa matiin hotkey app lain pake `RIDEV_NOHOTKEYS`, dan Microsoft bilang F12 "reserved" buat debugger. Jadi Ctrl+F12 itu pilihan paling apes.
- Paling aman: set tombol mouse ke 1 tombol multimedia yang gak kepake (mis. "My Computer"), terus app baca lewat Raw Input (jalan di background, bisa tau datangnya dari mouse mana). Opsional pasang LL hook kecil buat "nelen" key itu biar Windows gak buka Explorer.
- Bikin mouse "jadi Stream Deck" beneran: gak bisa (gak ada API device pihak ketiga, fake HID butuh driver signed). Yang realistis: plugin Stream Deck sendiri, plus app nembak deep link `streamdeck://plugins/message/<uuid>/...?streamdeck=hidden`.
- Wave Link 2.0.6 bisa dikontrol lewat WebSocket JSON-RPC lokal di port 1824 (gak ada dokumentasi resmi). Mute mic = `setInputConfig` dengan property `"Mute"` ke input `"Wave Link Aux 2"` (stream/monitor mix). API Wave Link 3 beda total.

## How this was gathered (read-only notes)

- **Local reads.** Files read: Stream Deck plugin folders, Stream Deck and plugin logs, and this repo's `HotkeyManager.cs` and `TrayApp.cs`. Commands run: process list, `netstat`, PnP device properties, and a registry read of `HKCR\streamdeck`.
- **HID descriptor caps (query only).** NOIR S1 caps came from `GetRawInputDeviceInfo(RIDI_PREPARSEDDATA)`. For the mouse and keyboard TLCs, a **zero-access** handle was opened and `HidD_GetPreparsedData` was called. No reports were read, and nothing was sent to any USB/HID device.
- **Wave Link.** Two read-only JSON-RPC calls (`getApplicationInfo`, `getInputConfigs`). No `set*` call was made.
- **Web.** Noir pages and the configurator's public JS bundles were fetched to stdout, without logging in. The bundles are hashed Next.js chunks, so their names will change on redeploy.
- **Temp files.** Helper scripts lived in the session scratchpad and were deleted afterwards.

---

## 1. NOIR S1

### 1.1 Who makes it, and which OEM platform

- **Verified.** Brand: **Noir Gear** (noirgear.com). The product page lists `Chipset: YJX2012` and `Software: Web Based`, alongside PAW3311, 1000 Hz, and 2.4 GHz / BT 5.0 / wired.
  Source: https://www.noirgear.com/products/noir-s1-mouse
- **Inferred.** Indonesian brand: the store is priced in IDR and the official store is on Tokopedia ("Noir Gear Official Store"). The About page does not state a headquarters (**Not found**).
  Sources: https://www.noirgear.com/pages/about-us, https://www.tokopedia.com/find/noir-s1
- **Verified.** The official configurator is hosted on the OEM's platform: `noirgear.yjx2012.com`, with assets under `cdn.yjx2012.com/noirgear/_next/...`. The yjx2012.com domain hosts web drivers for many other brands too (AJAZZ, K-Snake, LANGTU, KYSONA, and others; **Inferred** from search results).
  Source: https://www.noirgear.com/pages/software
- **Verified.** The configurator's English locale still contains a leftover product name from another brand (`"10_name":"MCHOSE G3 A"`). That points to a shared white-label codebase (**Inferred**).
  Source: https://cdn.yjx2012.com/noirgear/_next/static/chunks/124.654464ed87a9588f.js
- **Inferred (community, non-primary).** "YJX-CHIP" = 深圳市易捷行电子 (Shenzhen Yijiexing), a mouse "solution provider" (方案商). A GitHub issue reports the same **A8A5:2255** VID:PID on a different brand's PAW3311 mouse, with the same vendor channel (usage page 0xFF01, usage 0x10; 64-byte output report ID 0xF0).
  Sources: https://github.com/HeyOkay/HaloBattery/issues/275, https://github.com/OpenMouse-Project/mouse-protocol/pull/177
- **Not found.** The actual MCU part number.

**VID meaning:**

- **Verified (configurator code).** The configurator labels vendorId `43173` (0xA8A5) as `"<name> 2.4g"`. It accepts mouse VIDs `43172` (0xA8A4), `43173` (0xA8A5) and `14391` (0x3837), and keyboard VIDs `43174` (0xA8A6) and `14391`. So the user's **VID_A8A5 PID_2255 is the 2.4 GHz receiver path**.
- **Inferred (community PR #177).** Wired mode uses VID 0xA8A4.
- Source: https://cdn.yjx2012.com/noirgear/_next/static/chunks/515-e3455940c7e5a94f.js

### 1.2 Official configurator

- **Verified.** Configurator URL: **https://noirgear.yjx2012.com/**, linked from Noir's "Software & Manual Book" page for the Noir S1 (the E1 uses the same URL). The manual is a Google Drive folder with one file, "Noir S1 Manual Book.pdf".
  Source: https://www.noirgear.com/pages/software
- **Verified.** It is a WebHID app. It calls `navigator.hid.requestDevice({filters:[{usagePage:65281,usage:16}]})`, i.e. usage page 0xFF01, usage 0x10, and its UI says "Only supports Chrome and Edge browsers" (`13_tip`).
  Sources: chunk `515-…js` and locale chunk `124-…js` above.
- **Verified (local).** On this PC, the 0xFF01/0x10 collection is `HID\VID_A8A5&PID_2255&MI_02` (see 1.7).
- **Not verified.** The product page's "Open Device Hub" button points to https://hub.noirgear.com/ ("Configure supported Noir mice and keyboards in one place"). The page does not say whether the S1 is supported there.
- **Not verified.** The manual PDF is almost entirely outlined artwork. The only extractable text is about Bluetooth pairing ("Press the mode button to switch to Bluetooth Mode…"). Nothing about remapping or onboard memory could be read.

### 1.3 Exactly what a button can be assigned to

**Verified.** Source: the configurator's key-picker JSON in chunk `515-e3455940c7e5a94f.js` (objects `system`, `systemKey`, `Keyboard` and `Other`), plus English labels from `124-…js`. Each assignment is stored as `{type, code1, code2, code3}`.

| Category in UI | Entries | Encoding |
| --- | --- | --- |
| Mouse | Left, Right, Middle, Backward, Forward, Scroll up, Scroll down, DPI loop, Disable | `type 32` = HID button bitmask 1/2/4/8/16 (0 = disabled). `type 33` = wheel (code1 `56` = usage 0x38, code2 ±1) or DPI loop (`85`) |
| Keyboard | 102 single keys: A–Z, 0–9, punctuation, **F1–F12**, Esc/Tab/Caps/Backspace/Enter/Space, Ins/Del/Home/End/PgUp/PgDn, arrows, Pause, Scroll Lock, Num Lock, numpad, and the 8 modifiers | `type 16` = (modifier byte, HID Keyboard-page usage). Highest usage offered is `231` (Right GUI) |
| Edit / Navigation / Windows / Efficiency presets | Ctrl+X/C/V/Z/Y/A/S/N/O/T/W/−/=/0; Alt+Left/Right; Win+Tab/D/T/A/E/I/L/K/B/G/R/S/U/X/./=; Win+Ctrl+Enter; Ctrl+Shift+Esc; Ctrl+Esc; Alt+Tab; Alt+F4; Alt+Esc | `type 16` with modifier bits |
| Multimedia / System | See 1.5 | `type 48` = 16-bit **Consumer page** usage (code1 = low byte, code2 = high byte) |
| DPI | DPI+, DPI−, DPI switch | `type 240` |
| Macro | Recorded macros with loop modes (hold-to-loop, loop until another key, loop until same key, repeat) | `type 112` ("Macro N") |
| Fire key | Rapid clicks with interval and count ("Left button cannot be configured as fire key") | — |

Notes:

- **Verified.** "Left button modification not allowed" (`leftBtnNoChange`).
- **Verified.** `setMouseKeys()` writes **6 button slots** per profile.
- **Not verified.** There is also a "Game Safe Mode" setting ("Enabling this mode may reduce mouse performance."). What it changes is unknown.

### 1.4 Can it send F13–F24?

- **Verified: not from the key picker.** The `Keyboard` list has 102 entries, F1–F12 only, and no F13–F24.
- **Verified: a code path exists.** The macro editor stores the browser's `KeyboardEvent.code` per step (`webCode: e.code`). The configurator's code→HID map includes `F13…F24 → 104…115`, which is HID 0x68–0x73.
- **Verified (local): the device can represent them.** The mouse's keyboard collection (`MI_01&COL01`) declares a key array of Keyboard-page usages **0x00–0xFF** plus modifiers 0xE0–0xE7. F13–F24 (0x68–0x73) are therefore representable in its reports.
- **Not verified.** Whether a recorded macro containing F13–F24 is accepted by the firmware and played back. Recording one also needs a keyboard that actually emits F13–F24 in the browser.

### 1.5 Multimedia / consumer keys offered

**Verified.** The offered entries and their Consumer usages come from the configurator JSON. The Windows scan codes come from Microsoft's HID-usage→scan-code table, which says the scan code "is delivered in WM_KEYDOWN/WM_KEYUP/WM_SYSKEYDOWN/WM_SYSKEYUP and WM_INPUT messages".
Source: https://learn.microsoft.com/en-us/windows/win32/inputdev/about-keyboard-input#scan-codes

The VK column is **Inferred** (the standard mapping; not stated in that table). VK values are from https://learn.microsoft.com/en-us/windows/win32/inputdev/virtual-key-codes.

| Configurator label | Consumer usage | Scan code (MS table) | VK (inferred) |
| --- | --- | --- | --- |
| Volume + | 0x00E9 | 0xE030 | VK_VOLUME_UP (0xAF) |
| Volume − | 0x00EA | 0xE02E | VK_VOLUME_DOWN (0xAE) |
| Mute | 0x00E2 | 0xE020 | VK_VOLUME_MUTE (0xAD) |
| Play/Pause | 0x00CD | 0xE022 | VK_MEDIA_PLAY_PAUSE (0xB3) |
| Stop | 0x00B7 | 0xE024 | VK_MEDIA_STOP (0xB2) |
| Prev Track | 0x00B6 | 0xE010 | VK_MEDIA_PREV_TRACK (0xB1) |
| Next Track | 0x00B5 | 0xE019 | VK_MEDIA_NEXT_TRACK (0xB0) |
| Multimedia | 0x0183 | 0xE06D | VK_LAUNCH_MEDIA_SELECT (0xB5) |
| Homepage | 0x0223 | 0xE032 | VK_BROWSER_HOME (0xAC) |
| Web-Refresh | 0x0227 | 0xE067 | VK_BROWSER_REFRESH (0xA8) |
| Web-Stop | 0x0226 | 0xE068 | VK_BROWSER_STOP (0xA9) |
| Web-Forward | 0x0225 | 0xE069 | VK_BROWSER_FORWARD (0xA7) |
| Web-Backward | 0x0224 | 0xE06A | VK_BROWSER_BACK (0xA6) |
| Web-Favorites | 0x022A | 0xE066 | VK_BROWSER_FAVORITES (0xAB) |
| Web-Search | 0x0221 | 0xE065 | VK_BROWSER_SEARCH (0xAA) |
| Calculator | 0x0192 | 0xE021 | VK_LAUNCH_APP2 (0xB7) |
| My Computer | 0x0194 | 0xE06B | VK_LAUNCH_APP1 (0xB6) |
| Mail | 0x018A | 0xE06C | VK_LAUNCH_MAIL (0xB4) |
| Screen Brightness + / − | 0x006F / 0x0070 | not in MS table | Not found |

### 1.6 Does the assignment live in onboard memory?

- **Verified: yes.** Three pieces of evidence from the configurator:
  - UI text: "Once you have finished configuring, there's no need to keep the background running; just save, close, and enjoy." (`12_tip`).
  - Profiles include an "Onboard configuration" entry (`onboardConfig`).
  - The key mapping is written to the device: `setMouseKeys()` packs the 6 slots into a 64-byte report and sends it via `sendDeviceData`.
- **Not found.** The product page itself does not mention onboard memory.

### 1.7 HID layout of the receiver on this PC (local)

**Verified (local).** Source: `Get-PnpDevice` hardware IDs plus preparsed caps (method described above).

| Collection | Usage page : usage | Windows class | Input caps |
| --- | --- | --- | --- |
| MI_00 COL01 | 0x0001:0x0002 Mouse | Mouse | Button page usages 1–16 (report ID 2) |
| MI_00 COL02 | 0x000C:0x0001 Consumer | HIDClass | Consumer usages 0x0001–0x0514 (report ID 3) |
| MI_00 COL03 | 0x0001:0x0080 System control | HIDClass | – |
| MI_00 COL04 | 0xFFA5:0x0088 vendor | HIDClass | vendor 0x01–0xFF (report ID 9) |
| MI_01 COL01 | 0x0001:0x0006 Keyboard | Keyboard | modifiers 0xE0–0xE7 + key array 0x00–0xFF (report ID 1) |
| MI_01 COL02 | 0x000C:0x0001 Consumer | HIDClass | Consumer usages 0x0001–0x0514 (report ID 3) |
| MI_01 COL03 | 0x0001:0x0080 System control | HIDClass | – |
| MI_01 COL04 | 0xFF05:0x0002 vendor | HIDClass | usage 0x02 (report ID 10) |
| MI_01 COL05 | 0xFF06:0x0002 vendor | HIDClass | – |
| MI_02 | **0xFF01:0x0010 vendor (configurator channel)** | HIDClass | – |

- **Not verified.** Which of the two Consumer collections carries a consumer usage assigned to a mouse button. Finding out needs a WM_INPUT log while pressing the button; that test was not run.
- **Inferred.** Today's Ctrl+F12 comes out of `MI_01&COL01`, the receiver's keyboard collection. To a game it looks like a normal keyboard pressing Ctrl.

---

## 2. Windows input APIs (Microsoft Learn)

### 2a. RegisterHotKey

Source: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerhotkey

- **MOD_NOREPEAT (Verified).** "MOD_NOREPEAT 0x4000 — Changes the hotkey behavior so that the keyboard auto-repeat does not yield multiple hotkey notifications."
- **Modifier-less hotkeys.**
  - **Verified:** the docs only say "The fsModifiers parameter can be a combination of the following values." No sentence requires a modifier.
  - **Inferred:** 0 is accepted (common practice; not tested here).
- **Media/launch keys and F13–F24.**
  - **Verified:** `vk` is "The virtual-key code of the hot key. See Virtual Key Codes." That table lists `VK_F13` 0x7C … `VK_F24` 0x87, `VK_VOLUME_MUTE` 0xAD and `VK_LAUNCH_APP2` 0xB7. RegisterHotKey's docs exclude none of them.
  - **Inferred:** they register fine. Whether registering a media key suppresses Windows' default action is untested.
  - VK source: https://learn.microsoft.com/en-us/windows/win32/inputdev/virtual-key-codes. That page also warns: "Do not rely on the K_LWIN (0x5B) + VK_F17 (0x80) keys to permanently switch a setting."
- **F12 (Verified, directly relevant: the current app uses Ctrl+F12).** "The F12 key is reserved for use by the debugger at all times, so it should not be registered as a hot key. Even when you are not debugging an application, F12 is reserved in case a kernel-mode debugger or a just-in-time debugger is resident."
- **Conflicts (Verified).** "Typically, RegisterHotKey also fails if the keystrokes specified for the hot key have already been registered for another hot key."
- **Is the key withheld from the foreground window?**
  - **Not found** in the docs. They only say: "Upon finding a match, the system posts the WM_HOTKEY message to the message queue of the window with which the hot key is associated." The Keyboard Input Overview adds that WM_HOTKEY is "a message the system places at the top of a thread's message queue, bypassing any existing messages in the queue" (https://learn.microsoft.com/en-us/windows/win32/inputdev/about-keyboard-input#hot-key-support).
  - **Inferred:** a hotkey matches on the non-modifier key, so **Ctrl down/up are ordinary keystrokes delivered to the focused game**. A game reading Raw Input sees the full sequence (Ctrl, F12) no matter what. This matches the reported symptom.

### 2b. RAWINPUTDEVICE flags

Source: https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-rawinputdevice

- **RIDEV_NOHOTKEYS (Verified).** "If set, the application-defined keyboard device hotkeys are not handled. However, the system hotkeys; for example, ALT+TAB and CTRL+ALT+DEL, are still handled. By default, all keyboard hotkeys are handled. RIDEV_NOHOTKEYS can be specified even if RIDEV_NOLEGACY is not specified and hwndTarget is NULL."
  **Inferred:** a game that registers its keyboard with this flag stops other apps' RegisterHotKey hotkeys while it has focus. That is the documented mechanism behind "some games block global hotkeys". Which specific games use it: **Not found**.
- **RIDEV_INPUTSINK (Verified).** "If set, this enables the caller to receive the input even when the caller is not in the foreground. Note that hwndTarget must be specified."
- **RIDEV_EXINPUTSINK (Verified).** "If set, this enables the caller to receive input in the background only if the foreground application does not process it. In other words, if the foreground application is not registered for raw input, then the background application that is registered will receive the input."
  **Inferred:** games register for raw input, so a tray app must use **INPUTSINK**, not EXINPUTSINK.
- **RIDEV_DEVNOTIFY (Verified).** "If set, this enables the caller to receive WM_INPUT_DEVICE_CHANGE notifications for device arrival and device removal."
- **Default scope (Verified).** "By default, the operating system sends raw input from devices with the specified top level collection (TLC) to the registered application as long as it has the window focus."

### 2c. Raw Input per-device identification

- **Distinguishing devices (Verified).** "An application can distinguish the source of the input even if it is from the same type of device. For example, two mouse devices." Also: "An application can receive data when it is in the foreground and when it is in the background (if registered with RIDEV_INPUTSINK)."
  Source: https://learn.microsoft.com/en-us/windows/win32/inputdev/about-raw-input
- **GetRawInputDeviceInfo (Verified).**
  - `RIDI_DEVICENAME`: "pData points to a string that contains the device interface name. … For this uiCommand only, the value in pcbSize is the character count (not the byte count)."
  - `RIDI_DEVICEINFO`: "pData points to an RID_DEVICE_INFO structure."
  - Source: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getrawinputdeviceinfow
- **Where VID/PID come from.**
  - **Verified:** VID/PID are only in `RID_DEVICE_INFO_HID` ("dwVendorId — The vendor identifier for the HID", "dwProductId — The product identifier for the HID"), which applies when `dwType` is `RIM_TYPEHID`.
  - **Inferred:** for mice and keyboards, parse VID/PID from the `RIDI_DEVICENAME` string. On this PC those names look like `\\?\HID#VID_A8A5&PID_2255&MI_00&Col01#…` (**Verified local**).
  - Sources: https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-rid_device_info_hid, https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-rid_device_info
- **Consumer-control reports from one device.**
  - **Verified:** Windows opens Consumer controls (0x000C/0x0001) in **Shared** mode, while Mouse and Keyboard are **Exclusive** (https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/top-level-collections-opened-by-windows-for-system-use). So an app can register usage page 0x0C / usage 0x01 and get `RIM_TYPEHID` input.
  - **Verified:** per RAWHID, "Each WM_INPUT can indicate several inputs, but all of the inputs come from the same HID. The size of the bRawData array is dwSizeHid * dwCount." (https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-rawhid)
  - **Verified (local):** on the NOIR S1, both consumer TLCs use report ID 3 with a declared usage range of 0x0001–0x0514.
  - **Inferred:** parse the report with `HidP_GetUsages` using `RIDI_PREPARSEDDATA`.
- **Mouse button flags (Verified).** In `RAWMOUSE.usButtonFlags`:
  - `RI_MOUSE_BUTTON_4_DOWN 0x0040` "XBUTTON1 changed to down."
  - `RI_MOUSE_BUTTON_4_UP 0x0080`
  - `RI_MOUSE_BUTTON_5_DOWN 0x0100` "XBUTTON2 changed to down."
  - `RI_MOUSE_BUTTON_5_UP 0x0200`
  - Source: https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-rawmouse

### 2d. Low-level hooks (WH_MOUSE_LL / WH_KEYBOARD_LL)

Sources: https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelmouseproc, https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc

- **Blocking (Verified).** "If the hook procedure processed the message, it may return a nonzero value to prevent the system from passing the message to the rest of the hook chain or the target window procedure."
  **Inferred, must test:** blocking here also hides the event from games that read Raw Input. The docs only say "target window procedure".
- **Timeout and silent removal (Verified).** "The hook procedure should process a message in less time than the data entry specified in the LowLevelHooksTimeout value … The value is in milliseconds. If the hook procedure times out, the system passes the message to the next hook. However, on Windows 7 and later, the hook is silently removed without being called. There is no way for the application to know whether the hook is removed." Also: "Windows 10 version 1709 and later — The maximum timeout value the system allows is 1000 milliseconds (1 second)."
- **Threading (Verified).** "This hook is called in the context of the thread that installed it. … Therefore, the thread that installed the hook must have a message loop." Microsoft's advice: "it should run the hooks on a dedicated thread that passes the work off to a worker thread and then immediately returns. In most cases where the application needs to use low level hooks, it should monitor raw input instead."
- **.NET (Verified).** "In .NET apps, you must ensure the callback is not moved around by the garbage collector (otherwise your app will crash with an ExecutionEngineException)."
  Source: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowshookexw
- **Injected flags (Verified).**
  - MSLLHOOKSTRUCT: "Testing LLMHF_INJECTED (bit 0) will tell you whether the event was injected. If it was, then testing LLMHF_LOWER_IL_INJECTED (bit 1) will tell you whether or not the event was injected from a process running at lower integrity level."
  - KBDLLHOOKSTRUCT: "Testing LLKHF_INJECTED (bit 4) will tell you whether the event was injected …"
  - Sources: https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-msllhookstruct, https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-kbdllhookstruct
- **UIPI with an elevated window in front: Not found.** None of the fetched hook or raw-input pages describe it. The only integrity-level content is the `*_LOWER_IL_INJECTED` flags. Test this with an elevated game in the foreground before relying on hooks.

### 2e. Can user mode both identify the source device and block that event?

- **Verified: hooks have no device field.** `KBDLLHOOKSTRUCT` = vkCode, scanCode, flags, time, dwExtraInfo. `MSLLHOOKSTRUCT` = pt, mouseData, flags, time, dwExtraInfo.
- **Verified: Raw Input identifies the device but only filters for itself.** "If RIDEV_NOLEGACY is set for a mouse or a keyboard, the system does not generate any legacy message for that device **for the application**." The effect is scoped to the registering app.
- **Verified: the supported per-device route is a kernel filter driver.**
  - The Win32 subsystem "opens all keyboard and mouse devices for its exclusive use".
  - Device filter samples: "The sample filter service callbacks can be configured to modify the input data … For example, the filter service callback can delete, transform, or insert data."
  - Source (archived): https://learn.microsoft.com/en-us/previous-versions/windows/hardware/hid/keyboard-and-mouse-class-drivers
- **Verified: drivers need Microsoft signing.** "Starting with Windows 10, version 1607, Windows will not load any new kernel-mode drivers which are not signed by the Dev Portal."
  Source: https://learn.microsoft.com/en-us/windows-hardware/drivers/install/kernel-mode-code-signing-policy--windows-vista-and-later-
- **Verified: Scancode Map can't target one device.** It applies system-wide: "It isn't currently possible to create a map on a per-keyboard basis." (same archived page)
- **Conclusion (Inferred, from the absence of any API).** No documented user-mode API both tells the source device and blocks the event. Correlating Raw Input with an LL hook is unreliable, because the hook runs before WM_INPUT reaches you.
- **Practical answer for this PC (Inferred).** With one mouse, make the *trigger itself* unique (a consumer usage or XBUTTON the keyboard never sends). Then device identity is only a sanity filter, and a device-agnostic hook can swallow it.

---

## 3. Elgato Stream Deck (7.3.1 installed)

### 3a. Can an external program trigger a Stream Deck action or key?

- **Not found: no API to "press" an arbitrary key or action.** The SDK reference only covers the plugin ↔ Stream Deck WebSocket.
- **Verified: deep links are the official external → plugin channel.**
  - Format: `streamdeck://plugins/message/<PLUGIN_UUID>`.
  - Active deep links need SD 6.5+; passive ones need SD **7.0+** (this PC runs 7.3.1).
  - "By default, when your plugin receives a deep-link message, Stream Deck's window is brought to the foreground." For silent delivery you "must specify the query string parameter `streamdeck=hidden`".
  - "Deep-links are only accessible locally."
  - The plugin receives `didReceiveDeepLink`.
  - Source: https://docs.elgato.com/streamdeck/sdk/guides/deep-linking
  - **Verified (local):** `HKCR\streamdeck\shell\open\command` = `"C:\Program Files\Elgato\StreamDeck\StreamDeck.exe" "%1"`.
  - **Inferred:** a message reaches the plugin whose UUID is in the URL, i.e. your own plugin, not arbitrary actions.
- **Verified: Virtual Stream Deck.**
  - Requirements: "Windows 11 (64-bit) or later", "Stream Deck 7.0 or later", "Requires a supported Stream Deck or Corsair device".
  - "Launch each Virtual Stream Deck with its own hotkey, appearing at your mouse cursor." The hotkeys show/hide panels. **Not found:** any way for the keyboard or another program to press a VSD key.
  - Source: https://www.elgato.com/us/en/explorer/products/stream-deck/virtual-stream-deck-beta-overview/
  - `DeviceType 11 = Virtual Stream Deck`: https://docs.elgato.com/streamdeck/sdk/guides/devices/
  - **Verified (local):** `StreamDeck.1.log` says "Virtual Stream Deck in not enabled".
- **Not found: third-party devices.** No SDK mechanism registers a third-party input device (like a mouse) as a Stream Deck device. The device table lists only Elgato/Corsair products.
- **Observed, undocumented (don't rely on it).** StreamDeck.exe listens on `127.0.0.1:28196` (log: "Using port 28196 to communicate with plugins") and on `0.0.0.0:28198` (log: "[VSD2 Server] port 28198 is available; starting Bonjour").

### 3b. Plugin SDK

- **Native executable plugins: yes.**
  - **Verified (docs):** `CodePathWin`: "Path to the plugin's entry point specific to Windows; this is executed when the Stream Deck application starts the plugin on Windows." Listed examples include `Counter.exe`. Node plugins declare `"Nodejs": { "Version": "20" }` (allowed: "20", "24", "26"). Source: https://docs.elgato.com/streamdeck/sdk/references/manifest
  - **Verified (local):** `com.fredemmott.micmutetoggle.sdPlugin\manifest.json` → `"CodePathWin": "sdmicmute.exe"`. `dev.cazzar.streamdeck.vtubestudio.sdPlugin` → `"CodePathWin": "streamdeck-vtubestudio.exe"`, running as a process right now. The user's own `id.kurohiko.waku` plugin is Node 20.
- **Transport.**
  - **Verified:** the plugin is launched with `-port`, `-pluginUUID`, `-registerEvent` and `-info`. "Upon startup, your plugin must connect to the WebSocket on the specified -port, and send a registration message as a JSON serialized string": `{ event: <registerEvent>, uuid: <pluginUUID> }`. Source: https://docs.elgato.com/streamdeck/sdk/references/websocket/plugin
  - **Verified (local):** the port is 28196.
- **Updating keys from external events.**
  - **Verified:** the commands are `setTitle` ("Sets the title displayed for an instance of an action."), `setImage`, `setState`, `setFeedback` and `setFeedbackLayout` (Stream Deck + layouts), plus `showOk` and `showAlert`. Same source.
  - **Inferred:** a plugin can send these at any time, e.g. when the tray app reports a mute change, because they are plain WebSocket messages keyed by action context.
- **Pedal and Stream Deck +.**
  - **Verified:** `DeviceType 5 = Stream Deck Pedal` ("3 customizable pedals"), `7 = Stream Deck +` ("4 dials with rotation and press, with touch strip"). `Controllers: "Encoder"` "refers to a dial / touchscreen on the Stream Deck +". The events are `dialDown`, `dialRotate` and `touchTap`.
  - **Inferred:** pedals behave as keypad keys (`keyDown`/`keyUp`) with no display, so title/image updates have no visible effect.
- **Local note (Verified).** Elgato's own Marketplace plugins ship with an encrypted `manifest.json` (file starts with `ELGATO` plus binary), while their JS is plaintext.

### 3c. Emulating a Stream Deck as a fake USB HID device

- **Verified.** VHF: "In this release, VHF supports a HID source driver only in kernel mode." (KMDF or WDM). Source: https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/virtual-hid-framework--vhf-
- **Verified.** User mode: "HID minidrivers can be written by using … UMDF … KMDF … WDM", and "UMDF HID transport minidrivers are not supported on versions of Windows prior to Windows 8". Source: https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/transport-minidrivers
- **Verified.** Signing: new kernel drivers must be Dev-Portal signed on Windows 10 1607+ (quote in 2e). An EV certificate is needed for a dashboard account. Source: the signing policy page above.
- **Verified.** Elgato documents the Stream Deck HID protocol (Classic, XL, Neo, +, + XL, Mini) at https://docs.elgato.com/streamdeck/hid/intro. The Pedal is not listed there.
- **Not verified.** Whether the Stream Deck app would accept an emulated device with a spoofed Elgato VID/PID/serial.
- **Inferred.** Not worth it: it needs a signed driver plus undocumented acceptance by Elgato's app, and spoofing Elgato's VID raises its own problems.

---

## 4. Controlling Wave Link from another app

### 4.1 What is installed (Verified, local)

- **App.** `C:\Program Files\Elgato\WaveLink\WaveLink.exe`, FileVersion **2.0.6.3780** (Corsair Memory, Inc., Qt6). It runs alongside `WaveLinkSE.exe`.
- **Listening port.** WaveLink.exe listens on **`0.0.0.0:1824` and `[::]:1824`**, i.e. all interfaces, not just loopback (`netstat -ano`, PID 18944). Firewall rules were not checked.
- **Two Elgato plugins are installed:**
  - `%APPDATA%\Elgato\StreamDeck\Plugins\com.elgato.wavelink.sdPlugin\`: the HTML/JS plugin for Wave Link 1.x/2.x. Its log shows it connected: "AppClient: Connection established, connected to port 1824." (`%APPDATA%\Elgato\StreamDeck\logs\com.elgato.wavelink0.log`)
  - `%APPDATA%\Elgato\StreamDeck\Plugins\com.elgato.wave-link.sdPlugin\`: a Node plugin (`bin\plugin.js`) for **Wave Link 3**, which is not installed. Its log loops on "Failed to read ws-info.json … ENOENT" and "connect ECONNREFUSED 127.0.0.1:1893" (`…\com.elgato.wave-link.sdPlugin\logs\com.elgato.wave-link.0.log`).

### 4.2 Wave Link 2.x API (what this PC uses)

All **Verified** from `com.elgato.wavelink.sdPlugin\plugin\js\` unless marked.

- **Transport.** JSON-RPC 2.0 over WebSocket.
  - `AppClient.js` opens `new WebSocket(\`ws://127.0.0.1:${port}\`)` on 10 ports (`maxPort = minPort + 9`).
  - `WaveLinkClient.js` calls `super(1824)`, so the range is 1824–1833.
  - The envelope comes from `simple-jsonrpc-js.min.js` (`"jsonrpc": "2.0"`).
- **Version check.** `getApplicationInfo` returns `interfaceRevision`. The plugin accepts `'6'` through `'7'` (`WaveLinkConstants.js`).
  **Verified live (read-only call):** `{"appID":"egwl","appName":"Elgato Wave Link","interfaceRevision":"7"}`.
- **Methods.** `getApplicationInfo`, `getMicrophoneConfig`, `setMicrophoneConfig`, `getSwitchState`, `switchOutput`, `getOutputConfig`, `setOutputConfig`, `getOutputs`, `setSelectedOutput`, `addInput`, `getInputConfigs`, `setInputConfig`, `setFilterBypass`, `setFilter`, plus `getForegroundAppGroupIdentifier`.
- **Notifications.** `inputMuteChanged` (identifier, mixerID, value), `outputMuteChanged` (mixerID, value), `inputVolumeChanged`, `outputVolumeChanged`, `inputsChanged`, `microphoneConfigChanged`, `selectedOutputChanged`, `realTimeChanges` (level meters), and others.
- **Mix identifiers.** Monitor mix = `com.elgato.mix.local`, stream mix = `com.elgato.mix.stream`, both = `com.elgato.mix.all`.
- **Mute an input.** `setInputConfig` with params `{property: "Mute", identifier, mixerID, value: <bool>, forceLink}`. `forceLink` is `true` when `mixerID` is `com.elgato.mix.all` (`WaveLinkClient.js` lines 693–799; `InputAction.js:85`).
- **Mute a mix (output).** `setOutputConfig` with `{property: "Output Mute", mixerID, value, forceLink}` (`WaveLinkClient.js:585–590`).
- **Hardware Wave mic mute.** `setMicrophoneConfig` with `{identifier, property: "Microphone Mute", boolValue}`. This only applies to Elgato Wave microphones, so it doesn't fit the Maono.
- **This PC's mic (Verified live).** `getInputConfigs` returns identifier **`"Wave Link Aux 2"`**, name "Microphone (PD400X Podcast Microphone)", `localMixer: [true,100,false,false]`, `streamMixer: [false,100,false]`. Index 0 is mute state (the plugin reads `isMuted: input[kJSONKeyLocalMixer][0]`). So the mic is muted in the monitor mix and live in the stream mix.
- **Example request (Inferred, built from the plugin code; NOT sent):**
  `{"jsonrpc":"2.0","id":1,"method":"setInputConfig","params":{"property":"Mute","identifier":"Wave Link Aux 2","mixerID":"com.elgato.mix.stream","value":true,"forceLink":false}}`
- **Not verified.** The Windows default input is "Wave Link MicrophoneFX". Whether muting the stream mix in Wave Link also silences that endpoint for Discord/games was not tested. The current app mutes the Windows capture endpoint instead (`TrayApp.cs`: AudioSwitcher `ToggleMuteAsync` on the selected `MicDeviceId`).

### 4.3 Wave Link 3 API (what the newer plugin expects)

All **Verified** from `com.elgato.wave-link.sdPlugin\bin\plugin.js` (minified).

- **Install detection.** Windows: `Get-AppxPackage -Name "Elgato.WaveLink*"`. macOS: bundle id `com.elgato.WaveLink3`.
- **Port discovery.** It reads `%LOCALAPPDATA%\Packages\Elgato.WaveLink_g54w8ztgkx496\LocalState\ws-info.json` (`{"port": N}`) and falls back to ports **1884–1893**.
- **Envelope.** JSON-RPC 2.0: `{id, jsonrpc:"2.0", method, params}`.
- **Methods.** `getApplicationInfo`, `setPluginInfo`, `getInputDevices`, `setInputDevice`, `getOutputDevices`, `setOutputDevice`, `getChannels`, `setChannel`, `addToChannel`, `getMixes`, `setMix`, `getSounds`, `triggerSound`, `setSubscription`.
- **Notifications.** `channelChanged`, `mixChanged`, `inputDeviceChanged`, `levelMeterChanged`, `focusedAppChanged`, and others.
- **Mute payloads:**
  - Channel in all mixes: `setChannel({id, isMuted})`.
  - Channel in one mix: `setChannel({id, mixes:[{id: mixId, isMuted}]})`.
  - Mix master: `setMix({id, isMuted})`.
  - Hardware input: `setInputDevice({id, inputs:[{id, isMuted}]})`.

### 4.4 Documented or not?

- **Verified: undocumented.** docs.elgato.com's Wave Link section only has "Audio effects (VST)" and "Presets". It has no control/API reference (https://docs.elgato.com/).
- **Inferred (non-primary).** Community projects reverse-engineered both generations, e.g. https://github.com/DarrellVS/node-wave-link-sdk, https://github.com/Agash/WaveLink.Client and https://gist.github.com/ghostzero/34ddc050bda7850cca4e561d6244f056. Treat the API as unstable across versions.

---

## Summary of what could not be verified

- Noir/OEM gaps:
  - S1 support in Noir Device Hub (hub.noirgear.com).
  - The manual's contents beyond Bluetooth pairing.
  - The MCU part number.
  - What "Game Safe Mode" does.
- Whether F13–F24 can be produced via the configurator's macro editor and replayed by the firmware.
- Which of the two consumer collections (MI_00/MI_01 COL02) carries a button's consumer usage.
- Windows input behavior:
  - Whether RegisterHotKey withholds the triggering key from the focused app.
  - Whether modifier-less or media-key hotkeys suppress Windows' default action.
  - Whether LL-hook blocking also hides input from Raw Input readers.
  - LL hook and Raw Input behavior under UIPI with an elevated game in the foreground.
- Which real games set RIDEV_NOHOTKEYS.
- Whether pedals report as keypad actions with no visual feedback (inferred, not tested on the device).
- Whether the Stream Deck app would accept an emulated HID device.
- Whether muting Wave Link's stream mix silences the "Wave Link MicrophoneFX" endpoint.

## Implications for the rewrite

- **Most robust trigger in games: a unique consumer key from the mouse, read with Raw Input in the background.**
  - In the Noir configurator (stored onboard), assign the button to an unused Multimedia/System entry, e.g. "My Computer" (0x0194) or "Multimedia" (0x0183).
  - Register Raw Input for usage page 0x0C / usage 0x01 with `RIDEV_INPUTSINK`. Accept only devices whose `RIDI_DEVICENAME` contains `VID_A8A5&PID_2255` (receiver), or `VID_A8A4` (wired, inferred).
  - Why it's robust: no Ctrl is emitted, so nothing leaks into the game. `RIDEV_NOHOTKEYS` can't disable it. There is no hook timeout, and per-device filtering works.
  - Caveat: Windows still runs the key's default action (e.g. VK_LAUNCH_APP1 → Explorer; inferred). Swallow only that VK with a tiny `WH_KEYBOARD_LL` hook on a dedicated thread. If Windows silently drops the hook, mute still works through Raw Input.
  - Fallback: keep the button as XBUTTON1/2, detect it via `RI_MOUSE_BUTTON_4/5_*`, and swallow it with `WH_MOUSE_LL`.
  - Test both with an elevated game in front (UIPI unverified).
- **Retire Ctrl+F12 as the default.** Microsoft reserves F12 for the debugger, a foreground game can switch app hotkeys off with `RIDEV_NOHOTKEYS`, and Ctrl is an ordinary keystroke the game sees. Keep `RegisterHotKey` only as an optional keyboard shortcut (prefer a modifier-less key plus `MOD_NOREPEAT`). Per-device *blocking* would need a Dev-Portal-signed kernel filter driver, which is out of scope and unnecessary with a single mouse.
- **Stream Deck: feasible as your own plugin, not as "mouse becomes a Stream Deck".**
  - Ship a plugin (native `.exe` via `CodePathWin`, or Node) with a "Mic Mute" keypad action for the XL, the + keys and the Pedal, plus an optional encoder layout for the SD+.
  - Push state to it with `setState`/`setTitle`/`setImage`. Sync with the tray app over a local channel; the tray app can also poke the plugin with a passive deep link `streamdeck://plugins/message/<uuid>/toggle?streamdeck=hidden` (SD ≥ 7.0).
  - Faking a Stream Deck HID device isn't practical (signed driver, undocumented acceptance).
- **Wave Link: feasible but undocumented and version-specific; make it opt-in.**
  - For Wave Link 2.x: connect to `ws://127.0.0.1:1824` (scan 1824–1833) and require `interfaceRevision` 7 (or 6).
  - Toggle with `setInputConfig` (`property:"Mute"`, `identifier:"Wave Link Aux 2"`, `mixerID` stream/local/all), and drive the overlay from `inputMuteChanged`.
  - Wave Link 3 needs a separate adapter: `ws-info.json` or ports 1884–1893, with `setChannel`/`setMix`.
  - Keep today's Windows endpoint mute as the default. Offer "mute in Wave Link stream mix" only after testing which mute actually silences "Wave Link MicrophoneFX" for Discord and games.
