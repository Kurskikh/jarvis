use std::fmt;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub enum WakeWordEngine {
    Rustpotter,
    Vosk,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub enum NoiseSuppressionBackend {
    None,
    Nnnoiseless,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum SpeechToTextEngine {
    Vosk,
    // T-one through sherpa-onnx: streaming, Russian, on the processor. For
    // the command only - the wake word stays on Vosk, whose restricted
    // grammar is what makes catching one word out of a stream cheap.
    TOne,
}

#[derive(PartialEq, Debug)]
pub enum RecorderType {
    Cpal,
    PvRecorder,
    PortAudio,
}

#[derive(PartialEq, Debug)]
pub enum AudioType {
    Rodio,
    Kira,
}

impl fmt::Display for WakeWordEngine {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl fmt::Display for SpeechToTextEngine {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl fmt::Display for NoiseSuppressionBackend {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
