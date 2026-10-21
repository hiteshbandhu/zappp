//! The speech models zappp can run, and everything needed to fetch and run
//! each one. Adding a model is adding an entry to [`MODELS`].

use std::path::PathBuf;

use crate::store;

/// How a model is executed.
#[derive(Clone, Copy, Debug)]
pub enum Engine {
    /// Parakeet TDT compiled for CoreML; the encoder runs on the Neural Engine.
    CoremlParakeet(CoremlParakeet),
    /// Parakeet TDT int8 ONNX export through ONNX Runtime on the CPU.
    OnnxParakeet,
    /// Moonshine ONNX through ONNX Runtime on the CPU.
    OnnxMoonshine,
    /// Underdog Bark 3.0: CoreML encoder on the Neural Engine, audio
    /// frontend and TDT decoder on the CPU (see `engine::bark`).
    CoremlBark(CoremlBark),
}

/// Where a Bark 3.0 build lives on Hugging Face.
#[derive(Clone, Copy, Debug)]
pub struct CoremlBark {
    pub repo: &'static str,
}

/// Files and constants for a CoreML Parakeet build.
#[derive(Clone, Copy, Debug)]
pub struct CoremlParakeet {
    /// Hugging Face repository the compiled models come from.
    pub repo: &'static str,
    pub preprocessor: &'static str,
    pub encoder: &'static str,
    pub decoder: &'static str,
    pub joint: &'static str,
    pub vocab: &'static str,
    /// Id of the blank token (also the start symbol).
    pub blank: i32,
}

impl CoremlParakeet {
    pub fn components(&self) -> [&'static str; 4] {
        [self.preprocessor, self.encoder, self.decoder, self.joint]
    }
}

/// Where a model's files come from.
#[derive(Clone, Copy, Debug)]
pub enum Source {
    /// Selected files from a Hugging Face repo (see [`CoremlParakeet`]).
    HuggingFace,
    /// A `.tar.gz` whose top-level folder is the model folder.
    Tarball(&'static str),
}

#[derive(Clone, Copy, Debug)]
pub struct ModelSpec {
    /// Stable id, stored in settings.
    pub id: &'static str,
    pub name: &'static str,
    /// One line shown under the name.
    pub summary: &'static str,
    pub languages: &'static str,
    pub runs_on: &'static str,
    pub size_mb: u32,
    /// Folder under the models directory.
    pub dir: &'static str,
    pub engine: Engine,
    pub source: Source,
}

impl ModelSpec {
    pub fn path(&self) -> PathBuf {
        store::models_dir().join(self.dir)
    }

    pub fn is_installed(&self) -> bool {
        let dir = self.path();
        match self.engine {
            Engine::CoremlParakeet(c) => {
                c.components().iter().all(|f| dir.join(f).join("coremldata.bin").exists())
                    && dir.join(c.vocab).exists()
            }
            Engine::OnnxParakeet => dir.join("vocab.txt").exists() && dir.join("nemo128.onnx").exists(),
            Engine::OnnxMoonshine => dir.join("tokenizer.json").exists() && dir.join("encoder_model.onnx").exists(),
            Engine::CoremlBark(_) => {
                dir.join("BarkEncoder.mlmodelc").is_dir()
                    && dir.join("decoder/mel_filters.npy").exists()
                    && dir.join("vocab.json").exists()
            }
        }
    }
}

pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: "parakeet-v3-ane",
        name: "Parakeet v3",
        summary: "Fastest overall",
        languages: "25 languages",
        runs_on: "Neural Engine",
        size_mb: 461,
        dir: "parakeet-tdt-0.6b-v3-coreml",
        engine: Engine::CoremlParakeet(CoremlParakeet {
            repo: "FluidInference/parakeet-tdt-0.6b-v3-coreml",
            preprocessor: "Preprocessor.mlmodelc",
            encoder: "Encoder.mlmodelc",
            decoder: "Decoder.mlmodelc",
            joint: "JointDecisionv3.mlmodelc",
            vocab: "parakeet_v3_vocab.json",
            blank: 8192,
        }),
        source: Source::HuggingFace,
    },
    ModelSpec {
        id: "parakeet-v2-ane",
        name: "Parakeet v2",
        summary: "Most accurate in English",
        languages: "English",
        runs_on: "Neural Engine",
        size_mb: 464,
        dir: "parakeet-tdt-0.6b-v2-coreml",
        engine: Engine::CoremlParakeet(CoremlParakeet {
            repo: "FluidInference/parakeet-tdt-0.6b-v2-coreml",
            preprocessor: "Preprocessor.mlmodelc",
            encoder: "Encoder.mlmodelc",
            decoder: "Decoder.mlmodelc",
            joint: "JointDecision.mlmodelc",
            vocab: "parakeet_vocab.json",
            blank: 1024,
        }),
        source: Source::HuggingFace,
    },
    ModelSpec {
        id: "bark-3",
        name: "Bark 3.0",
        summary: "Compact Parakeet build · Conway Research",
        languages: "English",
        runs_on: "Neural Engine",
        size_mb: 205,
        dir: "underdog-bark-3.0",
        engine: Engine::CoremlBark(CoremlBark {
            repo: "ConwayResearch/Underdog-Bark-3.0",
        }),
        source: Source::HuggingFace,
    },
    ModelSpec {
        id: "bark-3-plus",
        name: "Bark 3.0 Plus",
        summary: "Bark, tuned for higher accuracy · Conway Research",
        languages: "English",
        runs_on: "Neural Engine",
        size_mb: 206,
        dir: "underdog-bark-3.0-plus",
        engine: Engine::CoremlBark(CoremlBark {
            repo: "ConwayResearch/Underdog-Bark-3.0-Plus",
        }),
        source: Source::HuggingFace,
    },
    ModelSpec {
        id: "parakeet-v3-cpu",
        name: "Parakeet v3 (CPU)",
        summary: "Same model on the CPU",
        languages: "25 languages",
        runs_on: "CPU",
        size_mb: 640,
        dir: "parakeet-tdt-0.6b-v3-int8",
        engine: Engine::OnnxParakeet,
        source: Source::Tarball("https://blob.handy.computer/parakeet-v3-int8.tar.gz"),
    },
    ModelSpec {
        id: "moonshine-base",
        name: "Moonshine Base",
        summary: "Small download, less accurate",
        languages: "English",
        runs_on: "CPU",
        size_mb: 58,
        dir: "moonshine-base",
        engine: Engine::OnnxMoonshine,
        source: Source::Tarball("https://blob.handy.computer/moonshine-base.tar.gz"),
    },
];

pub const DEFAULT_MODEL: &str = "parakeet-v3-ane";

pub fn find(id: &str) -> &'static ModelSpec {
    MODELS
        .iter()
        .find(|m| m.id == id)
        .unwrap_or_else(|| find(DEFAULT_MODEL))
}
