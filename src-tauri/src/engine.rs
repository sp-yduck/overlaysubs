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
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState, WhisperVadContext, WhisperVadContextParams, WhisperVadParams};

pub struct Whisper {
    ctx: WhisperContext,
    /// Reused across `transcribe` calls: creating one allocates the KV cache and compute buffers, and the
    /// state also caches the Silero model that `enable_vad` would otherwise reload from disk on every call.
    state: WhisperState,
    pub vad_path: String,
    threads: i32,
}

impl Whisper {
    pub fn load(model: &Path, vad: &Path, use_gpu: bool, threads: usize) -> Result<Self, String> {
        let mut p = WhisperContextParameters::default();
        p.use_gpu(use_gpu);
        let ctx = WhisperContext::new_with_params(model, p).map_err(|e| e.to_string())?;
        let state = ctx.create_state().map_err(|e| e.to_string())?;
        Ok(Self { ctx, state, vad_path: vad.to_string_lossy().into_owned(), threads: threads.min(8) as i32 })
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
        let st = &mut self.state;
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
        for ev in evs.into_iter().filter(|e| matches!(e, Event::Final(_))) {
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
    }

    /// Speed and quality of the whole pipeline on recorded clips, replayed as live audio.
    /// Run it with scripts/bench.sh, which documents the inputs. Env: OVERLAYSUBS_WAV (a 16 kHz mono WAV or a
    /// folder of them), OVERLAYSUBS_OUT (report path), OVERLAYSUBS_BASELINE (an earlier report to compare with),
    /// OVERLAYSUBS_ASR / OVERLAYSUBS_MT (model ids), OVERLAYSUBS_GPU=1.
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
                evs.extend(out.drain(..).map(|e| (pos as f64 / 16_000.0, matches!(e, Event::Final(_)), (e, pos))));
            }
            let vad_s = t0.elapsed().as_secs_f64();
            (asr.1, mt.1) = (Duration::ZERO, Duration::ZERO);
            let mut s = Session::new(SessionConfig::default());
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
