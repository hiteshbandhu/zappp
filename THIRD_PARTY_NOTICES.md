# Third-party notices

zappp's own code is MIT licensed (see [LICENSE](LICENSE)). It builds on the
work below.

## Speech models

zappp does **not** ship model weights. They are downloaded on demand from the
sources below, onto the user's Mac, under their own licenses.

| Model in zappp | Source | License | Attribution |
|---|---|---|---|
| Parakeet v3 | [FluidInference/parakeet-tdt-0.6b-v3-coreml](https://huggingface.co/FluidInference/parakeet-tdt-0.6b-v3-coreml) — CoreML conversion of [nvidia/parakeet-tdt-0.6b-v3](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3) | [CC-BY 4.0](https://creativecommons.org/licenses/by/4.0/) | © NVIDIA Corporation; CoreML conversion by FluidInference |
| Parakeet v2 | [FluidInference/parakeet-tdt-0.6b-v2-coreml](https://huggingface.co/FluidInference/parakeet-tdt-0.6b-v2-coreml) — of [nvidia/parakeet-tdt-0.6b-v2](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v2) | CC-BY 4.0 | © NVIDIA Corporation; CoreML conversion by FluidInference |
| Bark 3.0 | [ConwayResearch/Underdog-Bark-3.0](https://huggingface.co/ConwayResearch/Underdog-Bark-3.0) | CC-BY 4.0 | Post-trained and compressed by Conway Research from nvidia/parakeet-tdt-0.6b-v3 (© NVIDIA) and moondream/parakeet-ultra (© Moondream) |
| Bark 3.0 Plus | [ConwayResearch/Underdog-Bark-3.0-Plus](https://huggingface.co/ConwayResearch/Underdog-Bark-3.0-Plus) | CC-BY 4.0 | As above |
| Parakeet v3 (CPU) | int8 ONNX export ([istupakov/parakeet-tdt-0.6b-v3-onnx](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx)), mirrored by the [Handy](https://handy.computer) project | CC-BY 4.0 | © NVIDIA Corporation; ONNX export by istupakov |
| Moonshine Base | [Useful Sensors Moonshine](https://github.com/usefulsensors/moonshine), mirrored by the Handy project | MIT | © Useful Sensors |

zappp's Bark engine (`src/engine/bark.rs`) is a Rust port of Conway
Research's reference `bark_coreml.py`. Each Bark install keeps the model's
`LICENSE` and `NOTICE` next to its weights.

## Libraries

| Library | License |
|---|---|
| [gpui](https://github.com/zed-industries/zed) (Zed Industries) — UI framework | Apache-2.0 |
| [transcribe-rs](https://crates.io/crates/transcribe-rs) — ONNX model runners | MIT |
| [ONNX Runtime](https://github.com/microsoft/onnxruntime) via [ort](https://github.com/pykeio/ort) | MIT / Apache-2.0 |
| [objc2](https://github.com/madsmtm/objc2) family — AppKit, CoreML, Foundation bindings | MIT |
| [cpal](https://github.com/RustAudio/cpal) — audio capture | Apache-2.0 |
| [rustfft](https://github.com/ejmahler/RustFFT), [half](https://github.com/starkat99/half-rs) | MIT / Apache-2.0 |
| [ureq](https://github.com/algesten/ureq), serde, chrono, anyhow, futures, log | MIT / Apache-2.0 |
| [Lucide](https://lucide.dev) icons (`crates/zui/assets/icons`) | ISC |

Run `cargo tree` for the complete dependency list.
