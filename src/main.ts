import { invoke } from "@tauri-apps/api/core";
import { listen, emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open as openExternal } from "@tauri-apps/plugin-shell";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, Update } from "@tauri-apps/plugin-updater";

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
  companionEnabled: boolean;
  companionSize: string;
  companionFrequency: string;
  companionVisit: string;
  lastSeenVersion: string;
  whisperLanguage: string;
  dictionary: string[];
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
const languageSelect = $<HTMLSelectElement>("language-select");
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
const checkUpdatesBtn = $<HTMLButtonElement>("check-updates-btn");
const showWhatsNewBtn = $<HTMLButtonElement>("show-whatsnew-btn");
const updateStatus = $("update-status");
const updateModal = $("update-modal");
const updateVersion = $("update-version");
const updateBody = $("update-body");
const updateInstallBtn = $<HTMLButtonElement>("update-install");
const updateLaterBtn = $<HTMLButtonElement>("update-later");
let pendingUpdate: Update | null = null;

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
$("open-settings").addEventListener("click", () => {
  modal.classList.remove("hidden");
  // Reconcile the keychain status only when Settings opens, not on every
  // recording-state poll. On dev builds, has_groq_key() prompts for keychain
  // access (signature changes per rebuild), so we want this fired only at a
  // moment the user expects keychain interaction.
  invoke<boolean>("reconcile_groq_keychain")
    .then((found) => {
      if (found && !currentSettings.groqKeyConfigured) {
        currentSettings.groqKeyConfigured = true;
        groqKey.placeholder = "•••••••••••••••• (stored)";
        keyStatus.textContent = "Saved";
        keyStatus.classList.remove("hidden");
      }
    })
    .catch((e) => console.error("reconcile_groq_keychain:", e));
});
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
let llmRuntimeAvailable = false;
const STREAMING_AVAILABLE = false;

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
  languageSelect.value = currentSettings.whisperLanguage || "multi";
  await checkModelStatus();
  renderDictionary();
  llmRuntimeAvailable = await invoke<boolean>("llm_runtime_available");
  cleanupModeSelect.value = currentSettings.cleanupMode || "rules";
  if (!llmRuntimeAvailable && cleanupModeSelect.value === "llm") {
    cleanupModeSelect.value = "rules";
    currentSettings.cleanupMode = "rules";
    await saveSettings();
  }
  llmModelSelect.value = currentSettings.llmModel || "standard";
  applyCleanupModeUi();
  await checkLlmModelStatus();
  // The actual key is never echoed back from the keychain. We just show
  // "Saved" if a key was previously stored, and let the user overwrite it.
  groqKey.value = "";
  groqKey.placeholder = currentSettings.groqKeyConfigured ? "•••••••••••••••• (stored)" : "gsk_...";
  keyStatus.classList.toggle("hidden", !currentSettings.groqKeyConfigured);
  setRecordingMode(currentSettings.recordingMode);
  if (!STREAMING_AVAILABLE && currentSettings.streaming) {
    currentSettings.streaming = false;
    await saveSettings();
  }
  streamingToggle.disabled = !STREAMING_AVAILABLE;
  streamingToggle.setAttribute("aria-disabled", String(!STREAMING_AVAILABLE));
  streamingToggle.setAttribute("aria-checked", String(STREAMING_AVAILABLE && currentSettings.streaming));
  setSwitch(autostartToggle, currentSettings.launchAtLogin);
  setSwitch(dockToggle, currentSettings.showInDock);
  setSwitch(companionToggle, currentSettings.companionEnabled);
  companionSizeSelect.value = currentSettings.companionSize || "medium";
  companionFrequencySelect.value = currentSettings.companionFrequency || "30min";
  companionVisitSelect.value = currentSettings.companionVisit || "medium";
  applyCompanionUi();
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

async function installUpdate(update: Update, statusEl: HTMLElement) {
  const next = update.version ? `v${update.version}` : "the latest version";
  statusEl.textContent = `Downloading ${next}...`;
  updateInstallBtn.disabled = true;
  let downloaded = 0;
  let contentLength = 0;
  await update.downloadAndInstall((event) => {
    if (event.event === "Started") {
      contentLength = event.data.contentLength ?? 0;
    } else if (event.event === "Progress") {
      downloaded += event.data.chunkLength;
      if (contentLength > 0) {
        const pct = Math.min(100, Math.round((downloaded / contentLength) * 100));
        statusEl.textContent = `Downloading ${next}... ${pct}%`;
      }
    } else if (event.event === "Finished") {
      statusEl.textContent = "Installing update...";
    }
  });

  statusEl.textContent = "Update installed. Relaunching...";
  await relaunch();
}

