#!/usr/bin/env python3
"""R1419 §5.39 §5.16 — the OS-window-focus reactive read + its RPC drive peer.

PINION-PR73 recorded a verified gap: a binding's paint path
(`WidgetCore::view` / `reconcile_frame`) could read the focused paint TAG
(`focus_state::focused()`) but had NO reactive read of which OS window holds
the OS keyboard focus. The shell tracked it (`os_focused_window`, the key-
dispatch gate) and exposed it to an EXTERNAL observer over RPC
(`scene/input_state`), but the binding itself could not see a whole-window
blur — so a terminal multiplexer could not report `FocusOut`/`FocusIn` to its
children on alt-tab, and a TUI could not dim on deactivate.

R1419 adds the missing paint-path read `pinion_core::window_focus_state::
os_focused_window()` — the peer of `focus_state::focused()` on the OS-focus
axis, carrying the window IDENTITY (`Option<String>`), published by the shell's
one `set_os_focused_window` funnel. `hello-window-focus` consumes it: its view
dims the panel and re-renders a status label when OS focus changes.

To exercise a winit-driven signal headlessly, this round also adds the drive
PEER of the `os_focused_window` READ: `scene/window_focus {focused: bool}`
simulates a `WindowEvent::Focused` edge for the addressed `{window}` scope, so
an AI can drive the OS-focus-reactive display without a window manager.

Verification scope (≥ 30 assertions, counted exactly = 42):

  (A) rpc/methods discovery — scene/window_focus is a routed read method. (2)
  (B) boot: OS focus unknown — input_state gate leg null, the view's status
      label reads the blurred spelling.                                    (5)
  (C) drive focus(true) → the gate names "main", the drive echoes it, AND
      the reactive view re-renders the focused label (the whole point).    (5)
  (D) drive blur(false) → gate null, drive echoes null, the view re-renders
      the blurred label.                                                   (5)
  (E) a full re-focus / re-blur cycle stays consistent.                    (6)
  (F) an idempotent re-focus of the already-focused window.                (2)
  (G) input_state is side-effect-free across the drive.                    (1)
  (H) invalid params — missing / non-bool `focused` → -32602.              (4)
  (I) unknown-window scope rejection — the R889 gate rejects
      {window:"bogus"} before the drive, with data naming the id.          (6)
  (J) after the bogus traffic the real window still drives normally.       (2)
  (K) R1420 full-edge: a driven blur replays window_blurred, clearing a
      held-key chord (not just the gate/mirror) — no stranded chord.        (4)
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

EXAMPLE = "hello-window-focus"
STATUS_TAG = "os_focus_status"
MAIN = "main"  # pinion_runtime::DEFAULT_WINDOW


def _input_state(tf: RpcSubprocess) -> dict[str, Any]:
    resp = tf.request("scene/input_state", {})
    assert resp is not None and isinstance(resp.result, dict)
    return resp.result


def _os_focused(tf: RpcSubprocess) -> Optional[str]:
    kd = _input_state(tf)["key_dispatch"]
    assert kd is not None, "the GUI backend surfaces the key-dispatch gate"
    return kd["os_focused_window"]


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


def _status_label(tf: RpcSubprocess) -> Optional[str]:
    # `source="paint"` re-runs V::view, so the label reflects the CURRENT
    # os_focused_window() read — no repaint/poll needed.
    snap = tf.snapshot(source="paint", viewport=(360, 220))
    node = _find_by_tag(snap, STATUS_TAG)
    assert node is not None, "the view paints the tagged OS-focus status label"
    return node.get("content")


def _drive(tf: RpcSubprocess, focused: bool, **params: Any) -> Any:
    body = {"focused": focused, **params}
    resp = tf.request("scene/window_focus", body)
    assert resp is not None
    return resp.result


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) discovery ────────────────────────────────────────────
        methods = tf.request("rpc/methods", {})
        assert methods is not None
        by_name = {m["name"]: m for m in methods.result["methods"]}
        assert "scene/window_focus" in by_name, "the drive method is discoverable"  # 1
        assert_eq(by_name["scene/window_focus"]["occ"], "read",
                  "occ read — out-of-band OS-focus drive, no OCC bump")            # 2

        # ── (B) boot: OS focus unknown ───────────────────────────────
        boot = _input_state(tf)
        # R1428 — the gate object also carries the derived per-window `focused`
        # verdict beside the raw os_focused_window / key_press_owners legs.
        assert_eq(sorted(boot["key_dispatch"].keys()),
                  ["focused", "key_press_owners", "os_focused_window"],
                  "the key-dispatch gate carries the os_focused_window leg")       # 3
        assert_eq(_os_focused(tf), None, "boot: no OS window holds focus")         # 4
        assert_eq(_status_label(tf), "OS focus: (blurred)",
                  "boot: the reactive view reads None → the blurred label")        # 5
        # the label the view paints agrees with the gate leg
        assert_eq(_os_focused(tf), None, "gate + view agree at boot")              # 6
        assert_eq(sorted(boot.keys()),
                  ["cursor", "held_keys", "key_dispatch", "modifiers"],
                  "input_state carries the full axis set")                         # 7

        # ── (C) drive focus(true): gate + drive echo + reactive view ─
        res = _drive(tf, True)
        assert_eq(res, {"os_focused_window": MAIN},
                  "the drive echoes the resulting OS-focused window id")           # 8
        assert_eq(_os_focused(tf), MAIN, "focus names the primary window")         # 9
        assert_eq(_status_label(tf), f"OS focus: {MAIN}",
                  "the reactive view re-renders the focused label (PR73's ask)")   # 10
        assert_eq(_os_focused(tf), _status_label(tf).removeprefix("OS focus: "),
                  "the view's read equals the shell gate — one SSOT")              # 11
        assert isinstance(res["os_focused_window"], str), "result names a window"  # 12

        # ── (D) drive blur(false): gate clears, view re-renders ──────
        res = _drive(tf, False)
        assert_eq(res, {"os_focused_window": None},
                  "a blur echoes os_focused_window: null")                         # 13
        assert_eq(_os_focused(tf), None, "blur clears the gate")                   # 14
        assert_eq(_status_label(tf), "OS focus: (blurred)",
                  "the reactive view re-renders the blurred label on blur")        # 15
        assert_eq(_os_focused(tf), None, "gate + view agree after blur")           # 16
        assert res["os_focused_window"] is None, "blur result is null"             # 17

        # ── (E) a full re-focus / re-blur cycle ──────────────────────
        _drive(tf, True)
        assert_eq(_os_focused(tf), MAIN, "re-focus names main again")             # 18
        assert_eq(_status_label(tf), f"OS focus: {MAIN}", "view re-focused")      # 19
        assert_eq(_input_state(tf)["key_dispatch"]["key_press_owners"], {},
                  "no key held → focus drive leaves the press-owner map empty")   # 20
        _drive(tf, False)
        assert_eq(_os_focused(tf), None, "re-blur clears")                        # 21
        assert_eq(_status_label(tf), "OS focus: (blurred)", "view re-blurred")    # 22
        _drive(tf, True)
        assert_eq(_os_focused(tf), MAIN, "focus once more")                       # 23

        # ── (F) idempotent re-focus of the already-focused window ────
        res = _drive(tf, True)
        assert_eq(res, {"os_focused_window": MAIN}, "re-focus is idempotent")     # 24
        assert_eq(_os_focused(tf), MAIN, "still focused, no churn")               # 25

        # ── (G) the READ is side-effect-free across the drive ────────
        assert_eq(_input_state(tf), _input_state(tf),
                  "two consecutive input_state reads are identical")             # 26

        # ── (H) invalid params ───────────────────────────────────────
        try:
            tf.request("scene/window_focus", {})
        except RpcError as err:
            assert_eq(err.code, -32602, "missing focused → invalid params")      # 27
            assert "focused" in (err.data or ""), \
                "the error data names the missing field"                         # 28
        else:
            raise AssertionError("missing params.focused must error")
        try:
            tf.request("scene/window_focus", {"focused": "yes"})
        except RpcError as err:
            assert_eq(err.code, -32602, "non-bool focused → invalid params")     # 29
            assert "boolean" in (err.data or ""), \
                "the error data names the expected type"                         # 30
        else:
            raise AssertionError("non-bool params.focused must error")

        # ── (I) unknown-window scope rejection (R889 gate) ───────────
        try:
            tf.request("scene/window_focus", {"window": "bogus", "focused": True})
        except RpcError as err:
            assert_eq(err.code, -32602, "unknown window → -32602")               # 31
            assert_eq(err.message, "unknown_window", "the R889 gate shape")      # 32
            assert_eq(err.data, "bogus", "the error data names the supplied id") # 33
        else:
            raise AssertionError("unknown-window scope must be rejected")
        # the rejected drive changed nothing — main is still focused
        assert_eq(_os_focused(tf), MAIN, "a rejected drive left OS focus intact") # 34
        assert_eq(_status_label(tf), f"OS focus: {MAIN}",
                  "the reactive view is untouched by the rejected drive")        # 35
        try:
            tf.request("scene/window_focus", {"window": "", "focused": False})
        except RpcError as err:
            assert_eq(err.code, -32602, "empty-string window id rejected")       # 36

        # ── (J) the real window still drives after the bogus traffic ─
        _drive(tf, False)
        assert_eq(_os_focused(tf), None, "main still blurs normally")            # 37
        assert_eq(_status_label(tf), "OS focus: (blurred)",
                  "the view still tracks OS focus after rejected traffic")       # 38

        # ── (K) R1420: the blur drive replays the FULL winit Focused arm ─
        # A driven blur must do more than clear the gate/mirror: like a real OS
        # blur (and Qt window deactivation) it settles held state, so a chord
        # held across an alt-tab cannot strand. Arm a held key, drive a blur,
        # and observe the held-key cache cleared (window_blurred, not just
        # note_os_focus).
        _drive(tf, True)
        assert_eq(_os_focused(tf), MAIN, "re-focus for the full-edge check")     # 39
        tf.key(at=(10.0, 12.0), name="Space", state="down")
        assert_eq(_input_state(tf)["held_keys"], ["Space"],
                  "a chord is held while focused")                               # 40
        _drive(tf, False)
        assert_eq(_input_state(tf)["held_keys"], [],
                  "the blur drive replays window_blurred — the held chord is "
                  "cleared, not stranded (full OS-focus-edge fidelity)")         # 41
        assert_eq(_os_focused(tf), None, "and the gate/mirror blurred too")      # 42


if __name__ == "__main__":
    sys.exit(run_demo("R1419 §5.39 §5.16 — window_focus reactive read + drive", body))
