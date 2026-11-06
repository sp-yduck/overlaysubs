use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Cpu,
    Low,
    Mid,
    High,
}

impl Tier {
    pub fn lower(self) -> Option<Tier> {
        match self {
            Tier::High => Some(Tier::Mid),
            Tier::Mid => Some(Tier::Low),
            Tier::Low => Some(Tier::Cpu),
            Tier::Cpu => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gpu {
    pub name: String,
    pub vram_mb: u64,
    /// Apple Silicon: VRAM is system RAM.
    pub unified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hardware {
    pub gpu: Option<Gpu>,
    pub ram_mb: u64,
    pub cpu_threads: usize,
    pub cpu_name: String,
}

const GB: u64 = 1024;

pub fn select_tier(hw: &Hardware) -> Tier {
    let Some(gpu) = &hw.gpu else { return Tier::Cpu };
    // 7.5GB slack: "8GB" cards report slightly less.
    let (high, mid, low) = if gpu.unified { (32 * GB, 16 * GB, 8 * GB) } else { (12 * GB, 7 * GB + 512, 4 * GB) };
    let mem = if gpu.unified { hw.ram_mb } else { gpu.vram_mb };
    match mem {
        m if m >= high - 512 => Tier::High,
        m if m >= mid - 512 => Tier::Mid,
        m if m >= low - 512 => Tier::Low,
        _ => Tier::Cpu,
    }
}

/// Real-time factor budget: ASR + translation must leave headroom for the next utterance.
pub fn too_slow(processing_secs: f32, audio_secs: f32) -> bool {
    processing_secs / audio_secs > 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hw(gpu: Option<(u64, bool)>, ram_gb: u64) -> Hardware {
        Hardware {
            gpu: gpu.map(|(v, unified)| Gpu { name: "x".into(), vram_mb: v * GB, unified }),
            ram_mb: ram_gb * GB,
            cpu_threads: 8,
            cpu_name: "x".into(),
        }
    }

    #[test]
    fn tiers_match_spec_table() {
        assert_eq!(select_tier(&hw(Some((24, false)), 32)), Tier::High);
        assert_eq!(select_tier(&hw(Some((12, false)), 16)), Tier::High);
        assert_eq!(select_tier(&hw(Some((8, false)), 16)), Tier::Mid);
        assert_eq!(select_tier(&hw(Some((6, false)), 16)), Tier::Low);
        assert_eq!(select_tier(&hw(Some((4, false)), 16)), Tier::Low);
        assert_eq!(select_tier(&hw(Some((2, false)), 16)), Tier::Cpu);
        assert_eq!(select_tier(&hw(None, 64)), Tier::Cpu);
        assert_eq!(select_tier(&hw(Some((0, true)), 32)), Tier::High);
        assert_eq!(select_tier(&hw(Some((0, true)), 16)), Tier::Mid);
        assert_eq!(select_tier(&hw(Some((0, true)), 8)), Tier::Low);
        assert_eq!(Tier::High.lower(), Some(Tier::Mid));
        assert_eq!(Tier::Cpu.lower(), None);
        assert!(too_slow(3.0, 5.0) && !too_slow(1.0, 5.0));
    }
}
