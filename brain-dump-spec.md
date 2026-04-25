# Brain-Dump — Spec technique V1

> Document de référence à donner à Claude Code pour bootstrapper le projet en une session.
> **Langue d'implémentation** : code en anglais, commentaires en français si utile.
> **Projet name** : `brain-dump` (renommable)

---

## 0. TL;DR (à lire en premier)

Un système de capture vocale dual (desktop + mobile) qui transcrit et stocke brut dans Supabase. Aucune intelligence à l'insertion. Un agent Claude passe chaque soir, lit les notes non-triées, les range dans Notion via MCP, et laisse la trace en DB.

**Principe directeur** : *ne construire que la capture*. L'intelligence est externe (Claude via MCPs) et reste remplaçable.

**Temps estimé d'implémentation** : 2 jours.

---

## 1. Contexte et philosophie

### 1.1 Problème résolu

L'utilisateur (mz, data engineer francophone) veut capturer rapidement des idées, réflexions, todos, remarques tech, sans friction. Taper casse le flow. L'outil doit maximiser le ratio [friction de capture] / [récupération utile].

### 1.2 Principes non-négociables

1. **Capture = dumb + fast**. Zéro intelligence à l'insertion. Un pipeline trivial qui dump du texte en DB en <3s.
2. **Intelligence = externe + évolutive**. Le tri, la synthèse, le routing vers Notion sont faits par Claude via MCPs, pas codés en dur. Les règles évoluent sans redéploiement.
3. **Stockage = source of truth rejouable**. Supabase garde tout brut ad vitam. Si les règles de tri changent, on rejoue sur l'historique.
4. **Pas d'embedding, pas de vector search, pas de classification à l'insertion**. Le volume ne le justifie pas (<5k notes attendues sur 6 mois).

### 1.3 Non-goals explicites

- ❌ Pas d'app mobile native (Telegram suffit)
- ❌ Pas de dashboard web (Notion est la vue humaine)
- ❌ Pas d'embeddings / pgvector / recherche sémantique
- ❌ Pas de classification auto à l'insertion
- ❌ Pas de tags à l'insertion
- ❌ Pas de stockage des fichiers audio (transcript only)
- ❌ Pas de système multi-utilisateur (single-user personnel)
- ❌ Pas de versioning / historique des éditions
- ❌ Pas de support Linux (macOS + Windows only, comme typr)

---

## 2. Architecture générale

```
┌─────────────────┐     ┌─────────────────┐
│  Desktop (Rust) │     │ Mobile (TG Bot) │
│  Tauri app      │     │  via n8n        │
│  2 hotkeys      │     │  Voice msg      │
└────────┬────────┘     └────────┬────────┘
         │                       │
         │  Groq Whisper         │
         │  large-v3-turbo       │
         ▼                       ▼
    ┌────────────────────────────────┐
    │  Supabase Postgres             │
    │  table: notes                  │
    │  (transcripts bruts)           │
    └───────────┬────────────────────┘
                │
                │  1 fois/jour 22h
                ▼
    ┌────────────────────────────────┐
    │  Claude Cowork scheduled task  │
    │  Lit triaged=false             │
    │  Range dans Notion via MCP     │
    │  Update triaged=true           │
    └───────────┬────────────────────┘
                │
                ▼
    ┌────────────────────────────────┐
    │  Notion                        │
    │  Bases: Ideas, Tasks, Journal  │
    │  Pages projets                 │
    └────────────────────────────────┘
```

### 2.1 Les 4 couches

| Couche | Responsabilité | Outil | Qui code |
|---|---|---|---|
| Capture desktop | Hotkey → transcription → insert DB | Tauri fork de typr | Claude Code |
| Capture mobile | Voice TG → transcription → insert DB | n8n | Utilisateur (pattern connu) |
| Stockage | Notes brutes persistantes | Supabase | Claude Code (setup SQL) |
| Intelligence | Tri quotidien + routing Notion | Claude Cowork + MCPs | Utilisateur (prompt) |

---

## 3. Stack actée (décisions prises, ne pas redébattre)

