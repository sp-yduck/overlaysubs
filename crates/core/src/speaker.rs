//! Who is speaking: each committed sentence's audio gets a speaker embedding, and an online clusterer turns the
//! embeddings into stable speaker numbers. Overlapping speech (backchannels over someone else) is not handled.
use serde::{Deserialize, Serialize};

pub trait Embedder: Send {
    /// A fixed-size voice fingerprint of 16 kHz mono audio; need not be normalized.
    fn embed(&mut self, pcm: &[f32]) -> Result<Vec<f32>, String>;
}

/// Who talks when, inside a short stretch of audio: a speaker segmentation model (pyannote segmentation-3.0)
/// telling up to three local voices apart frame by frame. "Local": ids hold within one call only.
pub trait Segmenter: Send {
    /// One bit mask per frame (bit k: local voice k talks), and the frame step in samples. Frame i covers
    /// about `i * step .. (i + 1) * step`.
    fn segment(&mut self, pcm: &[f32]) -> Result<(Vec<u8>, usize), String>;
}

/// Where the main voice changes in `frames` (from `Segmenter::segment`, the audio starting at sample 0): the
/// sample where a voice that then talks at least `min_new` samples takes over from one that talked at least
/// `min_old` samples before it. Frames with no voice or two voices are skipped; gaps up to 0.3 s inside one
/// voice's run are bridged. The last change wins.
pub fn voice_change(frames: &[u8], step: usize, min_new: usize, min_old: usize) -> Option<usize> {
    const RATE: usize = 16_000;
    let bridge = RATE * 3 / 10;
    // (start, end, voice) runs of frames with exactly one voice.
    let mut runs: Vec<(usize, usize, u8)> = vec![];
    for (i, &m) in frames.iter().enumerate() {
        if m.count_ones() != 1 {
            continue;
        }
        let (a, b) = (i * step, (i + 1) * step);
        match runs.last_mut() {
            Some(r) if r.2 == m && a <= r.1 + bridge => r.1 = b,
            _ => runs.push((a, b, m)),
        }
    }
    let mut found = None;
    for j in 1..runs.len() {
        let (new, old) = (runs[j], runs[j - 1]);
        if new.2 == old.2 || new.1 - new.0 < min_new {
            continue;
        }
        // The previous voice's own talking before the change, gaps and all, back to another voice's long run.
        let stretch: Vec<_> = runs[..j].iter().rev().take_while(|r| r.2 == old.2 || r.1 - r.0 < min_new).collect();
        let talked: usize = stretch.iter().filter(|r| r.2 == old.2).map(|r| r.1 - r.0).sum();
        // A short interjection between two runs of the same voice is not a change of turn, either way.
        let before = runs[..j - stretch.len()].last();
        let interjection = talked < min_new && before.is_some_and(|b| b.2 == new.2);
        if talked >= min_old && !interjection {
            found = Some(new.0);
        }
    }
    found
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct ClusterConfig {
    /// Cosine similarity to a speaker at or above which a segment is theirs and refines their voice.
    pub assign: f32,
    /// Below this to every speaker, a long enough segment starts a new speaker. Between the two it goes to the
    /// nearest speaker without changing their voice, so an unclear segment cannot drag a centroid away.
    pub new_speaker: f32,
    /// Audio shorter than this is not embedded (too little voice to tell); it keeps the previous speaker.
    pub min_embed_s: f32,
    /// Audio shorter than this never starts a speaker or refines one.
    pub min_update_s: f32,
    pub max_speakers: usize,
    /// Added to the previous speaker's similarity: turns usually last several sentences.
    pub stick: f32,
    /// Two speakers this similar are merged into the older one.
    pub merge: f32,
    /// A segment's weight in its speaker's centroid is its length, capped here, so one long segment does not dominate.
    pub max_weight_s: f32,
    /// Share (0-1) of the session's mean voice taken off every embedding before comparing, once `center_after`
    /// segments are in: what every voice of a recording shares is mostly the room, microphone or channel. 0 is off.
    pub center: f32,
    pub center_after: usize,
    /// Thresholds follow the recording: `assign` and `new_speaker` become this much below the running mean
    /// similarity of segments to their own speaker (which starts at `within_prior`).
    pub adaptive: bool,
    pub adapt_assign_gap: f32,
    pub adapt_new_gap: f32,
    pub within_prior: f32,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self { assign: 0.5, new_speaker: 0.35, min_embed_s: 1.0, min_update_s: 2.0, max_speakers: 6, stick: 0.05, merge: 0.75, max_weight_s: 10.0, center: 0.0, center_after: 5, adaptive: false, adapt_assign_gap: 0.2, adapt_new_gap: 0.3, within_prior: 0.7 }
    }
}

