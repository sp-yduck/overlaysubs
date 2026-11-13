//! Startup check for a newer GitHub release (one anonymous GET, short timeouts, no telemetry) and the installer it offers.
use overlaysubs_core::update::{newer_release, Asset, LATEST_RELEASE_API_URL};
use serde::Serialize;
use std::path::Path;
use std::time::Duration;

/// A release newer than the running build, in the shape the UI consumes (snake_case fields).
#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub tag: String,
    pub url: String,
    pub published_at: Option<String>,
    /// The release has an installer for this platform that can be downloaded and verified.
    pub downloadable: bool,
    #[serde(skip)]
    pub installer: Option<Asset>,
}

impl UpdateInfo {
    /// File name of the downloaded installer; versioned so a newer release is not taken for an older download.
    pub fn installer_file(&self) -> Option<String> {
        self.installer.as_ref().map(|a| format!("{}-{}", self.latest, a.name))
    }
}

/// The release asset that updates this install, by its fixed name (see RELEASING.md). None on unsupported platforms.
fn installer_name() -> Option<&'static str> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return Some("overlaysubs-macos-arm64.dmg");
    }
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        // The NSIS installer is per-user, under %LOCALAPPDATA%; anywhere else was installed by the per-machine MSI.
        let per_user = match (std::env::current_exe(), std::env::var_os("LOCALAPPDATA")) {
            (Ok(exe), Some(local)) => exe.starts_with(local),
            _ => false,
        };
        return Some(if per_user { "overlaysubs-windows-x64-setup.exe" } else { "overlaysubs-windows-x64.msi" });
    }
    None
}

/// Starts the downloaded installer: the setup program, the MSI through msiexec, or the disk image in Finder.
pub fn launch(installer: &Path) -> Result<(), String> {
    let mut cmd = match installer.extension().and_then(|e| e.to_str()) {
        Some("exe") => std::process::Command::new(installer),
        Some("msi") => {
            let mut c = std::process::Command::new("msiexec");
            c.arg("/i").arg(installer);
            c
        }
        _ => {
            let mut c = std::process::Command::new("open");
            c.arg(installer);
            c
        }
    };
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Blocking: run from `spawn_blocking` (see the `check_update` command).
///
/// `Ok(None)` means up to date, and also covers prereleases or an unparsable running version.
pub fn fetch(current: &str) -> Result<Option<UpdateInfo>, String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(format!(
            "OverlaySubs/{current} (+https://github.com/sp-yduck/overlaysubs)"
        ))
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let res = client
        .get(LATEST_RELEASE_API_URL)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .map_err(|e| e.to_string())?;
    let status = res.status();
    if !status.is_success() {
        return Err(format!("GitHub API returned {status}"));
    }
    let body = res.text().map_err(|e| e.to_string())?;
    Ok(newer_release(current, &body)?.map(|r| {
        let installer = installer_name().and_then(|n| r.assets.into_iter().find(|a| a.name == n));
        UpdateInfo {
            current: current.to_string(),
            latest: r.version.to_string(),
            tag: r.tag_name,
            url: r.url,
            published_at: r.published_at,
            downloadable: installer.is_some(),
            installer,
        }
    }))
}
