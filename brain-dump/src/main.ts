import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface Settings {
  microphone: string;
  groqApiKey: string;
  recordingMode: string;
  hotkeyPasteOnly: string;
  hotkeyDbPaste: string;
  language: string;
  supabaseUrl: string;
  supabaseAnonKey: string;
  captureContext: boolean;
  whisperModel: string;
  vocabulary: string;
}

interface MicDevice {
  name: string;
  is_default: boolean;
}

interface QuotaStatus {
  date: string;
  used: number;
  limit: number;
  warned: boolean;
}

// DOM elements
const statusDot = document.getElementById("status-dot")!;
const statusText = document.getElementById("status-text")!;
const micSelect = document.getElementById("mic-select") as HTMLSelectElement;
const langSelect = document.getElementById("lang-select") as HTMLSelectElement;
const groqKey = document.getElementById("groq-key") as HTMLInputElement;
const supabaseUrl = document.getElementById("supabase-url") as HTMLInputElement;
const supabaseKey = document.getElementById("supabase-key") as HTMLInputElement;
const captureContextCheck = document.getElementById("capture-context") as HTMLInputElement;
const modeToggle = document.getElementById("mode-toggle")!;
const modePtt = document.getElementById("mode-ptt")!;
const hotkeyPasteOnlyBtn = document.getElementById("hotkey-paste-only-btn") as HTMLButtonElement;
const hotkeyDbPasteBtn = document.getElementById("hotkey-db-paste-btn") as HTMLButtonElement;
const modelTurbo = document.getElementById("model-turbo")!;
const modelLarge = document.getElementById("model-large")!;
const vocabularyTA = document.getElementById("vocabulary") as HTMLTextAreaElement;
const vocabCounter = document.getElementById("vocab-counter")!;
const quotaBadge = document.getElementById("quota-badge")!;
const quotaText = document.getElementById("quota-text")!;

// Section navigation
const navItems = document.querySelectorAll(".nav-item");
const sections = document.querySelectorAll(".content-section");

navItems.forEach((item) => {
  item.addEventListener("click", () => {
    const target = item.getAttribute("data-section");
    navItems.forEach((n) => n.classList.remove("active"));
    sections.forEach((s) => s.classList.remove("active"));
    item.classList.add("active");
    document.getElementById(`section-${target}`)?.classList.add("active");
  });
});

// Window drag
const titlebar = document.getElementById("titlebar")!;
const sidebar = document.getElementById("sidebar")!;
const appWindow = getCurrentWindow();

titlebar.addEventListener("mousedown", (e) => {
  if ((e.target as HTMLElement).closest("button, select, input, a, .nav-item")) return;
  appWindow.startDragging();
});

sidebar.addEventListener("mousedown", (e) => {
  if ((e.target as HTMLElement).closest("button, select, input, a, .nav-item")) return;
  appWindow.startDragging();
});

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
  micSelect.value = currentSettings.microphone;

  langSelect.value = currentSettings.language;

  groqKey.value = currentSettings.groqApiKey;
  supabaseUrl.value = currentSettings.supabaseUrl;
  supabaseKey.value = currentSettings.supabaseAnonKey;
  captureContextCheck.checked = currentSettings.captureContext;

  setRecordingMode(currentSettings.recordingMode);
  setWhisperModel(currentSettings.whisperModel);

  vocabularyTA.value = currentSettings.vocabulary;
  updateVocabCounter();

  hotkeyPasteOnlyBtn.textContent = formatHotkey(currentSettings.hotkeyPasteOnly);
  hotkeyDbPasteBtn.textContent = formatHotkey(currentSettings.hotkeyDbPaste);
}

function setWhisperModel(model: string) {
  currentSettings.whisperModel = model;
  modelTurbo.classList.toggle("active", model === "whisper-large-v3-turbo");
  modelLarge.classList.toggle("active", model === "whisper-large-v3");
}

function updateVocabCounter() {
  const text = vocabularyTA.value.trim();
  const wordCount = text === "" ? 0 : text.split(/\s+|,/).filter(Boolean).length;
  vocabCounter.textContent = `${wordCount} mots`;
  vocabCounter.classList.remove("warn", "over");
  if (wordCount > 200) vocabCounter.classList.add("over");
  else if (wordCount > 150) vocabCounter.classList.add("warn");
}

function formatHotkey(h: string): string {
  return h.replace("CmdOrCtrl", "Ctrl");
}

function setRecordingMode(mode: string) {
  currentSettings.recordingMode = mode;
  modeToggle.classList.toggle("active", mode === "toggle");
  modePtt.classList.toggle("active", mode === "push-to-talk");
}

async function saveSettings() {
  currentSettings.microphone = micSelect.value;
  currentSettings.groqApiKey = groqKey.value;
  currentSettings.language = langSelect.value;
  currentSettings.supabaseUrl = supabaseUrl.value;
  currentSettings.supabaseAnonKey = supabaseKey.value;
  currentSettings.captureContext = captureContextCheck.checked;
  currentSettings.vocabulary = vocabularyTA.value;
  await invoke("save_settings", { settings: currentSettings });
}

async function refreshQuota() {
  try {
    const q = await invoke<QuotaStatus>("get_quota_status");
    renderQuota(q);
  } catch (e) {
    console.error("get_quota_status failed:", e);
  }
}

