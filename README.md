# brain-dump

Capture vocale → Supabase → Notion. Un "second cerveau" qui ne perd rien.

## Idée

Tu as une pensée → tu appuies sur une hotkey → tu parles → la transcription part en base de données. Le soir, un agent Claude Cowork lit la table, classe chaque note (task / idée / journal / noise) et écrit dans Notion. Toi tu te lèves le matin, tout est trié.

Deux entrées possibles :
- **Desktop (Windows)** : app Tauri avec hotkeys globales, Whisper via Groq free tier
- **Mobile (Telegram)** : bot n8n qui ingère les vocaux où que tu sois

Une seule sortie : table `notes` Supabase, triagée nuitamment vers tes bases Notion.

## Pourquoi

- Quasi-tous les outils de capture vocale soit te collent en presse-papier sans rien sauver, soit te poussent en cloud propriétaire. Aucun ne combine *capture instantanée + DB persistante + triage agentique*.
- Free tier strict : Groq Whisper (2000 req/jour), Supabase free, Notion free. Zero coût récurrent tant que tu restes solo.
- Windows uniquement à dessein : pas de cross-platform fluff.

## Architecture

```
┌─────────────────┐     ┌─────────────────┐
│  Desktop Tauri  │     │  Telegram bot   │
│  (Win, Rust+TS) │     │  (n8n workflow) │
└────────┬────────┘     └────────┬────────┘
         │                       │
         │  Whisper transcribe   │
         │  (Groq free tier)     │
         │                       │
         ▼                       ▼
       ┌──────────────────────────┐
       │   Supabase `notes` table │
       │   (Postgres + RLS)       │
       └──────────────┬───────────┘
                      │
                      │  Cowork scheduled task @22:00 daily
                      │  (Claude Desktop + Supabase MCP + Notion MCP)
                      ▼
       ┌──────────────────────────┐
       │  Notion : Tasks / Ideas  │
       │           / Journal      │
       └──────────────────────────┘
```

## Desktop app — features

- **2 hotkeys configurables** (clic + capture combinaison) :
  - `Paste only` (def. `Ctrl+Shift+Space`) — paste dans l'app active, **rien en DB** (info sensible)
  - `DB + Paste` (def. `Ctrl+Shift+V`) — paste **et** sauve en DB
- **Overlay flottant** (top-right de l'écran) : pill avec `Prêt / ● REC / Transcrit… / ✓ Collé / ✗ Échec`
- **Modèle Whisper** au choix : `large-v3-turbo` (rapide) ou `large-v3` (précis)
- **Vocabulaire custom** : textarea libre, envoyé comme `prompt` Whisper pour biaiser la transcription vers ton jargon
- **Quota Groq** trackée localement, warning à 75 %, blocage dur à 100 % (reset minuit UTC)
- **Capture contexte** Windows (app + titre fenêtre) joint à chaque note pour aider le triage
- **Insert Supabase non-bloquant** : si la DB est down, la transcription est quand même collée

## Stack

- **Desktop** : Tauri 2 + Rust + TypeScript vanilla. Fork de [`albertshiney/typr`](https://github.com/albertshiney/typr), modifié pour les besoins ci-dessus.
- **Audio** : `cpal`
- **Hotkeys** : `tauri-plugin-global-shortcut`
- **Whisper** : Groq API (`whisper-large-v3-turbo` / `whisper-large-v3`)
- **Storage** : Supabase Postgres + RLS open-anon (single-user)
- **Triage** : Claude Cowork scheduled task, MCPs Supabase + Notion
- **Mobile** : n8n self-hosted, workflow Telegram → Whisper → Supabase

## Setup

Voir [`SETUP.md`](SETUP.md) pour le pas-à-pas Windows (winget, Build Tools, Tauri build, premières config dans l'app).

Pré-requis :
- Windows 10/11
- Node 20+, Rust + MSVC Build Tools
- Compte Groq (free tier)
- Projet Supabase (la table est dans la spec)
- Workspace Notion + intégration MCP

## Triage Cowork

Une scheduled task Claude Cowork tourne quotidiennement (22h locale) avec accès Supabase MCP + Notion MCP. Pour chaque note non triée elle décide une action et écrit dans la base Notion correspondante.

6 actions possibles par note :
- `deleted_as_noise` — bruit, vide, doublon
- `created_task` — action concrète → base Tasks
- `created_idea` — idée nouvelle → base Ideas
- `appended_to_existing` — précision sur idée déjà tracée
- `created_journal` — réflexion abstraite → page du jour
- `skipped_unclear` — ambigu, retentative demain (max 3 puis flagged manuel)

(Le prompt exact n'est pas inclus dans le repo — adapte-le à ton workspace.)

## Spec d'origine

[`brain-dump-spec.md`](brain-dump-spec.md) — spec complète (~900 lignes) qui a servi de base. Pas tout est implémenté à la lettre : voir SETUP.md pour les écarts décidés en cours de route.

## Crédits

- [`albertshiney/typr`](https://github.com/albertshiney/typr) — base de la desktop app, MIT.
- Whisper — OpenAI, hébergé par Groq.

## Licence

MIT (héritée de typr).
