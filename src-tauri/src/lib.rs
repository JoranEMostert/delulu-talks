use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use arboard::Clipboard;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleFormat, Stream, StreamConfig,
};
use enigo::{
    Direction::{Click, Press, Release},
    Enigo, Key, Keyboard, Settings,
};
use hound::{SampleFormat as WavSampleFormat, WavSpec, WavWriter};
use serde::{Deserialize, Serialize};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, PhysicalPosition, Position, State, WebviewUrl,
    WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

const SETTINGS_FILE: &str = "settings.json";
const DICTATION_EVENT: &str = "dictation-state";
const TRANSCRIPT_EVENT: &str = "dictation-transcript";
const OVERLAY_LABEL: &str = "overlay";
const DEFAULT_INPUT_DEVICE: &str = "default";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum RecordingMode {
    Hold,
    Toggle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ModelOption {
    Qwen3Asr17b,
    Qwen3Asr06b,
}

impl ModelOption {
    fn as_hf_id(self) -> &'static str {
        match self {
            Self::Qwen3Asr17b => "Qwen/Qwen3-ASR-1.7B",
            Self::Qwen3Asr06b => "Qwen/Qwen3-ASR-0.6B",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct AppSettings {
    shortcut: String,
    recording_mode: RecordingMode,
    model: ModelOption,
    language: String,
    python_command: String,
    input_device: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            shortcut: "Ctrl+Shift+Space".to_string(),
            recording_mode: RecordingMode::Hold,
            model: ModelOption::Qwen3Asr17b,
            language: "auto".to_string(),
            python_command: "python".to_string(),
            input_device: DEFAULT_INPUT_DEVICE.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
enum DictationPhase {
    Idle,
    Bootstrapping,
    Listening,
    Transcribing,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DictationStatus {
    phase: DictationPhase,
    message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimePhase {
    Idle,
    Listening,
    Transcribing,
}

enum WorkerCommand {
    Start,
    Stop,
    Toggle,
}

struct RecorderSession {
    stream: Stream,
    writer: Arc<Mutex<Option<WavWriter<std::io::BufWriter<std::fs::File>>>>>,
    path: PathBuf,
}

impl RecorderSession {
    fn finalize(self) -> Result<PathBuf, String> {
        drop(self.stream);

        if let Some(writer) = self
            .writer
            .lock()
            .map_err(|_| "Failed to lock audio writer".to_string())?
            .take()
        {
            writer
                .finalize()
                .map_err(|err| format!("Failed to finalize WAV file: {err}"))?;
        }

        Ok(self.path)
    }
}

struct AppRuntime {
    settings: Mutex<AppSettings>,
    phase: Mutex<RuntimePhase>,
    ready: Mutex<bool>,
    bootstrap_lock: Mutex<()>,
    registered_shortcut: Mutex<String>,
    worker_tx: Sender<WorkerCommand>,
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("Failed to resolve app data dir: {err}"))?;

    fs::create_dir_all(&dir).map_err(|err| format!("Failed to create app data dir: {err}"))?;
    Ok(dir.join(SETTINGS_FILE))
}

fn load_settings(app: &AppHandle) -> AppSettings {
    let Ok(path) = settings_path(app) else {
        return AppSettings::default();
    };

    let Ok(raw) = fs::read_to_string(path) else {
        return AppSettings::default();
    };

    serde_json::from_str::<AppSettings>(&raw).unwrap_or_default()
}

fn save_settings(app: &AppHandle, settings: &AppSettings) -> Result<(), String> {
    let path = settings_path(app)?;
    let serialized = serde_json::to_string_pretty(settings)
        .map_err(|err| format!("Failed to serialize settings: {err}"))?;
    fs::write(path, serialized).map_err(|err| format!("Failed to persist settings: {err}"))
}

fn list_input_devices_internal() -> Result<Vec<String>, String> {
    let host = cpal::default_host();
    let mut devices = vec![DEFAULT_INPUT_DEVICE.to_string()];

    let found = host
        .input_devices()
        .map_err(|err| format!("Failed to list input devices: {err}"))?;

    for device in found {
        if let Ok(name) = device.name() {
            if !name.trim().is_empty() && !devices.contains(&name) {
                devices.push(name);
            }
        }
    }

    Ok(devices)
}

fn next_wav_path(app: &AppHandle) -> Result<PathBuf, String> {
    let mut cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|err| format!("Failed to resolve app cache dir: {err}"))?;

    fs::create_dir_all(&cache_dir)
        .map_err(|err| format!("Failed to create app cache dir: {err}"))?;

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| format!("System time error: {err}"))?
        .as_millis();

    cache_dir.push(format!("dictation-{ts}.wav"));
    Ok(cache_dir)
}

fn write_i16_samples(
    samples: &[i16],
    writer: &Arc<Mutex<Option<WavWriter<std::io::BufWriter<std::fs::File>>>>>,
) {
    let Ok(mut guard) = writer.lock() else {
        return;
    };

    let Some(writer) = guard.as_mut() else {
        return;
    };

    for &sample in samples {
        let _ = writer.write_sample(sample);
    }
}

fn write_u16_samples(
    samples: &[u16],
    writer: &Arc<Mutex<Option<WavWriter<std::io::BufWriter<std::fs::File>>>>>,
) {
    let Ok(mut guard) = writer.lock() else {
        return;
    };

    let Some(writer) = guard.as_mut() else {
        return;
    };

    for &sample in samples {
        let centered = (sample as i32 - 32_768) as i16;
        let _ = writer.write_sample(centered);
    }
}

fn write_f32_samples(
    samples: &[f32],
    writer: &Arc<Mutex<Option<WavWriter<std::io::BufWriter<std::fs::File>>>>>,
) {
    let Ok(mut guard) = writer.lock() else {
        return;
    };

    let Some(writer) = guard.as_mut() else {
        return;
    };

    for &sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let s = (clamped * i16::MAX as f32) as i16;
        let _ = writer.write_sample(s);
    }
}

fn resolve_input_device(settings: &AppSettings) -> Result<cpal::Device, String> {
    let host = cpal::default_host();

    if settings.input_device == DEFAULT_INPUT_DEVICE {
        return host
            .default_input_device()
            .ok_or_else(|| "No default microphone found".to_string());
    }

    let devices = host
        .input_devices()
        .map_err(|err| format!("Failed to list input devices: {err}"))?;

    for device in devices {
        if let Ok(name) = device.name() {
            if name == settings.input_device {
                return Ok(device);
            }
        }
    }

    host.default_input_device().ok_or_else(|| {
        format!(
            "Configured microphone '{}' not found and no default device available",
            settings.input_device
        )
    })
}

fn start_recorder(app: &AppHandle, settings: &AppSettings) -> Result<RecorderSession, String> {
    let input_device = resolve_input_device(settings)?;

    let supported = input_device
        .default_input_config()
        .map_err(|err| format!("Failed to read input config: {err}"))?;

    let wav_path = next_wav_path(app)?;
    let spec = WavSpec {
        channels: supported.channels(),
        sample_rate: supported.sample_rate().0,
        bits_per_sample: 16,
        sample_format: WavSampleFormat::Int,
    };

    let writer = WavWriter::create(&wav_path, spec)
        .map_err(|err| format!("Failed to create WAV writer: {err}"))?;
    let writer = Arc::new(Mutex::new(Some(writer)));

    let stream_config: StreamConfig = supported.clone().into();
    let err_fn = |err| {
        eprintln!("audio input stream error: {err}");
    };

    let stream = match supported.sample_format() {
        SampleFormat::I16 => {
            let writer = writer.clone();
            input_device
                .build_input_stream(
                    &stream_config,
                    move |data: &[i16], _| write_i16_samples(data, &writer),
                    err_fn,
                    None,
                )
                .map_err(|err| format!("Failed to build i16 input stream: {err}"))?
        }
        SampleFormat::U16 => {
            let writer = writer.clone();
            input_device
                .build_input_stream(
                    &stream_config,
                    move |data: &[u16], _| write_u16_samples(data, &writer),
                    err_fn,
                    None,
                )
                .map_err(|err| format!("Failed to build u16 input stream: {err}"))?
        }
        SampleFormat::F32 => {
            let writer = writer.clone();
            input_device
                .build_input_stream(
                    &stream_config,
                    move |data: &[f32], _| write_f32_samples(data, &writer),
                    err_fn,
                    None,
                )
                .map_err(|err| format!("Failed to build f32 input stream: {err}"))?
        }
        other => {
            return Err(format!("Unsupported sample format: {other:?}"));
        }
    };

    stream
        .play()
        .map_err(|err| format!("Failed to start audio capture: {err}"))?;

    Ok(RecorderSession {
        stream,
        writer,
        path: wav_path,
    })
}

fn resolve_transcriber_script(app: &AppHandle) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();

    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join("python").join("qwen_asr_transcribe.py"));
    }

    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("python")
            .join("qwen_asr_transcribe.py"),
    );

    if let Ok(current_dir) = std::env::current_dir() {
        candidates.push(
            current_dir
                .join("src-tauri")
                .join("python")
                .join("qwen_asr_transcribe.py"),
        );
    }

    candidates
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| "Could not locate qwen_asr_transcribe.py".to_string())
}

