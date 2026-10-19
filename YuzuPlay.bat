@echo off
cd /d "%~dp0"
".venv\Scripts\python.exe" scripts\yuzu_play.py
if errorlevel 1 pause
