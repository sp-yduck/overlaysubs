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
    // Pid 0 is the system sounds session; it has no process to capture.
    let mut names: Vec<String> = pids.into_iter().filter_map(|pid| Some(sys.process(Pid::from_u32(pid))?.name().to_string_lossy().into_owned())).collect();
    names.sort();
    names.dedup();
    Ok(names.into_iter().map(|n| (n.clone(), n)).collect())
}

/// The top process named `name`: browsers play audio from a child process, and process loopback
/// follows the tree downwards only.
/// shortcut: two separate instances of one app capture only the first; store a pid per instance if that matters.
fn root_pid(sys: &System, name: &str) -> Option<u32> {
    let named = |p: &Process| p.name().eq_ignore_ascii_case(name);
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
                    eprintln!("capture error: {e}");
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
