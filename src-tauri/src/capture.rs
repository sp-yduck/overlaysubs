//! Audio capture → 16 kHz mono f32 chunks.
//! Windows system audio uses WASAPI loopback (cpal opens an *output* device as input), which
//! taps the mix without altering playback. macOS system audio uses ScreenCaptureKit (see
//! [`sck`]), which needs no virtual audio driver. Microphones go through cpal on every platform.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use overlaysubs_core::audio::Resampler;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

#[cfg(target_os = "macos")]
mod sck;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Source {
    System { device: Option<String> },
    Mic { device: Option<String> },
}

#[derive(Serialize)]
pub struct Devices {
    pub outputs: Vec<String>,
    pub inputs: Vec<String>,
}

pub fn devices() -> Devices {
    let h = cpal::default_host();
    let names = |it: Option<Box<dyn Iterator<Item = cpal::Device>>>| it.map(|d| d.filter_map(|d| name(&d)).collect()).unwrap_or_default();
    Devices {
        // macOS captures all system audio at once through ScreenCaptureKit, so there is no
        // output device to choose; the UI hides the device selector for that source.
        outputs: if cfg!(target_os = "macos") { Vec::new() } else { names(h.output_devices().ok().map(|d| Box::new(d) as _)) },
        inputs: names(h.input_devices().ok().map(|d| Box::new(d) as _)),
    }
}

fn name(d: &cpal::Device) -> Option<String> {
    d.description().ok().map(|x| x.name().to_string())
}

fn find(mut it: impl Iterator<Item = cpal::Device>, want: &str) -> Option<cpal::Device> {
    it.find(|d| name(d).is_some_and(|n| n == want))
}

pub struct Capture {
    stop: Arc<AtomicBool>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub fn start(src: &Source, tx: Sender<Vec<f32>>) -> Result<Capture, String> {
    #[cfg(target_os = "macos")]
    if let Source::System { .. } = src {
        return sck::start(tx).map(|stop| Capture { stop });
    }
    start_cpal(src, tx)
}

fn start_cpal(src: &Source, tx: Sender<Vec<f32>>) -> Result<Capture, String> {
    let h = cpal::default_host();
    let (device, config) = match src {
        Source::System { device } => {
            if !cfg!(windows) {
                // macOS is handled by ScreenCaptureKit in `start` before this point.
                return Err("System audio capture is not supported on this platform. Pick a microphone instead.".into());
            }
            let d = match device {
                Some(n) => h.output_devices().ok().and_then(|it| find(it, n)),
                None => h.default_output_device(),
            }
            .ok_or("output device not found")?;
            let c = d.default_output_config().map_err(|e| e.to_string())?;
            (d, c)
        }
        Source::Mic { device } => {
            let d = match device {
                Some(n) => h.input_devices().ok().and_then(|it| find(it, n)),
                None => h.default_input_device(),
            }
            .ok_or("input device not found")?;
            let c = d.default_input_config().map_err(|e| e.to_string())?;
            (d, c)
        }
    };
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    // cpal streams are !Send on some hosts, so the stream lives and dies on its own thread.
    std::thread::spawn(move || {
        let channels = config.channels() as usize;
        let mut rs = Resampler::new(config.sample_rate() as usize);
        let err = |e| log::error!("capture error: {e}");
        let cfg = config.config();
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(cfg, move |d: &[f32], _: &_| { let _ = tx.send(rs.push(d, channels)); }, err, None),
            cpal::SampleFormat::I16 => device.build_input_stream(
                cfg,
                move |d: &[i16], _: &_| {
                    let f: Vec<f32> = d.iter().map(|s| *s as f32 / 32768.0).collect();
                    let _ = tx.send(rs.push(&f, channels));
                },
                err,
                None,
            ),
            // cpal 0.18 ranks I32 above I16 in the default config when F32 is unavailable.
            cpal::SampleFormat::I32 => device.build_input_stream(
                cfg,
                move |d: &[i32], _: &_| {
                    let f: Vec<f32> = d.iter().map(|s| *s as f32 / 2147483648.0).collect();
                    let _ = tx.send(rs.push(&f, channels));
                },
                err,
                None,
            ),
            f => Err(cpal::Error::with_message(cpal::ErrorKind::UnsupportedConfig, format!("unsupported sample format {f}"))),
        };
        let stream = match stream.map_err(|e| e.to_string()).and_then(|s| s.play().map(|_| s).map_err(|e| e.to_string())) {
            Ok(s) => s,
            Err(e) => return ready_tx.send(Err(e)).unwrap_or(()),
        };
        let _ = ready_tx.send(Ok(()));
        while !flag.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        drop(stream);
    });
    ready_rx.recv().map_err(|e| e.to_string())??;
    Ok(Capture { stop })
}
