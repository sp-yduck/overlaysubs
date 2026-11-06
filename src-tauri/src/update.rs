//! Startup check for a newer GitHub release. One anonymous GET, short timeouts, no telemetry.
use overlaysubs_core::update::{newer_release, LATEST_RELEASE_API_URL};
use serde::Serialize;
use std::time::Duration;

/// A release newer than the running build, in the shape the UI consumes (snake_case fields).
#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub tag: String,
    pub url: String,
    pub published_at: Option<String>,
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
    Ok(newer_release(current, &body)?.map(|r| UpdateInfo {
        current: current.to_string(),
        latest: r.version.to_string(),
        tag: r.tag_name,
        url: r.url,
        published_at: r.published_at,
    }))
}
