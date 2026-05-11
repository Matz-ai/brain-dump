# =============================================================
# brain-dump - Installeur Windows (one-shot)
# =============================================================
# Verifie les prerequis, installe ce qui manque,
# fait npm install, compile le binaire Tauri.
#
# Usage (clic droit -> Executer avec PowerShell, ou) :
#   powershell -ExecutionPolicy Bypass -File .\INSTALL.ps1
#
# Options :
#   -SkipBuild   : ne lance pas `npm run tauri build` a la fin
#   -DevRun      : finit par `npm run tauri dev` au lieu du build
# =============================================================

[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [switch]$DevRun
)

# Important : PAS de "Stop" global, sinon les outils natifs qui ecrivent
# sur stderr (rustup, winget...) font crasher le script.
$ErrorActionPreference = 'Continue'
$ProgressPreference    = 'SilentlyContinue'

function Write-Step($msg)  { Write-Host ""; Write-Host "==> $msg" -ForegroundColor Cyan }
function Write-Ok($msg)    { Write-Host "    OK  - $msg" -ForegroundColor Green }
function Write-Warn2($msg) { Write-Host "    !!  - $msg" -ForegroundColor Yellow }
function Write-Err2($msg)  { Write-Host "    XX  - $msg" -ForegroundColor Red }

function Test-Cmd($name) {
    return [bool](Get-Command $name -ErrorAction SilentlyContinue)
}

# Lance une commande native, ignore le stderr "non-fatal", renvoie l'ExitCode.
# La sortie + stderr est imprimee mais ne fait PAS planter le script.
function Invoke-Native {
    param(
        [Parameter(Mandatory)] [string] $File,
        [string[]] $Arguments = @(),
        [switch] $Quiet
    )
    if ($Quiet) {
        & $File @Arguments 2>&1 | Out-Null
    } else {
        & $File @Arguments 2>&1 | ForEach-Object { Write-Host "    $_" }
    }
    return $LASTEXITCODE
}

# Reconstruit $env:Path depuis le registre (apres install d'un outil)
# + ajoute les dossiers classiques que les installers Windows oublient parfois.
function Refresh-Path {
    $machine = [System.Environment]::GetEnvironmentVariable('Path','Machine')
    $user    = [System.Environment]::GetEnvironmentVariable('Path','User')
    $extras = @(
        "$env:USERPROFILE\.cargo\bin",
        "$env:USERPROFILE\AppData\Local\Programs\Rustup",
        "$env:ProgramFiles\nodejs",
        "$env:ProgramFiles\Git\cmd"
    )
    $env:Path = ($machine, $user) + $extras -join ';'
}

# ------------------------------------------------------------
# 0. Contexte
# ------------------------------------------------------------
$scriptDir   = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectDir  = Join-Path $scriptDir 'brain-dump'

if (-not (Test-Path $projectDir)) {
    Write-Err2 "Dossier introuvable : $projectDir"
    Write-Err2 "Ce script doit etre dans le dossier racine du repo (a cote de README.md, SETUP.md)."
    exit 1
}

Write-Step "brain-dump installer"
Write-Host "    Repo racine : $scriptDir"
Write-Host "    App Tauri   : $projectDir"

Refresh-Path

# ------------------------------------------------------------
# 1. winget
# ------------------------------------------------------------
Write-Step "Verification de winget"
if (Test-Cmd 'winget') {
    Write-Ok "winget present"
} else {
    Write-Err2 "winget introuvable. Mets a jour 'App Installer' depuis le Microsoft Store puis relance."
    exit 1
}

