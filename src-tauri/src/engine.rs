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
use llama_cpp_2::token::LlamaToken;
use std::ffi::{c_char, c_void, CStr};
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::Mutex;
use whisper_rs::{DtwMode, DtwModelPreset, DtwParameters, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperVadContext, WhisperVadContextParams, WhisperVadParams};

/// Sends whisper.cpp's and llama.cpp's own messages to the `log` crate, which the app writes to its log file.
/// They are the only place the reason for a failed load or allocation is given, and a GUI build has no stderr
/// to show them. Every load calls this; repeating it is harmless.
fn install_log_hooks() {
    whisper_rs::install_logging_hooks();
    // Also sets the logger of the ggml that llama.cpp links.
    unsafe { llama_cpp_sys_2::llama_log_set(Some(llama_log), std::ptr::null_mut()) };
}

unsafe extern "C" fn llama_log(level: llama_cpp_sys_2::ggml_log_level, text: *const c_char, _: *mut c_void) {
    // A message without a trailing newline is continued by the next call.
    static LINE: Mutex<(log::Level, String)> = Mutex::new((log::Level::Info, String::new()));
    if text.is_null() {
        return;
    }
    // Never panic here: unwinding into llama.cpp aborts the process.
    let mut line = LINE.lock().unwrap_or_else(|e| e.into_inner());
    if line.1.is_empty() {
        line.0 = match level {
            llama_cpp_sys_2::GGML_LOG_LEVEL_ERROR => log::Level::Error,
            llama_cpp_sys_2::GGML_LOG_LEVEL_WARN => log::Level::Warn,
            llama_cpp_sys_2::GGML_LOG_LEVEL_DEBUG => log::Level::Debug,
            _ => log::Level::Info,
        };
    }
    line.1.push_str(&CStr::from_ptr(text).to_string_lossy());
    if line.1.ends_with('\n') {
        log::log!(target: "llama.cpp", line.0, "{}", line.1.trim_end());
        line.1.clear();
    }
}

pub struct Whisper {
    pub(crate) ctx: WhisperContext,
    pub vad_path: String,
    threads: i32,
    /// Token end times come from DTW (aligning the text to the audio), not from whisper.cpp's own estimate.
    dtw: bool,
}

impl Whisper {
    pub fn load(model: &Path, vad: &Path, use_gpu: bool, threads: usize) -> Result<Self, String> {
        install_log_hooks();
        let mut p = WhisperContextParameters::default();
        p.use_gpu(use_gpu);
        let preset = model.file_name().and_then(|f| dtw_preset(&f.to_string_lossy()));
        let dtw = preset.is_some();
        if let Some(model_preset) = preset {
            p.dtw_parameters(DtwParameters { mode: DtwMode::ModelPreset { model_preset }, ..Default::default() });
        }
        let ctx = WhisperContext::new_with_params(model, p).map_err(|e| e.to_string())?;
        Ok(Self { ctx, vad_path: vad.to_string_lossy().into_owned(), threads: threads.min(8) as i32, dtw })
    }
}

