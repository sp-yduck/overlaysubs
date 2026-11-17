//! One captioning session: endpointer events in, caption updates out. Engines are injected.
use crate::endpoint::{Event, RATE};
use crate::export::Entry;
use crate::pfml::{Form, TranslateRequest};
use crate::speaker::{Clusterer, Embedder, Segment, Segmenter};
use serde::{Deserialize, Serialize};

pub struct Transcript {
    pub text: String,
    pub lang: String,
    /// Where words end: (byte offset in `text`, sample offset in the audio). May be empty.
    pub word_ends: Vec<(usize, usize)>,
}

/// Byte offsets where words end: before a space or the end and, as Japanese and Chinese have no spaces,
/// before every kana or CJK ideograph.
/// shortcut: kana and CJK ideographs only; add ranges if Thai, Lao, Khmer or Burmese captions repeat words.
fn word_ends(text: &str) -> impl Iterator<Item = usize> + '_ {
    let unspaced = |c: char| matches!(c, '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{9fff}');
    text.char_indices().filter_map(move |(i, c)| {
        let e = i + c.len_utf8();
        let next = text[e..].chars().next();
        (!c.is_whitespace() && next.is_none_or(|n| n.is_whitespace() || unspaced(n))).then_some(e)
    })
}

/// Byte offset in `text` where what follows `overlap` begins: `overlap` is committed text whose audio is
/// heard again after the cut, so a transcript starts with (roughly) those words. 0 when no word prefix of
/// `text` is close to it.
fn strip_overlap(overlap: &str, text: &str) -> usize {
    let n = word_ends(overlap).count() + 4;
    match std::iter::once(0).chain(word_ends(text)).take(n + 1).map(|e| (crate::metrics::cer(overlap, &text[..e]), e)).min_by(|a, b| a.0.total_cmp(&b.0)) {
        Some((d, e)) if d <= 0.4 => e,
        _ => 0,
    }
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
    /// Who said it (0, 1, …) when speakers are told apart; on a draft, the speaker so far.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<u32>,
}

/// The audio of text being committed.
struct Audio<'a> {
    pcm: &'a [f32],
    /// Stream position of `pcm[0]`.
    start: usize,
    /// Samples at the front that belong to text committed before (heard again after an early commit).
    skip: usize,
    /// (byte offset in the text, sample offset in `pcm`) where words end.
    word_ends: &'a [(usize, usize)],
}

/// Byte offsets in `text` where each of `sentences` (made from it by `split_sentences`, which only trims and
/// rejoins spaces) ends, counting the characters that are not spaces.
fn sentence_ends(text: &str, sentences: &[String]) -> Vec<usize> {
    let mut want = 0;
    let mut chars = text.char_indices().filter(|(_, c)| !c.is_whitespace()).map(|(i, c)| i + c.len_utf8()).enumerate();
    sentences
        .iter()
        .map(|s| {
            want += s.chars().filter(|c| !c.is_whitespace()).count();
            chars.by_ref().find(|&(n, _)| n + 1 >= want).map_or(text.len(), |(_, e)| e)
        })
        .collect()
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

/// Byte offsets just past sentence-ending punctuation (and a closing quote) that is followed by a space or the end.
fn sentence_bounds(text: &str) -> Vec<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let closers = ['"', '\'', '」', '』', ')', '）', '”', '’'];
    let mut out = vec![];
    for (i, &(_, c)) in chars.iter().enumerate() {
        let full = matches!(c, '。' | '？' | '！');
        if !full && !matches!(c, '.' | '?' | '!' | '…') {
            continue;
        }
        let mut j = i + 1;
        while chars.get(j).is_some_and(|(_, c)| closers.contains(c)) {
            j += 1;
        }
        let end = chars.get(j).map_or(text.len(), |&(a, _)| a);
        let next = chars.get(j).map(|&(_, c)| c);
        if (full || next.is_none_or(char::is_whitespace)) && out.last() != Some(&end) {
            out.push(end);
        }
    }
    out
}

/// The longest run of whole sentences at the start of `now` that `prev` (the partial before) also had,
/// with words still following it: (byte offset where it ends, sample offset of its last word).
fn stable_head(prev: &str, now: &str, word_ends: &[(usize, usize)]) -> Option<(usize, usize)> {
    let prev_bounds = sentence_bounds(prev);
    sentence_bounds(now).into_iter().rev().find_map(|b| {
        let head = &now[..b];
        if !has_words(&now[b..]) || head.trim().chars().count() < MIN_SENTENCE_CHARS {
            return None;
        }
        if !prev_bounds.iter().any(|&pb| crate::metrics::cer(head, &prev[..pb]) <= 0.1) {
            return None;
        }
        word_ends.iter().find(|&&(o, _)| o >= b).map(|&(_, t)| (b, t))
    })
}