fn command_error(prefix: &str, stderr: &[u8]) -> String {
    let detail = String::from_utf8_lossy(stderr).trim().to_string();
    if detail.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}: {detail}")
    }
}

fn ensure_python_binary(settings: &AppSettings) -> Result<(), String> {
    let output = Command::new(&settings.python_command)
        .arg("--version")
        .output()
        .map_err(|err| {
            format!(
                "Python command '{}' failed to start: {err}",
                settings.python_command
            )
        })?;

    if output.status.success() {
        Ok(())
    } else {
        Err(command_error(
            &format!("Python command '{}' is not usable", settings.python_command),
            &output.stderr,
        ))
    }
}

fn ensure_python_dependencies(settings: &AppSettings) -> Result<(), String> {
    let check = Command::new(&settings.python_command)
        .args(["-c", "import qwen_asr, torch, torchvision"])
        .output()
        .map_err(|err| {
            format!(
                "Dependency check failed for '{}': {err}",
                settings.python_command
            )
        })?;

    if check.status.success() {
        return Ok(());
    }

    let install = Command::new(&settings.python_command)
        .args([
            "-m",
            "pip",
            "install",
            "-U",
            "qwen-asr",
            "torch",
            "torchvision",
        ])
        .output()
        .map_err(|err| format!("Failed launching pip installer: {err}"))?;

    if install.status.success() {
        Ok(())
    } else {
        Err(command_error(
            "Auto-install failed (pip install -U qwen-asr torch torchvision)",
            &install.stderr,
        ))
    }
}

