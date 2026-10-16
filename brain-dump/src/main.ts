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
  aiCorrection: boolean;
  snippets: Snippet[];
  keepHistory: boolean;
  replacements: Replacement[];
  ignoredSuggestions: string[];
}

interface Replacement {
  from: string;
  to: string;
}

interface HistoryEntry {
  id: number;
  date: string;
  text: string;
  edited: string | null;
  raw: string;
  app: string | null;
  durationMs: number;
}

interface Snippet {
  trigger: string;
  text: string;
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
const aiCorrectionCheck = document.getElementById("ai-correction") as HTMLInputElement;
const modeToggle = document.getElementById("mode-toggle")!;
const modePtt = document.getElementById("mode-ptt")!;
const hotkeyPasteOnlyBtn = document.getElementById("hotkey-paste-only-btn") as HTMLButtonElement;
const hotkeyDbPasteBtn = document.getElementById("hotkey-db-paste-btn") as HTMLButtonElement;
const modelTurbo = document.getElementById("model-turbo")!;
const modelLarge = document.getElementById("model-large")!;
const vocabChips = document.getElementById("vocab-chips")!;
const vocabInput = document.getElementById("vocab-input") as HTMLInputElement;
const vocabCounter = document.getElementById("vocab-counter")!;
let vocabWords: string[] = [];
const replacementChips = document.getElementById("replacement-chips")!;
const keepHistoryCheck = document.getElementById("keep-history") as HTMLInputElement;
const historyList = document.getElementById("history-list")!;
const historySearch = document.getElementById("history-search") as HTMLInputElement;
const historyClearBtn = document.getElementById("history-clear-btn") as HTMLButtonElement;
const vocabSuggestions = document.getElementById("vocab-suggestions")!;
let historyEntries: HistoryEntry[] = [];
let pendingSuggestions: Replacement[] = [];
const snippetList = document.getElementById("snippet-list")!;
const snippetAddBtn = document.getElementById("snippet-add-btn") as HTMLButtonElement;
const quotaBadge = document.getElementById("quota-badge")!;
const quotaText = document.getElementById("quota-text")!;
const quotaFill = document.getElementById("quota-fill")!;
const overlayShowBtn = document.getElementById("overlay-show-btn") as HTMLButtonElement;
const overlayHideBtn = document.getElementById("overlay-hide-btn") as HTMLButtonElement;
const overlayRepositionBtn = document.getElementById("overlay-reposition-btn") as HTMLButtonElement;
const quitAppBtn = document.getElementById("quit-app-btn") as HTMLButtonElement;

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

// The overlay title bar style only exists on macOS; Windows keeps its native title bar.
if (navigator.userAgent.includes("Windows")) titlebar.classList.add("hidden");

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
    option.textContent = mic.name + (mic.is_default ? " (par défaut)" : "");
    micSelect.appendChild(option);
  });
  micSelect.value = currentSettings.microphone;

  langSelect.value = currentSettings.language;

  groqKey.value = currentSettings.groqApiKey;
  supabaseUrl.value = currentSettings.supabaseUrl;
  supabaseKey.value = currentSettings.supabaseAnonKey;
  captureContextCheck.checked = currentSettings.captureContext;
  aiCorrectionCheck.checked = currentSettings.aiCorrection;
  keepHistoryCheck.checked = currentSettings.keepHistory;

  setRecordingMode(currentSettings.recordingMode);
  setWhisperModel(currentSettings.whisperModel);

  loadVocab(currentSettings.vocabulary);
  renderReplacements();
  renderSnippets();

  hotkeyPasteOnlyBtn.textContent = formatHotkey(currentSettings.hotkeyPasteOnly);
  hotkeyDbPasteBtn.textContent = formatHotkey(currentSettings.hotkeyDbPaste);
}

function setWhisperModel(model: string) {
  currentSettings.whisperModel = model;
  modelTurbo.classList.toggle("active", model === "whisper-large-v3-turbo");
  modelLarge.classList.toggle("active", model === "whisper-large-v3");
}

function parseVocab(raw: string): string[] {
  return raw
    .split(/[,\n]/)
    .map((w) => w.trim())
    .filter((w) => w.length > 0);
}

function loadVocab(raw: string) {
  vocabWords = parseVocab(raw);
  renderVocab();
}

function renderVocab() {
  vocabChips.querySelectorAll(".vocab-chip").forEach((c) => c.remove());
  for (const word of vocabWords) {
    const chip = document.createElement("span");
    chip.className = "vocab-chip";
    const text = document.createElement("span");
    text.className = "vocab-chip-text";
    text.textContent = word;
    const remove = document.createElement("button");
    remove.type = "button";
    remove.className = "vocab-chip-remove";
    remove.setAttribute("aria-label", `Supprimer ${word}`);
    remove.textContent = "×";
    remove.addEventListener("click", (e) => {
      e.stopPropagation();
      removeVocabWord(word);
    });
    chip.appendChild(text);
    chip.appendChild(remove);
    vocabChips.insertBefore(chip, vocabInput);
  }
  updateVocabCounter();
}

