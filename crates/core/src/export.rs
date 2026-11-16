use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub start_ms: u64,
    pub end_ms: u64,
    pub lang: String,
    pub source: String,
    pub translation: String,
    /// Who said it, when speakers are told apart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<u32>,
}

/// "A", "B", … for speaker 0, 1, …
pub fn speaker_label(n: u32) -> String {
    let (letter, round) = ((b'A' + (n % 26) as u8) as char, n / 26);
    if round == 0 { letter.to_string() } else { format!("{letter}{round}") }
}

fn ts(ms: u64) -> String {
    format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

pub fn to_srt(entries: &[Entry], with_source: bool) -> String {
    entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let who = e.speaker.map_or(String::new(), |n| format!("[{}] ", speaker_label(n)));
            let body = if with_source { format!("{who}{}\n{who}{}", e.source, e.translation) } else { format!("{who}{}", e.translation) };
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
        let e = [Entry { start_ms: 3_723_004, end_ms: 3_725_000, lang: "en".into(), source: "Hi".into(), translation: "やあ".into(), speaker: None }];
        assert_eq!(to_srt(&e, true), "1\n01:02:03,004 --> 01:02:05,000\nHi\nやあ\n");
        let e = [Entry { speaker: Some(1), ..e[0].clone() }];
        assert_eq!(to_srt(&e, false), "1\n01:02:03,004 --> 01:02:05,000\n[B] やあ\n");
    }
}
