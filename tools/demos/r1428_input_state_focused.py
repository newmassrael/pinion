#!/usr/bin/env python3
"""R1428 §5.39 §5.16 §5.41 — scene/input_state exposes a derived per-window
`focused` verdict, so an AI reads a terminal cursor's filled-vs-hollow state
(R1427) in ONE call instead of correlating a snapshot with a client-side compare.

The R1427 review asked for a per-window focus fact co-located with the
introspection an AI already uses to read the hollow cursor. Auditing the
snapshot path showed `scene/snapshot` returns a BARE node (no envelope) —
wrapping it is a breaking wire change and folding focus onto the node is the
category error R1427 itself warned against. The category-correct home is the
input/focus axis: `scene/input_state`'s `key_dispatch` already carries the
`os_focused_window` SSOT, so this round adds a derived, non-stored
`focused: bool` beside it.

`focused` is the SAME fails-open predicate (`is_key_dispatch_window`) that gates
key admission AND the R1427 cursor render:

    focused == true   → this window holds OS focus, OR no window's focus is
                        known (the gate fails open) → the cursor renders FILLED
    focused == false  → another window holds OS focus → the cursor renders HOLLOW

So an AI reads `key_dispatch.focused` for `{window}` and knows that window's
cursor state exactly — no compare of `os_focused_window` against a hard-coded
id. The raw SSOT stays exposed too (no drift: both are one value, projected).

Verification scope (>= 30 assertions, counted exactly = 36):

  (A) boot: both windows declared; OS focus unknown; BOTH fail open (filled).  (7)
  (B) focus main   → main focused=true, inspector focused=false (per window).   (7)
  (C) focus inspector → the verdict swaps on the single SSOT.                   (6)
  (D) `focused` predicts the R1419 painted dimming label (render parity).       (4)
  (E) blur the focused window → focus None → BOTH fail open (filled) again.     (5)
  (F) a full re-focus cycle stays per-window.                                   (4)
  (G) unknown-window drive rejected; the real windows still track.             (3)
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


def _key_dispatch(tf: RpcSubprocess, window: Optional[str]) -> dict[str, Any]:
    # `{window}` scopes the read: the shell derives `focused` from
    # is_key_dispatch_window(window). Omitting the scope defaults to the primary.
    params = {"window": window} if window is not None else {}
    resp = tf.request("scene/input_state", params)
    assert resp is not None and isinstance(resp.result, dict)
    kd = resp.result["key_dispatch"]
    assert kd is not None, "the GUI shell always supplies the multi-window axis"
    return kd


def _focused(tf: RpcSubprocess, window: str) -> bool:
    return _key_dispatch(tf, window)["focused"]


def _os_focused(tf: RpcSubprocess) -> Optional[str]:
    return _key_dispatch(tf, None)["os_focused_window"]


def _label(tf: RpcSubprocess, window: str) -> Optional[str]:
    snap = tf.snapshot(source="paint", viewport=(300, 180), window=window)
    node = _find_by_tag(snap, STATUS_TAG)
    assert node is not None, f"{window}: paints the tagged OS-focus status label"
    return node.get("content")


def _drive(tf: RpcSubprocess, window: str, focused: bool) -> None:
    resp = tf.request("scene/window_focus", {"window": window, "focused": focused})
    assert resp is not None


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot: two windows, OS focus unknown, BOTH fail open ──
        windows = tf.request("scene/windows", {})
        assert windows is not None
        declared = {w["id"] for w in windows.result["windows"]}
        assert MAIN in declared, "main window declared"                          # 1
        assert INSPECTOR in declared, "inspector window declared"                # 2
        assert_eq(_os_focused(tf), None, "boot: no window holds OS focus")       # 3
        # The gate fails OPEN with no focus, so EVERY window reads filled.
        assert_eq(_focused(tf, MAIN), True,
                  "boot: main fails open → filled cursor")                       # 4
        assert_eq(_focused(tf, INSPECTOR), True,
                  "boot: inspector fails open → filled cursor")                  # 5
        # `focused` is ALWAYS a bare bool on the GUI axis (never null).
        assert isinstance(_key_dispatch(tf, MAIN)["focused"], bool)              # 6
        assert isinstance(_key_dispatch(tf, INSPECTOR)["focused"], bool)         # 7

        # ── (B) focus main → main filled, inspector HOLLOW (per window) ──
        _drive(tf, MAIN, True)
        assert_eq(_os_focused(tf), MAIN, "OS focus is on main")                  # 8
        assert_eq(_focused(tf, MAIN), True,
                  "the focused window reads its OWN cursor as filled")           # 9
        assert_eq(_focused(tf, INSPECTOR), False,
                  "★ the inspector reads HOLLOW — per-window identity, "
                  "NOT a shared bool")                                           # 10
        assert _focused(tf, MAIN) != _focused(tf, INSPECTOR), \
            "the two windows read DIFFERENT verdicts from one SSOT"              # 11
        # The raw SSOT is identical in both scopes; only the projection differs.
        assert_eq(_key_dispatch(tf, MAIN)["os_focused_window"],
                  _key_dispatch(tf, INSPECTOR)["os_focused_window"],
                  "both scopes carry the identical raw os_focused_window")       # 12
        assert_eq(_key_dispatch(tf, INSPECTOR)["os_focused_window"], MAIN,
                  "the raw fact names main even when read via the inspector")    # 13
        assert_eq(_focused(tf, INSPECTOR), False,
                  "and the inspector's derived verdict still reads hollow")      # 14

        # ── (C) focus inspector → the verdict swaps on one SSOT ──────
        _drive(tf, INSPECTOR, True)
        assert_eq(_os_focused(tf), INSPECTOR, "OS focus moved to inspector")     # 15
        assert_eq(_focused(tf, INSPECTOR), True, "inspector now filled")         # 16
        assert_eq(_focused(tf, MAIN), False,
                  "★ and main reads HOLLOW — the roles swapped")                 # 17
        assert _focused(tf, MAIN) != _focused(tf, INSPECTOR), "still distinct"   # 18
        assert_eq(_os_focused(tf), INSPECTOR, "focus(new) supersedes")           # 19
        # A focus(new) won without a preceding blur (note_os_focus ordering).
        assert_eq(_focused(tf, INSPECTOR), True, "no stale-focus latch")         # 20

        # ── (D) `focused` predicts the R1419 painted dimming label ───
        # The window that paints "OS-ACTIVE" is exactly the focused one; the
        # cursor there is filled. This ties the derived verdict to the render.
        assert_eq(_label(tf, INSPECTOR).endswith("OS-ACTIVE"),
                  _focused(tf, INSPECTOR),
                  "the active window's label matches its filled-cursor verdict") # 21
        assert_eq(_label(tf, MAIN).endswith("blurred"),
                  not _focused(tf, MAIN),
                  "the dimmed window matches its hollow-cursor verdict")         # 22
        assert _focused(tf, INSPECTOR) and not _focused(tf, MAIN), \
            "exactly one window is filled while another holds focus"             # 23
        assert_eq(_focused(tf, INSPECTOR),
                  _os_focused(tf) == INSPECTOR,
                  "with a window focused, filled == (this window is that one)")  # 24

        # ── (E) blur the focused window → focus None → BOTH fail open ─
        _drive(tf, INSPECTOR, False)
        assert_eq(_os_focused(tf), None, "whole application blurred")            # 25
        assert_eq(_focused(tf, MAIN), True,
                  "app blur fails open → main filled again")                     # 26
        assert_eq(_focused(tf, INSPECTOR), True,
                  "app blur fails open → inspector filled again")                # 27
        # Distinct from (B): both labels read blurred yet both cursors are
        # filled — hollow means "another window is focused", not "app blurred".
        assert _label(tf, MAIN).endswith("blurred") and \
            _label(tf, INSPECTOR).endswith("blurred"), \
            "both windows paint the dimmed label with no OS focus"               # 28
        assert _focused(tf, MAIN) and _focused(tf, INSPECTOR), \
            "★ yet BOTH cursors are filled — fails open, not hollow"             # 29

        # ── (F) a full re-focus cycle stays per-window ───────────────
        _drive(tf, MAIN, True)
        assert_eq(_focused(tf, MAIN), True, "re-focus main → filled")            # 30
        assert_eq(_focused(tf, INSPECTOR), False, "inspector hollow")            # 31
        _drive(tf, INSPECTOR, True)
        assert_eq(_focused(tf, INSPECTOR), True, "re-focus inspector → filled")  # 32
        assert_eq(_focused(tf, MAIN), False, "main hollow again")                # 33

        # ── (G) unknown-window drive rejected; real windows unaffected
        try:
            tf.request("scene/window_focus", {"window": "ghost", "focused": True})
        except RpcError as err:
            assert_eq(err.code, -32602, "unknown window drive rejected")         # 34
        else:
            raise AssertionError("unknown-window drive must be rejected")
        assert_eq(_os_focused(tf), INSPECTOR, "rejected drive left focus intact") # 35
        assert_eq(_focused(tf, INSPECTOR), True,
                  "the real windows still track after the rejected drive")       # 36


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1428 §5.39 §5.16 §5.41 — scene/input_state per-window `focused`", body))
