import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open as openExternal } from "@tauri-apps/plugin-shell";

interface Settings {
  microphone: string;
  engine: string;
  whisperModel: string;
  groqApiKey: string;
  recordingMode: string;
  hotkey: string;
  streaming: boolean;
  groqKeyConfigured: boolean;
  launchAtLogin: boolean;
  showInDock: boolean;
  dictationSounds: boolean;
  pressEnterCommand: boolean;
  cleanupMode: string;
  llmModel: string;
}

interface VersionInfo {
  version: string;
  gitHash: string;
  dirty: boolean;
}

interface StatsSummary {
  today: number;
  total: number;
  streak: number;
  total_words: number;
  wpm: number;
  last30: number[];
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
const cleanupModeSelect = $<HTMLSelectElement>("cleanup-mode-select");
const llmSettings = $("llm-settings");
const llmModelSelect = $<HTMLSelectElement>("llm-model-select");
const llmDownloadBtn = $<HTMLButtonElement>("llm-download-btn");
const llmDownloadProgress = $("llm-download-progress");
const llmProgressFill = $("llm-progress-fill");
const groqKey = $<HTMLInputElement>("groq-key");
const keySave = $<HTMLButtonElement>("key-save");
const keyStatus = $("key-status");
const modeToggle = $("mode-toggle");
const modePtt = $("mode-ptt");
const hotkeyText = $("hotkey-text");
const streamingToggle = $<HTMLButtonElement>("streaming-toggle");

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

// Activate Pro buttons → open marketing site
const PRO_URL = "https://www.chibiteklabs.com";
const proHandler = (e?: Event) => {
  e?.preventDefault();
  openExternal(PRO_URL).catch((err) => console.error("open chibiteklabs.com failed:", err));
};
$("open-pro").addEventListener("click", proHandler);
$("cta-pro").addEventListener("click", proHandler);
document.querySelectorAll(".locked-card .btn-primary").forEach((b) => b.addEventListener("click", proHandler));
document.getElementById("help-pro-link")?.addEventListener("click", proHandler);

// Help button in sidebar footer → switch main view to help
$("open-help").addEventListener("click", () => {
  document.querySelectorAll(".nav-item").forEach((n) => n.classList.remove("active"));
  document.querySelectorAll<HTMLElement>(".view").forEach((s) => s.classList.remove("active"));
  document.querySelector('.view[data-view="help"]')?.classList.add("active");
});

// Auto-hide first-time setup card once the user has completed a full dictation.
// A successful Recording → Transcribing → Ready cycle proves mic + accessibility +
// automation permissions all worked, so the prompts won't fire again.
const SETUP_DONE_KEY = "mabel.setupComplete";
const setupCard = document.getElementById("setup-card");
function hideSetupCardIfDone() {
  if (setupCard && localStorage.getItem(SETUP_DONE_KEY) === "1") {
    setupCard.style.display = "none";
  }
}
hideSetupCardIfDone();

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
  cleanupModeSelect.value = currentSettings.cleanupMode || "rules";
  llmModelSelect.value = currentSettings.llmModel || "standard";
  applyCleanupModeUi();
  await checkLlmModelStatus();
  // The actual key is never echoed back from the keychain. We just show
  // "Saved" if a key was previously stored, and let the user overwrite it.
  groqKey.value = "";
  groqKey.placeholder = currentSettings.groqKeyConfigured ? "•••••••••••••••• (stored)" : "gsk_...";
  keyStatus.classList.toggle("hidden", !currentSettings.groqKeyConfigured);
  setRecordingMode(currentSettings.recordingMode);
  streamingToggle.setAttribute("aria-checked", String(currentSettings.streaming));
  setSwitch(autostartToggle, currentSettings.launchAtLogin);
  setSwitch(dockToggle, currentSettings.showInDock);
  setSwitch(soundsToggle, currentSettings.dictationSounds);
  setSwitch(pressEnterToggle, currentSettings.pressEnterCommand);

  const formatted = formatHotkey(currentSettings.hotkey);
  hotkeyText.textContent = formatted;
  homeHotkey.textContent = formatted;
}

