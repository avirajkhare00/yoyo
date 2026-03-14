use anyhow::{anyhow, Result};
use std::io::Write;

const REPO: &str = "avirajkhare00/yoyo";
const CACHE_TTL_SECS: u64 = 86_400; // 24 hours

/// Returns the latest release version string (e.g. "0.23.0") if a newer version
/// is available on GitHub, or None if the current version is up to date or the
/// check fails (network error, timeout, parse error — all silently ignored).
///
/// Results are cached for 24 hours in ~/.cache/yoyo/update-check to avoid
/// hammering the GitHub API on every session.
pub fn check_update() -> Option<String> {
    let cached = read_cache();
    if let Some(ref v) = cached {
        return newer_than_current(v).then(|| v.clone());
    }

    let latest = fetch_latest_version()?;
    write_cache(&latest);
    newer_than_current(&latest).then(|| latest)
}

/// Self-update: download the latest binary for this platform, replace the
/// running executable, and codesign on macOS. Returns a human-readable status
/// message.
pub fn self_update() -> Result<String> {
    let latest = fetch_latest_version().ok_or_else(|| {
        anyhow!("Could not fetch latest version from GitHub. Check your network.")
    })?;

    let current = env!("CARGO_PKG_VERSION");
    if !newer_than_current(&latest) {
        return Ok(format!("Already up to date (v{current})."));
    }

    let target = platform_target()
        .ok_or_else(|| anyhow!("Unsupported platform for self-update. Download manually from https://github.com/{REPO}/releases"))?;

    let url = format!("https://github.com/{REPO}/releases/download/v{latest}/yoyo-{target}.tar.gz");
    let exe = std::env::current_exe()
        .map_err(|e| anyhow!("Cannot determine current executable path: {e}"))?;

    // Download to a temp file
    let tmp_archive = exe.with_extension("update.tar.gz");
    let status = std::process::Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "60",
            "-o",
            tmp_archive.to_str().unwrap(),
            &url,
        ])
        .status()
        .map_err(|e| anyhow!("curl not found: {e}"))?;
    if !status.success() {
        return Err(anyhow!("Download failed. URL: {url}"));
    }

    // Extract binary from archive into a temp dir
    let tmp_dir = exe.parent().unwrap().join("yoyo-update-tmp");
    std::fs::create_dir_all(&tmp_dir)?;
    let status = std::process::Command::new("tar")
        .args([
            "-C",
            tmp_dir.to_str().unwrap(),
            "-xzf",
            tmp_archive.to_str().unwrap(),
        ])
        .status()
        .map_err(|e| anyhow!("tar not found: {e}"))?;
    std::fs::remove_file(&tmp_archive).ok();
    if !status.success() {
        return Err(anyhow!("Extraction failed."));
    }

    // Find the extracted binary
    let extracted = tmp_dir.join(format!("yoyo-{target}"));
    if !extracted.exists() {
        // Fallback: try plain "yoyo" name
        let fallback = tmp_dir.join("yoyo");
        if !fallback.exists() {
            std::fs::remove_dir_all(&tmp_dir).ok();
            return Err(anyhow!(
                "Extracted binary not found. Expected: {}",
                extracted.display()
            ));
        }
        std::fs::rename(&fallback, &extracted)?;
    }

    // Replace current binary (atomic on Unix via rename)
    let old_exe = exe.with_extension("old");
    std::fs::rename(&exe, &old_exe).ok();
    std::fs::copy(&extracted, &exe)?;
    std::fs::remove_dir_all(&tmp_dir).ok();
    std::fs::remove_file(&old_exe).ok();

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))?;
    }

    // codesign on macOS
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("codesign")
            .args(["--force", "--deep", "--sign", "-", exe.to_str().unwrap()])
            .status()
            .ok();
    }

    // Invalidate cache so next llm_instructions call reflects the new version
    invalidate_cache();

    Ok(format!(
        "Updated from v{current} to v{latest}. Restart your MCP client to pick up the new binary."
    ))
}

// --- internal helpers ---

fn fetch_latest_version() -> Option<String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let output = std::process::Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "3",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: yoyo-updater",
            &url,
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let body = String::from_utf8(output.stdout).ok()?;
    let json: serde_json::Value = serde_json::from_str(&body).ok()?;
    let tag = json["tag_name"].as_str()?;
    Some(tag.trim_start_matches('v').to_string())
}

fn newer_than_current(latest: &str) -> bool {
    parse_semver(latest)
        .zip(parse_semver(env!("CARGO_PKG_VERSION")))
        .map(|(l, c)| l > c)
        .unwrap_or(false)
}

fn parse_semver(s: &str) -> Option<(u64, u64, u64)> {
    let parts: Vec<&str> = s.trim_start_matches('v').splitn(3, '.').collect();
    if parts.len() < 3 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].split('-').next()?.parse().ok()?,
    ))
}

fn platform_target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        _ => None,
    }
}

fn cache_path() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let dir = std::path::PathBuf::from(home).join(".cache").join("yoyo");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("update-check"))
}

fn read_cache() -> Option<String> {
    let path = cache_path()?;
    let content = std::fs::read_to_string(&path).ok()?;
    let (ts_str, version) = content.split_once(':')?;
    let ts: u64 = ts_str.parse().ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    if now.saturating_sub(ts) > CACHE_TTL_SECS {
        return None;
    }
    Some(version.trim().to_string())
}

fn write_cache(version: &str) {
    if let Some(path) = cache_path() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let _ = std::fs::File::create(&path)
            .and_then(|mut f| write!(f, "{now}:{version}").map_err(Into::into));
    }
}

fn invalidate_cache() {
    if let Some(path) = cache_path() {
        std::fs::remove_file(path).ok();
    }
}
