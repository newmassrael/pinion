#!/usr/bin/env bash
# tools/sweep_headless.sh — headless demo sweep (R720 Xvfb / R808 real-GPU).
#
# Runs every `tools/demos/*.py` RPC-verification demo against real windowed
# winit + wgpu shells (the same binaries an end user runs — framework code
# is untouched per `[[abstraction-needs-second-consumer]]`; this is pure
# test-harness env-selection).
#
# R835 policy: this FULL sweep is CI-primary (.github/workflows/ci.yml).
# Run it locally only on explicit request. Windows render UNMAPPED by
# default — `rpc_verify.RpcSubprocess` exports `PINION_HIDDEN_WINDOW=1`, so
# the full real render pipeline runs without flashing windows on the
# developer's display. The 6 live-pixel / x11grab demos (r706/r707/r786/
# r794/r806/r1506) opt back to a visible window (`visible_window=True`)
# because they screen-capture the mapped window; locally they flicker, so
# they are the reason the full sweep is CI-primary. Day-to-day, a round verifies
# `cargo test` + `clippy` + its own affected demo(s) (hidden), not this.
#
# Two render modes (PINION_SWEEP_MODE):
#
#   realgpu (default, R808) — real GPU via Vulkan on the host X display
#     (PINION_SWEEP_DISPLAY, default :0). Forced because vello 0.9 + wgpu 29
#     broke the Xvfb + software-GL path: vello's RenderContext builds its
#     instance with display:None (upstream TODO), so wgpu 29's GL backend
#     finds no surface-compatible adapter -> NoCompatibleDevice (VELLO-002).
#     The host cursor returns (the R720 Xvfb move had removed it), but the
#     R719 `scene/pointer_leave` boot baseline absorbs the boot-hover, so
#     the sweep stays green (144/144 verified R808).
#
#   xvfb (R720, legacy) — deterministic throw-away Xvfb display +
#     software-GL (`WGPU_BACKEND=gl LIBGL_ALWAYS_SOFTWARE=1`); the parked
#     cursor is the *complete* fix for R719 boot-hover flakiness. Broken on
#     vello 0.9 by VELLO-002; retained for when the upstream display-handle
#     fix lands (windowed lavapipe Vulkan still OOMs on the Xvfb buffer, so
#     GL stays the Xvfb-mode backend). NOT the surfaceless
#     `headless_screenshot.rs` (R637) path — these are real windowed shells.
#
# DEMO BUDGET (R1472). Every demo is killed after 180s, and that one number is
# enough because a demo drives an ALREADY-BUILT binary over RPC and finishes in
# seconds. The budget therefore measures the demo; when it fires, something is
# genuinely stuck.
#
# It stays one number on evidence, not on principle. The first CI run of this
# job killed `r1447_font_free_tui.py` at 180.010s, which read as "this demo is
# too big for the budget" — the obvious fix being a per-demo override. Measuring
# it refuted that: the demo needs 3.63s once the DEBUG test binaries exist and
# 89.85s when it has to build them (it is the only demo that shells out to
# `cargo test`). The budget was never too small; the job was making one demo
# compile the workspace inside it. So the job pre-builds them and no override
# exists — a per-demo budget would have bought nothing except a longer wait
# before a genuine hang was reported.
#
# Usage:
#   tools/sweep_headless.sh                 # run all demos (realgpu mode)
#   tools/sweep_headless.sh r719 r697       # run only demos whose filename matches a substring
#   PINION_SWEEP_MODE=xvfb tools/sweep_headless.sh   # legacy Xvfb + GL path
#
# Exit 0 iff every selected demo passed.

set -u

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 2

# --- render mode ----------------------------------------------------------
# R808: vello 0.9 + wgpu 29 broke the Xvfb + software-GL path. vello's
# RenderContext builds its wgpu instance with display:None (an upstream
# TODO in vello/src/util.rs), so wgpu 29's GL backend enumerates no
# adapter compatible with the X11 window surface and vello returns
# NoCompatibleDevice (= VELLO-002). The deterministic Xvfb path is
# therefore unavailable on vello 0.9 until vello threads the window
# display handle through.
#
# Until then the sweep defaults to a real GPU via Vulkan on the host X
# display (PINION_SWEEP_DISPLAY, default :0) — 144/144 verified R808.
# That reintroduces the host cursor the R720 Xvfb move removed, but the
# R719 scene/pointer_leave boot baseline absorbs the boot-hover so the
# sweep stays green. Set PINION_SWEEP_MODE=xvfb to restore the old Xvfb +
# software-GL path once VELLO-002 lands upstream.
PINION_SWEEP_MODE="${PINION_SWEEP_MODE:-realgpu}"

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

