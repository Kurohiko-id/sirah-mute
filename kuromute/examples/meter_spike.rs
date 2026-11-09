//! Throwaway spike: is mic level still readable while the endpoint is muted?
//! Usage: cargo run --example meter_spike            (lists capture devices)
//!        cargo run --example meter_spike -- <index> (talk nonstop for ~24 s)
//!        cargo run --example meter_spike -- <watch> <mute>
//!            mutes <mute> (e.g. a virtual mic), reads only the meter of <watch>
//! Toggles endpoint mute every 4 s and prints meter peak vs captured-sample peak.
//! Ctrl+C mid-run may leave the mic muted; mute state is restored on normal exit.
use std::{thread::sleep, time::Duration};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation};
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;

fn main() -> windows::core::Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let coll = en.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)?;
        let arg = std::env::args().nth(1).and_then(|s| s.parse::<u32>().ok());
        let Some(idx) = arg else {
            for i in 0..coll.GetCount()? {
                let d = coll.Item(i)?;
                let name = d.OpenPropertyStore(STGM_READ)?.GetValue(&PKEY_Device_FriendlyName)?;
                println!("{i}: {name}");
            }
            return Ok(());
        };
        let dev = coll.Item(idx)?;
        let mute_idx = std::env::args().nth(2).and_then(|s| s.parse::<u32>().ok());
        let vol: IAudioEndpointVolume =
            coll.Item(mute_idx.unwrap_or(idx))?.Activate(CLSCTX_ALL, None)?;
        let meter: IAudioMeterInformation = dev.Activate(CLSCTX_ALL, None)?;
        let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
        let fmt = client.GetMixFormat()?;
        let (ch, bits) = ((*fmt).nChannels as usize, (*fmt).wBitsPerSample);
        println!("format: {ch} ch, {bits} bit (32 = float assumed)");
        client.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 1_000_000, 0, fmt, None)?;
        let cap: IAudioCaptureClient = client.GetService()?;
        if mute_idx.is_none() {
            client.Start()?; // meter-only mode: rely on Wave Link's own stream
        }

        let original = vol.GetMute()?.as_bool();
        for tick in 0..96 {
            let muted = (tick / 16) % 2 == 1; // 16 ticks * 250 ms = 4 s per phase
            if tick % 16 == 0 {
                vol.SetMute(muted, std::ptr::null())?;
                println!("--- {} ---", if muted { "MUTED" } else { "UNMUTED" });
            }
            sleep(Duration::from_millis(250));
            let (mut peak, mut silent) = (0f32, false);
            while mute_idx.is_none() && cap.GetNextPacketSize()? > 0 {
                let (mut data, mut n, mut flags) = (std::ptr::null_mut(), 0u32, 0u32);
                cap.GetBuffer(&mut data, &mut n, &mut flags, None, None)?;
                silent |= flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
                let len = n as usize * ch;
                if bits == 32 {
                    for s in std::slice::from_raw_parts(data as *const f32, len) {
                        peak = peak.max(s.abs());
                    }
                } else if bits == 16 {
                    for s in std::slice::from_raw_parts(data as *const i16, len) {
                        peak = peak.max(s.unsigned_abs() as f32 / 32768.0);
                    }
                }
                cap.ReleaseBuffer(n)?;
            }
            println!(
                "muted={} meter={:.3} samples={:.3} silentflag={}",
                vol.GetMute()?.as_bool() as u8,
                meter.GetPeakValue()?,
                peak,
                silent as u8
            );
        }
        vol.SetMute(original, std::ptr::null())?;
        let _ = client.Stop();
        CoTaskMemFree(Some(fmt as _));
    }
    Ok(())
}
