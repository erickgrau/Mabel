import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface Settings {
  microphone: string;
  engine: string;
  whisperModel: string;
  groqApiKey: string;
  recordingMode: string;
  hotkey: string;
}

interface MicDevice {
  name: string;
  is_default: boolean;
}

interface DownloadProgress {
  downloaded: number;
  total: number;
  percent: number;
}

const $ = <T extends HTMLElement = HTMLElement>(id: string) =>
  document.getElementById(id) as T;

const statusDot = $("status-dot");
const statusText = $("status-text");
const homeHotkey = $("home-hotkey");
const micSelect = $<HTMLSelectElement>("mic-select");
const engineLocal = $("engine-local");
const engineCloud = $("engine-cloud");
const localSettings = $("local-settings");
const cloudSettings = $("cloud-settings");
const modelSelect = $<HTMLSelectElement>("model-select");
const downloadBtn = $<HTMLButtonElement>("download-btn");
const downloadProgress = $("download-progress");
const progressFill = $("progress-fill");
const groqKey = $<HTMLInputElement>("groq-key");
const modeToggle = $("mode-toggle");
const modePtt = $("mode-ptt");
const hotkeyText = $("hotkey-text");

const appWindow = getCurrentWindow();
$("titlebar").addEventListener("mousedown", (e) => {
  if ((e.target as HTMLElement).closest("button, select, input, a, kbd")) return;
  appWindow.startDragging();
});

// Sidebar nav (Home + locked Pro views)
document.querySelectorAll<HTMLElement>(".nav-item").forEach((item) => {
  item.addEventListener("click", () => {
    const view = item.dataset.view!;
    document.querySelectorAll(".nav-item").forEach((n) => n.classList.remove("active"));
    document.querySelectorAll<HTMLElement>(".view").forEach((s) => s.classList.remove("active"));
    item.classList.add("active");
    document.querySelector(`.view[data-view="${view}"]`)?.classList.add("active");
  });
});

// Settings modal open/close
const modal = $("settings-modal");
$("open-settings").addEventListener("click", () => modal.classList.remove("hidden"));
$("close-settings").addEventListener("click", () => modal.classList.add("hidden"));
modal.querySelector(".modal-backdrop")?.addEventListener("click", () => modal.classList.add("hidden"));
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !modal.classList.contains("hidden") && !capturingHotkey) {
    modal.classList.add("hidden");
  }
});

// Modal pane nav
document.querySelectorAll<HTMLElement>(".modal-nav-item").forEach((item) => {
  item.addEventListener("click", () => {
    if (item.classList.contains("locked")) return;
    const pane = item.dataset.pane!;
    document.querySelectorAll(".modal-nav-item").forEach((n) => n.classList.remove("active"));
    document.querySelectorAll(".modal-pane").forEach((p) => p.classList.remove("active"));
    item.classList.add("active");
    document.querySelector(`.modal-pane[data-pane="${pane}"]`)?.classList.add("active");
  });
});

// Activate Pro buttons (placeholder)
const proHandler = () => alert("Mabel Pro is coming soon. Local LLM cleanup, app-aware formatting, and more.");
$("open-pro").addEventListener("click", proHandler);
$("cta-pro").addEventListener("click", proHandler);
document.querySelectorAll(".locked-card .btn-primary").forEach((b) => b.addEventListener("click", proHandler));
$("open-help").addEventListener("click", () => alert("Hold the hotkey, speak, release. The transcription is pasted at your cursor."));

let currentSettings: Settings;

async function loadSettings() {
  currentSettings = await invoke<Settings>("get_settings");

  const mics = await invoke<MicDevice[]>("list_microphones");
  micSelect.innerHTML = "";
  mics.forEach((mic) => {
    const option = document.createElement("option");
    option.value = mic.name;
    option.textContent = mic.name + (mic.is_default ? " (default)" : "");
    micSelect.appendChild(option);
  });
  micSelect.value = currentSettings.microphone || mics.find((m) => m.is_default)?.name || "";

  setEngine(currentSettings.engine);
  modelSelect.value = currentSettings.whisperModel;
  await checkModelStatus();
  groqKey.value = currentSettings.groqApiKey;
  setRecordingMode(currentSettings.recordingMode);

  const formatted = formatHotkey(currentSettings.hotkey);
  hotkeyText.textContent = formatted;
  homeHotkey.textContent = formatted;
}

function setEngine(engine: string) {
  currentSettings.engine = engine;
  engineLocal.classList.toggle("active", engine === "local");
  engineCloud.classList.toggle("active", engine === "cloud");
  localSettings.classList.toggle("hidden", engine !== "local");
  cloudSettings.classList.toggle("hidden", engine !== "cloud");
}

function setRecordingMode(mode: string) {
  currentSettings.recordingMode = mode;
  modeToggle.classList.toggle("active", mode === "toggle");
  modePtt.classList.toggle("active", mode === "push-to-talk");
}

