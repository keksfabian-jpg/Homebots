@echo off
rem Double-click: runs all HomeBots test scenarios (tests\scenarios.txt), frames go to logs\rec_*
cd /d "%~dp0.."
powershell -ExecutionPolicy Bypass -File "%~dp0hbtest.ps1" %*
if errorlevel 1 pause
