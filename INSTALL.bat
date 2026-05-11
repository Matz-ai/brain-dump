@echo off
REM Lanceur double-clic pour INSTALL.ps1.
REM Ouvre PowerShell avec ExecutionPolicy Bypass et lance l'installeur.

cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0INSTALL.ps1" %*
set ERR=%ERRORLEVEL%
echo.
echo ----------------------------------------------------
if "%ERR%"=="0" (
    echo  Installation terminee. Tu peux fermer cette fenetre.
) else (
    echo  L'installeur s'est termine avec le code %ERR%.
    echo  Lis les messages plus haut pour comprendre.
)
echo ----------------------------------------------------
pause
