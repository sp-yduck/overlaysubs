#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod capture;
mod download;
mod engine;
mod hw;
mod update;

use capture::Source;
use overlaysubs_core::cache::HardwareCache;
use overlaysubs_core::device::Device;
use overlaysubs_core::endpoint::{EndpointConfig, Endpointer, Event};
use overlaysubs_core::export::{self, Entry};
use overlaysubs_core::registry::{Registry, Selection};
use overlaysubs_core::session::{Asr, Caption, Session, SessionConfig, Translator};
use overlaysubs_core::tier::{self, Hardware, Tier};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::menu::{CheckMenuItem, Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// Whether this build can use a GPU at all: the official builds enable one of these backends.
/// A build without one still launches, and `snapshot` publishes this so the settings UI can hide
/// the GPU choices instead of offering a switch that the build cannot honour.
const GPU_BUILD: bool = cfg!(any(feature = "vulkan", feature = "cuda", feature = "metal"));

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    models_dir: Option<PathBuf>,
    tier: Option<Tier>,
    selection: Option<Selection>,
    source: Source,
    session: SessionConfig,
    /// Where each model runs. Auto uses the GPU when this build has the backend and the hardware
    /// probe found one. VAD has no setting: whisper.cpp forces the GPU VAD off upstream.
    asr_device: Device,
    translator_device: Device,
    /// Written by builds before the per-model choice, parsed only so `migrate` can carry a CPU
    /// choice over; never serialized, so the keys disappear from settings.json on the next save.
    #[serde(default, skip_serializing)]
    whisper_gpu: Option<bool>,
    #[serde(default, skip_serializing)]
    gpu_layers: Option<u32>,
    hotkey: String,
    /// Empty until the UI picks one from the OS languages on first launch.
    ui_lang: String,
    /// Opaque to Rust: owned by the overlay/settings UI.
    overlay: serde_json::Value,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            models_dir: None,
            tier: None,
            selection: None,
            source: Source::System { device: None },
            session: SessionConfig::default(),
            asr_device: Device::Auto,
            translator_device: Device::Auto,
            whisper_gpu: None,
            gpu_layers: None,
            hotkey: "CmdOrCtrl+Shift+T".into(),
            ui_lang: String::new(),
            overlay: serde_json::json!({}),
        }
    }
}

impl Settings {
    /// Folds the removed `whisper_gpu` / `gpu_layers` settings into the per-model choice and drops
    /// them, so users upgrading from an older file keep the CPU choice they made. Idempotent: the
    /// new fields are left alone when the old ones are absent, and this runs again on every launch
    /// until the next settings save rewrites the file without them (a write here could fail or
    /// race the UI rewriting settings.json, so it is not done at startup).
    fn migrate(&mut self) {
        let (whisper_gpu, gpu_layers) = (self.whisper_gpu, self.gpu_layers);
        overlaysubs_core::device::apply_legacy(&mut self.asr_device, &mut self.translator_device, whisper_gpu, gpu_layers);
        self.whisper_gpu = None;
        self.gpu_layers = None;
    }
}

/// The model files and device settings the loaded engines were requested with. Kept next to them
/// so a settings change reloads the models, and stored as *requested* even when a GPU load fell
/// back to the CPU: recomputing it from what actually loaded would retry a broken or missing GPU
/// on every launch.
#[derive(PartialEq, Eq)]
struct EngineConfig {
    ids: Selection,
    asr_gpu: bool,
    gpu_layers: u32,
}

struct Engines {
    cfg: EngineConfig,
    asr: engine::Whisper,
    mt: engine::Plamo,
}

struct Running {
    _capture: capture::Capture,
    stop_tx: mpsc::Sender<()>,
    /// Identifies this session so a delayed hide cannot hide a newer session's overlay.
    id: u64,
    /// Joined after stop so the overlay is hidden only once the last utterance is shown.
    session: std::thread::JoinHandle<()>,
}

#[derive(Default)]
struct App {
    /// Shared with the session thread, which copies `session` from it before every utterance so
    /// that edits made on the Translate page apply without stopping and starting again.
    settings: Arc<Mutex<Settings>>,
    engines: Arc<Mutex<Option<Engines>>>,
    running: Mutex<Option<Running>>,
    history: Arc<Mutex<Vec<Entry>>>,
    /// Transient: the overlay starts hidden and is not persisted across launches.
    overlay_visible: AtomicBool,
    /// Serializes visibility changes so concurrent toggles apply in order.
    overlay_switch: Mutex<()>,
    /// Tray item whose check state mirrors `overlay_visible`.
    overlay_item: Mutex<Option<CheckMenuItem<tauri::Wry>>>,
    /// Bumped on every start; lets a stale session skip hiding a newer one.
    session_seq: AtomicU64,
    /// True while the appearance settings preview a sample caption in the overlay.
    overlay_preview: AtomicBool,
    /// Overlay visibility to restore when the preview ends (running or manually shown).
    preview_prev_visible: AtomicBool,
    /// A stopped session is waiting out its grace period before hiding the overlay.
    overlay_hide_pending: AtomicBool,
    /// Where tauri.conf.json put the overlay at launch (desktop units), before a saved position was applied.
    overlay_default_position: Mutex<Option<(f64, f64)>>,
    /// Set once the saved position and size were applied at launch. The window reports moves and resizes while it
    /// is being created, and saving those would replace the saved spot before it is read.
    overlay_geometry_restored: AtomicBool,
}

fn config_dir(app: &AppHandle) -> PathBuf {
    app.path().app_config_dir().expect("config dir")
}

fn models_dir(app: &AppHandle, s: &Settings) -> PathBuf {
    s.models_dir.clone().unwrap_or_else(|| app.path().app_data_dir().expect("data dir").join("models"))
}

/// A `models.json` in the config dir overrides the bundled one, so models can be added without an app update.
fn registry(app: &AppHandle) -> Result<Registry, String> {
    match std::fs::read_to_string(config_dir(app).join("models.json")) {
        Ok(s) => Registry::parse(&s),
        Err(_) => Registry::parse(include_str!("../../models.json")),
    }
}

/// Bump when the terms text changes; users are asked to accept again.
const TERMS_VERSION: &str = "4";

/// Kept outside settings.json so a settings reset or schema change never re-asks for consent.
fn terms_accepted(app: &AppHandle) -> bool {
    std::fs::read_to_string(config_dir(app).join("terms-accepted")).is_ok_and(|v| v.trim() == TERMS_VERSION)
}

