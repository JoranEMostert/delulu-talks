<p align="center">
  <img src="public/delulu-talks-icon.svg" alt="Delulu Talks logo" width="120" />
</p>

<h1 align="center">Delulu Talks</h1>

<p align="center">
  Chaotic blue-scribble desktop dictation app built with Tauri, Bun, and Qwen ASR.
</p>

<p align="center">
  <a href="https://github.com/JoranEMostert/delulu-talks/actions/workflows/ci.yml">
    <img src="https://github.com/JoranEMostert/delulu-talks/actions/workflows/ci.yml/badge.svg" alt="Quick Checks" />
  </a>
  <a href="https://github.com/JoranEMostert/delulu-talks/actions/workflows/release.yml">
    <img src="https://github.com/JoranEMostert/delulu-talks/actions/workflows/release.yml/badge.svg" alt="Release Build" />
  </a>
  <img src="https://img.shields.io/badge/Tauri-2.x-24C8DB?logo=tauri&logoColor=white" alt="Tauri" />
  <img src="https://img.shields.io/badge/Bun-1.x-f9f1e1?logo=bun&logoColor=111" alt="Bun" />
  <img src="https://img.shields.io/badge/ASR-Qwen3--ASR-blue" alt="Qwen ASR" />
</p>

## Features

- Global shortcut dictation with `hold` (default) or `toggle` mode
- Two ASR models: `Qwen3-ASR-1.7B` and `Qwen3-ASR-0.6B`
- Searchable language selector with full supported language list
- Tray-first behavior with floating voice activity pill
- Transcript insertion into the focused field
- Startup bootstrap flow (Python check, dependency install, model warmup)

## Tech Stack

- Frontend: React + Vite + Tailwind
- Desktop runtime: Tauri v2 (Rust backend)
- Audio capture: CPAL
- ASR sidecar: Python (`qwen-asr`, `torch`, `torchvision`)

## Quick Start

```bash
bun install
bun run tauri dev
```

## Python Sidecar Setup

Delulu Talks uses `src-tauri/python/qwen_asr_transcribe.py` for inference.

Install Python dependencies:

```bash
pip install -U qwen-asr torch torchvision
```

You can configure the interpreter in Settings (`python`, `py`, or full path).

## Build

```bash
bun run build
```

For desktop build:

```bash
bun run tauri build
```

## CI / Release

- `Quick Checks` workflow: fast PR/build verification (`bun run build` + `cargo check`)
- `Build and Release Tauri App` workflow: cross-platform artifacts on `main` push
