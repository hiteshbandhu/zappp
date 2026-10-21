//! Underdog Bark 3.0 (Conway Research, CC-BY 4.0): a compressed Parakeet TDT
//! 0.6B. A port of the reference `coreml/ane/bark_coreml.py`:
//!
//! - frontend on the CPU: pre-emphasis, centred 512-point STFT, 128 slaney
//!   mel bands, log, per-utterance normalization;
//! - encoder in CoreML on the Neural Engine. The package holds 8 s and 15 s
//!   functions (`w8`, `w15`), but only the default (`w15`) gets its Neural
//!   Engine plan cached by macOS: selecting a function rebuilt both plans on
//!   every launch (~270 s on an M2) for no speed gain on short clips. So
//!   every piece runs through the 15 s window, masked — the fixed window
//!   Bark's GPU package uses;
//! - greedy TDT decoding on the CPU: a 2-layer LSTM and the joint head, with
//!   int8 weights scaled per row (`*.q` × `*.s`).
//!
//! Each piece of audio gets 0.25 s of leading silence (the setting the model
//! was evaluated with); audio over 15 s is cut at pauses.

use std::{collections::HashSet, path::Path, sync::Arc};

use anyhow::{Context as _, Result, anyhow};
use rustfft::{Fft, FftPlanner, num_complex::Complex};

use super::{
    coreml::{Array, Model, Units},
    parakeet_coreml::split_at_pauses,
};
use crate::models::CoremlBark;

const SR: usize = 16_000;
const HOP: usize = 160;
const NFFT: usize = 512;
const WIN: usize = 400;
const MELS: usize = 128;
const BINS: usize = NFFT / 2 + 1;
/// The encoder's window in seconds (its default function, `w15`).
const WINDOW_SECS: usize = 15;
const LEAD_IN: usize = SR / 4;
const ENC_DIM: usize = 640;

#[link(name = "Accelerate", kind = "framework")]
unsafe extern "C" {
    fn cblas_sgemv(
        order: i32,
        trans: i32,
        m: i32,
        n: i32,
        alpha: f32,
        a: *const f32,
        lda: i32,
        x: *const f32,
        incx: i32,
        beta: f32,
        y: *mut f32,
        incy: i32,
    );
}

/// A row-major `rows × cols` matrix.
struct Matrix {
    data: Vec<f32>,
    rows: usize,
    cols: usize,
}

impl Matrix {
    /// `y += self · x`, on Accelerate. The lengths are checked in release
    /// builds too: BLAS reads through raw pointers.
    fn mul_add(&self, x: &[f32], y: &mut [f32]) {
        assert!(
            x.len() == self.cols && y.len() == self.rows && self.data.len() == self.rows * self.cols,
            "matrix shape mismatch"
        );
        const ROW_MAJOR: i32 = 101;
        const NO_TRANS: i32 = 111;
        unsafe {
            cblas_sgemv(
                ROW_MAJOR,
                NO_TRANS,
                self.rows as i32,
                self.cols as i32,
                1.0,
                self.data.as_ptr(),
                self.cols as i32,
                x.as_ptr(),
                1,
                1.0,
                y.as_mut_ptr(),
                1,
            );
        }
    }

    fn row(&self, r: usize) -> &[f32] {
        &self.data[r * self.cols..(r + 1) * self.cols]
    }
}

/// `y = w · x + b`.
struct Linear {
    w: Matrix,
    b: Vec<f32>,
}

impl Linear {
    fn apply(&self, x: &[f32]) -> Vec<f32> {
        let mut y = self.b.clone();
        self.w.mul_add(x, &mut y);
        y
    }
}

// ── .npy files (the unpacked decoder.npz) ───────────────────────────────

struct Npy {
    descr: String,
    shape: Vec<usize>,
    data: Vec<u8>,
}

