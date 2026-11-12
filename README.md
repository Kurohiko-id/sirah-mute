<p align="center"><img src="assets/banner.png" alt="Sirah Mute: a red glow around the screen and a 'Mic mute 0:42' popup while the mic is muted" width="100%"></p>

<h1 align="center">Sirah Mute</h1>

<p align="center">
  <a href="https://kurohiko.id">kurohiko.id</a> ·
  <a href="https://saweria.co/Kurohiko">Donate via Saweria</a> ·
  <a href="https://kurohikotopup.com">Game top-ups at KurohikoTopUp</a>
</p>

<p align="center"><b>English</b> · <a href="README.id.md">Bahasa Indonesia</a></p>

---

**Never talk into a muted mic again.**

I'm Kurohiko. I've had a cough for years, so on stream I hit mute a lot. The problem was the other half: I kept forgetting to unmute, and talked to nobody for minutes. Sirah Mute is the fix. Muting is still one key, but now it's loud about it: a red glow around your screen, a popup that counts how long you've been muted, and a nudge if you start talking while muted.

*Sirah* means red in Minang.

## Features

- **One-key mute** for the mic you pick, from any app or game. Any key or combo works, including F13–F24 and media keys.
- **Red screen border** while muted. Solid or Glow, adjustable thickness, with a short pulse when you mute.
- **Popup with a mute timer**: `Mic mute 0:42`. Pill or Card style, any corner, any size, and it can stay up the whole time you're muted.
- **Talk-while-muted warning**: reads your physical mic's level and pulses the border (optionally beeps) when you speak while muted. Works with setups like Wave Link where you mute a virtual mic.
- **Click the pill to unmute**: the red popup takes clicks (Pill style). It is clickable while visible; turn on "Keep popup while muted" to keep it clickable all the time. The border still lets clicks through to your game.
- **Hold to cough**: off by default. Hold the hotkey and you are muted only while it is held; a short press is the normal toggle. The hold threshold is adjustable.
- **Other mics**: pick extra mics to watch. A `{name} mute` pill shows while one of them is muted, and a click unmutes it.
- **Home Assistant**: `--mute`, `--unmute` and `--status` for HASS Agent (see [Home Assistant](#home-assistant-hass-agent)).
- **Keeps your default devices**: pick your input and output, and Sirah Mute can force them at startup, notify you, or switch back automatically when Windows changes them behind your back.
- **Tray icon shows the state**: grey capsule = live, red capsule = muted, yellow badge = mic missing.
- Choose which monitors show the border and popup, optional mute/unmute beep, start with Windows, update check, English and Bahasa Indonesia.
- **Small**: one exe, about 3.5 MB of RAM, hotkey-to-indicator in a few milliseconds. Network use: the GitHub update check and anonymous usage stats (see [Privacy](#privacy)).

## Install

Download **SirahMute-Setup.exe** (per-user installer, no admin) or the portable zip from [Releases](../../releases/latest).

> **Windows SmartScreen:** releases aren't code-signed yet, so Windows may say "Windows protected your PC". Click **More info → Run anyway**.

### Coming from Kuro Mute & Force Mic?

Sirah Mute is the new version of Kuro Mute. It picks up your old settings automatically on first start and replaces the old app's autostart entry. You can uninstall Kuro Mute afterwards.

## Home Assistant (HASS Agent)

Sirah Mute can be controlled from PowerShell or the command line, so HASS Agent can mute, unmute and report the mic state. The app must be running.

From PowerShell:

```
& "$env:LOCALAPPDATA\Programs\SirahMute\sirahmute.exe" --mute
& "$env:LOCALAPPDATA\Programs\SirahMute\sirahmute.exe" --unmute
& "$env:LOCALAPPDATA\Programs\SirahMute\sirahmute.exe" --status
```

Commands in HASS Agent: use the **Custom** command type. It is supported and recommended, because it is lighter than the PowerShell type.

```
"%LOCALAPPDATA%\Programs\SirahMute\sirahmute.exe" --mute
"%LOCALAPPDATA%\Programs\SirahMute\sirahmute.exe" --unmute
```

These lines only work in the Custom type. They don't work in the PowerShell type.

Sensor (PowerShell sensor):

```
(& "$env:LOCALAPPDATA\Programs\SirahMute\sirahmute.exe" --status | Out-String).Trim()
```

Create the commands and the sensor in the HASS Agent app itself, not in the Satellite Service. The Satellite Service runs as SYSTEM, so it can't reach the Sirah Mute running in your account.

`--status` prints one word and also sets the exit code:

| State | Output | Exit code |
|---|---|---|
| Mic live | `live` | 0 |
| Muted | `muted` | 1 |
| No mic selected or unplugged | `no_mic` | 2 |
| Sirah Mute not running | `not_running` | 3 |

`--mute` and `--unmute` print the resulting state the same way. They set the state, they don't toggle.

Notes: for the portable version, use the path of the folder you unzipped it to. The state in Home Assistant lags by up to the sensor interval you set in HASS Agent.

## Privacy

Hi, I'm Kurohiko, the developer of Sirah Mute. I care a lot about your privacy.

To keep Sirah Mute moving in the right direction, I need a few basic numbers: how many people install it, which features get used, roughly how much people mute, and which Windows version you're on. That tells me which features are worth improving, and which ones hardly anyone uses.

All of this is **fully anonymous**. I don't know who installed Sirah Mute, and I don't know who uses which feature.

**What is sent:**

- A random ID created the first time Sirah Mute opens. It isn't linked to your accounts, your name, or your device.
- Sirah Mute version, Windows version, CPU architecture (x64/arm64), and system language.
- Country, estimated from the connection. IP addresses are not stored.
- Names of the settings that are switched on, such as border Glow/Solid, popup Pill/Card, its corner and whether it stays up while muted, talk-while-muted warning, hold to cough (`cough_hold`), other mics (`other_mics`), beep, default-device guard, start with Windows, and whether you changed the hotkey. Each one is counted at most once a day, so there is no per-click record.
- How much you muted each day, as rough ranges only: how many times (1–5, 6–20, 21–50, more than 50) and for how long in total (under 5 minutes, 5–30 minutes, 30 minutes–2 hours, longer). Not the exact times or lengths of each mute.
- Crash messages when something goes wrong, with your user folder name masked.

**Never sent:** your mic and device names or IDs, which key your hotkey is, monitor names, the contents of your settings, audio, or screenshots.

The data goes to my own server (`dash.kurohiko.id`). It is not sold or shared with anyone. The code that sends it is open in [`kuromute/src/stats.rs`](kuromute/src/stats.rs). If you'd rather send nothing, block `dash.kurohiko.id` in your firewall or hosts file. Sirah Mute keeps working normally.

## Build from source

Needs Rust (stable, MSVC toolchain) on Windows 10/11.

```
cd kuromute
cargo build --release
```

The exe lands in `target/release/sirahmute.exe`. Run it once and it lives in the tray; right-click the tray icon for settings.

## Support

Bugs and ideas: [open an issue](../../issues). If Sirah Mute saved you from a muted rant, you can [buy me a coffee on Saweria](https://saweria.co/Kurohiko) or top up your games at [KurohikoTopUp](https://kurohikotopup.com).

The settings window is built with [Slint](https://slint.dev) under the Slint Royalty-free License.
