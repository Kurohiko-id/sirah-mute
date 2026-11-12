use crate::enforcer::Defaults;
use crate::msg::{WM_APP_DEVICES, WM_APP_MUTE, WM_APP_OTHERS};
use crate::policy_config::{CLSID_POLICY_CONFIG, IPolicyConfig};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{HWND, LPARAM, PROPERTYKEY, WPARAM};
use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
    IAudioMeterInformation,
};
use windows::Win32::Media::Audio::{
    AUDIO_VOLUME_NOTIFICATION_DATA, DEVICE_STATE, DEVICE_STATE_ACTIVE, DEVICE_STATE_DISABLED,
    DEVICE_STATE_UNPLUGGED, EDataFlow, ERole, IMMDevice, IMMDeviceEnumerator,
    IMMNotificationClient, IMMNotificationClient_Impl, MMDeviceEnumerator, eCapture,
    eCommunications, eConsole, eMultimedia, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize, STGM_READ,
};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::core::{HSTRING, PCWSTR, Result, implement};

const ROLES: [ERole; 3] = [eConsole, eMultimedia, eCommunications];

pub enum Flow {
    Capture,
    Render,
}

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub present: bool,
}

// HWND is not Send; callbacks run on COM worker threads, so keep it as isize.
fn post(hwnd: isize, msg: u32, wparam: usize) {
    unsafe {
        let _ = PostMessageW(Some(HWND(hwnd as *mut _)), msg, WPARAM(wparam), LPARAM(0));
    }
}

#[implement(IMMNotificationClient)]
struct DeviceNotify {
    hwnd: isize,
}

impl IMMNotificationClient_Impl for DeviceNotify_Impl {
    fn OnDeviceStateChanged(&self, _: &PCWSTR, _: DEVICE_STATE) -> Result<()> {
        post(self.hwnd, WM_APP_DEVICES, 0);
        Ok(())
    }
    fn OnDeviceAdded(&self, _: &PCWSTR) -> Result<()> {
        post(self.hwnd, WM_APP_DEVICES, 0);
        Ok(())
    }
    fn OnDeviceRemoved(&self, _: &PCWSTR) -> Result<()> {
        post(self.hwnd, WM_APP_DEVICES, 0);
        Ok(())
    }
    fn OnDefaultDeviceChanged(&self, _: EDataFlow, _: ERole, _: &PCWSTR) -> Result<()> {
        post(self.hwnd, WM_APP_DEVICES, 0);
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> Result<()> {
        // Fires constantly; posting here would keep the device debounce busy.
        Ok(())
    }
}

#[implement(IAudioEndpointVolumeCallback)]
struct MuteNotify {
    hwnd: isize,
}

impl IAudioEndpointVolumeCallback_Impl for MuteNotify_Impl {
    fn OnNotify(&self, data: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> Result<()> {
        if let Some(d) = unsafe { data.as_ref() } {
            post(self.hwnd, WM_APP_MUTE, d.bMuted.as_bool() as usize);
        }
        Ok(())
    }
}

#[implement(IAudioEndpointVolumeCallback)]
struct OtherNotify {
    hwnd: isize,
}

impl IAudioEndpointVolumeCallback_Impl for OtherNotify_Impl {
    fn OnNotify(&self, _: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> Result<()> {
        post(self.hwnd, WM_APP_OTHERS, 0);
        Ok(())
    }
}

type Watched = (String, IAudioEndpointVolume, IAudioEndpointVolumeCallback);

pub struct Audio {
    enumerator: IMMDeviceEnumerator,
    notify: IMMNotificationClient,
    volume: Option<(IAudioEndpointVolume, IAudioEndpointVolumeCallback)>,
    others: Vec<Watched>,
    hwnd: isize,
}

fn flow_of(flow: Flow) -> EDataFlow {
    match flow {
        Flow::Capture => eCapture,
        Flow::Render => eRender,
    }
}

fn device_id(dev: &IMMDevice) -> Result<String> {
    unsafe {
        let p = dev.GetId()?;
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as _));
        Ok(s)
    }
}

fn device_name(dev: &IMMDevice) -> String {
    unsafe {
        dev.OpenPropertyStore(STGM_READ)
            .and_then(|s| s.GetValue(&PKEY_Device_FriendlyName))
            .map(|v| v.to_string())
            .unwrap_or_default()
    }
}

impl Audio {
    /// Caller must have called `CoInitializeEx(COINIT_APARTMENTTHREADED)`.
    pub fn new(hwnd: HWND) -> Result<Self> {
        let hwnd = hwnd.0 as isize;
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let notify: IMMNotificationClient = DeviceNotify { hwnd }.into();
            enumerator.RegisterEndpointNotificationCallback(&notify)?;
            Ok(Self {
                enumerator,
                notify,
                volume: None,
                others: Vec::new(),
                hwnd,
            })
        }
    }

