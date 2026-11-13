//! Latest-release check: parse GitHub's `releases/latest` payload and compare its tag
//! against the running version. The HTTP call lives in `src-tauri`.

use serde::Deserialize;

pub const LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/sp-yduck/overlaysubs/releases/latest";

/// Semantic version taken from a release tag; pre-release and build metadata are ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    /// Parse `0.1.0`, `v0.2.0` or `v0.2.0-beta.1`; requires exactly three numeric parts.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let s = s.strip_prefix(['v', 'V']).unwrap_or(s);
        // Drop a pre-release or build suffix: `-beta.1`, `+20261013`.
        let core = s.split(['-', '+']).next()?;
        let mut parts = core.split('.');
        let major = parse_part(parts.next()?)?;
        let minor = parse_part(parts.next()?)?;
        let patch = parse_part(parts.next()?)?;
        if parts.next().is_some() {
            return None;
        }
        Some(Version {
            major,
            minor,
            patch,
        })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

fn parse_part(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// A published release that is newer than the running build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub tag_name: String,
    pub version: Version,
    pub url: String,
    pub published_at: Option<String>,
    /// Downloadable files that carry a SHA-256, so a download can be verified before it is run.
    pub assets: Vec<Asset>,
}

/// A release file with the checksum GitHub computed for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// Lowercase hex.
    pub sha256: String,
}

/// Installers are only ever fetched from this repository's releases.
const ASSET_URL_PREFIX: &str = "https://github.com/sp-yduck/overlaysubs/releases/download/";

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}

impl GitHubAsset {
    /// None when the file cannot be verified or does not come from this repository's releases.
    fn verified(self) -> Option<Asset> {
        let sha256 = self.digest?.strip_prefix("sha256:")?.to_string();
        let hex = sha256.len() == 64 && sha256.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        (hex && self.browser_download_url.starts_with(ASSET_URL_PREFIX)).then_some(Asset {
            name: self.name,
            url: self.browser_download_url,
            size: self.size,
            sha256,
        })
    }
}

/// Subset of GitHub's release object; unknown fields are ignored.
#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    published_at: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

