@echo off
rem Drag a report zip (or a session folder) onto this file, or: Replay.bat <report.zip> [-Force] [-LocalConfig]
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0launcher\launch.ps1" -Mode replay -Report %*