    pub fn list(&self, flow: Flow) -> Vec<DeviceInfo> {
        let mut out = Vec::new();
        unsafe {
            let Ok(coll) = self.enumerator.EnumAudioEndpoints(
                flow_of(flow),
                DEVICE_STATE(
                    DEVICE_STATE_ACTIVE.0 | DEVICE_STATE_UNPLUGGED.0 | DEVICE_STATE_DISABLED.0,
                ),
            ) else {
                return out;
            };
            for i in 0..coll.GetCount().unwrap_or(0) {
                let Ok(dev) = coll.Item(i) else { continue };
                let Ok(id) = device_id(&dev) else { continue };
                out.push(DeviceInfo {
                    id,
                    name: device_name(&dev),
                    present: dev.GetState().is_ok_and(|s| s == DEVICE_STATE_ACTIVE),
                });
            }
        }
        out
    }

    fn device(&self, id: &str) -> Result<IMMDevice> {
        unsafe { self.enumerator.GetDevice(&HSTRING::from(id)) }
    }

    fn present(&self, id: &str) -> bool {
        self.device(id)
            .and_then(|d| unsafe { d.GetState() })
            .is_ok_and(|s| s == DEVICE_STATE_ACTIVE)
    }

    fn unwatch(&mut self) {
        if let Some((vol, cb)) = self.volume.take() {
            unsafe {
                let _ = vol.UnregisterControlChangeNotify(&cb);
            }
        }
    }

    /// Re-targets the mute callback. Returns the current mute state, or None
    /// if `id` is None or the device is not active.
    pub fn watch_mute(&mut self, id: Option<&str>) -> Option<bool> {
        self.unwatch();
        let dev = self.device(id?).ok()?;
        unsafe {
            if dev.GetState().ok()? != DEVICE_STATE_ACTIVE {
                return None;
            }
            let vol: IAudioEndpointVolume = dev.Activate(CLSCTX_ALL, None).ok()?;
            let cb: IAudioEndpointVolumeCallback = MuteNotify { hwnd: self.hwnd }.into();
            vol.RegisterControlChangeNotify(&cb).ok()?;
            let muted = vol.GetMute().ok().map(|b| b.as_bool());
            self.volume = Some((vol, cb));
            muted
        }
    }

    /// Peak meter of capture device `id`; None if it is absent or inactive.
    pub fn meter(&self, id: &str) -> Option<IAudioMeterInformation> {
        let dev = self.device(id).ok()?;
        unsafe {
            if dev.GetState().ok()? != DEVICE_STATE_ACTIVE {
                return None;
            }
            dev.Activate(CLSCTX_ALL, None).ok()
        }
    }

    /// Flips mute on the watched device; returns the new state.
    pub fn toggle_mute(&self) -> Result<bool> {
        let Some((vol, _)) = &self.volume else {
            return Err(windows::Win32::Foundation::E_POINTER.into());
        };
        unsafe {
            let m = !vol.GetMute()?.as_bool();
            vol.SetMute(m, std::ptr::null())?;
            Ok(m)
        }
    }

