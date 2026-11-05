//! Cheap energy endpointer that cuts 16 kHz mono audio into utterances.
//! Silero VAD (inside whisper.cpp) then decides what is speech within each cut.

pub const RATE: usize = 16_000;

#[derive(Debug, Clone, Copy)]
pub struct EndpointConfig {
    pub threshold: f32,
    pub silence_ms: usize,
    pub partial_every_ms: usize,
    pub max_ms: usize,
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self { threshold: 0.01, silence_ms: 600, partial_every_ms: 1000, max_ms: 15_000 }
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
    buf: Vec<f32>,
    silent: usize,
    since_partial: usize,
}

const FRAME: usize = RATE / 50; // 20 ms

impl Endpointer {
    pub fn new(cfg: EndpointConfig) -> Self {
        Self { cfg, buf: Vec::new(), silent: 0, since_partial: 0 }
    }

    pub fn push(&mut self, pcm: &[f32], out: &mut Vec<Event>) {
        let ms = |n: usize| n * 1000 / RATE;
        for frame in pcm.chunks(FRAME) {
            let rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
            let speaking = rms >= self.cfg.threshold;
            if self.buf.is_empty() && !speaking {
                continue;
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speech_then_silence_yields_partials_then_one_final() {
        let mut e = Endpointer::new(EndpointConfig::default());
        let mut ev = Vec::new();
        e.push(&vec![0.0; RATE], &mut ev);
        assert!(ev.is_empty(), "leading silence is ignored");
        e.push(&vec![0.1; RATE * 2], &mut ev);
        e.push(&vec![0.0; RATE], &mut ev);
        let partials = ev.iter().filter(|x| matches!(x, Event::Partial(_))).count();
        let finals: Vec<_> = ev.iter().filter_map(|x| if let Event::Final(v) = x { Some(v.len()) } else { None }).collect();
        assert_eq!(partials, 2);
        assert_eq!(finals, vec![RATE * 2], "trailing silence trimmed");
    }

    #[test]
    fn long_speech_is_force_cut() {
        let mut e = Endpointer::new(EndpointConfig::default());
        let mut ev = Vec::new();
        e.push(&vec![0.1; RATE * 31], &mut ev);
        assert_eq!(ev.iter().filter(|x| matches!(x, Event::Final(_))).count(), 2);
    }
}
