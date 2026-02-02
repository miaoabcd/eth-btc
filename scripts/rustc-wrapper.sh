#!/usr/bin/env bash
set -euo pipefail

rustc_bin="rustc"
if [[ "${1:-}" == *rustc ]]; then
  rustc_bin="$1"
  shift
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
lib_path="${script_dir}/liblink_fallback.so"
src_path="${script_dir}/link_fallback.c"
lock_dir="${script_dir}/.link_fallback.lock"

needs_build() {
  [[ ! -f "$lib_path" || "$src_path" -nt "$lib_path" ]]
}

if needs_build; then
  if mkdir "$lock_dir" 2>/dev/null; then
    if needs_build; then
      if ! command -v gcc >/dev/null 2>&1; then
        echo "rustc-wrapper: gcc required to build link fallback library" >&2
        rmdir "$lock_dir" 2>/dev/null || true
        exit 1
      fi
      gcc -shared -fPIC -O2 -o "$lib_path" "$src_path"
    fi
    rmdir "$lock_dir" 2>/dev/null || true
  else
    while needs_build; do
      sleep 0.05
    done
  fi
fi

export LD_PRELOAD="${lib_path}${LD_PRELOAD:+:${LD_PRELOAD}}"

out_dir=""
args=("$@")
for ((i=0; i<${#args[@]}; i++)); do
  arg="${args[$i]}"
  if [[ "$arg" == "--out-dir" ]]; then
    if (( i + 1 < ${#args[@]} )); then
      out_dir="${args[$((i+1))]}"
    fi
  elif [[ "$arg" == --out-dir=* ]]; then
    out_dir="${arg#--out-dir=}"
  fi
 done

if [[ -n "$out_dir" ]]; then
  export TMPDIR="$out_dir"
  export RUSTC_TMPDIR="$out_dir"
fi

exec "$rustc_bin" "$@"
