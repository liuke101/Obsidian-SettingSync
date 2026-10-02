@echo off
rem ============================================================
rem  Start obsidian-sync GUI (double-click this file)
rem  Batch files are read in the system ANSI codepage,
rem  so this file is intentionally kept ASCII-only.
rem ============================================================
setlocal
cd /d "%~dp0"

if not exist "bin\obsidian-sync.exe" (
  echo [info] not built yet, running build.cmd ...
  call build.cmd
  if errorlevel 1 (
    echo [error] build failed.
    pause
    exit /b 1
  )
)

"bin\obsidian-sync.exe" gui %*
if errorlevel 1 (
  echo.
  echo [info] GUI exited. Press any key to close.
  pause
)
endlocal