fn warmup_selected_model(settings: &AppSettings, app: &AppHandle) -> Result<(), String> {
    let script_path = resolve_transcriber_script(app)?;

    let output = Command::new(&settings.python_command)
        .arg(script_path)
        .arg("--warmup")
        .arg("--model")
        .arg(settings.model.as_hf_id())
        .arg("--language")
        .arg(&settings.language)
        .output()
        .map_err(|err| format!("Failed launching model warmup: {err}"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(command_error("Model warmup failed", &output.stderr))
    }
}

fn bootstrap_asr_runtime(
    app: &AppHandle,
    state: &Arc<AppRuntime>,
    settings: AppSettings,
) -> Result<(), String> {
    let _bootstrap_guard = state
        .bootstrap_lock
        .lock()
        .map_err(|_| "Failed to lock bootstrap state".to_string())?;

    let _ = set_runtime_ready(state, false);
    emit_status(
        app,
        DictationPhase::Bootstrapping,
        Some("Checking Python runtime...".to_string()),
    );

    ensure_python_binary(&settings)?;

    emit_status(
        app,
        DictationPhase::Bootstrapping,
        Some("Ensuring ASR dependencies are installed...".to_string()),
    );
    ensure_python_dependencies(&settings)?;

    emit_status(
        app,
        DictationPhase::Bootstrapping,
        Some("Preparing selected model (first run may download)...".to_string()),
    );
    warmup_selected_model(&settings, app)?;

    let _ = set_runtime_ready(state, true);
    emit_status(app, DictationPhase::Idle, Some("Ready".to_string()));
    Ok(())
}

fn spawn_bootstrap_task(app: AppHandle, state: Arc<AppRuntime>, settings: AppSettings) {
    thread::spawn(move || {
        if let Err(err) = bootstrap_asr_runtime(&app, &state, settings) {
            let _ = set_runtime_ready(&state, false);
            emit_status(&app, DictationPhase::Error, Some(err));
        }
    });
}

fn transcribe_audio(
    settings: &AppSettings,
    app: &AppHandle,
    audio_path: &Path,
) -> Result<String, String> {
    let script_path = resolve_transcriber_script(app)?;

    let output = Command::new(&settings.python_command)
        .arg(script_path)
        .arg("--audio")
        .arg(audio_path)
        .arg("--model")
        .arg(settings.model.as_hf_id())
        .arg("--language")
        .arg(&settings.language)
        .output()
        .map_err(|err| {
            format!(
                "Failed to launch Python process '{}': {err}",
                settings.python_command
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ASR sidecar failed: {stderr}"));
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|err| format!("Invalid UTF-8 from sidecar: {err}"))?;
    let transcript = stdout.trim().to_string();

    if transcript.is_empty() {
        return Err("ASR returned empty transcript".to_string());
    }

    Ok(transcript)
}

fn inject_text_at_cursor(transcript: &str) -> Result<(), String> {
    if transcript.is_empty() {
        return Ok(());
    }

    let mut clipboard = Clipboard::new().map_err(|err| format!("Clipboard init failed: {err}"))?;
    let previous_clipboard = clipboard.get_text().ok();
    clipboard
        .set_text(transcript.to_string())
        .map_err(|err| format!("Failed to write transcript to clipboard: {err}"))?;

    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|err| format!("Input automation init failed: {err}"))?;

    enigo
        .key(Key::Control, Press)
        .and_then(|_| enigo.key(Key::Unicode('v'), Click))
        .and_then(|_| enigo.key(Key::Control, Release))
        .map_err(|err| format!("Failed to paste transcript: {err}"))?;

    thread::sleep(Duration::from_millis(140));

    if let Some(previous) = previous_clipboard {
        let _ = clipboard.set_text(previous);
    }

    Ok(())
}

fn show_settings_window(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window not found".to_string())?;
    window
        .show()
        .map_err(|err| format!("Failed to show main window: {err}"))?;
    window
        .set_focus()
        .map_err(|err| format!("Failed to focus main window: {err}"))
}

fn hide_settings_window(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window not found".to_string())?;
    window
        .hide()
        .map_err(|err| format!("Failed to hide main window: {err}"))
}

fn ensure_overlay_window(app: &AppHandle) -> Result<(), String> {
    if app.get_webview_window(OVERLAY_LABEL).is_some() {
        return Ok(());
    }

    let _window = WebviewWindowBuilder::new(
        app,
        OVERLAY_LABEL,
        WebviewUrl::App("index.html?overlay=1".into()),
    )
    .title("Dictation Overlay")
    .inner_size(280.0, 72.0)
    .resizable(false)
    .decorations(false)
    .always_on_top(true)
    .transparent(true)
    .focusable(false)
    .skip_taskbar(true)
    .visible(false)
    .build()
    .map_err(|err| format!("Failed to create overlay window: {err}"))?;

    Ok(())
}

fn place_overlay_bottom_center(app: &AppHandle) {
    let Some(window) = app.get_webview_window(OVERLAY_LABEL) else {
        return;
    };

    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten());

    let Some(monitor) = monitor else {
        return;
    };

    let work_area = monitor.work_area();
    let overlay_size = match window.inner_size() {
        Ok(size) => size,
        Err(_) => return,
    };

    let x = work_area.position.x + ((work_area.size.width as i32 - overlay_size.width as i32) / 2);
    let y = work_area.position.y + ((work_area.size.height as f32 * 0.90) as i32)
        - (overlay_size.height as i32 / 2);

    let _ = window.set_position(Position::Physical(PhysicalPosition::new(x, y)));
}

fn emit_status(app: &AppHandle, phase: DictationPhase, message: Option<String>) {
    let payload = DictationStatus {
        phase: phase.clone(),
        message,
    };

    let _ = app.emit(DICTATION_EVENT, payload.clone());

    if let Some(overlay) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = overlay.emit(DICTATION_EVENT, payload);

        match phase {
            DictationPhase::Idle => {
                let _ = overlay.hide();
            }
            _ => {
                place_overlay_bottom_center(app);
                let _ = overlay.show();
            }
        }
    }
}

