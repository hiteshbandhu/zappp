//! Microphone capture. The cpal stream lives on its own thread (it is not
//! `Send` on macOS); callers drive it with start/stop and read a live level.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
        mpsc,
    },
    thread,
};

use anyhow::{Context as _, Result, anyhow};
use cpal::{
    FromSample, Sample as _, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

pub const TARGET_RATE: u32 = 16_000;

enum Cmd {
    Start(Option<String>, mpsc::Sender<Result<()>>),
    Stop(mpsc::Sender<Recording>),
}

pub struct Recording {
    /// Mono 16 kHz samples.
    pub samples: Vec<f32>,
}

/// Raw capture shared between the audio callback and readers.
#[derive(Default)]
struct Capture {
    samples: Vec<f32>,
    rate: u32,
}

pub struct Recorder {
    tx: mpsc::Sender<Cmd>,
    level: Arc<AtomicU32>,
    capture: Arc<Mutex<Capture>>,
}

impl Recorder {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<Cmd>();
        let level = Arc::new(AtomicU32::new(0));
        let capture = Arc::new(Mutex::new(Capture::default()));
        let (thread_level, thread_capture) = (level.clone(), capture.clone());
        thread::Builder::new()
            .name("zappp-audio".into())
            .spawn(move || audio_thread(rx, thread_level, thread_capture))
            .expect("spawn audio thread");
        Self { tx, level, capture }
    }

    /// Starts capturing from `device` (by name), or the system default.
    pub fn start(&self, device: Option<String>) -> Result<()> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx.send(Cmd::Start(device, reply_tx))?;
        reply_rx.recv()?
    }

    pub fn stop(&self) -> Recording {
        let (reply_tx, reply_rx) = mpsc::channel();
        if self.tx.send(Cmd::Stop(reply_tx)).is_err() {
            return Recording { samples: vec![] };
        }
        reply_rx.recv().unwrap_or(Recording { samples: vec![] })
    }

    /// The most recent `max_secs` of audio captured so far, as 16 kHz mono,
    /// without stopping. Used for the live preview.
    pub fn snapshot(&self, max_secs: f32) -> Vec<f32> {
        let (raw, rate) = {
            let capture = self.capture.lock().unwrap();
            if capture.rate == 0 {
                return vec![];
            }
            let keep = (max_secs * capture.rate as f32) as usize;
            let start = capture.samples.len().saturating_sub(keep);
            (capture.samples[start..].to_vec(), capture.rate)
        };
        resample(&raw, rate, TARGET_RATE)
    }

    /// Smoothed input level, roughly 0..=1.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }
}

/// Names of the available input devices; the first is the system default.
pub fn input_devices() -> Vec<String> {
    let host = cpal::default_host();
    let default = host.default_input_device().and_then(|d| d.name().ok());
    let mut names: Vec<String> = host
        .input_devices()
        .map(|devices| devices.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    names.dedup();
    if let Some(default) = default {
        names.retain(|n| *n != default);
        names.insert(0, default);
    }
    names
}

fn audio_thread(rx: mpsc::Receiver<Cmd>, level: Arc<AtomicU32>, capture: Arc<Mutex<Capture>>) {
    let mut stream: Option<cpal::Stream> = None;
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Start(device, reply) => {
                stream = None;
                *capture.lock().unwrap() = Capture::default();
                let result = open_stream(device.as_deref(), level.clone(), capture.clone()).map(|s| {
                    stream = Some(s);
                });
                let _ = reply.send(result);
            }
            Cmd::Stop(reply) => {
                drop(stream.take());
                let Capture { samples, rate } = std::mem::take(&mut *capture.lock().unwrap());
                let recording = Recording {
                    samples: if rate == 0 { vec![] } else { resample(&samples, rate, TARGET_RATE) },
                };
                level.store(0f32.to_bits(), Ordering::Relaxed);
                let _ = reply.send(recording);
            }
        }
    }
}