#[tauri::command]
fn accept_terms(app: AppHandle) -> Result<(), String> {
    let dir = config_dir(&app);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("terms-accepted"), TERMS_VERSION).map_err(|e| e.to_string())
}

fn save(app: &AppHandle, s: &Settings) -> Result<(), String> {
    let dir = config_dir(app);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("settings.json"), serde_json::to_string_pretty(s).unwrap()).map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct Snapshot {
    settings: Settings,
    terms_accepted: bool,
    registry: Registry,
    downloaded: Vec<String>,
    running: bool,
    overlay_visible: bool,
    models_dir: PathBuf,
    /// `std::env::consts::OS`; the UI needs it because macOS system audio is captured as a
    /// whole and has no device to choose.
    platform: &'static str,
    devices: capture::Devices,
    /// Last "Check this PC" result from disk, so the model setup screen can show it again after a restart.
    hardware: Option<HardwareCache>,
    /// False in a build without a GPU backend; the settings UI then offers Auto / CPU only.
    gpu_build: bool,
}

#[tauri::command]
fn snapshot(app: AppHandle, st: State<App>) -> Result<Snapshot, String> {
    let settings = st.settings.lock().unwrap().clone();
    let registry = registry(&app)?;
    let dir = models_dir(&app, &settings);
    let downloaded = registry.models.iter().filter(|m| dir.join(&m.file).exists()).map(|m| m.id.clone()).collect();
    Ok(Snapshot {
        running: st.running.lock().unwrap().is_some(),
        overlay_visible: st.overlay_visible.load(Ordering::Relaxed),
        terms_accepted: terms_accepted(&app),
        settings,
        registry,
        downloaded,
        models_dir: dir,
        platform: std::env::consts::OS,
        devices: capture::devices(),
        hardware: hw::cached(&config_dir(&app)),
        gpu_build: GPU_BUILD,
    })
}

