use std::ffi::c_void;
use windows::Win32::Networking::WinHttp::*;
use windows::core::PCWSTR;

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub tag: String,
    pub url: String,
}

fn parts(v: &str) -> Vec<u64> {
    v.trim_start_matches(['v', 'V'])
        .split('.')
        .map(|p| p.parse().unwrap_or(0))
        .collect()
}

pub fn is_newer(tag: &str, current: &str) -> bool {
    let (mut a, mut b) = (parts(tag), parts(current));
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}

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

fn http_get() -> Result<Vec<u8>, String> {
    const MAX: usize = 1 << 20;
    let err = |what: &str| format!("{what}: {}", windows::core::Error::from_thread());
    unsafe {
        let agent = w("SirahMute");
        let host = w("api.github.com");
        let path = w("/repos/Kurohiko-id/sirah-mute/releases/latest");
        let verb = w("GET");
        let hdr: Vec<u16> = "User-Agent: SirahMute\r\nAccept: application/vnd.github+json"
            .encode_utf16()
            .collect();

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
        WinHttpSetTimeouts(session.0, 10000, 10000, 10000, 10000).map_err(|_| err("timeouts"))?;
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
        WinHttpSendRequest(req.0, Some(&hdr), None, 0, 0, 0).map_err(|_| err("send"))?;
        WinHttpReceiveResponse(req.0, std::ptr::null_mut()).map_err(|_| err("receive"))?;

        let mut code = 0u32;
        let mut len = size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut code as *mut u32 as *mut c_void),
            &mut len,
            std::ptr::null_mut(),
        )
        .map_err(|_| err("status"))?;
        if code != 200 {
            return Err(format!("HTTP {code}"));
        }

        let mut body = Vec::new();
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
            body.extend_from_slice(&buf[..n as usize]);
            if body.len() > MAX {
                return Err("response too large".into());
            }
        }
        Ok(body)
    }
}

pub fn fetch_latest() -> Result<Release, String> {
    let body = http_get()?;
    let v: serde_json::Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    let get = |k: &str| {
        v[k].as_str()
            .map(String::from)
            .ok_or_else(|| format!("missing {k}"))
    };
    Ok(Release {
        tag: get("tag_name")?,
        url: get("html_url")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(is_newer("v2.0.1", "2.0.0"));
        assert!(!is_newer("v2.0.0", "2.0.0"));
        assert!(is_newer("2.1", "2.0.9"));
        assert!(!is_newer("v1.2.0", "2.0.0"));
        assert!(!is_newer("nightly", "2.0.0"));
    }

    #[test]
    #[ignore]
    fn fetch_works() {
        println!("{:?}", fetch_latest());
    }
}
