#!/usr/bin/env bash
# ============================================================
#  Start obsidian-sync GUI (run this file to launch)
#  Windows 版对应脚本：启动界面.cmd
# ============================================================
cd "$(dirname "$0")"

if [ ! -x "bin/obsidian-sync" ]; then
  echo "[info] not built yet, running build.sh ..."
  if ! bash build.sh; then
    echo "[error] build failed."
    read -r -p "Press Enter to close..."
    exit 1
  fi
fi

bin/obsidian-sync gui "$@"
code=$?
if [ "$code" -ne 0 ]; then
  echo
  echo "[info] GUI exited. Press Enter to close."
  read -r
fi
exit "$code"