/// Check GitHub for a newer release; `None` when up to date or the release is unusable.
#[tauri::command]
async fn check_update(app: AppHandle) -> Result<Option<update::UpdateInfo>, String> {
    let current = app.package_info().version.to_string();
    tauri::async_runtime::spawn_blocking(move || update::fetch(&current)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn save_settings(app: AppHandle, st: State<App>, settings: Settings) -> Result<(), String> {
    let old = std::mem::replace(&mut *st.settings.lock().unwrap(), settings.clone());
    if old.hotkey != settings.hotkey {
        let gs = app.global_shortcut();
        let _ = gs.unregister(old.hotkey.as_str());
        gs.register(settings.hotkey.as_str()).map_err(|e| e.to_string())?;
    }
    save(&app, &settings)?;
    app.emit("settings", &settings).map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct Detected {
    hardware: Hardware,
    tier: Tier,
    selection: Selection,
}

/// Detects the hardware and caches it, so the setup screen can show the result on later launches.
#[tauri::command]
fn detect(app: AppHandle) -> Result<Detected, String> {
    let hardware = hw::detect(&config_dir(&app));
    let tier = tier::select_tier(&hardware);
    let selection = registry(&app)?.defaults(tier).ok_or("no models for tier")?;
    Ok(Detected { hardware, tier, selection })
}

#[tauri::command]
async fn download(app: AppHandle, ids: Vec<String>) -> Result<(), String> {
    let s = app.state::<App>().settings.lock().unwrap().clone();
    let reg = registry(&app)?;
    let dir = models_dir(&app, &s);
    tauri::async_runtime::spawn_blocking(move || {
        for id in ids {
            let m = reg.get(&id).ok_or(format!("unknown model {id}"))?;
            let mut last = 0;
            download::fetch(m, &dir, &mut |done, total| {
                if done - last > (1 << 22) || done == total {
                    last = done;
                    let _ = app.emit("download", serde_json::json!({ "id": id, "done": done, "total": total }));
                }
            })?;
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn delete_model(app: AppHandle, st: State<App>, id: String) -> Result<(), String> {
    let s = st.settings.lock().unwrap().clone();
    let m = registry(&app)?.get(&id).cloned().ok_or("unknown model")?;
    let mut eng = st.engines.lock().unwrap();
    if eng.as_ref().is_some_and(|e| [&e.cfg.ids.asr, &e.cfg.ids.translator, &e.cfg.ids.vad].contains(&&id)) {
        *eng = None;
    }
    std::fs::remove_file(models_dir(&app, &s).join(m.file)).map_err(|e| e.to_string())
}

/// Whether a GPU can be used: this build needs a GPU backend and the last hardware probe must have
/// found a GPU. No probe yet (a fresh install that has not opened the setup screen) counts as
/// available: a wrong guess only costs one failed load, which `load_engines` retries on the CPU,
/// and the probe is cached by then for later launches.
fn gpu_available(app: &AppHandle) -> bool {
    GPU_BUILD && hw::cached(&config_dir(app)).map_or(true, |c| c.hardware.gpu.is_some())
}

fn ensure_engines(app: &AppHandle, st: &App) -> Result<(), String> {
    let s = st.settings.lock().unwrap().clone();
    let sel = s.selection.clone().ok_or("no models selected")?;
    let gpu = gpu_available(app);
    let cfg = EngineConfig {
        ids: sel,
        asr_gpu: s.asr_device.uses_gpu(gpu),
        // llama.cpp takes a layer count rather than a flag: all layers on the GPU, or none.
        gpu_layers: if s.translator_device.uses_gpu(gpu) { 999 } else { 0 },
    };
    let mut eng = st.engines.lock().unwrap();
    // Compare the requested config: after a CPU fallback the loaded engines are on the CPU but a
    // machine whose GPU driver is broken must not retry the GPU load on every launch.
    if eng.as_ref().is_some_and(|e| e.cfg == cfg) {
        return Ok(());
    }
    *eng = None;
    let (asr, mt) = load_engines(app, &s, &cfg)?;
    *eng = Some(Engines { cfg, asr, mt });
    Ok(())
}

/// Loads both models, retrying everything on the CPU when a GPU load fails: a machine with no
/// usable GPU (no driver, basic display adapter, a VM) must still get subtitles, and a load error
/// would otherwise leave the app unable to start translating at all. Both are retried rather than
/// the one that failed: the first engine may already hold a GPU context on the backend that just
/// failed, and a half-GPU/half-CPU pair is harder to reason about than the documented CPU setup.
fn load_engines(app: &AppHandle, s: &Settings, cfg: &EngineConfig) -> Result<(engine::Whisper, engine::Plamo), String> {
    let reg = registry(app)?;
    let dir = models_dir(app, s);
    let path = |id: &str| reg.get(id).map(|m| dir.join(&m.file)).filter(|p| p.exists()).ok_or(format!("model {id} not downloaded"));
    let (asr, vad, mt) = (path(&cfg.ids.asr)?, path(&cfg.ids.vad)?, path(&cfg.ids.translator)?);
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let load = |asr_gpu: bool, gpu_layers: u32| {
        let asr = engine::Whisper::load(&asr, &vad, asr_gpu, threads)?;
        let mt = engine::Plamo::load(&mt, gpu_layers, threads)?;
        Ok::<_, String>((asr, mt))
    };
    match load(cfg.asr_gpu, cfg.gpu_layers) {
        Ok(engines) => Ok(engines),
        Err(gpu_err) if cfg.asr_gpu || cfg.gpu_layers > 0 => {
            eprintln!("loading the models with the GPU failed ({gpu_err}); retrying on the CPU");
            load(false, 0).map_err(|cpu_err| format!("GPU load failed ({gpu_err}), and the CPU retry failed too ({cpu_err})"))
        }
        Err(err) => Err(err),
    }
}

#[derive(Serialize)]
struct Bench {
    rtf: f32,
    downgraded_to: Option<Detected>,
}

/// Time ASR + translation on a fixed clip; suggest one tier lower if too slow.
#[tauri::command]
async fn benchmark(app: AppHandle) -> Result<Bench, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<App>();
        ensure_engines(&app, &st)?;
        // Read the tier before locking the engines. The session thread takes the settings lock and
        // then the engines lock, so keeping that order here avoids an inversion if a session starts
        // while the benchmark runs.
        let cur = st.settings.lock().unwrap().tier.unwrap_or(Tier::Mid);
        let mut eng = st.engines.lock().unwrap();
        let e = eng.as_mut().unwrap();
        let secs = 5.0;
        let clip: Vec<f32> = (0..(16_000.0 * secs) as usize).map(|i| (i as f32 * 0.05).sin() * 0.2).collect();
        let t0 = Instant::now();
        e.asr.bench(&clip)?;
        let req = overlaysubs_core::pfml::TranslateRequest {
            text: "The meeting has been moved to next Monday, so please update your calendar.",
            source_lang: Some("en"),
            target_lang: "ja",
            ..Default::default()
        };
        e.mt.translate(&req, &mut |_| {})?;
        let took = t0.elapsed().as_secs_f32();
        let downgraded_to = match (tier::too_slow(took, secs), cur.lower()) {
            (true, Some(t)) => Some(Detected { hardware: hw::cached_or_detect(&config_dir(&app)), tier: t, selection: registry(&app)?.defaults(t).unwrap() }),
            _ => None,
        };
        Ok(Bench { rtf: took / secs, downgraded_to })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn start(app: AppHandle, st: State<App>) -> Result<(), String> {
    if st.running.lock().unwrap().is_some() {
        return Ok(());
    }
    if !terms_accepted(&app) {
        return Err("terms not accepted".into());
    }
    ensure_engines(&app, &st)?;
    let s = st.settings.lock().unwrap().clone();
    let (audio_tx, audio_rx) = mpsc::channel::<Vec<f32>>();
    let (ev_tx, ev_rx) = mpsc::channel::<(Event, usize)>();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let mut vad = engine::Silero::load(&st.engines.lock().unwrap().as_ref().unwrap().asr.vad_path)?;
    let cap = capture::start(&s.source, audio_tx)?;
    let vad_app = app.clone();
    std::thread::spawn(move || {
        let mut ep = Endpointer::new(EndpointConfig::default());
        let mut out = Vec::new();
        let mut pos = 0;
        for chunk in audio_rx {
            pos += chunk.len();
            if let Err(err) = ep.push(&chunk, &mut vad, &mut out) {
                let _ = vad_app.emit("error", err);
                return;
            }
            for e in out.drain(..) {
                if ev_tx.send((e, pos)).is_err() {
                    return;
                }
            }
        }
    });
    let engines = st.engines.clone();
    let history = st.history.clone();
    let live_settings = st.settings.clone();
    history.lock().unwrap().clear();
    let emit_app = app.clone();
    let session_id = st.session_seq.fetch_add(1, Ordering::SeqCst) + 1;
    let session_thread = std::thread::spawn(move || {
        let mut session = Session::new(s.session);
        let mut seen = 0;
        let mut emit = |c: Caption| {
            let _ = emit_app.emit("caption", &c);
        };
        loop {
            let stopping = stop_rx.try_recv().is_ok();
            let next = if stopping { None } else { ev_rx.recv_timeout(std::time::Duration::from_millis(200)).ok() };
            let Some(first) = next else {
                if stopping {
                    return;
                }
                continue;
            };
            // The Translate page saves settings while translating. Copy them before the engines
            // are locked below so a language, style, topic or glossary change applies to the next
            // utterance without a restart. The guard is a temporary of the statement, so the
            // settings lock is never held while waiting for the engines (the order ensure_engines
            // and the benchmark use). A poisoned lock must not freeze the settings for the rest of
            // the session either.
            session.cfg = live_settings.lock().unwrap_or_else(|e| e.into_inner()).session.clone();
            // Under load, skip stale partials: keep every final and only the newest partial.
            let batch: Vec<_> = std::iter::once(first).chain(ev_rx.try_iter()).collect();
            let n = batch.len();
            let batch = batch.into_iter().enumerate().filter(|(i, (e, _))| matches!(e, Event::Final(_)) || *i == n - 1).map(|(_, x)| x);
            for (ev, pos) in batch {
                session.tick(pos - seen);
                seen = pos;
                let mut g = engines.lock().unwrap();
                let Some(e) = g.as_mut() else { return };
                let (asr, mt): (&mut dyn Asr, &mut dyn Translator) = (&mut e.asr, &mut e.mt);
                if let Err(err) = session.handle(ev, asr, mt, &mut emit) {
                    let _ = emit_app.emit("error", err);
                }
            }
            *history.lock().unwrap() = session.entries.clone();
        }
    });
    *st.running.lock().unwrap() = Some(Running { _capture: cap, stop_tx, id: session_id, session: session_thread });
    st.overlay_hide_pending.store(false, Ordering::SeqCst);
    set_overlay_visible(&app, true);
    app.emit("running", true).map_err(|e| e.to_string())
}

#[tauri::command]
fn stop(app: AppHandle, st: State<App>) -> Result<(), String> {
    // Bind first so the `running` mutex guard is released before any window work.
    let running = st.running.lock().unwrap().take();
    let Some(running) = running else {
        // Not translating: make sure a manually shown overlay does not linger, but keep it
        // while the appearance settings are previewing a sample caption.
        if !st.overlay_preview.load(Ordering::SeqCst) {
            set_overlay_visible(&app, false);
        }
        return app.emit("running", false).map_err(|e| e.to_string());
    };
    let Running { _capture, stop_tx, id, session } = running;
    let _ = stop_tx.send(());
    drop(_capture);
    // The session thread may still be translating the last utterance after stop; hide only once it is
    // done (plus a short grace period) so the final subtitle is actually shown.
    st.overlay_hide_pending.store(true, Ordering::SeqCst);
    let hide_app = app.clone();
    std::thread::spawn(move || {
        let _ = session.join();
        std::thread::sleep(std::time::Duration::from_millis(1500));
        hide_overlay_if_session(&hide_app, id);
    });
    app.emit("running", false).map_err(|e| e.to_string())
}

fn toggle(app: &AppHandle) {
    let st = app.state::<App>();
    let running = st.running.lock().unwrap().is_some();
    let r = if running { stop(app.clone(), st) } else { start(app.clone(), st) };
    if let Err(e) = r {
        let _ = app.emit("error", e);
    }
}

/// The single entry point for overlay visibility: window, tray check state, overlay
/// webview and the settings UI all stay in sync.
fn set_overlay_visible(app: &AppHandle, show: bool) {
    {
        let st = app.state::<App>();
        let _switch = st.overlay_switch.lock().unwrap();
        apply_overlay_visible(app, show);
    }
    sync_overlay_check(app);
}

fn toggle_overlay(app: &AppHandle) {
    let st = app.state::<App>();
    let show = {
        let _switch = st.overlay_switch.lock().unwrap();
        let show = !st.overlay_visible.load(Ordering::SeqCst);
        apply_overlay_visible(app, show);
        show
    };
    sync_overlay_check(app);
    // A manual toggle during the preview decides what stays visible afterwards.
    if st.overlay_preview.load(Ordering::SeqCst) {
        st.preview_prev_visible.store(show, Ordering::SeqCst);
    }
}

/// Hides the overlay only if `session_id` is still the current session, so a stop that
/// overlaps a quick restart does not hide the new session's overlay.
fn hide_overlay_if_session(app: &AppHandle, session_id: u64) {
    {
        let st = app.state::<App>();
        let _switch = st.overlay_switch.lock().unwrap();
        if st.session_seq.load(Ordering::SeqCst) != session_id {
            return;
        }
        st.overlay_hide_pending.store(false, Ordering::SeqCst);
        // Keep the overlay up while the appearance settings preview a sample caption; the
        // preview restores the visibility when it ends.
        if st.overlay_preview.load(Ordering::SeqCst) {
            return;
        }
        apply_overlay_visible(app, false);
    }
    sync_overlay_check(app);
}

/// Window, state and events. Callers must hold `overlay_switch`.
fn apply_overlay_visible(app: &AppHandle, show: bool) {
    let st = app.state::<App>();
    match app.get_webview_window("overlay") {
        Some(w) => {
            let res = if show { w.show() } else { w.hide() };
            if let Err(e) = res {
                eprintln!("overlay visibility ({show}): {e}");
            }
        }
        None => eprintln!("overlay window not found"),
    }
    st.overlay_visible.store(show, Ordering::SeqCst);
    if !show {
        // A hidden webview has its timers throttled, so clear captions as it disappears
        // rather than on the next show (which could discard a caption emitted in between).
        let _ = app.emit_to("overlay", "overlay-hidden", ());
    }
    let _ = app.emit("overlay-visible", show);
}

/// `set_checked` blocks on the main thread, so it must run after `overlay_switch` is released
/// to keep a tray click and a hotkey press from deadlocking.
fn sync_overlay_check(app: &AppHandle) {
    let st = app.state::<App>();
    let show = st.overlay_visible.load(Ordering::SeqCst);
    let item = st.overlay_item.lock().unwrap().clone();
    if let Some(item) = item {
        let _ = item.set_checked(show);
    }
}

#[tauri::command]
fn history(st: State<App>) -> Vec<Entry> {
    st.history.lock().unwrap().clone()
}

#[tauri::command]
fn export_history(st: State<App>, path: PathBuf, format: String, with_source: bool) -> Result<(), String> {
    let h = st.history.lock().unwrap();
    let body = match format.as_str() {
        "srt" => export::to_srt(&h, with_source),
        _ => export::to_txt(&h),
    };
    std::fs::write(path, body).map_err(|e| e.to_string())
}

/// OverlaySubs terms. The PLaMo Community License is linked from the terms, not bundled.
#[tauri::command]
fn terms_text(lang: String) -> String {
    if lang == "ja" {
        include_str!("../../licenses/overlaysubs-terms.ja.txt").to_string()
    } else {
        include_str!("../../licenses/overlaysubs-terms.en.txt").to_string()
    }
}

/// Open an https URL in the user's default browser.
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("only https URLs can be opened".into());
    }
    #[cfg(target_os = "windows")]
    let status = {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url.as_str()])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .status()
    };
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open").arg(&url).status();
    #[cfg(all(unix, not(target_os = "macos")))]
    let status = std::process::Command::new("xdg-open").arg(&url).status();
    status.map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_click_through(app: AppHandle, on: bool) -> Result<(), String> {
    let w = app.get_webview_window("overlay").ok_or("no overlay")?;
    w.set_ignore_cursor_events(on).map_err(|e| e.to_string())
}

/// Where the overlay was last moved, in desktop units (see `DESKTOP_IN_POINTS`), and the size it was last resized to,
/// in logical pixels: `{"x": .., "y": .., "width": .., "height": ..}`. Kept out of settings.json, which the settings
/// window rewrites whole.
const OVERLAY_POSITION: &str = "overlay-position.json";

/// The smallest saved size that is applied (logical pixels). A smaller one would leave nothing to grab or read.
const OVERLAY_MIN_SIZE: (f64, f64) = (120.0, 40.0);

/// The unit of the global desktop coordinates, which all overlay placement uses. macOS lays the desktop out in points,
/// so a physical value is divided by the scale of the display it is on. Windows (and X11) lay it out in physical
/// pixels across displays with different scales, and there tao converts a logical position with the scale of the
/// display the window is on now, so a logical position would land on the wrong display.
const DESKTOP_IN_POINTS: bool = cfg!(target_os = "macos");

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum SavedPosition {
    Desktop {
        x: f64,
        y: f64,
        /// Logical size. Missing in files written before the size was saved.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        height: Option<f64>,
    },
    /// Physical pixels `[x, y]`, written by #42 before this format.
    Physical(i32, i32),
}

/// A display's bounds or work area, or the overlay's bounds, in desktop units: x, y, width, height.
type Rect = (f64, f64, f64, f64);

/// A connected display: its whole bounds and its work area (without the menu bar and Dock or taskbar).
struct Display {
    bounds: Rect,
    work_area: Rect,
    /// Turns the overlay's logical size into desktop units on this display: its scale where the desktop is in
    /// physical pixels (the window is rescaled when it moves there), 1 where the desktop is in points.
    size_scale: f64,
}

/// Physical pixels to desktop units, for a value on a display (or window) with `scale`.
fn to_desktop(v: f64, scale: f64, in_points: bool) -> f64 {
    if in_points { v / scale } else { v }
}

fn desktop_position((x, y): (f64, f64)) -> tauri::Position {
    if DESKTOP_IN_POINTS {
        tauri::LogicalPosition::new(x, y).into()
    } else {
        tauri::PhysicalPosition::new(x.round() as i32, y.round() as i32).into()
    }
}

/// Saves where the overlay is and how large it is, after a move (`moved` is the new position) or a resize.
fn save_overlay_geometry(w: &tauri::Window, moved: Option<tauri::PhysicalPosition<i32>>) {
    if !w.state::<App>().overlay_geometry_restored.load(Ordering::SeqCst) {
        return;
    }
    let Ok(scale) = w.scale_factor() else { return };
    let Some(p) = moved.or_else(|| w.outer_position().ok()) else { return };
    let at = |v: i32| to_desktop(v as f64, scale, DESKTOP_IN_POINTS);
    // The logical size stays the same when the overlay moves to a display with another scale.
    // A window without a usable size is minimized or collapsed; where it is then is not worth keeping either.
    let Some((width, height)) = w.inner_size().ok().map(|s| s.to_logical::<f64>(scale)).and_then(|s| usable_size(Some(s.width), Some(s.height))) else { return };
    let json = serde_json::to_string(&SavedPosition::Desktop { x: at(p.x), y: at(p.y), width: Some(width), height: Some(height) }).unwrap();
    let dir = config_dir(w.app_handle());
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(dir.join(OVERLAY_POSITION), json)) {
        eprintln!("{OVERLAY_POSITION}: {e}");
    }
}

/// Back to where the overlay started at launch, kept fully on a display; centered on the primary display when that
/// spot is on no display any more (the display it was on was unplugged, say). The move is saved like a drag.
/// `center()` is not used: it put the overlay higher than the launch position (measured on macOS).
#[tauri::command]
fn reset_overlay_position(app: AppHandle, st: State<App>) -> Result<(), String> {
    let w = app.get_webview_window("overlay").ok_or("no overlay")?;
    let size = overlay_size(&w).ok_or("no overlay size")?;
    let launch = *st.overlay_default_position.lock().unwrap();
    let p = launch
        .and_then(|p| fit_on_screen(p, size, &displays(&w)))
        .or_else(|| w.primary_monitor().ok().flatten().map(|m| centered(size, &display(&m))))
        .ok_or("no display for the overlay")?;
    w.set_position(desktop_position(p)).map_err(|e| e.to_string())
}

/// A saved size that can be applied: both sides present, finite and not too small to use.
fn usable_size(width: Option<f64>, height: Option<f64>) -> Option<(f64, f64)> {
    let (w, h) = (width?, height?);
    (w.is_finite() && h.is_finite() && w >= OVERLAY_MIN_SIZE.0 && h >= OVERLAY_MIN_SIZE.1).then_some((w, h))
}

/// Applies the saved size and position, and lets later moves and resizes be saved.
fn restore_overlay_geometry(app: &AppHandle) {
    restore_saved_overlay_geometry(app);
    app.state::<App>().overlay_geometry_restored.store(true, Ordering::SeqCst);
}

/// Applies the saved size, then the saved position moved so the whole overlay is on a display. Left at the launch
/// position when the saved spot overlaps no display (an external display was unplugged, say), since the overlay would
/// then sit out of sight.
fn restore_saved_overlay_geometry(app: &AppHandle) {
    let Some(w) = app.get_webview_window("overlay") else { return };
    if let (Ok(p), Ok(scale)) = (w.outer_position(), w.scale_factor()) {
        let at = |v: i32| to_desktop(v as f64, scale, DESKTOP_IN_POINTS);
        *app.state::<App>().overlay_default_position.lock().unwrap() = Some((at(p.x), at(p.y)));
    }
    let Ok(j) = std::fs::read_to_string(config_dir(app).join(OVERLAY_POSITION)) else { return };
    let saved = match serde_json::from_str::<SavedPosition>(&j) {
        Ok(SavedPosition::Desktop { x, y, width, height }) => {
            // Before the position, which is fitted to a display with the size the overlay will have.
            if let Some((width, height)) = usable_size(width, height) {
                let _ = w.set_size(tauri::LogicalSize::new(width, height));
            }
            (x, y)
        }
        Ok(SavedPosition::Physical(x, y)) => legacy_to_desktop(&w, x, y),
        Err(_) => return,
    };
    let Some(size) = overlay_size(&w) else { return };
    if let Some(p) = fit_on_screen(saved, size, &displays(&w)) {
        let _ = w.set_position(desktop_position(p));
    }
}

/// On macOS, converts with the scale of the display that holds the point (falling back to the overlay's own).
/// Elsewhere physical pixels already are desktop units.
fn legacy_to_desktop(w: &tauri::WebviewWindow, x: i32, y: i32) -> (f64, f64) {
    let scale = w
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .find(|m| {
            let (o, s) = (m.position(), m.size());
            x >= o.x && y >= o.y && x < o.x + s.width as i32 && y < o.y + s.height as i32
        })
        .map(|m| m.scale_factor())
        .or_else(|| w.scale_factor().ok())
        .unwrap_or(1.0);
    (to_desktop(x as f64, scale, DESKTOP_IN_POINTS), to_desktop(y as f64, scale, DESKTOP_IN_POINTS))
}

/// The logical size, which stays the same on every display (the physical size changes with the display's scale).
fn overlay_size(w: &tauri::WebviewWindow) -> Option<(f64, f64)> {
    let s = w.outer_size().ok()?.to_logical::<f64>(w.scale_factor().ok()?);
    Some((s.width, s.height))
}

fn displays(w: &tauri::WebviewWindow) -> Vec<Display> {
    w.available_monitors().unwrap_or_default().iter().map(display).collect()
}

fn display(m: &tauri::Monitor) -> Display {
    let (a, wa) = (m.position(), m.work_area());
    let px = |p: &tauri::PhysicalPosition<i32>, s: &tauri::PhysicalSize<u32>| (p.x as f64, p.y as f64, s.width as f64, s.height as f64);
    display_in_desktop(px(a, m.size()), px(&wa.position, &wa.size), m.scale_factor(), DESKTOP_IN_POINTS)
}

/// A display from its physical bounds and work area.
fn display_in_desktop(bounds: Rect, work_area: Rect, scale: f64, in_points: bool) -> Display {
    let r = |(x, y, w, h): Rect| (to_desktop(x, scale, in_points), to_desktop(y, scale, in_points), to_desktop(w, scale, in_points), to_desktop(h, scale, in_points));
    Display { bounds: r(bounds), work_area: r(work_area), size_scale: if in_points { 1.0 } else { scale } }
}

/// Where the overlay at `(x, y)` with logical `size` goes so all of it shows. It stays put when it is fully inside the
/// work area of the display that holds its top-left corner, measured with its size on that display. Otherwise it moves
/// into the work area of the display it overlaps most (its top-left corner when it is larger than the area), measured
/// with its size there. Overlaps are compared with one rectangle in desktop units: the overlay at the scale of the
/// display that holds its top-left corner (the logical size when no display does). Sizes scaled per display would make
/// a larger-scale neighbour look like it holds more of the overlay, and the bare logical size is too small where the
/// desktop is in physical pixels. A spot over the Dock or menu bar still counts as on that display. None when it
/// overlaps no display.
fn fit_on_screen((x, y): (f64, f64), size: (f64, f64), displays: &[Display]) -> Option<(f64, f64)> {
    let on = |d: &Display| (size.0 * d.size_scale, size.1 * d.size_scale);
    let inside = |a: Rect, (w, h): (f64, f64)| x >= a.0 && y >= a.1 && x + w <= a.0 + a.2 && y + h <= a.1 + a.3;
    let holds_corner = |a: Rect| x >= a.0 && y >= a.1 && x < a.0 + a.2 && y < a.1 + a.3;
    let corner = displays.iter().find(|d| holds_corner(d.bounds));
    if corner.is_some_and(|d| inside(d.work_area, on(d))) {
        return Some((x, y));
    }
    let (w, h) = corner.map_or(size, on);
    let overlap = |a: &Rect| ((x + w).min(a.0 + a.2) - x.max(a.0)).max(0.0) * ((y + h).min(a.1 + a.3) - y.max(a.1)).max(0.0);
    let d = displays.iter().filter(|d| overlap(&d.bounds) > 0.0).max_by(|a, b| overlap(&a.bounds).total_cmp(&overlap(&b.bounds)))?;
    let ((w, h), a) = (on(d), d.work_area);
    Some((x.min(a.0 + a.2 - w).max(a.0), y.min(a.1 + a.3 - h).max(a.1)))
}

/// The overlay with logical `size` centered in the work area of `d`.
fn centered(size: (f64, f64), d: &Display) -> (f64, f64) {
    let (w, h, a) = (size.0 * d.size_scale, size.1 * d.size_scale, d.work_area);
    (a.0 + (a.2 - w) / 2.0, a.1 + (a.3 - h) / 2.0)
}

/// Show or hide the overlay window from the settings UI.
#[tauri::command]
fn show_overlay(app: AppHandle, st: State<App>, show: bool) {
    set_overlay_visible(&app, show);
    // A manual toggle during the preview decides what stays visible afterwards.
    if st.overlay_preview.load(Ordering::SeqCst) {
        st.preview_prev_visible.store(show, Ordering::SeqCst);
    }
}

/// Show a sample caption in the overlay while the appearance settings are open, so font size,
/// font, opacity and the like can be checked even when translation is stopped and there is no text.
/// The overlay window is shown for the preview (it is normally hidden when stopped) and the
/// previous visibility is restored when the preview ends.
#[tauri::command]
fn set_overlay_preview(app: AppHandle, st: State<App>, on: bool) {
    if on {
        if !st.overlay_preview.swap(true, Ordering::SeqCst) {
            // Remember what to restore. A visibility that a just-stopped session would have
            // hidden after its grace period does not count as manually shown.
            let visible = st.overlay_visible.load(Ordering::SeqCst);
            let hiding = st.overlay_hide_pending.load(Ordering::SeqCst);
            st.preview_prev_visible
                .store(visible && !hiding, Ordering::SeqCst);
        }
        set_overlay_visible(&app, true);
        let _ = app.emit_to("overlay", "overlay-preview", true);
    } else {
        st.overlay_preview.store(false, Ordering::SeqCst);
        let _ = app.emit_to("overlay", "overlay-preview", false);
        let running = st.running.lock().unwrap().is_some();
        if !running && !st.preview_prev_visible.load(Ordering::SeqCst) {
            st.overlay_hide_pending.store(false, Ordering::SeqCst);
            set_overlay_visible(&app, false);
        }
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _, ev| {
                    if ev.state() == ShortcutState::Pressed {
                        toggle(app);
                    }
                })
                .build(),
        )
        .manage(App::default())
        .setup(|app| {
            let h = app.handle().clone();
            let path = config_dir(&h).join("settings.json");
            let mut s: Settings = match std::fs::read_to_string(&path) {
                Err(_) => Settings::default(),
                Ok(j) => serde_json::from_str(&j).unwrap_or_else(|e| {
                    // Keep the unreadable file instead of silently overwriting it on the next save.
                    eprintln!("settings.json: {e}; moved to settings.json.bak");
                    let _ = std::fs::rename(&path, path.with_extension("json.bak"));
                    Settings::default()
                }),
            };
            // One-time: turn the removed GPU settings into the per-model choice before anything uses them.
            s.migrate();
            if let Err(e) = h.global_shortcut().register(s.hotkey.as_str()) {
                eprintln!("hotkey {}: {e}", s.hotkey);
            }
            *app.state::<App>().settings.lock().unwrap() = s;
            // The windows are created here and not by Tauri (`"create": false` in tauri.conf.json), which would do
            // it before this hook. On Windows, creating a webview runs the event loop, so the settings window
            // could load while the overlay was still being created, read the default settings that were in place
            // until the line above, and save them over settings.json (#64).
            for config in &app.config().app.windows {
                tauri::WebviewWindowBuilder::from_config(app, config)?.build()?;
            }
            restore_overlay_geometry(&h);

            let toggle_i = MenuItem::with_id(app, "toggle", "Start / Stop", true, None::<&str>)?;
            let overlay_i = CheckMenuItem::with_id(app, "overlay", "Show subtitles", true, false, None::<&str>)?;
            *app.state::<App>().overlay_item.lock().unwrap() = Some(overlay_i.clone());
            let show_i = MenuItem::with_id(app, "show", "Settings", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("OverlaySubs — Built with PLaMo")
                .menu(&Menu::with_items(app, &[&toggle_i, &overlay_i, &show_i, &quit_i])?)
                .on_menu_event(|app, e| match e.id.as_ref() {
                    "toggle" => toggle(app),
                    "overlay" => toggle_overlay(app),
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|w, e| {
            // Closing settings quits the app so the overlay does not linger.
            match e {
                tauri::WindowEvent::CloseRequested { .. } if w.label() == "main" => w.app_handle().exit(0),
                tauri::WindowEvent::Moved(p) if w.label() == "overlay" => save_overlay_geometry(w, Some(*p)),
                tauri::WindowEvent::Resized(_) if w.label() == "overlay" => save_overlay_geometry(w, None),
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            snapshot, save_settings, detect, download, delete_model, benchmark, start, stop, history, export_history, terms_text, accept_terms, set_click_through, reset_overlay_position, open_url, show_overlay, set_overlay_preview, check_update
        ])
        .build(tauri::generate_context!())
        .expect("error while building overlaysubs")
        .run(|app, event| {
            // Free the models before the process exits. ggml's Metal backend aborts in its exit-time cleanup
            // (ggml_metal_rsets_free) while a loaded model still holds GPU buffers, so every quit crashed.
            if let tauri::RunEvent::Exit = event {
                app.state::<App>().engines.lock().unwrap().take();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A display whose work area is all of it.
    const fn whole(r: Rect) -> Display {
        Display { bounds: r, work_area: r, size_scale: 1.0 }
    }
    const HD: Display = whole((0.0, 0.0, 1920.0, 1080.0));
    const OVERLAY: (f64, f64) = (1100.0, 220.0);

    fn fit(x: f64, y: f64, displays: &[Display]) -> Option<(f64, f64)> {
        fit_on_screen((x, y), OVERLAY, displays)
    }

    #[test]
    fn overlay_inside_a_display_stays_put() {
        assert_eq!(fit(400.0, 600.0, &[HD]), Some((400.0, 600.0)));
    }

    #[test]
    fn overlay_past_an_edge_is_pulled_fully_on_screen() {
        assert_eq!(fit(400.0, 1070.0, &[HD]), Some((400.0, 860.0)), "top-left on screen but the subtitles below the bottom edge");
        assert_eq!(fit(1800.0, 300.0, &[HD]), Some((820.0, 300.0)), "right edge");
        assert_eq!(fit(-500.0, -100.0, &[HD]), Some((0.0, 0.0)), "left and top edges");
    }

    #[test]
    fn overlay_goes_to_the_display_it_overlaps_most() {
        let displays = [HD, whole((1920.0, 0.0, 1440.0, 900.0))];
        assert_eq!(fit(1800.0, 100.0, &displays), Some((1920.0, 100.0)));
        assert_eq!(fit(1000.0, 100.0, &displays), Some((820.0, 100.0)));
    }

    #[test]
    fn overlay_over_the_dock_is_pulled_into_the_work_area() {
        // 1728x1117 MacBook display: menu bar above y = 33, Dock below y = 1036.
        let mac = Display { bounds: (0.0, 0.0, 1728.0, 1117.0), work_area: (0.0, 33.0, 1728.0, 1003.0), size_scale: 1.0 };
        assert_eq!(fit_on_screen((200.0, 1100.0), (1100.0, 221.0), &[mac]), Some((200.0, 815.0)));
    }

    /// 1x primary display and a 2x display to its right, as tao reports them (physical pixels).
    fn mixed_scale(in_points: bool) -> [Display; 2] {
        [
            display_in_desktop((0.0, 0.0, 1920.0, 1080.0), (0.0, 0.0, 1920.0, 1040.0), 1.0, in_points),
            display_in_desktop((1920.0, 0.0, 3840.0, 2160.0), (1920.0, 0.0, 3840.0, 2120.0), 2.0, in_points),
        ]
    }

    #[test]
    fn mixed_scale_displays_in_physical_desktop_units_do_not_overlap() {
        // Windows: a spot saved on the 2x display stays there, wherever the overlay starts.
        let d = mixed_scale(false);
        assert_eq!((d[1].bounds.0, d[1].bounds.2), (1920.0, 3840.0));
        assert_eq!(fit_on_screen((2400.0, 200.0), OVERLAY, &d), Some((2400.0, 200.0)));
        // Divided by each display's own scale, the 2x display would start at 960 and overlap the 1x one.
        assert_eq!(mixed_scale(true)[1].bounds.0, 960.0);
    }

    /// A 200% primary display and a 100% display to its right, in physical pixels.
    fn primary_200_secondary_100() -> [Display; 2] {
        [
            display_in_desktop((0.0, 0.0, 3840.0, 2160.0), (0.0, 0.0, 3840.0, 2080.0), 2.0, false),
            display_in_desktop((3840.0, 0.0, 1920.0, 1080.0), (3840.0, 0.0, 1920.0, 1040.0), 1.0, false),
        ]
    }

    #[test]
    fn overlay_fully_on_a_display_stays_next_to_a_larger_scale_one() {
        // At 2200x440, the 2x neighbour would cover more of the overlay than the 1x display that really holds it.
        assert_eq!(fit_on_screen((800.0, 200.0), OVERLAY, &mixed_scale(false)), Some((800.0, 200.0)));
    }

    #[test]
    fn overlay_across_two_2x_displays_goes_to_the_one_that_holds_more_of_it() {
        // Side by side at 200%: at (3100, 200) the overlay is 2200 pixels wide, 740 on the left display and 1460 on
        // the right one. With the bare logical width (1100) the right display would seem to hold only 360.
        let d = [
            display_in_desktop((0.0, 0.0, 3840.0, 2160.0), (0.0, 0.0, 3840.0, 2080.0), 2.0, false),
            display_in_desktop((3840.0, 0.0, 3840.0, 2160.0), (3840.0, 0.0, 3840.0, 2080.0), 2.0, false),
        ];
        assert_eq!(fit_on_screen((3100.0, 200.0), OVERLAY, &d), Some((3840.0, 200.0)));
    }

    #[test]
    fn overlay_is_measured_at_the_scale_of_the_display_it_goes_to() {
        let d = primary_200_secondary_100();
        // Saved on the 100% display, restored while the overlay starts on the 200% one (2200x440 there).
        // At 100% it is 1100x220, so the spot is valid and must not move.
        assert_eq!(fit_on_screen((4500.0, 700.0), OVERLAY, &d), Some((4500.0, 700.0)));
        // The other way round: on the 200% display the overlay is 2200x440, so it is pulled up to fit.
        assert_eq!(fit_on_screen((1000.0, 1800.0), OVERLAY, &d), Some((1000.0, 1640.0)));
    }

    #[test]
    fn reset_centers_with_the_size_on_the_primary_display() {
        let d = primary_200_secondary_100();
        assert_eq!(centered(OVERLAY, &d[0]), (820.0, 820.0));
        assert_eq!(centered(OVERLAY, &d[1]), (4250.0, 410.0));
    }

    #[test]
    fn macos_displays_are_in_points() {
        let mac = display_in_desktop((0.0, 0.0, 3456.0, 2234.0), (0.0, 66.0, 3456.0, 2006.0), 2.0, true);
        assert_eq!((mac.bounds, mac.work_area), ((0.0, 0.0, 1728.0, 1117.0), (0.0, 33.0, 1728.0, 1003.0)));
    }

    #[test]
    fn overlay_on_no_display_is_not_placed() {
        assert_eq!(fit(-50000.0, -50000.0, &[HD]), None);
        assert_eq!(fit(2500.0, 100.0, &[HD]), None, "the display it was on was unplugged");
    }

    #[test]
    fn overlay_larger_than_the_work_area_keeps_its_top_left_on_screen() {
        assert_eq!(fit(100.0, 100.0, &[whole((0.0, 0.0, 800.0, 600.0))]), Some((0.0, 100.0)));
    }

    #[test]
    fn centered_on_a_work_area() {
        assert_eq!(centered(OVERLAY, &whole((0.0, 25.0, 1920.0, 1055.0))), (410.0, 442.5));
    }

    #[test]
    fn saved_position_reads_desktop_and_earlier_physical_files() {
        assert!(matches!(serde_json::from_str::<SavedPosition>(r#"{"x":200.5,"y":600}"#), Ok(SavedPosition::Desktop { x, y, width: None, height: None }) if x == 200.5 && y == 600.0));
        assert!(matches!(
            serde_json::from_str::<SavedPosition>(r#"{"x":200.5,"y":600,"width":900,"height":300.5}"#),
            Ok(SavedPosition::Desktop { width: Some(w), height: Some(h), .. }) if w == 900.0 && h == 300.5
        ));
        // A position saved without a size is written as before, so an older version still reads the file.
        assert_eq!(serde_json::to_string(&SavedPosition::Desktop { x: 1.0, y: 2.0, width: None, height: None }).unwrap(), r#"{"x":1.0,"y":2.0}"#);
        assert!(matches!(serde_json::from_str::<SavedPosition>("[400, 1200]"), Ok(SavedPosition::Physical(400, 1200))));
        assert!(serde_json::from_str::<SavedPosition>("garbage").is_err());
    }

    #[test]
    fn only_a_usable_saved_size_is_applied() {
        assert_eq!(usable_size(Some(900.0), Some(300.0)), Some((900.0, 300.0)));
        assert_eq!(usable_size(Some(900.0), None), None);
        // A minimized or collapsed window must not come back as a sliver.
        assert_eq!(usable_size(Some(0.0), Some(0.0)), None);
        assert_eq!(usable_size(Some(900.0), Some(10.0)), None);
        assert_eq!(usable_size(Some(f64::NAN), Some(300.0)), None);
    }

    #[test]
    fn legacy_settings_carry_a_cpu_choice_over_and_leave_the_rest_auto() {
        // An older settings.json where the user turned the ASR GPU off and set the translator to
        // 0 layers; both meant "run this model on the CPU" and must survive the upgrade.
        let mut cpu: Settings = serde_json::from_str(r#"{"whisper_gpu": false, "gpu_layers": 0}"#).unwrap();
        cpu.migrate();
        assert_eq!((cpu.asr_device, cpu.translator_device), (Device::Cpu, Device::Cpu));
        assert!(cpu.whisper_gpu.is_none() && cpu.gpu_layers.is_none(), "the legacy fields are cleared after migrating");

        // A file that kept the old defaults (GPU on, every layer) behaved exactly like Auto.
        let mut defaults: Settings = serde_json::from_str(r#"{"whisper_gpu": true, "gpu_layers": 999}"#).unwrap();
        defaults.migrate();
        assert_eq!((defaults.asr_device, defaults.translator_device), (Device::Auto, Device::Auto));

        // A file from this build has neither legacy key; migrating must not touch its choices.
        let mut fresh: Settings = serde_json::from_str(r#"{"asr_device": "gpu", "translator_device": "cpu"}"#).unwrap();
        fresh.migrate();
        assert_eq!((fresh.asr_device, fresh.translator_device), (Device::Gpu, Device::Cpu));
    }

    #[test]
    fn saved_settings_write_the_device_choices_and_drop_the_legacy_keys() {
        // The legacy fields only ever parse: a save must not bring them back, or the UI would keep
        // showing (and migrating) settings that no longer exist.
        let mut s = Settings { asr_device: Device::Gpu, translator_device: Device::Cpu, ..Settings::default() };
        s.whisper_gpu = Some(false);
        s.gpu_layers = Some(0);
        let j = serde_json::to_value(&s).unwrap();
        assert_eq!(j["asr_device"], "gpu");
        assert_eq!(j["translator_device"], "cpu");
        assert!(j.get("whisper_gpu").is_none() && j.get("gpu_layers").is_none());

        // Untouched settings are Auto for both, which is today's behaviour: GPU when one was found.
        let defaults = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(defaults["asr_device"], "auto");
        assert_eq!(defaults["translator_device"], "auto");
    }
}
