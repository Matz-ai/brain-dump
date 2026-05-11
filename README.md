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

### Installation pas-à-pas (Windows, débutant)

> Suis les étapes dans l'ordre. Aucune connaissance préalable nécessaire. Compte environ **30 min** au total (dont 5-20 min de compilation que l'ordi fait tout seul).

#### 1. Ouvrir PowerShell

Appuie sur la touche **Windows**, tape `powershell`, et lance **Windows PowerShell**. Une fenêtre noire/bleue s'ouvre — c'est ton terminal. Tout ce qui suit se tape là-dedans.

> Astuce : clic droit dans la fenêtre = coller (au lieu de Ctrl+V).

#### 2. Installer Git (si ce n'est pas déjà fait)

Tape la commande suivante, puis **Entrée** :

```powershell
winget install -e --id Git.Git --accept-source-agreements --accept-package-agreements
```

Quand c'est terminé, **ferme** la fenêtre PowerShell et **rouvre-en une nouvelle** (sinon `git` ne sera pas reconnu). Vérifie :

```powershell
git --version
```

Tu dois voir quelque chose comme `git version 2.x.x`.

#### 3. Cloner le repo

Place-toi dans le dossier où tu veux installer le projet (ici, `Documents`) puis clone :

```powershell
cd $HOME\Documents
git clone https://github.com/Matz-ai/brain-dump.git
cd brain-dump
```

#### 4. Lancer l'installeur tout-en-un

Toujours dans la même fenêtre PowerShell (et bien dans le dossier `brain-dump`) :

```powershell
.\INSTALL.bat
```

> **Si le script demande des droits administrateur** (popup UAC pour installer Visual Studio Build Tools), accepte. Sinon, ferme la fenêtre, **clic droit sur `INSTALL.bat` → Exécuter en tant qu'administrateur**.

Le script installe automatiquement :

- Node.js 20 LTS
- Rust + toolchain MSVC
- Visual Studio Build Tools 2022 (le plus long — 10-15 min)
- WebView2 Runtime
- Les dépendances npm
- Compile le binaire de l'application

Va boire un café. Quand c'est fini tu verras `Termine` en vert.

#### 5. Lancer l'application

Deux façons :

**A — Lancer le binaire compilé** (production, recommandé) :

Ouvre l'explorateur Windows et navigue jusqu'à :

```text
brain-dump\brain-dump\src-tauri\target\release\
```

Double-clic sur `Brain Dump.exe`.

> **Pro-tip — épingler pour ne plus jamais avoir à chercher** : une fois l'app lancée, clic droit sur son icône **dans la barre des tâches → *Épingler à la barre des tâches***. Ou alors clic droit directement sur `Brain Dump.exe` → ***Épingler au menu Démarrer***. Tu pourras la relancer en un clic depuis la barre des tâches ou la touche Windows.

**B — Lancer en mode développeur** (hot-reload, utile pour bidouiller) :

Dans PowerShell, depuis la racine `brain-dump\` :

```powershell
cd brain-dump
npm run tauri dev
```

> Oui, il y a bien deux fois `brain-dump` : la racine du repo s'appelle `brain-dump`, et elle contient un sous-dossier `brain-dump` (l'app Tauri elle-même).

#### 6. Configurer l'app

Une fois l'app ouverte, va dans **Settings**.

**Le seul truc obligatoire** pour que l'app marche, c'est ta **clé API Groq** (gratuite) :

- Crée un compte sur [console.groq.com](https://console.groq.com), génère une API key, et colle-la dans **Settings → Groq → API Key**.
- Mets aussi **General → Language** sur `Français`.

C'est tout. Tu peux déjà tester : presse `Ctrl+Shift+Space`, parle 3 secondes, re-presse `Ctrl+Shift+Space` pour stopper. Le texte transcrit est collé dans l'app active.

**Optionnel — pour avoir la persistance en base** (sinon les notes ne sont collées que dans l'app courante, rien n'est sauvegardé) :

- Crée un projet [Supabase](https://supabase.com) gratuit, exécute la SQL fournie dans [`brain-dump-spec.md`](brain-dump-spec.md) pour créer la table `notes`.
- Renseigne **Storage → Supabase URL** et **Anon Key** dans Settings.
- Utilise alors `Ctrl+Shift+V` (au lieu de `Ctrl+Shift+Space`) pour coller **et** sauvegarder en base.

Sans Supabase, l'app reste utile comme dictée vocale instantanée — c'est juste qu'aucune note n'est conservée pour le triage Cowork nocturne.

---

### Options de l'installeur

- `.\INSTALL.bat -SkipBuild` — prépare l'environnement mais ne compile pas (utile si tu veux build toi-même après).
- `.\INSTALL.bat -DevRun` — enchaîne directement sur `npm run tauri dev` au lieu du build release.

### Pré-requis qu'il faut quand même avoir

- Windows 10 ou 11 (récent, avec `winget` disponible — c'est par défaut sur Win11 et Win10 récent).
- Connexion internet.
- Une clé API Groq (gratuit). Supabase est **optionnel** mais nécessaire si tu veux conserver tes notes pour le triage automatique.

### Setup manuel (avancé)

Si tu préfères tout faire à la main, voir [`SETUP.md`](SETUP.md).

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
