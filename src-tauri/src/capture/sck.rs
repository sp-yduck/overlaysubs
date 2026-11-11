//! macOS system-audio capture with ScreenCaptureKit.
//!
//! ScreenCaptureKit mixes every app's output into the stream, so no loopback/virtual audio
//! driver (BlackHole and friends) is needed. Only the audio output is registered; the tiny
//! screen configuration is never consumed, it only exists because `SCContentFilter` is built
//! from a display.
//!
//! Per-app capture uses the same stream with a content filter that includes only that app.
//!
//! macOS gates this behind the Screen & System Audio Recording permission. The first start
//! shows the system prompt; after the user grants it, the app has to be relaunched before the
//! permission takes effect. A denied/missing permission maps to [`ERR_PERMISSION`] so the UI can
//! show the exact System Settings path.

use overlaysubs_core::audio::{downmix_buffers, f32_pcm, Resampler};
use overlaysubs_core::endpoint::RATE;
use screencapturekit::error::{SCError, SCStreamErrorCode};
use screencapturekit::prelude::*;
use screencapturekit::utils::completion::is_timeout_error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

/// Screen Recording permission is missing or was declined.
pub const ERR_PERMISSION: &str = "E_MACOS_PERMISSION";
/// There is no display ScreenCaptureKit could build a content filter from.
pub const ERR_NO_DISPLAY: &str = "E_MACOS_NO_DISPLAY";
/// Any other ScreenCaptureKit failure; the underlying error follows a `|`.
const ERR_AUDIO: &str = "E_MACOS_AUDIO";

/// The running apps a content filter can single out, as (bundle identifier, name).
pub fn apps() -> Result<Vec<(String, String)>, String> {
    // Triggers the Screen Recording prompt on first use, like `open`.
    let mut apps: Vec<(String, String)> = SCShareableContent::get()
        .map_err(start_error)?
        .applications()
        .into_iter()
        .map(|app| (app.bundle_identifier(), app.application_name()))
        .filter(|(id, name)| !id.is_empty() && !name.is_empty())
        .collect();
    apps.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    apps.dedup();
    Ok(apps)
}

/// Start capturing system audio, or only the audio of the app with bundle identifier `app`.
/// Returns the stop flag once the stream is running.
///
/// The stream is created, started, and dropped on its own thread because `SCStream` is not
/// `Send` everywhere it touches AppKit; the stop flag is polled there.
pub fn start(app: Option<String>, tx: Sender<Vec<f32>>) -> Result<Arc<AtomicBool>, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    std::thread::Builder::new()
        .name("sck-audio".into())
        .spawn(move || match open(app.as_deref(), tx) {
            Ok(stream) => {
                let _ = ready_tx.send(Ok(()));
                while !flag.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                if let Err(error) = stream.stop_capture() {
                    eprintln!("system audio capture stop error: {error}");
                }
                // Dropping the stream removes the output handler and releases the capture.
            }
            Err(error) => {
                let _ = ready_tx.send(Err(error));
            }
        })
        .map_err(|error| format!("{ERR_AUDIO}|failed to spawn capture thread: {error}"))?;
    ready_rx
        .recv()
        .map_err(|error| format!("{ERR_AUDIO}|capture thread stopped: {error}"))??;
    Ok(stop)
}

fn open(app: Option<&str>, tx: Sender<Vec<f32>>) -> Result<SCStream, String> {
    // Triggers the Screen Recording prompt on first use and blocks until the user answers.
    let content = SCShareableContent::get().map_err(start_error)?;
    let display = content
        .displays()
        .into_iter()
        .next()
        .ok_or(ERR_NO_DISPLAY)?;
    let filter = SCContentFilter::create().with_display(&display);
    let filter = match app {
        Some(id) => {
            let apps = content.applications();
            let app = apps
                .iter()
                .find(|app| app.bundle_identifier() == id)
                .ok_or(super::ERR_APP_NOT_RUNNING)?;
            filter.with_including_applications(&[app], &[])
        }
        None => filter.with_excluding_windows(&[]),
    }
    .build()
    .map_err(start_error)?;
    let config = SCStreamConfiguration::new()
        // Audio-only: no screen output is registered, so these dimensions are never rendered.
        .with_width(2)
        .with_height(2)
        .with_shows_cursor(false)
        .with_captures_audio(true)
        .with_sample_rate(RATE as i32)
        .with_channel_count(1)
        // Never feed our own playback (none today) back into the pipeline.
        .with_excludes_current_process_audio(true);
    let mut stream = SCStream::new(&filter, &config).map_err(start_error)?;
    stream
        .add_output_handler(AudioSink::new(tx), SCStreamOutputType::Audio)
        .map_err(start_error)?;
    stream.start_capture().map_err(start_error)?;
    Ok(stream)
}