- **Desktop** : Tauri 2 + Rust + Vanilla TS (fork du projet `albertshiney/typr`)
- **Transcription desktop** : Groq `whisper-large-v3-turbo` (cloud par défaut) + whisper.cpp local (fallback optionnel, modèle `small`)
- **Mobile** : Telegram Bot + n8n cloud
- **Transcription mobile** : Groq `whisper-large-v3-turbo` uniquement
- **Langue transcription** : `fr` (hardcodé à patcher depuis typr qui était en `en`)
- **Storage** : Supabase free tier (Postgres)
- **Auth write vers Supabase** : anon key + RLS policy (single user, acceptable)
- **Intelligence** : Claude Cowork scheduled task quotidienne (utilisateur a Claude Pro)
- **Destination post-tri** : Notion via MCP connector

---

## 4. Scope MVP : inclus / exclus

### 4.1 Inclus dans le MVP (à livrer)

- [ ] Fork de typr avec :
  - [ ] Langue configurable (défaut `fr`)
  - [ ] 2 hotkeys distincts (silent / inline)
  - [ ] POST HTTP vers Supabase après transcription
  - [ ] Settings étendues (Supabase URL + key)
  - [ ] Capture du contexte (app active) optionnelle
- [ ] Workflow n8n Telegram (voice → Groq → Supabase)
- [ ] Schéma Supabase + RLS policy
- [ ] Prompt de tri quotidien calibré (à coller dans Cowork)
- [ ] Documentation setup (clés API, Notion bases à créer)

### 4.2 Explicitement reporté (pas dans le MVP)

- Mode "needs_review" pour notes ambiguës → **ajouter après 2 semaines d'usage**
- Détection de doublons → **si le problème apparaît**
- Digest hebdo tech news → **scheduled task séparée à ajouter plus tard**
- Export Obsidian → **si besoin se fait sentir**

---

## 5. Supabase : setup complet

### 5.1 Projet

Créer un nouveau projet Supabase (free tier suffit). Région EU (Paris ou Francfort, latence).

### 5.2 Schéma (à exécuter dans SQL Editor)

```sql
-- Table principale : stockage brut des notes
create table notes (
  id uuid primary key default gen_random_uuid(),
  
  -- métadonnées de capture
  source text not null check (source in ('desktop_note', 'desktop_inline', 'telegram')),
  transcript text not null,
  context jsonb default '{}'::jsonb,  -- {"app": "Cursor", "window": "main.rs", "url": null}
  created_at timestamptz not null default now(),
  
  -- métadonnées de triage (remplies par l'agent Claude)
  triaged boolean not null default false,
  triaged_at timestamptz,
  notion_page_id text,
  notion_database text,
  triage_action text  -- "created_page", "appended_to_existing", "deleted_as_noise", "skipped_unclear"
);

-- Index pour requêtes fréquentes
create index notes_triaged_idx on notes (triaged, created_at desc);
create index notes_created_at_idx on notes (created_at desc);
create index notes_source_idx on notes (source);

-- RLS activé
alter table notes enable row level security;

-- Policy : insert libre depuis anon (single-user personal, acceptable)
-- L'anon key reste dans le binaire Tauri et le workflow n8n, tous deux contrôlés par l'utilisateur
create policy "allow_anon_insert" on notes
  for insert to anon
  with check (true);

-- Policy : select depuis anon (pour que l'utilisateur puisse lire en debug via Supabase Studio)
create policy "allow_anon_select" on notes
  for select to anon
  using (true);

-- Policy : update depuis anon (pour que l'agent Claude puisse marquer triaged=true via MCP)
create policy "allow_anon_update" on notes
  for update to anon
  using (true);
```

**Note sécurité** : l'approche single-user avec anon key + RLS ouvert est acceptable *si et seulement si* l'URL et la clé anon restent privées (pas de commit sur GitHub public). Ajouter `.env` au `.gitignore` systématiquement.

### 5.3 Vérification

```sql
-- Test insert
insert into notes (source, transcript) 
values ('desktop_note', 'test de capture');

-- Verify
select * from notes order by created_at desc limit 1;

-- Cleanup
delete from notes where transcript = 'test de capture';
```

