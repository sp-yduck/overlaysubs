//! Scores the speaker labels of a benchmark report (`bench_report` with OVERLAYSUBS_SPK) against a reference RTTM,
//! and re-runs the online clusterer over the dumped embeddings with other settings, so thresholds can be tuned
//! without running Whisper again. Overlapped reference speech is not scored (out of scope for now).
//!
//! cargo run -p overlaysubs-core --example spk_eval -- <report.json> <ref.rttm> [offset_s] [collar_s]
//! offset_s: where the clip starts in the RTTM's recording.
use overlaysubs_core::speaker::{ClusterConfig, Clusterer, Segment};
use serde::Deserialize;

const FRAME_S: f64 = 0.01;
const RATE: f64 = 16_000.0;

#[derive(Deserialize)]
struct Report {
    clips: Vec<Clip>,
}
#[derive(Deserialize)]
struct Clip {
    name: String,
    audio_s: f64,
    #[serde(default)]
    segments: Vec<Segment>,
}

/// Per frame: the reference speakers.
fn reference(rttm: &str, offset: f64, frames: usize) -> (Vec<Vec<usize>>, Vec<String>) {
    let mut names: Vec<String> = vec![];
    let mut out = vec![vec![]; frames];
    for l in rttm.lines() {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.first() != Some(&"SPEAKER") || f.len() < 8 {
            continue;
        }
        let (start, dur): (f64, f64) = (f[3].parse().unwrap(), f[4].parse().unwrap());
        let id = names.iter().position(|n| n == f[7]).unwrap_or_else(|| {
            names.push(f[7].to_string());
            names.len() - 1
        });
        let (a, b) = (((start - offset) / FRAME_S).round(), ((start + dur - offset) / FRAME_S).round());
        for fr in a.max(0.0) as usize..(b.max(0.0) as usize).min(frames) {
            if !out[fr].contains(&id) {
                out[fr].push(id);
            }
        }
    }
    (out, names)
}

/// Frames within `collar` of a reference boundary, or with overlapped speech, are not scored.
fn scored(r: &[Vec<usize>], collar: f64) -> Vec<bool> {
    let c = (collar / FRAME_S).round() as usize;
    let mut ok: Vec<bool> = r.iter().map(|s| s.len() <= 1).collect();
    for i in 1..r.len() {
        if r[i] != r[i - 1] {
            for fr in i.saturating_sub(c)..(i + c).min(r.len()) {
                ok[fr] = false;
            }
        }
    }
    ok
}

/// Per frame: the hypothesis speaker.
fn hypothesis(segs: &[Segment], labels: &[Option<u32>], frames: usize) -> Vec<Option<u32>> {
    let mut out = vec![None; frames];
    for (g, l) in segs.iter().zip(labels) {
        let (a, b) = ((g.start as f64 / RATE / FRAME_S) as usize, ((g.end as f64 / RATE / FRAME_S) as usize).min(frames));
        for o in out.iter_mut().take(b).skip(a) {
            *o = *l;
        }
    }
    out
}

/// The mapping from reference to hypothesis speakers with the most overlap (exhaustive over up to 8 hypothesis speakers).
fn best_map(overlap: &[Vec<usize>], n_hyp: usize) -> Vec<Option<usize>> {
    let n_ref = overlap.len();
    let n_hyp = n_hyp.min(12);
    // dp over references with the set of hypothesis speakers used.
    let states = 1usize << n_hyp;
    let mut dp = vec![vec![None::<(usize, Vec<Option<usize>>)>; states]; n_ref + 1];
    dp[0][0] = Some((0, vec![]));
    for r in 0..n_ref {
        for m in 0..states {
            let Some((v, map)) = dp[r][m].clone() else { continue };
            let mut try_set = |m2: usize, v2: usize, h: Option<usize>| {
                if dp[r + 1][m2].as_ref().is_none_or(|(b, _)| v2 > *b) {
                    let mut mm = map.clone();
                    mm.push(h);
                    dp[r + 1][m2] = Some((v2, mm));
                }
            };
            try_set(m, v, None);
            for h in 0..n_hyp {
                if m & (1 << h) == 0 {
                    try_set(m | (1 << h), v + overlap[r].get(h).copied().unwrap_or(0), Some(h));
                }
            }
        }
    }
    dp[n_ref].iter().flatten().max_by_key(|(v, _)| *v).map(|(_, m)| m.clone()).unwrap_or_default()
}