fn read_npy(path: &Path) -> Result<Npy> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    if bytes.len() < 10 || &bytes[..6] != b"\x93NUMPY" {
        return Err(anyhow!("{} is not a .npy file", path.display()));
    }
    let (header_len, start) = if bytes[6] == 1 {
        (u16::from_le_bytes([bytes[8], bytes[9]]) as usize, 10)
    } else {
        (u32::from_le_bytes(bytes[8..12].try_into()?) as usize, 12)
    };
    let header = std::str::from_utf8(&bytes[start..start + header_len])?;
    let descr = header
        .split("'descr': '")
        .nth(1)
        .and_then(|s| s.split('\'').next())
        .ok_or_else(|| anyhow!("bad .npy header"))?
        .to_string();
    let shape = header
        .split('(')
        .nth(1)
        .and_then(|s| s.split(')').next())
        .ok_or_else(|| anyhow!("bad .npy header"))?
        .split(',')
        .filter_map(|d| d.trim().parse().ok())
        .collect();
    Ok(Npy {
        descr,
        shape,
        data: bytes[start + header_len..].to_vec(),
    })
}

impl Npy {
    fn to_f32(&self) -> Result<Vec<f32>> {
        Ok(match self.descr.as_str() {
            "|i1" => self.data.iter().map(|&b| b as i8 as f32).collect(),
            "<f2" => self
                .data
                .chunks_exact(2)
                .map(|c| half::f16::from_le_bytes([c[0], c[1]]).to_f32())
                .collect(),
            "<f4" => self
                .data
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect(),
            other => return Err(anyhow!("unsupported .npy type {other}")),
        })
    }
}

/// Loads `name` from the decoder folder, dequantizing `name.q × name.s`
/// (int8 rows times a per-row scale) when the weight is quantized.
fn load_weight(dir: &Path, name: &str) -> Result<(Vec<f32>, Vec<usize>)> {
    let quantized = dir.join(format!("{name}.q.npy"));
    if quantized.exists() {
        let q = read_npy(&quantized)?;
        let scales = read_npy(&dir.join(format!("{name}.s.npy")))?.to_f32()?;
        let cols = q.shape.get(1).copied().unwrap_or(1).max(1);
        let mut w = q.to_f32()?;
        if scales.len() != w.len() / cols {
            return Err(anyhow!("{name}: one scale per row expected"));
        }
        for (row, scale) in w.chunks_mut(cols).zip(&scales) {
            row.iter_mut().for_each(|v| *v *= scale);
        }
        Ok((w, q.shape))
    } else {
        let n = read_npy(&dir.join(format!("{name}.npy")))?;
        Ok((n.to_f32()?, n.shape))
    }
}

fn load_matrix(dir: &Path, name: &str) -> Result<Matrix> {
    let (data, shape) = load_weight(dir, name)?;
    let [rows, cols] = shape[..] else {
        return Err(anyhow!("{name} is not a matrix"));
    };
    if rows == 0 || cols == 0 || rows.checked_mul(cols) != Some(data.len()) {
        return Err(anyhow!("{name}: {} values don't fill a {rows}×{cols} matrix", data.len()));
    }
    Ok(Matrix { data, rows, cols })
}

fn load_vector(dir: &Path, name: &str) -> Result<Vec<f32>> {
    Ok(load_weight(dir, name)?.0)
}

// ── Frontend ────────────────────────────────────────────────────────────

struct Frontend {
    /// Mel filter bank, `MELS × BINS`.
    mel: Matrix,
    window: Vec<f32>,
    fft: Arc<dyn Fft<f32>>,
}

impl Frontend {
    fn new(mel: Matrix) -> Self {
        // Symmetric Hann of 400 samples centred in the 512-sample frame
        // (= torch.hann_window(periodic=False)).
        let mut window = vec![0f32; NFFT];
        let offset = (NFFT - WIN) / 2;
        for n in 0..WIN {
            let phase = 2. * std::f32::consts::PI * n as f32 / (WIN - 1) as f32;
            window[offset + n] = 0.5 - 0.5 * phase.cos();
        }
        let fft = FftPlanner::new().plan_fft_forward(NFFT);
        Self { mel, window, fft }
    }

