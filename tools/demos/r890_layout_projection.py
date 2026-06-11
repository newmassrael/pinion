#!/usr/bin/env python3
"""R890 §5.12 §5.16 — scene/layout = per-window paint-scene projection.

R889's carry: `scene/layout {viewport: null}` was served from caches that
could alias one window's geometry onto another — a binding-wide
last-writer-wins mirror on the substrate (any window's paint or dispatch
finalize overwrote it) plus a per-slot copy whose absence fell back to
that mirror (a known-but-unpainted window answered with the PRIMARY's
tree). R890 deletes both caches: the per-window paint scene the publish
primitives already store (the R705 `snapshot from:paint` SSOT) is the ONE
layout source, projected into a LayoutNode on demand at the AI-paced read
(`ShellCore::last_paint_layout_for_window`). A never-painted window now
answers the honest `NoLastPaintLayout`; a painted one can only ever
answer with its own frame. Bonus: the per-winit-frame LayoutNode build
(O(painted tree) every frame, consumed only by RPC) is gone, and the
TUI's `""`-prefixed mirror is retired so layout paths are `"/0"`-rooted
on both backends (§2 #6 parity, pinned in rpc_ingress).

Verification scope (32 assertions, counted exactly; gates per
[[zero-flake-policy]] — action→assert edges poll observed state):

  (A) per-window truth — main (320x200, has main_btn) and inspector
      (480x320, no main_btn) each answer viewport:null with their OWN
      frame, canonical "/0" root path, full rect shape.              (10)
  (B) interleaved stability — alternating reads never leak the other
      window's tree (the last-writer-wins failure mode).              (6)
  (C) mutation survives per-window — a click routed to main re-renders
      both windows (re-store loop); each keeps its own geometry and
      the inspector mirrors the new state.                            (7)
  (D) projection unification — viewport-supplied and viewport:null
      reads share dims + path shape (one projection home).           (5)
  (E) R889 gate composition — unknown window still rejected; known
      windows unaffected after the rejected read.                     (4)
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
    wait_snap,
    wait_until,
)

MAIN_SIZE = (320, 200)
INSPECTOR_SIZE = (480, 320)


def _layout(
    tf: RpcSubprocess,
    window: str,
    viewport: Optional[tuple[int, int]] = None,
) -> dict[str, Any]:
    params: dict[str, Any] = {"window": window}
    if viewport is not None:
        params["viewport"] = {"width": viewport[0], "height": viewport[1]}
    resp = tf.request("scene/layout", params)
    assert resp is not None and isinstance(resp.result, dict)
    return resp.result


def _has_tag(node: dict[str, Any], tag: str) -> bool:
    if node.get("tag") == tag:
        return True
    return any(_has_tag(c, tag) for c in node.get("children", []))


def _all_paths_rooted(node: dict[str, Any]) -> bool:
    if not str(node.get("path", "")).startswith("/0"):
        return False
    return all(_all_paths_rooted(c) for c in node.get("children", []))


def _walk_for_text(node: Any, needle: str) -> bool:
    if not isinstance(node, dict):
        return False
    content = node.get("content")
    if isinstance(content, str) and needle in content:
        return True
    if isinstance(content, dict) and _walk_for_text(content, needle):
        return True
    children = node.get("children")
    if isinstance(children, list):
        return any(_walk_for_text(c, needle) for c in children)
    return False


def body() -> None:
    with RpcSubprocess("hello-multi-window", boot_grace=1.5) as tf:
        # Both windows must have painted once before viewport:null
        # answers (the projection reads the stored paint scene).
        wait_until(lambda: _layout(tf, "main")["rect"]["w"] == MAIN_SIZE[0],
                   desc="main first paint stored")                                      # 1
        wait_until(lambda: _layout(tf, "inspector")["rect"]["w"] == INSPECTOR_SIZE[0],
                   desc="inspector first paint stored")                                 # 2

        # ── (A) per-window truth ─────────────────────────────────────
        lm = _layout(tf, "main")
        assert_eq(lm["path"], "/0", "main root path is canonical /0")                   # 3
        assert_eq((lm["rect"]["w"], lm["rect"]["h"]), MAIN_SIZE,
                  "main viewport:null = main's own frame")                              # 4
        assert_eq(sorted(lm["rect"].keys()), ["h", "w", "x", "y"],
                  "root rect carries the full shape")                                   # 5
        assert _has_tag(lm, "main_btn"), "main tree carries main_btn"                   # 6
        assert _all_paths_rooted(lm), "every main node path is /0-rooted"               # 7
        li = _layout(tf, "inspector")
        assert_eq(li["path"], "/0", "inspector root path is canonical /0")              # 8
        assert_eq((li["rect"]["w"], li["rect"]["h"]), INSPECTOR_SIZE,
                  "inspector viewport:null = inspector's own frame")                    # 9
        assert not _has_tag(li, "main_btn"), "inspector tree has NO main_btn"           # 10

        # ── (B) interleaved stability (no last-writer leak) ──────────
        lm2 = _layout(tf, "main")
        assert_eq((lm2["rect"]["w"], lm2["rect"]["h"]), MAIN_SIZE,
                  "main keeps its frame after an inspector read")                       # 11
        assert _has_tag(lm2, "main_btn"), "main keeps its own tree"                     # 12
        li2 = _layout(tf, "inspector")
        assert_eq((li2["rect"]["w"], li2["rect"]["h"]), INSPECTOR_SIZE,
                  "inspector keeps its frame after a main read")                        # 13
        assert not _has_tag(li2, "main_btn"), "inspector never inherits main's tree"    # 14
        assert_eq(lm2, lm, "back-to-back main reads identical (read-only)")             # 15
        assert_eq(li2, li, "back-to-back inspector reads identical")                    # 16

        # ── (C) mutation survives per-window ─────────────────────────
        tf.click(path="main_btn")
        wait_snap(tf, lambda s: _walk_for_text(s, "Hover"),
                  viewport=INSPECTOR_SIZE, window="inspector",
                  desc="inspector mirrors the post-click state")                        # 17
        lm3 = _layout(tf, "main")
        assert_eq((lm3["rect"]["w"], lm3["rect"]["h"]), MAIN_SIZE,
                  "main keeps its frame across the mutation")                           # 18
        assert _has_tag(lm3, "main_btn"), "main tree intact post-click"                 # 19
        li3 = _layout(tf, "inspector")
        assert_eq((li3["rect"]["w"], li3["rect"]["h"]), INSPECTOR_SIZE,
                  "inspector keeps its frame across the mutation")                      # 20
        assert not _has_tag(li3, "main_btn"), "no cross-window leak post-mutation"      # 21
        assert _all_paths_rooted(li3), "inspector paths stay /0-rooted"                 # 22
        assert_eq(li3["path"], "/0", "projection shape stable across repaints")         # 23

        # ── (D) one projection home: supplied vs null parity ─────────
        lm_v = _layout(tf, "main", viewport=MAIN_SIZE)
        assert_eq((lm_v["rect"]["w"], lm_v["rect"]["h"]),
                  (lm3["rect"]["w"], lm3["rect"]["h"]),
                  "viewport-supplied dims match viewport:null")                         # 24
        assert_eq(lm_v["path"], "/0", "supplied-viewport read shares the /0 shape")     # 25
        li_v = _layout(tf, "inspector", viewport=INSPECTOR_SIZE)
        assert_eq((li_v["rect"]["w"], li_v["rect"]["h"]),
                  (li3["rect"]["w"], li3["rect"]["h"]),
                  "inspector supplied vs null parity")                                  # 26
        assert _has_tag(lm_v, "main_btn"), "supplied-viewport tree = same window tree"  # 27
        assert not _has_tag(li_v, "main_btn"), "supplied inspector tree stays its own"  # 28

        # ── (E) R889 gate composition ────────────────────────────────
        try:
            _layout(tf, "bogus")
            raise AssertionError("unknown window must be rejected")
        except RpcError as err:
            assert_eq((err.code, err.message), (-32602, "unknown_window"),
                      "layout on unknown window rejected by the R889 gate")             # 29
            assert_eq(err.data, "bogus", "error data names the supplied id")            # 30
        lm4 = _layout(tf, "main")
        assert_eq((lm4["rect"]["w"], lm4["rect"]["h"]), MAIN_SIZE,
                  "main unaffected after the rejected read")                            # 31
        li4 = _layout(tf, "inspector")
        assert_eq((li4["rect"]["w"], li4["rect"]["h"]), INSPECTOR_SIZE,
                  "inspector unaffected after the rejected read")                       # 32


if __name__ == "__main__":
    run_demo("r890_layout_projection", body)