fn set_phase(state: &Arc<AppRuntime>, phase: RuntimePhase) -> Result<(), String> {
    *state
        .phase
        .lock()
        .map_err(|_| "Failed to lock runtime phase".to_string())? = phase;
    Ok(())
}

fn current_phase(state: &Arc<AppRuntime>) -> Result<RuntimePhase, String> {
    state
        .phase
        .lock()
        .map(|phase| *phase)
        .map_err(|_| "Failed to lock runtime phase".to_string())
}

fn set_runtime_ready(state: &Arc<AppRuntime>, ready: bool) -> Result<(), String> {
    *state
        .ready
        .lock()
        .map_err(|_| "Failed to lock runtime readiness".to_string())? = ready;
    Ok(())
}

fn is_runtime_ready(state: &Arc<AppRuntime>) -> Result<bool, String> {
    state
        .ready
        .lock()
        .map(|ready| *ready)
        .map_err(|_| "Failed to lock runtime readiness".to_string())
}

fn worker_start(app: &AppHandle, state: &Arc<AppRuntime>, active: &mut Option<RecorderSession>) {
    if active.is_some() {
        return;
    }

    match current_phase(state) {
        Ok(RuntimePhase::Transcribing) => return,
        Ok(RuntimePhase::Listening) => return,
        Ok(RuntimePhase::Idle) => {}
        Err(err) => {
            emit_status(app, DictationPhase::Error, Some(err));
            return;
        }
    }

    match is_runtime_ready(state) {
        Ok(true) => {}
        Ok(false) => {
            emit_status(
                app,
                DictationPhase::Bootstrapping,
                Some("ASR setup still running. Please wait...".to_string()),
            );
            return;
        }
        Err(err) => {
            emit_status(app, DictationPhase::Error, Some(err));
            return;
        }
    }

    let settings = match state.settings.lock() {
        Ok(settings) => settings.clone(),
        Err(_) => {
            emit_status(
                app,
                DictationPhase::Error,
                Some("Failed to lock settings".to_string()),
            );
            return;
        }
    };

    match start_recorder(app, &settings) {
        Ok(session) => {
            *active = Some(session);
            let _ = set_phase(state, RuntimePhase::Listening);
            emit_status(
                app,
                DictationPhase::Listening,
                Some("Listening...".to_string()),
            );
        }
        Err(err) => {
            let _ = set_phase(state, RuntimePhase::Idle);
            emit_status(app, DictationPhase::Error, Some(err));
        }
    }
}