async function loadVersion() {
  try {
    const info = await invoke<VersionInfo>("get_version");
    const footer = document.getElementById("footer-version");
    if (footer) footer.textContent = `v${info.version} · ${info.gitHash} · © Chibitek Labs`;
    const about = document.getElementById("about-version");
    if (about) about.textContent = `Version ${info.version} · ${info.gitHash}`;
  } catch (e) {
    console.error("get_version failed:", e);
  }
}

const insWpm = document.getElementById("ins-wpm");
const insTotalWords = document.getElementById("ins-total-words");
const insTotal = document.getElementById("ins-total");
const insStreak = document.getElementById("ins-streak");
const streakGrid = document.getElementById("streak-grid");
const statToday = document.getElementById("stat-today");
const statTotal = document.getElementById("stat-total");
const statStreak = document.getElementById("stat-streak");

function fmt(n: number): string {
  return n.toLocaleString();
}

async function loadStats() {
  try {
    const s = await invoke<StatsSummary>("get_stats");
    if (statToday) statToday.textContent = fmt(s.today);
    if (statTotal) statTotal.textContent = fmt(s.total);
    if (statStreak) statStreak.textContent = fmt(s.streak);
    if (insWpm) insWpm.textContent = fmt(s.wpm);
    if (insTotalWords) insTotalWords.textContent = fmt(s.total_words);
    if (insTotal) insTotal.textContent = fmt(s.total);
    if (insStreak) insStreak.textContent = fmt(s.streak);
    if (streakGrid) {
      const max = Math.max(1, ...s.last30);
      streakGrid.innerHTML = "";
      s.last30.forEach((count) => {
        const cell = document.createElement("span");
        cell.className = "streak-cell";
        if (count === 0) cell.classList.add("l0");
        else {
          const pct = count / max;
          if (pct > 0.75) cell.classList.add("l4");
          else if (pct > 0.5) cell.classList.add("l3");
          else if (pct > 0.25) cell.classList.add("l2");
          else cell.classList.add("l1");
        }
        cell.title = `${count} dictation${count === 1 ? "" : "s"}`;
        streakGrid.appendChild(cell);
      });
    }
  } catch (e) {
    console.error("get_stats failed:", e);
  }
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

async function checkLlmModelStatus() {
  const downloaded = await invoke<boolean>("check_llm_model_downloaded", {
    model: llmModelSelect.value,
  });
  llmDownloadBtn.textContent = downloaded ? "Downloaded" : "Download";
  llmDownloadBtn.disabled = downloaded;
}

function applyCleanupModeUi() {
  llmSettings.classList.toggle("hidden", cleanupModeSelect.value !== "llm");
}

async function saveSettings() {
  // Most settings saves should never carry the key — we don't want every mic
  // change to trip the keychain prompt on unsigned builds. The key is saved
  // explicitly via the Save button next to the input.
  currentSettings.microphone = micSelect.value;
  currentSettings.whisperModel = modelSelect.value;
  currentSettings.cleanupMode = cleanupModeSelect.value;
  currentSettings.llmModel = llmModelSelect.value;
  const previousKey = currentSettings.groqApiKey;
  currentSettings.groqApiKey = "";
  await invoke("save_settings", { settings: currentSettings });
  currentSettings.groqApiKey = previousKey;
}

async function saveGroqKey() {
  const value = groqKey.value.trim();
  if (!value) return;
  const settingsWithKey = { ...currentSettings, groqApiKey: value };
  try {
    await invoke("save_settings", { settings: settingsWithKey });
    currentSettings.groqKeyConfigured = true;
    groqKey.value = "";
    groqKey.placeholder = "•••••••••••••••• (stored)";
    keyStatus.textContent = "Saved";
    keyStatus.classList.remove("hidden");
    keyStatus.classList.remove("flash");
    void keyStatus.offsetWidth; // restart animation
    keyStatus.classList.add("flash");
  } catch (e) {
    console.error("save groq key failed:", e);
    keyStatus.textContent = "Error";
    keyStatus.classList.remove("hidden");
  }
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

cleanupModeSelect.addEventListener("change", async () => {
  applyCleanupModeUi();
  await saveSettings();
  if (cleanupModeSelect.value === "llm") {
    // Best-effort warm start. Errors are logged; the actual cleanup path falls
    // back to rules if the server isn't ready when dictation lands.
    invoke("ensure_llm_started").catch((e) => console.error("LLM warm start:", e));
  }
});

llmModelSelect.addEventListener("change", async () => {
  await checkLlmModelStatus();
  await saveSettings();
});

llmDownloadBtn.addEventListener("click", async () => {
  llmDownloadBtn.disabled = true;
  llmDownloadProgress.classList.remove("hidden");
  llmProgressFill.style.width = "0%";
  try {
    await invoke("download_llm_model", { model: llmModelSelect.value });
    llmDownloadBtn.textContent = "Downloaded";
  } catch (e) {
    llmDownloadBtn.textContent = "Retry";
    llmDownloadBtn.disabled = false;
    console.error("LLM download failed:", e);
  }
  llmDownloadProgress.classList.add("hidden");
});

keySave.addEventListener("click", () => saveGroqKey());
groqKey.addEventListener("keydown", (e) => {
  if (e.key === "Enter") saveGroqKey();
});
modeToggle.addEventListener("click", () => { setRecordingMode("toggle"); saveSettings(); });
modePtt.addEventListener("click", () => { setRecordingMode("push-to-talk"); saveSettings(); });

streamingToggle.addEventListener("click", () => {
  const next = streamingToggle.getAttribute("aria-checked") !== "true";
  streamingToggle.setAttribute("aria-checked", String(next));
  currentSettings.streaming = next;
  saveSettings();
});

const autostartToggle = $<HTMLButtonElement>("autostart-toggle");
const dockToggle = $<HTMLButtonElement>("dock-toggle");
const soundsToggle = $<HTMLButtonElement>("sounds-toggle");
const pressEnterToggle = $<HTMLButtonElement>("press-enter-toggle");

function setSwitch(btn: HTMLButtonElement, on: boolean) {
  btn.setAttribute("aria-checked", String(on));
}

autostartToggle.addEventListener("click", async () => {
  const next = autostartToggle.getAttribute("aria-checked") !== "true";
  setSwitch(autostartToggle, next);
  currentSettings.launchAtLogin = next;
  try {
    await invoke("set_launch_at_login", { enabled: next });
    await saveSettings();
  } catch (e) {
    console.error("set_launch_at_login failed:", e);
    setSwitch(autostartToggle, !next);
    currentSettings.launchAtLogin = !next;
  }
});

dockToggle.addEventListener("click", async () => {
  const next = dockToggle.getAttribute("aria-checked") !== "true";
  setSwitch(dockToggle, next);
  currentSettings.showInDock = next;
  try {
    await invoke("set_show_in_dock", { show: next });
    await saveSettings();
  } catch (e) {
    console.error("set_show_in_dock failed:", e);
  }
});

soundsToggle.addEventListener("click", () => {
  const next = soundsToggle.getAttribute("aria-checked") !== "true";
  setSwitch(soundsToggle, next);
  currentSettings.dictationSounds = next;
  saveSettings();
});

pressEnterToggle.addEventListener("click", () => {
  const next = pressEnterToggle.getAttribute("aria-checked") !== "true";
  setSwitch(pressEnterToggle, next);
  currentSettings.pressEnterCommand = next;
  saveSettings();
});

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

let lastRecordingState = "Ready";
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
    // Transcribing → Ready means a paste just succeeded, so all required
    // permissions are granted. Stash the flag and hide the setup card.
    if (lastRecordingState === "Transcribing" && localStorage.getItem(SETUP_DONE_KEY) !== "1") {
      localStorage.setItem(SETUP_DONE_KEY, "1");
      hideSetupCardIfDone();
    }
  }
  lastRecordingState = state;
});

