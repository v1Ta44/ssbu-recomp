@echo off
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0launcher\launch.ps1" -Mode dump %*