function addVocabWord(raw: string) {
  const word = raw.trim();
  if (!word) return;
  // Accept multiple words separated by comma in one paste
  const parts = parseVocab(word);
  let changed = false;
  for (const p of parts) {
    if (!vocabWords.some((w) => w.toLowerCase() === p.toLowerCase())) {
      vocabWords.push(p);
      changed = true;
    }
  }
  if (changed) {
    renderVocab();
    saveSettings();
  }
}

function removeVocabWord(word: string) {
  const before = vocabWords.length;
  vocabWords = vocabWords.filter((w) => w !== word);
  if (vocabWords.length !== before) {
    renderVocab();
    saveSettings();
  }
}

function serializeVocab(): string {
  return vocabWords.join(", ");
}

// Limite du prompt Whisper, en phase avec WHISPER_PROMPT_MAX_CHARS dans vocab.rs
const WHISPER_VOCAB_MAX_CHARS = 550;

function updateVocabCounter() {
  const count = vocabWords.length;
  const chars = serializeVocab().length;
  const words = `${count} mot${count > 1 ? "s" : ""}`;
  vocabCounter.classList.remove("warn", "over");
  if (chars > WHISPER_VOCAB_MAX_CHARS) {
    vocabCounter.textContent = `${words} · limite Whisper dépassée : seuls les plus récents lui sont envoyés`;
    vocabCounter.classList.add("over");
  } else {
    vocabCounter.textContent = `${words} · ${chars} / ${WHISPER_VOCAB_MAX_CHARS} caractères`;
    if (chars > WHISPER_VOCAB_MAX_CHARS * 0.8) vocabCounter.classList.add("warn");
  }
}

function renderReplacements() {
  replacementChips.innerHTML = "";
  if (currentSettings.replacements.length === 0) {
    const empty = document.createElement("span");
    empty.className = "muted-hint";
    empty.textContent = "Aucune pour l'instant";
    replacementChips.appendChild(empty);
    return;
  }
  currentSettings.replacements.forEach((r, index) => {
    const chip = document.createElement("span");
    chip.className = "vocab-chip";
    const text = document.createElement("span");
    text.className = "vocab-chip-text";
    text.textContent = `${r.from} → ${r.to}`;
    const remove = document.createElement("button");
    remove.type = "button";
    remove.className = "vocab-chip-remove";
    remove.setAttribute("aria-label", `Supprimer ${r.from}`);
    remove.textContent = "×";
    remove.addEventListener("click", () => {
      currentSettings.replacements.splice(index, 1);
      renderReplacements();
      saveSettings();
    });
    chip.append(text, remove);
    replacementChips.appendChild(chip);
  });
}

// ── Historique ─────────────────────────────────────────

const dateFormat = new Intl.DateTimeFormat("fr-FR", {
  day: "numeric",
  month: "short",
  hour: "2-digit",
  minute: "2-digit",
});

function entryText(e: HistoryEntry): string {
  return e.edited ?? e.text;
}

function renderHistory() {
  historyList.innerHTML = "";
  const query = historySearch.value.trim().toLowerCase();
  const shown = historyEntries.filter(
    (e) => !query || entryText(e).toLowerCase().includes(query) || e.raw.toLowerCase().includes(query)
  );
  if (shown.length === 0) {
    const empty = document.createElement("p");
    empty.className = "muted-hint";
    if (query) empty.textContent = "Aucune dictée ne correspond.";
    else if (!keepHistoryCheck.checked) empty.textContent = "L'historique est désactivé.";
    else empty.textContent = "Aucune dictée pour l'instant.";
    historyList.appendChild(empty);
    return;
  }
  for (const entry of shown) historyList.appendChild(historyCard(entry));
}