pub struct Session {
    pub cfg: SessionConfig,
    pub entries: Vec<Entry>,
    history: Vec<(String, String)>,
    next_id: u64,
    pos_ms: u64,
    /// Commit sentences before the endpointer's final (see `handle`). On by default.
    pub early_commit: bool,
    /// How many times sentences were committed before the final.
    pub early_commits: usize,
    /// Commit at a change of speaker inside a draft (needs an embedder). Off by default.
    pub speaker_commit: bool,
    /// How many times that happened.
    pub speaker_commits: usize,
    /// Stream position of the speaker change the previous partial found, to be confirmed by the next.
    pending_turn: Option<usize>,
    /// Stream position (samples) before which the audio is committed and no longer transcribed.
    committed: usize,
    /// The previous partial's text.
    prev: Option<String>,
    /// Committed words still in the audio after the cut.
    overlap: Option<String>,
    /// Tells speakers apart when set.
    pub embedder: Option<Box<dyn Embedder>>,
    /// Finds changes of speaker inside a draft when set (with `speaker_commit`); without one, windows of the
    /// embedder are compared instead.
    pub segmenter: Option<Box<dyn Segmenter>>,
    /// Seconds spent in the segmenter.
    pub segment_s: f64,
    pub speakers: Clusterer,
    /// Every committed sentence's speaker decision, for the benchmark.
    pub segments: Vec<Segment>,
    /// Seconds spent embedding.
    pub embed_s: f64,
}

