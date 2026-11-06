//! One captioning session: endpointer events in, caption updates out. Engines are injected.
use crate::endpoint::{Event, RATE};
use crate::export::Entry;
use crate::pfml::{Form, TranslateRequest};
use serde::{Deserialize, Serialize};

pub struct Transcript {
    pub text: String,
    pub lang: String,
}

pub trait Asr: Send {
    /// `lang = None` auto-detects.
    fn transcribe(&mut self, pcm: &[f32], lang: Option<&str>) -> Result<Transcript, String>;
}

pub trait Translator: Send {
    fn translate(&mut self, req: &TranslateRequest, on_piece: &mut dyn FnMut(&str)) -> Result<String, String>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub source_lang: Option<String>,
    pub target_lang: String,
    /// Used when the speech is already in `target_lang` (e.g. ja speech while target is ja → en).
    pub alt_target_lang: String,
    pub form: Option<Form>,
    pub use_context: bool,
    pub glossary: Vec<(String, String)>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self { source_lang: None, target_lang: "ja".into(), alt_target_lang: "en".into(), form: None, use_context: true, glossary: vec![] }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Partial,
    Streaming,
    Final,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Caption {
    pub id: u64,
    pub stage: Stage,
    pub lang: String,
    pub source: String,
    pub translation: String,
}

/// Whisper's well-known outputs on silence/music.
const HALLUCINATIONS: &[&str] = &[
    "thank you for watching",
    "thanks for watching",
    "please subscribe",
    "subtitles by",
    "ご視聴ありがとうございました",
    "チャンネル登録",
    "字幕",
    "시청해 주셔서 감사합니다",
];

pub fn is_hallucination(text: &str) -> bool {
    let t = text.trim().trim_matches(|c: char| c.is_ascii_punctuation() || c == '。' || c == '♪').to_lowercase();
    t.chars().filter(|c| c.is_alphanumeric()).count() < 2 || HALLUCINATIONS.iter().any(|h| t.contains(h))
}

const HISTORY: usize = 3;

pub struct Session {
    pub cfg: SessionConfig,
    pub entries: Vec<Entry>,
    history: Vec<(String, String)>,
    next_id: u64,
    pos_ms: u64,
}

impl Session {
    pub fn new(cfg: SessionConfig) -> Self {
        Self { cfg, entries: vec![], history: vec![], next_id: 0, pos_ms: 0 }
    }

    /// Advance the wall clock used for history timestamps (call with every captured chunk).
    pub fn tick(&mut self, samples: usize) {
        self.pos_ms += (samples * 1000 / RATE) as u64;
    }

    pub fn handle(&mut self, ev: Event, asr: &mut dyn Asr, mt: &mut dyn Translator, emit: &mut dyn FnMut(Caption)) -> Result<(), String> {
        let (pcm, fin) = match ev {
            Event::Partial(p) => (p, false),
            Event::Final(p) => (p, true),
        };
        let t = asr.transcribe(&pcm, self.cfg.source_lang.as_deref())?;
        if is_hallucination(&t.text) {
            return Ok(());
        }
        let id = self.next_id;
        let target = if t.lang == self.cfg.target_lang { self.cfg.alt_target_lang.clone() } else { self.cfg.target_lang.clone() };
        let history = if self.cfg.use_context { &self.history[..] } else { &[] };
        let req = TranslateRequest {
            text: &t.text,
            source_lang: Some(&t.lang),
            target_lang: &target,
            form: if target == "ja" { self.cfg.form } else { None },
            context: None,
            history,
            glossary: &self.cfg.glossary,
        };
        let mut cap = Caption { id, stage: Stage::Partial, lang: t.lang.clone(), source: t.text.clone(), translation: String::new() };
        if !fin {
            emit(cap.clone());
            cap.translation = mt.translate(&req, &mut |_| {})?;
            emit(cap);
            return Ok(());
        }
        cap.stage = Stage::Streaming;
        let mut acc = String::new();
        let out = mt.translate(&req, &mut |piece| {
            acc.push_str(piece);
            emit(Caption { translation: acc.clone(), ..cap.clone() });
        })?;
        cap.stage = Stage::Final;
        cap.translation = out.trim().to_string();
        emit(cap.clone());
        self.next_id += 1;
        let dur = (pcm.len() * 1000 / RATE) as u64;
        self.entries.push(Entry { start_ms: self.pos_ms.saturating_sub(dur), end_ms: self.pos_ms, lang: t.lang, source: cap.source.clone(), translation: cap.translation.clone() });
        self.history.push((cap.source, cap.translation));
        if self.history.len() > HISTORY {
            self.history.remove(0);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeAsr(Vec<&'static str>);
    impl Asr for FakeAsr {
        fn transcribe(&mut self, _: &[f32], _: Option<&str>) -> Result<Transcript, String> {
            let text = self.0.remove(0).to_string();
            let lang = if text.is_ascii() { "en" } else { "ja" }.to_string();
            Ok(Transcript { text, lang })
        }
    }
    struct FakeMt(Vec<String>);
    impl Translator for FakeMt {
        fn translate(&mut self, r: &TranslateRequest, on: &mut dyn FnMut(&str)) -> Result<String, String> {
            self.0.push(format!("{}>{} h{}", r.source_lang.unwrap(), r.target_lang, r.history.len()));
            on("訳");
            on("文");
            Ok("訳文".into())
        }
    }

    #[test]
    fn final_streams_then_records_history_and_routes_language() {
        let mut s = Session::new(SessionConfig::default());
        let mut asr = FakeAsr(vec!["Hello there", "Thank you for watching!", "こんにちは"]);
        let mut mt = FakeMt(vec![]);
        let mut caps = vec![];
        for _ in 0..3 {
            s.handle(Event::Final(vec![0.0; 16000]), &mut asr, &mut mt, &mut |c| caps.push(c)).unwrap();
        }
        let stages: Vec<_> = caps.iter().map(|c| c.stage).collect();
        assert_eq!(stages, [Stage::Streaming, Stage::Streaming, Stage::Final, Stage::Streaming, Stage::Streaming, Stage::Final]);
        assert_eq!(caps[1].translation, "訳文");
        assert_eq!(mt.0, ["en>ja h0", "ja>en h1"], "hallucination dropped, ja speech goes to alt target with context");
        assert_eq!(s.entries.len(), 2);
    }

    #[test]
    fn partial_emits_source_then_translation_without_recording() {
        let mut s = Session::new(SessionConfig::default());
        let mut caps = vec![];
        s.handle(Event::Partial(vec![0.0; 16000]), &mut FakeAsr(vec!["Hello"]), &mut FakeMt(vec![]), &mut |c| caps.push(c)).unwrap();
        assert_eq!(caps.len(), 2);
        assert!(caps[0].translation.is_empty() && caps[1].translation == "訳文" && caps[1].stage == Stage::Partial);
        assert!(s.entries.is_empty());
    }
}
