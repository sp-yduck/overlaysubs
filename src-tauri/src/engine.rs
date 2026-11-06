//! whisper.cpp and llama.cpp adapters for the core `Asr` / `Translator` / `Vad` traits.
use overlaysubs_core::endpoint::{Vad, FRAME};
use overlaysubs_core::pfml::{self, TranslateRequest};
use overlaysubs_core::session::{Asr, Transcript, Translator};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::sampling::LlamaSampler;
use std::num::NonZeroU32;
use std::path::Path;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperVadContext, WhisperVadContextParams, WhisperVadParams};

pub struct Whisper {
    ctx: WhisperContext,
    pub vad_path: String,
    threads: i32,
}

impl Whisper {
    pub fn load(model: &Path, vad: &Path, use_gpu: bool, threads: usize) -> Result<Self, String> {
        let mut p = WhisperContextParameters::default();
        p.use_gpu(use_gpu);
        let ctx = WhisperContext::new_with_params(model, p).map_err(|e| e.to_string())?;
        Ok(Self { ctx, vad_path: vad.to_string_lossy().into_owned(), threads: threads.min(8) as i32 })
    }
}

impl Whisper {
    /// Full decode without VAD, so synthetic audio still costs real work.
    pub fn bench(&mut self, pcm: &[f32]) -> Result<(), String> {
        let mut st = self.ctx.create_state().map_err(|e| e.to_string())?;
        let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        p.set_n_threads(self.threads);
        p.set_language(Some("en"));
        p.set_print_progress(false);
        st.full(p, pcm).map_err(|e| e.to_string())
    }
}

impl Asr for Whisper {
    fn transcribe(&mut self, pcm: &[f32], lang: Option<&str>, prompt: &str) -> Result<Transcript, String> {
        let mut st = self.ctx.create_state().map_err(|e| e.to_string())?;
        let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        p.set_n_threads(self.threads);
        p.set_language(Some(lang.unwrap_or("auto")));
        p.set_no_context(true);
        // The prompt comes from user settings; set_initial_prompt panics on NUL.
        let prompt = prompt.replace('\0', "");
        if !prompt.is_empty() {
            p.set_initial_prompt(&prompt);
        }
        p.set_single_segment(true);
        p.set_suppress_blank(true);
        p.set_suppress_nst(true);
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_special(false);
        p.set_print_timestamps(false);
        p.set_vad_model_path(Some(&self.vad_path));
        p.set_vad_params(WhisperVadParams::new());
        p.enable_vad(true);
        st.full(p, pcm).map_err(|e| e.to_string())?;
        let text: String = (0..st.full_n_segments()).filter_map(|i| st.get_segment(i)).map(|s| s.to_string()).collect();
        let lang = whisper_rs::get_lang_str(st.full_lang_id_from_state()).unwrap_or("en").to_string();
        Ok(Transcript { text: text.trim().to_string(), lang })
    }
}

/// Silero VAD for endpointing. whisper.cpp resets the LSTM state on every call, so each call
/// re-scores the last `CONTEXT` samples to warm it up and returns only the new frames' probabilities.
pub struct Silero {
    ctx: WhisperVadContext,
    history: Vec<f32>,
}

const CONTEXT: usize = 16 * FRAME; // ~0.5 s

impl Silero {
    pub fn load(path: &str) -> Result<Self, String> {
        // Without a log backend feature this discards whisper.cpp's per-call INFO lines.
        whisper_rs::install_logging_hooks();
        let mut p = WhisperVadContextParams::new();
        p.set_n_threads(1);
        Ok(Self { ctx: WhisperVadContext::new(path, p).map_err(|e| e.to_string())?, history: vec![] })
    }
}

impl Vad for Silero {
    fn probs(&mut self, pcm: &[f32]) -> Result<Vec<f32>, String> {
        self.history.extend_from_slice(pcm);
        self.ctx.detect_speech(&self.history).map_err(|e| e.to_string())?;
        let p = self.ctx.probabilities();
        let out = p[p.len() - pcm.len() / FRAME..].to_vec();
        let excess = self.history.len().saturating_sub(CONTEXT);
        self.history.drain(..excess);
        Ok(out)
    }
}

pub struct Plamo {
    backend: LlamaBackend,
    model: LlamaModel,
    threads: i32,
}

