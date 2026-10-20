#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

if [ -f .env ]; then
  set -a
  source .env
  set +a
fi
if [ -f .fleet-auth.env ]; then
  set -a
  source .fleet-auth.env
  set +a
fi

if pgrep -f 'target/release/kpp-fleet' >/dev/null; then
  echo "Aplikasi sudah berjalan di http://localhost:3000"
  exit 0
fi

if [ ! -x target/release/kpp-fleet ] \
  || [ Cargo.toml -nt target/release/kpp-fleet ] \
  || { [ -f Cargo.lock ] && [ Cargo.lock -nt target/release/kpp-fleet ]; } \
  || [ -n "$(find src -type f -newer target/release/kpp-fleet -print -quit)" ]; then
  source "$HOME/.cargo/env"
  cargo build --release
fi

nohup target/release/kpp-fleet > /tmp/kpp-fleet.log 2>&1 &
echo $! > /tmp/kpp-fleet.pid
sleep 1

if kill -0 "$(cat /tmp/kpp-fleet.pid)" 2>/dev/null; then
  echo "Aplikasi berjalan di http://localhost:3000"
else
  echo "Aplikasi gagal dijalankan. Lihat /tmp/kpp-fleet.log"
  exit 1
fi