    /// Normalized log-mel features (`frames × MELS`) and how many frames
    /// hold real audio.
    fn features(&self, x: &[f32]) -> (Vec<f32>, usize) {
        let len = x.len();
        // Pre-emphasis, then centre-pad by half a frame on each side.
        let mut padded = vec![0f32; len + NFFT];
        for i in 0..len {
            padded[NFFT / 2 + i] = if i == 0 { x[0] } else { x[i] - 0.97 * x[i - 1] };
        }
        let frames = 1 + (padded.len() - NFFT) / HOP;
        let mut feats = vec![0f32; frames * MELS];
        let mut buf = vec![Complex::new(0f32, 0f32); NFFT];
        let mut power = vec![0f32; BINS];
        for f in 0..frames {
            let start = f * HOP;
            for k in 0..NFFT {
                buf[k] = Complex::new(padded[start + k] * self.window[k], 0.);
            }
            self.fft.process(&mut buf);
            for (p, c) in power.iter_mut().zip(&buf) {
                *p = c.re * c.re + c.im * c.im;
            }
            let row = &mut feats[f * MELS..(f + 1) * MELS];
            row.fill(0.);
            self.mel.mul_add(&power, row);
            row.iter_mut().for_each(|v| *v = (*v + 2f32.powi(-24)).ln());
        }

        // Normalize each mel band over the real frames; zero the rest.
        let valid = (len / HOP).min(frames);
        for m in 0..MELS {
            let mean = (0..valid).map(|f| feats[f * MELS + m]).sum::<f32>() / valid.max(1) as f32;
            let var = (0..valid).map(|f| (feats[f * MELS + m] - mean).powi(2)).sum::<f32>()
                / valid.saturating_sub(1).max(1) as f32;
            let std = var.sqrt() + 1e-5;
            for f in 0..frames {
                let v = &mut feats[f * MELS + m];
                *v = if f < valid { (*v - mean) / std } else { 0. };
            }
        }
        (feats, valid)
    }
}

/// Encoder output length after three stride-2 convolutions.
fn encoded_len(mut n: usize) -> usize {
    for _ in 0..3 {
        n = (n.saturating_sub(1)) / 2 + 1;
    }
    n
}

// ── TDT decoder ─────────────────────────────────────────────────────────

struct Lstm {
    input: Matrix,
    hidden: Matrix,
    bias: Vec<f32>,
}

struct Decoder {
    embedding: Matrix,
    layers: Vec<Lstm>,
    projector: Linear,
    head: Linear,
    vocab_size: usize,
    blank: usize,
    durations: Vec<usize>,
    max_symbols: usize,
}

type State = Vec<(Vec<f32>, Vec<f32>)>;

fn sigmoid(x: f32) -> f32 {
    1. / (1. + (-x).exp())
}

impl Decoder {
    /// Checks that every weight agrees with every other and with the
    /// vocabulary, so decoding can't index or multiply out of bounds even if
    /// the downloaded files are corrupt or hostile.
    fn validate(&self) -> Result<()> {
        let hidden = self.embedding.cols;
        let gates = 4 * hidden;
        let mut input = hidden;
        for (i, l) in self.layers.iter().enumerate() {
            let ok = (l.input.rows, l.input.cols) == (gates, input)
                && (l.hidden.rows, l.hidden.cols) == (gates, hidden)
                && l.bias.len() == gates;
            if !ok {
                return Err(anyhow!("Bark LSTM layer {i} has inconsistent shapes"));
            }
            input = hidden;
        }
        let projector = &self.projector;
        if projector.w.cols != hidden || projector.w.rows != ENC_DIM || projector.b.len() != ENC_DIM {
            return Err(anyhow!("Bark decoder projector has unexpected shape"));
        }
        let head = &self.head;
        let outputs = self.vocab_size + self.durations.len();
        if head.w.cols != ENC_DIM || head.w.rows < outputs || head.b.len() != head.w.rows {
            return Err(anyhow!("Bark joint head has unexpected shape"));
        }
        if self.layers.is_empty()
            || self.durations.is_empty()
            || self.max_symbols == 0
            || self.blank >= self.vocab_size
            || self.embedding.rows < self.vocab_size
        {
            return Err(anyhow!("Bark vocabulary settings don't match its weights"));
        }
        Ok(())
    }

