#!/usr/bin/env bash
set -euo pipefail

pid=""
if [ -f /tmp/kpp-fleet.pid ] && kill -0 "$(cat /tmp/kpp-fleet.pid)" 2>/dev/null; then
  pid="$(cat /tmp/kpp-fleet.pid)"
else
  pid="$(pgrep -f 'target/release/kpp-fleet' | head -n 1 || true)"
fi

if [ -n "$pid" ]; then
  kill "$pid"
  rm -f /tmp/kpp-fleet.pid
  echo "Aplikasi dihentikan."
else
  rm -f /tmp/kpp-fleet.pid
  echo "Aplikasi tidak sedang berjalan."
fi
