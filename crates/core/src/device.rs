//! Where each model runs: Auto (the GPU when one is usable), or a forced GPU / CPU.
//!
//! Speech recognition and translation are separate engines with separate settings, because they
//! do not want the same thing: the translator is the model that cannot keep up on a CPU, while a
//! small Whisper model may still be fast enough there, and a GPU that runs one may lack the memory
//! for both. VAD has no choice here: whisper.cpp forces the GPU VAD off upstream, so it stays CPU.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Device {
    /// The GPU when this build has a GPU backend and the hardware probe found a GPU; the CPU otherwise.
    #[default]
    Auto,
    /// Always ask the backend for the GPU. A machine with no usable GPU then fails to load and the
    /// app retries on the CPU: the explicit choice must not be silently ignored, but it must not
    /// cost the user subtitles either.
    Gpu,
    /// Always run on the CPU.
    Cpu,
}

impl Device {
    /// Whether the engine with this setting should be asked to load on the GPU.
    pub fn uses_gpu(self, gpu_available: bool) -> bool {
        match self {
            Device::Auto => gpu_available,
            Device::Gpu => true,
            Device::Cpu => false,
        }
    }
}

/// Folds the settings of builds before [`Device`] (`whisper_gpu`, `gpu_layers`) into the per-model
/// choice, so a user who picked the CPU once does not get the GPU back after an update. Only an
/// explicit CPU choice is carried over: the old defaults (`true` / `999`) meant the same as
/// [`Device::Auto`], and an absent field (`None`) was never changed away from the default either.
/// A non-zero layer cap (the old "gaming" setting) has no equivalent any more, so it becomes Auto.
pub fn apply_legacy(asr: &mut Device, translator: &mut Device, whisper_gpu: Option<bool>, gpu_layers: Option<u32>) {
    if whisper_gpu == Some(false) {
        *asr = Device::Cpu;
    }
    if gpu_layers == Some(0) {
        *translator = Device::Cpu;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_as_the_lowercase_names_the_settings_ui_writes() {
        for (device, name) in [(Device::Auto, "auto"), (Device::Gpu, "gpu"), (Device::Cpu, "cpu")] {
            assert_eq!(serde_json::to_string(&device).unwrap(), format!("\"{name}\""));
            assert_eq!(serde_json::from_str::<Device>(&format!("\"{name}\"")).unwrap(), device);
        }
        // The old values are gone, so an unknown one must not silently deserialize into Auto.
        assert!(serde_json::from_str::<Device>("\"GPU\"").is_err());
        assert_eq!(Device::default(), Device::Auto);
    }

    #[test]
    fn auto_follows_the_gpu_and_a_forced_choice_does_not() {
        assert!(Device::Auto.uses_gpu(true) && !Device::Auto.uses_gpu(false));
        assert!(Device::Gpu.uses_gpu(true) && Device::Gpu.uses_gpu(false));
        assert!(!Device::Cpu.uses_gpu(true) && !Device::Cpu.uses_gpu(false));
    }

    #[test]
    fn legacy_cpu_choices_become_cpu_and_everything_else_stays() {
        let migrated = |whisper_gpu, gpu_layers| {
            let (mut asr, mut translator) = (Device::Auto, Device::Auto);
            apply_legacy(&mut asr, &mut translator, whisper_gpu, gpu_layers);
            (asr, translator)
        };
        // The two ways the old UI said "CPU" for one of the models.
        assert_eq!(migrated(Some(false), None), (Device::Cpu, Device::Auto));
        assert_eq!(migrated(None, Some(0)), (Device::Auto, Device::Cpu));
        assert_eq!(migrated(Some(false), Some(0)), (Device::Cpu, Device::Cpu));
        // The old defaults ("GPU on, all layers") mean Auto, and a file without the keys changes nothing.
        assert_eq!(migrated(Some(true), Some(999)), (Device::Auto, Device::Auto));
        assert_eq!(migrated(None, None), (Device::Auto, Device::Auto));
        // A capped layer count was only for gaming; there is no per-layer setting any more.
        assert_eq!(migrated(Some(true), Some(20)), (Device::Auto, Device::Auto));
    }
}