### 5.4 Récupérer les credentials

Dans le projet Supabase → Settings → API :
- **Project URL** : `https://xxxxx.supabase.co`
- **anon/public key** : `eyJhbGciOi...` (starts with eyJ)

Ces deux valeurs sont à mettre dans les settings de l'app Tauri et dans n8n.

---

## 6. Capture desktop : fork de typr

### 6.1 Point de départ

Fork du repo : `https://github.com/albertshiney/typr`

Structure du projet typr déjà connue :
```
typr/
├── src/                    # Frontend TS
│   ├── main.ts             # UI settings
│   ├── overlay.html        # Icône mic flottant
│   └── style.css
├── src-tauri/
│   ├── src/
│   │   ├── main.rs         # Entry point, hotkeys, overlay
│   │   ├── audio.rs        # Capture cpal
│   │   ├── recorder.rs     # Orchestration record → transcribe → paste
│   │   ├── transcribe_groq.rs
│   │   ├── transcribe_local.rs
│   │   ├── settings.rs     # Persistence JSON
│   │   ├── paste.rs        # Clipboard + Cmd+V / Ctrl+V
│   │   ├── cleanup.rs      # Capitalisation basique
│   │   └── downloader.rs   # Download des modèles Whisper
│   ├── Cargo.toml
│   └── tauri.conf.json
└── package.json
```

### 6.2 Modifications requises (par fichier)

#### 6.2.1 `src-tauri/src/transcribe_groq.rs`

**Changement 1** : la langue est hardcodée `en`, rendre configurable.

Remplacer :
```rust
.text("language", "en")
```
Par :
```rust
.text("language", language)  // où language est un paramètre de la fonction
```

Modifier la signature :
```rust
pub async fn transcribe_groq(
    api_key: &str, 
    audio_path: &PathBuf,
    language: &str,  // <- ajouté
) -> Result<String, String>
```

#### 6.2.2 `src-tauri/src/transcribe_local.rs`

Même changement. Remplacer `"-l", "en"` par `"-l", language` avec un paramètre `language: &str` dans la signature.

#### 6.2.3 `src-tauri/src/settings.rs`

Étendre la struct `Settings` :

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub microphone: String,
    pub engine: String,
    #[serde(rename = "whisperModel")]
    pub whisper_model: String,
    #[serde(rename = "groqApiKey")]
    pub groq_api_key: String,
    #[serde(rename = "recordingMode")]
    pub recording_mode: String,
    
    // Existant (renommé pour clarté)
    #[serde(rename = "hotkeyNote")]
    pub hotkey_note: String,        // ex: "CmdOrCtrl+Shift+Space" (silent)
    
    // Nouveaux champs
    #[serde(rename = "hotkeyInline")]
    pub hotkey_inline: String,      // ex: "CmdOrCtrl+Shift+V" (paste inline)
    
    pub language: String,           // "fr", "en", "auto"
    
    #[serde(rename = "supabaseUrl")]
    pub supabase_url: String,       // https://xxxxx.supabase.co
    
    #[serde(rename = "supabaseAnonKey")]
    pub supabase_anon_key: String,  // eyJ...
    
    #[serde(rename = "captureContext")]
    pub capture_context: bool,      // true = capte app active
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            microphone: "default".to_string(),
            engine: "cloud".to_string(),  // défaut Groq
            whisper_model: "small".to_string(),
            groq_api_key: String::new(),
            recording_mode: "toggle".to_string(),
            hotkey_note: "CmdOrCtrl+Shift+Space".to_string(),
            hotkey_inline: "CmdOrCtrl+Shift+V".to_string(),
            language: "fr".to_string(),
            supabase_url: String::new(),
            supabase_anon_key: String::new(),
            capture_context: true,
        }
    }
}
```

#### 6.2.4 Nouveau fichier : `src-tauri/src/supabase.rs`

```rust
use reqwest::Client;
use serde_json::json;
use std::path::PathBuf;

