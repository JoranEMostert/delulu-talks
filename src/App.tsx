import { useEffect, useMemo, useState, type ReactElement } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type RecordingMode = "hold" | "toggle";
type ModelOption = "qwen3Asr17b" | "qwen3Asr06b";
type DictationPhase =
  | "idle"
  | "bootstrapping"
  | "listening"
  | "transcribing"
  | "error";

type SettingsTab = "general" | "speech" | "audio" | "runtime";

type AppSettings = {
  shortcut: string;
  recordingMode: RecordingMode;
  model: ModelOption;
  language: string;
  pythonCommand: string;
  inputDevice: string;
};

type DictationStatus = {
  phase: DictationPhase;
  message?: string | null;
};

type LanguageOption = {
  code: string;
  label: string;
};

type IconProps = {
  className?: string;
};

function IconGeneral({ className = "h-4 w-4" }: IconProps) {
  return (
    <svg viewBox="0 0 24 24" fill="none" className={className} aria-hidden="true">
      <path d="M4 7h16M4 12h16M4 17h10" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
    </svg>
  );
}

function IconSpeech({ className = "h-4 w-4" }: IconProps) {
  return (
    <svg viewBox="0 0 24 24" fill="none" className={className} aria-hidden="true">
      <path d="M8 8a4 4 0 1 1 8 0v4a4 4 0 1 1-8 0V8Z" stroke="currentColor" strokeWidth="1.8" />
      <path d="M5 11.5a7 7 0 0 0 14 0M12 18v3M9 21h6" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
    </svg>
  );
}

function IconAudio({ className = "h-4 w-4" }: IconProps) {
  return (
    <svg viewBox="0 0 24 24" fill="none" className={className} aria-hidden="true">
      <path d="M4 10h4l5-4v12l-5-4H4v-4Z" stroke="currentColor" strokeWidth="1.8" strokeLinejoin="round" />
      <path d="M17 9.5a4 4 0 0 1 0 5M19.5 7a7.5 7.5 0 0 1 0 10" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
    </svg>
  );
}

function IconRuntime({ className = "h-4 w-4" }: IconProps) {
  return (
    <svg viewBox="0 0 24 24" fill="none" className={className} aria-hidden="true">
      <rect x="5" y="5" width="14" height="14" rx="2.5" stroke="currentColor" strokeWidth="1.8" />
      <path d="M9 9h6v6H9z" stroke="currentColor" strokeWidth="1.8" />
      <path d="M3 9h2M3 15h2M19 9h2M19 15h2M9 3v2M15 3v2M9 19v2M15 19v2" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
    </svg>
  );
}

function IconSave({ className = "h-4 w-4" }: IconProps) {
  return (
    <svg viewBox="0 0 24 24" fill="none" className={className} aria-hidden="true">
      <path d="M5 4h11l3 3v13H5V4Z" stroke="currentColor" strokeWidth="1.8" strokeLinejoin="round" />
      <path d="M8 4v6h8V4M8 20v-5h8v5" stroke="currentColor" strokeWidth="1.8" />
    </svg>
  );
}

function IconPlay({ className = "h-4 w-4" }: IconProps) {
  return (
    <svg viewBox="0 0 24 24" fill="none" className={className} aria-hidden="true">
      <path d="M8 6.5v11l9-5.5-9-5.5Z" fill="currentColor" />
    </svg>
  );
}

function IconStop({ className = "h-4 w-4" }: IconProps) {
  return (
    <svg viewBox="0 0 24 24" fill="none" className={className} aria-hidden="true">
      <rect x="7" y="7" width="10" height="10" rx="1.5" fill="currentColor" />
    </svg>
  );
}

const defaultSettings: AppSettings = {
  shortcut: "Ctrl+Shift+Space",
  recordingMode: "hold",
  model: "qwen3Asr06b",
  language: "auto",
  pythonCommand: "python",
  inputDevice: "default",
};

