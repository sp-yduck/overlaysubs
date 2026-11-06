#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod capture;
mod download;
mod engine;
mod hw;

use capture::Source;
use overlaysubs_core::cache::HardwareCache;
use overlaysubs_core::endpoint::{EndpointConfig, Endpointer, Event};
use overlaysubs_core::export::{self, Entry};
use overlaysubs_core::registry::{Registry, Selection};
use overlaysubs_core::session::{Asr, Caption, Session, SessionConfig, Translator};
use overlaysubs_core::tier::{self, Hardware, Tier};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    models_dir: Option<PathBuf>,
    tier: Option<Tier>,
    selection: Option<Selection>,
    source: Source,
    session: SessionConfig,
    /// Cap GPU use when gaming. 0 = translator on CPU.
    gpu_layers: u32,
    whisper_gpu: bool,
    hotkey: String,
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
            gpu_layers: 999,
            whisper_gpu: true,
            hotkey: "CmdOrCtrl+Shift+T".into(),
            ui_lang: "ja".into(),
            overlay: serde_json::json!({}),
        }
    }
}

struct Engines {
    ids: Selection,
    asr: engine::Whisper,
    mt: engine::Plamo,
}

struct Running {
    _capture: capture::Capture,
    stop_tx: mpsc::Sender<()>,
}

#[derive(Default)]
struct App {
    settings: Mutex<Settings>,
    engines: Arc<Mutex<Option<Engines>>>,
    running: Mutex<Option<Running>>,
    history: Arc<Mutex<Vec<Entry>>>,
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
const TERMS_VERSION: &str = "2";

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
    models_dir: PathBuf,
    devices: capture::Devices,
    /// Last "Check this PC" result from disk, so the model setup screen can show it again after a restart.
    hardware: Option<HardwareCache>,
}

#[tauri::command]
fn snapshot(app: AppHandle, st: State<App>) -> Result<Snapshot, String> {
    let settings = st.settings.lock().unwrap().clone();
    let registry = registry(&app)?;
    let dir = models_dir(&app, &settings);
    let downloaded = registry.models.iter().filter(|m| dir.join(&m.file).exists()).map(|m| m.id.clone()).collect();
    Ok(Snapshot {
        running: st.running.lock().unwrap().is_some(),
        terms_accepted: terms_accepted(&app),
        settings,
        registry,
        downloaded,
        models_dir: dir,
        devices: capture::devices(),
        hardware: hw::cached(&config_dir(&app)),
    })
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
    if eng.as_ref().is_some_and(|e| [&e.ids.asr, &e.ids.translator, &e.ids.vad].contains(&&id)) {
        *eng = None;
    }
    std::fs::remove_file(models_dir(&app, &s).join(m.file)).map_err(|e| e.to_string())
}

fn ensure_engines(app: &AppHandle, st: &App) -> Result<(), String> {
    let s = st.settings.lock().unwrap().clone();
    let sel = s.selection.clone().ok_or("no models selected")?;
    let mut eng = st.engines.lock().unwrap();
    if eng.as_ref().is_some_and(|e| e.ids == sel) {
        return Ok(());
    }
    *eng = None;
    let reg = registry(app)?;
    let dir = models_dir(app, &s);
    let path = |id: &str| reg.get(id).map(|m| dir.join(&m.file)).filter(|p| p.exists()).ok_or(format!("model {id} not downloaded"));
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let asr = engine::Whisper::load(&path(&sel.asr)?, &path(&sel.vad)?, s.whisper_gpu, threads)?;
    let mt = engine::Plamo::load(&path(&sel.translator)?, s.gpu_layers, threads)?;
    *eng = Some(Engines { ids: sel, asr, mt });
    Ok(())
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
        let cur = st.settings.lock().unwrap().tier.unwrap_or(Tier::Mid);
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
    history.lock().unwrap().clear();
    let emit_app = app.clone();
    std::thread::spawn(move || {
        let mut session = Session::new(s.session);
        let mut seen = 0;
        let mut last_event = Instant::now();
        let mut emit = |c: Caption| {
            let _ = emit_app.emit("caption", &c);
        };
        loop {
            let stopping = stop_rx.try_recv().is_ok();
            let next = if stopping { None } else { ev_rx.recv_timeout(std::time::Duration::from_millis(200)).ok() };
            let Some(first) = next else {
                // The speaker paused mid-sentence, or capture stopped: stop waiting for the rest.
                if stopping || last_event.elapsed() > std::time::Duration::from_secs(2) {
                    let mut g = engines.lock().unwrap();
                    let Some(e) = g.as_mut() else { return };
                    let n = session.entries.len();
                    if let Err(err) = session.flush(&mut e.mt, &mut emit) {
                        let _ = emit_app.emit("error", err);
                    }
                    if session.entries.len() != n {
                        *history.lock().unwrap() = session.entries.clone();
                    }
                }
                if stopping {
                    return;
                }
                continue;
            };
            last_event = Instant::now();
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
    *st.running.lock().unwrap() = Some(Running { _capture: cap, stop_tx });
    app.emit("running", true).map_err(|e| e.to_string())
}

#[tauri::command]
fn stop(app: AppHandle, st: State<App>) -> Result<(), String> {
    if let Some(r) = st.running.lock().unwrap().take() {
        let _ = r.stop_tx.send(());
    }
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

/// OverlaySubs terms. The PLaMo 3 license is linked from the terms, not bundled.
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
            let s: Settings = match std::fs::read_to_string(&path) {
                Err(_) => Settings::default(),
                Ok(j) => serde_json::from_str(&j).unwrap_or_else(|e| {
                    // Keep the unreadable file instead of silently overwriting it on the next save.
                    eprintln!("settings.json: {e}; moved to settings.json.bak");
                    let _ = std::fs::rename(&path, path.with_extension("json.bak"));
                    Settings::default()
                }),
            };
            if let Err(e) = h.global_shortcut().register(s.hotkey.as_str()) {
                eprintln!("hotkey {}: {e}", s.hotkey);
            }
            *app.state::<App>().settings.lock().unwrap() = s;

            let toggle_i = MenuItem::with_id(app, "toggle", "Start / Stop", true, None::<&str>)?;
            let show_i = MenuItem::with_id(app, "show", "Settings", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("OverlaySubs — Built with PLaMo")
                .menu(&Menu::with_items(app, &[&toggle_i, &show_i, &quit_i])?)
                .on_menu_event(|app, e| match e.id.as_ref() {
                    "toggle" => toggle(app),
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
            if let tauri::WindowEvent::CloseRequested { .. } = e {
                if w.label() == "main" {
                    w.app_handle().exit(0);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            snapshot, save_settings, detect, download, delete_model, benchmark, start, stop, history, export_history, terms_text, accept_terms, set_click_through, open_url
        ])
        .run(tauri::generate_context!())
        .expect("error while running overlaysubs");
}