impl Session {
    pub fn new(cfg: SessionConfig) -> Self {
        Self { cfg, entries: vec![], history: vec![], next_id: 0, pos_ms: 0, early_commit: true, early_commits: 0, speaker_commit: false, speaker_commits: 0, pending_turn: None, committed: 0, prev: None, overlap: None, embedder: None, segmenter: None, segment_s: 0.0, speakers: Clusterer::default(), segments: vec![], embed_s: 0.0 }
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
    ///
    /// Speakers rarely pause between sentences (on a 5 min English talk, 79 of 98 sentence ends inside an
    /// utterance had under 100 ms of silence), so waiting for the final held 4-7 sentences back and then showed
    /// them at once. Instead, a sentence two partials in a row agree on is committed right away and the audio
    /// before it is dropped (`committed()`). Whisper's word times are rough, and cutting right at the last word
    /// lost or garbled the words around the cut, so the cut is a second early and the committed words heard
    /// again are removed from later transcripts by matching the text.
    pub fn handle(&mut self, ev: Event, asr: &mut dyn Asr, mt: &mut dyn Translator, emit: &mut dyn FnMut(Caption)) -> Result<(), String> {
        let (mut pcm, mut start, fin) = match ev {
            Event::Partial { pcm, start } => (pcm, start, false),
            Event::Final { pcm, start } => (pcm, start, true),
        };
        if start < self.committed {
            let skip = self.committed - start;
            if skip >= pcm.len() {
                return Ok(());
            }
            pcm.drain(..skip);
            start = self.committed;
        }
        let t = asr.transcribe(&pcm, self.cfg.source_lang.as_deref(), &self.asr_prompt())?;
        if is_hallucination(&t.text) {
            return Ok(());
        }
        let (mut text, mut word_ends) = (t.text, t.word_ends);
        // Audio before this belongs to the committed words heard again.
        let mut skip = 0;
        if let Some(ov) = &self.overlap {
            let rest = text[strip_overlap(ov, &text)..].trim_start().to_string();
            let cut = text.len() - rest.len();
            skip = word_ends.iter().filter(|&&(o, _)| o <= cut && cut > 0).map(|&(_, t)| t).max().unwrap_or(0);
            word_ends = word_ends.into_iter().filter(|&(o, _)| o > cut).map(|(o, t)| (o - cut, t)).collect();
            text = rest;
        }
        if fin {
            (self.overlap, self.prev, self.pending_turn) = (None, None, None);
        }
        if !has_words(&text) {
            return Ok(());
        }
        // History timestamps follow the wall clock (`tick`); the event's audio ends about now.
        let pos_ms = self.pos_ms;
        let ago = move |samples: usize| pos_ms.saturating_sub((samples * 1000 / RATE) as u64);
        let start_ms = ago(pcm.len());
        if fin {
            let audio = Audio { pcm: &pcm, start, skip, word_ends: &word_ends };
            return self.finalize(text, t.lang, start_ms, self.pos_ms, audio, mt, emit);
        }
        // Where the draft's own audio starts in `pcm`.
        let mut draft_from = skip;
        let prev = self.prev.take().filter(|_| self.early_commit);
        let stable = prev.and_then(|prev| stable_head(&prev, &text, &word_ends)).filter(|&(_, end)| end > 0 && end <= pcm.len());
        // Someone else started talking before the sentence ended (no full stop, no pause): their first words
        // start a new caption, and what came before is committed as the previous speaker's.
        // A change is acted on only when two partials in a row put it in the same place (within half a second).
        let turn = match stable {
            None if self.speaker_commit => {
                let found = match self.segmenter.is_some() {
                    true => self.voice_change(&pcm, draft_from, &text, &word_ends),
                    false => self.speaker_change(&pcm, draft_from, &text, &word_ends),
                };
                let pending = self.pending_turn.take();
                match found {
                    Some((_, end)) if pending.is_some_and(|p| p.abs_diff(start + end) <= RATE / 2) => found,
                    Some((_, end)) => {
                        self.pending_turn = Some(start + end);
                        None
                    }
                    None => None,
                }
            }
            _ => None,
        };
        if let Some((b, end)) = stable.or(turn) {
            let (head, tail) = (text[..b].trim().to_string(), text[b..].trim().to_string());
            let head_words: Vec<_> = word_ends.iter().copied().filter(|&(o, _)| o <= b).collect();
            let audio = Audio { pcm: &pcm[..end], start, skip, word_ends: &head_words };
            self.finalize(head.clone(), t.lang.clone(), start_ms, ago(pcm.len() - end), audio, mt, emit)?;
            let cut = end.saturating_sub(RATE);
            if stable.is_none() {
                // A change of speaker: cut right there. Hearing a second again only works when the words are
                // transcribed the same both times, which mid-sentence they often were not (lost and doubled words).
                self.overlap = None;
                self.committed = start + end;
                self.pending_turn = None;
            } else if cut > 0 {
                // The committed words after the cut are heard again.
                let from = word_ends.iter().rfind(|&&(o, t)| t <= cut && o <= b).map_or(0, |&(o, _)| o);
                self.overlap = Some(text[from..b].trim().to_string());
                self.committed = start + cut;
            } else {
                // Within a second of the last cut: keep the audio; all of it is heard again.
                self.overlap = Some(match self.overlap.take() {
                    Some(o) => format!("{o} {head}"),
                    None => head,
                });
            }
            match stable {
                Some(_) => self.early_commits += 1,
                None => self.speaker_commits += 1,
            }
            draft_from = end;
            text = tail;
        }
        self.prev = Some(text.clone());
        // Not final yet: show a draft translation under the same id. The source is sent together with it,
        // because a caption without a translation would blank the previous draft in the overlay.
        // A draft of an unfinished fragment can come back empty or as just "…"; showing it would replace a readable one.
        if !has_words(&text) {
            return Ok(());
        }
        let translation = self.translate(&text, &t.lang, mt, &mut |_| {})?;
        if has_words(&translation) {
            let speaker = self.draft_speaker(&pcm[draft_from.min(pcm.len())..]);
            emit(Caption { id: self.next_id, stage: Stage::Partial, lang: t.lang, source: text, translation, speaker });
        }
        Ok(())
    }

    /// Where the speaker changes in a draft, as (byte offset in `text`, sample offset in `pcm`) of the last word
    /// before the change: the draft's start and its last seconds sound like different speakers, and 2 s windows
    /// every 0.5 s (labelled in order on a copy of the clusterer, so a voice heard for the first time gets an id of
    /// its own) end with at least 2 s of the new voice after at least 1.5 s of the old one. Short replies inside a
    /// turn are shorter than that and do not cut it.
    fn speaker_change(&mut self, pcm: &[f32], from: usize, text: &str, word_ends: &[(usize, usize)]) -> Option<(usize, usize)> {
        const WIN: usize = 2 * RATE;
        const HOP: usize = RATE / 2;
        const MIN_NEW: usize = 2 * RATE;
        const MIN_OLD: usize = 3 * RATE / 2;
        const SCAN: usize = 10 * RATE;
        let from = from.min(pcm.len());
        if pcm.len() - from < MIN_OLD + MIN_NEW || self.embedder.is_none() {
            return None;
        }
        let t0 = std::time::Instant::now();
        let embed = |s: &mut Self, a: usize, b: usize| s.embedder.as_mut().and_then(|e| e.embed(&pcm[a..b]).ok());
        // Cheap check first: the draft's opening and its end.
        let mut quick = self.speakers.clone();
        let (head, tail) = (embed(self, from, from + WIN), embed(self, pcm.len() - WIN, pcm.len()));
        let (a, b) = (quick.assign(head.as_deref(), 2.0).0, quick.assign(tail.as_deref(), 2.0).0);
        let found = if a == b || a.is_none() {
            None
        } else {
            let mut c = self.speakers.clone();
            let starts: Vec<usize> = (pcm.len().saturating_sub(SCAN).max(from)..=pcm.len() - WIN).step_by(HOP).collect();
            let labels: Vec<Option<u32>> = starts.iter().map(|&w| {
                let e = embed(self, w, w + WIN);
                c.assign(e.as_deref(), 2.0).0
            }).collect();
            let last = *labels.last()?;
            let run = labels.iter().rev().take_while(|&&l| l == last).count();
            let first_new = starts[labels.len() - run];
            let change = first_new + WIN / 2;
            let old_ok = labels.len() > run && change - from >= MIN_OLD;
            (old_ok && pcm.len() - change >= MIN_NEW && last != labels[labels.len() - run - 1])
                .then(|| word_ends.iter().copied().filter(|&(_, t)| t <= change + RATE / 5).last())
                .flatten()
                .filter(|&(o, _)| text[..o].trim().chars().count() >= MIN_SENTENCE_CHARS && has_words(&text[o..]))
        };
        self.embed_s += t0.elapsed().as_secs_f64();
        found
    }

    /// Like `speaker_change`, from the segmenter over the draft's last 10 s: the main voice changes to one that
    /// then talks at least 1.5 s, after at least 0.5 s of the previous one. The cut is the end of the last word
    /// that ends at or before the change.
    fn voice_change(&mut self, pcm: &[f32], from: usize, text: &str, word_ends: &[(usize, usize)]) -> Option<(usize, usize)> {
        const SCAN: usize = 10 * RATE;
        const MIN_NEW: usize = 3 * RATE / 2;
        const MIN_OLD: usize = RATE / 2;
        let from = from.min(pcm.len());
        if pcm.len() - from < MIN_OLD + MIN_NEW {
            return None;
        }
        let a = pcm.len().saturating_sub(SCAN).max(from);
        let t0 = std::time::Instant::now();
        let seg = self.segmenter.as_mut()?.segment(&pcm[a..]).map_err(|e| log::warn!("speaker segmentation: {e}")).ok();
        self.segment_s += t0.elapsed().as_secs_f64();
        let (frames, step) = seg?;
        let change = a + crate::speaker::voice_change(&frames, step, MIN_NEW, MIN_OLD)?;
        word_ends
            .iter()
            .copied()
            .filter(|&(_, t)| t <= change)
            .last()
            .filter(|&(o, _)| text[..o].trim().chars().count() >= MIN_SENTENCE_CHARS && has_words(&text[o..]))
    }

    /// Who is speaking in a draft, from its most recent audio (the draft follows the current speaker, and its last
    /// seconds are cheap to embed); nothing is learned from it. Too little audio keeps the last speaker.
    fn draft_speaker(&mut self, pcm: &[f32]) -> Option<u32> {
        const RECENT: usize = 6 * RATE;
        let embedder = self.embedder.as_mut()?;
        let pcm = &pcm[pcm.len().saturating_sub(RECENT)..];
        let dur = pcm.len() as f32 / RATE as f32;
        if dur < self.speakers.cfg.min_embed_s {
            return self.speakers.last();
        }
        let t0 = std::time::Instant::now();
        let emb = embedder.embed(pcm).ok();
        self.embed_s += t0.elapsed().as_secs_f64();
        self.speakers.peek(emb.as_deref(), dur)
    }

    /// The speaker of `audio.pcm[from..to]`; None without an embedder. A failed embedding is treated as too short.
    fn label(&mut self, audio: &Audio, from: usize, to: usize, text: &str) -> Option<u32> {
        let embedder = self.embedder.as_mut()?;
        let pcm = &audio.pcm[from..to];
        let dur = pcm.len() as f32 / RATE as f32;
        let emb = if dur >= self.speakers.cfg.min_embed_s {
            let t0 = std::time::Instant::now();
            let e = embedder.embed(pcm).map_err(|e| log::warn!("speaker embedding: {e}")).ok();
            self.embed_s += t0.elapsed().as_secs_f64();
            e
        } else {
            None
        };
        let (speaker, sim, decision) = self.speakers.assign(emb.as_deref(), dur);
        let embedding = emb.unwrap_or_default();
        self.segments.push(Segment { start: audio.start + from, end: audio.start + to, speaker, sim: Some(sim).filter(|s| s.is_finite()), decision, text: text.to_string(), embedding });
        speaker
    }

    /// Stream position (samples) up to which audio is committed; the endpointer can drop it.
    pub fn committed(&self) -> usize {
        self.committed
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
    /// Each sentence's audio, from the word times (or by length without them), labels its speaker.
    #[allow(clippy::too_many_arguments)]
    fn finalize(&mut self, text: String, lang: String, start_ms: u64, end_ms: u64, audio: Audio, mt: &mut dyn Translator, emit: &mut dyn FnMut(Caption)) -> Result<(), String> {
        let sentences = split_sentences(&punctuate(&text, &lang));
        let total: usize = sentences.iter().map(|s| s.chars().count()).sum::<usize>().max(1);
        let span = end_ms.saturating_sub(start_ms);
        let ends = sentence_ends(&text, &sentences);
        let n = audio.pcm.len();
        let mut seg_start = audio.skip.min(n);
        let mut done = 0;
        for (k, source) in sentences.into_iter().enumerate() {
            let seg_end = match ends.get(k) {
                _ if k + 1 == ends.len() => n,
                Some(&b) => audio.word_ends.iter().find(|&&(o, _)| o >= b).map_or(b * n / text.len().max(1), |&(_, t)| t),
                None => n,
            }
            .clamp(seg_start, n);
            let speaker = self.label(&audio, seg_start, seg_end, &source);
            seg_start = seg_end;
            let from = start_ms + span * done as u64 / total as u64;
            done += source.chars().count();
            let to = start_ms + span * done as u64 / total as u64;
            let mut cap = Caption { id: self.next_id, stage: Stage::Streaming, lang: lang.clone(), source, translation: String::new(), speaker };
            let mut acc = String::new();
            cap.translation = self.translate(&cap.source, &cap.lang, mt, &mut |piece| {
                acc.push_str(piece);
                emit(Caption { translation: acc.clone(), ..cap.clone() });
            })?;
            cap.stage = Stage::Final;
            emit(cap.clone());
            self.next_id += 1;
            self.entries.push(Entry { start_ms: from, end_ms: to, lang: cap.lang, source: cap.source.clone(), translation: cap.translation.clone(), speaker });
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
            // One word per second; Japanese has no spaces, so there four characters per second.
            let word_ends = if kana {
                text.char_indices().enumerate().map(|(n, (i, c))| (i + c.len_utf8(), (n + 1) * SEC / 4)).collect()
            } else {
                text.match_indices(' ').map(|(i, _)| i).chain(std::iter::once(text.len())).enumerate().map(|(n, o)| (o, (n + 1) * SEC)).collect()
            };
            Ok(Transcript { text, lang, word_ends })
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
        s.handle(Event::Final { pcm: vec![0.0; SEC], start: 0 }, a, mt, &mut |c| caps.push(c)).unwrap();
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
        s.handle(Event::Partial { pcm: vec![0.0; SEC], start: 0 }, &mut asr(vec!["Hello"]), &mut FakeMt(vec![]), &mut |c| caps.push(c)).unwrap();
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
            s.handle(Event::Partial { pcm: vec![0.0; SEC], start: 0 }, &mut a, &mut mt, &mut |c| caps.push(c)).unwrap();
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

    fn part(s: &mut Session, a: &mut FakeAsr, caps: &mut Vec<Caption>) {
        s.handle(Event::Partial { pcm: vec![0.0; 4 * SEC], start: 0 }, a, &mut FakeMt(vec![]), &mut |c| caps.push(c)).unwrap();
    }

    #[test]
    fn sentence_two_partials_agree_on_is_committed_and_its_audio_cut_a_second_early() {
        let mut s = Session::new(SessionConfig::default());
        let mut caps = vec![];
        // FakeAsr: one word per second. The sentence ends at 4 s, so the cut is at 3 s and "friend." is heard again.
        let mut a = asr(vec!["Hello there my friend. How", "Hello there, my friend. How are you", "friend. How are you doing today?"]);
        part(&mut s, &mut a, &mut caps);
        assert!(s.entries.is_empty(), "one partial alone commits nothing");
        part(&mut s, &mut a, &mut caps);
        assert_eq!(s.entries.iter().map(|e| e.source.as_str()).collect::<Vec<_>>(), ["Hello there, my friend."]);
        assert_eq!((s.committed(), s.overlap.as_deref(), s.early_commits), (3 * SEC, Some("friend."), 1));
        let last = caps.last().unwrap();
        assert!(last.stage == Stage::Partial && last.source == "How are you" && last.id == 1, "the rest is a new draft");
        s.handle(Event::Final { pcm: vec![0.0; 6 * SEC], start: 0 }, &mut a, &mut FakeMt(vec![]), &mut |c| caps.push(c)).unwrap();
        let sources: Vec<_> = s.entries.iter().map(|e| e.source.as_str()).collect();
        assert_eq!(sources, ["Hello there, my friend.", "How are you doing today?"], "the repeated word is dropped");
        assert_eq!(s.handle(Event::Final { pcm: vec![0.0; 2 * SEC], start: 0 }, &mut a, &mut FakeMt(vec![]), &mut |_| {}), Ok(()), "audio that is all committed is skipped");
    }

    #[test]
    fn japanese_sentence_is_cut_at_its_own_end_and_the_repeated_characters_are_dropped() {
        let mut s = Session::new(SessionConfig::default());
        let mut caps = vec![];
        // The sentence ends at 2.75 s (11 characters), so the cut is at 1.75 s and "います。" is heard again.
        let mut a = asr(vec!["ありがとうございます。まず", "ありがとうございます。まず、来月の", "います。まず、来月の納品です。"]);
        part(&mut s, &mut a, &mut caps);
        part(&mut s, &mut a, &mut caps);
        assert_eq!((s.committed(), s.overlap.as_deref()), (7 * SEC / 4, Some("います。")), "not at the end of the partial's audio (#78)");
        s.handle(Event::Final { pcm: vec![0.0; 6 * SEC], start: 0 }, &mut a, &mut FakeMt(vec![]), &mut |c| caps.push(c)).unwrap();
        let sources: Vec<_> = s.entries.iter().map(|e| e.source.as_str()).collect();
        assert_eq!(sources, ["ありがとうございます。", "まず、来月の納品です。"]);
    }

    #[test]
    fn early_commits_can_be_turned_off() {
        let mut s = Session::new(SessionConfig::default());
        s.early_commit = false;
        let mut caps = vec![];
        let mut a = asr(vec!["Hello there my friend. How", "Hello there, my friend. How are you"]);
        part(&mut s, &mut a, &mut caps);
        part(&mut s, &mut a, &mut caps);
        assert!(s.entries.is_empty() && s.committed() == 0);
    }

    #[test]
    fn overlap_matching_is_fuzzy_but_not_blind() {
        assert_eq!(strip_overlap("friend.", "Fred, how are you"), 5, "close enough");
        assert_eq!(strip_overlap("my friend.", "How are you"), 0, "not heard again");
        assert_eq!(sentence_bounds("Hi. U.S. then 3.5? Yes!"), vec![3, 8, 18, 23]);
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

    /// The voice is the audio's level: 0.1 is one speaker, 0.2 another (orthogonal embeddings).
    struct LevelVoice;
    impl Embedder for LevelVoice {
        fn embed(&mut self, pcm: &[f32]) -> Result<Vec<f32>, String> {
            let mean = pcm.iter().sum::<f32>() / pcm.len() as f32;
            Ok(if mean < 0.15 { vec![1.0, 0.0] } else { vec![0.0, 1.0] })
        }
    }

    fn voices(parts: &[(f32, usize)]) -> Vec<f32> {
        parts.iter().flat_map(|&(level, secs)| std::iter::repeat_n(level, secs * SEC)).collect()
    }

    #[test]
    fn each_sentence_of_a_final_gets_the_speaker_of_its_own_audio() {
        let mut s = Session::new(SessionConfig::default());
        s.embedder = Some(Box::new(LevelVoice));
        let mut caps = vec![];
        // One word per second: the first sentence is 0-4 s, the second 4-8 s.
        let mut a = asr(vec!["Hello there my friend. Yes I agree with that.", "And a third one here."]);
        let pcm = voices(&[(0.1, 4), (0.2, 5)]);
        s.handle(Event::Final { pcm, start: 0 }, &mut a, &mut FakeMt(vec![]), &mut |c| caps.push(c)).unwrap();
        let who: Vec<_> = s.entries.iter().map(|e| e.speaker).collect();
        assert_eq!(who, [Some(0), Some(1)]);
        assert_eq!(s.segments.iter().map(|g| (g.start, g.end)).collect::<Vec<_>>(), [(0, 4 * SEC), (4 * SEC, 9 * SEC)], "the last sentence runs to the end");
        assert!(caps.iter().filter(|c| c.stage == Stage::Final).map(|c| c.speaker).eq([Some(0), Some(1)]));
        // A later utterance by the first voice gets the first id back.
        s.handle(Event::Final { pcm: voices(&[(0.1, 5)]), start: 9 * SEC }, &mut a, &mut FakeMt(vec![]), &mut |c| caps.push(c)).unwrap();
        assert_eq!(s.entries[2].speaker, Some(0));
        assert_eq!(s.segments[2].start, 9 * SEC, "positions are in the stream");
    }

    #[test]
    fn without_an_embedder_there_are_no_speakers() {
        let mut s = Session::new(SessionConfig::default());
        let mut caps = vec![];
        s.handle(Event::Final { pcm: voices(&[(0.1, 4)]), start: 0 }, &mut asr(vec!["Hello there my friend."]), &mut FakeMt(vec![]), &mut |c| caps.push(c)).unwrap();
        assert!(caps.iter().all(|c| c.speaker.is_none()) && s.segments.is_empty());
        assert!(!serde_json::to_string(&caps[0]).unwrap().contains("speaker"), "the overlay payload is unchanged");
    }

    #[test]
    fn an_early_commit_is_labelled_from_its_own_audio_without_the_part_heard_again() {
        let mut s = Session::new(SessionConfig::default());
        s.embedder = Some(Box::new(LevelVoice));
        let mut caps = vec![];
        let mut a = asr(vec!["Hello there my friend. How", "Hello there, my friend. How are you", "friend. How are you doing today?"]);
        let pcm = voices(&[(0.1, 4), (0.2, 2)]);
        for _ in 0..2 {
            s.handle(Event::Partial { pcm: pcm.clone(), start: 0 }, &mut a, &mut FakeMt(vec![]), &mut |c| caps.push(c)).unwrap();
        }
        assert_eq!((s.entries[0].speaker, s.segments[0].end), (Some(0), 4 * SEC), "the head is the audio up to its last word");
        // The final starts at the cut (3 s); "friend." (to 1 s in) was the first speaker's and is left out.
        s.handle(Event::Final { pcm: voices(&[(0.1, 1), (0.2, 5)]), start: 3 * SEC }, &mut a, &mut FakeMt(vec![]), &mut |c| caps.push(c)).unwrap();
        assert_eq!((s.entries[1].speaker, s.segments[1].start), (Some(1), 4 * SEC));
    }

    #[test]
    fn sentence_ends_map_back_to_the_text() {
        let text = "Hello there.  Hi!  This is it";
        let ends = sentence_ends(text, &split_sentences(text));
        assert_eq!(ends.iter().map(|&e| &text[..e]).collect::<Vec<_>>(), ["Hello there.", "Hello there.  Hi!  This is it"], "a short sentence joins the next");
        let ja = "ありがとうございます。まず、来月の納品です。";
        assert_eq!(sentence_ends(ja, &split_sentences(ja)), [33, ja.len()]);
    }

    #[test]
    fn a_draft_follows_the_current_voice_without_learning_from_it() {
        let mut s = Session::new(SessionConfig::default());
        s.embedder = Some(Box::new(LevelVoice));
        let mut caps = vec![];
        let mut a = asr(vec!["Hello there my friend.", "So what do you", "So what do you think"]);
        s.handle(Event::Final { pcm: voices(&[(0.1, 4)]), start: 0 }, &mut a, &mut FakeMt(vec![]), &mut |c| caps.push(c)).unwrap();
        // A new voice talks: its draft gets a new id right away, before anything is committed.
        s.handle(Event::Partial { pcm: voices(&[(0.2, 4)]), start: 4 * SEC }, &mut a, &mut FakeMt(vec![]), &mut |c| caps.push(c)).unwrap();
        let last = caps.last().unwrap();
        assert_eq!((last.stage, last.speaker), (Stage::Partial, Some(1)));
        assert_eq!(s.speakers.speakers(), 1, "a draft does not create the speaker");
        // Under a second of draft audio keeps the last speaker.
        s.handle(Event::Partial { pcm: voices(&[(0.2, 4)])[..SEC / 2].to_vec(), start: 4 * SEC }, &mut a, &mut FakeMt(vec![]), &mut |c| caps.push(c)).unwrap();
        assert_eq!(caps.last().unwrap().speaker, Some(0));
    }

    #[test]
    fn a_new_voice_inside_a_sentence_commits_what_came_before() {
        let mut s = Session::new(SessionConfig::default());
        s.embedder = Some(Box::new(LevelVoice));
        s.speaker_commit = true;
        let mut caps = vec![];
        // One word per second; the first voice says five words, then the second interrupts. No full stop anywhere.
        let mut a = asr(vec!["Hello there.", "so I was going to say that we could maybe", "so I was going to say that we could maybe now"]);
        s.handle(Event::Final { pcm: voices(&[(0.1, 3)]), start: 0 }, &mut a, &mut FakeMt(vec![]), &mut |c| caps.push(c)).unwrap();
        s.handle(Event::Partial { pcm: voices(&[(0.1, 5), (0.2, 5)]), start: 3 * SEC }, &mut a, &mut FakeMt(vec![]), &mut |c| caps.push(c)).unwrap();
        assert_eq!(s.speaker_commits, 0, "one partial alone does not cut");
        s.handle(Event::Partial { pcm: voices(&[(0.1, 5), (0.2, 6)]), start: 3 * SEC }, &mut a, &mut FakeMt(vec![]), &mut |c| caps.push(c)).unwrap();
        assert_eq!(s.speaker_commits, 1, "the next one confirms the change");
        let committed: Vec<_> = s.entries.iter().map(|e| (e.source.as_str(), e.speaker)).collect();
        assert_eq!(committed, [("Hello there.", Some(0)), ("so I was going to.", Some(0))], "cut at the change, about 5 s in");
        let last = caps.last().unwrap();
        assert_eq!((last.stage, last.source.as_str(), last.speaker), (Stage::Partial, "say that we could maybe now", Some(1)));
        assert_eq!(s.committed(), 3 * SEC + 5 * SEC, "cut right at the change, nothing heard again");
    }

    /// Segmentation from the audio's level, like `LevelVoice`: 0.1 is local voice 0, 0.2 voice 1; 0.1 s frames.
    struct LevelSegmenter;
    impl Segmenter for LevelSegmenter {
        fn segment(&mut self, pcm: &[f32]) -> Result<(Vec<u8>, usize), String> {
            Ok((pcm.chunks(SEC / 10).map(|c| if c[0] == 0.0 { 0 } else if c[0] < 0.15 { 1 } else { 2 }).collect(), SEC / 10))
        }
    }

    #[test]
    fn the_segmenter_finds_the_change_and_two_partials_confirm_it() {
        let mut s = Session::new(SessionConfig::default());
        s.embedder = Some(Box::new(LevelVoice));
        s.segmenter = Some(Box::new(LevelSegmenter));
        s.speaker_commit = true;
        let mut caps = vec![];
        let mut a = asr(vec!["Hello there.", "so I was going to say that we could maybe", "so I was going to say that we could maybe now"]);
        s.handle(Event::Final { pcm: voices(&[(0.1, 3)]), start: 0 }, &mut a, &mut FakeMt(vec![]), &mut |c| caps.push(c)).unwrap();
        // The new voice talks 2 s (over the 1.5 s needed): found, but one partial alone does not cut.
        s.handle(Event::Partial { pcm: voices(&[(0.1, 5), (0.2, 2)]), start: 3 * SEC }, &mut a, &mut FakeMt(vec![]), &mut |c| caps.push(c)).unwrap();
        assert_eq!(s.speaker_commits, 0);
        s.handle(Event::Partial { pcm: voices(&[(0.1, 5), (0.2, 3)]), start: 3 * SEC }, &mut a, &mut FakeMt(vec![]), &mut |c| caps.push(c)).unwrap();
        assert_eq!(s.speaker_commits, 1);
        let committed: Vec<_> = s.entries.iter().map(|e| (e.source.as_str(), e.speaker)).collect();
        assert_eq!(committed, [("Hello there.", Some(0)), ("so I was going to.", Some(0))], "cut at the last word end before the change (5 s)");
        assert_eq!(s.committed(), 3 * SEC + 5 * SEC, "cut right at it, nothing heard again");
        assert!(s.segment_s > 0.0);
        let last = caps.last().unwrap();
        assert_eq!((last.source.as_str(), last.speaker), ("say that we could maybe now", Some(1)));
    }

    #[test]
    fn the_segmenter_ignores_a_short_reply() {
        let mut s = Session::new(SessionConfig::default());
        s.embedder = Some(Box::new(LevelVoice));
        s.segmenter = Some(Box::new(LevelSegmenter));
        s.speaker_commit = true;
        let mut a = asr(vec!["Hello there.", "so I was going to say that we could maybe", "so I was going to say that we could maybe"]);
        s.handle(Event::Final { pcm: voices(&[(0.1, 3)]), start: 0 }, &mut a, &mut FakeMt(vec![]), &mut |_| {}).unwrap();
        for _ in 0..2 {
            s.handle(Event::Partial { pcm: voices(&[(0.1, 8), (0.2, 1), (0.1, 1)]), start: 3 * SEC }, &mut a, &mut FakeMt(vec![]), &mut |_| {}).unwrap();
        }
        assert_eq!((s.speaker_commits, s.entries.len()), (0, 1));
    }

    #[test]
    fn a_short_reply_does_not_cut_a_turn() {
        let mut s = Session::new(SessionConfig::default());
        s.embedder = Some(Box::new(LevelVoice));
        s.speaker_commit = true;
        let mut a = asr(vec!["Hello there.", "so I was going to say that we could maybe", "so I was going to say that we could maybe"]);
        s.handle(Event::Final { pcm: voices(&[(0.1, 3)]), start: 0 }, &mut a, &mut FakeMt(vec![]), &mut |_| {}).unwrap();
        // One second of the other voice at the end, twice: under the 2 s needed.
        for _ in 0..2 {
            s.handle(Event::Partial { pcm: voices(&[(0.1, 9), (0.2, 1)]), start: 3 * SEC }, &mut a, &mut FakeMt(vec![]), &mut |_| {}).unwrap();
        }
        assert_eq!((s.speaker_commits, s.entries.len()), (0, 1));
    }
}