const modelDescriptions: Record<ModelOption, string> = {
  qwen3Asr17b:
    "Higher quality, but can run out of VRAM on smaller Linux GPUs.",
  qwen3Asr06b:
    "Recommended on this machine: lighter, faster, and stable on lower VRAM.",
};

const tabs: Array<{ id: SettingsTab; label: string; hint: string; icon: (props: IconProps) => ReactElement }> = [
  { id: "general", label: "General", hint: "Shortcut and recording", icon: IconGeneral },
  { id: "speech", label: "Speech", hint: "Model and language", icon: IconSpeech },
  { id: "audio", label: "Audio", hint: "Input device", icon: IconAudio },
  { id: "runtime", label: "Runtime", hint: "Python and setup", icon: IconRuntime },
];

const supportedLanguages: LanguageOption[] = [
  { code: "auto", label: "Automatic detection" },
  { code: "zh", label: "Chinese" },
  { code: "en", label: "English" },
  { code: "yue", label: "Cantonese" },
  { code: "ar", label: "Arabic" },
  { code: "de", label: "German" },
  { code: "fr", label: "French" },
  { code: "es", label: "Spanish" },
  { code: "pt", label: "Portuguese" },
  { code: "id", label: "Indonesian" },
  { code: "it", label: "Italian" },
  { code: "ko", label: "Korean" },
  { code: "ru", label: "Russian" },
  { code: "th", label: "Thai" },
  { code: "vi", label: "Vietnamese" },
  { code: "ja", label: "Japanese" },
  { code: "tr", label: "Turkish" },
  { code: "hi", label: "Hindi" },
  { code: "ms", label: "Malay" },
  { code: "nl", label: "Dutch" },
  { code: "sv", label: "Swedish" },
  { code: "da", label: "Danish" },
  { code: "fi", label: "Finnish" },
  { code: "pl", label: "Polish" },
  { code: "cs", label: "Czech" },
  { code: "fil", label: "Filipino" },
  { code: "fa", label: "Persian" },
  { code: "el", label: "Greek" },
  { code: "hu", label: "Hungarian" },
  { code: "mk", label: "Macedonian" },
  { code: "ro", label: "Romanian" },
];

function formatLanguageLabel(code: string): string {
  const found = supportedLanguages.find((language) => language.code === code);
  if (!found) {
    return code;
  }

  return `${found.label} (${found.code})`;
}

function resolveLanguageInput(input: string): LanguageOption | undefined {
  const query = input.trim().toLowerCase();
  if (!query) {
    return undefined;
  }

  return supportedLanguages.find((language) => {
    const formatted = `${language.label} (${language.code})`.toLowerCase();
    return (
      language.code.toLowerCase() === query ||
      language.label.toLowerCase() === query ||
      formatted === query
    );
  });
}

function shortcutKeyToken(event: React.KeyboardEvent<HTMLInputElement>): string | null {
  const key = event.key;

  if (["Control", "Shift", "Alt", "Meta"].includes(key)) {
    return null;
  }

  if (key === " " || key === "Spacebar") {
    return "Space";
  }

  if (key.length === 1) {
    if (/^[a-z]$/i.test(key)) {
      return key.toUpperCase();
    }

    if (/^[0-9]$/.test(key)) {
      return key;
    }
  }

  if (/^F\d{1,2}$/i.test(key)) {
    return key.toUpperCase();
  }

  const mapped: Record<string, string> = {
    Escape: "Escape",
    Esc: "Escape",
    Enter: "Enter",
    Tab: "Tab",
    Backspace: "Backspace",
    Delete: "Delete",
    Insert: "Insert",
    Home: "Home",
    End: "End",
    PageUp: "PageUp",
    PageDown: "PageDown",
    ArrowUp: "ArrowUp",
    ArrowDown: "ArrowDown",
    ArrowLeft: "ArrowLeft",
    ArrowRight: "ArrowRight",
  };

  return mapped[key] ?? null;
}