listen<DownloadProgress>("download-progress", (event) => {
  // The same progress event drives both the Whisper and LLM download bars,
  // since only one download runs at a time. Update whichever bar is currently
  // visible (its container is .hidden when not in flight).
  progressFill.style.width = `${event.payload.percent}%`;
  llmProgressFill.style.width = `${event.payload.percent}%`;
  // Mirror to first-run modal if it's the active context.
  const firstrunFill = document.getElementById("firstrun-fill");
  const firstrunPct = document.getElementById("firstrun-pct");
  if (firstrunFill) firstrunFill.style.width = `${event.payload.percent}%`;
  if (firstrunPct) firstrunPct.textContent = `${Math.round(event.payload.percent)}%`;
});

async function maybeRunFirstTimeSetup() {
  const settings = await invoke<Settings>("get_settings");
  const smallReady = await invoke<boolean>("check_model_downloaded", { modelSize: "small" });
  const mediumReady = await invoke<boolean>("check_model_downloaded", { modelSize: "medium" });
  if (smallReady || mediumReady) return; // already have a model

  const modal = document.getElementById("firstrun-modal")!;
  const body = document.getElementById("firstrun-body")!;
  const foot = document.getElementById("firstrun-foot")!;
  const done = document.getElementById("firstrun-done") as HTMLButtonElement;
  const retry = document.getElementById("firstrun-retry") as HTMLButtonElement;
  const fill = document.getElementById("firstrun-fill")!;
  const pct = document.getElementById("firstrun-pct")!;
  modal.classList.remove("hidden");

  const startDownload = async () => {
    body.textContent = "Downloading the Whisper Small model (~500 MB) so dictation works fully offline. This is a one-time setup.";
    foot.classList.remove("hidden");
    done.classList.add("hidden");
    retry.classList.add("hidden");
    fill.style.width = "0%";
    pct.textContent = "0%";
    // Front-load every macOS permission Mabel needs while the model downloads,
    // so the user grants them once during setup instead of being interrupted
    // mid-dictation later. Both calls are non-blocking — they trigger the
    // system prompts and return immediately. The Microphone prompt fires
    // automatically on first audio capture, no need to prime here.
    invoke("request_accessibility").catch((e) => console.error("accessibility prompt:", e));
    invoke("request_apple_events_permission").catch((e) => console.error("apple events prompt:", e));
    try {
      await invoke("download_model", { modelSize: "small" });
      // Persist Small as the active model.
      currentSettings = { ...settings, whisperModel: "small" };
      await invoke("save_settings", { settings: currentSettings });
      body.textContent = "Whisper Small is ready. Mabel works fully offline, on this Mac. Audio never leaves the device.";
      foot.innerHTML = 'For better accuracy on long dictations, switch to the <b>Medium</b> model anytime in <b>Settings → Engine</b>. It is a larger one-time download.';
      fill.style.width = "100%";
      pct.textContent = "100%";
      done.classList.remove("hidden");
    } catch (e) {
      console.error("first-run download failed:", e);
      body.textContent = "Couldn't download the model. Check your internet connection and retry.";
      foot.classList.add("hidden");
      retry.classList.remove("hidden");
    }
  };

  done.addEventListener("click", () => {
    modal.classList.add("hidden");
    loadSettings();
  });
  retry.addEventListener("click", () => startDownload());

  startDownload();
}

listen("stats-updated", () => {
  loadStats();
});

loadSettings();
loadVersion();
loadStats();
maybeRunFirstTimeSetup();

// Check Accessibility permission on launch. If not granted, fire the system
// dialog and also open the Privacy pane. Re-checks every 3s so the UI updates
// once the user toggles it on.
async function ensureAccessibility() {
  try {
    const trusted = await invoke<boolean>("check_accessibility");
    if (!trusted) {
      await invoke("request_accessibility");
      const interval = setInterval(async () => {
        const ok = await invoke<boolean>("check_accessibility");
        if (ok) clearInterval(interval);
      }, 3000);
    }
  } catch (e) {
    console.error("accessibility check failed:", e);
  }
}
ensureAccessibility();
