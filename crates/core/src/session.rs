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
    /// `lang = None` auto-detects. `prompt` biases spelling (glossary terms, previous sentence); may be empty.
    fn transcribe(&mut self, pcm: &[f32], lang: Option<&str>, prompt: &str) -> Result<Transcript, String>;
}

pub trait Translator: Send {
    fn translate(&mut self, req: &TranslateRequest, on_piece: &mut dyn FnMut(&str)) -> Result<String, String>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionConfig {
    pub source_lang: Option<String>,
    pub target_lang: String,
    /// Used when the speech is already in `target_lang` (e.g. ja speech while target is ja → en).
    pub alt_target_lang: String,
    pub form: Option<Form>,
    pub use_context: bool,
    pub glossary: Vec<(String, String)>,
    /// Free-text hint for the translator, e.g. "Stanford commencement speech".
    pub topic: String,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self { source_lang: None, target_lang: "ja".into(), alt_target_lang: "en".into(), form: None, use_context: true, glossary: vec![], topic: String::new() }
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
/// A fragment without sentence-ending punctuation is held and merged with the next one, up to this length.
const MAX_HOLD_CHARS: usize = 150;
/// shortcut: char cap approximating whisper's ~224-token prompt limit; count tokens if long prompts get cut.
const MAX_PROMPT_CHARS: usize = 150;

/// False for text made only of punctuation and spaces, such as "…" or "".
pub fn has_words(text: &str) -> bool {
    text.chars().any(char::is_alphanumeric)
}

pub fn ends_sentence(text: &str) -> bool {
    text.trim_end().trim_end_matches(['"', '\'', '」', '』', ')', '）', '”', '’']).ends_with(['.', '?', '!', '。', '？', '！', '…'])
}

fn join(a: &str, b: &str, lang: &str) -> String {
    match (a.is_empty(), lang) {
        (true, _) => b.to_string(),
        (false, "ja" | "zh") => format!("{a}{b}"),
        _ => format!("{a} {b}"),
    }
}

/// An unfinished sentence waiting for its continuation.
struct Pending {
    text: String,
    lang: String,
    start_ms: u64,
}

pub struct Session {
    pub cfg: SessionConfig,
    pub entries: Vec<Entry>,
    history: Vec<(String, String)>,
    pending: Option<Pending>,
    next_id: u64,
    pos_ms: u64,
}

impl Session {
    pub fn new(cfg: SessionConfig) -> Self {
        Self { cfg, entries: vec![], history: vec![], pending: None, next_id: 0, pos_ms: 0 }
    }

    /// Advance the wall clock used for history timestamps (call with every captured chunk).
    pub fn tick(&mut self, samples: usize) {
        self.pos_ms += (samples * 1000 / RATE) as u64;
    }

    /// Glossary source terms, then the previous sentence; the front is cut first when too long.
    pub fn asr_prompt(&self) -> String {
        let terms: Vec<&str> = self.cfg.glossary.iter().map(|(s, _)| s.as_str()).collect();
        let prev = match (&self.pending, self.history.last()) {
            _ if !self.cfg.use_context => "",
            (Some(p), _) => &p.text,
            (None, Some((src, _))) => src,
            (None, None) => "",
        };
        // Whisper mimics the prompt's style, so the term list ends with a period to keep punctuation on.
        let p = match (terms.is_empty(), prev.is_empty()) {
            (true, _) => prev.to_string(),
            (false, true) => format!("{}.", terms.join(", ")),
            (false, false) => format!("{}. {prev}", terms.join(", ")),
        };
        let n = p.chars().count();
        p.chars().skip(n.saturating_sub(MAX_PROMPT_CHARS)).collect()
    }

    pub fn handle(&mut self, ev: Event, asr: &mut dyn Asr, mt: &mut dyn Translator, emit: &mut dyn FnMut(Caption)) -> Result<(), String> {
        let (pcm, fin) = match ev {
            Event::Partial(p) => (p, false),
            Event::Final(p) => (p, true),
        };
        let t = asr.transcribe(&pcm, self.cfg.source_lang.as_deref(), &self.asr_prompt())?;
        if is_hallucination(&t.text) {
            return Ok(());
        }
        let start_ms = self.pending.as_ref().map_or(self.pos_ms.saturating_sub((pcm.len() * 1000 / RATE) as u64), |p| p.start_ms);
        let text = join(self.pending.as_ref().map_or("", |p| &p.text), &t.text, &t.lang);
        if fin && (ends_sentence(&text) || text.chars().count() >= MAX_HOLD_CHARS) {
            self.pending = None;
            return self.finalize(text, t.lang, start_ms, mt, emit);
        }
        if fin {
            self.pending = Some(Pending { text: text.clone(), lang: t.lang.clone(), start_ms });
        }
        // Not final yet: show a draft translation under the same id. The source is sent together with it,
        // because a caption without a translation would blank the previous draft in the overlay.
        // A draft of an unfinished fragment can come back empty or as just "…"; showing it would replace a readable one.
        let translation = self.translate(&text, &t.lang, mt, &mut |_| {})?;
        if has_words(&translation) {
            emit(Caption { id: self.next_id, stage: Stage::Partial, lang: t.lang, source: text, translation });
        }
        Ok(())
    }

    /// Finalize a held fragment, e.g. after the speaker has been quiet for a while.
    pub fn flush(&mut self, mt: &mut dyn Translator, emit: &mut dyn FnMut(Caption)) -> Result<(), String> {
        match self.pending.take() {
            Some(p) => self.finalize(p.text, p.lang, p.start_ms, mt, emit),
            None => Ok(()),
        }
    }

    fn translate(&self, text: &str, lang: &str, mt: &mut dyn Translator, on_piece: &mut dyn FnMut(&str)) -> Result<String, String> {
        let target = if lang == self.cfg.target_lang { &self.cfg.alt_target_lang } else { &self.cfg.target_lang };
        // Only matching entries, so a long glossary neither bloats the prompt nor leaks unrelated terms.
        let glossary: Vec<_> = self.cfg.glossary.iter().filter(|(s, _)| text.contains(s.as_str())).cloned().collect();
        let req = TranslateRequest {
            text,
            source_lang: Some(lang),
            target_lang: target,
            form: if target == "ja" { self.cfg.form } else { None },
            context: Some(&self.cfg.topic),
            history: if self.cfg.use_context { &self.history } else { &[] },
            glossary: &glossary,
        };
        Ok(mt.translate(&req, on_piece)?.trim().to_string())
    }

    fn finalize(&mut self, text: String, lang: String, start_ms: u64, mt: &mut dyn Translator, emit: &mut dyn FnMut(Caption)) -> Result<(), String> {
        let mut cap = Caption { id: self.next_id, stage: Stage::Streaming, lang, source: text, translation: String::new() };
        let mut acc = String::new();
        cap.translation = self.translate(&cap.source, &cap.lang, mt, &mut |piece| {
            acc.push_str(piece);
            emit(Caption { translation: acc.clone(), ..cap.clone() });
        })?;
        cap.stage = Stage::Final;
        emit(cap.clone());
        self.next_id += 1;
        self.entries.push(Entry { start_ms, end_ms: self.pos_ms, lang: cap.lang, source: cap.source.clone(), translation: cap.translation.clone() });
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

    /// Returns scripted texts and records the prompts it was given.
    struct FakeAsr(Vec<&'static str>, Vec<String>);
    impl Asr for FakeAsr {
        fn transcribe(&mut self, _: &[f32], _: Option<&str>, prompt: &str) -> Result<Transcript, String> {
            self.1.push(prompt.to_string());
            let text = self.0.remove(0).to_string();
            let lang = if text.is_ascii() { "en" } else { "ja" }.to_string();
            Ok(Transcript { text, lang })
        }
    }
    fn asr(texts: Vec<&'static str>) -> FakeAsr {
        FakeAsr(texts, vec![])
    }
    /// Records "src>dst h<history> <text> [glossary] {context}" per call.
    struct FakeMt(Vec<String>);
    impl Translator for FakeMt {
        fn translate(&mut self, r: &TranslateRequest, on: &mut dyn FnMut(&str)) -> Result<String, String> {
            let g: Vec<_> = r.glossary.iter().map(|(s, _)| s.as_str()).collect();
            self.0.push(format!("{}>{} h{} {} {:?} {:?}", r.source_lang.unwrap(), r.target_lang, r.history.len(), r.text, g, r.context.unwrap_or("")));
            on("訳");
            on("文");
            Ok("訳文".into())
        }
    }
    const SEC: usize = 16000;
    fn fin(s: &mut Session, a: &mut FakeAsr, mt: &mut FakeMt, caps: &mut Vec<Caption>) {
        s.handle(Event::Final(vec![0.0; SEC]), a, mt, &mut |c| caps.push(c)).unwrap();
    }

    #[test]
    fn final_streams_then_records_history_and_routes_language() {
        let mut s = Session::new(SessionConfig::default());
        let (mut a, mut mt, mut caps) = (asr(vec!["Hello there.", "Thank you for watching!", "こんにちは。"]), FakeMt(vec![]), vec![]);
        for _ in 0..3 {
            fin(&mut s, &mut a, &mut mt, &mut caps);
        }
        let stages: Vec<_> = caps.iter().map(|c| c.stage).collect();
        assert_eq!(stages, [Stage::Streaming, Stage::Streaming, Stage::Final, Stage::Streaming, Stage::Streaming, Stage::Final]);
        assert_eq!(caps[1].translation, "訳文");
        assert!(mt.0[0].starts_with("en>ja h0") && mt.0[1].starts_with("ja>en h1"), "hallucination dropped, ja speech goes to alt target with context: {:?}", mt.0);
        assert_eq!(s.entries.len(), 2);
    }

    #[test]
    fn partial_emits_one_translated_draft_without_recording() {
        let mut s = Session::new(SessionConfig::default());
        let mut caps = vec![];
        s.handle(Event::Partial(vec![0.0; SEC]), &mut asr(vec!["Hello"]), &mut FakeMt(vec![]), &mut |c| caps.push(c)).unwrap();
        assert_eq!(caps.len(), 1, "no caption with an empty translation, which would blank the previous draft");
        assert!(caps[0].source == "Hello" && caps[0].translation == "訳文" && caps[0].stage == Stage::Partial);
        assert!(s.entries.is_empty());
    }

    /// Returns scripted translations in one piece.
    struct ScriptMt(Vec<&'static str>);
    impl Translator for ScriptMt {
        fn translate(&mut self, _: &TranslateRequest, on: &mut dyn FnMut(&str)) -> Result<String, String> {
            let out = self.0.remove(0);
            on(out);
            Ok(out.into())
        }
    }

    #[test]
    fn partial_without_words_in_the_translation_is_not_shown() {
        let mut s = Session::new(SessionConfig::default());
        let (mut a, mut mt, mut caps) = (asr(vec!["So when", "So when you", "So when you blink"]), ScriptMt(vec!["それで", "…", ""]), vec![]);
        for _ in 0..3 {
            s.handle(Event::Partial(vec![0.0; SEC]), &mut a, &mut mt, &mut |c| caps.push(c)).unwrap();
        }
        let shown: Vec<_> = caps.iter().map(|c| c.translation.as_str()).collect();
        assert_eq!(shown, ["それで"], "a draft of only \"…\" or nothing would replace the readable one");
    }

    #[test]
    fn unfinished_sentence_is_held_and_merged_with_the_next_final() {
        let mut s = Session::new(SessionConfig::default());
        let (mut a, mut mt, mut caps) = (asr(vec!["So when you blink", "you should already be behind them."]), FakeMt(vec![]), vec![]);
        s.tick(SEC);
        fin(&mut s, &mut a, &mut mt, &mut caps);
        assert!(s.entries.is_empty() && caps.iter().all(|c| c.stage == Stage::Partial && c.id == 0), "held fragment shows as a draft");
        s.tick(2 * SEC);
        fin(&mut s, &mut a, &mut mt, &mut caps);
        assert_eq!(s.entries.len(), 1);
        let e = &s.entries[0];
        assert_eq!(e.source, "So when you blink you should already be behind them.");
        assert_eq!((e.start_ms, e.end_ms), (0, 3000), "entry spans both fragments");
        assert_eq!(caps.last().unwrap().id, 0, "the merged final replaces the draft");
    }

    #[test]
    fn japanese_fragments_join_without_space_and_flush_finalizes() {
        let mut s = Session::new(SessionConfig::default());
        let (mut a, mut mt, mut caps) = (asr(vec!["この設定を変えるのが", "早すぎます"]), FakeMt(vec![]), vec![]);
        fin(&mut s, &mut a, &mut mt, &mut caps);
        fin(&mut s, &mut a, &mut mt, &mut caps);
        assert!(s.entries.is_empty());
        s.flush(&mut mt, &mut |c| caps.push(c)).unwrap();
        assert_eq!(s.entries[0].source, "この設定を変えるのが早すぎます");
        s.flush(&mut mt, &mut |c| caps.push(c)).unwrap();
        assert_eq!(s.entries.len(), 1, "second flush is a no-op");
    }

    #[test]
    fn long_held_text_is_finalized_anyway() {
        let mut s = Session::new(SessionConfig::default());
        let long: &'static str = Box::leak("word ".repeat(40).into_boxed_str());
        let (mut a, mut mt, mut caps) = (asr(vec![long]), FakeMt(vec![]), vec![]);
        fin(&mut s, &mut a, &mut mt, &mut caps);
        assert_eq!(s.entries.len(), 1);
    }

    #[test]
    fn glossary_and_previous_sentence_go_to_asr_topic_and_matching_terms_to_mt() {
        let cfg = SessionConfig {
            glossary: vec![("dots".into(), "点".into()), ("calligraphy".into(), "カリグラフィー".into())],
            topic: "Stanford commencement speech".into(),
            ..Default::default()
        };
        let mut s = Session::new(cfg);
        let (mut a, mut mt, mut caps) = (asr(vec!["The dots connect later.", "Trust the process."]), FakeMt(vec![]), vec![]);
        fin(&mut s, &mut a, &mut mt, &mut caps);
        fin(&mut s, &mut a, &mut mt, &mut caps);
        assert_eq!(a.1, ["dots, calligraphy.", "dots, calligraphy. The dots connect later."]);
        assert_eq!(mt.0[0], r#"en>ja h0 The dots connect later. ["dots"] "Stanford commencement speech""#);
        assert!(mt.0[1].contains("[]"), "no matching terms: {}", mt.0[1]);
    }

    #[test]
    fn asr_prompt_keeps_the_tail_and_respects_use_context() {
        let mut s = Session::new(SessionConfig { glossary: vec![("x".repeat(200), "y".into())], ..Default::default() });
        let (mut a, mut mt, mut caps) = (asr(vec!["Done."]), FakeMt(vec![]), vec![]);
        fin(&mut s, &mut a, &mut mt, &mut caps);
        let p = s.asr_prompt();
        assert!(p.chars().count() == MAX_PROMPT_CHARS && p.ends_with(". Done."), "{p}");
        s.cfg.use_context = false;
        assert!(!s.asr_prompt().contains("Done."));
    }

    #[test]
    fn sentence_end_detection() {
        for t in ["Hi.", "本当？", "いいね！」", "そうです。 ", "Really?\""] {
            assert!(ends_sentence(t), "{t}");
        }
        for t in ["So when", "그래서", "1,000"] {
            assert!(!ends_sentence(t), "{t}");
        }
        for (t, words) in [("…", false), ("...", false), ("", false), (" 。", false), ("でも…", true), ("그래서", true), ("1", true)] {
            assert_eq!(has_words(t), words, "{t:?}");
        }
    }

    #[test]
    fn old_settings_without_new_fields_still_load() {
        let c: SessionConfig = serde_json::from_str(r#"{"source_lang":null,"target_lang":"ja","alt_target_lang":"en","form":null,"use_context":true,"glossary":[["a","b"]]}"#).unwrap();
        assert!(c.topic.is_empty() && c.glossary.len() == 1);
    }
}
