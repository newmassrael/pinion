#!/usr/bin/env python3
"""R913 §5.38 §5.22 — editor status bar end-to-end RPC demo.

R913 adds the bottom chrome of an editor shell (VS Code / JetBrains
status bar): a reactive cursor-position segment (Ln/Col), clickable
language-mode + encoding segments that cycle on click, and a message
area. It rounds out the self-hosted-editor shell next to the R912
command palette and the R909/R910 inspector.

A primary `StatusBarExternal` holds the position / mode / encoding /
message; clickable segments are tagged `statusbar#mode` /
`statusbar#encoding` and route a click through the R51.42 `#`-split to
the External `send` wire, which cycles the *named* segment on the
activation edge. All scene-as-data and RPC-drivable (§2 #2).

## Demo scope (>=30 assertions, sections A-F)

  (A) Boot: defaults (Ln 1, Col 1 / Rust / UTF-8 / empty message).
  (B) Cursor position reflects intervene line/col.
  (C) Mode cycles via RPC invoke (advances + wraps).
  (D) Mode + encoding cycle via segment clicks (named send wire).
  (E) Message round-trips.
  (F) Read-only axes + activation-edge gating.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    run_demo,
    wait_query,
    wait_until,
)

_MODES = ["Plain Text", "Rust", "Python", "Markdown", "JSON"]


def _q(tf: RpcSubprocess, key: str) -> Any:
    return tf.query(f"/external/{key}")


def _wait_ready(tf: RpcSubprocess) -> None:
    wait_until(lambda: True if _q(tf, "mode") == "Rust" else None, desc="status bar ready")


def _assert_boot(tf: RpcSubprocess) -> None:
    """(A) defaults."""
    assert _q(tf, "line") == 1 and _q(tf, "col") == 1, "cursor at 1,1"
    assert _q(tf, "position") == "Ln 1, Col 1", "position formatted"
    assert _q(tf, "mode") == "Rust", "boot mode Rust"
    assert _q(tf, "mode_index") == 1, "Rust is mode index 1"
    assert _q(tf, "encoding") == "UTF-8", "boot encoding UTF-8"
    assert _q(tf, "message") == "", "no message at boot"


def _assert_position(tf: RpcSubprocess) -> None:
    """(B) position tracks line/col (synchronous External intervene)."""
    tf.intervene("/external/line", 128)
    tf.intervene("/external/col", 12)
    assert _q(tf, "line") == 128 and _q(tf, "col") == 12
    assert _q(tf, "position") == "Ln 128, Col 12", "position reflects the cursor"
    # Floor at 1 (both axes).
    tf.intervene("/external/line", 0)
    assert _q(tf, "line") == 1, "line floors at 1"
    tf.intervene("/external/col", 0)
    assert _q(tf, "col") == 1, "col floors at 1"
    assert _q(tf, "position") == "Ln 1, Col 1", "position reflects the floored cursor"


def _assert_mode_cycle_rpc(tf: RpcSubprocess) -> None:
    """(C) cycle_mode advances + wraps."""
    assert _q(tf, "mode") == "Rust"
    ran = tf.invoke("/external/cycle_mode", None)
    assert ran == "Python", "cycle_mode returns the new mode"
    assert _q(tf, "mode") == "Python"
    assert _q(tf, "mode_index") == 2, "Python is mode index 2"
    # Cycle the rest of the way around -> back to Rust.
    for _ in range(len(_MODES) - 1):
        tf.invoke("/external/cycle_mode", None)
    assert _q(tf, "mode") == "Rust", "a full cycle wraps to Rust"


def _assert_segment_clicks(tf: RpcSubprocess) -> None:
    """(D) clicking a segment cycles it (named send wire)."""
    assert _q(tf, "mode") == "Rust"
    tf.click(path="statusbar#mode")
    wait_query(tf, "/external/mode", "Python", desc="click mode -> Python")
    tf.click(path="statusbar#mode")
    wait_query(tf, "/external/mode", "Markdown", desc="click mode -> Markdown")
    # Encoding segment is independent.
    assert _q(tf, "encoding") == "UTF-8"
    tf.click(path="statusbar#encoding")
    wait_query(tf, "/external/encoding", "UTF-16 LE", desc="click encoding -> UTF-16 LE")
    assert _q(tf, "mode") == "Markdown", "encoding click did not touch mode"
    # Two more encoding clicks wrap back to UTF-8 (3 encodings).
    tf.click(path="statusbar#encoding")
    wait_query(tf, "/external/encoding", "Latin-1", desc="click encoding -> Latin-1")
    tf.click(path="statusbar#encoding")
    wait_query(tf, "/external/encoding", "UTF-8", desc="encoding wraps to UTF-8")


def _assert_message(tf: RpcSubprocess) -> None:
    """(E) message round-trips."""
    assert _q(tf, "message") == ""
    tf.intervene("/external/message", "Saved 3 files")
    assert _q(tf, "message") == "Saved 3 files", "message set"
    tf.intervene("/external/message", "")
    assert _q(tf, "message") == "", "message cleared"


def _assert_errors(tf: RpcSubprocess) -> None:
    """(F) read-only axes + negative cursor rejection."""
    for ro in ("mode", "encoding", "position"):
        raised = False
        try:
            tf.intervene(f"/external/{ro}", "x")
        except RpcError:
            raised = True
        assert raised, f"{ro} must be read-only"
    # Negative line rejected.
    raised = False
    try:
        tf.intervene("/external/line", -5)
    except RpcError:
        raised = True
    assert raised, "negative line rejected"
    # Unknown window rejected.
    raised = False
    try:
        tf.request("scene/query", {"path": "/external/mode", "window": "ghost"})
    except RpcError as exc:
        raised = True
        assert exc.code == -32602
    assert raised, "unknown window rejected"


def body() -> None:
    with RpcSubprocess("hello-status-bar", boot_grace=1.5) as tf:
        _wait_ready(tf)
        _assert_boot(tf)            # (A)
        _assert_position(tf)        # (B)
        _assert_mode_cycle_rpc(tf)  # (C)
        _assert_segment_clicks(tf)  # (D)
        _assert_message(tf)         # (E)
        _assert_errors(tf)          # (F)


if __name__ == "__main__":
    sys.exit(run_demo("R913 editor status bar", body))