impl ClusterConfig {
    /// The PoC's pick (AMI meetings plus noise and telephone-band copies, leave-one-out): half the session's mean
    /// voice off, then fixed thresholds. Plain fixed thresholds fell apart on audio unlike what they were tuned on.
    pub fn recommended() -> Self {
        Self { assign: 0.20, new_speaker: 0.15, stick: 0.0, merge: 0.8, center: 0.5, center_after: 5, ..Self::default() }
    }
}

fn normalized(v: &[f32]) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    v.iter().map(|x| x / n).collect()
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[derive(Debug, Clone)]
struct Speaker {
    id: u32,
    /// Weighted sum of normalized embeddings; its direction is the speaker's voice.
    sum: Vec<f32>,
    weight: f32,
}

impl Speaker {
    /// The voice with `m` (per unit weight) taken off.
    fn dir(&self, m: &[f32]) -> Vec<f32> {
        normalized(&self.sum.iter().zip(m).map(|(s, x)| s - x * self.weight).collect::<Vec<_>>())
    }
    fn add(&mut self, e: &[f32], w: f32) {
        for (s, x) in self.sum.iter_mut().zip(e) {
            *s += x * w;
        }
        self.weight += w;
    }
}

/// How a segment was labelled, for the benchmark dump.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    /// Too short to embed (or no embedding): the previous speaker.
    Inherited,
    Assigned,
    /// Nearest speaker, voice left unchanged.
    Tentative,
    New,
}

#[derive(Debug, Clone, Default)]
pub struct Clusterer {
    pub cfg: ClusterConfig,
    speakers: Vec<Speaker>,
    /// Merged-away id -> the id it became.
    merged: Vec<(u32, u32)>,
    next_id: u32,
    last: Option<u32>,
    /// Weighted sum of every normalized embedding seen, for `center`.
    seen_sum: Vec<f32>,
    seen_w: f32,
    seen_n: usize,
    /// Running mean similarity of segments to the speaker they were assigned to, for `adaptive`.
    within: Option<f32>,
}

impl Clusterer {
    pub fn new(cfg: ClusterConfig) -> Self {
        Self { cfg, ..Default::default() }
    }

    /// The speaker of the last segment.
    pub fn last(&self) -> Option<u32> {
        self.last
    }

    pub fn speakers(&self) -> usize {
        self.speakers.len()
    }

    /// The current id of a speaker that may have been merged since.
    pub fn resolve(&self, mut id: u32) -> u32 {
        while let Some(&(_, to)) = self.merged.iter().find(|(from, _)| *from == id) {
            id = to;
        }
        id
    }

    /// The speaker `assign` would give, without changing anything: for drafts, which are heard again.
    /// A voice nobody has yet gets the id the next new speaker will have.
    pub fn peek(&self, emb: Option<&[f32]>, dur_s: f32) -> Option<u32> {
        self.clone().assign(emb, dur_s).0
    }

    /// Labels one segment of `dur_s` seconds; `emb` is None when it was too short or failed to embed.
    /// Returns the speaker (None until the first speaker is known), the best similarity and the decision.
    pub fn assign(&mut self, emb: Option<&[f32]>, dur_s: f32) -> (Option<u32>, f32, Decision) {
        let Some(emb) = emb.filter(|_| dur_s >= self.cfg.min_embed_s) else {
            return (self.last, f32::NAN, Decision::Inherited);
        };
        let e = normalized(emb);
        let w = dur_s.min(self.cfg.max_weight_s);
        let m = self.mean(e.len());
        let ec = normalized(&e.iter().zip(&m).map(|(x, y)| x - y).collect::<Vec<_>>());
        let best = self
            .speakers
            .iter()
            .enumerate()
            .map(|(i, s)| (i, dot(&s.dir(&m), &ec) + if Some(s.id) == self.last { self.cfg.stick } else { 0.0 }))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        let updatable = dur_s >= self.cfg.min_update_s;
        let (assign, new_speaker) = match (self.cfg.adaptive, self.within.unwrap_or(self.cfg.within_prior)) {
            (true, within) => (within - self.cfg.adapt_assign_gap, within - self.cfg.adapt_new_gap),
            (false, _) => (self.cfg.assign, self.cfg.new_speaker),
        };
        if self.seen_sum.is_empty() {
            self.seen_sum = vec![0.0; e.len()];
        }
        for (s, x) in self.seen_sum.iter_mut().zip(&e) {
            *s += x * w;
        }
        (self.seen_w, self.seen_n) = (self.seen_w + w, self.seen_n + 1);
        let (id, sim, decision) = match best {
            Some((i, sim)) if sim >= assign => {
                if updatable {
                    let own = sim - if Some(self.speakers[i].id) == self.last { self.cfg.stick } else { 0.0 };
                    self.within = Some(self.within.map_or(own, |v| 0.9 * v + 0.1 * own));
                }
                if updatable {
                    self.speakers[i].add(&e, w);
                }
                (self.speakers[i].id, sim, if updatable { Decision::Assigned } else { Decision::Tentative })
            }
            // The first speaker may start from a short segment: there is nothing to confuse them with.
            None => (self.create(&e, w), f32::NAN, Decision::New),
            Some((_, sim)) if sim < new_speaker && updatable && self.speakers.len() < self.cfg.max_speakers => (self.create(&e, w), sim, Decision::New),
            Some((i, sim)) => (self.speakers[i].id, sim, Decision::Tentative),
        };
        self.last = Some(id);
        self.merge_close();
        (self.last.map(|id| self.resolve(id)), sim, decision)
    }