    /// Runs the prediction network on one token.
    fn step(&self, token: usize, state: &State) -> (Vec<f32>, State) {
        let mut x = self.embedding.row(token).to_vec();
        let mut next = Vec::with_capacity(state.len());
        for (layer, (h, c)) in self.layers.iter().zip(state) {
            let hidden = h.len();
            let mut gates = layer.bias.clone();
            layer.input.mul_add(&x, &mut gates);
            layer.hidden.mul_add(h, &mut gates);
            let mut c = c.clone();
            let mut h = vec![0f32; hidden];
            for j in 0..hidden {
                let i = sigmoid(gates[j]);
                let f = sigmoid(gates[hidden + j]);
                let g = gates[2 * hidden + j].tanh();
                let o = sigmoid(gates[3 * hidden + j]);
                c[j] = f * c[j] + i * g;
                h[j] = o * c[j].tanh();
            }
            x = h.clone();
            next.push((h, c));
        }
        (self.projector.apply(&x), next)
    }

    /// Greedy TDT decoding of `frames × ENC_DIM` encoder output.
    fn decode(&self, enc: &[f32]) -> Vec<usize> {
        let frames = enc.len() / ENC_DIM;
        let hidden = self.embedding.cols;
        let mut state: State = self.layers.iter().map(|_| (vec![0.; hidden], vec![0.; hidden])).collect();
        let (mut dec, s) = self.step(self.blank, &state);
        state = s;
        let (mut t, mut symbols, mut out) = (0usize, 0usize, Vec::new());
        let mut joint = vec![0f32; ENC_DIM];
        while t < frames {
            for (j, (e, d)) in joint.iter_mut().zip(enc[t * ENC_DIM..].iter().zip(&dec)) {
                *j = (e + d).max(0.);
            }
            let logits = self.head.apply(&joint);
            let token = argmax(&logits[..self.vocab_size]);
            let duration_logits = &logits[self.vocab_size..self.vocab_size + self.durations.len()];
            let mut duration = self.durations[argmax(duration_logits)];
            if token == self.blank {
                t += duration.max(1);
                symbols = 0;
            } else {
                out.push(token);
                let (d, s) = self.step(token, &state);
                dec = d;
                state = s;
                symbols += 1;
                if duration == 0 && symbols >= self.max_symbols {
                    duration = 1;
                }
                if duration > 0 {
                    symbols = 0;
                }
                t += duration;
            }
        }
        out
    }
}

