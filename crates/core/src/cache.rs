//! Persisted result of the hardware probe ("Check this PC"), so later launches skip the probe.
//!
//! Detection shells out to nvidia-smi / PowerShell and takes a moment; the result only changes
//! when the hardware does, so the app keeps the last one on disk and refreshes on demand.

use crate::tier::Hardware;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Bump when `Hardware`'s serialized shape changes, so caches written by older builds are ignored.
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareCache {
    pub version: u32,
    /// Unix seconds. Informational: the cache does not expire, "Check again" forces a new probe.
    pub detected_at: u64,
    pub hardware: Hardware,
}

impl HardwareCache {
    pub fn new(hardware: Hardware) -> Self {
        Self { version: VERSION, detected_at: now(), hardware }
    }

    /// `None` for a broken cache or one written by a different [`VERSION`]; the caller then re-probes.
    pub fn parse(json: &str) -> Option<Self> {
        let cache: Self = serde_json::from_str(json).ok()?;
        (cache.version == VERSION).then_some(cache)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("hardware cache is serializable")
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tier::Gpu;

    fn hardware() -> Hardware {
        Hardware {
            gpu: Some(Gpu { name: "GPU".into(), vram_mb: 8192, unified: false }),
            ram_mb: 32768,
            cpu_threads: 16,
            cpu_name: "CPU".into(),
        }
    }

    #[test]
    fn round_trips() {
        let cache = HardwareCache::new(hardware());
        let parsed = HardwareCache::parse(&cache.to_json()).unwrap();
        assert_eq!(parsed.version, VERSION);
        assert_eq!(parsed.detected_at, cache.detected_at);
        assert_eq!(parsed.hardware.gpu.unwrap().vram_mb, 8192);
        assert_eq!(parsed.hardware.cpu_threads, 16);
    }

    #[test]
    fn round_trips_unified_and_missing_gpus() {
        let unified = Hardware { gpu: Some(Gpu { name: "Apple Silicon".into(), vram_mb: 16384, unified: true }), ..hardware() };
        assert!(HardwareCache::parse(&HardwareCache::new(unified).to_json()).unwrap().hardware.gpu.unwrap().unified);
        let cpu_only = Hardware { gpu: None, ..hardware() };
        assert!(HardwareCache::parse(&HardwareCache::new(cpu_only).to_json()).unwrap().hardware.gpu.is_none());
    }

    #[test]
    fn ignores_broken_or_outdated_caches() {
        assert!(HardwareCache::parse("not json").is_none());
        // A cache written by another build must not be reused.
        for version in [VERSION.saturating_sub(1), VERSION + 1] {
            let outdated = HardwareCache { version, ..HardwareCache::new(hardware()) };
            assert!(HardwareCache::parse(&outdated.to_json()).is_none(), "version {version} must be rejected");
        }
        let mut json = serde_json::to_value(HardwareCache::new(hardware())).unwrap();
        json.as_object_mut().unwrap().remove("detected_at");
        assert!(HardwareCache::parse(&json.to_string()).is_none());
    }
}