function historyCard(entry: HistoryEntry): HTMLElement {
  const card = document.createElement("div");
  card.className = "settings-list history-card";

  const meta = document.createElement("span");
  meta.className = "history-meta";
  const parts = [dateFormat.format(new Date(entry.date))];
  if (entry.app) parts.push(entry.app.replace(/\.exe$/i, ""));
  if (entry.durationMs) parts.push(`${Math.max(1, Math.round(entry.durationMs / 1000))} s`);
  if (entry.edited !== null) parts.push("modifiée");
  meta.textContent = parts.join(" · ");
  meta.title = `Transcription Whisper brute :\n${entry.raw}`;

  const copy = document.createElement("button");
  copy.type = "button";
  copy.className = "btn-secondary";
  copy.textContent = "Copier";
  copy.addEventListener("click", async () => {
    try {
      await invoke("copy_text", { text: entryText(entry) });
      copy.textContent = "Copié";
      setTimeout(() => (copy.textContent = "Copier"), 1500);
    } catch (e) {
      alert(`Impossible de copier : ${e}`);
    }
  });

  const head = document.createElement("div");
  head.className = "snippet-head history-head";
  head.append(meta, copy);

  const text = document.createElement("textarea");
  text.className = "snippet-text";
  text.rows = Math.min(8, Math.max(2, Math.ceil(entryText(entry).length / 90)));
  text.value = entryText(entry);
  text.addEventListener("change", () => saveEdit(entry, text.value));

  card.append(head, text);
  return card;
}

async function saveEdit(entry: HistoryEntry, value: string) {
  try {
    const result = await invoke<{ entry: HistoryEntry; suggestions: Replacement[] }>(
      "edit_history_entry",
      { id: entry.id, text: value }
    );
    Object.assign(entry, result.entry);
    for (const s of result.suggestions) {
      if (!pendingSuggestions.some((p) => p.to.toLowerCase() === s.to.toLowerCase())) {
        pendingSuggestions.push(s);
      }
    }
    renderHistory();
    renderSuggestions();
  } catch (e) {
    alert(`Impossible d'enregistrer la modification : ${e}`);
  }
}

async function loadHistory() {
  historyEntries = await invoke<HistoryEntry[]>("get_history");
  renderHistory();
}

// ── Dictionnaire qui apprend ───────────────────────────

function renderSuggestions() {
  vocabSuggestions.innerHTML = "";
  pendingSuggestions.forEach((s, index) => {
    const label = document.createElement("div");
    label.className = "setting-label";
    const title = document.createElement("span");
    title.className = "label-text";
    title.textContent = `Ajouter « ${s.to} » au vocabulaire ?`;
    const hint = document.createElement("span");
    hint.className = "label-hint";
    hint.textContent = `« ${s.from} » sera aussi remplacé par « ${s.to} » dans tes prochaines dictées.`;
    label.append(title, hint);

    const add = document.createElement("button");
    add.type = "button";
    add.className = "btn-secondary";
    add.textContent = "Ajouter";
    add.addEventListener("click", () => {
      pendingSuggestions.splice(index, 1);
      if (!vocabWords.some((w) => w.toLowerCase() === s.to.toLowerCase())) {
        vocabWords.push(s.to);
        renderVocab();
      }
      if (!currentSettings.replacements.some((r) => r.from.toLowerCase() === s.from.toLowerCase())) {
        currentSettings.replacements.push(s);
        renderReplacements();
      }
      renderSuggestions();
      saveSettings();
    });

    const ignore = document.createElement("button");
    ignore.type = "button";
    ignore.className = "btn-secondary";
    ignore.textContent = "Ignorer";
    ignore.addEventListener("click", () => {
      pendingSuggestions.splice(index, 1);
      currentSettings.ignoredSuggestions.push(s.to.toLowerCase());
      renderSuggestions();
      saveSettings();
    });

    const actions = document.createElement("div");
    actions.className = "suggestion-actions";
    actions.append(add, ignore);

    const row = document.createElement("div");
    row.className = "setting-row";
    row.append(label, actions);

    const card = document.createElement("div");
    card.className = "settings-list suggestion-card";
    card.appendChild(row);
    vocabSuggestions.appendChild(card);
  });
}

// ── Raccourcis vocaux ──────────────────────────────────

