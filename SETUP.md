# brain-dump — Setup local (Windows)

## État actuel

✅ **Supabase** — projet `brain-dump` créé en eu-west-1, table `notes` + RLS + index en place. Credentials dans `CREDENTIALS.md`.
✅ **Desktop app** — fork de typr dans `brain-dump/`, modifié pour :
- 2 hotkeys distincts : `Ctrl+Shift+Space` (silent) et `Ctrl+Shift+V` (paste)
- Langue configurable (défaut `fr`)
- Insert vers Supabase (non-bloquant : la transcription n'est jamais perdue)
- Capture contexte Windows (app + titre de fenêtre)
- Tracking quota Groq free tier (warning à 1500, block à 2000)
- UI settings étendue (Storage + Language + 2 hotkeys)

🟡 **À faire côté utilisateur** — voir plus bas.

## Ce qu'il te reste à faire

### 1. Clé Groq (si engine cloud)
- Récupère ta clé sur https://console.groq.com
- À renseigner dans l'app (section Engine → Groq API Key)

### 2. Build & installe l'app desktop

```powershell
cd brain-dump
npm install
npm run tauri build
```

Le binaire sera dans `brain-dump/src-tauri/target/release/`. Pour le dev :

```powershell
npm run tauri dev
```

### 3. Configure l'app au premier lancement

Ouvre la fenêtre, renseigne :
- **General → Language** : `Français`
- **Groq → API Key** : colle ta clé Groq (console.groq.com)
- **Storage → Supabase URL** : `https://dovlhxfezoyebnhpeskv.supabase.co`
- **Storage → Supabase Anon Key** : copie la publishable key depuis `CREDENTIALS.md`
- **Storage → Capturer le contexte** : coché

### 4. Test rapide
- Hotkey `Ctrl+Shift+Space` (silent) → parle 3 sec → re-hotkey pour stop
- Vérifie dans Supabase Studio : `SELECT * FROM notes ORDER BY created_at DESC LIMIT 1`
- Hotkey `Ctrl+Shift+V` → curseur dans un Notepad → parle → stop → le texte doit être pasté

### 5. n8n Telegram
Déjà fait par toi.

### 6. Notion + Cowork
- Crée les 3 bases Notion (voir `cowork-prompt.md` section bas)
- Copie-colle le prompt de `cowork-prompt.md` dans Claude Desktop → Cowork → Scheduled → New task, cadence Daily 22:00

## Fichiers

- [brain-dump/](brain-dump/) — code Tauri modifié
- [CREDENTIALS.md](CREDENTIALS.md) — Supabase URL + clés (non-committé)
- [cowork-prompt.md](cowork-prompt.md) — prompt de triage à coller dans Cowork
- [brain-dump-spec.md](brain-dump-spec.md) — spec originale

## Points de décision implémentés (différents de la spec)

- **skipped_unclear loop fix** : ajout colonne `triage_attempts`. Après 3 tentatives Claude lâche la note et te la signale dans le rapport.
- **Context Windows** : implémenté via `GetForegroundWindow` + `GetModuleBaseNameW` (crate `windows`). Retourne `{app, window, os}`.
- **Engine par défaut** : `cloud` (Groq free tier).
- **Quota Groq** : tracking local dans `%APPDATA%\com.brain-dump.app\groq_quota.json`. Warning popup à 1500, block dur à 2000. Reset auto à minuit UTC.
- **Logs** : préfixe `[brain-dump]` (ex-`[Typr]`).