fn argmax(values: &[f32]) -> usize {
    values
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

// ── The model ───────────────────────────────────────────────────────────

pub struct Bark {
    encoder: Model,
    frontend: Frontend,
    decoder: Decoder,
    pieces: Vec<String>,
    special: HashSet<usize>,
}

impl Bark {
    pub fn load(dir: &Path, units: Units) -> Result<Self> {
        let vocab: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("vocab.json")).context("reading Bark vocabulary")?)?;
        let ids = |key: &str| -> Vec<usize> {
            vocab[key]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_u64()).map(|v| v as usize).collect())
                .unwrap_or_default()
        };
        let number = |key: &str| vocab[key].as_u64().map(|v| v as usize).ok_or_else(|| anyhow!("vocab.json lacks {key}"));
        let pieces: Vec<String> = vocab["pieces"]
            .as_array()
            .ok_or_else(|| anyhow!("vocab.json lacks pieces"))?
            .iter()
            .map(|p| p.as_str().unwrap_or_default().to_string())
            .collect();

        let weights = dir.join("decoder");
        let layers = (0..2)
            .map(|l| -> Result<Lstm> {
                let mut bias = load_vector(&weights, &format!("decoder.lstm.bias_ih_l{l}"))?;
                for (b, h) in bias.iter_mut().zip(load_vector(&weights, &format!("decoder.lstm.bias_hh_l{l}"))?) {
                    *b += h;
                }
                Ok(Lstm {
                    input: load_matrix(&weights, &format!("decoder.lstm.weight_ih_l{l}"))?,
                    hidden: load_matrix(&weights, &format!("decoder.lstm.weight_hh_l{l}"))?,
                    bias,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let decoder = Decoder {
            embedding: load_matrix(&weights, "decoder.embedding.weight")?,
            layers,
            projector: Linear {
                w: load_matrix(&weights, "decoder.decoder_projector.weight")?,
                b: load_vector(&weights, "decoder.decoder_projector.bias")?,
            },
            head: Linear {
                w: load_matrix(&weights, "joint.head.weight")?,
                b: load_vector(&weights, "joint.head.bias")?,
            },
            vocab_size: number("vocab_size")?,
            blank: number("blank_id")?,
            durations: ids("durations"),
            max_symbols: number("max_symbols_per_step")?,
        };
        decoder.validate()?;
        let mel = load_matrix(&weights, "mel_filters")?;
        if (mel.rows, mel.cols) != (MELS, BINS) {
            return Err(anyhow!("mel_filters is {}×{}, expected {MELS}×{BINS}", mel.rows, mel.cols));
        }
        let frontend = Frontend::new(mel);

        let encoder = Model::load(&dir.join("BarkEncoder.mlmodelc"), units)?;

        let this = Self {
            encoder,
            frontend,
            decoder,
            pieces,
            special: ids("special_ids").into_iter().collect(),
        };
        // Run once so the Neural Engine plan is ready before the first
        // real dictation.
        this.transcribe_piece(&vec![0.; SR])?;
        Ok(this)
    }

    pub fn transcribe(&self, samples: &[f32]) -> Result<String> {
        let max_piece = WINDOW_SECS * SR - LEAD_IN;
        let mut text = Vec::new();
        for piece in split_at_pauses(samples, max_piece) {
            let ids = self.transcribe_piece(piece)?;
            let words = self.detokenize(&ids);
            if !words.is_empty() {
                text.push(words);
            }
        }
        Ok(text.join(" "))
    }

    fn transcribe_piece(&self, piece: &[f32]) -> Result<Vec<usize>> {
        let mut x = vec![0f32; LEAD_IN];
        x.extend_from_slice(piece);
        let (feats, valid) = self.frontend.features(&x);
        let frames = WINDOW_SECS * 100 + 1;

        let mut input = vec![0f32; frames * MELS];
        let n = (feats.len() / MELS).min(frames);
        input[..n * MELS].copy_from_slice(&feats[..n * MELS]);
        let mask: Vec<i32> = (0..frames).map(|f| (f < valid) as i32).collect();
        let outputs = self.encoder.predict(&[
            ("features", &Array::f16_from(&[1, frames, MELS], &input)?),
            ("mask", &Array::i32_from(&[1, frames], &mask)?),
        ])?;
        let encoded = outputs.array("encoded")?;
        let shape = encoded.shape();
        if shape.len() != 3 || shape[2] != ENC_DIM {
            return Err(anyhow!("the Bark encoder returned shape {shape:?}"));
        }
        let available = shape[1];
        let len = encoded_len(valid).min(available);
        let view = encoded.view();
        let mut enc = vec![0f32; len * ENC_DIM];
        for t in 0..len {
            for d in 0..ENC_DIM {
                enc[t * ENC_DIM + d] = view.f32(&[0, t, d]);
            }
        }
        if enc.iter().any(|v| !v.is_finite()) {
            return Err(anyhow!("the Bark encoder produced invalid output"));
        }
        let started = std::time::Instant::now();
        let tokens = self.decoder.decode(&enc);
        log::debug!(
            "bark piece {:.2}s: {valid} feature frames → {len} encoded, {} tokens, decode {:?}",
            piece.len() as f32 / SR as f32,
            tokens.len(),
            started.elapsed()
        );
        Ok(tokens)
    }

    fn detokenize(&self, ids: &[usize]) -> String {
        ids.iter()
            .filter(|i| !self.special.contains(i))
            .filter_map(|&i| self.pieces.get(i))
            .map(String::as_str)
            .collect::<String>()
            .replace('▁', " ")
            .trim()
            .to_string()
    }
}

/// Downloads a Bark build's Neural Engine package, compiles the encoder and
/// unpacks the decoder weights into `dir`.
pub fn install(dir: &Path, cfg: CoremlBark, progress: impl Fn(f32)) -> Result<()> {
    let staging = dir.with_extension("part");
    let build = dir.with_extension("build");
    for d in [&staging, &build] {
        let _ = std::fs::remove_dir_all(d);
    }
    super::hf::download(
        cfg.repo,
        |path| {
            (path.starts_with("coreml/ane/") && !path.ends_with(".py"))
                || path == "coreml/LICENSE"
                || path == "coreml/NOTICE"
        },
        &staging,
        progress,
    )?;
    let ane = staging.join("coreml/ane");
    std::fs::create_dir_all(&build)?;
    super::coreml::compile(&ane.join("BarkEncoder.mlpackage"), &build.join("BarkEncoder.mlmodelc"))?;
    let status = std::process::Command::new("unzip")
        .arg("-oq")
        .arg(ane.join("decoder.npz"))
        .arg("-d")
        .arg(build.join("decoder"))
        .status()
        .context("could not run unzip")?;
    if !status.success() {
        return Err(anyhow!("could not unpack the Bark decoder weights"));
    }
    std::fs::copy(ane.join("vocab.json"), build.join("vocab.json"))?;
    // Keep the CC-BY licence and attribution next to the weights.
    for file in ["LICENSE", "NOTICE"] {
        let _ = std::fs::copy(staging.join("coreml").join(file), build.join(file));
    }
    let _ = std::fs::remove_dir_all(&staging);
    let _ = std::fs::remove_dir_all(dir);
    std::fs::rename(&build, dir)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoder_length_matches_three_stride_two_convs() {
        // 8 s + 1 frame → 801 → 401 → 201 → 101
        assert_eq!(encoded_len(801), 101);
        assert_eq!(encoded_len(1), 1);
        assert_eq!(encoded_len(1501), 188);
    }

    fn matrix(rows: usize, cols: usize) -> Matrix {
        Matrix {
            data: vec![0.; rows * cols],
            rows,
            cols,
        }
    }

    fn tiny_decoder(head_rows: usize) -> Decoder {
        let h = 4;
        Decoder {
            embedding: matrix(10, h),
            layers: vec![Lstm {
                input: matrix(4 * h, h),
                hidden: matrix(4 * h, h),
                bias: vec![0.; 4 * h],
            }],
            projector: Linear {
                w: matrix(ENC_DIM, h),
                b: vec![0.; ENC_DIM],
            },
            head: Linear {
                w: matrix(head_rows, ENC_DIM),
                b: vec![0.; head_rows],
            },
            vocab_size: 10,
            blank: 9,
            durations: vec![0, 1, 2],
            max_symbols: 10,
        }
    }

    #[test]
    fn decoder_shapes_are_validated() {
        assert!(tiny_decoder(13).validate().is_ok());
        // A joint head too small for vocabulary + durations is refused
        // instead of being read out of bounds.
        assert!(tiny_decoder(12).validate().is_err());
        let mut bad = tiny_decoder(13);
        bad.blank = 10;
        assert!(bad.validate().is_err());
    }

    #[test]
    fn frontend_shapes_and_normalization() {
        let mel = Matrix {
            data: vec![1. / BINS as f32; MELS * BINS],
            rows: MELS,
            cols: BINS,
        };
        let front = Frontend::new(mel);
        let x: Vec<f32> = (0..SR).map(|i| (i as f32 * 0.05).sin() * 0.1).collect();
        let (feats, valid) = front.features(&x);
        assert_eq!(valid, SR / HOP);
        assert_eq!(feats.len() / MELS, 1 + SR / HOP);
        // Padding frames beyond `valid` are zeroed.
        assert!(feats[valid * MELS..].iter().all(|v| *v == 0.));
    }
}
