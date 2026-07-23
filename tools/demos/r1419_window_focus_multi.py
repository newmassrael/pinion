#!/usr/bin/env python3
"""R1419/R1420 §5.39 §5.16 — per-window OS focus: two OS windows, each dims only
when ITS OWN window is blurred.

This is the concrete payoff of the R1419 design review: the paint-path OS-focus
read is a window IDENTITY (`os_focused_window() -> Option<String>`), NOT a
binding-wide bool. `hello-window-focus-multi` opens `main` + `inspector`; each
window's `view_for_window` reads the SAME binding-wide mirror and compares it
against its own id (`== Some(window_id)`). So driving OS focus to one window
leaves ONLY the other dimmed — a bool read could never tell the two windows
apart.

The demo reads each window's painted scene separately (`snapshot(window="…")`,
which re-runs that window's `view_for_window`) and drives OS focus per window
with `scene/window_focus {window, focused}`.

Verification scope (≥ 30 assertions, counted exactly = 34):

  (A) boot: both windows declared; OS focus unknown; both read blurred.      (6)
  (B) focus main → ONLY the inspector stays dimmed (identity, not bool).     (5)
  (C) focus inspector → the roles swap on the same mirror.                   (5)
  (D) the gate leg (scene/input_state) agrees with the painted labels.       (3)
  (E) blur the focused window → BOTH windows dim.                            (4)
  (F) a full re-focus cycle across both windows stays per-window.            (6)
  (G) unknown-window drive rejected; the real windows still track.           (5)
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    run_demo,
)

EXAMPLE = "hello-window-focus-multi"
STATUS_TAG = "os_focus_status"
MAIN = "main"
INSPECTOR = "inspector"


def _find_by_tag(node: Any, tag: str) -> Optional[dict[str, Any]]:
    if not isinstance(node, dict):
        return None
    if node.get("tag") == tag:
        return node
    for child in node.get("children") or []:
        hit = _find_by_tag(child, tag)
        if hit is not None:
            return hit
    return None


def _label(tf: RpcSubprocess, window: str) -> Optional[str]:
    # `source="paint"` re-runs THIS window's view_for_window against the live
    # os_focused_window() mirror — no repaint/poll needed.
    snap = tf.snapshot(source="paint", viewport=(300, 180), window=window)
    node = _find_by_tag(snap, STATUS_TAG)
    assert node is not None, f"{window}: paints the tagged OS-focus status label"
    return node.get("content")


def _os_focused(tf: RpcSubprocess) -> Optional[str]:
    resp = tf.request("scene/input_state", {})
    assert resp is not None and isinstance(resp.result, dict)
    kd = resp.result["key_dispatch"]
    assert kd is not None
    return kd["os_focused_window"]


def _drive(tf: RpcSubprocess, window: str, focused: bool) -> None:
    resp = tf.request("scene/window_focus", {"window": window, "focused": focused})
    assert resp is not None


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot: two windows, OS focus unknown, both blurred ────
        windows = tf.request("scene/windows", {})
        assert windows is not None
        declared = {w["id"] for w in windows.result["windows"]}
        assert MAIN in declared, "main window declared"                          # 1
        assert INSPECTOR in declared, "inspector window declared"                # 2
        assert_eq(_os_focused(tf), None, "boot: no window holds OS focus")       # 3
        assert_eq(_label(tf, MAIN), "main: blurred", "boot: main dimmed")        # 4
        assert_eq(_label(tf, INSPECTOR), "inspector: blurred",
                  "boot: inspector dimmed")                                      # 5
        assert_eq(_label(tf, MAIN)[:5], "main:", "the label names its window")   # 6

        # ── (B) focus main → ONLY the inspector stays dimmed ─────────
        _drive(tf, MAIN, True)
        assert_eq(_os_focused(tf), MAIN, "OS focus is on main")                  # 7
        assert_eq(_label(tf, MAIN), "main: OS-ACTIVE",
                  "the main window reads its OWN focus as active")               # 8
        assert_eq(_label(tf, INSPECTOR), "inspector: blurred",
                  "★ the inspector stays dimmed — per-window identity, "
                  "NOT a shared bool")                                           # 9
        assert _label(tf, MAIN) != _label(tf, INSPECTOR), \
            "the two windows read DIFFERENT states from one mirror"              # 10
        assert_eq(_os_focused(tf), MAIN, "gate names main (single SSOT)")        # 11

        # ── (C) focus inspector → the roles swap on the same mirror ──
        _drive(tf, INSPECTOR, True)
        assert_eq(_os_focused(tf), INSPECTOR, "OS focus moved to inspector")     # 12
        assert_eq(_label(tf, INSPECTOR), "inspector: OS-ACTIVE",
                  "the inspector now reads active")                             # 13
        assert_eq(_label(tf, MAIN), "main: blurred",
                  "★ and the main window dims — the roles swapped")             # 14
        assert _label(tf, MAIN) != _label(tf, INSPECTOR), "still distinct"       # 15
        # driving a focus to inspector did not need a main blur first: a
        # focus(new) wins over the stale focus (note_os_focus ordering).
        assert_eq(_os_focused(tf), INSPECTOR, "focus(new) supersedes")           # 16

        # ── (D) the gate leg agrees with the painted labels ─────────
        assert_eq(_label(tf, INSPECTOR).endswith("OS-ACTIVE"),
                  _os_focused(tf) == INSPECTOR,
                  "the active window's label matches the gate leg")             # 17
        assert_eq(_label(tf, MAIN).endswith("blurred"),
                  _os_focused(tf) != MAIN, "the dimmed window matches the gate") # 18
        assert isinstance(_os_focused(tf), str), "a window holds focus"          # 19

        # ── (E) blur the focused window → BOTH dim ──────────────────
        _drive(tf, INSPECTOR, False)
        assert_eq(_os_focused(tf), None, "whole application blurred")            # 20
        assert_eq(_label(tf, MAIN), "main: blurred", "main dimmed on app blur")  # 21
        assert_eq(_label(tf, INSPECTOR), "inspector: blurred",
                  "inspector dimmed on app blur")                               # 22
        assert _label(tf, MAIN).endswith("blurred") and \
            _label(tf, INSPECTOR).endswith("blurred"), \
            "with no OS focus, BOTH windows read the blurred state"             # 23

        # ── (F) a full re-focus cycle across both windows ────────────
        _drive(tf, MAIN, True)
        assert_eq(_label(tf, MAIN), "main: OS-ACTIVE", "re-focus main")          # 24
        assert_eq(_label(tf, INSPECTOR), "inspector: blurred", "inspector off")  # 25
        _drive(tf, INSPECTOR, True)
        assert_eq(_label(tf, INSPECTOR), "inspector: OS-ACTIVE", "re-focus insp") # 26
        assert_eq(_label(tf, MAIN), "main: blurred", "main off again")           # 27
        _drive(tf, MAIN, True)
        assert_eq(_os_focused(tf), MAIN, "focus back on main")                   # 28
        assert_eq(_label(tf, MAIN), "main: OS-ACTIVE", "main active once more")  # 29

        # ── (G) unknown-window drive rejected; real windows unaffected
        try:
            tf.request("scene/window_focus", {"window": "ghost", "focused": True})
        except RpcError as err:
            assert_eq(err.code, -32602, "unknown window drive rejected")         # 30
            assert_eq(err.message, "unknown_window", "R889 gate shape")          # 31
            assert_eq(err.data, "ghost", "error names the supplied id")          # 32
        else:
            raise AssertionError("unknown-window drive must be rejected")
        assert_eq(_os_focused(tf), MAIN, "rejected drive left OS focus intact")   # 33
        assert_eq(_label(tf, MAIN), "main: OS-ACTIVE",
                  "the real windows still track after the rejected drive")       # 34


if __name__ == "__main__":
    sys.exit(run_demo("R1419/R1420 §5.39 §5.16 — per-window OS focus", body))
