//! Who is speaking: each committed sentence's audio gets a speaker embedding, and an online clusterer turns the
//! embeddings into stable speaker numbers. Overlapping speech (backchannels over someone else) is not handled.
use serde::{Deserialize, Serialize};

pub trait Embedder: Send {
    /// A fixed-size voice fingerprint of 16 kHz mono audio; need not be normalized.
    fn embed(&mut self, pcm: &[f32]) -> Result<Vec<f32>, String>;
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
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self { assign: 0.5, new_speaker: 0.35, min_embed_s: 1.0, min_update_s: 2.0, max_speakers: 6, stick: 0.05, merge: 0.75, max_weight_s: 10.0 }
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
    fn sim(&self, e: &[f32]) -> f32 {
        dot(&normalized(&self.sum), e)
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

    /// Labels one segment of `dur_s` seconds; `emb` is None when it was too short or failed to embed.
    /// Returns the speaker (None until the first speaker is known), the best similarity and the decision.
    pub fn assign(&mut self, emb: Option<&[f32]>, dur_s: f32) -> (Option<u32>, f32, Decision) {
        let Some(emb) = emb.filter(|_| dur_s >= self.cfg.min_embed_s) else {
            return (self.last, f32::NAN, Decision::Inherited);
        };
        let e = normalized(emb);
        let w = dur_s.min(self.cfg.max_weight_s);
        let best = self
            .speakers
            .iter()
            .enumerate()
            .map(|(i, s)| (i, s.sim(&e) + if Some(s.id) == self.last { self.cfg.stick } else { 0.0 }))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        let updatable = dur_s >= self.cfg.min_update_s;
        let (id, sim, decision) = match best {
            Some((i, sim)) if sim >= self.cfg.assign => {
                if updatable {
                    self.speakers[i].add(&e, w);
                }
                (self.speakers[i].id, sim, if updatable { Decision::Assigned } else { Decision::Tentative })
            }
            // The first speaker may start from a short segment: there is nothing to confuse them with.
            None => (self.create(&e, w), f32::NAN, Decision::New),
            Some((_, sim)) if sim < self.cfg.new_speaker && updatable && self.speakers.len() < self.cfg.max_speakers => (self.create(&e, w), sim, Decision::New),
            Some((i, sim)) => (self.speakers[i].id, sim, Decision::Tentative),
        };
        self.last = Some(id);
        self.merge_close();
        (self.last.map(|id| self.resolve(id)), sim, decision)
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
                    if dot(&normalized(&self.speakers[i].sum), &normalized(&self.speakers[j].sum)) >= self.cfg.merge {
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
}
