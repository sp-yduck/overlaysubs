//! Side-by-side check of caption segmentation: the same clip with pause commits off and on.
//! Run: OVERLAYSUBS_WAV=clip.wav [OVERLAYSUBS_GPU=1] [OVERLAYSUBS_MODES=off,stable,overlap] \
//!   cargo test --release -p overlaysubs [--features vulkan] segmentation_compare -- --ignored --nocapture
//! It replays the app's loop on a virtual clock: audio arrives in real time, the session takes what is
//! queued when it is free (dropping stale partials), and its commits reach the endpointer when it finishes.
use crate::engine::{Plamo, Silero, Whisper};
use overlaysubs_core::endpoint::{EndpointConfig, Endpointer, Event};
use overlaysubs_core::metrics::cer;
use overlaysubs_core::registry::Registry;
use overlaysubs_core::session::{Caption, Early, Session, SessionConfig, Stage};
use std::collections::VecDeque;
use std::time::Instant;

fn wav(path: &str) -> Vec<f32> {
    let wav = std::fs::read(path).unwrap();
    let data = wav.windows(4).position(|w| w == b"data").unwrap() + 8;
    wav[data..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect()
}

#[derive(serde::Serialize)]
struct Commit {
    /// Virtual seconds when the last caption of this commit was done.
    at: f64,
    early: bool,
    sentences: Vec<(String, String)>,
    /// Seconds from the end of each sentence's audio (history timestamp) to `at`.
    lags: Vec<f64>,
}

#[derive(serde::Serialize)]
struct Run {
    mode: String,
    pause_ms: usize,
    load: f64,
    commits: Vec<Commit>,
    /// Longest draft each time a draft was replaced by a commit, in chars of translation.
    drafts_before_commit: Vec<usize>,
    source: String,
    /// Seconds where the endpointer ended an utterance (end of its audio).
    final_ends: Vec<f64>,
    /// Seconds of each pause and what became of it.
    pauses: Vec<(f64, String)>,
    /// The longest draft: (source, translation).
    longest: (String, String),
}

fn pct(v: &mut [f64], q: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v.get(((v.len() as f64 - 1.0) * q).round() as usize).copied().unwrap_or(0.0)
}

/// Modes: "off" (today), "pause<ms>" (commit at a pause), "stable" (commit what two partials agree on,
/// cut at its last word), "overlap" (the same, cut a second early and the repeat removed).
fn run(pcm: &[f32], mode: &str, vad_path: &str, asr: &mut Whisper, mt: &mut Plamo) -> Run {
    let (early, pause_ms) = match mode {
        "off" => (Early::Off, 0),
        "stable" => (Early::Stable, 0),
        "overlap" => (Early::Overlap, 0),
        m => (Early::Pause, m.strip_prefix("pause").and_then(|v| v.parse().ok()).expect("mode: off, stable or pause<ms>")),
    };
    let mut vad = Silero::load(vad_path).unwrap();
    let mut ep = Endpointer::new(EndpointConfig { pause_ms, ..Default::default() });
    let mut s = Session::new(SessionConfig::default());
    s.early = early;
    let mut longest = (String::new(), String::new());
    let (mut out, mut queue) = (vec![], VecDeque::<(f64, Event, usize)>::new());
    let (mut busy_until, mut busy, mut seen, mut pos) = (0.0f64, 0.0f64, 0usize, 0usize);
    let mut pending_commit: Option<(f64, usize)> = None;
    let mut commits = vec![];
    let mut drafts_before_commit = vec![];
    let mut longest_draft = 0;
    let (mut final_ends, mut pauses) = (vec![], vec![]);
    let silence = vec![0.0; 16_000];
    let chunks: Vec<&[f32]> = pcm.chunks(160).chain(std::iter::once(&silence[..])).collect();
    let n_chunks = chunks.len();
    for (ci, chunk) in chunks.into_iter().enumerate() {
        pos += chunk.len();
        let t = pos as f64 / 16_000.0;
        if let Some((at, c)) = pending_commit {
            if at <= t {
                ep.commit(c);
                pending_commit = None;
            }
        }
        ep.push(chunk, &mut vad, &mut out).unwrap();
        queue.extend(out.drain(..).map(|e| (t, e, pos)));
        let last = ci + 1 == n_chunks;
        while !queue.is_empty() && (busy_until <= t || last) {
            let mut now = busy_until.max(queue[0].0);
            let mut batch = vec![];
            while queue.front().is_some_and(|e| e.0 <= now) {
                batch.extend(queue.pop_front());
            }
            let n = batch.len();
            for (i, (_, ev, epos)) in batch.into_iter().enumerate() {
                if !ev.is_final() && i != n - 1 {
                    if let Event::Partial { pause: Some(cut), .. } = ev {
                        pauses.push((cut as f64 / 16_000.0, "dropped while the engines were busy".to_string()));
                    }
                    continue;
                }
                if let Event::Final { pcm, start } = &ev {
                    final_ends.push((start + pcm.len()) as f64 / 16_000.0);
                }
                s.tick(epos - seen);
                seen = epos;
                let (entries0, early0) = (s.entries.len(), s.early_commits);
                let t0 = Instant::now();
                let mut caps: Vec<Caption> = vec![];
                s.handle(ev, asr, mt, &mut |c| caps.push(c)).unwrap();
                let took = t0.elapsed().as_secs_f64();
                now += took;
                busy += took;
                if s.entries.len() > entries0 {
                    let new = &s.entries[entries0..];
                    commits.push(Commit {
                        at: now,
                        early: s.early_commits > early0,
                        sentences: new.iter().map(|e| (e.source.clone(), e.translation.clone())).collect(),
                        lags: new.iter().map(|e| now - e.end_ms as f64 / 1000.0).collect(),
                    });
                    drafts_before_commit.push(longest_draft);
                    longest_draft = 0;
                }
                for c in caps.iter().filter(|c| c.stage == Stage::Partial) {
                    longest_draft = longest_draft.max(c.translation.chars().count());
                    if c.translation.chars().count() > longest.1.chars().count() {
                        longest = (c.source.clone(), c.translation.clone());
                    }
                }
            }
            busy_until = now;
            pending_commit = Some((now, s.committed()));
        }
    }
    let source = s.entries.iter().map(|e| e.source.as_str()).collect::<Vec<_>>().join(" ");
    pauses.extend(s.pause_log.iter().map(|&(c, why)| (c as f64 / 16_000.0, why.to_string())));
    Run { mode: mode.to_string(), pause_ms, longest, load: busy / (pcm.len() as f64 / 16_000.0), commits, drafts_before_commit, source, final_ends, pauses }
}

/// Sentence ends over the whole clip (seconds, sentence): one Whisper pass with token timestamps.
fn sentence_ends(asr: &Whisper, pcm: &[f32], lang: &str) -> Vec<(f64, String)> {
    use whisper_rs::{FullParams, SamplingStrategy};
    let mut st = asr.ctx.create_state().unwrap();
    let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    p.set_n_threads(std::thread::available_parallelism().unwrap().get().min(8) as i32);
    p.set_language(Some(lang));
    p.set_token_timestamps(true);
    p.set_print_progress(false);
    st.full(p, pcm).unwrap();
    let (mut out, mut cur) = (vec![], String::new());
    for seg in st.as_iter() {
        for i in 0..seg.n_tokens() {
            let tok = seg.get_token(i).unwrap();
            let text = tok.to_str_lossy().map(|t| t.into_owned()).unwrap_or_default();
            if text.starts_with("[_") || text.starts_with("<|") {
                continue;
            }
            cur.push_str(&text);
            if text.trim_end().ends_with(['.', '?', '!']) {
                out.push((tok.token_data().t1 as f64 / 100.0, cur.trim().to_string()));
                cur.clear();
            }
        }
    }
    out
}

/// Longest run of non-speech (below Silero's continue threshold) near `t`, in ms.
fn gap_ms(probs: &[f32], t: f64) -> usize {
    let frame = 512.0 / 16_000.0;
    let (a, b) = (((t - 0.3) / frame).max(0.0) as usize, (((t + 0.6) / frame) as usize).min(probs.len()));
    let (mut best, mut run) = (0, 0);
    for &p in &probs[a.min(b)..b] {
        run = if p < 0.35 { run + 1 } else { 0 };
        best = best.max(run);
    }
    (best as f64 * frame * 1000.0) as usize
}

#[test]
#[ignore]
fn segmentation_compare() {
    let env = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.into());
    let reg = Registry::parse(include_str!("../../models.json")).unwrap();
    let dir = std::env::temp_dir().join("overlaysubs-models");
    let ids = [env("OVERLAYSUBS_ASR", "whisper-base"), env("OVERLAYSUBS_MT", "plamo-3-translate-0.8b-q4_k_m"), "silero-vad-v6.2.0".into()];
    for id in &ids {
        crate::download::fetch(reg.get(id).unwrap(), &dir, &mut |_, _| {}).unwrap();
    }
    let p = |i: usize| dir.join(&reg.get(&ids[i]).unwrap().file);
    let gpu = env("OVERLAYSUBS_GPU", "0") == "1";
    let threads = std::thread::available_parallelism().unwrap().get();
    let mut asr = Whisper::load(&p(0), &p(2), gpu, threads).unwrap();
    let mut mt = Plamo::load(&p(1), if gpu { 999 } else { 0 }, threads).unwrap();
    let pcm = wav(&env("OVERLAYSUBS_WAV", ""));
    let vad_path = asr.vad_path.clone();
    let runs: Vec<Run> = env("OVERLAYSUBS_MODES", "off,stable,overlap").split(',').map(|m| run(&pcm, m.trim(), &vad_path, &mut asr, &mut mt)).collect();

    for r in &runs {
        let mut sizes: Vec<f64> = r.commits.iter().map(|c| c.sentences.len() as f64).collect();
        let mut lags: Vec<f64> = r.commits.iter().flat_map(|c| c.lags.clone()).collect();
        let mut drafts: Vec<f64> = r.drafts_before_commit.iter().map(|&d| d as f64).collect();
        let sentences: usize = r.commits.iter().map(|c| c.sentences.len()).sum();
        // A commit followed by one that starts in lower case most likely cut a sentence.
        let lower_starts = r.commits.iter().skip(1).filter(|c| c.sentences[0].0.starts_with(|ch: char| ch.is_lowercase())).count();
        eprintln!("\n=== {} ({}) ===", r.mode, if gpu { "GPU" } else { "CPU" });
        eprintln!("  load x realtime            {:.2}", r.load);
        eprintln!("  commits / sentences        {} / {} ({} early)", r.commits.len(), sentences, r.commits.iter().filter(|c| c.early).count());
        eprintln!("  sentences per commit       p50 {:.0}  p90 {:.0}  max {:.0}", pct(&mut sizes, 0.5), pct(&mut sizes, 0.9), pct(&mut sizes, 1.0));
        eprintln!("  draft chars before commit  p50 {:.0}  p90 {:.0}  max {:.0}", pct(&mut drafts, 0.5), pct(&mut drafts, 0.9), pct(&mut drafts, 1.0));
        eprintln!("  sentence end -> shown s    p50 {:.2}  p90 {:.2}  max {:.2}", pct(&mut lags, 0.5), pct(&mut lags, 0.9), pct(&mut lags, 1.0));
        eprintln!("  commits starting lowercase {lower_starts}");
        eprintln!("  longest draft              {}\n      -> {}", r.longest.0, r.longest.1);
        for c in &r.commits {
            eprintln!("  [{:6.2}s{}]", c.at, if c.early { " early" } else { "" });
            for (src, tr) in &c.sentences {
                eprintln!("      {src}\n      -> {tr}");
            }
        }
    }
    // Ground truth: sentence ends that the endpointer (pauses off) did not end an utterance at.
    // Those are the places a caption is held today and an early commit would help.
    let ends = sentence_ends(&asr, &pcm, &env("OVERLAYSUBS_LANG", "en"));
    let mut vad = Silero::load(&vad_path).unwrap();
    let probs = overlaysubs_core::endpoint::Vad::probs(&mut vad, &pcm[..pcm.len() / 512 * 512]).unwrap();
    let base = &runs[0];
    let near = |xs: &[f64], t: f64| xs.iter().any(|x| (x - t).abs() < 0.8);
    let opportunities: Vec<&(f64, String)> = ends.iter().filter(|(t, _)| !near(&base.final_ends, *t)).collect();
    eprintln!("\n=== where an early commit was due ===");
    eprintln!("  sentence ends {}, at an utterance end already {}, inside an utterance (due) {}", ends.len(), ends.len() - opportunities.len(), opportunities.len());
    let buckets = [(0, 100), (100, 250), (250, 600), (600, usize::MAX)];
    let gaps: Vec<usize> = opportunities.iter().map(|(t, _)| gap_ms(&probs, *t)).collect();
    eprintln!("  pause at those ends: {}", buckets.iter().map(|&(a, b)| format!("{a}-{}ms: {}", if b == usize::MAX { "".into() } else { b.to_string() }, gaps.iter().filter(|&&g| g >= a && g < b).count())).collect::<Vec<_>>().join(", "));
    let reference = ends.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>().join(" ");
    for r in &runs {
        eprintln!("  transcript CER vs one offline pass over the clip, {}: {:.1}%", r.mode, cer(&reference, &r.source) * 100.0);
    }
    for r in runs.iter().filter(|r| r.mode != "off") {
        let mut tally = std::collections::BTreeMap::<String, usize>::new();
        // Matched by text: a due sentence is caught when an early commit shows a sentence ending the same way.
        // Time matching miscounted, as Whisper's word times are rough and the offline pass joins sentences.
        let tail = |x: &str| -> String {
            let v: Vec<char> = x.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
            v[v.len().saturating_sub(30)..].iter().collect()
        };
        let early: Vec<&String> = r.commits.iter().filter(|c| c.early).flat_map(|c| c.sentences.iter().map(|(src, _)| src)).collect();
        let same_end = |a: &str, b: &str| cer(&tail(a), &tail(b)) <= 0.3;
        let heads: Vec<&String> = r.commits.iter().filter(|c| c.early).filter_map(|c| c.sentences.last().map(|(src, _)| src)).collect();
        let unmatched = heads.iter().filter(|h| !ends.iter().any(|(_, e)| same_end(h, e))).count();
        eprintln!("\n  {}: early commits {}, last sentence not matching any offline sentence end {} (rough: the offline pass joins some sentences)", r.mode, heads.len(), unmatched);
        for ((t, sentence), gap) in opportunities.iter().zip(&gaps) {
            let text_caught = early.iter().any(|e| same_end(e, sentence));
            let why = match r.pauses.iter().filter(|(c, _)| (c - t).abs() < 0.8).min_by(|a, b| (a.0 - t).abs().total_cmp(&(b.0 - t).abs())) {
                _ if text_caught => "committed".to_string(),
                _ if r.pause_ms == 0 => "not committed".to_string(),
                Some((_, why)) if why != "committed" => why.clone(),
                _ if *gap < r.pause_ms => format!("pause shorter than {} ms", r.pause_ms),
                _ => format!("no pause event (utterance under {} ms?)", EndpointConfig::default().pause_after_ms),
            };
            eprintln!("    {t:7.2}s gap {gap:4}ms  {why:<36} {sentence}");
            *tally.entry(why).or_default() += 1;
        }
        eprintln!("    caught {} of {}", tally.get("committed").copied().unwrap_or(0), opportunities.len());
        for (why, n) in &tally {
            eprintln!("    {n:3}  {why}");
        }
    }
    for r in &runs[1..] {
        eprintln!("\ntranscript CER, {} vs {}: {:.1}%", runs[0].mode, r.mode, cer(&runs[0].source, &r.source) * 100.0);
    }
    if let Ok(out) = std::env::var("OVERLAYSUBS_OUT") {
        std::fs::write(out, serde_json::to_string_pretty(&runs).unwrap()).unwrap();
    }
}