/// Hints that a ScreenCaptureKit message is about the TCC permission rather than a stream fault.
/// The Swift bridge prefixes messages with "Content unavailable:" / "Stream error:", and macOS
/// reports a declined prompt as a TCC decline.
fn looks_like_permission(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    [
        "declined",
        "tcc",
        "permission",
        "not authorized",
        "unauthorized",
        "screen recording",
    ]
    .iter()
    .any(|hint| lower.contains(hint))
}

/// Map a ScreenCaptureKit error onto the UI's stable error codes.
fn start_error(error: SCError) -> String {
    let message = match &error {
        SCError::PermissionDenied(message)
        | SCError::NoShareableContent(message)
        | SCError::StreamError(message)
        | SCError::CaptureStartFailed(message) => Some(message.as_str()),
        _ => None,
    };
    // A completion timeout is not a denial: the permission prompt may still be on screen, and
    // answering it and retrying works. Report the detail instead of sending the user to Settings.
    if message.is_some_and(is_timeout_error) {
        return format!("{ERR_AUDIO}|{error}");
    }
    let permission = match &error {
        SCError::PermissionDenied(_) => true,
        // `SCShareableContent::get` funnels every non-timeout completion failure (a declined TCC
        // prompt included) into this variant.
        SCError::NoShareableContent(_) => true,
        SCError::SCStreamError { code, .. } => *code == SCStreamErrorCode::UserDeclined,
        // A first-run grant needs an app relaunch; depending on the macOS version that surfaces
        // as a plain localized message from `startCapture` rather than a typed error code.
        _ => message.is_some_and(looks_like_permission),
    };
    if permission {
        ERR_PERMISSION.into()
    } else {
        format!("{ERR_AUDIO}|{error}")
    }
}

/// Receives 16 kHz mono chunks from ScreenCaptureKit's dispatch queue.
struct AudioSink {
    state: Mutex<SinkState>,
}

struct SinkState {
    tx: Sender<Vec<f32>>,
    in_rate: usize,
    resampler: Resampler,
}

impl AudioSink {
    fn new(tx: Sender<Vec<f32>>) -> Self {
        Self {
            state: Mutex::new(SinkState {
                tx,
                in_rate: RATE,
                resampler: Resampler::new(RATE),
            }),
        }
    }
}

impl SCStreamOutputTrait for AudioSink {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, of_type: SCStreamOutputType) {
        if of_type != SCStreamOutputType::Audio {
            return;
        }
        // Never unwind into the dispatch queue: a panic here would abort the capture thread.
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some((mono, rate)) = mono_from_sample(&sample) else {
            return;
        };
        if rate != state.in_rate {
            // ScreenCaptureKit can reconfigure under us (display change, sample rate reset).
            state.in_rate = rate;
            state.resampler = Resampler::new(rate);
        }
        let chunk = state.resampler.push_mono(&mono);
        if !chunk.is_empty() {
            let _ = state.tx.send(chunk);
        }
    }
}

/// Convert one ScreenCaptureKit audio sample buffer to 16 kHz-mono input samples.
///
/// The rate is returned alongside the samples so the caller can rebuild the resampler if it
/// changes. The PCM layout is read from the buffer list instead of assumed: ScreenCaptureKit
/// delivers non-interleaved float32 today, but interleaved and mixed layouts are handled too.
fn mono_from_sample(sample: &CMSampleBuffer) -> Option<(Vec<f32>, usize)> {
    let format = sample.format_description()?;
    if !format.is_audio() || !format.audio_is_float() {
        return None;
    }
    let rate = format
        .audio_sample_rate()
        .map(|rate| rate.round() as usize)
        .filter(|rate| *rate > 0)
        .unwrap_or(RATE);
    let big_endian = format.audio_is_big_endian();
    let list = sample.audio_buffer_list().ok()?;
    let mut buffers: Vec<(usize, Vec<f32>)> = Vec::with_capacity(list.num_buffers());
    for index in 0..list.num_buffers() {
        let Some(buffer) = list.get(index) else {
            continue;
        };
        let samples = f32_pcm(buffer.data(), big_endian);
        if !samples.is_empty() {
            buffers.push((buffer.number_channels as usize, samples));
        }
    }
    let views: Vec<(usize, &[f32])> = buffers
        .iter()
        .map(|(channels, samples)| (*channels, samples.as_slice()))
        .collect();
    let mono = downmix_buffers(&views);
    (!mono.is_empty()).then_some((mono, rate))
}
