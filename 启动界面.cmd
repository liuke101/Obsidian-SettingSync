@echo off
rem ============================================================
rem  启动 obsidian-sync 图形界面（免开终端敲命令）
rem  双击本文件即可；浏览器会自动打开控制台。
rem ============================================================
setlocal
cd /d "%~dp0"

if not exist "bin\obsidian-sync.exe" (
  echo 尚未构建，正在调用 build.cmd ...
  call build.cmd
  if errorlevel 1 (
    echo [错误] 构建失败，无法启动界面。
    pause
    exit /b 1
  )
)

"bin\obsidian-sync.exe" gui %*
if errorlevel 1 (
  echo.
  echo 界面已退出（若为报错，请查看上方信息）。
  pause
)
endlocal
