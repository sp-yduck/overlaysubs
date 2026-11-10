//! Engine-agnostic logic: audio PCM conversion, per-model device selection, tier selection, cached hardware probe, model registry, PLaMo prompt, endpointing, session, export, latest-release check.
pub mod audio;
pub mod cache;
pub mod device;
pub mod endpoint;
pub mod export;
pub mod pfml;
pub mod registry;
pub mod session;
pub mod tier;
pub mod update;
