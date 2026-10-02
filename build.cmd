@echo off
rem ============================================================
rem  Build obsidian-sync (single-file executable)
rem  Batch files are parsed in the system ANSI codepage, so this
rem  file is kept ASCII-only on purpose. Do not add non-ASCII.
rem  Output: bin\obsidian-sync.exe
rem ============================================================
setlocal
cd /d "%~dp0"

where cargo >nul 2>nul
if errorlevel 1 (
  echo [error] cargo not found. Install the Rust toolchain first: https://rustup.rs
  exit /b 1
)

echo == running unit tests ==
cargo test --quiet
if errorlevel 1 (
  echo [error] unit tests failed, build aborted.
  exit /b 1
)

echo == building release ==
cargo build --release
if errorlevel 1 (
  echo [error] build failed.
  exit /b 1
)

if not exist "bin" mkdir "bin"
copy /y "target\release\obsidian-sync.exe" "bin\obsidian-sync.exe" >nul

echo.
echo Built: %~dp0bin\obsidian-sync.exe
echo.
echo Common commands:
echo   bin\obsidian-sync.exe gui              launch the GUI
echo   bin\obsidian-sync.exe doctor           check environment and vaults
echo   bin\obsidian-sync.exe new ..\NewVault  add a new vault
echo   bin\obsidian-sync.exe check            health check only
endlocal