/// DTW alignment heads for a model, from its file name in models.json. None for an unknown model, which then
/// keeps whisper.cpp's own token times.
fn dtw_preset(file: &str) -> Option<DtwModelPreset> {
    use DtwModelPreset::*;
    let en = file.contains(".en");
    Some(match file {
        f if f.contains("large-v3-turbo") => LargeV3Turbo,
        f if f.contains("large-v3") => LargeV3,
        f if f.contains("medium") => if en { MediumEn } else { Medium },
        f if f.contains("small") => if en { SmallEn } else { Small },
        f if f.contains("base") => if en { BaseEn } else { Base },
        _ => return None,
    })
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
        // Token-level segments (the text is the same), so each token's end time is known; whisper.cpp maps
        // segment times back past the silence its VAD removed, which token times are not.
        // Not split_on_word: it splits only before a space, so Japanese came back as one segment (#78).
        p.set_token_timestamps(true);
        p.set_max_len(1);
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
        let eot = self.ctx.token_eot();
        // Bytes, not strings: a token can end in the middle of a UTF-8 character.
        let (mut raw, mut ends) = (Vec::new(), vec![]);
        for seg in (0..st.full_n_segments()).filter_map(|i| st.get_segment(i)) {
            raw.extend_from_slice(seg.to_bytes().map_err(|e| e.to_string())?);
            // whisper.cpp's own estimate puts a sentence-final "。" and every token after it at the end of the
            // audio, so an early commit cut into the next sentence and lost its first words (#84); DTW times are
            // right there. They are not mapped past the silence whisper.cpp's VAD removed, so they can only be
            // early: the cut moves earlier and more audio is heard again, none is lost.
            let last_text = (0..seg.n_tokens()).rev().filter_map(|k| seg.get_token(k)).map(|t| t.token_data()).find(|d| d.id < eot);
            let dtw = last_text.filter(|_| self.dtw).map(|d| d.t_dtw).filter(|&t| t >= 0);
            // Centiseconds into `pcm` -> samples.
            ends.push((raw.len(), (dtw.unwrap_or_else(|| seg.end_timestamp()).max(0) as usize * 160).min(pcm.len())));
        }
        let raw = String::from_utf8_lossy(&raw).into_owned();
        let lang = whisper_rs::get_lang_str(st.full_lang_id_from_state()).unwrap_or("en").to_string();
        // Languages written without spaces: every token ends a word. Elsewhere a word ends before a space.
        let unspaced = matches!(lang.as_str(), "ja" | "zh" | "yue" | "th" | "lo" | "km" | "my");
        let word_end = |o: usize| raw.is_char_boundary(o) && (unspaced || raw[o..].chars().next().is_none_or(char::is_whitespace));
        let lead = raw.len() - raw.trim_start().len();
        let text = raw.trim().to_string();
        let word_ends = ends.into_iter().filter(|&(o, _)| o > lead && word_end(o)).map(|(o, t)| ((o - lead).min(text.len()), t)).collect();
        Ok(Transcript { text, lang, word_ends })
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
        install_log_hooks();
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

/// Speaker embeddings from a sherpa-onnx speaker model (3D-Speaker CAM++, WeSpeaker ResNet, …), on the CPU.
#[cfg(feature = "diarize")]
pub struct Voice(sherpa_onnx::SpeakerEmbeddingExtractor);

#[cfg(feature = "diarize")]
impl Voice {
    pub fn load(model: &Path, threads: usize) -> Result<Self, String> {
        let cfg = sherpa_onnx::SpeakerEmbeddingExtractorConfig { model: Some(model.to_string_lossy().into_owned()), num_threads: threads as i32, ..Default::default() };
        sherpa_onnx::SpeakerEmbeddingExtractor::create(&cfg).map(Self).ok_or_else(|| format!("cannot load speaker model {}", model.display()))
    }
}

#[cfg(feature = "diarize")]
impl overlaysubs_core::speaker::Embedder for Voice {
    fn embed(&mut self, pcm: &[f32]) -> Result<Vec<f32>, String> {
        let st = self.0.create_stream().ok_or("cannot create a speaker stream")?;
        st.accept_waveform(16_000, pcm);
        st.input_finished();
        if !self.0.is_ready(&st) {
            return Err("too little audio for a speaker embedding".into());
        }
        self.0.compute(&st).ok_or_else(|| "speaker embedding failed".into())
    }
}

/// Speaker segmentation (pyannote segmentation-3.0 ONNX) on the CPU, run with the ONNX Runtime that sherpa-onnx
/// ships next to the exe. The model gives, per 270-sample frame, one of 7 classes: nobody, one of 3 local
/// voices, or a pair of them.
#[cfg(feature = "diarize")]
pub struct Segmentation(ort::session::Session);

#[cfg(feature = "diarize")]
impl Segmentation {
    pub fn load(model: &Path, threads: usize) -> Result<Self, String> {
        static ENV: std::sync::OnceLock<Result<ort::environment::Environment, String>> = std::sync::OnceLock::new();
        // The exe's own copy (Windows would find a System32 one first by name).
        let dll = onnxruntime_library();
        let env = ENV
            .get_or_init(|| ort::init_from(&dll).map_err(|e| format!("cannot load {}: {e}", dll.display()))?.build().map_err(|e| e.to_string()))
            .as_ref()
            .map_err(|e| e.clone())?;
        let session = ort::session::Session::builder(env)
            .map_err(|e| e.to_string())?
            .with_intra_threads(threads)
            .map_err(|e| e.to_string())?
            .with_inter_threads(1)
            .map_err(|e| e.to_string())?
            .commit_from_file(model)
            .map_err(|e| format!("cannot load segmentation model {}: {e}", model.display()))?;
        Ok(Self(session))
    }
}

/// The ONNX Runtime library sherpa-onnx ships with the app: next to the exe (Windows; System32 has an unrelated
/// onnxruntime.dll that Windows would otherwise find first), or in the bundle's Frameworks (macOS), where its name
/// carries the version.
#[cfg(feature = "diarize")]
fn onnxruntime_library() -> std::path::PathBuf {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_default();
    if cfg!(windows) {
        return exe_dir.join("onnxruntime.dll");
    }
    let (dirs, prefix, ext) = match cfg!(target_os = "macos") {
        true => (vec![exe_dir.join("../Frameworks"), exe_dir.clone()], "libonnxruntime", ".dylib"),
        false => (vec![exe_dir.clone()], "libonnxruntime", ".so"),
    };
    let found = dirs.iter().flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten()).map(|e| e.path()).find(|p| {
        let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        n.starts_with(prefix) && n.contains(ext) && !n.contains("providers")
    });
    found.unwrap_or_else(|| exe_dir.join(format!("{prefix}{ext}")))
}

