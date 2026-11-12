//! Minimal blocking HTTPS over WinHTTP (update check, usage stats).

use std::ffi::c_void;
use windows::Win32::Networking::WinHttp::*;
use windows::core::PCWSTR;

struct Handle(*mut c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Sends one HTTPS request to `host:443`. Returns (status code, body up to 1 MB).
/// `headers` are `\r\n`-separated; `timeout_ms` applies to every WinHTTP phase.
pub fn request(
    verb: &str,
    host: &str,
    path: &str,
    headers: &str,
    body: Option<&[u8]>,
    timeout_ms: i32,
) -> Result<(u32, Vec<u8>), String> {
    const MAX: usize = 1 << 20;
    let err = |what: &str| format!("{what}: {}", windows::core::Error::from_thread());
    unsafe {
        let agent = w("SirahMute");
        let (host, path, verb) = (w(host), w(path), w(verb));
        let hdr: Vec<u16> = headers.encode_utf16().collect();

        let session = Handle(WinHttpOpen(
            PCWSTR(agent.as_ptr()),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ));
        if session.0.is_null() {
            return Err(err("WinHttpOpen"));
        }
        WinHttpSetTimeouts(session.0, timeout_ms, timeout_ms, timeout_ms, timeout_ms)
            .map_err(|_| err("timeouts"))?;
        let conn = Handle(WinHttpConnect(session.0, PCWSTR(host.as_ptr()), 443, 0));
        if conn.0.is_null() {
            return Err(err("WinHttpConnect"));
        }
        let req = Handle(WinHttpOpenRequest(
            conn.0,
            PCWSTR(verb.as_ptr()),
            PCWSTR(path.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ));
        if req.0.is_null() {
            return Err(err("WinHttpOpenRequest"));
        }
        let (ptr, len) = match body {
            Some(b) => (Some(b.as_ptr() as *const c_void), b.len() as u32),
            None => (None, 0),
        };
        WinHttpSendRequest(req.0, Some(&hdr), ptr, len, len, 0).map_err(|_| err("send"))?;
        WinHttpReceiveResponse(req.0, std::ptr::null_mut()).map_err(|_| err("receive"))?;

        let mut code = 0u32;
        let mut n = size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut code as *mut u32 as *mut c_void),
            &mut n,
            std::ptr::null_mut(),
        )
        .map_err(|_| err("status"))?;

        let mut out = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let mut n = 0u32;
            WinHttpReadData(
                req.0,
                buf.as_mut_ptr() as *mut c_void,
                buf.len() as u32,
                &mut n,
            )
            .map_err(|_| err("read"))?;
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n as usize]);
            if out.len() > MAX {
                return Err("response too large".into());
            }
        }
        Ok((code, out))
    }
}