function showUpdatePrompt(update: Update) {
  pendingUpdate = update;
  updateVersion.textContent = `v${update.version} is available`;
  updateBody.innerHTML = renderChangelog(
    update.body || "A new signed Mabel update is ready to install."
  );
  updateInstallBtn.disabled = false;
  updateModal.classList.remove("hidden");
}

async function checkForUpdates(showPrompt: boolean) {
  if (!showPrompt) {
    checkUpdatesBtn.disabled = true;
    updateStatus.textContent = "Checking...";
  }
  try {
    const update = await check({ timeout: 30000 });
    if (!update) {
      if (!showPrompt) updateStatus.textContent = "Mabel is up to date.";
      return;
    }

    if (showPrompt) {
      showUpdatePrompt(update);
    } else {
      await installUpdate(update, updateStatus);
    }
  } catch (e) {
    console.error("update check failed:", e);
    if (!showPrompt) updateStatus.textContent = `Update failed: ${String(e)}`;
  } finally {
    if (!showPrompt) checkUpdatesBtn.disabled = false;
  }
}

checkUpdatesBtn.addEventListener("click", () => {
  checkForUpdates(false);
});

updateLaterBtn.addEventListener("click", () => {
  updateModal.classList.add("hidden");
});

updateInstallBtn.addEventListener("click", () => {
  if (!pendingUpdate) return;
  installUpdate(pendingUpdate, updateBody).catch((e) => {
    console.error("update install failed:", e);
    updateBody.textContent = `Update failed: ${String(e)}`;
    updateInstallBtn.disabled = false;
  });
});

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
    language: languageSelect?.value || "multi",
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
  const llmOption = cleanupModeSelect.querySelector('option[value="llm"]') as HTMLOptionElement | null;
  if (llmOption) {
    llmOption.disabled = !llmRuntimeAvailable;
    llmOption.textContent = llmRuntimeAvailable ? "AI cleanup (local)" : "AI cleanup (runtime unavailable)";
  }
  llmSettings.classList.toggle("hidden", cleanupModeSelect.value !== "llm" || !llmRuntimeAvailable);
}

async function saveSettings() {
  // Most settings saves should never carry the key — we don't want every mic
  // change to trip the keychain prompt on unsigned builds. The key is saved
  // explicitly via the Save button next to the input.
  currentSettings.microphone = micSelect.value;
  currentSettings.whisperModel = modelSelect.value;
  currentSettings.whisperLanguage = languageSelect.value;
  currentSettings.cleanupMode = cleanupModeSelect.value;
  currentSettings.llmModel = llmModelSelect.value;
  currentSettings.companionSize = companionSizeSelect.value;
  currentSettings.companionFrequency = companionFrequencySelect.value;
  currentSettings.companionVisit = companionVisitSelect.value;
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
languageSelect.addEventListener("change", async () => { await checkModelStatus(); saveSettings(); });

// Custom dictionary editor. Words are stored locally in settings and prepended
// to whisper.cpp's --prompt so proper nouns / acronyms / jargon decode
// correctly. Never uploaded.
const dictInput = $<HTMLInputElement>("dict-input");
const dictAddBtn = $<HTMLButtonElement>("dict-add-btn");
const dictList = $("dict-list");
const dictEmpty = $("dict-empty");

function renderDictionary() {
  if (!dictList) return;
  const words = currentSettings.dictionary || [];
  dictList.innerHTML = "";
  if (words.length === 0) {
    dictEmpty.classList.remove("hidden");
    return;
  }
  dictEmpty.classList.add("hidden");
  for (const word of words) {
    const chip = document.createElement("span");
    chip.className = "dict-chip";
    const text = document.createElement("span");
    text.textContent = word;
    const remove = document.createElement("button");
    remove.type = "button";
    remove.setAttribute("aria-label", `Remove ${word}`);
    remove.textContent = "×";
    remove.addEventListener("click", () => {
      currentSettings.dictionary = currentSettings.dictionary.filter((w) => w !== word);
      renderDictionary();
      saveSettings();
    });
    chip.appendChild(text);
    chip.appendChild(remove);
    dictList.appendChild(chip);
  }
}

function addDictionaryEntry() {
  const raw = dictInput.value.trim();
  if (!raw) return;
  // Allow multiple entries split by comma or newline so users can paste a list.
  const candidates = raw
    .split(/[,\n]/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
  const existing = new Set((currentSettings.dictionary || []).map((w) => w.toLowerCase()));
  const next = [...(currentSettings.dictionary || [])];
  for (const c of candidates) {
    if (!existing.has(c.toLowerCase())) {
      next.push(c);
      existing.add(c.toLowerCase());
    }
  }
  currentSettings.dictionary = next;
  dictInput.value = "";
  renderDictionary();
  saveSettings();
}

dictAddBtn.addEventListener("click", addDictionaryEntry);
dictInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter") {
    e.preventDefault();
    addDictionaryEntry();
  }
});