#[cfg(feature = "diarize")]
impl overlaysubs_core::speaker::Segmenter for Segmentation {
    fn segment(&mut self, pcm: &[f32]) -> Result<(Vec<u8>, usize), String> {
        // Powerset classes -> local voice bit masks: {}, {0}, {1}, {2}, {0,1}, {0,2}, {1,2}.
        const MASKS: [u8; 7] = [0, 1, 2, 4, 3, 5, 6];
        let x = ort::value::Tensor::from_array(([1usize, 1, pcm.len()], pcm.to_vec())).map_err(|e| e.to_string())?;
        let out = self.0.run(ort::inputs!["x" => x]).map_err(|e| e.to_string())?;
        let (shape, y) = out["y"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        let classes = *shape.last().ok_or("no output shape")? as usize;
        let frames = y.chunks(classes).map(|c| MASKS[c.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map_or(0, |(i, _)| i).min(6)]).collect();
        Ok((frames, 270))
    }
}

pub struct Plamo {
    backend: LlamaBackend,
    model: LlamaModel,
    threads: i32,
    /// The model file name and the GPU layer count: a failed generation is explained in the log,
    /// and `translate` holds no path of its own.
    model_file: String,
    gpu_layers: u32,
}

const N_CTX: u32 = 4096;
const MAX_NEW: usize = 256;
/// A real translation does not say one token 32 times in a row; a degenerate generation does.
const REPEAT_STOP: usize = 32;

impl Plamo {
    pub fn load(model: &Path, gpu_layers: u32, threads: usize) -> Result<Self, String> {
        install_log_hooks();
        let backend = LlamaBackend::init().map_err(|e| e.to_string())?;
        let params = LlamaModelParams::default().with_n_gpu_layers(gpu_layers);
        let loaded = LlamaModel::load_from_file(&backend, model, &params).map_err(|e| e.to_string())?;
        let model_file = model.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let threads = threads.min(8) as i32;
        // App-level INFO is written to the log as it happens; the native engine's lines are only held
        // back until the next ERROR, and may have rotated out by the time a translation fails.
        log::info!("translator: {model_file}, gpu_layers {gpu_layers}, {threads} threads, n_ctx {N_CTX}");
        Ok(Self { backend, model: loaded, threads, model_file, gpu_layers })
    }

