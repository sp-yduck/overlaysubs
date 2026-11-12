use crate::tier::Tier;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Asr,
    Translator,
    Vad,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDef {
    pub id: String,
    pub kind: Kind,
    pub label: String,
    pub url: String,
    pub file: String,
    pub size: u64,
    pub sha256: String,
    /// Tiers for which this model is the automatic choice.
    #[serde(default)]
    pub default_for: Vec<Tier>,
}

const HF: &str = "https://huggingface.co";

impl ModelDef {
    /// The download URL, with the Hugging Face host swapped for a user-chosen mirror (e.g. where
    /// huggingface.co is unreachable). The pinned sha256 still verifies whatever the mirror serves.
    pub fn url_via(&self, mirror: &str) -> String {
        let mirror = mirror.trim().trim_end_matches('/');
        match self.url.strip_prefix(HF) {
            Some(path) if !mirror.is_empty() && path.starts_with('/') => format!("{mirror}{path}"),
            _ => self.url.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Registry {
    pub models: Vec<ModelDef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub asr: String,
    pub translator: String,
    pub vad: String,
}

impl Registry {
    pub fn parse(json: &str) -> Result<Self, String> {
        let r: Registry = serde_json::from_str(json).map_err(|e| e.to_string())?;
        for t in [Tier::Cpu, Tier::Low, Tier::Mid, Tier::High] {
            r.defaults(t).ok_or_else(|| format!("models.json has no default for tier {t:?}"))?;
        }
        Ok(r)
    }

    pub fn get(&self, id: &str) -> Option<&ModelDef> {
        self.models.iter().find(|m| m.id == id)
    }

    pub fn defaults(&self, tier: Tier) -> Option<Selection> {
        let pick = |k: Kind| {
            self.models
                .iter()
                .find(|m| m.kind == k && (k == Kind::Vad || m.default_for.contains(&tier)))
                .map(|m| m.id.clone())
        };
        Some(Selection { asr: pick(Kind::Asr)?, translator: pick(Kind::Translator)?, vad: pick(Kind::Vad)? })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_registry_covers_every_tier() {
        let r = Registry::parse(include_str!("../../../models.json")).unwrap();
        let mid = r.defaults(Tier::Mid).unwrap();
        assert!(r.get(&mid.asr).unwrap().file.contains("turbo"));
        assert!(r.get(&mid.translator).unwrap().file.contains("plamo"));
        assert!(r.models.iter().all(|m| m.sha256.len() == 64 && m.url.starts_with("https://")));
        // A moving ref like `main` lets upstream re-uploads break the pinned sha256.
        assert!(r.models.iter().all(|m| !m.url.contains("/resolve/main/")));
    }

    #[test]
    fn mirror_replaces_only_the_hugging_face_host() {
        let r = Registry::parse(include_str!("../../../models.json")).unwrap();
        let m = &r.models[0];
        assert_eq!(m.url_via(""), m.url);
        assert_eq!(m.url_via("  "), m.url);
        let path = m.url.strip_prefix("https://huggingface.co").unwrap();
        assert_eq!(m.url_via(" https://mirror.example/ "), format!("https://mirror.example{path}"));
        let other = ModelDef { url: "https://example.com/a.bin".into(), ..m.clone() };
        assert_eq!(other.url_via("https://mirror.example"), other.url);
    }
}
