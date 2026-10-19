@echo off
setlocal
cd /d "%~dp0"
py -3 scripts\analyze_latency_slider.py %*
pause
