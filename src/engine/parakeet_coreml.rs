//! NVIDIA Parakeet TDT v3 running natively on CoreML, using the compiled
//! models published by FluidInference (the ones FluidAudio ships).
//!
//! Pipeline per ≤15 s window:
//!   Preprocessor (CPU)  audio[240000] → mel[1,128,1501]
//!   Encoder (ANE/GPU)   mel → encoder[1,1024,188]
//!   TDT greedy decode   Decoder (LSTM step) + JointDecision (token, duration)
//!
//! The greedy loop follows NeMo's TDT decoding (also what FluidAudio does):
//! blank is the start symbol, blank never emits, a predicted duration of 0 on
//! blank is bumped to 1, and at most 10 symbols are emitted per frame.

use std::{collections::HashMap, path::Path};

use anyhow::{Context as _, Result, anyhow};

use super::coreml::{Array, Model, Outputs, Units};
use crate::models::CoremlParakeet;


const SAMPLE_RATE: usize = 16_000;
/// The preprocessor's fixed input: 15 s.
const WINDOW: usize = 240_000;
const MEL_FRAMES: i32 = 1501;
const ENC_DIM: usize = 1024;
const PRED_DIM: usize = 640;
const LSTM_LAYERS: usize = 2;
const MAX_SYMBOLS_PER_FRAME: usize = 10;

pub struct ParakeetCoreml {
    preprocessor: Model,
    encoder: Model,
    decoder: Model,
    joint: Model,
    vocab: Vec<String>,
    blank: i32,
}

/// Prediction-network (LSTM) state while decoding one window.
struct DecoderState {
    output: Array,
    h: Array,
    c: Array,
}

impl ParakeetCoreml {
    /// `encoder_units` is where the heavy lifting happens (Neural Engine by
    /// default). The preprocessor stays on CPU like FluidAudio does.
    pub fn load(dir: &Path, cfg: CoremlParakeet, encoder_units: Units) -> Result<Self> {
        // The per-token decoder/joint calls are tiny; on the CPU they avoid
        // Neural Engine dispatch overhead (measured: no faster elsewhere).
        let small_units = Units::from_env("ZAPPP_DECODER_UNITS", Units::Cpu);
        let preprocessor = Model::load(&dir.join(cfg.preprocessor), Units::Cpu)?;
        let encoder = Model::load(&dir.join(cfg.encoder), encoder_units)?;
        let decoder = Model::load(&dir.join(cfg.decoder), small_units)?;
        let joint = Model::load(&dir.join(cfg.joint), small_units)?;

        let raw = std::fs::read(dir.join(cfg.vocab)).context("reading Parakeet vocabulary")?;
        let map: HashMap<String, String> = serde_json::from_slice(&raw).context("parsing Parakeet vocabulary")?;
        let size = map.keys().filter_map(|k| k.parse::<usize>().ok()).max().unwrap_or(0) + 1;
        let mut vocab = vec![String::new(); size];
        for (k, v) in map {
            if let Ok(i) = k.parse::<usize>() {
                vocab[i] = v;
            }
        }
        let blank = cfg.blank;

        Ok(Self {
            preprocessor,
            encoder,
            decoder,
            joint,
            vocab,
            blank,
        })
    }

    pub fn transcribe(&self, samples: &[f32]) -> Result<String> {
        let mut tokens = Vec::new();
        for window in split_windows(samples) {
            // Each window starts from a fresh prediction network. Carrying the
            // LSTM state across a cut made the joint emit only blanks for the
            // start of the next window (whole sentences went missing).
            let mut state = self.initial_state()?;
            let before = tokens.len();
            self.decode_window(window, &mut state, &mut tokens, false)?;
            // Parakeet occasionally decodes a speech window to nothing; telling
            // the encoder the padding is valid usually recovers it.
            if tokens.len() == before && window.len() >= 2 * SAMPLE_RATE && rms(window) > 0.003 {
                state = self.initial_state()?;
                self.decode_window(window, &mut state, &mut tokens, true)?;
            }
        }
        Ok(self.detokenize(&tokens))
    }

    fn initial_state(&self) -> Result<DecoderState> {
        let h = Array::f32_zeros(&[LSTM_LAYERS, 1, PRED_DIM])?;
        let c = Array::f32_zeros(&[LSTM_LAYERS, 1, PRED_DIM])?;
        let out = self.run_decoder(self.blank, &h, &c)?;
        Ok(DecoderState {
            output: out.array("decoder")?,
            h: out.array("h_out")?,
            c: out.array("c_out")?,
        })
    }

    fn run_decoder(&self, token: i32, h: &Array, c: &Array) -> Result<Outputs> {
        let targets = Array::i32_from(&[1, 1], &[token])?;
        let length = Array::i32_from(&[1], &[1])?;
        self.decoder.predict(&[
            ("targets", &targets),
            ("target_length", &length),
            ("h_in", h),
            ("c_in", c),
        ])
    }

