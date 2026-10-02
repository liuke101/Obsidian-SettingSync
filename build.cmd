@echo off
rem ============================================================
rem  构建 obsidian-sync（单文件可执行）
rem  用法：在仓库集合根目录或本目录执行 build.cmd
rem  产物：bin\obsidian-sync.exe
rem ============================================================
setlocal
cd /d "%~dp0"

where cargo >nul 2>nul
if errorlevel 1 (
  echo [错误] 未找到 cargo。请先安装 Rust 工具链：https://rustup.rs
  exit /b 1
)

echo == 运行单元测试 ==
cargo test --quiet
if errorlevel 1 (
  echo [错误] 单元测试未通过，已中止构建。
  exit /b 1
)

echo == 构建 release ==
cargo build --release
if errorlevel 1 (
  echo [错误] 构建失败。
  exit /b 1
)

if not exist "bin" mkdir "bin"
copy /y "target\release\obsidian-sync.exe" "bin\obsidian-sync.exe" >nul

echo.
echo 构建完成：%~dp0bin\obsidian-sync.exe
echo.
echo 常用命令（在本目录或任意子目录执行）：
echo   bin\obsidian-sync.exe doctor          检查环境与各库状态
echo   bin\obsidian-sync.exe new ..\NewVault 把新仓库纳入同步
echo   bin\obsidian-sync.exe link            建立/修复全部链接
echo   bin\obsidian-sync.exe check           只体检不改动
endlocal
