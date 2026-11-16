#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub tag: String,
    pub url: String,
    /// Short "What's new" lines from the release notes, for the settings window.
    pub highlights: Vec<String>,
    /// The setup exe of this release, when it has one with a checksum.
    pub installer: Option<Asset>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Asset {
    pub url: String,
    /// Lowercase hex, from the GitHub API `digest` field.
    pub sha256: String,
    /// Bytes, for the download progress.
    pub size: u64,
}

const DOWNLOAD_PREFIX: &str = "https://github.com/Kurohiko-id/sirah-mute/releases/download/";

/// The `SirahMute-Setup-*.exe` asset of a GitHub release JSON; only our repo's download URLs.
fn installer_asset(release: &serde_json::Value) -> Option<Asset> {
    release["assets"].as_array()?.iter().find_map(|a| {
        let name = a["name"].as_str()?;
        let url = a["browser_download_url"].as_str()?;
        let sha = a["digest"].as_str()?.strip_prefix("sha256:")?;
        let ok = name.starts_with("SirahMute-Setup-")
            && name.ends_with(".exe")
            && url.starts_with(DOWNLOAD_PREFIX)
            && sha.len() == 64;
        ok.then(|| Asset {
            url: url.into(),
            sha256: sha.to_lowercase(),
            size: a["size"].as_u64().unwrap_or(0),
        })
    })
}

fn sha256_hex(data: &[u8]) -> Result<String, String> {
    use windows::Win32::Security::Cryptography::{BCRYPT_SHA256_ALG_HANDLE, BCryptHash};
    let mut out = [0u8; 32];
    unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut out) }
        .ok()
        .map_err(|e| e.to_string())?;
    Ok(out.iter().map(|b| format!("{b:02x}")).collect())
}

/// Downloads the setup exe to %TEMP% and checks its SHA-256 before anything runs it.
/// `progress` gets the bytes received so far.
pub fn download_installer(
    a: &Asset,
    progress: &mut dyn FnMut(usize),
) -> Result<std::path::PathBuf, String> {
    let path = a
        .url
        .strip_prefix("https://github.com")
        .filter(|_| a.url.starts_with(DOWNLOAD_PREFIX))
        .ok_or("unexpected download url")?;
    let (code, body) = crate::http::request_max(
        "GET",
        "github.com",
        path,
        "User-Agent: SirahMute",
        None,
        30000,
        64 << 20,
        progress,
    )?;
    if code != 200 {
        return Err(format!("HTTP {code}"));
    }
    if sha256_hex(&body)? != a.sha256 {
        return Err("checksum mismatch".into());
    }
    let out = std::env::temp_dir().join("SirahMute-Setup-update.exe");
    std::fs::write(&out, &body).map_err(|e| e.to_string())?;
    Ok(out)
}

const MAX_HIGHLIGHTS: usize = 4;

/// Bullets under the release notes' "What's new" heading, each cut to its bold lead
/// ("- **Hold to cough**: ..." -> "Hold to cough"). Empty when there is no such heading.
pub fn highlights(body: &str) -> Vec<String> {
    let mut in_new = false;
    let mut out = Vec::new();
    for line in body.lines().map(str::trim) {
        if line.starts_with('#') {
            in_new = line.to_lowercase().contains("what's new");
            continue;
        }
        let Some(item) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) else {
            continue;
        };
        if in_new {
            out.push(headline(item));
            if out.len() == MAX_HIGHLIGHTS {
                break;
            }
        }
    }
    out
}

fn headline(item: &str) -> String {
    if let Some(rest) = item.strip_prefix("**")
        && let Some(end) = rest.find("**")
    {
        return rest[..end].trim().trim_end_matches(['.', ':']).to_string();
    }
    let plain = item.replace("**", "").replace('`', "");
    match plain.char_indices().nth(80) {
        Some((i, _)) => format!("{}...", plain[..i].trim_end()),
        None => plain,
    }
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
        highlights: highlights(v["body"].as_str().unwrap_or_default()),
        installer: installer_asset(&v),
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
    fn highlights_take_bold_leads_of_whats_new_only() {
        let body = "### What's new\n\
            - **Click the mute pill to unmute.** While the mic is on...\n\
            - **Hold to cough**: hold your hotkey...\n\
            - Plain item with `code` and **bold** inside\n\
            \n\
            ### Fixes\n\
            - Tray fix that must not show\n";
        assert_eq!(
            highlights(body),
            [
                "Click the mute pill to unmute",
                "Hold to cough",
                "Plain item with code and bold inside"
            ]
        );
        assert!(highlights("### Fixes\n- only fixes").is_empty());
        let many = "### What's new\n- **a**\n- **b**\n- **c**\n- **d**\n- **e**";
        assert_eq!(highlights(many).len(), MAX_HIGHLIGHTS);
        let long = format!("### What's new\n- {}", "é".repeat(100));
        assert!(highlights(&long)[0].ends_with("..."));
    }

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            sha256_hex(b"abc").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn installer_asset_only_our_setup_with_digest() {
        let sha = "a".repeat(64);
        let v = serde_json::json!({ "assets": [
            { "name": "SirahMute-2.1.0-portable.zip", "digest": format!("sha256:{sha}"),
              "browser_download_url": format!("{DOWNLOAD_PREFIX}v2.1.0/SirahMute-2.1.0-portable.zip") },
            { "name": "SirahMute-Setup-2.1.0.exe", "digest": format!("sha256:{}", sha.to_uppercase()),
              "browser_download_url": format!("{DOWNLOAD_PREFIX}v2.1.0/SirahMute-Setup-2.1.0.exe") },
        ]});
        let a = installer_asset(&v).unwrap();
        assert!(a.url.ends_with("SirahMute-Setup-2.1.0.exe"));
        assert_eq!(a.sha256, sha);
        let other_host = serde_json::json!({ "assets": [
            { "name": "SirahMute-Setup-2.1.0.exe", "digest": format!("sha256:{sha}"),
              "browser_download_url": "https://evil.example/SirahMute-Setup-2.1.0.exe" }
        ]});
        assert_eq!(installer_asset(&other_host), None);
        let no_digest = serde_json::json!({ "assets": [
            { "name": "SirahMute-Setup-2.1.0.exe",
              "browser_download_url": format!("{DOWNLOAD_PREFIX}v2.1.0/SirahMute-Setup-2.1.0.exe") }
        ]});
        assert_eq!(installer_asset(&no_digest), None);
    }

    #[test]
    #[ignore]
    fn download_latest_installer() {
        let rel = fetch_latest().unwrap();
        let mut last = 0;
        let p = download_installer(&rel.installer.unwrap(), &mut |n| last = n).unwrap();
        assert!(last > 0);
        println!(
            "{} {} bytes",
            p.display(),
            std::fs::metadata(&p).unwrap().len()
        );
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    #[ignore]
    fn fetch_works() {
        println!("{:?}", fetch_latest());
    }
}