    fn decode_window(
        &self,
        window: &[f32],
        state: &mut DecoderState,
        tokens: &mut Vec<i32>,
        full_length: bool,
    ) -> Result<()> {
        let audio = Array::f32_zeros(&[1, WINDOW])?;
        audio.f32_mut()[..window.len()].copy_from_slice(window);
        let audio_len = Array::i32_from(&[1], &[window.len() as i32])?;
        let t0 = std::time::Instant::now();
        let pre = self
            .preprocessor
            .predict(&[("audio_signal", &audio), ("audio_length", &audio_len)])?;
        let mel = pre.array("mel")?;
        let mel_len = if full_length {
            Array::i32_from(&[1], &[MEL_FRAMES])?
        } else {
            pre.array("mel_length")?
        };

        let t1 = std::time::Instant::now();
        let enc = self.encoder.predict(&[("mel", &mel), ("mel_length", &mel_len)])?;
        let t2 = std::time::Instant::now();
        let encoder = enc.array("encoder")?;
        let shape = encoder.shape();
        if shape.len() != 3 || shape[1] != ENC_DIM {
            return Err(anyhow!("unexpected encoder output shape {shape:?}"));
        }
        let frames = (enc.array("encoder_length")?.get_i32(&[0]) as usize).min(shape[2]);
        // Transpose to [frame][dim] once so each step is a contiguous copy.
        let view = encoder.view();
        let mut by_frame = vec![0f32; frames * ENC_DIM];
        for d in 0..ENC_DIM {
            for t in 0..frames {
                by_frame[t * ENC_DIM + d] = view.f32(&[0, d, t]);
            }
        }

        let t3 = std::time::Instant::now();
        let mut joint_calls = 0usize;
        let mut decoder_calls = 0usize;
        let step = Array::f32_zeros(&[1, ENC_DIM, 1])?;
        let mut t = 0usize;
        let mut emitted_here = 0usize;
        while t < frames {
            step.f32_mut().copy_from_slice(&by_frame[t * ENC_DIM..(t + 1) * ENC_DIM]);
            let out = self
                .joint
                .predict(&[("encoder_step", &step), ("decoder_step", &state.output)])?;
            joint_calls += 1;
            let token = out.array("token_id")?.get_i32(&[0, 0, 0]);
            let duration = out.array("duration")?.get_i32(&[0, 0, 0]).max(0) as usize;

            if token == self.blank {
                t += duration.max(1);
                emitted_here = 0;
                continue;
            }

            tokens.push(token);
            decoder_calls += 1;
            let dec = self.run_decoder(token, &state.h, &state.c)?;
            state.output = dec.array("decoder")?;
            state.h = dec.array("h_out")?;
            state.c = dec.array("c_out")?;

            if duration == 0 {
                emitted_here += 1;
                if emitted_here >= MAX_SYMBOLS_PER_FRAME {
                    t += 1;
                    emitted_here = 0;
                }
            } else {
                t += duration;
                emitted_here = 0;
            }
        }
        log::debug!(
            "window {:.1}s: preprocess {:?} encoder {:?} transpose {:?} decode {:?} ({joint_calls} joint, {decoder_calls} decoder, {frames} frames)",
            window.len() as f32 / SAMPLE_RATE as f32,
            t1 - t0,
            t2 - t1,
            t3 - t2,
            t3.elapsed(),
        );
        Ok(())
    }

    fn detokenize(&self, tokens: &[i32]) -> String {
        let mut text = String::new();
        for &id in tokens {
            let Some(piece) = self.vocab.get(id as usize) else {
                continue;
            };
            if piece.starts_with('<') && piece.ends_with('>') {
                continue;
            }
            text.push_str(&piece.replace('▁', " "));
        }
        text.trim().to_string()
    }
}

/// Downloads the CoreML model files from Hugging Face into `dir`, reporting
/// progress (0..=1). Files land in a temporary folder that is renamed into
/// place only when everything arrived.
pub fn install(dir: &Path, cfg: CoremlParakeet, progress: impl Fn(f32)) -> Result<()> {
    let components = cfg.components();
    let staging = dir.with_extension("part");
    let _ = std::fs::remove_dir_all(&staging);
    super::hf::download(
        cfg.repo,
        |path| path == cfg.vocab || components.iter().any(|c| path.starts_with(&format!("{c}/"))),
        &staging,
        progress,
    )?;
    let complete = components.iter().all(|c| staging.join(c).join("coremldata.bin").exists())
        && staging.join(cfg.vocab).exists();
    if !complete {
        return Err(anyhow!("download is missing model files"));
    }
    let _ = std::fs::remove_dir_all(dir);
    std::fs::rename(&staging, dir)?;
    Ok(())
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}

/// Splits audio into ≤15 s windows, cutting each at the quietest 20 ms in
/// its last 3 seconds so words aren't chopped in half.
fn split_windows(samples: &[f32]) -> Vec<&[f32]> {
    split_at_pauses(samples, WINDOW)
}

/// Splits audio into pieces of at most `max` samples, cutting each at the
/// quietest 20 ms in its last 3 seconds.
pub(super) fn split_at_pauses(samples: &[f32], max: usize) -> Vec<&[f32]> {
    const HOP: usize = 320;
    let mut windows = Vec::new();
    let mut start = 0;
    while samples.len() - start > max {
        let search_from = start + max - 3 * SAMPLE_RATE;
        let search_to = start + max - HOP;
        let cut = (search_from..search_to)
            .step_by(HOP)
            .min_by(|&a, &b| {
                rms(&samples[a..a + HOP]).total_cmp(&rms(&samples[b..b + HOP]))
            })
            .unwrap_or(start + max)
            + HOP / 2;
        windows.push(&samples[start..cut]);
        start = cut;
    }
    if start < samples.len() {
        windows.push(&samples[start..]);
    }
    windows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_cover_everything_and_fit() {
        let samples: Vec<f32> = (0..(WINDOW * 2 + 12_345)).map(|i| ((i as f32) * 0.01).sin()).collect();
        let windows = split_windows(&samples);
        assert!(windows.iter().all(|w| w.len() <= WINDOW && !w.is_empty()));
        assert_eq!(windows.iter().map(|w| w.len()).sum::<usize>(), samples.len());
        assert_eq!(split_windows(&samples[..1000]).len(), 1);
    }
}
