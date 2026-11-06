//! Endpointer that cuts 16 kHz mono audio into utterances from per-frame speech probabilities.
//! The app feeds it Silero VAD; `Energy` is a plain RMS gate for tests.

pub const RATE: usize = 16_000;
/// Silero's window at 16 kHz (32 ms).
pub const FRAME: usize = 512;
/// Frames scored per VAD call, so the VAD is not invoked for every tiny capture chunk.
const BATCH: usize = 4;

pub trait Vad: Send {
    /// One speech probability per `FRAME` samples; `pcm.len()` is a multiple of `FRAME`.
    fn probs(&mut self, pcm: &[f32]) -> Result<Vec<f32>, String>;
}

/// RMS gate: 1.0 at or above the threshold, else 0.0.
pub struct Energy(pub f32);

impl Vad for Energy {
    fn probs(&mut self, pcm: &[f32]) -> Result<Vec<f32>, String> {
        Ok(pcm.chunks(FRAME).map(|f| if (f.iter().map(|s| s * s).sum::<f32>() / f.len() as f32).sqrt() >= self.0 { 1.0 } else { 0.0 }).collect())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EndpointConfig {
    /// Speech probability that starts an utterance; it continues while above `threshold - 0.15` (Silero's hysteresis).
    pub threshold: f32,
    pub silence_ms: usize,
    pub partial_every_ms: usize,
    pub max_ms: usize,
    /// Audio kept from before speech onset, so the first syllable is not clipped.
    pub pre_roll_ms: usize,
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self { threshold: 0.5, silence_ms: 600, partial_every_ms: 1000, max_ms: 15_000, pre_roll_ms: 200 }
    }
}

#[derive(Debug, PartialEq)]
pub enum Event {
    /// Utterance still running; audio so far.
    Partial(Vec<f32>),
    /// Utterance finished.
    Final(Vec<f32>),
}

pub struct Endpointer {
    cfg: EndpointConfig,
    pending: Vec<f32>,
    pre: Vec<f32>,
    buf: Vec<f32>,
    silent: usize,
    since_partial: usize,
}

impl Endpointer {
    pub fn new(cfg: EndpointConfig) -> Self {
        Self { cfg, pending: Vec::new(), pre: Vec::new(), buf: Vec::new(), silent: 0, since_partial: 0 }
    }

    pub fn push(&mut self, pcm: &[f32], vad: &mut dyn Vad, out: &mut Vec<Event>) -> Result<(), String> {
        let ms = |n: usize| n * 1000 / RATE;
        self.pending.extend_from_slice(pcm);
        if self.pending.len() < BATCH * FRAME {
            return Ok(());
        }
        let n = self.pending.len() / FRAME * FRAME;
        let pcm: Vec<f32> = self.pending.drain(..n).collect();
        let probs = vad.probs(&pcm)?;
        for (frame, p) in pcm.chunks(FRAME).zip(probs) {
            let speaking = p >= if self.buf.is_empty() { self.cfg.threshold } else { self.cfg.threshold - 0.15 };
            if self.buf.is_empty() && !speaking {
                self.pre.extend_from_slice(frame);
                let excess = self.pre.len().saturating_sub(self.cfg.pre_roll_ms * RATE / 1000);
                self.pre.drain(..excess);
                continue;
            }
            if self.buf.is_empty() {
                self.buf = std::mem::take(&mut self.pre);
            }
            self.buf.extend_from_slice(frame);
            self.silent = if speaking { 0 } else { self.silent + frame.len() };
            self.since_partial += frame.len();
            if ms(self.silent) >= self.cfg.silence_ms || ms(self.buf.len()) >= self.cfg.max_ms {
                let keep = self.buf.len() - self.silent.min(self.buf.len());
                self.buf.truncate(keep.max(1));
                out.push(Event::Final(std::mem::take(&mut self.buf)));
                self.silent = 0;
                self.since_partial = 0;
            } else if ms(self.since_partial) >= self.cfg.partial_every_ms {
                out.push(Event::Partial(self.buf.clone()));
                self.since_partial = 0;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_PRE: EndpointConfig = EndpointConfig { threshold: 0.5, silence_ms: 600, partial_every_ms: 1000, max_ms: 15_000, pre_roll_ms: 0 };

    /// Replays fixed probabilities, one per frame.
    struct Script(Vec<f32>);
    impl Vad for Script {
        fn probs(&mut self, pcm: &[f32]) -> Result<Vec<f32>, String> {
            Ok(self.0.drain(..pcm.len() / FRAME).collect())
        }
    }

    fn finals(ev: &[Event]) -> Vec<usize> {
        ev.iter().filter_map(|x| if let Event::Final(v) = x { Some(v.len()) } else { None }).collect()
    }

    #[test]
    fn speech_then_silence_yields_partials_then_one_final() {
        let mut e = Endpointer::new(NO_PRE);
        let mut ev = Vec::new();
        let vad = &mut Energy(0.01);
        e.push(&vec![0.0; 32 * FRAME], vad, &mut ev).unwrap();
        assert!(ev.is_empty(), "leading silence is ignored");
        e.push(&vec![0.1; 64 * FRAME], vad, &mut ev).unwrap();
        e.push(&vec![0.0; 32 * FRAME], vad, &mut ev).unwrap();
        assert_eq!(ev.iter().filter(|x| matches!(x, Event::Partial(_))).count(), 2);
        assert_eq!(finals(&ev), vec![64 * FRAME], "trailing silence trimmed");
    }

    #[test]
    fn long_speech_is_force_cut() {
        let mut e = Endpointer::new(EndpointConfig::default());
        let mut ev = Vec::new();
        e.push(&vec![0.1; RATE * 31], &mut Energy(0.01), &mut ev).unwrap();
        assert_eq!(finals(&ev).len(), 2);
    }

    #[test]
    fn pre_roll_keeps_audio_before_onset() {
        let mut e = Endpointer::new(EndpointConfig::default());
        let mut ev = Vec::new();
        let vad = &mut Energy(0.01);
        e.push(&vec![0.0; 32 * FRAME], vad, &mut ev).unwrap();
        e.push(&vec![0.1; 32 * FRAME], vad, &mut ev).unwrap();
        e.push(&vec![0.0; 32 * FRAME], vad, &mut ev).unwrap();
        let pre = 200 * RATE / 1000;
        assert_eq!(finals(&ev), vec![pre + 32 * FRAME]);
    }

    #[test]
    fn hysteresis_bridges_dips_and_vad_ends_utterance_despite_loud_audio() {
        // 0.4 starts nothing but continues speech; 0.2 is silence. Audio is loud throughout, as with game sound.
        let mut probs = vec![0.4; 8];
        probs.extend([0.9; 8]);
        probs.extend([0.4; 8]);
        probs.extend([0.9; 8]);
        probs.extend([0.2; 24]);
        let mut e = Endpointer::new(NO_PRE);
        let mut ev = Vec::new();
        e.push(&vec![0.3; 56 * FRAME], &mut Script(probs), &mut ev).unwrap();
        assert_eq!(finals(&ev), vec![24 * FRAME]);
    }

    #[test]
    fn small_chunks_are_batched() {
        let mut e = Endpointer::new(NO_PRE);
        let mut ev = Vec::new();
        let vad = &mut Energy(0.01);
        for _ in 0..(64 * FRAME / 160) {
            e.push(&[0.1; 160], vad, &mut ev).unwrap();
        }
        e.push(&vec![0.0; 32 * FRAME], vad, &mut ev).unwrap();
        assert_eq!(finals(&ev), vec![64 * FRAME]);
    }
}