function buildShortcutCandidate(
  event: React.KeyboardEvent<HTMLInputElement>,
): string | null {
  const keyToken = shortcutKeyToken(event);
  if (!keyToken) {
    return null;
  }

  const parts: string[] = [];
  if (event.ctrlKey) {
    parts.push("Ctrl");
  }
  if (event.shiftKey) {
    parts.push("Shift");
  }
  if (event.altKey) {
    parts.push("Alt");
  }
  if (event.metaKey) {
    parts.push("Super");
  }

  parts.push(keyToken);
  return parts.join("+");
}

function OverlayPill() {
  const [status, setStatus] = useState<DictationStatus>({ phase: "idle" });

  useEffect(() => {
    void invoke<DictationStatus>("get_runtime_status")
      .then((runtimeStatus) => {
        setStatus(runtimeStatus);
      })
      .catch(() => {
        // Ignore initial sync failures and fall back to event updates.
      });

    let mounted = true;
    let unlistenPromise: Promise<() => void> | undefined;

    unlistenPromise = listen<DictationStatus>("dictation-state", (event) => {
      if (mounted) {
        setStatus(event.payload);
      }
    });

    return () => {
      mounted = false;
      void unlistenPromise?.then((unlisten) => unlisten());
    };
  }, []);

  const statusColor =
    status.phase === "bootstrapping"
      ? "text-cyan-400"
      : status.phase === "listening"
        ? "text-emerald-400"
        : status.phase === "transcribing"
          ? "text-amber-400"
          : status.phase === "error"
            ? "text-rose-400"
            : "text-slate-400";

  const dotColor =
    status.phase === "bootstrapping"
      ? "bg-cyan-400"
      : status.phase === "listening"
        ? "bg-emerald-400"
        : status.phase === "transcribing"
          ? "bg-amber-400"
          : status.phase === "error"
            ? "bg-rose-400"
            : "bg-slate-400";

  const label =
    status.phase === "bootstrapping"
      ? "Preparing"
      : status.phase === "listening"
        ? "Listening"
        : status.phase === "transcribing"
          ? "Transcribing"
          : status.phase === "error"
            ? "Error"
            : "Ready";

  return (
    <main className="h-screen w-screen bg-transparent">
      <div className="flex h-full w-full items-center justify-center p-2">
        <div className="overlay-pill flex items-center gap-3 rounded-full px-5 py-3 shadow-2xl">
          <span className={`status-dot h-3 w-3 rounded-full ${dotColor}`} />
          <span className={`text-sm font-semibold tracking-wide ${statusColor}`}>
            {label}
          </span>
          {(status.phase === "listening" || status.phase === "transcribing") && (
            <div className="ml-1 flex items-end gap-1">
              <span className="scribble-wave h-2 w-1 rounded bg-cyan-400" />
              <span className="scribble-wave h-3 w-1 rounded bg-cyan-400" />
              <span className="scribble-wave h-2 w-1 rounded bg-cyan-400" />
            </div>
          )}
        </div>
      </div>
    </main>
  );
}