    /// The share of the session's mean voice to take off (zeros while off or too early).
    fn mean(&self, dim: usize) -> Vec<f32> {
        if self.cfg.center <= 0.0 || self.seen_n < self.cfg.center_after || self.seen_w <= 0.0 {
            return vec![0.0; dim];
        }
        self.seen_sum.iter().map(|s| s / self.seen_w * self.cfg.center).collect()
    }

    fn create(&mut self, e: &[f32], w: f32) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        let mut s = Speaker { id, sum: vec![0.0; e.len()], weight: 0.0 };
        s.add(e, w);
        self.speakers.push(s);
        id
    }

    /// Speakers whose voices drifted together are one person heard in two ways (first heard from a short or
    /// noisy segment); the newer joins the older.
    fn merge_close(&mut self) {
        loop {
            let mut pair = None;
            'outer: for i in 0..self.speakers.len() {
                for j in i + 1..self.speakers.len() {
                    let m = self.mean(self.speakers[i].sum.len());
                    if dot(&self.speakers[i].dir(&m), &self.speakers[j].dir(&m)) >= self.cfg.merge {
                        pair = Some((i, j));
                        break 'outer;
                    }
                }
            }
            let Some((i, j)) = pair else { return };
            let gone = self.speakers.remove(j);
            let keep = &mut self.speakers[i];
            for (s, x) in keep.sum.iter_mut().zip(&gone.sum) {
                *s += x;
            }
            keep.weight += gone.weight;
            let to = keep.id;
            self.merged.push((gone.id, to));
            if self.last == Some(gone.id) {
                self.last = Some(to);
            }
        }
    }
}