function renderSnippets() {
  snippetList.innerHTML = "";
  currentSettings.snippets.forEach((snippet, index) => {
    const card = document.createElement("div");
    card.className = "settings-list snippet-card";

    const trigger = document.createElement("input");
    trigger.type = "text";
    trigger.className = "snippet-trigger";
    trigger.placeholder = "Déclencheur, ex. ma signature";
    trigger.value = snippet.trigger;
    trigger.addEventListener("change", () => {
      snippet.trigger = trigger.value.trim();
      saveSettings();
    });

    const remove = document.createElement("button");
    remove.type = "button";
    remove.className = "btn-secondary";
    remove.textContent = "Supprimer";
    remove.addEventListener("click", () => {
      currentSettings.snippets.splice(index, 1);
      renderSnippets();
      saveSettings();
    });

    const head = document.createElement("div");
    head.className = "snippet-head";
    head.append(trigger, remove);

    const text = document.createElement("textarea");
    text.className = "snippet-text";
    text.placeholder = "Texte collé à la place (plusieurs lignes possibles)";
    text.rows = 3;
    text.value = snippet.text;
    text.addEventListener("change", () => {
      snippet.text = text.value;
      saveSettings();
    });

    card.append(head, text);
    snippetList.appendChild(card);
  });
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
  currentSettings.aiCorrection = aiCorrectionCheck.checked;
  currentSettings.keepHistory = keepHistoryCheck.checked;
  currentSettings.vocabulary = serializeVocab();
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
  quotaText.textContent = `${q.used} / ${q.limit}`;
  quotaFill.style.width = `${Math.min(100, (q.used / q.limit) * 100)}%`;
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
aiCorrectionCheck.addEventListener("change", saveSettings);
keepHistoryCheck.addEventListener("change", () => { saveSettings(); renderHistory(); });
historySearch.addEventListener("input", renderHistory);
historyClearBtn.addEventListener("click", async () => {
  if (!confirm("Effacer tout l'historique des dictées ? C'est définitif.")) return;
  try {
    await invoke("clear_history");
    historyEntries = [];
    renderHistory();
  } catch (e) {
    alert(`Impossible d'effacer l'historique : ${e}`);
  }
});
modeToggle.addEventListener("click", () => { setRecordingMode("toggle"); saveSettings(); });
modePtt.addEventListener("click", () => { setRecordingMode("push-to-talk"); saveSettings(); });
modelTurbo.addEventListener("click", () => { setWhisperModel("whisper-large-v3-turbo"); saveSettings(); });
snippetAddBtn.addEventListener("click", () => {
  currentSettings.snippets.push({ trigger: "", text: "" });
  renderSnippets();
  snippetList.querySelector<HTMLInputElement>(".snippet-card:last-child .snippet-trigger")?.focus();
});
modelLarge.addEventListener("click", () => { setWhisperModel("whisper-large-v3"); saveSettings(); });
vocabInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter" || e.key === ",") {
    e.preventDefault();
    addVocabWord(vocabInput.value);
    vocabInput.value = "";
  } else if (e.key === "Backspace" && vocabInput.value === "" && vocabWords.length > 0) {
    removeVocabWord(vocabWords[vocabWords.length - 1]);
  }
});
vocabInput.addEventListener("blur", () => {
  if (vocabInput.value.trim()) {
    addVocabWord(vocabInput.value);
    vocabInput.value = "";
  }
});
vocabChips.addEventListener("click", (e) => {
  if (e.target === vocabChips) vocabInput.focus();
});

// State events
listen<string>("recording-state", (event) => {
  const state = event.payload;
  statusDot.className = "";
  if (state === "Recording") {
    statusDot.classList.add("recording");
    statusText.textContent = "J'écoute…";
  } else if (state === "Transcribing") {
    statusDot.classList.add("transcribing");
    statusText.textContent = "Transcription…";
  } else {
    statusDot.classList.add("ready");
    statusText.textContent = "Prêt";
  }
});

listen<HistoryEntry>("history-added", (event) => {
  historyEntries.unshift(event.payload);
  historyEntries.length = Math.min(historyEntries.length, 100);
  renderHistory();
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

// ── Overlay controls ───────────────────────────────────
function setOverlayVisibleUI(visible: boolean) {
  overlayShowBtn.classList.toggle("active", visible);
  overlayHideBtn.classList.toggle("active", !visible);
}

overlayShowBtn.addEventListener("click", async () => {
  try {
    await invoke("set_overlay_visible", { visible: true });
    setOverlayVisibleUI(true);
  } catch (e) {
    console.error("set_overlay_visible(true) failed:", e);
  }
});

overlayHideBtn.addEventListener("click", async () => {
  try {
    await invoke("set_overlay_visible", { visible: false });
    setOverlayVisibleUI(false);
  } catch (e) {
    console.error("set_overlay_visible(false) failed:", e);
  }
});

overlayRepositionBtn.addEventListener("click", async () => {
  try {
    await invoke("reposition_overlay");
    setOverlayVisibleUI(true);
    overlayRepositionBtn.disabled = true;
    overlayRepositionBtn.textContent = "Mode déplacement (10s)…";
    setTimeout(() => {
      overlayRepositionBtn.disabled = false;
      overlayRepositionBtn.textContent = "Repositionner";
    }, 10000);
  } catch (e) {
    console.error("reposition_overlay failed:", e);
  }
});

quitAppBtn.addEventListener("click", () => {
  if (confirm("Quitter Brain Dump ? Le voyant et les hotkeys s'arrêteront.")) {
    invoke("quit_app");
  }
});

async function syncOverlayState() {
  try {
    const visible = await invoke<boolean>("get_overlay_visible");
    setOverlayVisibleUI(visible);
  } catch (e) {
    console.error("get_overlay_visible failed:", e);
  }
}

// Init
loadSettings().then(loadHistory);
refreshQuota();
syncOverlayState();
setInterval(refreshQuota, 30_000);