fn open_stream(device: Option<&str>, level: Arc<AtomicU32>, capture: Arc<Mutex<Capture>>) -> Result<cpal::Stream> {
    let host = cpal::default_host();
    let chosen = device.and_then(|name| {
        host.input_devices()
            .ok()?
            .find(|d| d.name().is_ok_and(|n| n == name))
    });
    if device.is_some() && chosen.is_none() {
        log::warn!("microphone {device:?} not found, using the default");
    }
    let device = chosen
        .or_else(|| host.default_input_device())
        .ok_or_else(|| anyhow!("No microphone found"))?;
    log::info!("recording from {}", device.name().unwrap_or_default());
    let config = device
        .default_input_config()
        .context("Could not read microphone format")?;
    let channels = config.channels() as usize;
    capture.lock().unwrap().rate = config.sample_rate().0;

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&device, &config.into(), channels, capture, level),
        cpal::SampleFormat::I16 => build::<i16>(&device, &config.into(), channels, capture, level),
        cpal::SampleFormat::I32 => build::<i32>(&device, &config.into(), channels, capture, level),
        cpal::SampleFormat::U16 => build::<u16>(&device, &config.into(), channels, capture, level),
        other => return Err(anyhow!("Unsupported microphone format {other:?}")),
    }?;
    stream.play().context("Could not start the microphone")?;
    Ok(stream)
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    capture: Arc<Mutex<Capture>>,
    level: Arc<AtomicU32>,
) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let mut smoothed = 0f32;
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _| {
            let mut sum_sq = 0f32;
            let mut frames = 0usize;
            let mut capture = capture.lock().unwrap();
            let buf = &mut capture.samples;
            for frame in data.chunks(channels) {
                let mono = frame.iter().map(|s| f32::from_sample(*s)).sum::<f32>() / channels as f32;
                buf.push(mono);
                sum_sq += mono * mono;
                frames += 1;
            }
            drop(capture);
            if frames > 0 {
                let rms = (sum_sq / frames as f32).sqrt();
                // Perceptual-ish curve: quiet speech should still move the bars.
                let target = (rms * 9.0).powf(0.7).min(1.0);
                let k = if target > smoothed { 0.55 } else { 0.18 };
                smoothed += (target - smoothed) * k;
                level.store(smoothed.to_bits(), Ordering::Relaxed);
            }
        },
        |err| log::error!("microphone stream error: {err}"),
        None,
    )?;
    Ok(stream)
}

/// Resamples mono audio. Box-filters first when downsampling to limit aliasing,
/// then interpolates linearly.
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let filtered: Vec<f32> = if ratio > 1.0 {
        let w = ratio.round().max(1.0) as usize;
        let mut acc = 0f32;
        let mut out = Vec::with_capacity(input.len());
        for (i, s) in input.iter().enumerate() {
            acc += s;
            if i >= w {
                acc -= input[i - w];
            }
            out.push(acc / w.min(i + 1) as f32);
        }
        out
    } else {
        input.to_vec()
    };
    let out_len = (input.len() as f64 / ratio) as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;
            let a = filtered[idx.min(filtered.len() - 1)];
            let b = filtered[(idx + 1).min(filtered.len() - 1)];
            a + (b - a) * frac
        })
        .collect()
}

/// Trims leading/trailing silence with a simple energy gate, keeping a little
/// padding so word onsets survive.
/// Raises quiet recordings to a normal level (peak ≈ 0.9, at most 30× gain)
/// so a low microphone input volume doesn't get mistaken for silence.
pub fn normalize(samples: &mut [f32]) {
    let peak = samples.iter().fold(0f32, |m, s| m.max(s.abs()));
    if peak <= f32::EPSILON {
        return;
    }
    let gain = (0.9 / peak).min(30.0);
    if gain > 1.0 {
        samples.iter_mut().for_each(|s| *s *= gain);
    }
}

pub fn trim_silence(samples: &[f32]) -> &[f32] {
    const WIN: usize = 320; // 20 ms
    const THRESH: f32 = 0.006;
    const PAD: usize = 4800; // 300 ms
    let loud = |chunk: &[f32]| {
        (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len().max(1) as f32).sqrt() > THRESH
    };
    let chunks: Vec<&[f32]> = samples.chunks(WIN).collect();
    let Some(first) = chunks.iter().position(|c| loud(c)) else {
        return &[];
    };
    let last = chunks.iter().rposition(|c| loud(c)).unwrap_or(first);
    let start = (first * WIN).saturating_sub(PAD);
    let end = ((last + 1) * WIN + PAD).min(samples.len());
    &samples[start..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_speech_survives_normalize_and_trim() {
        // 1 s of near-silence, 1 s of a quiet 220 Hz "voice", 1 s of near-silence.
        let mut samples: Vec<f32> = (0..TARGET_RATE as usize * 3)
            .map(|i| {
                let t = i as f32 / TARGET_RATE as f32;
                let voice = if (1.0..2.0).contains(&t) { 0.004 * (t * 220.0 * std::f32::consts::TAU).sin() } else { 0.0 };
                voice + 0.0002 * ((i * 7919 % 101) as f32 / 50.0 - 1.0)
            })
            .collect();
        assert!(trim_silence(&samples).is_empty(), "quiet voice is below the raw threshold");
        normalize(&mut samples);
        let kept = trim_silence(&samples).len();
        assert!(kept >= TARGET_RATE as usize, "kept only {kept} samples");
    }
}
