//! Runs whichever speech model is selected (see `models::MODELS`):
//! CoreML Parakeet on the Neural Engine, or ONNX models on the CPU.

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow};
use transcribe_rs::{
    OrtAccelerator, SpeechModel, TranscribeOptions, set_ort_accelerator,
    onnx::{
        Quantization,
        moonshine::{MoonshineModel, MoonshineVariant},
        parakeet::ParakeetModel,
    },
};

use super::{bark::Bark, coreml::Units, parakeet_coreml::ParakeetCoreml};
use crate::models::{Engine, ModelSpec};

enum Backend {
    Coreml(ParakeetCoreml),
    Bark(Bark),
    Onnx(Box<dyn SpeechModel + Send>),
}

pub struct Asr {
    backend: Mutex<Option<Backend>>,
}

impl Asr {
    pub const fn new() -> Self {
        Self {
            backend: Mutex::new(None),
        }
    }

    /// Loads (and warms up) a model, replacing the current one.
    pub fn load(&self, spec: &ModelSpec) -> Result<Duration> {
        let started = Instant::now();
        let dir = spec.path();
        let mut backend = match spec.engine {
            Engine::CoremlParakeet(cfg) => Backend::Coreml(ParakeetCoreml::load(
                &dir,
                cfg,
                Units::from_env("ZAPPP_ENCODER_UNITS", Units::CpuAndNeuralEngine),
            )?),
            Engine::CoremlBark(_) => Backend::Bark(Bark::load(
                &dir,
                Units::from_env("ZAPPP_ENCODER_UNITS", Units::CpuAndNeuralEngine),
            )?),
            Engine::OnnxParakeet => {
                set_ort_accelerator(OrtAccelerator::CpuOnly);
                Backend::Onnx(Box::new(
                    ParakeetModel::load(&dir, &Quantization::Int8).map_err(|e| anyhow!("Could not load {}: {e}", spec.name))?,
                ))
            }
            Engine::OnnxMoonshine => {
                set_ort_accelerator(OrtAccelerator::CpuOnly);
                Backend::Onnx(Box::new(
                    MoonshineModel::load(&dir, MoonshineVariant::Base, &Quantization::default())
                        .map_err(|e| anyhow!("Could not load {}: {e}", spec.name))?,
                ))
            }
        };
        // Warm up so the first dictation doesn't pay for graph setup. For
        // CoreML the very first load also compiles the Neural Engine plan,
        // which macOS then caches.
        let _ = transcribe_with(&mut backend, &vec![0.0; 16_000]);
        *self.backend.lock().unwrap() = Some(backend);
        Ok(started.elapsed())
    }

    pub fn unload(&self) {
        *self.backend.lock().unwrap() = None;
    }

    pub fn is_loaded(&self) -> bool {
        self.backend.lock().unwrap().is_some()
    }

    pub fn transcribe(&self, samples: &[f32]) -> Result<String> {
        let mut guard = self.backend.lock().unwrap();
        let backend = guard.as_mut().ok_or_else(|| anyhow!("Speech model is still loading"))?;
        transcribe_with(backend, samples)
    }
}

fn transcribe_with(backend: &mut Backend, samples: &[f32]) -> Result<String> {
    match backend {
        Backend::Coreml(model) => model.transcribe(samples),
        Backend::Bark(model) => model.transcribe(samples),
        Backend::Onnx(model) => model
            .transcribe(samples, &TranscribeOptions::default())
            .map(|r| r.text.trim().to_string())
            .map_err(|e| anyhow!("Transcription failed: {e}")),
    }
}
