//! PCM conversion shared by the capture backends: downmix to mono and resample to 16 kHz.
//!
//! The cpal capture path and the macOS ScreenCaptureKit path both hand raw PCM to the same
//! endpointer, so the conversion lives here where `cargo test -p overlaysubs-core` can cover it
//! on every platform.

use crate::endpoint::RATE;

/// Decode 32-bit float PCM bytes into samples. Trailing bytes that do not form a whole `f32`
/// are ignored, which is what partially filled capture buffers produce.
pub fn f32_pcm(bytes: &[u8], big_endian: bool) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|b| {
            let bits = [b[0], b[1], b[2], b[3]];
            f32::from_bits(if big_endian {
                u32::from_be_bytes(bits)
            } else {
                u32::from_le_bytes(bits)
            })
        })
        .collect()
}

/// Downmix interleaved frames to mono (average of the channels). A trailing partial frame is
/// ignored rather than panicking.
pub fn downmix_interleaved(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|f| f.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Downmix planar audio (one slice per channel) to mono. A shorter channel ends the output, so
/// a partially filled buffer cannot read past the end of another channel.
pub fn downmix_planar(channels: &[&[f32]]) -> Vec<f32> {
    let frames = channels.iter().map(|c| c.len()).min().unwrap_or(0);
    if channels.is_empty() || frames == 0 {
        return Vec::new();
    }
    let mut out = vec![0.0f32; frames];
    for channel in channels {
        for (acc, sample) in out.iter_mut().zip(channel.iter()) {
            *acc += sample;
        }
    }
    let n = channels.len() as f32;
    for acc in &mut out {
        *acc /= n;
    }
    out
}

/// Downmix a buffer list whose per-buffer channel count is known.
///
/// ScreenCaptureKit delivers audio non-interleaved (one mono buffer per channel), but the layout
/// has changed between macOS releases, so handle the interleaved and mixed cases too instead of
/// assuming one of them.
pub fn downmix_buffers(buffers: &[(usize, &[f32])]) -> Vec<f32> {
    if buffers.is_empty() {
        return Vec::new();
    }
    if buffers.len() == 1 {
        return downmix_interleaved(buffers[0].1, buffers[0].0.max(1));
    }
    if buffers.iter().all(|(channels, _)| *channels <= 1) {
        let planes: Vec<&[f32]> = buffers.iter().map(|(_, samples)| *samples).collect();
        return downmix_planar(&planes);
    }
    // Unusual layout: several interleaved buffers. Average each buffer's mono downmix.
    let monos: Vec<Vec<f32>> = buffers
        .iter()
        .map(|(channels, samples)| downmix_interleaved(samples, (*channels).max(1)))
        .collect();
    let planes: Vec<&[f32]> = monos.iter().map(|mono| mono.as_slice()).collect();
    downmix_planar(&planes)
}

/// Downmix + linear resample to [`RATE`]. Linear interpolation aliases slightly; fine for ASR,
/// swap for a polyphase filter if WER suffers.
pub struct Resampler {
    step: f64,
    pos: f64,
    prev: f32,
}

impl Resampler {
    pub fn new(in_rate: usize) -> Self {
        Self {
            step: in_rate.max(1) as f64 / RATE as f64,
            pos: 0.0,
            prev: 0.0,
        }
    }

    /// Push interleaved frames and return 16 kHz mono samples.
    pub fn push(&mut self, interleaved: &[f32], channels: usize) -> Vec<f32> {
        self.push_mono(&downmix_interleaved(interleaved, channels))
    }

    /// Push already downmixed mono frames and return 16 kHz samples.
    pub fn push_mono(&mut self, mono: &[f32]) -> Vec<f32> {
        let mut out = Vec::with_capacity((mono.len() as f64 / self.step) as usize + 1);
        while self.pos < mono.len() as f64 {
            let i = self.pos.floor() as isize;
            let frac = (self.pos - i as f64) as f32;
            let a = if i == 0 {
                self.prev
            } else {
                mono[(i - 1) as usize]
            };
            let b = mono[i as usize];
            out.push(a + (b - a) * frac);
            self.pos += self.step;
        }
        self.pos -= mono.len() as f64;
        if let Some(last) = mono.last() {
            self.prev = *last;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_rate_and_downmix() {
        let mut r = Resampler::new(48_000);
        let stereo: Vec<f32> = (0..9600)
            .map(|i| if i % 2 == 0 { 1.0 } else { 0.0 })
            .collect();
        let a = r.push(&stereo, 2);
        let b = r.push(&stereo, 2);
        assert_eq!(a.len() + b.len(), 3200, "4800 frames × 2 at 48k → 16k");
        assert!(b.iter().all(|s| (*s - 0.5).abs() < 1e-6));
    }

    #[test]
    fn identity_resample_at_16k_keeps_the_stream_length() {
        let mut r = Resampler::new(16_000);
        let input: Vec<f32> = (0..1_000).map(|i| i as f32).collect();
        let out = r.push_mono(&input);
        assert_eq!(out.len(), input.len());
        assert!(out[1..]
            .iter()
            .zip(input.iter())
            .all(|(o, i)| (*o - *i).abs() < 1e-6));
    }

    #[test]
    fn interleaved_downmix_averages_channels() {
        let stereo = [1.0, 0.0, 0.5, 0.5, -1.0, 1.0];
        assert_eq!(downmix_interleaved(&stereo, 2), vec![0.5, 0.5, 0.0]);
        // A partial trailing frame is dropped instead of panicking.
        assert_eq!(downmix_interleaved(&[1.0, 0.0, 0.7], 2), vec![0.5]);
        assert_eq!(downmix_interleaved(&[0.25], 1), vec![0.25]);
    }

    #[test]
    fn planar_downmix_averages_channels() {
        let left = [1.0f32, 0.0, -0.5];
        let right = [0.0f32, 1.0];
        assert_eq!(downmix_planar(&[&left, &right]), vec![0.5, 0.5]);
        assert!(downmix_planar(&[]).is_empty());
    }

    #[test]
    fn buffer_list_layouts_downmix_to_mono() {
        let planar = [(1, &[1.0f32, 0.0][..]), (1, &[0.0f32, 1.0][..])];
        assert_eq!(downmix_buffers(&planar), vec![0.5, 0.5]);
        let interleaved = [(2, &[1.0f32, 0.0, 0.0, 1.0][..])];
        assert_eq!(downmix_buffers(&interleaved), vec![0.5, 0.5]);
        assert!(downmix_buffers(&[]).is_empty());
    }

    #[test]
    fn f32_pcm_decodes_both_endiannesses() {
        let little = 1.5f32.to_le_bytes();
        let big = 1.5f32.to_be_bytes();
        assert_eq!(f32_pcm(&little, false), vec![1.5]);
        assert_eq!(f32_pcm(&big, true), vec![1.5]);
        // Trailing partial bytes and empty buffers decode to nothing.
        assert_eq!(f32_pcm(&[0, 1, 2], false), Vec::<f32>::new());
        assert!(f32_pcm(&[], false).is_empty());
    }
}
