pub mod click;
pub mod engine;
pub mod loader;
pub mod manifest;
pub mod midi;
pub mod routing;
pub mod stretcher;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AudioCoreError {
    #[error("Audio device error: {0}")]
    Device(String),
    #[error("Audio stream error: {0}")]
    Stream(String),
    #[error("Decode error: {0}")]
    Decode(String),
    #[error("Manifest error: {0}")]
    Manifest(String),
    #[error("MIDI error: {0}")]
    Midi(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, AudioCoreError>;
