@echo off
rem Double-click: builds HomeBots and packs dist\HomeBots (exe + own assets) and dist\HomeBots.zip
cd /d "%~dp0.."
powershell -ExecutionPolicy Bypass -File "%~dp0make_release.ps1"
pause