pub async fn insert_note(
    supabase_url: &str,
    anon_key: &str,
    transcript: &str,
    source: &str,  // "desktop_note" ou "desktop_inline"
    context: Option<serde_json::Value>,
) -> Result<(), String> {
    if supabase_url.is_empty() || anon_key.is_empty() {
        return Err("Supabase not configured".to_string());
    }

    let url = format!("{}/rest/v1/notes", supabase_url);
    let body = json!({
        "source": source,
        "transcript": transcript,
        "context": context.unwrap_or(json!({}))
    });

    let client = Client::new();
    let response = client
        .post(&url)
        .header("apikey", anon_key)
        .header("Authorization", format!("Bearer {}", anon_key))
        .header("Content-Type", "application/json")
        .header("Prefer", "return=minimal")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Supabase request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let err_body = response.text().await.unwrap_or_default();
        return Err(format!("Supabase error ({}): {}", status, err_body));
    }

    println!("[brain-dump] Note inserted in Supabase");
    Ok(())
}
```

Ne pas oublier de l'ajouter à `lib.rs` ou équivalent :
```rust
pub mod supabase;
```

Et dans `Cargo.toml`, vérifier que `reqwest` a bien les features `json` et `multipart` (déjà présent dans typr).

#### 6.2.5 Nouveau fichier : `src-tauri/src/context.rs`

Capture l'app active pour enrichir la note. Facultatif mais recommandé.

```rust
use serde_json::{json, Value};

#[cfg(target_os = "macos")]
pub fn capture_active_context() -> Value {
    use std::process::Command;
    
    let output = Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events" to get name of first application process whose frontmost is true"#
        ])
        .output()
        .ok();
    
    let app_name = output
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    
    json!({
        "app": app_name,
        "os": "macos",
    })
}

#[cfg(target_os = "windows")]
pub fn capture_active_context() -> Value {
    // Implementation basique Windows via GetForegroundWindow + GetWindowTextW
    // Pour le MVP on peut retourner un contexte vide
    json!({
        "os": "windows",
    })
}
```

#### 6.2.6 `src-tauri/src/recorder.rs` — la grosse modif

Remplacer la méthode `stop_and_transcribe` pour qu'elle :
1. Transcrive comme avant
2. POST vers Supabase (toujours)
3. Paste uniquement si le mode est `inline`

```rust
pub async fn stop_and_transcribe(
    &self,
    app: &AppHandle,
    settings: &Settings,
    app_dir: &PathBuf,
    mode: TranscribeMode,  // <- NOUVEAU param: Note ou Inline
) -> Result<String, String> {
    // ... code existant pour stop recording et save WAV ...

    // Transcription avec la langue des settings
    let raw_text = match settings.engine.as_str() {
        "local" => {
            let model_path = app_dir.join(transcribe_local::model_filename(&settings.whisper_model));
            transcribe_local::transcribe_local(
                app, 
                &model_path, 
                &temp_path, 
                &settings.language  // <- passer la langue
            ).await?
        }
        "cloud" => {
            transcribe_groq::transcribe_groq(
                &settings.groq_api_key, 
                &temp_path,
                &settings.language  // <- passer la langue
            ).await?
        }
        _ => return Err(format!("Unknown engine: {}", settings.engine)),
    };

    // Cleanup temp file
    let _ = std::fs::remove_file(&temp_path);

    // Cleanup text
    let cleaned = cleanup_text(&raw_text);

    if cleaned.is_empty() {
        return Ok(String::new());
    }

    // Capture context
    let context = if settings.capture_context {
        Some(crate::context::capture_active_context())
    } else {
        None
    };

    // Insert Supabase (TOUJOURS, les deux modes)
    let source = match mode {
        TranscribeMode::Note => "desktop_note",
        TranscribeMode::Inline => "desktop_inline",
    };
    
    // Non-blocking: on log mais on fait pas échouer si Supabase down
    if let Err(e) = crate::supabase::insert_note(
        &settings.supabase_url,
        &settings.supabase_anon_key,
        &cleaned,
        source,
        context,
    ).await {
        eprintln!("[brain-dump] Supabase insert failed: {}", e);
        // On continue quand même pour le paste
    }

    // Paste uniquement en mode inline
    if matches!(mode, TranscribeMode::Inline) {
        paste_text(&cleaned)?;
    }

    // Reset state ...
    
    Ok(cleaned)
}