fn worker_stop(app: &AppHandle, state: &Arc<AppRuntime>, active: &mut Option<RecorderSession>) {
    if current_phase(state).ok() != Some(RuntimePhase::Listening) {
        return;
    }

    let Some(session) = active.take() else {
        return;
    };

    let audio_path = match session.finalize() {
        Ok(path) => path,
        Err(err) => {
            let _ = set_phase(state, RuntimePhase::Idle);
            emit_status(app, DictationPhase::Error, Some(err));
            return;
        }
    };

    let _ = set_phase(state, RuntimePhase::Transcribing);
    emit_status(
        app,
        DictationPhase::Transcribing,
        Some("Transcribing speech...".to_string()),
    );

    let settings = match state.settings.lock() {
        Ok(settings) => settings.clone(),
        Err(_) => {
            let _ = set_phase(state, RuntimePhase::Idle);
            emit_status(
                app,
                DictationPhase::Error,
                Some("Failed to lock settings".to_string()),
            );
            return;
        }
    };

    let transcript = transcribe_audio(&settings, app, &audio_path);

    match transcript {
        Ok(text) => {
            let _ = app.emit(TRANSCRIPT_EVENT, text.clone());

            if let Some(overlay) = app.get_webview_window(OVERLAY_LABEL) {
                let _ = overlay.hide();
            }

            if let Err(err) = inject_text_at_cursor(&text) {
                emit_status(app, DictationPhase::Error, Some(err));
            }
        }
        Err(err) => {
            emit_status(app, DictationPhase::Error, Some(err));
        }
    }

    let _ = fs::remove_file(&audio_path);
    let _ = set_phase(state, RuntimePhase::Idle);
    emit_status(app, DictationPhase::Idle, None);
}