struct Score {
    der: f64,
    miss: f64,
    fa: f64,
    conf: f64,
    /// Confusion over frames where both speak: the clustering error alone.
    spk_err: f64,
    /// Sentences whose speaker is the (mapped) reference majority in their span.
    caption_acc: f64,
    n_hyp: usize,
}

fn score(r: &[Vec<usize>], ok: &[bool], n_ref: usize, segs: &[Segment], labels: &[Option<u32>]) -> Score {
    let h = hypothesis(segs, labels, r.len());
    let mut ids: Vec<u32> = labels.iter().flatten().copied().collect();
    ids.sort();
    ids.dedup();
    let hi = |l: u32| ids.iter().position(|&x| x == l).unwrap();
    let mut overlap = vec![vec![0usize; ids.len()]; n_ref];
    for i in 0..r.len() {
        if let (true, [rs], Some(l)) = (ok[i], r[i].as_slice(), h[i]) {
            overlap[*rs][hi(l)] += 1;
        }
    }
    let map = best_map(&overlap, ids.len());
    let (mut total, mut miss, mut fa, mut conf, mut both) = (0, 0, 0, 0, 0);
    for i in 0..r.len() {
        if !ok[i] {
            continue;
        }
        match (r[i].first(), h[i]) {
            (Some(&rs), Some(l)) => {
                total += 1;
                both += 1;
                if map.get(rs).copied().flatten() != Some(hi(l)) {
                    conf += 1;
                }
            }
            (Some(_), None) => {
                total += 1;
                miss += 1;
            }
            (None, Some(_)) => fa += 1,
            (None, None) => {}
        }
    }
    // Caption level.
    let (mut good, mut n) = (0, 0);
    for (g, l) in segs.iter().zip(labels) {
        let (a, b) = ((g.start as f64 / RATE / FRAME_S) as usize, ((g.end as f64 / RATE / FRAME_S) as usize).min(r.len()));
        let mut count = vec![0usize; n_ref];
        for fr in r.iter().take(b).skip(a) {
            if let [rs] = fr.as_slice() {
                count[*rs] += 1;
            }
        }
        let Some((major, &c)) = count.iter().enumerate().max_by_key(|(_, c)| **c) else { continue };
        if c == 0 {
            continue;
        }
        n += 1;
        if l.is_some_and(|l| map.get(major).copied().flatten() == Some(hi(l))) {
            good += 1;
        }
    }
    let t = total.max(1) as f64;
    Score { der: (miss + fa + conf) as f64 / t, miss: miss as f64 / t, fa: fa as f64 / t, conf: conf as f64 / t, spk_err: conf as f64 / both.max(1) as f64, caption_acc: good as f64 / n.max(1) as f64, n_hyp: ids.len() }
}

fn online(segs: &[Segment], cfg: ClusterConfig) -> Vec<Option<u32>> {
    let mut c = Clusterer::new(cfg);
    let raw: Vec<_> = segs.iter().map(|g| c.assign(Some(&g.embedding).filter(|e| !e.is_empty()).map(|e| e.as_slice()), (g.end - g.start) as f32 / RATE as f32).0).collect();
    // Merges happen later: score with the ids as they end up, and as they were shown.
    raw.into_iter().map(|l| l.map(|l| c.resolve(l))).collect()
}

fn online_as_shown(segs: &[Segment], cfg: ClusterConfig) -> Vec<Option<u32>> {
    let mut c = Clusterer::new(cfg);
    segs.iter().map(|g| c.assign(Some(&g.embedding).filter(|e| !e.is_empty()).map(|e| e.as_slice()), (g.end - g.start) as f32 / RATE as f32).0).collect()
}

fn norm(v: &[f32]) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    v.iter().map(|x| x / n).collect()
}

