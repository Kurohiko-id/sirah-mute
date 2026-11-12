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

fn http_get() -> Result<Vec<u8>, String> {
    let (code, body) = crate::http::request(
        "GET",
        "api.github.com",
        "/repos/Kurohiko-id/sirah-mute/releases/latest",
        "User-Agent: SirahMute\r\nAccept: application/vnd.github+json",
        None,
        10000,
    )?;
    if code != 200 {
        return Err(format!("HTTP {code}"));
    }
    Ok(body)
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