fn run_worker_loop(app: AppHandle, state: Arc<AppRuntime>, rx: Receiver<WorkerCommand>) {
    let mut active_session: Option<RecorderSession> = None;

    while let Ok(command) = rx.recv() {
        match command {
            WorkerCommand::Start => worker_start(&app, &state, &mut active_session),
            WorkerCommand::Stop => worker_stop(&app, &state, &mut active_session),
            WorkerCommand::Toggle => {
                if current_phase(&state).ok() == Some(RuntimePhase::Listening) {
                    worker_stop(&app, &state, &mut active_session);
                } else {
                    worker_start(&app, &state, &mut active_session);
                }
            }
        }
    }
}

fn queue_command(state: &Arc<AppRuntime>, command: WorkerCommand) -> Result<(), String> {
    if current_phase(state).ok() == Some(RuntimePhase::Transcribing) {
        match command {
            WorkerCommand::Start | WorkerCommand::Stop | WorkerCommand::Toggle => {
                return Ok(());
            }
        }
    }

    state
        .worker_tx
        .send(command)
        .map_err(|err| format!("Failed to send worker command: {err}"))
}

fn start_dictation_internal(state: &Arc<AppRuntime>) -> Result<(), String> {
    queue_command(state, WorkerCommand::Start)
}

fn stop_dictation_internal(state: &Arc<AppRuntime>) -> Result<(), String> {
    queue_command(state, WorkerCommand::Stop)
}

fn toggle_dictation_internal(state: &Arc<AppRuntime>) -> Result<(), String> {
    queue_command(state, WorkerCommand::Toggle)
}

fn normalize_shortcut_key_token(token: &str) -> Result<String, String> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return Err("Shortcut key cannot be empty".to_string());
    }

    if trimmed.eq_ignore_ascii_case("space") || trimmed == " " {
        return Ok("Space".to_string());
    }

    if trimmed.eq_ignore_ascii_case("esc") || trimmed.eq_ignore_ascii_case("escape") {
        return Ok("Escape".to_string());
    }

    if trimmed.eq_ignore_ascii_case("enter") {
        return Ok("Enter".to_string());
    }

    if trimmed.eq_ignore_ascii_case("tab") {
        return Ok("Tab".to_string());
    }

    if trimmed.eq_ignore_ascii_case("backspace") {
        return Ok("Backspace".to_string());
    }

    if trimmed.eq_ignore_ascii_case("delete") {
        return Ok("Delete".to_string());
    }

    if trimmed.eq_ignore_ascii_case("up") || trimmed.eq_ignore_ascii_case("arrowup") {
        return Ok("ArrowUp".to_string());
    }

    if trimmed.eq_ignore_ascii_case("down") || trimmed.eq_ignore_ascii_case("arrowdown") {
        return Ok("ArrowDown".to_string());
    }

    if trimmed.eq_ignore_ascii_case("left") || trimmed.eq_ignore_ascii_case("arrowleft") {
        return Ok("ArrowLeft".to_string());
    }

    if trimmed.eq_ignore_ascii_case("right") || trimmed.eq_ignore_ascii_case("arrowright") {
        return Ok("ArrowRight".to_string());
    }

    if trimmed.len() == 1 {
        let ch = trimmed.chars().next().unwrap_or_default();
        if ch.is_ascii_alphabetic() {
            return Ok(ch.to_ascii_uppercase().to_string());
        }

        if ch.is_ascii_digit() {
            return Ok(ch.to_string());
        }
    }

    let upper = trimmed.to_ascii_uppercase();
    if upper.starts_with('F')
        && upper.len() <= 3
        && upper
            .chars()
            .skip(1)
            .all(|character| character.is_ascii_digit())
    {
        return Ok(upper);
    }

    Ok(trimmed.to_string())
}

