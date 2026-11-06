//! Audio capture → 16 kHz mono f32 chunks.
//! Windows system audio uses WASAPI loopback (cpal opens an *output* device as input), which
//! taps the mix without altering playback.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use floatsubs_core::endpoint::RATE;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

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
    let names = |it: Option<Box<dyn Iterator<Item = cpal::Device>>>| it.map(|d| d.filter_map(|d| d.name().ok()).collect()).unwrap_or_default();
    Devices {
        outputs: names(h.output_devices().ok().map(|d| Box::new(d) as _)),
        inputs: names(h.input_devices().ok().map(|d| Box::new(d) as _)),
    }
}

fn find(mut it: impl Iterator<Item = cpal::Device>, name: &str) -> Option<cpal::Device> {
    it.find(|d| d.name().is_ok_and(|n| n == name))
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
    let h = cpal::default_host();
    let (device, config) = match src {
        Source::System { device } => {
            if !cfg!(windows) {
                // shortcut: macOS system audio needs ScreenCaptureKit (Phase 3); use a loopback input device until then.
                return Err("System audio capture is Windows-only for now. On macOS pick a loopback input device under Microphone.".into());
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
        let mut rs = Resampler::new(config.sample_rate().0 as usize);
        let err = |e| eprintln!("capture error: {e}");
        let cfg = config.config();
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(&cfg, move |d: &[f32], _: &_| { let _ = tx.send(rs.push(d, channels)); }, err, None),
            cpal::SampleFormat::I16 => device.build_input_stream(
                &cfg,
                move |d: &[i16], _: &_| {
                    let f: Vec<f32> = d.iter().map(|s| *s as f32 / 32768.0).collect();
                    let _ = tx.send(rs.push(&f, channels));
                },
                err,
                None,
            ),
            f => Err(cpal::BuildStreamError::BackendSpecific { err: cpal::BackendSpecificError { description: format!("unsupported sample format {f}") } }),
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

/// Downmix + linear resample. shortcut: linear interpolation aliases slightly; fine for ASR, swap for a polyphase filter if WER suffers.
struct Resampler {
    step: f64,
    pos: f64,
    prev: f32,
}

impl Resampler {
    fn new(in_rate: usize) -> Self {
        Self { step: in_rate as f64 / RATE as f64, pos: 0.0, prev: 0.0 }
    }

    fn push(&mut self, interleaved: &[f32], channels: usize) -> Vec<f32> {
        let mono: Vec<f32> = interleaved.chunks(channels).map(|f| f.iter().sum::<f32>() / channels as f32).collect();
        let mut out = Vec::with_capacity((mono.len() as f64 / self.step) as usize + 1);
        while self.pos < mono.len() as f64 {
            let i = self.pos.floor() as isize;
            let frac = (self.pos - i as f64) as f32;
            let a = if i == 0 { self.prev } else { mono[(i - 1) as usize] };
            let b = mono[i as usize];
            out.push(a + (b - a) * frac);
            self.pos += self.step;
        }
        self.pos -= mono.len() as f64;
        if let Some(l) = mono.last() {
            self.prev = *l;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn resampler_rate_and_downmix() {
        let mut r = super::Resampler::new(48_000);
        let stereo: Vec<f32> = (0..9600).map(|i| if i % 2 == 0 { 1.0 } else { 0.0 }).collect();
        let a = r.push(&stereo, 2);
        let b = r.push(&stereo, 2);
        assert_eq!(a.len() + b.len(), 3200, "4800 frames ×2 at 48k → 16k");
        assert!(b.iter().all(|s| (*s - 0.5).abs() < 1e-6));
    }
}