async function checkModelStatus() {
  const downloaded = await invoke<boolean>("check_model_downloaded", {
    modelSize: modelSelect.value,
  });
  downloadBtn.textContent = downloaded ? "Downloaded" : "Download";
  downloadBtn.disabled = downloaded;
}

async function saveSettings() {
  currentSettings.microphone = micSelect.value;
  currentSettings.whisperModel = modelSelect.value;
  currentSettings.groqApiKey = groqKey.value;
  await invoke("save_settings", { settings: currentSettings });
}

engineLocal.addEventListener("click", () => { setEngine("local"); saveSettings(); });
engineCloud.addEventListener("click", () => { setEngine("cloud"); saveSettings(); });
micSelect.addEventListener("change", () => saveSettings());
modelSelect.addEventListener("change", async () => { await checkModelStatus(); saveSettings(); });

downloadBtn.addEventListener("click", async () => {
  downloadBtn.disabled = true;
  downloadProgress.classList.remove("hidden");
  progressFill.style.width = "0%";
  try {
    await invoke("download_model", { modelSize: modelSelect.value });
    downloadBtn.textContent = "Downloaded";
  } catch (e) {
    downloadBtn.textContent = "Retry";
    downloadBtn.disabled = false;
    console.error("Download failed:", e);
  }
  downloadProgress.classList.add("hidden");
});

groqKey.addEventListener("change", () => saveSettings());
modeToggle.addEventListener("click", () => { setRecordingMode("toggle"); saveSettings(); });
modePtt.addEventListener("click", () => { setRecordingMode("push-to-talk"); saveSettings(); });

function formatHotkey(accelerator: string): string {
  return accelerator.replace("CmdOrCtrl", "Cmd");
}

function eventToAccelerator(e: KeyboardEvent): string | null {
  if (["Control", "Meta", "Alt", "Shift"].includes(e.key)) return null;
  const parts: string[] = [];
  if (e.metaKey || e.ctrlKey) parts.push("CmdOrCtrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  const code = e.code;
  let key: string | null = null;
  if (code === "Space") key = "Space";
  else if (code === "Enter") key = "Enter";
  else if (code === "Tab") key = "Tab";
  else if (code === "Escape") key = "Escape";
  else if (code === "Backspace") key = "Backspace";
  else if (code === "Delete") key = "Delete";
  else if (code.startsWith("Arrow")) key = code.slice(5);
  else if (/^F\d{1,2}$/.test(code)) key = code;
  else if (code.startsWith("Key")) key = code.slice(3);
  else if (code.startsWith("Digit")) key = code.slice(5);
  else if (code === "Minus") key = "-";
  else if (code === "Equal") key = "=";
  else if (code === "BracketLeft") key = "[";
  else if (code === "BracketRight") key = "]";
  else if (code === "Backslash") key = "\\";
  else if (code === "Semicolon") key = ";";
  else if (code === "Quote") key = "'";
  else if (code === "Comma") key = ",";
  else if (code === "Period") key = ".";
  else if (code === "Slash") key = "/";
  else if (code === "Backquote") key = "`";
  else return null;
  const isFunctionKey = /^F\d{1,2}$/.test(key);
  if (parts.length === 0 && !isFunctionKey) return null;
  parts.push(key);
  return parts.join("+");
}

let capturingHotkey = false;
hotkeyText.addEventListener("click", () => {
  if (capturingHotkey) return;
  capturingHotkey = true;
  hotkeyText.classList.add("capturing");
  const previousText = hotkeyText.textContent ?? "";
  hotkeyText.textContent = "Press keys...";

  const cleanup = () => {
    capturingHotkey = false;
    hotkeyText.classList.remove("capturing");
    document.removeEventListener("keydown", onKey, true);
  };

  const onKey = async (e: KeyboardEvent) => {
    if (e.key === "Escape" && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
      e.preventDefault();
      e.stopPropagation();
      hotkeyText.textContent = previousText;
      cleanup();
      return;
    }
    const accelerator = eventToAccelerator(e);
    if (!accelerator) {
      e.preventDefault();
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    try {
      await invoke("update_hotkey", { hotkey: accelerator });
      currentSettings.hotkey = accelerator;
      const formatted = formatHotkey(accelerator);
      hotkeyText.textContent = formatted;
      homeHotkey.textContent = formatted;
    } catch (err) {
      console.error("update_hotkey failed:", err);
      hotkeyText.textContent = previousText;
      alert(`Couldn't bind that combination: ${err}`);
    } finally {
      cleanup();
    }
  };

  document.addEventListener("keydown", onKey, true);
});

listen<string>("recording-state", (event) => {
  const state = event.payload;
  statusDot.className = "status-dot";
  if (state === "Recording") {
    statusDot.classList.add("recording");
    statusText.textContent = "Recording";
  } else if (state === "Transcribing") {
    statusDot.classList.add("transcribing");
    statusText.textContent = "Transcribing";
  } else {
    statusText.textContent = "Ready";
  }
});

listen<DownloadProgress>("download-progress", (event) => {
  progressFill.style.width = `${event.payload.percent}%`;
});

loadSettings();
