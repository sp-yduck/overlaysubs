//! Engine-agnostic logic: tier selection, cached hardware probe, model registry, PLaMo prompt, endpointing, session, export.
pub mod cache;
pub mod endpoint;
pub mod export;
pub mod pfml;
pub mod registry;
pub mod session;
pub mod tier;