/// Return the newest release iff it is strictly newer than `current`.
///
/// `Ok(None)` means "nothing to show": current version unparsable, release is a draft or
/// prerelease, or the release is not newer. `Err` means the payload is not usable.
pub fn newer_release(current: &str, json: &str) -> Result<Option<Release>, String> {
    let raw: GitHubRelease =
        serde_json::from_str(json).map_err(|e| format!("invalid release JSON: {e}"))?;
    // Never offer drafts or prereleases as updates.
    if raw.draft || raw.prerelease {
        return Ok(None);
    }
    let version = Version::parse(&raw.tag_name)
        .ok_or_else(|| format!("unparsable release tag {:?}", raw.tag_name))?;
    // An unknown running version must not trigger a notification.
    let Some(current) = Version::parse(current) else {
        return Ok(None);
    };
    if version <= current {
        return Ok(None);
    }
    if !raw.html_url.starts_with("https://") {
        return Err(format!("release url is not https: {:?}", raw.html_url));
    }
    Ok(Some(Release {
        tag_name: raw.tag_name,
        version,
        url: raw.html_url,
        published_at: raw.published_at,
        assets: raw
            .assets
            .into_iter()
            .filter_map(GitHubAsset::verified)
            .collect(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed version of a real `releases/latest` body, including fields we ignore.
    const RELEASE_V020: &str = r#"{
        "url": "https://api.github.com/repos/sp-yduck/overlaysubs/releases/123",
        "html_url": "https://github.com/sp-yduck/overlaysubs/releases/tag/v0.2.0",
        "id": 123,
        "tag_name": "v0.2.0",
        "target_commitish": "main",
        "name": "v0.2.0",
        "draft": false,
        "prerelease": false,
        "published_at": "2026-10-01T12:34:56Z",
        "assets": []
    }"#;

    fn release(tag: &str) -> String {
        format!(
            r#"{{"tag_name": "{tag}", "html_url": "https://github.com/sp-yduck/overlaysubs/releases/tag/{tag}"}}"#
        )
    }

    #[test]
    fn parses_versions_with_optional_v_and_suffix() {
        let plain = Version {
            major: 0,
            minor: 1,
            patch: 0,
        };
        assert_eq!(Version::parse("0.1.0"), Some(plain));
        assert_eq!(
            Version::parse("v0.2.0"),
            Some(Version {
                major: 0,
                minor: 2,
                patch: 0
            })
        );
        assert_eq!(
            Version::parse("V1.2.3"),
            Some(Version {
                major: 1,
                minor: 2,
                patch: 3
            })
        );
        assert_eq!(
            Version::parse("  v0.2.0  "),
            Some(Version {
                major: 0,
                minor: 2,
                patch: 0
            })
        );
        // Pre-release and build metadata are ignored.
        assert_eq!(
            Version::parse("v0.2.0-beta.1"),
            Some(Version {
                major: 0,
                minor: 2,
                patch: 0
            })
        );
        assert_eq!(
            Version::parse("1.2.3+build.7"),
            Some(Version {
                major: 1,
                minor: 2,
                patch: 3
            })
        );
        assert_eq!(
            Version::parse("v1.2.3-rc.1+build.7"),
            Some(Version {
                major: 1,
                minor: 2,
                patch: 3
            })
        );
    }

    #[test]
    fn rejects_malformed_versions() {
        for bad in [
            "",
            "v",
            "1",
            "1.2",
            "nightly",
            "v1.2.3.4",
            "1.2.x",
            "1..3",
            "1.2.3 beta",
        ] {
            assert_eq!(Version::parse(bad), None, "{bad:?} should be rejected");
        }
    }

    #[test]
    fn orders_versions_numerically_not_lexicographically() {
        assert!(Version::parse("1.10.0") > Version::parse("1.9.9"));
        assert!(Version::parse("0.1.10") > Version::parse("0.1.9"));
        assert_eq!(
            newer_release("1.9.9", &release("v1.10.0"))
                .unwrap()
                .map(|r| r.version),
            Some(Version {
                major: 1,
                minor: 10,
                patch: 0
            })
        );
    }

    #[test]
    fn display_drops_the_v_prefix() {
        assert_eq!(Version::parse("v1.2.3").unwrap().to_string(), "1.2.3");
        assert_eq!(
            Version::parse("v1.2.3-beta.1").unwrap().to_string(),
            "1.2.3"
        );
    }

    #[test]
    fn newer_release_maps_a_published_payload() {
        let r = newer_release("0.1.0", RELEASE_V020)
            .unwrap()
            .expect("v0.2.0 is newer");
        assert_eq!(r.tag_name, "v0.2.0");
        assert_eq!(
            r.version,
            Version {
                major: 0,
                minor: 2,
                patch: 0
            }
        );
        assert_eq!(
            r.url,
            "https://github.com/sp-yduck/overlaysubs/releases/tag/v0.2.0"
        );
        assert_eq!(r.published_at.as_deref(), Some("2026-10-01T12:34:56Z"));
    }

    #[test]
    fn only_verifiable_assets_from_this_repository_are_offered() {
        let sha = "a".repeat(64);
        let dl = "https://github.com/sp-yduck/overlaysubs/releases/download/v0.2.0";
        let body = format!(
            r#"{{"tag_name": "v0.2.0", "html_url": "https://github.com/sp-yduck/overlaysubs/releases/tag/v0.2.0", "assets": [
                {{"name": "ok.dmg", "browser_download_url": "{dl}/ok.dmg", "size": 7, "digest": "sha256:{sha}"}},
                {{"name": "no-digest.exe", "browser_download_url": "{dl}/no-digest.exe", "size": 7, "digest": null}},
                {{"name": "md5.exe", "browser_download_url": "{dl}/md5.exe", "size": 7, "digest": "md5:{sha}"}},
                {{"name": "short.exe", "browser_download_url": "{dl}/short.exe", "size": 7, "digest": "sha256:abc"}},
                {{"name": "elsewhere.exe", "browser_download_url": "https://example.com/elsewhere.exe", "size": 7, "digest": "sha256:{sha}"}}
            ]}}"#
        );
        let r = newer_release("0.1.0", &body).unwrap().unwrap();
        assert_eq!(
            r.assets,
            vec![Asset {
                name: "ok.dmg".into(),
                url: format!("{dl}/ok.dmg"),
                size: 7,
                sha256: sha
            }]
        );
    }

    #[test]
    fn newer_release_is_none_for_equal_or_older() {
        assert_eq!(newer_release("0.2.0", &release("v0.2.0")).unwrap(), None);
        assert_eq!(newer_release("0.2.0", &release("v0.1.9")).unwrap(), None);
        assert_eq!(newer_release("1.0.0", &release("v0.9.9")).unwrap(), None);
    }

    #[test]
    fn newer_release_is_none_for_drafts_and_prereleases() {
        let draft = r#"{"tag_name": "v9.9.9", "html_url": "https://example.com", "draft": true}"#;
        let prerelease =
            r#"{"tag_name": "v9.9.9", "html_url": "https://example.com", "prerelease": true}"#;
        assert_eq!(newer_release("0.1.0", draft).unwrap(), None);
        assert_eq!(newer_release("0.1.0", prerelease).unwrap(), None);
    }

    #[test]
    fn newer_release_is_none_when_current_is_unparsable() {
        assert_eq!(newer_release("dev", &release("v0.2.0")).unwrap(), None);
        assert_eq!(newer_release("", &release("v0.2.0")).unwrap(), None);
        assert_eq!(newer_release("1.2", &release("v0.2.0")).unwrap(), None);
    }

    #[test]
    fn newer_release_errors_on_unusable_payloads() {
        assert!(newer_release("0.1.0", "not json").is_err());
        assert!(newer_release("0.1.0", "{}").is_err());
        // Required fields are missing.
        assert!(newer_release("0.1.0", r#"{"html_url": "https://example.com"}"#).is_err());
        assert!(newer_release("0.1.0", r#"{"tag_name": "v0.2.0"}"#).is_err());
        // The tag has to be a version.
        assert!(newer_release("0.1.0", &release("nightly")).is_err());
        // Only https links are offered to the UI.
        for url in ["http://example.com/r", "javascript:alert(1)", ""] {
            let body = format!(r#"{{"tag_name": "v9.9.9", "html_url": "{url}"}}"#);
            assert!(
                newer_release("0.1.0", &body).is_err(),
                "{url:?} should be rejected"
            );
        }
    }
}