#[derive(Debug, Clone, Copy)]
pub enum TranscribeMode {
    Note,    // silent: DB only
    Inline,  // paste: DB + paste dans app active
}
```

#### 6.2.7 `src-tauri/src/main.rs` — enregistrement des 2 hotkeys

Remplacer l'enregistrement d'UN seul hotkey par DEUX :

```rust
// Hotkey "note" (silent)
app.global_shortcut().on_shortcut(
    settings.hotkey_note.as_str(),
    move |_app, shortcut, event| {
        // ... logique existante, mais passe mode=TranscribeMode::Note
    },
)?;

// Hotkey "inline" (paste)
app.global_shortcut().on_shortcut(
    settings.hotkey_inline.as_str(),
    move |_app, shortcut, event| {
        // ... logique existante, mais passe mode=TranscribeMode::Inline
    },
)?;
```

La fonction `do_toggle_recording` doit prendre un paramètre `mode: TranscribeMode` et le passer à `stop_and_transcribe`.

#### 6.2.8 Frontend : `src/main.ts`

Étendre l'interface `Settings` TypeScript pour matcher la struct Rust, et ajouter les champs UI pour :
- Langue (select fr/en/auto)
- Hotkey note + hotkey inline
- Supabase URL + anon key (password input)
- Capture context (checkbox)

### 6.3 Build et signature

- `bun install` (ou `npm install`)
- `bun run tauri dev` pour tester
- `bun run tauri build` pour produire le binaire

Signature/notarization macOS : hors MVP. L'utilisateur acceptera le warning Gatekeeper au premier lancement (clic droit → Ouvrir).

---

## 7. Capture mobile : workflow n8n Telegram

### 7.1 Prérequis

- Bot Telegram créé via @BotFather → récupérer le TOKEN
- Supabase URL + anon key (déjà en possession)
- Groq API key (déjà en possession)

### 7.2 Workflow n8n (à construire dans n8n cloud)

**Nœud 1 — Telegram Trigger**
- Type : `Telegram Trigger`
- Updates : `message`
- Filtre : seulement voice messages
- Condition : `$json.message.voice !== undefined`

**Nœud 2 — Download file**
- Type : `Telegram` → action `Get File`
- File ID : `{{ $json.message.voice.file_id }}`
- Output : contenu binaire OGG

**Nœud 3 — HTTP Request (Groq Whisper)**
- Méthode : POST
- URL : `https://api.groq.com/openai/v1/audio/transcriptions`
- Headers :
  - `Authorization: Bearer {{ $credentials.groqApiKey }}`
- Body : `multipart-form-data`
  - `file` : binary data du nœud 2
  - `model` : `whisper-large-v3-turbo`
  - `language` : `fr`
  - `response_format` : `json`

**Nœud 4 — HTTP Request (Supabase insert)**
- Méthode : POST
- URL : `{{ $env.SUPABASE_URL }}/rest/v1/notes`
- Headers :
  - `apikey` : anon key
  - `Authorization` : `Bearer <anon key>`
  - `Content-Type` : `application/json`
  - `Prefer` : `return=minimal`
- Body :
```json
{
  "source": "telegram",
  "transcript": "{{ $node['Groq'].json.text }}",
  "context": {
    "chat_id": "{{ $node['Trigger'].json.message.chat.id }}",
    "duration": {{ $node['Trigger'].json.message.voice.duration }}
  }
}
```

**Nœud 5 — Telegram Send Message (confirmation)**
- Chat ID : `{{ $node['Trigger'].json.message.chat.id }}`
- Text : `✓ Noté ({{ $node['Groq'].json.text.length }} chars)`

**Error handling** : en cas d'échec Groq ou Supabase, envoyer un message Telegram `⚠️ Erreur : <message>` pour que l'utilisateur ne perde pas la note (il peut la re-dicter).

### 7.3 Test

Envoyer un voice message au bot → vérifier dans Supabase Studio que la row apparaît avec `source='telegram'`.

---

## 8. Intelligence : scheduled task Claude Cowork

