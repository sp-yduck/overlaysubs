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
    #[serde(deserialize_with = "target_code")]
    pub target_lang: String,
    /// Used when the speech is already in `target_lang` (e.g. en speech while target is en → ja).
    #[serde(deserialize_with = "target_code")]
    pub alt_target_lang: String,
    pub form: Option<Form>,
    pub use_context: bool,
    pub glossary: Vec<(String, String)>,
    /// Free-text hint for the translator, e.g. "Stanford commencement speech".
    pub topic: String,
}

/// Settings saved before the Chinese target split hold "zh", which PLaMo does not know; they become Traditional.
fn target_code<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let code = String::deserialize(d)?;
    Ok(if code == "zh" { "zh-hant".into() } else { code })
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self { source_lang: None, target_lang: "en".into(), alt_target_lang: "ja".into(), form: None, use_context: true, glossary: vec![], topic: String::new() }
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
/// shortcut: char cap approximating whisper's ~224-token prompt limit; count tokens if long prompts get cut.
const MAX_PROMPT_CHARS: usize = 150;
/// A sentence shorter than this stays with the next one instead of becoming its own caption.
const MIN_SENTENCE_CHARS: usize = 10;

/// False for text made only of punctuation and spaces, such as "…" or "".
pub fn has_words(text: &str) -> bool {
    text.chars().any(char::is_alphanumeric)
}

pub fn ends_sentence(text: &str) -> bool {
    text.trim_end().trim_end_matches(['"', '\'', '」', '』', ')', '）', '”', '’']).ends_with(['.', '?', '!', '。', '？', '！', '…'])
}

/// Ends `text` with a full stop unless it already ends a sentence.
/// The caption becomes Whisper's prompt for the next utterance, and Whisper copies the prompt's style:
/// a prompt without punctuation makes it stop punctuating (measured on Korean speech).
pub fn punctuate(text: &str, lang: &str) -> String {
    match (ends_sentence(text), lang) {
        (true, _) => text.to_string(),
        (false, "ja" | "zh") => format!("{text}。"),
        _ => format!("{text}."),
    }
}