/// Offline average-linkage clustering of every embedded segment (the whole clip known in advance): what the
/// embeddings allow at best. Segments without an embedding take the previous label.
fn offline(segs: &[Segment], threshold: f32) -> Vec<Option<u32>> {
    let idx: Vec<usize> = (0..segs.len()).filter(|&i| !segs[i].embedding.is_empty()).collect();
    let e: Vec<Vec<f32>> = idx.iter().map(|&i| norm(&segs[i].embedding)).collect();
    let sim = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
    let mut clusters: Vec<Vec<usize>> = (0..e.len()).map(|i| vec![i]).collect();
    loop {
        let mut best = (f32::MIN, 0, 0);
        for a in 0..clusters.len() {
            for b in a + 1..clusters.len() {
                let s: f32 = clusters[a].iter().flat_map(|&i| clusters[b].iter().map(move |&j| (i, j))).map(|(i, j)| sim(&e[i], &e[j])).sum::<f32>() / (clusters[a].len() * clusters[b].len()) as f32;
                if s > best.0 {
                    best = (s, a, b);
                }
            }
        }
        if best.0 < threshold || clusters.len() < 2 {
            break;
        }
        let b = clusters.remove(best.2);
        clusters[best.1].extend(b);
    }
    let mut label = vec![None; segs.len()];
    for (c, members) in clusters.iter().enumerate() {
        for &m in members {
            label[idx[m]] = Some(c as u32);
        }
    }
    let mut last = None;
    for l in label.iter_mut() {
        match l {
            Some(x) => last = Some(*x),
            None => *l = last,
        }
    }
    label
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    assert!(a.len() >= 3, "usage: spk_eval <report.json> <ref.rttm> [offset_s] [collar_s]");
    let report: Report = serde_json::from_str(&std::fs::read_to_string(&a[1]).unwrap()).unwrap();
    let rttm = std::fs::read_to_string(&a[2]).unwrap();
    let offset: f64 = a.get(3).map_or(0.0, |s| s.parse().unwrap());
    let collar: f64 = a.get(4).map_or(0.25, |s| s.parse().unwrap());
    let pct = |x: f64| format!("{:5.1}%", x * 100.0);
    for clip in &report.clips {
        let frames = (clip.audio_s / FRAME_S) as usize;
        let (r, names) = reference(&rttm, offset, frames);
        let ok = scored(&r, collar);
        let n_ref = names.iter().enumerate().filter(|(i, _)| r.iter().any(|f| f.contains(i))).count();
        let segs = &clip.segments;
        let embedded = segs.iter().filter(|g| !g.embedding.is_empty()).count();
        println!("{}: {} sentences ({} embedded), {} reference speakers, collar {collar} s, overlap excluded", clip.name, segs.len(), embedded, n_ref);
        let line = |name: &str, s: &Score| println!("  {name:<34} DER {} (miss {} fa {} conf {})  spk err {}  caption acc {}  speakers {}", pct(s.der), pct(s.miss), pct(s.fa), pct(s.conf), pct(s.spk_err), pct(s.caption_acc), s.n_hyp);
        let as_run: Vec<_> = segs.iter().map(|g| g.speaker).collect();
        line("as run (labels as shown)", &score(&r, &ok, names.len(), segs, &as_run));
        let mut grid = vec![];
        for assign in [0.3f32, 0.35, 0.4, 0.45, 0.5, 0.55, 0.6, 0.65, 0.7] {
            for gap in [0.05f32, 0.1, 0.15, 0.2] {
                for stick in [0.0f32, 0.05, 0.1] {
                    for merge in [0.7f32, 0.8, 0.9] {
                        let cfg = ClusterConfig { assign, new_speaker: assign - gap, stick, merge, ..Default::default() };
                        grid.push((cfg, score(&r, &ok, names.len(), segs, &online_as_shown(segs, cfg))));
                    }
                }
            }
        }
        grid.sort_by(|a, b| a.1.der.total_cmp(&b.1.der));
        let d = ClusterConfig::default();
        line("online, default settings", &score(&r, &ok, names.len(), segs, &online_as_shown(segs, d)));
        line("online, default, merges applied", &score(&r, &ok, names.len(), segs, &online(segs, d)));
        println!("  best online settings:");
        for (cfg, s) in grid.iter().take(5) {
            line(&format!("assign {:.2} new {:.2} stick {:.2} merge {:.1}", cfg.assign, cfg.new_speaker, cfg.stick, cfg.merge), s);
        }
        let mut best_off = None::<(f32, Score)>;
        for t in [0.1f32, 0.15, 0.2, 0.25, 0.3, 0.35, 0.4, 0.45, 0.5, 0.55, 0.6] {
            let s = score(&r, &ok, names.len(), segs, &offline(segs, t));
            if best_off.as_ref().is_none_or(|(_, b)| s.der < b.der) {
                best_off = Some((t, s));
            }
        }
        if let Some((t, s)) = best_off {
            line(&format!("offline upper bound (AHC {t:.2})"), &s);
        }
    }
}