function SettingsPage() {
  const [settings, setSettings] = useState<AppSettings>(defaultSettings);
  const [inputDevices, setInputDevices] = useState<string[]>(["default"]);
  const [status, setStatus] = useState<DictationStatus>({
    phase: "idle",
    message: "Ready",
  });
  const [saving, setSaving] = useState(false);
  const [activeTab, setActiveTab] = useState<SettingsTab>("general");
  const [languageQuery, setLanguageQuery] = useState(formatLanguageLabel("auto"));
  const [languageMenuOpen, setLanguageMenuOpen] = useState(false);
  const [capturingShortcut, setCapturingShortcut] = useState(false);

  useEffect(() => {
    void (async () => {
      const [loaded, runtimeStatus, devices] = await Promise.all([
        invoke<AppSettings>("get_settings"),
        invoke<DictationStatus>("get_runtime_status"),
        invoke<string[]>("list_input_devices"),
      ]);
      setSettings(loaded);
      setStatus(runtimeStatus);
      setLanguageQuery(formatLanguageLabel(loaded.language));

      const normalized = devices.length > 0 ? devices : ["default"];
      if (loaded.inputDevice && !normalized.includes(loaded.inputDevice)) {
        normalized.push(loaded.inputDevice);
      }
      setInputDevices(normalized);
    })();

    let mounted = true;
    let unlistenPromise: Promise<() => void> | undefined;

    unlistenPromise = listen<DictationStatus>("dictation-state", (event) => {
      if (mounted) {
        setStatus(event.payload);
      }
    });

    return () => {
      mounted = false;
      void unlistenPromise?.then((unlisten) => unlisten());
    };
  }, []);

  const filteredLanguages = useMemo(() => {
    const query = languageQuery.trim().toLowerCase();
    if (!query) {
      return supportedLanguages;
    }

    return supportedLanguages.filter((language) => {
      const formatted = `${language.label} (${language.code})`.toLowerCase();
      return (
        language.code.toLowerCase().includes(query) ||
        language.label.toLowerCase().includes(query) ||
        formatted.includes(query)
      );
    });
  }, [languageQuery]);

  const statusColor = useMemo(() => {
    if (status.phase === "bootstrapping") {
      return "text-cyan-400 border-cyan-400/30 bg-cyan-400/10";
    }
    if (status.phase === "listening") {
      return "text-emerald-400 border-emerald-400/30 bg-emerald-400/10";
    }
    if (status.phase === "transcribing") {
      return "text-amber-400 border-amber-400/30 bg-amber-400/10";
    }
    if (status.phase === "error") {
      return "text-rose-400 border-rose-400/30 bg-rose-400/10";
    }
    return "text-slate-400 border-slate-400/30 bg-slate-400/10";
  }, [status.phase]);

  function chooseLanguage(code: string) {
    setSettings((previous) => ({ ...previous, language: code }));
    setLanguageQuery(formatLanguageLabel(code));
    setLanguageMenuOpen(false);
  }

  async function captureShortcut(
    event: React.KeyboardEvent<HTMLInputElement>,
  ): Promise<void> {
    event.preventDefault();
    event.stopPropagation();

    const candidate = buildShortcutCandidate(event);
    if (!candidate) {
      return;
    }

    try {
      const normalized = await invoke<string>("normalize_shortcut", {
        shortcut: candidate,
      });
      setSettings((previous) => ({ ...previous, shortcut: normalized }));
      setStatus({ phase: "idle", message: `Shortcut set to ${normalized}` });
    } catch (error) {
      setStatus({
        phase: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    }
  }

  async function persistSettings() {
    setSaving(true);
    try {
      const updated = await invoke<AppSettings>("update_settings", { settings });
      setSettings(updated);
      setLanguageQuery(formatLanguageLabel(updated.language));
      setStatus({ phase: "idle", message: "Settings saved" });
    } catch (error) {
      setStatus({
        phase: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setSaving(false);
    }
  }

  async function toggleDictation() {
    try {
      await invoke("toggle_dictation");
    } catch (error) {
      setStatus({
        phase: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    }
  }

  return (
    <main className="scribble-bg h-screen w-screen overflow-hidden text-[#e0f4ff]">
      <div className="pointer-events-none absolute inset-0 overflow-hidden">
        <svg
          className="floating-scribble absolute left-10 top-10 h-32 w-32 opacity-10"
          viewBox="0 0 100 100"
        >
          <path
            d="M10,50 Q25,20 50,50 T90,50"
            stroke="#00E5FF"
            strokeWidth="2"
            fill="none"
          />
        </svg>
        <svg
          className="floating-scribble absolute bottom-20 right-20 h-40 w-40 opacity-10"
          style={{ animationDelay: "2s" }}
          viewBox="0 0 100 100"
        >
          <path
            d="M20,80 Q50,20 80,80"
            stroke="#2962FF"
            strokeWidth="2"
            fill="none"
          />
        </svg>
      </div>

      <div className="relative z-10 flex h-full flex-col">
        <header className="flex items-center gap-4 border-b border-[#00E5FF]/20 bg-[#060d18]/70 px-6 py-4">
          <img src="/delulu-talks-icon.svg" alt="Delulu Talks" className="h-12 w-12" />
          <div className="min-w-0">
            <h1 className="bg-gradient-to-r from-[#00E5FF] to-[#2962FF] bg-clip-text text-xl font-bold text-transparent">
              Delulu Talks
            </h1>
            <p className="text-xs text-slate-400">
              Chaotic speech-to-text assistant running in tray
            </p>
          </div>
          <div className="ml-auto rounded-full border border-[#00E5FF]/30 bg-[#00E5FF]/10 px-3 py-1 text-xs text-[#7befff]">
            {tabs.find((tab) => tab.id === activeTab)?.label}
          </div>
        </header>

        <div className="flex min-h-0 flex-1">
          <aside className="w-60 border-r border-[#00E5FF]/20 bg-[#060d18]/45 p-4">
            <nav className="space-y-2">
              {tabs.map((tab) => {
                const isActive = activeTab === tab.id;
                const TabIcon = tab.icon;
                return (
                  <button
                    key={tab.id}
                    type="button"
                    onClick={() => setActiveTab(tab.id)}
                    className={`w-full rounded-xl px-4 py-3 text-left transition ${
                      isActive
                        ? "scribble-border-active text-[#00E5FF]"
                        : "border border-[#00E5FF]/15 text-slate-400 hover:border-[#00E5FF]/30 hover:text-slate-200"
                    }`}
                  >
                    <div className="flex items-center gap-2">
                      <TabIcon className="h-4 w-4" />
                      <p className="text-sm font-semibold">{tab.label}</p>
                    </div>
                    <p className="mt-1 text-xs opacity-80">{tab.hint}</p>
                  </button>
                );
              })}
            </nav>

            <div className={`mt-4 rounded-xl border px-3 py-2 text-xs ${statusColor}`}>
              <p className="uppercase tracking-wide">{status.phase}</p>
              <p className="mt-1 text-slate-300">{status.message ?? "Ready"}</p>
            </div>
          </aside>

          <section className="flex min-h-0 flex-1 flex-col">
            <div className="flex-1 overflow-y-auto p-6">
              <div className="mx-auto max-w-2xl space-y-6">
                {activeTab === "general" && (
                  <div className="scribble-border scribble-glow rounded-2xl p-6">
                    <h2 className="mb-5 flex items-center gap-2 text-lg font-semibold text-[#00E5FF]"><IconGeneral className="h-5 w-5" />General</h2>

                    <div className="space-y-5">
                      <div className="grid gap-2">
                        <label className="text-sm font-medium text-slate-300">
                          Shortcut Recorder
                        </label>
                        <input
                          className="scribble-input h-11 rounded-xl px-4 text-sm"
                          value={settings.shortcut}
                          readOnly
                          onFocus={() => setCapturingShortcut(true)}
                          onBlur={() => setCapturingShortcut(false)}
                          onKeyDown={(event) => {
                            void captureShortcut(event);
                          }}
                        />
                        <p className="text-xs text-slate-500">
                          {capturingShortcut
                            ? "Press any key or combo now. Single key works too."
                            : "Click the field, then press your shortcut."}
                        </p>
                        <p className="text-xs text-slate-500">
                          Hold mode supports one-key push-to-talk: press starts, release stops.
                        </p>
                      </div>

                      <div className="grid gap-2">
                        <label className="text-sm font-medium text-slate-300">
                          Recording Mode
                        </label>
                        <div className="grid gap-2 sm:grid-cols-2">
                          <button
                            type="button"
                            onClick={() =>
                              setSettings((previous) => ({
                                ...previous,
                                recordingMode: "hold",
                              }))
                            }
                            className={`rounded-xl border px-4 py-3 text-left text-sm transition ${
                              settings.recordingMode === "hold"
                                ? "scribble-border-active bg-[#00E5FF]/10 text-[#00E5FF]"
                                : "border-[#00E5FF]/20 text-slate-400 hover:border-[#00E5FF]/40"
                            }`}
                          >
                            Hold-to-talk (default)
                          </button>
                          <button
                            type="button"
                            onClick={() =>
                              setSettings((previous) => ({
                                ...previous,
                                recordingMode: "toggle",
                              }))
                            }
                            className={`rounded-xl border px-4 py-3 text-left text-sm transition ${
                              settings.recordingMode === "toggle"
                                ? "scribble-border-active bg-[#00E5FF]/10 text-[#00E5FF]"
                                : "border-[#00E5FF]/20 text-slate-400 hover:border-[#00E5FF]/40"
                            }`}
                          >
                            Toggle-to-record
                          </button>
                        </div>
                      </div>
                    </div>
                  </div>
                )}

                {activeTab === "speech" && (
                  <div className="scribble-border scribble-glow rounded-2xl p-6">
                    <h2 className="mb-5 flex items-center gap-2 text-lg font-semibold text-[#00E5FF]"><IconSpeech className="h-5 w-5" />Speech</h2>

                    <div className="space-y-5">
                      <div className="grid gap-2">
                        <label className="text-sm font-medium text-slate-300">ASR Model</label>
                        <select
                          className="scribble-input h-11 rounded-xl px-4 text-sm"
                          value={settings.model}
                          onChange={(event) =>
                            setSettings((previous) => ({
                              ...previous,
                              model: event.target.value as ModelOption,
                            }))
                          }
                        >
                          <option value="qwen3Asr17b">Qwen3-ASR-1.7B (High VRAM)</option>
                          <option value="qwen3Asr06b">Qwen3-ASR-0.6B (Recommended)</option>
                        </select>
                        <p className="text-xs text-slate-500">
                          {modelDescriptions[settings.model]}
                        </p>
                      </div>

                      <div className="grid gap-2">
                        <label className="text-sm font-medium text-slate-300">
                          Language (searchable)
                        </label>
                        <div className="relative">
                          <input
                            className="scribble-input h-11 w-full rounded-xl px-4 text-sm"
                            value={languageQuery}
                            onFocus={() => setLanguageMenuOpen(true)}
                            onChange={(event) => {
                              setLanguageQuery(event.target.value);
                              setLanguageMenuOpen(true);
                            }}
                            onBlur={() => {
                              window.setTimeout(() => {
                                const resolved = resolveLanguageInput(languageQuery);
                                if (resolved) {
                                  chooseLanguage(resolved.code);
                                } else {
                                  setLanguageQuery(formatLanguageLabel(settings.language));
                                }
                                setLanguageMenuOpen(false);
                              }, 120);
                            }}
                            onKeyDown={(event) => {
                              if (event.key === "Enter") {
                                event.preventDefault();
                                const resolved = resolveLanguageInput(languageQuery);
                                if (resolved) {
                                  chooseLanguage(resolved.code);
                                } else if (filteredLanguages[0]) {
                                  chooseLanguage(filteredLanguages[0].code);
                                }
                              }

                              if (event.key === "Escape") {
                                setLanguageQuery(formatLanguageLabel(settings.language));
                                setLanguageMenuOpen(false);
                              }
                            }}
                            placeholder="Search language name or code"
                          />

                          {languageMenuOpen && (
                            <div className="scribble-border absolute z-20 mt-1 max-h-56 w-full overflow-y-auto rounded-xl border bg-[#081224] p-1">
                              {filteredLanguages.slice(0, 20).map((language) => (
                                <button
                                  key={language.code}
                                  type="button"
                                  onMouseDown={(event) => {
                                    event.preventDefault();
                                    chooseLanguage(language.code);
                                  }}
                                  className={`block w-full rounded-lg px-3 py-2 text-left text-sm transition ${
                                    settings.language === language.code
                                      ? "bg-[#00E5FF]/20 text-[#00E5FF]"
                                      : "text-slate-300 hover:bg-[#00E5FF]/10"
                                  }`}
                                >
                                  {language.label} ({language.code})
                                </button>
                              ))}
                              {filteredLanguages.length === 0 && (
                                <p className="px-3 py-2 text-xs text-slate-500">
                                  No language matches your search.
                                </p>
                              )}
                            </div>
                          )}
                        </div>
                        <p className="text-xs text-slate-500">
                          Selected language code: <code>{settings.language}</code>
                        </p>
                      </div>
                    </div>
                  </div>
                )}

                {activeTab === "audio" && (
                  <div className="scribble-border scribble-glow rounded-2xl p-6">
                    <h2 className="mb-5 flex items-center gap-2 text-lg font-semibold text-[#00E5FF]"><IconAudio className="h-5 w-5" />Audio</h2>
                    <div className="grid gap-2">
                      <label className="text-sm font-medium text-slate-300">
                        Microphone Input
                      </label>
                      <select
                        className="scribble-input h-11 rounded-xl px-4 text-sm"
                        value={settings.inputDevice}
                        onChange={(event) =>
                          setSettings((previous) => ({
                            ...previous,
                            inputDevice: event.target.value,
                          }))
                        }
                      >
                        {inputDevices.map((deviceName) => (
                          <option key={deviceName} value={deviceName}>
                            {deviceName === "default" ? "System Default" : deviceName}
                          </option>
                        ))}
                      </select>
                      <p className="text-xs text-slate-500">
                        Pick which microphone is used when recording starts.
                      </p>
                    </div>
                  </div>
                )}

                {activeTab === "runtime" && (
                  <div className="scribble-border scribble-glow rounded-2xl p-6">
                    <h2 className="mb-5 flex items-center gap-2 text-lg font-semibold text-[#00E5FF]"><IconRuntime className="h-5 w-5" />Runtime</h2>
                    <div className="space-y-5">
                      <div className="grid gap-2">
                        <label className="text-sm font-medium text-slate-300">
                          Python Command
                        </label>
                        <input
                          className="scribble-input h-11 rounded-xl px-4 text-sm"
                          value={settings.pythonCommand}
                          onChange={(event) =>
                            setSettings((previous) => ({
                              ...previous,
                              pythonCommand: event.target.value,
                            }))
                          }
                          placeholder="python"
                        />
                        <p className="text-xs text-slate-500">
                          Use <code>python</code>, <code>py</code>, or full path to interpreter.
                        </p>
                      </div>

                      <div className={`rounded-xl border px-4 py-3 text-sm ${statusColor}`}>
                        <p className="font-medium">ASR bootstrap state</p>
                        <p className="mt-1 text-slate-300">{status.message ?? "Ready"}</p>
                      </div>
                    </div>
                  </div>
                )}
              </div>
            </div>

            <footer className="flex items-center gap-3 border-t border-[#00E5FF]/20 bg-[#060d18]/75 p-4">
              <button
                type="button"
                onClick={persistSettings}
                disabled={saving}
                className="scribble-button inline-flex h-11 items-center gap-2 rounded-xl px-6 text-sm"
              >
                <IconSave className="h-4 w-4" />
                {saving ? "Saving..." : "Save Settings"}
              </button>

              <button
                type="button"
                onClick={toggleDictation}
                disabled={status.phase === "bootstrapping"}
                className="scribble-button scribble-button-secondary inline-flex h-11 items-center gap-2 rounded-xl px-6 text-sm"
              >
                {status.phase === "listening" ? (
                  <IconStop className="h-4 w-4" />
                ) : (
                  <IconPlay className="h-4 w-4" />
                )}
                {status.phase === "listening" ? "Stop Dictation" : "Start Dictation"}
              </button>
            </footer>
          </section>
        </div>
      </div>
    </main>
  );
}

function App() {
  const isOverlayWindow = useMemo(() => {
    if (typeof window === "undefined") {
      return false;
    }

    const params = new URLSearchParams(window.location.search);
    return params.get("overlay") === "1";
  }, []);

  if (isOverlayWindow) {
    return <OverlayPill />;
  }

  return <SettingsPage />;
}

export default App;