const N_CTX: u32 = 4096;
const MAX_NEW: usize = 256;

impl Plamo {
    pub fn load(model: &Path, gpu_layers: u32, threads: usize) -> Result<Self, String> {
        let mut backend = LlamaBackend::init().map_err(|e| e.to_string())?;
        backend.void_logs();
        let params = LlamaModelParams::default().with_n_gpu_layers(gpu_layers);
        let model = LlamaModel::load_from_file(&backend, model, &params).map_err(|e| e.to_string())?;
        Ok(Self { backend, model, threads: threads.min(8) as i32 })
    }
}

impl Translator for Plamo {
    fn translate(&mut self, req: &TranslateRequest, on_piece: &mut dyn FnMut(&str)) -> Result<String, String> {
        let vocab = self.model.vocab();
        // Fresh context per call: cheap at this size and avoids KV-cache bookkeeping.
        let cp = LlamaContextParams::default().with_n_ctx(NonZeroU32::new(N_CTX)).with_n_batch(N_CTX).with_n_threads(self.threads).with_n_threads_batch(self.threads);
        let mut ctx = self.model.new_context(&self.backend, cp).map_err(|e| e.to_string())?;
        let tokens = vocab.tokenize(pfml::build(req).as_bytes(), true, true);
        if tokens.len() + MAX_NEW > N_CTX as usize {
            return Err("utterance too long for context".into());
        }
        let mut batch = LlamaBatch::new(N_CTX as usize, 1);
        batch.add_sequence(&tokens, 0, false).map_err(|e| e.to_string())?;
        ctx.decode(&mut batch).map_err(|e| e.to_string())?;
        let mut sampler = LlamaSampler::greedy();
        let mut out = Vec::<u8>::new();
        let mut emitted = 0;
        let mut pos = tokens.len() as i32;
        for _ in 0..MAX_NEW {
            let tok = sampler.sample(&ctx, batch.n_tokens() - 1);
            if vocab.is_eog(tok) {
                break;
            }
            out.extend(vocab.token_to_piece(tok, true, None));
            if let Some(stop) = pfml::STOP.iter().find_map(|s| find(&out, s.as_bytes())) {
                out.truncate(stop);
                break;
            }
            // Emit only whole UTF-8 characters.
            let valid = match std::str::from_utf8(&out[emitted..]) {
                Ok(s) => s.len(),
                Err(e) => e.valid_up_to(),
            };
            if valid > 0 {
                on_piece(std::str::from_utf8(&out[emitted..emitted + valid]).unwrap());
                emitted += valid;
            }
            batch.clear();
            batch.add(tok, pos, &[0], true).map_err(|e| e.to_string())?;
            pos += 1;
            ctx.decode(&mut batch).map_err(|e| e.to_string())?;
        }
        Ok(String::from_utf8_lossy(&out).trim().to_string())
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use overlaysubs_core::endpoint::{EndpointConfig, Endpointer, Event};
    use overlaysubs_core::registry::Registry;
    use overlaysubs_core::session::{Session, SessionConfig, Stage};

    /// 16 kHz mono s16 WAV from OVERLAYSUBS_WAV.
    fn wav() -> Vec<f32> {
        let wav = std::fs::read(std::env::var("OVERLAYSUBS_WAV").unwrap()).unwrap();
        let data = wav.windows(4).position(|w| w == b"data").unwrap() + 8;
        wav[data..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect()
    }

    /// Prints where the energy gate and Silero cut the clip, fed in 10 ms chunks like live capture.
    /// Run: OVERLAYSUBS_WAV=path/to/16k-mono.wav cargo test -p overlaysubs endpoint_dump -- --ignored --nocapture
    #[test]
    #[ignore]
    fn endpoint_dump() {
        let reg = Registry::parse(include_str!("../../models.json")).unwrap();
        let dir = std::env::temp_dir().join("overlaysubs-models");
        let m = reg.get("silero-vad-v6.2.0").unwrap();
        crate::download::fetch(m, &dir, &mut |_, _| {}).unwrap();
        let pcm = wav();
        let vads: [(&str, Box<dyn overlaysubs_core::endpoint::Vad>); 2] =
            [("energy", Box::new(overlaysubs_core::endpoint::Energy(0.01))), ("silero", Box::new(Silero::load(&dir.join(&m.file).to_string_lossy()).unwrap()))];
        for (name, mut vad) in vads {
            let mut ep = Endpointer::new(EndpointConfig::default());
            let (mut evs, mut pos, t0) = (vec![], 0, std::time::Instant::now());
            for chunk in pcm.chunks(160).chain(std::iter::once(&[0.0; 16_000][..])) {
                pos += chunk.len();
                ep.push(chunk, vad.as_mut(), &mut evs).unwrap();
                for e in evs.drain(..) {
                    if let Event::Final(v) = e {
                        eprintln!("{name}: final emitted at {:6.2}s, {:5.2}s long", pos as f32 / 16_000.0, v.len() as f32 / 16_000.0);
                    }
                }
            }
            eprintln!("{name}: {:?} for {:.1}s of audio", t0.elapsed(), pos as f32 / 16_000.0);
        }
    }

    /// Downloads ~0.7 GB. Run: OVERLAYSUBS_WAV=path/to/16k-mono.wav cargo test -p overlaysubs -- --ignored
    /// Optional: OVERLAYSUBS_LANG (spoken language, default en), OVERLAYSUBS_ASR (model id, default whisper-base),
    /// OVERLAYSUBS_GLOSSARY ("src=dst;src=dst", empty for none), OVERLAYSUBS_TOPIC, OVERLAYSUBS_SOURCE_LANG (fixes the spoken language).
    #[test]
    #[ignore]
    fn real_models_transcribe_and_translate() {
        let env = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.into());
        let reg = Registry::parse(include_str!("../../models.json")).unwrap();
        let dir = std::env::temp_dir().join("overlaysubs-models");
        let asr_id = env("OVERLAYSUBS_ASR", "whisper-base");
        let ids = [asr_id.as_str(), "plamo-3-translate-0.8b-q4_k_m", "silero-vad-v6.2.0"];
        for id in ids {
            crate::download::fetch(reg.get(id).unwrap(), &dir, &mut |_, _| {}).unwrap();
        }
        let p = |i: usize| dir.join(&reg.get(ids[i]).unwrap().file);
        // Threads above the core count make ggml's spin barriers thrash (measured 30x slower).
        let threads = std::thread::available_parallelism().unwrap().get();
        let mut asr = Whisper::load(&p(0), &p(2), false, threads).unwrap();
        let mut mt = Plamo::load(&p(1), 0, threads).unwrap();

        let pcm = wav();
        let mut ep = Endpointer::new(EndpointConfig::default());
        let mut evs = vec![];
        let mut vad = Silero::load(&asr.vad_path).unwrap();
        ep.push(&pcm, &mut vad, &mut evs).unwrap();
        ep.push(&vec![0.0; 16_000], &mut vad, &mut evs).unwrap();
        let lang = env("OVERLAYSUBS_LANG", "en");
        // Glossary terms reach Whisper as its prompt.
        let glossary = env("OVERLAYSUBS_GLOSSARY", "Blink=ブリンク;Tracer=トレーサー").split(';').filter_map(|e| e.split_once('=')).map(|(a, b)| (a.into(), b.into())).collect();
        let source_lang = std::env::var("OVERLAYSUBS_SOURCE_LANG").ok();
        let mut s = Session::new(SessionConfig { glossary, topic: env("OVERLAYSUBS_TOPIC", ""), source_lang, ..Default::default() });
        let mut finals = vec![];
        for ev in evs.into_iter().filter(|e| matches!(e, Event::Final(_))) {
            let t0 = std::time::Instant::now();
            s.handle(ev, &mut asr, &mut mt, &mut |c| if c.stage == Stage::Final { finals.push(c) }).unwrap();
            eprintln!("utterance took {:?}", t0.elapsed());
        }
        s.flush(&mut mt, &mut |c| if c.stage == Stage::Final { finals.push(c) }).unwrap();
        for c in &finals {
            eprintln!("[{}] {}\n  → {}", c.lang, c.source, c.translation);
        }
        assert!(!finals.is_empty(), "speech produced captions");
        assert!(finals.iter().all(|c| c.lang == lang && c.translation.chars().any(|ch| ('\u{3040}'..='\u{30ff}').contains(&ch) || ('\u{4e00}'..='\u{9fff}').contains(&ch))), "{lang} speech translated into Japanese");
    }
}