    /// The most frequent sampled tokens as `id '<piece>' xN`, for a failure log line. The pieces come
    /// from the tokenizer (`special = true`), so a broken output reads as token ids and names; no
    /// caption text is logged.
    fn summarize(&self, ids: &[i32]) -> String {
        let vocab = self.model.vocab();
        let mut counts: Vec<(i32, usize)> = Vec::new();
        for &id in ids {
            match counts.iter_mut().find(|e| e.0 == id) {
                Some(e) => e.1 += 1,
                None => counts.push((id, 1)),
            }
        }
        counts.sort_by(|a, b| b.1.cmp(&a.1));
        counts.iter().take(3).map(|&(id, n)| {
            let piece = String::from_utf8_lossy(&vocab.token_to_piece(LlamaToken::new(id), true, None)).replace('\n', "\\n");
            format!("{id} '{piece}' x{n}")
        }).collect::<Vec<_>>().join(", ")
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
        // A backend that is broken for this model can make every logit NaN or inf. llama.cpp's greedy
        // sampler only replaces `selected` on a strictly greater logit, so all-NaN logits select vocab
        // entry 0 at every position, and PLaMo 3's entry 0 is `<|plamo:unk|>` (llama.cpp issue #31509
        // reports exactly this on Vulkan). Record the numbers rather than captioning the fallout.
        let logits = ctx.get_logits_ith(batch.n_tokens() - 1);
        let mut finite = 0usize;
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for &l in logits {
            if l.is_finite() {
                finite += 1;
                lo = lo.min(l);
                hi = hi.max(l);
            }
        }
        if finite < logits.len() {
            let not_finite = logits.len() - finite;
            let range = match finite {
                0 => "no finite logits".to_string(),
                _ => format!("finite range {lo:.2}..{hi:.2}"),
            };
            log::error!("translator logits are not finite: {not_finite} of {} ({range}), {} prompt tokens, {}->{}, model {}, gpu_layers {}; the backend is broken for this model",
                logits.len(), tokens.len(), req.source_lang.unwrap_or("auto"), req.target_lang, self.model_file, self.gpu_layers);
            return Err(format!("translator logits are not finite ({not_finite} of {}); the GPU backend is likely broken for this model, see the log (try the translator on the CPU)", logits.len()));
        }
        let mut sampler = LlamaSampler::greedy();
        let mut out = Vec::<u8>::new();
        let mut emitted = 0;
        let mut pos = tokens.len() as i32;
        // Why generation ended and what it sampled. A failed generation is explained in the log and
        // never becomes a caption; `finished` means the model itself ended the sentence.
        let mut finished = false;
        let mut failure: Option<&'static str> = None;
        let mut sampled = Vec::<i32>::with_capacity(MAX_NEW);
        let mut last = None::<i32>;
        let mut run = 0usize;
        for _ in 0..MAX_NEW {
            let tok = sampler.sample(&ctx, batch.n_tokens() - 1);
            let id = tok.0;
            sampled.push(id);
            run = if last == Some(id) { run + 1 } else { 1 };
            last = Some(id);
            if vocab.is_eog(tok) {
                finished = true;
                break;
            }
            // `<|plamo:unk|>`: not EOG and not in `pfml::STOP`. A broken generation would loop here
            // until `MAX_NEW` and caption the unknown tokens.
            if id == 0 {
                failure = Some("an <|plamo:unk|> token");
                break;
            }
            // `のののの…` in llama.cpp #31889: stop before a degenerate run reaches a caption.
            if run >= REPEAT_STOP {
                failure = Some("one token repeated");
                break;
            }
            // `special = false`: control tokens are never rendered, so a model that emits one by
            // mistake cannot leak it into a caption.
            out.extend(vocab.token_to_piece(tok, false, None));
            if let Some(stop) = pfml::STOP.iter().find_map(|s| find(&out, s.as_bytes())) {
                out.truncate(stop);
                finished = true;
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
        if failure.is_none() && out.is_empty() && !finished {
            failure = Some("no text before the token budget ran out");
        }
        if let Some(reason) = failure {
            let summary = self.summarize(&sampled);
            log::error!("translator failed: {reason}; {} token(s) sampled [{summary}], {} prompt tokens, {}->{}, history {}, model {}, gpu_layers {}",
                sampled.len(), tokens.len(), req.source_lang.unwrap_or("auto"), req.target_lang, req.history.len(), self.model_file, self.gpu_layers);
            let hint = match self.gpu_layers {
                0 => "the model output is degenerate",
                _ => "the GPU backend is likely broken for this model (try the translator on the CPU)",
            };
            return Err(format!("translator failed ({reason}; {summary}); {hint}"));
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

    /// The reason a native library gives for a failure reaches the `log` crate, and so the log file.
    #[test]
    fn native_messages_reach_the_log() {
        struct Lines(Mutex<Vec<String>>);
        impl log::Log for Lines {
            fn enabled(&self, _: &log::Metadata) -> bool {
                true
            }
            fn log(&self, r: &log::Record) {
                self.0.lock().unwrap().push(format!("{} {} {}", r.target(), r.level(), r.args()));
            }
            fn flush(&self) {}
        }
        static LINES: Lines = Lines(Mutex::new(Vec::new()));
        log::set_logger(&LINES).unwrap();
        log::set_max_level(log::LevelFilter::Info);
        let bad = std::env::temp_dir().join("overlaysubs-not-a-model.bin");
        std::fs::write(&bad, "not a model").unwrap();
        assert!(Plamo::load(&bad, 0, 1).is_err() && Whisper::load(&bad, &bad, false, 1).is_err());
        let lines = LINES.0.lock().unwrap();
        for from in ["llama.cpp ERROR", "whisper_rs::whisper_logging_hook ERROR"] {
            assert!(lines.iter().any(|l| l.starts_with(from)), "no {from} line in {lines:#?}");
        }
    }

    /// A 16 kHz mono s16 WAV.
    fn wav(path: impl AsRef<Path>) -> Vec<f32> {
        let wav = std::fs::read(path).unwrap();
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
        let pcm = wav(std::env::var("OVERLAYSUBS_WAV").unwrap());
        let vads: [(&str, Box<dyn overlaysubs_core::endpoint::Vad>); 2] =
            [("energy", Box::new(overlaysubs_core::endpoint::Energy(0.01))), ("silero", Box::new(Silero::load(&dir.join(&m.file).to_string_lossy()).unwrap()))];
        for (name, mut vad) in vads {
            let mut ep = Endpointer::new(EndpointConfig::default());
            let (mut evs, mut pos, t0) = (vec![], 0, std::time::Instant::now());
            for chunk in pcm.chunks(160).chain(std::iter::once(&[0.0; 16_000][..])) {
                pos += chunk.len();
                ep.push(chunk, vad.as_mut(), &mut evs).unwrap();
                for e in evs.drain(..) {
                    if let Event::Final { pcm: v, .. } = e {
                        eprintln!("{name}: final emitted at {:6.2}s, {:5.2}s long", pos as f32 / 16_000.0, v.len() as f32 / 16_000.0);
                    }
                }
            }
            eprintln!("{name}: {:?} for {:.1}s of audio", t0.elapsed(), pos as f32 / 16_000.0);
        }
    }

    #[test]
    fn every_whisper_model_has_dtw_heads() {
        let reg = Registry::parse(include_str!("../../models.json")).unwrap();
        let whisper: Vec<_> = reg.models.iter().filter(|m| m.file.starts_with("ggml-") && !m.file.contains("silero")).collect();
        assert!(!whisper.is_empty());
        for m in whisper {
            assert!(dtw_preset(&m.file).is_some(), "{}", m.file);
        }
        assert!(matches!(dtw_preset("ggml-large-v3-turbo-q5_0.bin"), Some(DtwModelPreset::LargeV3Turbo)));
        assert!(matches!(dtw_preset("ggml-base.en.bin"), Some(DtwModelPreset::BaseEn)));
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

        let pcm = wav(std::env::var("OVERLAYSUBS_WAV").unwrap());
        let mut ep = Endpointer::new(EndpointConfig::default());
        let mut evs = vec![];
        let mut vad = Silero::load(&asr.vad_path).unwrap();
        ep.push(&pcm, &mut vad, &mut evs).unwrap();
        ep.push(&vec![0.0; 16_000], &mut vad, &mut evs).unwrap();
        let lang = env("OVERLAYSUBS_LANG", "en");
        // Glossary terms reach Whisper as its prompt.
        let glossary = env("OVERLAYSUBS_GLOSSARY", "dots=点;calligraphy=カリグラフィー").split(';').filter_map(|e| e.split_once('=')).map(|(a, b)| (a.into(), b.into())).collect();
        let source_lang = std::env::var("OVERLAYSUBS_SOURCE_LANG").ok();
        let mut s = Session::new(SessionConfig { glossary, topic: env("OVERLAYSUBS_TOPIC", ""), source_lang, ..Default::default() });
        let mut finals = vec![];
        for ev in evs.into_iter().filter(|e| e.is_final()) {
            let t0 = std::time::Instant::now();
            s.handle(ev, &mut asr, &mut mt, &mut |c| if c.stage == Stage::Final { finals.push(c) }).unwrap();
            eprintln!("utterance took {:?}", t0.elapsed());
        }
        for c in &finals {
            eprintln!("[{}] {}\n  → {}", c.lang, c.source, c.translation);
        }
        assert!(!finals.is_empty(), "speech produced captions");
        assert!(finals.iter().all(|c| c.lang == lang && c.translation.chars().any(|ch| ('\u{3040}'..='\u{30ff}').contains(&ch) || ('\u{4e00}'..='\u{9fff}').contains(&ch))), "{lang} speech translated into Japanese");
    }

    /// Adds up the time spent inside an engine.
    struct Timed<T>(T, std::time::Duration);
    impl<T: Asr> Asr for Timed<T> {
        fn transcribe(&mut self, pcm: &[f32], lang: Option<&str>, prompt: &str) -> Result<Transcript, String> {
            let t0 = std::time::Instant::now();
            let r = self.0.transcribe(pcm, lang, prompt);
            self.1 += t0.elapsed();
            r
        }
    }
    impl<T: Translator> Translator for Timed<T> {
        fn translate(&mut self, req: &TranslateRequest, on_piece: &mut dyn FnMut(&str)) -> Result<String, String> {
            let t0 = std::time::Instant::now();
            let r = self.0.translate(req, on_piece);
            self.1 += t0.elapsed();
            r
        }
    }

    #[derive(serde::Serialize, serde::Deserialize)]
    struct Report {
        asr: String,
        mt: String,
        gpu: bool,
        /// Peaks over the whole run in MB, models included; see `mem_sampler`. Absent in reports from before they were measured.
        #[serde(default)]
        ram_peak_mb: Option<f64>,
        #[serde(default)]
        vram_peak_mb: Option<f64>,
        clips: Vec<Clip>,
    }

    /// Samples memory until `stop` is set and returns the peaks in MB. RAM is this process's resident set.
    /// VRAM is how far GPU 0's used memory rose above its reading at the start, so keep other GPU apps idle.
    /// shortcut: VRAM comes from nvidia-smi, so it is None without an NVIDIA GPU; add a per-vendor query if AMD / Intel / Metal numbers are needed.
    fn mem_sampler(stop: std::sync::Arc<std::sync::atomic::AtomicBool>) -> std::thread::JoinHandle<(f64, Option<f64>)> {
        use std::io::BufRead;
        use std::process::{Command, Stdio};
        let smi = |extra: &[&str]| {
            let mut c = Command::new("nvidia-smi");
            c.args(["--query-gpu=memory.used", "--format=csv,noheader,nounits", "-i", "0"]).args(extra).stdout(Stdio::piped()).stderr(Stdio::null());
            c
        };
        // Read synchronously, before the caller loads a model.
        let base = smi(&[]).output().ok().and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().ok());
        std::thread::spawn(move || {
            // One long-lived nvidia-smi printing every 200 ms costs less than spawning one per sample.
            let mut child = base.and_then(|_| smi(&["-lms", "200"]).spawn().ok());
            let lines = child.as_mut().and_then(|c| c.stdout.take()).map(std::io::BufReader::new);
            let vram = lines.map(|l| std::thread::spawn(move || l.lines().map_while(Result::ok).filter_map(|l| l.trim().parse::<f64>().ok()).fold(0.0, f64::max)));
            let pid = sysinfo::get_current_pid().unwrap();
            let mut sys = sysinfo::System::new().unwrap();
            let mut ram = 0;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                sys.refresh_processes_specifics(sysinfo::ProcessesToUpdate::Some(&[pid]), false, sysinfo::ProcessRefreshKind::nothing().with_memory());
                ram = ram.max(sys.process(pid).map_or(0, |p| p.memory()));
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            if let Some(mut c) = child {
                let _ = c.kill();
                let _ = c.wait();
            }
            let peak = vram.map(|t| t.join().unwrap());
            (ram as f64 / (1 << 20) as f64, base.zip(peak).map(|(b, p)| (p - b).max(0.0)))
        })
    }

    /// Times are seconds. `load` is engine time over audio time (above 1 cannot keep up);
    /// `final_delay_*` is how long after the endpointer ended an utterance its captions were done.
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Clip {
        name: String,
        audio_s: f64,
        vad_s: f64,
        asr_s: f64,
        mt_s: f64,
        load: f64,
        final_delay_p50: f64,
        final_delay_max: f64,
        partials_run: usize,
        partials_skipped: usize,
        source: String,
        translation: String,
        /// Against `<clip>.txt` / `<clip>.tr.txt` next to the WAV, when present.
        cer: Option<f64>,
        tr_cer: Option<f64>,
        /// With OVERLAYSUBS_DIARIZE=1: seconds in the speaker models, speakers found, cuts at speaker changes.
        #[serde(default)]
        embed_s: f64,
        #[serde(default)]
        speakers: usize,
        #[serde(default)]
        early_commits: usize,
        #[serde(default)]
        speaker_commits: usize,
        #[serde(default)]
        segment_s: f64,
    }

    /// Speed and quality of the whole pipeline on recorded clips, replayed as live audio.
    /// Run it with scripts/bench.sh, which documents the inputs. Env: OVERLAYSUBS_WAV (a 16 kHz mono WAV or a
    /// folder of them), OVERLAYSUBS_OUT (report path), OVERLAYSUBS_BASELINE (an earlier report to compare with),
    /// OVERLAYSUBS_ASR / OVERLAYSUBS_MT (model ids), OVERLAYSUBS_GPU=1, OVERLAYSUBS_DIARIZE=1 (speaker colors as with the
    /// setting on; needs `--features diarize`, downloads the speaker models).
    #[test]
    #[ignore]
    fn bench_report() {
        use overlaysubs_core::metrics::{cer, replay};
        use std::time::{Duration, Instant};
        let env = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.into());
        let reg = Registry::parse(include_str!("../../models.json")).unwrap();
        let dir = std::env::temp_dir().join("overlaysubs-models");
        let ids = [env("OVERLAYSUBS_ASR", "whisper-base"), env("OVERLAYSUBS_MT", "plamo-3-translate-0.8b-q4_k_m"), "silero-vad-v6.2.0".into()];
        for id in &ids {
            crate::download::fetch(reg.get(id).unwrap(), &dir, &mut |_, _| {}).unwrap();
        }
        let p = |i: usize| dir.join(&reg.get(&ids[i]).unwrap().file);
        let gpu = env("OVERLAYSUBS_GPU", "0") == "1";
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mem = mem_sampler(stop.clone());
        let threads = std::thread::available_parallelism().unwrap().get();
        let mut asr = Timed(Whisper::load(&p(0), &p(2), gpu, threads).unwrap(), Duration::ZERO);
        let mut mt = Timed(Plamo::load(&p(1), if gpu { 999 } else { 0 }, threads).unwrap(), Duration::ZERO);

        let input = std::path::PathBuf::from(std::env::var("OVERLAYSUBS_WAV").unwrap());
        let mut wavs = match input.is_dir() {
            true => std::fs::read_dir(&input).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|x| x == "wav")).collect(),
            false => vec![input],
        };
        wavs.sort();
        assert!(!wavs.is_empty(), "no .wav in OVERLAYSUBS_WAV");
        let mut report = Report { asr: ids[0].clone(), mt: ids[1].clone(), gpu, ram_peak_mb: None, vram_peak_mb: None, clips: vec![] };
        for path in wavs {
            let pcm = wav(&path);
            // 10 ms chunks, as live capture delivers them; a second of silence ends the last utterance.
            let mut vad = Silero::load(&asr.0.vad_path).unwrap();
            let mut ep = Endpointer::new(EndpointConfig::default());
            let (mut evs, mut out, mut pos, t0) = (vec![], vec![], 0, Instant::now());
            for chunk in pcm.chunks(160).chain(std::iter::once(&[0.0; 16_000][..])) {
                pos += chunk.len();
                ep.push(chunk, &mut vad, &mut out).unwrap();
                evs.extend(out.drain(..).map(|e| (pos as f64 / 16_000.0, e.is_final(), (e, pos))));
            }
            let vad_s = t0.elapsed().as_secs_f64();
            (asr.1, mt.1) = (Duration::ZERO, Duration::ZERO);
            let mut s = Session::new(SessionConfig::default());
            #[cfg(feature = "diarize")]
            if env("OVERLAYSUBS_DIARIZE", "") == "1" {
                use overlaysubs_core::registry::Kind;
                let file = |k: Kind| {
                    let m = reg.models.iter().find(|m| m.kind == k).unwrap();
                    crate::download::fetch(m, &dir, &mut |_, _| {}).unwrap();
                    dir.join(&m.file)
                };
                s.embedder = Some(Box::new(Voice::load(&file(Kind::Speaker), 2).unwrap()));
                s.segmenter = Some(Box::new(Segmentation::load(&file(Kind::Segmentation), 2).unwrap()));
                s.speaker_commit = true;
            }
            let mut seen = 0;
            let mut r = replay(evs, |(ev, pos)| {
                s.tick(pos - seen);
                seen = pos;
                let t0 = Instant::now();
                s.handle(ev, &mut asr, &mut mt, &mut |_| {}).unwrap();
                t0.elapsed().as_secs_f64()
            });
            r.final_delays.sort_by(f64::total_cmp);
            let join = |f: fn(&overlaysubs_core::export::Entry) -> &str| s.entries.iter().map(f).collect::<Vec<_>>().join(" ");
            let (source, translation) = (join(|e| &e.source), join(|e| &e.translation));
            let audio_s = pcm.len() as f64 / 16_000.0;
            let reference = |ext: &str, got: &str| std::fs::read_to_string(path.with_extension(ext)).ok().map(|want| cer(&want, got));
            report.clips.push(Clip {
                name: path.file_stem().unwrap().to_string_lossy().into_owned(),
                audio_s,
                vad_s,
                asr_s: asr.1.as_secs_f64(),
                mt_s: mt.1.as_secs_f64(),
                load: r.busy / audio_s,
                final_delay_p50: r.final_delays.get(r.final_delays.len() / 2).copied().unwrap_or(0.0),
                final_delay_max: r.final_delays.last().copied().unwrap_or(0.0),
                partials_run: r.partials_run,
                partials_skipped: r.partials_skipped,
                cer: reference("txt", &source),
                tr_cer: reference("tr.txt", &translation),
                source,
                translation,
                embed_s: s.embed_s,
                speakers: s.speakers.speakers(),
                early_commits: s.early_commits,
                speaker_commits: s.speaker_commits,
                segment_s: s.segment_s,
            });
        }

        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let (ram, vram) = mem.join().unwrap();
        (report.ram_peak_mb, report.vram_peak_mb) = (Some(ram), vram);

        let base: Option<Report> = std::env::var("OVERLAYSUBS_BASELINE").ok().filter(|p| !p.is_empty()).map(|p| serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap());
        let pct = |v: Option<f64>| v.map_or("-".into(), |v| format!("{:.1}%", v * 100.0));
        let row = |name: &str, new: f64, old: Option<f64>| match old {
            Some(old) => eprintln!("  {name:<18}{old:>8.2} -> {new:>8.2}  ({:+.0}%)", (new / old - 1.0) * 100.0),
            None => eprintln!("  {name:<18}{new:>8.2}"),
        };
        for c in &report.clips {
            eprintln!("\n{} ({:.1} s, {} + {}, {})", c.name, c.audio_s, report.asr, report.mt, if gpu { "GPU" } else { "CPU" });
            let b = base.as_ref().and_then(|b| b.clips.iter().find(|b| b.name == c.name));
            row("load (x realtime)", c.load, b.map(|b| b.load));
            row("final delay p50 s", c.final_delay_p50, b.map(|b| b.final_delay_p50));
            row("final delay max s", c.final_delay_max, b.map(|b| b.final_delay_max));
            row("asr s", c.asr_s, b.map(|b| b.asr_s));
            row("translate s", c.mt_s, b.map(|b| b.mt_s));
            row("vad s", c.vad_s, b.map(|b| b.vad_s));
            if c.speakers > 0 {
                row("embed s", c.embed_s, b.map(|b| b.embed_s));
                row("speakers", c.speakers as f64, b.map(|b| b.speakers as f64));
                row("early commits", c.early_commits as f64, b.map(|b| b.early_commits as f64));
                row("speaker commits", c.speaker_commits as f64, b.map(|b| b.speaker_commits as f64));
                row("segment s", c.segment_s, b.map(|b| b.segment_s));
            }
            row("partials run", c.partials_run as f64, b.map(|b| b.partials_run as f64));
            row("partials skipped", c.partials_skipped as f64, b.map(|b| b.partials_skipped as f64));
            match b {
                Some(b) => {
                    eprintln!("  transcript CER    {} -> {}, changed from baseline {}", pct(b.cer), pct(c.cer), pct(Some(cer(&b.source, &c.source))));
                    eprintln!("  translation CER   {} -> {}, changed from baseline {}", pct(b.tr_cer), pct(c.tr_cer), pct(Some(cer(&b.translation, &c.translation))));
                }
                None => eprintln!("  transcript CER    {}\n  translation CER   {}", pct(c.cer), pct(c.tr_cer)),
            }
            eprintln!("  {}\n  -> {}", c.source, c.translation);
        }
        eprintln!("\nmemory, whole run");
        row("RAM peak MB", ram, base.as_ref().and_then(|b| b.ram_peak_mb));
        match vram {
            Some(v) => row("VRAM peak MB", v, base.as_ref().and_then(|b| b.vram_peak_mb)),
            None => eprintln!("  VRAM peak MB             -  (needs nvidia-smi)"),
        }
        if let Ok(out) = std::env::var("OVERLAYSUBS_OUT") {
            std::fs::write(out, serde_json::to_string_pretty(&report).unwrap()).unwrap();
        }
    }
}
