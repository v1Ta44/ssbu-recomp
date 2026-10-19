@echo off
chcp 65001 >nul
setlocal
set "patch_root=%~dp0.."
if not "%~1"=="" set "patch_root=%~1"
set "patch_action=install"
if /i "%~n0"=="Rollback" set "patch_action=rollback"
if /i "%~n0"=="Recover" set "patch_action=recover"
if "%patch_action%"=="install" (
  "%~dp0SSBU Updater.exe" install --root "%patch_root%" --archive "%~dp0Update.zip"
) else (
  "%~dp0SSBU Updater.exe" %patch_action% --root "%patch_root%"
)
set "patch_result=%errorlevel%"
if "%patch_result%"=="0" (
  echo 操作完成。现在可以从原来的 SSBU Console.exe 启动。
) else (
  echo 操作未完成。请先关闭游戏、控制台、手柄工具和延迟面板，核对测试包版本后再试。
  echo 详情保存在测试包 updates 文件夹中的 last-result.txt。
)
pause
exit /b %patch_result%
