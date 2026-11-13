//! Windows per-app audio capture with WASAPI process loopback (Windows 10 version 2004 or later).
//!
//! cpal only opens whole-device loopback, so this goes through the `wasapi` crate. An app is
//! identified by its executable name, because process ids do not survive a restart of the app.

use overlaysubs_core::audio::{f32_pcm, Resampler};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use sysinfo::{Pid, Process, ProcessesToUpdate, System};
use wasapi::{initialize_mta, AudioClient, DeviceEnumerator, Direction, SampleType, StreamMode, WasapiError, WaveFormat};

/// Process loopback has no mix format of its own; it converts to whatever is requested.
const IN_RATE: usize = 48_000;
const CHANNELS: usize = 2;

fn processes() -> Result<System, String> {
    let mut sys = System::new().map_err(|e| e.to_string())?;
    sys.refresh_processes(ProcessesToUpdate::All, true);
    Ok(sys)
}

/// The executables that have an audio session on the default output device, as (id, label).
pub fn apps() -> Result<Vec<(String, String)>, String> {
    // COM is initialized on a thread of our own so the caller's thread keeps its apartment.
    let pids = std::thread::spawn(|| -> Result<Vec<u32>, WasapiError> {
        initialize_mta().ok()?;
        let sessions = DeviceEnumerator::new()?.get_default_device(Direction::Render)?.get_iaudiosessionmanager()?.get_audiosessionenumerator()?;
        Ok((0..sessions.get_count()?).filter_map(|i| sessions.get_session(i).ok()?.get_process_id().ok()).collect())
    })
    .join()
    .map_err(|_| "audio session thread panicked".to_string())?
    .map_err(|e| e.to_string())?;
    let sys = processes()?;
    let own = std::process::id();
    let own_name = sys.process(Pid::from_u32(own)).map(|p| p.name().to_owned());
    let mut apps: Vec<(String, String)> = Vec::new();
    for pid in pids {
        // Pid 0 is the system sounds session: sysinfo lists it as "[System Process]", but process
        // loopback rejects it with E_INVALIDARG. Our own audio is not worth translating either.
        if pid == 0 || pid == own {
            continue;
        }
        let Some(p) = sys.process(Pid::from_u32(pid)) else { continue };
        if own_name.as_deref() == Some(p.name()) {
            continue;
        }
        let id = p.name().to_string_lossy().into_owned();
        if apps.iter().any(|(i, _)| i.eq_ignore_ascii_case(&id)) {
            continue;
        }
        let label = p.exe().and_then(display_name).unwrap_or_else(|| id.clone());
        apps.push((id, label));
    }
    // Two executables with one description (e.g. a launcher and its helper) stay distinguishable.
    let labels: Vec<String> = apps.iter().map(|(_, l)| l.to_lowercase()).collect();
    for (id, label) in &mut apps {
        if labels.iter().filter(|l| **l == label.to_lowercase()).count() > 1 && !label.eq_ignore_ascii_case(id) {
            *label = format!("{label} ({id})");
        }
    }
    apps.sort_by_key(|(_, l)| l.to_lowercase());
    Ok(apps)
}

/// The FileDescription (or ProductName) in the version info of the executable at `path`.
fn display_name(path: &std::path::Path) -> Option<String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
    let file = HSTRING::from(path.as_os_str());
    let size = unsafe { GetFileVersionInfoSizeW(PCWSTR(file.as_ptr()), None) };
    if size == 0 {
        return None;
    }
    let mut data = vec![0u8; size as usize];
    unsafe { GetFileVersionInfoW(PCWSTR(file.as_ptr()), None, size, data.as_mut_ptr().cast()) }.ok()?;
    // `len` counts bytes for the translation table but UTF-16 units for strings.
    let query = |sub: &str, unit: usize| -> Option<&[u8]> {
        let sub = HSTRING::from(sub);
        let (mut ptr, mut len) = (std::ptr::null_mut(), 0u32);
        unsafe { VerQueryValueW(data.as_ptr().cast(), PCWSTR(sub.as_ptr()), &mut ptr, &mut len) }.as_bool().then_some(())?;
        (!ptr.is_null() && len > 0).then(|| unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize * unit) })
    };
    // The language/code page pairs the string tables are keyed by, then the usual US English ones.
    let mut langs: Vec<String> = query(r"\VarFileInfo\Translation", 1)
        .map(|t| t.chunks_exact(4).map(|c| format!("{:04x}{:04x}", u16::from_le_bytes([c[0], c[1]]), u16::from_le_bytes([c[2], c[3]]))).collect())
        .unwrap_or_default();
    langs.extend(["040904b0".into(), "040904e4".into()]);
    for key in ["FileDescription", "ProductName"] {
        for lang in &langs {
            let Some(raw) = query(&format!(r"\StringFileInfo\{lang}\{key}"), 2) else { continue };
            let wide: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
            let name = String::from_utf16_lossy(&wide).trim().to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

/// The top process named `name`: browsers play audio from a child process, and process loopback
/// follows the tree downwards only.
/// shortcut: two separate instances of one app capture only the first; store a pid per instance if that matters.
fn root_pid(sys: &System, name: &str) -> Option<u32> {
    // Pid 0 ("[System Process]", saved by older versions) cannot be captured.
    let named = |p: &Process| p.pid().as_u32() != 0 && p.name().eq_ignore_ascii_case(name);
    sys.processes().values().find(|p| named(p) && !p.parent().and_then(|parent| sys.process(parent)).is_some_and(named)).map(|p| p.pid().as_u32())
}

/// Start capturing `app`'s audio. Returns the stop flag once the stream is running.
pub fn start(app: &str, tx: Sender<Vec<f32>>) -> Result<Arc<AtomicBool>, String> {
    let pid = root_pid(&processes()?, app).ok_or(super::ERR_APP_NOT_RUNNING)?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    // The COM objects stay on the thread that created them.
    std::thread::spawn(move || {
        let open = || -> Result<_, WasapiError> {
            initialize_mta().ok()?;
            let mut client = AudioClient::new_application_loopback_client(pid, true)?;
            let format = WaveFormat::new(32, 32, SampleType::Float, IN_RATE, CHANNELS, None);
            client.initialize_client(&format, Direction::Capture, StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 0 }, None)?;
            let event = client.set_get_eventhandle()?;
            let capture = client.get_audiocaptureclient()?;
            client.start_stream()?;
            Ok((client, event, capture))
        };
        let (client, event, capture) = match open() {
            Ok(opened) => opened,
            Err(e) => return ready_tx.send(Err(e.to_string())).unwrap_or(()),
        };
        let _ = ready_tx.send(Ok(()));
        let mut rs = Resampler::new(IN_RATE);
        let mut bytes = VecDeque::new();
        while !flag.load(Ordering::Relaxed) {
            // No event fires while the app is silent; the timeout keeps the stop flag polled.
            if event.wait_for_event(100).is_err() {
                continue;
            }
            while capture.get_next_packet_size().ok().flatten().unwrap_or(0) > 0 {
                if let Err(e) = capture.read_from_device_to_deque(&mut bytes) {
                    log::error!("capture error: {e}");
                    break;
                }
            }
            let _ = tx.send(rs.push(&f32_pcm(bytes.make_contiguous(), false), CHANNELS));
            bytes.clear();
        }
        let _ = client.stop_stream();
    });
    ready_rx.recv().map_err(|e| e.to_string())??;
    Ok(stop)
}