    /// Sets mute on the main watched device.
    pub fn set_mute(&self, muted: bool) -> Result<()> {
        let Some((vol, _)) = &self.volume else {
            return Err(windows::Win32::Foundation::E_POINTER.into());
        };
        unsafe { vol.SetMute(muted, std::ptr::null()) }
    }

    fn unwatch_others(&mut self) {
        for (_, vol, cb) in self.others.drain(..) {
            unsafe {
                let _ = vol.UnregisterControlChangeNotify(&cb);
            }
        }
    }

    /// Replaces the watched set of other mics; absent or inactive ids are skipped.
    #[allow(dead_code)] // consumed by the others-pill task
    pub fn watch_others(&mut self, ids: &[String]) {
        self.unwatch_others();
        for id in ids {
            let Ok(dev) = self.device(id) else { continue };
            unsafe {
                if dev.GetState().ok() != Some(DEVICE_STATE_ACTIVE) {
                    continue;
                }
                let Ok(vol) = dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) else {
                    continue;
                };
                let cb: IAudioEndpointVolumeCallback = OtherNotify { hwnd: self.hwnd }.into();
                if vol.RegisterControlChangeNotify(&cb).is_ok() {
                    self.others.push((id.clone(), vol, cb));
                }
            }
        }
    }

    /// Mute state of each watched other mic, in `watch_others` order.
    #[allow(dead_code)] // consumed by the others-pill task
    pub fn others_muted(&self) -> Vec<(String, bool)> {
        self.others
            .iter()
            .filter_map(|(id, vol, _)| {
                let m = unsafe { vol.GetMute() }.ok()?.as_bool();
                Some((id.clone(), m))
            })
            .collect()
    }

    #[allow(dead_code)] // consumed by the others-pill task
    pub fn set_other_mute(&self, id: &str, muted: bool) -> Result<()> {
        let Some((_, vol, _)) = self.others.iter().find(|(i, _, _)| i == id) else {
            return Err(windows::Win32::Foundation::E_INVALIDARG.into());
        };
        unsafe { vol.SetMute(muted, std::ptr::null()) }
    }

    pub fn defaults(&self, input_id: Option<&str>, output_id: Option<&str>) -> Defaults {
        let get = |flow: EDataFlow| -> [Option<String>; 3] {
            ROLES.map(|role| unsafe {
                self.enumerator
                    .GetDefaultAudioEndpoint(flow, role)
                    .and_then(|d| device_id(&d))
                    .ok()
            })
        };
        Defaults {
            capture: get(eCapture),
            render: get(eRender),
            input_present: input_id.is_some_and(|id| self.present(id)),
            output_present: output_id.is_some_and(|id| self.present(id)),
        }
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.unwatch();
        self.unwatch_others();
        unsafe {
            let _ = self
                .enumerator
                .UnregisterEndpointNotificationCallback(&self.notify);
        }
    }
}

/// For worker threads: initializes COM (MTA) itself, sets `id` as default
/// for console, multimedia and communications.
pub fn set_default(id: &str) -> Result<()> {
    unsafe {
        let init = CoInitializeEx(None, COINIT_MULTITHREADED);
        let res = (|| {
            let policy: IPolicyConfig = CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL)?;
            let wid = HSTRING::from(id);
            for role in ROLES {
                policy.SetDefaultEndpoint(PCWSTR(wid.as_ptr()), role).ok()?;
            }
            Ok(())
        })();
        if init.is_ok() {
            CoUninitialize();
        }
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::COINIT_APARTMENTTHREADED;

    #[test]
    #[ignore] // touches real audio devices (read-only)
    fn others_watch_smoke() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let mut a = Audio::new(HWND::default()).unwrap();
        let ids: Vec<String> = a
            .list(Flow::Capture)
            .into_iter()
            .take(2)
            .map(|d| d.id)
            .collect();
        a.watch_others(&ids);
        assert!(a.others_muted().len() <= 2);
    }
}
