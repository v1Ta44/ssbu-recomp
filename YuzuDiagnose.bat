@echo off
cd /d "%~dp0"
".venv\Scripts\python.exe" scripts\yuzu_play.py --diagnostic %*
if errorlevel 1 pause