/// One labelled segment, kept for the benchmark: stream positions in samples.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub start: usize,
    pub end: usize,
    pub speaker: Option<u32>,
    pub sim: Option<f32>,
    pub decision: Decision,
    pub text: String,
    /// Empty when not embedded.
    pub embedding: Vec<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frames of 0.1 s with the given (voice bit mask, seconds) parts.
    fn frames(parts: &[(u8, f32)]) -> Vec<u8> {
        parts.iter().flat_map(|&(m, s)| std::iter::repeat_n(m, (s * 10.0).round() as usize)).collect()
    }
    const STEP: usize = 1600;

    #[test]
    fn a_voice_change_needs_enough_of_both_voices() {
        let (new, old) = (24_000, 8_000); // 1.5 s, 0.5 s
        assert_eq!(voice_change(&frames(&[(1, 3.0), (2, 2.0)]), STEP, new, old), Some(48_000));
        assert_eq!(voice_change(&frames(&[(1, 3.0), (2, 1.0)]), STEP, new, old), None, "1 s of the new voice is a reply");
        assert_eq!(voice_change(&frames(&[(1, 0.3), (2, 3.0)]), STEP, new, old), None, "0.3 s of the old voice is not a turn");
        assert_eq!(voice_change(&frames(&[(2, 4.0)]), STEP, new, old), None);
    }

    #[test]
    fn silence_and_overlap_do_not_break_a_voice_run() {
        let (new, old) = (24_000, 8_000);
        // Old voice, a short pause, old again, overlap, then the new voice with a short pause inside.
        let f = frames(&[(1, 2.0), (0, 0.2), (1, 1.0), (3, 0.5), (2, 1.0), (0, 0.2), (2, 1.0)]);
        assert_eq!(voice_change(&f, STEP, new, old), Some(37 * STEP), "the change is where the new voice starts alone");
        // A 1 s backchannel by the other voice inside a long turn does not count, nor split the turn.
        let f = frames(&[(1, 3.0), (2, 1.0), (1, 3.0)]);
        assert_eq!(voice_change(&f, STEP, new, old), None);
    }

    /// A unit vector along `axis` with a little of `noise_axis`.
    fn v(axis: usize, noise_axis: usize, noise: f32) -> Vec<f32> {
        let mut e = vec![0.0; 8];
        e[axis] = 1.0;
        e[noise_axis] += noise;
        e
    }

    #[test]
    fn two_voices_get_two_stable_ids() {
        let mut c = Clusterer::new(ClusterConfig::default());
        let got: Vec<_> = [v(0, 7, 0.1), v(0, 6, 0.2), v(1, 7, 0.1), v(1, 6, 0.1), v(0, 5, 0.1)].iter().map(|e| c.assign(Some(e), 3.0).0).collect();
        assert_eq!(got, [Some(0), Some(0), Some(1), Some(1), Some(0)]);
        assert_eq!(c.speakers(), 2);
    }

    #[test]
    fn short_segments_inherit_and_never_start_a_speaker() {
        let mut c = Clusterer::new(ClusterConfig::default());
        assert_eq!(c.assign(None, 0.5).0, None, "nobody known yet");
        c.assign(Some(&v(0, 7, 0.0)), 3.0);
        let (id, _, d) = c.assign(Some(&v(1, 7, 0.0)), 1.5);
        assert_eq!((id, d), (Some(0), Decision::Tentative), "1.5 s of a new voice goes to the nearest speaker");
        assert_eq!(c.speakers(), 1);
        let (id, _, d) = c.assign(Some(&v(1, 7, 0.0)), 0.6);
        assert_eq!((id, d), (Some(0), Decision::Inherited), "under a second is not embedded");
        let (id, _, d) = c.assign(Some(&v(1, 7, 0.0)), 2.5);
        assert_eq!((id, d), (Some(1), Decision::New));
    }

    #[test]
    fn the_speaker_count_is_capped() {
        let mut c = Clusterer::new(ClusterConfig { max_speakers: 2, ..Default::default() });
        for axis in 0..4 {
            c.assign(Some(&v(axis, 7, 0.0)), 3.0);
        }
        assert_eq!(c.speakers(), 2);
    }

    #[test]
    fn speakers_that_drift_together_merge_into_the_older_id() {
        let mut c = Clusterer::new(ClusterConfig { merge: 0.6, ..Default::default() });
        c.assign(Some(&v(0, 1, 0.0)), 3.0);
        // Similarity to speaker 0 is about 0.3: a new speaker.
        assert_eq!(c.assign(Some(&v(1, 0, 0.3)), 3.0).0, Some(1));
        // Pull speaker 0 towards axis 1 with in-between segments (similar enough to either).
        for _ in 0..20 {
            c.assign(Some(&v(0, 1, 1.0)), 10.0);
        }
        assert_eq!(c.speakers(), 1);
        assert_eq!(c.resolve(1), 0);
    }

    /// Two voices over a strong shared channel component: similar in raw cosine, apart once it is taken off.
    fn channel(axis: usize, noise_axis: usize, noise: f32) -> Vec<f32> {
        let mut e = v(axis, noise_axis, noise);
        e[7] += 2.0;
        e
    }

    #[test]
    fn centering_separates_voices_that_share_a_channel() {
        let segs: Vec<_> = (0..12).map(|k| channel(k % 2, 2 + k % 3, 0.1)).collect();
        let mut raw = Clusterer::new(ClusterConfig::default());
        for e in &segs {
            raw.assign(Some(e), 3.0);
        }
        assert_eq!(raw.speakers(), 1, "raw cosine is about 0.8 between the two voices");
        let mut c = Clusterer::new(ClusterConfig { center: 1.0, center_after: 2, ..Default::default() });
        let got: Vec<_> = segs.iter().map(|e| c.assign(Some(e), 3.0).0.unwrap()).collect();
        assert_eq!(c.speakers(), 2);
        assert!(got[4..].iter().enumerate().all(|(k, &id)| id == got[4 + k % 2]), "alternating speakers once centered: {got:?}");
    }

    #[test]
    fn adaptive_thresholds_follow_the_within_speaker_similarity() {
        // Same-speaker cosine about 0.95, other speaker about 0.8: fixed defaults merge them, adaptive does not.
        let cfg = ClusterConfig { adaptive: true, adapt_assign_gap: 0.05, adapt_new_gap: 0.1, within_prior: 0.95, ..Default::default() };
        let mut c = Clusterer::new(cfg);
        for _ in 0..3 {
            c.assign(Some(&channel(0, 3, 0.1)), 3.0);
        }
        assert_eq!(c.assign(Some(&channel(1, 3, 0.1)), 3.0).2, Decision::New);
    }
}