# ------------------------------------------------------------
# 2. Node.js (>= 20)
# ------------------------------------------------------------
Write-Step "Verification de Node.js (>= 20)"
$needNode = $true
if (Test-Cmd 'node') {
    $nv = (& node --version) -replace 'v',''
    $major = [int]($nv.Split('.')[0])
    if ($major -ge 20) {
        Write-Ok "Node $nv"
        $needNode = $false
    } else {
        Write-Warn2 "Node $nv trop ancien, upgrade vers LTS"
    }
}
if ($needNode) {
    Write-Host "    Installation Node.js LTS via winget..."
    Invoke-Native -File winget -Arguments @('install','-e','--id','OpenJS.NodeJS.LTS',
        '--accept-source-agreements','--accept-package-agreements','--silent') -Quiet | Out-Null
    Refresh-Path
    if (-not (Test-Cmd 'node')) {
        Write-Err2 "Node n'est toujours pas dans le PATH. Ferme/rouvre PowerShell et relance le script."
        exit 1
    }
    Write-Ok "Node installe : $(node --version)"
}

# ------------------------------------------------------------
# 3. Rust + cargo (toolchain MSVC)
# ------------------------------------------------------------
Write-Step "Verification de Rust"

# Si rustup deja la mais cargo pas dans PATH : juste refresh PATH
if ((Test-Cmd 'rustup') -and -not (Test-Cmd 'cargo')) {
    Refresh-Path
}

if ((Test-Cmd 'cargo') -and (Test-Cmd 'rustc')) {
    $rv = (& rustc --version) 2>&1 | Select-Object -First 1
    Write-Ok "Rust deja installe : $rv"
} elseif (Test-Cmd 'rustup') {
    Write-Host "    Rustup detecte, on s'assure de la toolchain stable MSVC..."
    Invoke-Native -File rustup -Arguments @('default','stable-x86_64-pc-windows-msvc') -Quiet | Out-Null
    Invoke-Native -File rustup -Arguments @('update','stable') -Quiet | Out-Null
    Refresh-Path
    if ((Test-Cmd 'cargo') -and (Test-Cmd 'rustc')) {
        $rv = (& rustc --version) 2>&1 | Select-Object -First 1
        Write-Ok "Rust pret : $rv"
    } else {
        Write-Err2 "rustup est la mais cargo introuvable. Ferme/rouvre PowerShell et relance."
        exit 1
    }
} else {
    Write-Host "    Installation Rustup via winget..."
    Invoke-Native -File winget -Arguments @('install','-e','--id','Rustlang.Rustup',
        '--accept-source-agreements','--accept-package-agreements','--silent') -Quiet | Out-Null
    Refresh-Path
    if (Test-Cmd 'rustup') {
        Invoke-Native -File rustup -Arguments @('default','stable-x86_64-pc-windows-msvc') -Quiet | Out-Null
        Refresh-Path
    }
    if ((Test-Cmd 'cargo') -and (Test-Cmd 'rustc')) {
        Write-Ok "Rust installe : $(rustc --version)"
    } else {
        Write-Err2 "cargo introuvable apres install. Ferme/rouvre PowerShell et relance le script."
        exit 1
    }
}

# ------------------------------------------------------------
# 4. Visual Studio Build Tools (MSVC + Windows SDK)
# ------------------------------------------------------------
Write-Step "Verification de Visual Studio Build Tools"
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$haveBuildTools = $false
$instPath = $null
if (Test-Path $vswhere) {
    $instPath = & $vswhere -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath -latest 2>$null
    if ($instPath) { $haveBuildTools = $true }
}
if ($haveBuildTools) {
    Write-Ok "MSVC Build Tools detectees ($instPath)"
} else {
    Write-Host "    Installation VS Build Tools 2022 + composants Tauri..."
    Write-Host "    (cela peut prendre 10-15 minutes selon ta connexion)"
    $btOverride = "--add Microsoft.VisualStudio.Workload.VCTools --add Microsoft.VisualStudio.Component.VC.Tools.x86.x64 --add Microsoft.VisualStudio.Component.Windows11SDK.22621 --includeRecommended --quiet --wait --norestart --nocache"
    Invoke-Native -File winget -Arguments @('install','-e','--id','Microsoft.VisualStudio.2022.BuildTools',
        '--accept-source-agreements','--accept-package-agreements','--silent','--override',$btOverride) -Quiet | Out-Null
    if (Test-Path $vswhere) {
        $instPath = & $vswhere -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath -latest 2>$null
        if ($instPath) {
            Write-Ok "MSVC Build Tools installees ($instPath)"
        } else {
            Write-Warn2 "Build Tools peut-etre installees mais composant VCTools non detecte. Continue quand meme."
        }
    } else {
        Write-Warn2 "vswhere.exe pas trouve. Continue quand meme, on verra a la compil."
    }
}

