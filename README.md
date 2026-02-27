# Delulu Talks

Tray-first Tauri dictation app with:

- Global shortcut dictation
- Hold-to-talk (default) or toggle recording mode
- Qwen model switch (`Qwen3-ASR-1.7B` / `Qwen3-ASR-0.6B`)
- Microphone input selector (system default or specific device)
- Small floating pill overlay while listening/transcribing
- Transcript insertion at cursor target input
- Launch bootstrap checks (auto-installs Python deps + warms selected model)

## Run

```bash
bun install
bun run tauri dev
```

## Python ASR Sidecar

The app runs a Python script at `src-tauri/python/qwen_asr_transcribe.py`.

Install dependencies in your Python environment:

```bash
pip install -U torch qwen-asr
```

You can configure the interpreter command from Settings (`python`, `py`, or full path).