### 8.1 Prérequis utilisateur (à faire dans Notion)

Créer au minimum ces bases dans Notion :

- **`Ideas`** — base de données (not page), propriétés :
  - `Name` (title)
  - `Category` (select : projet_perso, projet_datanalyse, reflexion, autre)
  - `Project` (select, libre)
  - `Created` (date)
  - `Source note` (URL, optionnel)

- **`Tasks`** — base de données, propriétés :
  - `Name` (title)
  - `Status` (select : todo, doing, done)
  - `Project` (select, libre)
  - `Priority` (select : low, med, high)
  - `Due` (date, optionnel)

- **`Journal`** — base de données, propriétés :
  - `Name` (title, ex: "2026-04-25 — Réflexions")
  - `Date` (date)
  - `Theme` (select, libre)

Connecter Notion MCP à Claude (déjà fait chez l'utilisateur).

### 8.2 Créer le scheduled task

Dans Claude Desktop → Cowork sidebar → Scheduled → New task.

**Cadence** : Daily à 22:00 (fuseau local).

**Prompt** (à coller tel quel dans le champ instructions) :

```
Tu es l'agent de triage quotidien du système brain-dump de mz.

CONTEXTE :
mz est data engineer francophone, ex-poker pro, intérêts : IA/ML, Rust, 
Tauri, n8n, Supabase, LoL analytics, consulting chez Datanalyse (clients 
GREF, SADE), projets perso (typr, draft CFR LoL, multi-agent Undercover, 
prospecting tool).

INPUTS :
Via Supabase MCP, query :
  SELECT id, source, transcript, context, created_at 
  FROM notes 
  WHERE triaged = false 
  ORDER BY created_at ASC

TON BOULOT :

Pour CHAQUE note non-triée :

1. LIS le transcript.

2. DÉCIDE l'action :
   
   A) "deleted_as_noise" : note vide, phrase test, déclenchement raté, 
      bruit de fond, répétition évidente d'une note précédente.
      → Ne rien créer dans Notion. Update Supabase : 
         triaged=true, triage_action='deleted_as_noise'.
   
   B) "created_task" : action concrète à faire.
      Signaux : "il faut que je", "je dois", "penser à", "ne pas oublier".
      → Créer une row dans base Notion "Tasks" avec :
         - Name : reformulation concise du todo
         - Status : todo
         - Project : inféré depuis contexte/transcript
         - Priority : med par défaut (high si "urgent", "vite", "critique")
      → Update Supabase avec triage_action='created_task', 
         notion_page_id, notion_database='Tasks'.
   
   C) "created_idea" : idée nouvelle de projet, feature, approche.
      Signaux : "et si", "ça serait cool", "on pourrait", "imagine que".
      → Créer une row dans base Notion "Ideas" avec :
         - Name : titre court de l'idée
         - Category : projet_perso | projet_datanalyse | reflexion | autre
         - Project : nom du projet si identifiable ("typr", "cfr-draft", 
           "undercover", "prospecting", client GREF/SADE, etc.)
      → Update Supabase.
   
   D) "appended_to_existing" : continuation/précision d'une idée existante.
      → Rechercher dans Ideas/Tasks une page liée (même projet, même tag).
      → Append le transcript comme bloc à la page trouvée 
         (avec date + heure).
      → Update Supabase avec l'ID de la page existante.
   
   E) "created_journal" : réflexion abstraite, analyse, méta-pensée 
      qui n'est ni task ni idée produit.
      → Ajouter bloc dans la base "Journal" du jour 
         (créer la page du jour si elle n'existe pas, format 
         "YYYY-MM-DD — Réflexions").
      → Grouper par thème (Theme property).
   
   F) "skipped_unclear" : tu n'arrives pas à décider avec >70% confiance.
      → Ne rien créer dans Notion.
      → Update Supabase avec triage_action='skipped_unclear', 
         triaged=false (donc retentatif demain ou review manuelle).

3. UPDATE Supabase pour chaque note traitée (sauf skipped_unclear qui reste 
   triaged=false).

RAPPORT FINAL :

Après avoir tout traité, envoie-moi un résumé via la MCP Telegram 
(ou simplement en réponse ici si pas de MCP Telegram) :

📊 Triage du {date} à 22h
━━━━━━━━━━━━━━━━━━━━━━
✓ N notes traitées
🗑  X supprimées comme noise
✅ Y tasks créées
💡 Z idées créées  
📔 W entrées journal
⏭  V notes skippées (à revoir)

Top 3 nouvelles idées projets :
• [titre idée 1]
• [titre idée 2]
• [titre idée 3]

Todos high priority :
• [todo 1]

Notes skippées (action needed) :
• "[début transcript]..." → à clarifier demain

CONTRAINTES :

- Respecte les 4 catégories exactes : projet_perso, projet_datanalyse, 
  reflexion, autre. Pas d'invention.
- Si tu crées une Ideas row, essaie MAX de réutiliser un Project existant 
  avant d'en créer un nouveau.
- Transcripts en français, reformulations en français.
- Ne supprime JAMAIS une note de Supabase, même si action=noise. 
  Mark triaged=true suffit.
- Si Notion MCP fail sur une note, skip cette note (triaged=false) 
  et continue les autres.
```

### 8.3 Test initial

Après setup, laisser s'accumuler 10-15 notes sur 2 jours, puis déclencher manuellement le task une première fois. Vérifier :
- Les notes sont bien bougées dans Notion
- Supabase marque triaged=true
- Le rapport final est clair

Si la classification est trop agressive (beaucoup de noise) ou trop timide (beaucoup de skipped_unclear) → tuner le prompt.

---

## 9. Configuration utilisateur : checklist d'onboarding

### 9.1 Clés et comptes à avoir

- [ ] Compte Supabase créé, projet créé, URL et anon key récupérés
- [ ] Clé API Groq récupérée (console.groq.com)
- [ ] Bot Telegram créé via @BotFather, token récupéré
- [ ] Compte n8n cloud (ou self-hosted) actif
- [ ] Claude Pro actif (scheduled tasks disponibles)
- [ ] Notion MCP connecté à Claude (déjà fait)
- [ ] Supabase MCP connecté à Claude (déjà fait)

### 9.2 Bases Notion prérequises

- [ ] Base `Ideas` créée avec les bonnes properties
- [ ] Base `Tasks` créée
- [ ] Base `Journal` créée
- [ ] IDs/URLs de ces bases notés (pour que l'agent Claude sache où les trouver)

### 9.3 Installation desktop

- [ ] Fork du repo typr
- [ ] Appliquer les modifications décrites section 6
- [ ] Build local (`bun run tauri build`)
- [ ] Installer le binaire
- [ ] Premier lancement : settings → remplir Supabase URL + anon key + Groq key + langue fr
- [ ] Tester les 2 hotkeys (vérifier que la row apparaît dans Supabase pour chacun)

### 9.4 Installation mobile

- [ ] Workflow n8n créé selon section 7.2
- [ ] Webhook Telegram activé
- [ ] Test : envoi d'un voice message au bot → vérif row dans Supabase

### 9.5 Agent de tri

- [ ] Task Cowork créé avec le prompt section 8.2
- [ ] Cadence : daily 22h
- [ ] Run manuel initial pour test

---

## 10. Testing et critères de succès

### 10.1 Tests unitaires

- `transcribe_groq` accepte bien le paramètre language (passer "fr" et vérifier que l'API est appelée avec)
- `supabase::insert_note` : test avec Supabase URL/key invalides → erreur propre
- Les 2 hotkeys : simuler trigger, vérifier que le bon mode est passé

### 10.2 Tests d'intégration

Scénario A — capture desktop silent :
1. Hotkey "note" pressé
2. Parler 5s
3. Hotkey pressé à nouveau
4. Vérif : row dans Supabase avec source='desktop_note'
5. Vérif : rien n'a été pasté dans l'app active

Scénario B — capture desktop inline :
1. Curseur dans une TextEdit / Notepad
2. Hotkey "inline" pressé
3. Parler 5s
4. Hotkey pressé à nouveau
5. Vérif : row dans Supabase avec source='desktop_inline'
6. Vérif : texte transcrit a été pasté dans TextEdit

Scénario C — capture mobile :
1. Voice message Telegram 3s
2. Vérif : row dans Supabase avec source='telegram'
3. Vérif : confirmation reçue dans Telegram

Scénario D — triage quotidien :
1. 5 notes de types variés dans la DB
2. Trigger manuel du Cowork task
3. Vérif : toutes sont `triaged=true` (ou skipped_unclear)
4. Vérif : Notion contient les pages correspondantes
5. Vérif : le rapport Telegram arrive

### 10.3 Critère de succès MVP

Au bout d'une semaine d'usage réel :
- >20 notes capturées
- <5% de notes perdues (échec pipeline)
- >70% des notes correctement rangées sans review manuelle
- L'utilisateur consulte Notion au moins 3×/semaine pour retrouver des idées

---

## 11. Roadmap post-MVP (pour référence, PAS à implémenter)

- **Phase 2 — affinage** (après 2 semaines d'usage)
  - Ajouter mode `needs_review` pour notes ambiguës
  - Affiner le prompt de triage avec patterns observés
  - Ajouter détection de doublons simple via ILIKE

- **Phase 3 — intelligence supplémentaire** (si l'usage confirme l'intérêt)
  - Scheduled task hebdo dimanche 20h : digest tech news + croisement avec notes
  - Scheduled task mensuelle : synthèse des idées non-explorées
  - Notifications Telegram sur relances ("tu parlais de X il y a 2 semaines, action ?")

- **Phase 4 — si vraiment besoin** (volume >5k notes, recherche structurée insuffisante)
  - Ajouter pgvector + embeddings (nomic-embed-text via Ollama local)
  - Recherche sémantique exposée via slash-command

---

## 12. Variables d'environnement attendues

### 12.1 Desktop (Tauri settings JSON)

Stockées dans `~/Library/Application Support/com.brain-dump.app/config.json` (macOS) ou `%APPDATA%\com.brain-dump.app\config.json` (Windows) :

```json
{
  "microphone": "default",
  "engine": "cloud",
  "whisperModel": "small",
  "groqApiKey": "gsk_...",
  "recordingMode": "toggle",
  "hotkeyNote": "CmdOrCtrl+Shift+Space",
  "hotkeyInline": "CmdOrCtrl+Shift+V",
  "language": "fr",
  "supabaseUrl": "https://xxxxx.supabase.co",
  "supabaseAnonKey": "eyJ...",
  "captureContext": true
}
```

### 12.2 n8n environment

```
SUPABASE_URL=https://xxxxx.supabase.co
SUPABASE_ANON_KEY=eyJ...
GROQ_API_KEY=gsk_...
TELEGRAM_BOT_TOKEN=1234...
```

---

## 13. Notes pour Claude Code

Quand tu implémentes :

1. **Commence par fork typr proprement** et assure-toi qu'il build before tout changement.
2. **Applique les changements fichier par fichier** dans l'ordre de la section 6, en testant le build après chaque.
3. **Les tests unitaires existants de typr** doivent continuer à passer après tes modifs.
4. **N'ajoute pas de dependencies Cargo inutiles** — `reqwest` et `serde_json` sont déjà là.
5. **Ne touche pas à la logique whisper.cpp sidecar** — elle marche, il ne faut juste pas oublier de lui passer `-l fr`.
6. **Le contexte capture (`context.rs`)** est optionnel. Si ça te prend >30 min d'implémenter le Windows, skip et retourne `json!({"os": "windows"})` basique.
7. **Respecte le principe de non-blocage** : si Supabase est down, on log et on continue. Le pipeline ne doit JAMAIS perdre une transcription à cause d'un échec réseau secondaire.
8. **Pour la UI settings frontend**, reste minimaliste : les champs en texte brut, pas de UX sophistiquée. L'utilisateur éditera le JSON si besoin.

Tu peux poser des questions si un point est ambigu, mais évite de redébattre les décisions actées en section 3.

---

*Fin de spec. Total : 13 sections, ~500 lignes.*
*Version : 1.0 — 2026-04-25*