function renderQuota(q: QuotaStatus) {
  quotaText.textContent = `Groq : ${q.used} / ${q.limit}`;
  quotaBadge.classList.remove("warn", "blocked");
  if (q.used >= q.limit) {
    quotaBadge.classList.add("blocked");
  } else if (q.used >= Math.floor(q.limit * 0.75)) {
    quotaBadge.classList.add("warn");
  }
}

// ── Hotkey capture ─────────────────────────────────────
// Click on a hotkey button → "Appuie sur la combinaison…" → first valid combo replaces it.
// Modifiers seuls (Ctrl, Shift, Alt, Meta) ne valident pas. Echap annule.

const MODIFIER_KEYS = new Set(["Control", "Shift", "Alt", "Meta", "AltGraph"]);

function eventToAccelerator(e: KeyboardEvent): string | null {
  // Au moins une touche non-modificateur requise
  if (MODIFIER_KEYS.has(e.key)) return null;

  const parts: string[] = [];
  if (e.ctrlKey || e.metaKey) parts.push("CmdOrCtrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");

  // Normalisation de la touche principale
  let key = e.key;
  if (key === " ") key = "Space";
  else if (key.length === 1) key = key.toUpperCase();
  // Les flèches → "ArrowUp" etc. fonctionnent tel quel avec Tauri/global-shortcut
  parts.push(key);

  return parts.join("+");
}

let activeCapture: { btn: HTMLButtonElement; slot: string; previous: string } | null = null;

function startCapture(btn: HTMLButtonElement) {
  // Si un autre capture en cours, l'annuler
  if (activeCapture) cancelCapture();

  const slot = btn.dataset.slot!;
  activeCapture = { btn, slot, previous: btn.textContent || "" };
  btn.classList.add("capturing");
  btn.textContent = "Appuie sur la combinaison…";
}

function cancelCapture() {
  if (!activeCapture) return;
  activeCapture.btn.classList.remove("capturing");
  activeCapture.btn.textContent = activeCapture.previous;
  activeCapture = null;
}

async function commitCapture(accelerator: string) {
  if (!activeCapture) return;
  const { btn, slot } = activeCapture;
  try {
    await invoke("update_hotkey", { slot, accelerator });
    btn.textContent = formatHotkey(accelerator);
    if (slot === "paste_only") currentSettings.hotkeyPasteOnly = accelerator;
    else if (slot === "db_paste") currentSettings.hotkeyDbPaste = accelerator;
  } catch (e) {
    console.error("update_hotkey failed:", e);
    btn.textContent = activeCapture.previous;
    alert(`Impossible d'enregistrer ce raccourci : ${e}`);
  }
  btn.classList.remove("capturing");
  activeCapture = null;
}

[hotkeyPasteOnlyBtn, hotkeyDbPasteBtn].forEach((btn) => {
  btn.addEventListener("click", (e) => {
    e.stopPropagation();
    startCapture(btn);
  });
});

window.addEventListener("keydown", (e) => {
  if (!activeCapture) return;
  e.preventDefault();
  e.stopPropagation();

  if (e.key === "Escape") {
    cancelCapture();
    return;
  }

  const accel = eventToAccelerator(e);
  if (accel) {
    commitCapture(accel);
  }
}, true);

// Click ailleurs → annule
document.addEventListener("click", (e) => {
  if (!activeCapture) return;
  if (e.target === activeCapture.btn) return;
  cancelCapture();
});

// ── Event listeners ────────────────────────────────────
micSelect.addEventListener("change", saveSettings);
langSelect.addEventListener("change", saveSettings);
groqKey.addEventListener("change", saveSettings);
supabaseUrl.addEventListener("change", saveSettings);
supabaseKey.addEventListener("change", saveSettings);
captureContextCheck.addEventListener("change", saveSettings);
modeToggle.addEventListener("click", () => { setRecordingMode("toggle"); saveSettings(); });
modePtt.addEventListener("click", () => { setRecordingMode("push-to-talk"); saveSettings(); });
modelTurbo.addEventListener("click", () => { setWhisperModel("whisper-large-v3-turbo"); saveSettings(); });
modelLarge.addEventListener("click", () => { setWhisperModel("whisper-large-v3"); saveSettings(); });
vocabularyTA.addEventListener("input", updateVocabCounter);
vocabularyTA.addEventListener("change", saveSettings);

// State events
listen<string>("recording-state", (event) => {
  const state = event.payload;
  statusDot.className = "";
  if (state === "Recording") {
    statusDot.classList.add("recording");
    statusText.textContent = "Recording...";
  } else if (state === "Transcribing") {
    statusDot.classList.add("transcribing");
    statusText.textContent = "Transcribing...";
  } else {
    statusDot.classList.add("ready");
    statusText.textContent = "Ready";
  }
});

// Quota events
listen<QuotaStatus>("quota-warning", (event) => {
  renderQuota(event.payload);
  alert(
    `⚠️ Groq free tier : 75% utilisé (${event.payload.used}/${event.payload.limit}).\nReset à minuit UTC.`
  );
});

listen<QuotaStatus>("quota-blocked", (event) => {
  renderQuota(event.payload);
  alert(
    `🚫 Groq free tier épuisé (${event.payload.used}/${event.payload.limit}).\nReset à minuit UTC.`
  );
});

// Init
loadSettings();
refreshQuota();
setInterval(refreshQuota, 30_000);
