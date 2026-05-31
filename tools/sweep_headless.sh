#!/usr/bin/env bash
# tools/sweep_headless.sh — R720 deterministic headless demo sweep.
#
# Runs every `tools/demos/*.py` RPC-verification demo inside a single
# throw-away Xvfb display so the sweep no longer borrows the developer's
# physical X server (`:0`). Removing the physical display removes the
# physical cursor, which is the *complete* textbook fix for the class of
# boot-hover flakiness that R719 (`scene/pointer_leave` + boot baseline)
# could only mask at the boot instant: under Xvfb the pointer is parked
# forever, so a window the WM happens to map under the host cursor can no
# longer receive a spurious boot `CursorMoved` (the R719 root cause).
#
# Framework code is untouched (`[[abstraction-needs-second-consumer]]`):
# this is pure test-harness env-selection, not a pinion RPC-input-only
# test mode. The framework binaries are the same windowed winit + wgpu
# shells an end user runs.
#
# wgpu backend: a windowed winit surface under lavapipe (Vulkan) panics
# `Out of Memory` on the Xvfb framebuffer (rc=101). The GL/llvmpipe path
# (`WGPU_BACKEND=gl LIBGL_ALWAYS_SOFTWARE=1`) creates the surface fine, so
# the wrapper pins it. This is the same software-render combo CI uses for
# headless GL; it is NOT the surfaceless `headless_screenshot.rs` (R637)
# path — these demos are real windowed shells.
#
# Usage:
#   tools/sweep_headless.sh                 # run all demos
#   tools/sweep_headless.sh r719 r697       # run only demos whose filename matches a substring
#
# Exit 0 iff every selected demo passed.

set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 2

# --- headless render env (see header) -------------------------------------
export WGPU_BACKEND="${WGPU_BACKEND:-gl}"
export LIBGL_ALWAYS_SOFTWARE="${LIBGL_ALWAYS_SOFTWARE:-1}"
# Drop any inherited DISPLAY so a stray `:0` cannot leak the physical
# server back in; xvfb-run injects its own throw-away DISPLAY.
unset DISPLAY

if ! command -v xvfb-run >/dev/null 2>&1; then
  echo "[sweep] FATAL: xvfb-run not found (install the 'xvfb' package)" >&2
  exit 2
fi

# --- demo selection -------------------------------------------------------
declare -a demos=()
if [ "$#" -gt 0 ]; then
  for f in tools/demos/*.py; do
    base="$(basename "$f")"
    for pat in "$@"; do
      case "$base" in *"$pat"*) demos+=("$f"); break;; esac
    done
  done
else
  for f in tools/demos/*.py; do demos+=("$f"); done
fi

if [ "${#demos[@]}" -eq 0 ]; then
  echo "[sweep] no demos matched: $*" >&2
  exit 2
fi

# --- run all demos under ONE Xvfb display ---------------------------------
# The whole loop runs inside a single `xvfb-run`, so one virtual display is
# created for the entire sweep instead of one per demo (faster + a single
# parked cursor for the whole run).
# The runner body is a single-quoted string so it survives the xvfb-run
# boundary intact (exported bash *functions* do not cross xvfb-run's fresh
# shell). The demo list rides across as positional args.
# shellcheck disable=SC2016  # intentional: expands in the child bash, not here
runner='
  total=0; passed=0; n="$#"; failures=""
  for f in "$@"; do
    total=$((total + 1))
    printf "[sweep %2d/%d] %s ... " "$total" "$n" "$(basename "$f")"
    if out="$(timeout 180 python3 "$f" 2>&1)"; then
      passed=$((passed + 1)); echo "PASS"
    else
      echo "FAIL"; failures="$failures $(basename "$f")"
      echo "$out" | sed "s/^/    | /" >&2
    fi
  done
  echo "----------------------------------------"
  echo "[sweep] $passed/$total passed"
  if [ -n "$failures" ]; then echo "[sweep] FAILURES:$failures" >&2; exit 1; fi
  exit 0
'
exec xvfb-run -a -s "-screen 0 1024x768x24" \
  bash -c "$runner" _ "${demos[@]}"