fn normalize_shortcut_text(shortcut_text: &str) -> Result<String, String> {
    let parsed_direct: Result<Shortcut, _> = shortcut_text.trim().parse();
    if let Ok(shortcut) = parsed_direct {
        return Ok(shortcut.into_string());
    }

    let mut tokens: Vec<String> = shortcut_text
        .split('+')
        .map(|token| token.trim())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_string())
        .collect();

    if tokens.is_empty() {
        return Err("Shortcut cannot be empty".to_string());
    }

    let key_token = tokens
        .pop()
        .ok_or_else(|| "Shortcut key cannot be empty".to_string())?;

    let mut modifiers = Vec::new();
    for token in tokens {
        let normalized_modifier = match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => "Ctrl",
            "shift" => "Shift",
            "alt" | "option" => "Alt",
            "meta" | "super" | "cmd" | "command" | "win" | "windows" => "Super",
            _ => {
                return Err(format!(
                    "Unsupported modifier '{token}'. Use Ctrl, Shift, Alt, or Super."
                ));
            }
        };

        if !modifiers
            .iter()
            .any(|existing: &String| existing == normalized_modifier)
        {
            modifiers.push(normalized_modifier.to_string());
        }
    }

    let key = normalize_shortcut_key_token(&key_token)?;
    let normalized = if modifiers.is_empty() {
        key
    } else {
        format!("{}+{key}", modifiers.join("+"))
    };

    normalized
        .parse::<Shortcut>()
        .map(|shortcut| shortcut.into_string())
        .map_err(|error| {
            format!(
                "Invalid shortcut '{shortcut_text}'. Try keys like F8, Space, Ctrl+Shift+Space: {error}"
            )
        })
}

fn register_shortcut(
    app: &AppHandle,
    state: &Arc<AppRuntime>,
    shortcut_text: &str,
) -> Result<String, String> {
    let normalized_shortcut = normalize_shortcut_text(shortcut_text)?;

    let shortcut: Shortcut = normalized_shortcut
        .parse()
        .map_err(|err| format!("Invalid shortcut '{normalized_shortcut}': {err}"))?;

    app.global_shortcut()
        .unregister_all()
        .map_err(|err| format!("Failed to clear previous shortcuts: {err}"))?;

    let state_for_handler = state.clone();
    app.global_shortcut()
        .on_shortcut(shortcut, move |_app_handle, _shortcut, event| {
            let settings = match state_for_handler.settings.lock() {
                Ok(settings) => settings.clone(),
                Err(_) => return,
            };

            match settings.recording_mode {
                RecordingMode::Hold => {
                    if event.state == ShortcutState::Pressed {
                        let _ = start_dictation_internal(&state_for_handler);
                    }

                    if event.state == ShortcutState::Released {
                        let _ = stop_dictation_internal(&state_for_handler);
                    }
                }
                RecordingMode::Toggle => {
                    if event.state == ShortcutState::Pressed {
                        let _ = toggle_dictation_internal(&state_for_handler);
                    }
                }
            }
        })
        .map_err(|err| format!("Failed to register shortcut handler: {err}"))?;

    *state
        .registered_shortcut
        .lock()
        .map_err(|_| "Failed to lock shortcut state".to_string())? = normalized_shortcut.clone();

    Ok(normalized_shortcut)
}

fn install_tray(app: &AppHandle, state: Arc<AppRuntime>) -> Result<(), String> {
    let open_item = MenuItem::with_id(app, "open", "Open Settings", true, None::<&str>)
        .map_err(|err| err.to_string())?;
    let toggle_item =
        MenuItem::with_id(app, "toggle", "Start / Stop Dictation", true, None::<&str>)
            .map_err(|err| err.to_string())?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)
        .map_err(|err| err.to_string())?;

    let menu = Menu::with_items(app, &[&open_item, &toggle_item, &quit_item])
        .map_err(|err| err.to_string())?;

    let state_for_menu = state.clone();
    let mut tray_builder = TrayIconBuilder::with_id("dictation-tray");

    if let Some(icon) = app.default_window_icon() {
        tray_builder = tray_builder.icon(icon.clone());
    }

    tray_builder
        .tooltip("Delulu Talks")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app_handle, event| match event.id().as_ref() {
            "open" => {
                let _ = show_settings_window(app_handle);
            }
            "toggle" => {
                let _ = toggle_dictation_internal(&state_for_menu);
            }
            "quit" => {
                app_handle.exit(0);
            }
            _ => {}
        })
        .build(app)
        .map_err(|err| format!("Failed to create tray icon: {err}"))?;

    Ok(())
}

