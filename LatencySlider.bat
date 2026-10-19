@echo off
setlocal
cd /d "%~dp0"
py -3 scripts\play_latency_slider.py %*
if errorlevel 1 pause
