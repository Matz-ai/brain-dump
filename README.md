# brain-dump

Capture vocale → Supabase → Notion. Un "second cerveau" qui ne perd rien.

Tu appuies sur une hotkey, tu parles, la transcription part en base. Le soir, un agent Claude la trie dans Notion (Tasks / Ideas / Journal).

## Install

### Utilisateur final (Windows)

1. Télécharge le dernier `Brain Dump_x.y.z_x64-setup.exe` depuis [Releases](https://github.com/Matz-ai/brain-dump/releases)
2. Double-clic → installation → l'app apparaît dans le menu Démarrer
3. Lance Brain Dump → **Settings** → renseigne :
   - **Groq API Key** ([console.groq.com](https://console.groq.com), free tier 2000 req/j) — *obligatoire*
   - **Supabase URL** + **Anon Key** (table `notes`, schéma dans [`brain-dump-spec.md`](brain-dump-spec.md)) — *optionnel, pour la persistance*

**Hotkeys par défaut :**
- `Ctrl+Shift+Space` → transcrit + colle (pas de DB)
- `Ctrl+Shift+V` → transcrit + colle + sauve en DB

**Voyant flottant :** indicateur d'état click-through (Prêt / ● REC / Transcrit…). Géré depuis Settings → General.

**Mise à jour :** lance simplement le nouveau `.exe`, upgrade en place, ta config dans `%APPDATA%\com.brain-dump.app\` est préservée.

### Build from source (dev)

**Windows** (depuis le repo cloné) :
```powershell
.\INSTALL.bat
```
Le script installe winget, Node 20, Rust, Build Tools, WebView2, compile l'app. ~30 min.

**macOS** :
```bash
chmod +x install.sh && ./install.sh
```
Installe Homebrew, Node, Rust, compile. ~20 min.

**Build manuel** :
```bash
cd brain-dump && npm install && npm run tauri build
```
Produit `src-tauri/target/release/bundle/nsis/*.exe` (Win) ou `bundle/macos/*.app` (Mac).

Détails complets : [`SETUP.md`](SETUP.md).

## Architecture

```
Desktop Tauri ───┐                   ┌── Tasks
                 ├──► Supabase ──► Claude Cowork @22h ──┼── Ideas
Telegram bot ────┘   (notes table)    (Supabase + Notion MCPs)  └── Journal
```

## Stack

Tauri 2 + Rust + TypeScript · Groq Whisper · Supabase Postgres · Claude Cowork. Fork modifié de [`albertshiney/typr`](https://github.com/albertshiney/typr).

## Licence

MIT (héritée de typr).