#[tauri::command]
fn get_settings(state: State<'_, Arc<AppRuntime>>) -> Result<AppSettings, String> {
    state
        .settings
        .lock()
        .map(|settings| settings.clone())
        .map_err(|_| "Failed to lock settings".to_string())
}

#[tauri::command]
fn list_input_devices() -> Result<Vec<String>, String> {
    list_input_devices_internal()
}

#[tauri::command]
fn normalize_shortcut(shortcut: String) -> Result<String, String> {
    normalize_shortcut_text(&shortcut)
}

#[tauri::command]
fn update_settings(
    app: AppHandle,
    state: State<'_, Arc<AppRuntime>>,
    mut settings: AppSettings,
) -> Result<AppSettings, String> {
    let normalized_shortcut = register_shortcut(&app, state.inner(), &settings.shortcut)?;
    settings.shortcut = normalized_shortcut;
    save_settings(&app, &settings)?;

    let mut current = state
        .settings
        .lock()
        .map_err(|_| "Failed to lock settings".to_string())?;

    let should_rebootstrap = current.python_command != settings.python_command
        || current.model != settings.model
        || current.language != settings.language;

    *current = settings.clone();
    drop(current);

    if should_rebootstrap {
        let _ = set_runtime_ready(state.inner(), false);
        spawn_bootstrap_task(app.clone(), state.inner().clone(), settings.clone());
    }

    Ok(settings)
}

#[tauri::command]
fn start_dictation(state: State<'_, Arc<AppRuntime>>) -> Result<(), String> {
    start_dictation_internal(state.inner())
}

#[tauri::command]
fn stop_dictation(state: State<'_, Arc<AppRuntime>>) -> Result<(), String> {
    stop_dictation_internal(state.inner())
}

#[tauri::command]
fn toggle_dictation(state: State<'_, Arc<AppRuntime>>) -> Result<(), String> {
    toggle_dictation_internal(state.inner())
}

#[tauri::command]
fn open_settings_window(app: AppHandle) -> Result<(), String> {
    show_settings_window(&app)
}

#[tauri::command]
fn hide_settings(app: AppHandle) -> Result<(), String> {
    hide_settings_window(&app)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let initial_settings = load_settings(app.handle());
            let (worker_tx, worker_rx) = mpsc::channel::<WorkerCommand>();

            let runtime = Arc::new(AppRuntime {
                settings: Mutex::new(initial_settings.clone()),
                phase: Mutex::new(RuntimePhase::Idle),
                ready: Mutex::new(false),
                bootstrap_lock: Mutex::new(()),
                registered_shortcut: Mutex::new(initial_settings.shortcut.clone()),
                worker_tx,
            });

            app.manage(runtime.clone());
            let normalized_shortcut =
                register_shortcut(app.handle(), &runtime, &initial_settings.shortcut)?;

            if normalized_shortcut != initial_settings.shortcut {
                let mut loaded_settings = initial_settings.clone();
                loaded_settings.shortcut = normalized_shortcut;
                save_settings(app.handle(), &loaded_settings)?;
                *runtime
                    .settings
                    .lock()
                    .map_err(|_| "Failed to lock settings".to_string())? = loaded_settings.clone();
            }

            let app_handle_for_worker = app.handle().clone();
            let runtime_for_worker = runtime.clone();
            std::thread::spawn(move || {
                run_worker_loop(app_handle_for_worker, runtime_for_worker, worker_rx)
            });

            ensure_overlay_window(app.handle())?;
            install_tray(app.handle(), runtime.clone())?;

            if let Some(main_window) = app.get_webview_window("main") {
                let window_handle = main_window.clone();
                main_window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = window_handle.hide();
                    }
                });
            }

            let bootstrap_settings = runtime
                .settings
                .lock()
                .map_err(|_| "Failed to lock settings".to_string())?
                .clone();
            spawn_bootstrap_task(app.handle().clone(), runtime.clone(), bootstrap_settings);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            list_input_devices,
            normalize_shortcut,
            update_settings,
            start_dictation,
            stop_dictation,
            toggle_dictation,
            open_settings_window,
            hide_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