# --- run all demos --------------------------------------------------------
# The runner body is a single-quoted string so it survives the bash -c /
# xvfb-run boundary intact (exported bash *functions* do not cross a fresh
# shell). The demo list rides across as positional args.
# shellcheck disable=SC2016  # intentional: expands in the child bash, not here
runner='
  total=0; passed=0; n="$#"; failures=""; skipped=""; skip_count=0
  # One number, named once: the kill and the message that reports it must not
  # be able to disagree. They briefly did while R1472 was being written, and a
  # counterfactual run caught the harness announcing a budget it had not used.
  budget=180
  for f in "$@"; do
    total=$((total + 1))
    printf "[sweep %2d/%d] %s ... " "$total" "$n" "$(basename "$f")"
    # R1472 — `python3 -u`: a demo killed by `timeout` dies without flushing,
    # and Python block-buffers a pipe, so the buffered form threw away EVERY
    # byte of the diagnostic. Measured: `timeout 2 python3 -c "print(..); sleep"`
    # captures 0 bytes, the same call with -u captures all of it. That is why
    # the first CI run of this sweep reported a bare `|` under its one failure
    # and nothing else — the gate could not say why it failed.
    if out="$(timeout "$budget" python3 -u "$f" 2>&1)"; then
      # R1333 — a demo that exits 0 is only a REAL pass if it did not SKIP a
      # phase. Several live-pixel / native-drag demos (r706 r707 r786 r794 r787
      # r806) print an uppercase "SKIP" line and return 0 when an environment
      # dep is absent (Pillow / XTEST / a locatable capture), so a bare exit-0
      # tally reports "did nothing" as PASS and the gate looks greener than it
      # is. Detect the marker and tally SKIP distinctly so the headline cannot
      # hide vacuous coverage. Non-fatal (a dev box legitimately lacks some
      # deps); the summary keeps the gap visible instead of hidden.
      if echo "$out" | grep -q "SKIP"; then
        skip_count=$((skip_count + 1)); skipped="$skipped $(basename "$f")"
        echo "PASS (skipped a phase)"
      else
        passed=$((passed + 1)); echo "PASS"
      fi
    else
      rc="$?"
      # R1472 — name the KIND of failure. `timeout` exits 124 when it kills the
      # child, which is a budget verdict and not an assertion verdict: the demo
      # was still working. Reported as a bare FAIL it looks like a broken claim,
      # and that is how the r1447 timeout read on the first CI run of this job.
      if [ "$rc" -eq 124 ]; then
        echo "TIMEOUT (killed at ${budget}s)"
        failures="$failures $(basename "$f")(timeout)"
      else
        echo "FAIL (exit $rc)"
        failures="$failures $(basename "$f")"
      fi
      if [ -n "$out" ]; then
        echo "$out" | sed "s/^/    | /" >&2
      else
        # Nothing to show is itself a finding: with -u above, a demo that wrote
        # nothing really wrote nothing, so do not let an empty block read as a
        # lost diagnostic.
        echo "    | (the demo produced no output before exiting $rc)" >&2
      fi
    fi
  done
  echo "----------------------------------------"
  echo "[sweep] $passed asserted / $total run; $skip_count skipped a phase"
  if [ "$skip_count" -gt 0 ]; then echo "[sweep] SKIPPED (env dep absent, coverage NOT exercised):$skipped" >&2; fi
  if [ -n "$failures" ]; then echo "[sweep] FAILURES:$failures" >&2; exit 1; fi
  exit 0
'

# R1330 — freshness gate. The demo harness (`rpc_verify.RpcSubprocess`) rebuilds
# each example on launch by default, so a stale binary can never outrun a source
# edit (a clean incremental build is a ~0.2s fingerprint check). For the FULL sweep
# that would be one such check per demo (~350); collapse it to a single workspace
# build here and export PINION_ASSUME_BUILT so demos skip their own — this also
# aborts the whole sweep with cargo's own error if the tree does not compile, rather
# than every demo failing to build in turn. For a FILTERED sweep (a handful of
# demos) the per-demo build is cheaper than a workspace fingerprint pass over a dirty
# tree, so skip the upfront build and let the harness rebuild only what it runs.
# PINION_SWEEP_NO_BUILD=1 forces the upfront build off (you vouch the tree is built).
if [ "$#" -eq 0 ] && [ "${PINION_SWEEP_NO_BUILD:-0}" != "1" ]; then
  echo "[sweep] cargo build --release --workspace (freshness gate) ..." >&2
  if ! CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}" cargo build --release --workspace >&2; then
    echo "[sweep] FATAL: workspace build failed — fix it before the sweep runs" >&2
    exit 2
  fi
  export PINION_ASSUME_BUILT=1
elif [ "${PINION_SWEEP_NO_BUILD:-0}" = "1" ]; then
  export PINION_ASSUME_BUILT=1
fi

case "$PINION_SWEEP_MODE" in
  realgpu)
    # Real GPU via Vulkan on the host display (VELLO-002 workaround).
    export WGPU_BACKEND="${WGPU_BACKEND:-vulkan}"
    export DISPLAY="${PINION_SWEEP_DISPLAY:-:0}"
    if ! xdpyinfo >/dev/null 2>&1; then
      echo "[sweep] FATAL: host display $DISPLAY unavailable. A real-GPU X" >&2
      echo "        server is required while VELLO-002 blocks Xvfb + GL on" >&2
      echo "        vello 0.9. Set PINION_SWEEP_DISPLAY, or PINION_SWEEP_MODE" >&2
      echo "        =xvfb once vello threads the window display handle." >&2
      exit 2
    fi
    exec bash -c "$runner" _ "${demos[@]}"
    ;;
  xvfb)
    # Legacy deterministic Xvfb + software-GL path. Broken on vello 0.9 by
    # VELLO-002; retained for when the upstream display-handle fix lands.
    export WGPU_BACKEND="${WGPU_BACKEND:-gl}"
    export LIBGL_ALWAYS_SOFTWARE="${LIBGL_ALWAYS_SOFTWARE:-1}"
    unset DISPLAY
    if ! command -v xvfb-run >/dev/null 2>&1; then
      echo "[sweep] FATAL: xvfb-run not found (install the 'xvfb' package)" >&2
      exit 2
    fi
    exec xvfb-run -a -s "-screen 0 1024x768x24" \
      bash -c "$runner" _ "${demos[@]}"
    ;;
  *)
    echo "[sweep] FATAL: unknown PINION_SWEEP_MODE='$PINION_SWEEP_MODE' (want realgpu|xvfb)" >&2
    exit 2
    ;;
esac