downloadBtn.addEventListener("click", async () => {
  downloadBtn.disabled = true;
  downloadProgress.classList.remove("hidden");
  progressFill.style.width = "0%";
  try {
    await invoke("download_model", {
      modelSize: modelSelect.value,
      language: languageSelect?.value || "multi",
    });
    downloadBtn.textContent = "Downloaded";
  } catch (e) {
    downloadBtn.textContent = "Retry";
    downloadBtn.disabled = false;
    console.error("Download failed:", e);
  }
  downloadProgress.classList.add("hidden");
});

cleanupModeSelect.addEventListener("change", async () => {
  if (cleanupModeSelect.value === "llm" && !llmRuntimeAvailable) {
    cleanupModeSelect.value = "rules";
  }
  applyCleanupModeUi();
  await saveSettings();
  if (cleanupModeSelect.value === "llm" && llmRuntimeAvailable) {
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
  if (!STREAMING_AVAILABLE) return;
  const next = streamingToggle.getAttribute("aria-checked") !== "true";
  streamingToggle.setAttribute("aria-checked", String(next));
  currentSettings.streaming = next;
  saveSettings();
});

const autostartToggle = $<HTMLButtonElement>("autostart-toggle");
const dockToggle = $<HTMLButtonElement>("dock-toggle");
const soundsToggle = $<HTMLButtonElement>("sounds-toggle");
const pressEnterToggle = $<HTMLButtonElement>("press-enter-toggle");
const companionToggle = $<HTMLButtonElement>("companion-toggle");
const companionSettings = $("companion-settings");
const companionFrequencyRow = $("companion-frequency-row");
const companionVisitRow = $("companion-visit-row");
const companionTestRow = $("companion-test-row");
const companionSizeSelect = $<HTMLSelectElement>("companion-size-select");
const companionFrequencySelect = $<HTMLSelectElement>("companion-frequency-select");
const companionVisitSelect = $<HTMLSelectElement>("companion-visit-select");
const companionTestBtn = $<HTMLButtonElement>("companion-test-btn");

function applyCompanionUi() {
  const on = companionToggle.getAttribute("aria-checked") === "true";
  for (const row of [companionSettings, companionFrequencyRow, companionVisitRow, companionTestRow]) {
    row.classList.toggle("hidden", !on);
  }
}

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

companionToggle.addEventListener("click", () => {
  const next = companionToggle.getAttribute("aria-checked") !== "true";
  setSwitch(companionToggle, next);
  currentSettings.companionEnabled = next;
  applyCompanionUi();
  saveSettings();
});

for (const sel of [companionSizeSelect, companionFrequencySelect, companionVisitSelect]) {
  sel.addEventListener("change", () => {
    currentSettings.companionSize = companionSizeSelect.value;
    currentSettings.companionFrequency = companionFrequencySelect.value;
    currentSettings.companionVisit = companionVisitSelect.value;
    saveSettings();
  });
}

companionTestBtn.addEventListener("click", () => {
  invoke("companion_visit_now").catch((e) => console.error("companion test:", e));
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

listen<string>("transcription-error", (event) => {
  const msg = event.payload || "Unknown transcription error";
  statusDot.className = "status-dot";
  statusText.textContent = "Transcription failed";
  console.error("transcription-error:", msg);
  alert(`Transcription failed: ${msg}`);
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
  // Any existing whisper ggml on disk skips first-run, regardless of size or
  // language variant — we don't want to nag a returning user with a fresh
  // download just because we added .en variants.
  const variants = [
    { modelSize: "small", language: "multi" },
    { modelSize: "small", language: "en" },
    { modelSize: "medium", language: "multi" },
    { modelSize: "medium", language: "en" },
  ];
  for (const v of variants) {
    if (await invoke<boolean>("check_model_downloaded", v)) return;
  }

  const modal = document.getElementById("firstrun-modal")!;
  const body = document.getElementById("firstrun-body")!;
  const foot = document.getElementById("firstrun-foot")!;
  const done = document.getElementById("firstrun-done") as HTMLButtonElement;
  const retry = document.getElementById("firstrun-retry") as HTMLButtonElement;
  const fill = document.getElementById("firstrun-fill")!;
  const pct = document.getElementById("firstrun-pct")!;
  modal.classList.remove("hidden");

  const startDownload = async () => {
    body.textContent = "Downloading the Whisper Small (English) model (~500 MB) so dictation works fully offline. This is a one-time setup.";
    foot.classList.remove("hidden");
    done.classList.add("hidden");
    retry.classList.add("hidden");
    fill.style.width = "0%";
    pct.textContent = "0%";
    // Don't proactively trigger system permission prompts here. Unsigned test
    // builds can re-prompt due to changing signatures, which is disruptive.
    // Prompts will appear when the relevant feature is actually used.
    try {
      await invoke("download_model", { modelSize: "small", language: "en" });
      // Persist Small (English-only) as the active model on first run —
      // best accuracy out of the box for English dictation.
      currentSettings = { ...settings, whisperModel: "small", whisperLanguage: "en" };
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
setTimeout(() => {
  checkForUpdates(true);
}, 2500);

// Check Accessibility status on launch, but do not auto-prompt. Unsigned test
// builds can trigger repeated permission dialogs because binary signatures
// change across builds.
async function ensureAccessibility() {
  try {
    const trusted = await invoke<boolean>("check_accessibility");
    if (!trusted) {
      console.warn("Accessibility not granted yet. Prompt is deferred until needed.");
    }
  } catch (e) {
    console.error("accessibility check failed:", e);
  }
}
ensureAccessibility();

async function showWhatsNew(markSeen: boolean) {
  const entry = await invoke<{ version: string; body: string } | null>("get_whats_new");
  if (!entry) {
    updateStatus.textContent = "No release notes are bundled for this version.";
    return false;
  }
  const modal = document.getElementById("whatsnew-modal")!;
  const verEl = document.getElementById("whatsnew-version")!;
  const bodyEl = document.getElementById("whatsnew-body")!;
  const done = document.getElementById("whatsnew-done")!;
  verEl.textContent = "v" + entry.version;
  bodyEl.innerHTML = renderChangelog(entry.body);
  modal.classList.remove("hidden");

  const dismiss = async () => {
    modal.classList.add("hidden");
    if (markSeen) await invoke("mark_version_seen");
  };
  done.replaceWith(done.cloneNode(true));
  document.getElementById("whatsnew-done")!.addEventListener("click", dismiss, { once: true });
  return true;
}

showWhatsNewBtn.addEventListener("click", () => {
  showWhatsNew(false).catch((e) => {
    console.error("show whats-new failed:", e);
    updateStatus.textContent = `What's New failed: ${String(e)}`;
  });
});

// What's New popup. On first launch after an update (or first launch ever
// after the user has finished the initial Whisper download), if the bundled
// changelog has an entry for the running version, pop a modal showing what
// changed. Marks the version as seen on dismiss so we don't re-show it.
async function maybeShowWhatsNew() {
  try {
    const settings = await invoke<Settings>("get_settings");
    const ver = await invoke<VersionInfo>("get_version");
    if (settings.lastSeenVersion === ver.version) return;
    // Don't pop on the very first install before they've used the app at all.
    // The first-run download modal handles that surface; popping What's New on
    // top of it would be noisy. We detect "first launch ever" as no
    // lastSeenVersion AND no Whisper model on disk.
    if (!settings.lastSeenVersion) {
      const variants = [
        { modelSize: "small", language: "multi" },
        { modelSize: "small", language: "en" },
        { modelSize: "medium", language: "multi" },
        { modelSize: "medium", language: "en" },
      ];
      let hasModel = false;
      for (const v of variants) {
        if (await invoke<boolean>("check_model_downloaded", v)) {
          hasModel = true;
          break;
        }
      }
      if (!hasModel) return;
    }
    const shown = await showWhatsNew(true);
    if (!shown) {
      // No changelog entry for this version — still mark it seen so we don't
      // try again next launch.
      await invoke("mark_version_seen");
    }
  } catch (e) {
    console.error("whats-new check failed:", e);
  }
}

// Tiny changelog renderer: handles ### subheaders and bullet lists, escapes
// everything else. Keeps the popup safe against accidental HTML in the
// changelog source.
function renderChangelog(md: string): string {
  const escape = (s: string) =>
    s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const lines = md.split("\n");
  const html: string[] = [];
  let inList = false;
  for (const raw of lines) {
    const line = raw.trim();
    if (!line) {
      if (inList) { html.push("</ul>"); inList = false; }
      continue;
    }
    if (line.startsWith("### ")) {
      if (inList) { html.push("</ul>"); inList = false; }
      html.push(`<h3 class="whatsnew-h">${escape(line.slice(4))}</h3>`);
    } else if (line.startsWith("- ")) {
      if (!inList) { html.push("<ul class=\"whatsnew-list\">"); inList = true; }
      html.push(`<li>${escape(line.slice(2))}</li>`);
    } else {
      if (inList) { html.push("</ul>"); inList = false; }
      html.push(`<p>${escape(line)}</p>`);
    }
  }
  if (inList) html.push("</ul>");
  return html.join("");
}

maybeShowWhatsNew();

// Easter egg: seven clicks anywhere on the hero (portrait + title block) within
// the first ten seconds of opening the app toggle Mochi mode (hero portrait,
// brand name, and view title swap). The flag is persisted in localStorage so
// the chosen skin survives across launches; the same gesture toggles it back.
(function mochiModeEasterEgg() {
  const STORAGE_KEY = "mabel.mochiMode";
  const UNLOCKED_KEY = "mabel.mochiUnlocked";
  const portrait = document.getElementById("hero-portrait") as HTMLImageElement | null;
  const title = document.getElementById("meet-title");
  const brand = document.getElementById("brand-name");
  const skinRow = document.getElementById("companion-skin-row");
  const skinSelect = document.getElementById("companion-skin-select") as HTMLSelectElement | null;

  const setMode = (on: boolean) => {
    if (on) {
      localStorage.setItem(STORAGE_KEY, "1");
      localStorage.setItem(UNLOCKED_KEY, "1");
    } else {
      localStorage.removeItem(STORAGE_KEY);
    }
    apply(on);
  };

  const apply = (on: boolean) => {
    if (portrait) {
      portrait.src = on ? "/mochi.png" : "/mabel.png";
      portrait.alt = on ? "Mochi" : "Mabel";
      portrait.classList.toggle("mochi", on);
    }
    if (title) title.textContent = on ? "Meet Mochi" : "Meet Mabel";
    if (brand) brand.textContent = on ? "Mochi" : "Mabel";
    // Once unlocked, expose the Settings toggle so the user doesn't have to
    // re-do the easter-egg gesture to switch back. Currently being in mochi
    // mode counts as unlocked too — covers users who activated the egg on a
    // build that didn't yet write the unlocked flag.
    if (on) localStorage.setItem(UNLOCKED_KEY, "1");
    if (skinRow && localStorage.getItem(UNLOCKED_KEY) === "1") {
      skinRow.classList.remove("hidden");
    }
    if (skinSelect) skinSelect.value = on ? "mochi" : "mabel";
    // Tell the companion window to swap its sprite skin. Re-emit a few times
    // because the companion's listener may not have attached yet on first
    // boot — the emits are idempotent.
    const payload = { skin: on ? "mochi" : "mabel" };
    emit("mabel-companion-skin", payload).catch(() => {});
    setTimeout(() => emit("mabel-companion-skin", payload).catch(() => {}), 500);
    setTimeout(() => emit("mabel-companion-skin", payload).catch(() => {}), 2000);
  };

  apply(localStorage.getItem(STORAGE_KEY) === "1");

  if (skinSelect) {
    skinSelect.addEventListener("change", () => {
      setMode(skinSelect.value === "mochi");
    });
  }

  const hero = document.querySelector(".hero") as HTMLElement | null;
  if (!hero) return;
  const start = Date.now();
  let count = 0;
  const onClick = () => {
    if (Date.now() - start > 10000) {
      hero.removeEventListener("click", onClick);
      return;
    }
    count++;
    console.log("[mochi-egg] click", count);
    if (count >= 7) {
      hero.removeEventListener("click", onClick);
      setMode(localStorage.getItem(STORAGE_KEY) !== "1");
    }
  };
  hero.addEventListener("click", onClick);
  setTimeout(() => hero.removeEventListener("click", onClick), 10100);
})();