/// Splits after sentence-ending punctuation (followed by a space, or full-width), so a long utterance is
/// shown and translated one sentence at a time. Sentences under `MIN_SENTENCE_CHARS` join the next one.
pub fn split_sentences(text: &str) -> Vec<String> {
    let closers = ['"', '\'', '」', '』', ')', '）', '”', '’'];
    let chars: Vec<char> = text.chars().collect();
    let (mut out, mut cur) = (Vec::new(), String::new());
    for (i, &c) in chars.iter().enumerate() {
        cur.push(c);
        let next = chars.get(i + 1).copied();
        let boundary = match c {
            '。' | '？' | '！' => !next.is_some_and(|n| closers.contains(&n) || matches!(n, '。' | '？' | '！')),
            '.' | '?' | '!' | '…' => next.is_some_and(char::is_whitespace),
            // A quote is part of the sentence when the text goes on right after it (「いいね！」と言った).
            c if closers.contains(&c) => cur.trim_end_matches(closers).ends_with(['.', '?', '!', '。', '？', '！', '…']) && next.is_some_and(char::is_whitespace),
            _ => false,
        };
        if boundary && cur.trim().chars().count() >= MIN_SENTENCE_CHARS {
            out.push(cur.trim().to_string());
            cur.clear();
        }
    }
    match (cur.trim(), out.last_mut()) {
        ("", _) => {}
        // A short tail would be a caption of its own; keep it with the sentence before it.
        (t, Some(last)) if t.chars().count() < MIN_SENTENCE_CHARS => {
            let sep = if last.ends_with(['。', '？', '！']) { "" } else { " " };
            last.push_str(sep);
            last.push_str(t);
        }
        (t, _) => out.push(t.to_string()),
    }
    out
}

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

    /// Glossary source terms, then the previous sentence; the front is cut first when too long.
    pub fn asr_prompt(&self) -> String {
        let terms: Vec<&str> = self.cfg.glossary.iter().map(|(s, _)| s.as_str()).collect();
        let prev = match self.history.last() {
            Some((src, _)) if self.cfg.use_context => src.as_str(),
            _ => "",
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

    /// A final (the endpointer heard a pause) is committed right away, even without sentence-ending punctuation.
    /// Holding it for the rest of the sentence made captions pile up to 150+ chars and 20+ s whenever Whisper
    /// left out punctuation, which it usually does for Korean.
    pub fn handle(&mut self, ev: Event, asr: &mut dyn Asr, mt: &mut dyn Translator, emit: &mut dyn FnMut(Caption)) -> Result<(), String> {
        let (pcm, fin) = match ev {
            Event::Partial(p) => (p, false),
            Event::Final(p) => (p, true),
        };
        let t = asr.transcribe(&pcm, self.cfg.source_lang.as_deref(), &self.asr_prompt())?;
        if is_hallucination(&t.text) {
            return Ok(());
        }
        if fin {
            let start_ms = self.pos_ms.saturating_sub((pcm.len() * 1000 / RATE) as u64);
            return self.finalize(t.text, t.lang, start_ms, mt, emit);
        }
        // Not final yet: show a draft translation under the same id. The source is sent together with it,
        // because a caption without a translation would blank the previous draft in the overlay.
        // A draft of an unfinished fragment can come back empty or as just "…"; showing it would replace a readable one.
        let translation = self.translate(&t.text, &t.lang, mt, &mut |_| {})?;
        if has_words(&translation) {
            emit(Caption { id: self.next_id, stage: Stage::Partial, lang: t.lang, source: t.text, translation });
        }
        Ok(())
    }

    fn translate(&self, text: &str, lang: &str, mt: &mut dyn Translator, on_piece: &mut dyn FnMut(&str)) -> Result<String, String> {
        // Whisper reports "zh" while PLaMo's target codes are "zh-hans" / "zh-hant", so compare the base language.
        let base = |code: &str| code.split('-').next().unwrap_or(code).to_string();
        let target = if base(lang) == base(&self.cfg.target_lang) { &self.cfg.alt_target_lang } else { &self.cfg.target_lang };
        // Only matching entries, so a long glossary neither bloats the prompt nor leaks unrelated terms.
        let glossary: Vec<_> = self.cfg.glossary.iter().filter(|(s, _)| text.contains(s.as_str())).cloned().collect();
        let req = TranslateRequest {
            text,
            // PLaMo has no plain "zh" code, and Whisper cannot tell Simplified from Traditional; let PLaMo detect it.
            source_lang: (lang != "zh").then_some(lang),
            target_lang: target,
            form: if target == "ja" { self.cfg.form } else { None },
            context: Some(&self.cfg.topic),
            history: if self.cfg.use_context { &self.history } else { &[] },
            glossary: &glossary,
        };
        Ok(mt.translate(&req, on_piece)?.trim().to_string())
    }

    /// One caption per sentence: the first replaces the draft (same id), the rest follow with new ids.
    /// History timestamps are spread over the utterance by sentence length.
    fn finalize(&mut self, text: String, lang: String, start_ms: u64, mt: &mut dyn Translator, emit: &mut dyn FnMut(Caption)) -> Result<(), String> {
        let sentences = split_sentences(&punctuate(&text, &lang));
        let total: usize = sentences.iter().map(|s| s.chars().count()).sum::<usize>().max(1);
        let span = self.pos_ms.saturating_sub(start_ms);
        let mut done = 0;
        for source in sentences {
            let from = start_ms + span * done as u64 / total as u64;
            done += source.chars().count();
            let to = start_ms + span * done as u64 / total as u64;
            let mut cap = Caption { id: self.next_id, stage: Stage::Streaming, lang: lang.clone(), source, translation: String::new() };
            let mut acc = String::new();
            cap.translation = self.translate(&cap.source, &cap.lang, mt, &mut |piece| {
                acc.push_str(piece);
                emit(Caption { translation: acc.clone(), ..cap.clone() });
            })?;
            cap.stage = Stage::Final;
            emit(cap.clone());
            self.next_id += 1;
            self.entries.push(Entry { start_ms: from, end_ms: to, lang: cap.lang, source: cap.source.clone(), translation: cap.translation.clone() });
            self.history.push((cap.source, cap.translation));
            if self.history.len() > HISTORY {
                self.history.remove(0);
            }
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
            let kana = text.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c));
            let lang = if text.is_ascii() { "en" } else if kana { "ja" } else { "zh" }.to_string();
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
            self.0.push(format!("{}>{} h{} {} {:?} {:?}", r.source_lang.unwrap_or("auto"), r.target_lang, r.history.len(), r.text, g, r.context.unwrap_or("")));
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
        let cfg = SessionConfig::default();
        assert_eq!((cfg.target_lang.as_str(), cfg.alt_target_lang.as_str()), ("en", "ja"), "English is the default target, Japanese the fallback when the speech is already English");
        let mut s = Session::new(cfg);
        let (mut a, mut mt, mut caps) = (asr(vec!["Hello there.", "Thank you for watching!", "こんにちは。"]), FakeMt(vec![]), vec![]);
        for _ in 0..3 {
            fin(&mut s, &mut a, &mut mt, &mut caps);
        }
        let stages: Vec<_> = caps.iter().map(|c| c.stage).collect();
        assert_eq!(stages, [Stage::Streaming, Stage::Streaming, Stage::Final, Stage::Streaming, Stage::Streaming, Stage::Final]);
        assert_eq!(caps[1].translation, "訳文");
        assert!(mt.0[0].starts_with("en>ja h0") && mt.0[1].starts_with("ja>en h1"), "hallucination dropped; English speech is already the target so it goes to the alternate language, Japanese speech to the target, with context: {:?}", mt.0);
        assert_eq!(s.entries.len(), 2);
    }

    #[test]
    fn chinese_speech_matches_a_script_specific_chinese_target() {
        let cfg = SessionConfig { target_lang: "zh-hant".into(), alt_target_lang: "ja".into(), ..Default::default() };
        let mut s = Session::new(cfg);
        let (mut a, mut mt, mut caps) = (asr(vec!["Hello there.", "今天天氣很好。"]), FakeMt(vec![]), vec![]);
        fin(&mut s, &mut a, &mut mt, &mut caps);
        fin(&mut s, &mut a, &mut mt, &mut caps);
        assert!(mt.0[0].starts_with("en>zh-hant ") && mt.0[1].starts_with("auto>ja "), "zh speech goes to the alt target, PLaMo detects the script: {:?}", mt.0);
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
    fn final_without_punctuation_is_committed_with_a_full_stop_that_reaches_the_next_prompt() {
        let mut s = Session::new(SessionConfig::default());
        let (mut a, mut mt, mut caps) = (asr(vec!["So when you blink", "you should already be behind them", "この設定を変えるのが早すぎます"]), FakeMt(vec![]), vec![]);
        for _ in 0..3 {
            fin(&mut s, &mut a, &mut mt, &mut caps);
        }
        let sources: Vec<_> = s.entries.iter().map(|e| e.source.as_str()).collect();
        assert_eq!(sources, ["So when you blink.", "you should already be behind them.", "この設定を変えるのが早すぎます。"]);
        assert_eq!(a.1[1], "So when you blink.", "Whisper sees a punctuated prompt and keeps punctuating");
        assert!(caps.iter().all(|c| c.stage != Stage::Partial), "a final is not shown as a held draft");
    }

    #[test]
    fn long_utterance_becomes_one_caption_per_sentence() {
        let mut s = Session::new(SessionConfig::default());
        let (mut a, mut mt, mut caps) = (asr(vec!["The tank is already in there. Get ready to go in here. Same way? Then go around with the tank"]), FakeMt(vec![]), vec![]);
        s.tick(10 * SEC);
        fin(&mut s, &mut a, &mut mt, &mut caps);
        let sources: Vec<_> = s.entries.iter().map(|e| e.source.as_str()).collect();
        assert_eq!(sources, ["The tank is already in there.", "Get ready to go in here.", "Same way? Then go around with the tank."], "a short sentence joins the next one");
        let finals: Vec<_> = caps.iter().filter(|c| c.stage == Stage::Final).map(|c| c.id).collect();
        assert_eq!(finals, [0, 1, 2], "the first sentence replaces the draft, the rest get their own captions");
        assert_eq!(mt.0.len(), 3, "each sentence is translated on its own");
        assert!(mt.0[1].contains(" h1 ") && mt.0[2].contains(" h2 "), "earlier sentences are context: {:?}", mt.0);
        let spans: Vec<_> = s.entries.iter().map(|e| (e.start_ms, e.end_ms)).collect();
        assert_eq!((spans[0].0, spans[2].1), (9000, 10000), "the utterance's span is shared out");
        assert!(spans.windows(2).all(|w| w[0].1 == w[1].0 && w[0].0 < w[0].1), "{spans:?}");
    }

    #[test]
    fn sentence_splitting() {
        let cases: [(&str, &[&str]); 7] = [
            ("오늘 날씨가 정말 좋아서 산책하고 싶어. 그럼 같이 공원에 가 볼까?", &["오늘 날씨가 정말 좋아서 산책하고 싶어.", "그럼 같이 공원에 가 볼까?"]),
            ("今日はいい天気ですね。明日は雨が降るそうですよ。", &["今日はいい天気ですね。", "明日は雨が降るそうですよ。"]),
            ("彼は「本当にいいね！」と言ってくれました。", &["彼は「本当にいいね！」と言ってくれました。"]),
            ("He said \"we should go now.\" Then he left the room.", &["He said \"we should go now.\"", "Then he left the room."]),
            ("It costs 1.5 dollars per item today.", &["It costs 1.5 dollars per item today."]),
            ("그게.. 다시 설명해 줄게.", &["그게.. 다시 설명해 줄게."]),
            ("I think we should go now. Okay.", &["I think we should go now. Okay."]),
        ];
        for (text, want) in cases {
            assert_eq!(split_sentences(text), want, "{text}");
        }
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
    fn config_edited_between_utterances_applies_to_the_next_one() {
        let mut s = Session::new(SessionConfig::default());
        let (mut a, mut mt, mut caps) = (asr(vec!["Hello there.", "The dots connect."]), FakeMt(vec![]), vec![]);
        fin(&mut s, &mut a, &mut mt, &mut caps);
        // The settings UI saves while a session runs and the session is not rebuilt, so the next
        // utterance must use the new values and the entries recorded so far must be kept.
        s.cfg = SessionConfig {
            target_lang: "en".into(),
            alt_target_lang: "ko".into(),
            use_context: false,
            glossary: vec![("dots".into(), "点".into())],
            topic: "a lecture".into(),
            ..Default::default()
        };
        fin(&mut s, &mut a, &mut mt, &mut caps);
        assert_eq!(mt.0[0], r#"en>ja h0 Hello there. [] """#, "the first utterance uses the old settings");
        assert_eq!(mt.0[1], r#"en>ko h0 The dots connect. ["dots"] "a lecture""#, "the new alternate language, context flag, glossary and topic apply right away");
        assert_eq!(s.entries.len(), 2, "the entries recorded before the change are kept");
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
        for (t, lang, want) in [("그래서", "ko", "그래서."), ("Hi!", "en", "Hi!"), ("そうです", "ja", "そうです。"), ("本当？", "ja", "本当？")] {
            assert_eq!(punctuate(t, lang), want);
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

    #[test]
    fn old_plain_chinese_targets_load_as_traditional() {
        let c: SessionConfig = serde_json::from_str(r#"{"source_lang":"zh","target_lang":"zh","alt_target_lang":"zh"}"#).unwrap();
        assert_eq!((c.source_lang.as_deref(), c.target_lang.as_str(), c.alt_target_lang.as_str()), (Some("zh"), "zh-hant", "zh-hant"), "Whisper's source code stays \"zh\"");
        let c: SessionConfig = serde_json::from_str(r#"{"target_lang":"zh-hans","alt_target_lang":"en"}"#).unwrap();
        assert_eq!((c.target_lang.as_str(), c.alt_target_lang.as_str()), ("zh-hans", "en"));
    }
}
