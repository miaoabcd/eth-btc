#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_PATH="${BIN_PATH:-$ROOT_DIR/target/release/eth_btc_strategy}"
CONFIG_PATH="${CONFIG_PATH:-$ROOT_DIR/config.toml}"
LOCK_PATH="${LOCK_PATH:-/tmp/eth-btc-live.lock}"
LOG_PATH="${LOG_PATH:-$ROOT_DIR/data/logs/cron-run.log}"
START_DELAY_SECS="${START_DELAY_SECS:-10}"

export RUST_LOG="${RUST_LOG:-info}"

mkdir -p "$(dirname "$LOG_PATH")"

if [[ ! -x "$BIN_PATH" ]]; then
  echo "binary not found or not executable: $BIN_PATH" >> "$LOG_PATH"
  echo "build it first: cargo build --release" >> "$LOG_PATH"
  exit 1
fi

if [[ ! -f "$CONFIG_PATH" ]]; then
  echo "config not found: $CONFIG_PATH" >> "$LOG_PATH"
  exit 1
fi

{
  echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] run_once start delay=${START_DELAY_SECS}s"
  sleep "$START_DELAY_SECS"

  set +e
  flock -n "$LOCK_PATH" "$BIN_PATH" --config "$CONFIG_PATH" --once
  rc=$?
  set -e

  if [[ "$rc" -eq 1 ]]; then
    echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] run_once skipped lock_busy"
    echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] run_once end rc=0"
    exit 0
  fi

  echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] run_once end rc=$rc"
  exit "$rc"
} >> "$LOG_PATH" 2>&1
