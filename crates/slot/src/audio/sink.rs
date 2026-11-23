use std::fmt;
use std::sync::Arc;

use super::ring::Ring;

#[derive(Debug)]
pub enum AudioError {
    NoDevice,
    Config(String),
    Device(String),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioError::NoDevice => write!(f, "no default output device"),
            AudioError::Config(m) => write!(f, "unusable output config: {m}"),
            AudioError::Device(m) => write!(f, "audio device: {m}"),
        }
    }
}

impl std::error::Error for AudioError {}

pub trait AudioSink: Send {
    fn open(&mut self, sample_rate: u32) -> Result<(), AudioError>;
    fn ring(&self) -> Arc<Ring>;

    /// Release exclusive output ownership for an external video player.
    /// Non-device backends may leave audio attached; the Brick ALSA sink
    /// explicitly closes its output thread and PCM handle.
    fn suspend(&mut self) {}
}
