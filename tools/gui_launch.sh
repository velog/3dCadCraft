#!/bin/bash
# Starts the debug GUI for scripted visual tests and prints its X11 window id.
# Usage: tools/gui_launch.sh [ENV=value ...] [-- PROJECT_PATH]
#   e.g. WID=$(tools/gui_launch.sh CADCRAFT3D_DEMO=slot)
# Then drive it with tools/gui_sim.py (see that file) and stop it with
#   pkill -x 3dcadcraft      (never `pkill -f`: it matches the calling shell)
set -e
cd "$(dirname "$0")/.."
envs=(); args=()
while [ $# -gt 0 ]; do
  if [ "$1" = "--" ]; then shift; args=("$@"); break; fi
  envs+=("$1"); shift
done
# The title is "<file>[ *] - 3DCadCraft": match its end.
find_wid() { xwininfo -root -tree 2>/dev/null | awk '/- 3DCadCraft"/ {print $1; exit}'; }
while [ -n "$(find_wid)" ]; do sleep 0.3; done
(env "${envs[@]}" RUST_LOG=warn timeout 300 ./target/debug/3dcadcraft "${args[@]}" > target/gui-test.log 2>&1 &)
for _ in $(seq 1 30); do
  sleep 0.4
  WID=$(find_wid)
  [ -n "$WID" ] && break
done
sleep 2
echo "$WID"
