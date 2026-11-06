use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub start_ms: u64,
    pub end_ms: u64,
    pub lang: String,
    pub source: String,
    pub translation: String,
}

fn ts(ms: u64) -> String {
    format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

pub fn to_srt(entries: &[Entry], with_source: bool) -> String {
    entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let body = if with_source { format!("{}\n{}", e.source, e.translation) } else { e.translation.clone() };
            format!("{}\n{} --> {}\n{}\n", i + 1, ts(e.start_ms), ts(e.end_ms), body)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn to_txt(entries: &[Entry]) -> String {
    entries.iter().map(|e| format!("[{}] {}\n    {}\n", &ts(e.start_ms)[..8], e.source, e.translation)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srt_format() {
        let e = [Entry { start_ms: 3_723_004, end_ms: 3_725_000, lang: "en".into(), source: "Hi".into(), translation: "やあ".into() }];
        assert_eq!(to_srt(&e, true), "1\n01:02:03,004 --> 01:02:05,000\nHi\nやあ\n");
    }
}
