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
    /// A pause this long, once the utterance is `pause_after_ms` old, marks a partial as a place the
    /// session may cut if the text so far ends a sentence. 0 turns it off (the default: only `Early::Pause` uses it).
    pub pause_ms: usize,
    pub pause_after_ms: usize,
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self { threshold: 0.5, silence_ms: 600, partial_every_ms: 1000, max_ms: 15_000, pre_roll_ms: 200, pause_ms: 0, pause_after_ms: 3000 }
    }
}

#[derive(Debug, PartialEq)]
pub enum Event {
    /// Utterance still running; audio so far. `start` is the stream position (in samples) of `pcm[0]`.
    /// `pause` is set when the speaker just paused: the stream position where the speech stopped.
    Partial { pcm: Vec<f32>, start: usize, pause: Option<usize> },
    /// Utterance finished.
    Final { pcm: Vec<f32>, start: usize },
}

impl Event {
    pub fn pcm(&self) -> &[f32] {
        match self {
            Event::Partial { pcm, .. } | Event::Final { pcm, .. } => pcm,
        }
    }
    pub fn is_final(&self) -> bool {
        matches!(self, Event::Final { .. })
    }
}

pub struct Endpointer {
    cfg: EndpointConfig,
    pending: Vec<f32>,
    pre: Vec<f32>,
    buf: Vec<f32>,
    silent: usize,
    since_partial: usize,
    /// Stream position of the next sample pushed into `buf`, and of `buf[0]`.
    pos: usize,
    start: usize,
    paused: bool,
}

impl Endpointer {
    pub fn new(cfg: EndpointConfig) -> Self {
        Self { cfg, pending: Vec::new(), pre: Vec::new(), buf: Vec::new(), silent: 0, since_partial: 0, pos: 0, start: 0, paused: false }
    }

    /// The session committed everything before stream position `pos`: drop that audio from the running
    /// utterance, so later partials and `max_ms` only count what is still open.
    pub fn commit(&mut self, pos: usize) {
        if self.buf.is_empty() || pos <= self.start {
            return;
        }
        let n = (pos - self.start).min(self.buf.len());
        self.buf.drain(..n);
        self.start += n;
        self.silent = self.silent.min(self.buf.len());
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
            self.pos += frame.len();
            let speaking = p >= if self.buf.is_empty() { self.cfg.threshold } else { self.cfg.threshold - 0.15 };
            if self.buf.is_empty() && !speaking {
                self.pre.extend_from_slice(frame);
                let excess = self.pre.len().saturating_sub(self.cfg.pre_roll_ms * RATE / 1000);
                self.pre.drain(..excess);
                continue;
            }
            if self.buf.is_empty() {
                self.buf = std::mem::take(&mut self.pre);
                self.start = self.pos - frame.len() - self.buf.len();
                self.paused = false;
            }
            self.buf.extend_from_slice(frame);
            self.silent = if speaking { 0 } else { self.silent + frame.len() };
            if speaking {
                self.paused = false;
            }
            self.since_partial += frame.len();
            let at_pause = self.cfg.pause_ms > 0 && !self.paused && ms(self.silent) >= self.cfg.pause_ms && ms(self.buf.len()) >= self.cfg.pause_after_ms;
            if ms(self.silent) >= self.cfg.silence_ms || ms(self.buf.len()) >= self.cfg.max_ms {
                let keep = self.buf.len() - self.silent.min(self.buf.len());
                self.buf.truncate(keep.max(1));
                out.push(Event::Final { pcm: std::mem::take(&mut self.buf), start: self.start });
                self.silent = 0;
                self.since_partial = 0;
            } else if at_pause {
                // Once per pause; it stands in for the regular partial.
                self.paused = true;
                out.push(Event::Partial { pcm: self.buf.clone(), start: self.start, pause: Some(self.pos - self.silent) });
                self.since_partial = 0;
            } else if ms(self.since_partial) >= self.cfg.partial_every_ms {
                out.push(Event::Partial { pcm: self.buf.clone(), start: self.start, pause: None });
                self.since_partial = 0;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_PRE: EndpointConfig = EndpointConfig { threshold: 0.5, silence_ms: 600, partial_every_ms: 1000, max_ms: 15_000, pre_roll_ms: 0, pause_ms: 0, pause_after_ms: 3000 };

    /// Replays fixed probabilities, one per frame.
    struct Script(Vec<f32>);
    impl Vad for Script {
        fn probs(&mut self, pcm: &[f32]) -> Result<Vec<f32>, String> {
            Ok(self.0.drain(..pcm.len() / FRAME).collect())
        }
    }

    fn finals(ev: &[Event]) -> Vec<usize> {
        ev.iter().filter_map(|x| if let Event::Final { pcm, .. } = x { Some(pcm.len()) } else { None }).collect()
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
        assert_eq!(ev.iter().filter(|x| matches!(x, Event::Partial { .. })).count(), 2);
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
    fn pause_marks_a_partial_and_commit_drops_the_audio_before_it() {
        let cfg = EndpointConfig { pause_ms: 250, ..NO_PRE };
        let mut e = Endpointer::new(cfg);
        let mut ev = Vec::new();
        let vad = &mut Energy(0.01);
        e.push(&vec![0.0; 32 * FRAME], vad, &mut ev).unwrap();
        e.push(&vec![0.1; 100 * FRAME], vad, &mut ev).unwrap(); // 3.2 s of speech
        e.push(&vec![0.0; 12 * FRAME], vad, &mut ev).unwrap(); // 384 ms pause, under silence_ms
        let pauses: Vec<_> = ev.iter().filter_map(|x| if let Event::Partial { pause: Some(p), start, .. } = x { Some((*start, *p)) } else { None }).collect();
        assert_eq!(pauses, vec![(32 * FRAME, 132 * FRAME)], "one pause partial, at the end of the speech");
        e.commit(132 * FRAME);
        e.push(&vec![0.1; 32 * FRAME], vad, &mut ev).unwrap();
        e.push(&vec![0.0; 32 * FRAME], vad, &mut ev).unwrap();
        let Some(Event::Final { pcm, start }) = ev.pop() else { panic!() };
        assert_eq!((start, pcm.len()), (132 * FRAME, 44 * FRAME), "the final starts at the commit: the pause, then the new speech");
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