# ------------------------------------------------------------
# 5. WebView2 Runtime
# ------------------------------------------------------------
Write-Step "Verification de WebView2 Runtime"
$wv2Key = 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
if (Test-Path $wv2Key) {
    $ver = (Get-ItemProperty $wv2Key -ErrorAction SilentlyContinue).pv
    Write-Ok "WebView2 present (v$ver)"
} else {
    Write-Host "    Installation WebView2 Runtime..."
    Invoke-Native -File winget -Arguments @('install','-e','--id','Microsoft.EdgeWebView2Runtime',
        '--accept-source-agreements','--accept-package-agreements','--silent') -Quiet | Out-Null
    Write-Ok "WebView2 installe (ou deja present)"
}

# ------------------------------------------------------------
# 6. npm install
# ------------------------------------------------------------
Write-Step "npm install (dans $projectDir)"
Push-Location $projectDir
try {
    & npm install --no-audit --no-fund 2>&1 | ForEach-Object { Write-Host "    $_" }
    if ($LASTEXITCODE -ne 0) {
        Write-Err2 "npm install a echoue (exit $LASTEXITCODE)"
        Pop-Location
        exit 1
    }
    Write-Ok "deps npm installees"
} finally {
    if ((Get-Location).Path -eq $projectDir) { Pop-Location }
}

# ------------------------------------------------------------
# 7. Build ou Dev
# ------------------------------------------------------------
if ($SkipBuild) {
    Write-Step "Build skippe (option -SkipBuild)"
} elseif ($DevRun) {
    Write-Step "Lancement en mode dev (Ctrl+C pour stopper)"
    Push-Location $projectDir
    try {
        & npm run tauri dev
    } finally {
        if ((Get-Location).Path -eq $projectDir) { Pop-Location }
    }
} else {
    Write-Step "Build Tauri release (peut prendre 5-15 min la premiere fois)"
    Push-Location $projectDir
    try {
        & npm run tauri build 2>&1 | ForEach-Object { Write-Host "    $_" }
        if ($LASTEXITCODE -ne 0) {
            Write-Err2 "Build Tauri a echoue (exit $LASTEXITCODE)"
            Write-Err2 "Si l'erreur parle de 'link.exe' ou 'MSVC', ferme/rouvre PowerShell et relance."
            Pop-Location
            exit 1
        }
        Write-Ok "Build OK"
        $exe = Get-ChildItem (Join-Path $projectDir 'src-tauri\target\release') -Filter '*.exe' -ErrorAction SilentlyContinue | Select-Object -First 1
        $msi = Get-ChildItem (Join-Path $projectDir 'src-tauri\target\release\bundle') -Filter '*.msi' -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($exe) { Write-Host "    Binaire : $($exe.FullName)" -ForegroundColor Green }
        if ($msi) { Write-Host "    MSI     : $($msi.FullName)" -ForegroundColor Green }
    } finally {
        if ((Get-Location).Path -eq $projectDir) { Pop-Location }
    }
}

Write-Step "Termine"
Write-Host "    Etapes manuelles restantes (voir SETUP.md):"
Write-Host "      1. Lancer l'app (.exe ou MSI), aller dans Settings"
Write-Host "      2. Renseigner Groq API Key, Supabase URL + Anon Key"
Write-Host "      3. Tester avec Ctrl+Shift+Space (silent) ou Ctrl+Shift+V (paste)"
