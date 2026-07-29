#!/usr/bin/env python3
"""R1467 §5.16 §5.39 §2 #7 — the produce path dresses the window, on
`hello-window-chrome`.

A borderless window has no OS title bar, so pinion injects its own: a chrome
strip with real minimize / maximize / close nodes, and the content laid out
BELOW it. R1121 sells that as the reason client-side chrome beats OS chrome —
"the buttons are Scene nodes an AI agent observes and drives", where an OS
frame's controls are outside the tree entirely.

Three producers build a paint scene: the winit paint, the introspection mirror,
and the RPC dispatch's produce closure. Only the first two dressed the window.
The third — the one every path-addressed call resolves its coordinate through —
ran the bare view fn, so on a chromed window it produced:

  - no strip, so the controls R1121 promises were not addressable at all; and
  - no content INSET, because the inset rides the same policy read as the strip,
    so every rect it reported sat one strip-height too high.

Both halves are read here over the wire, on a real (hidden) window:

  - `scene/layout {viewport}` runs the producer by construction (that IS the
    hypothetical-size query). Its tree must carry the same chrome the STORED
    frame carries, at the same rects — the R1466 carry asked whether a
    viewport-specified read may differ from `from: paint`, and this is the
    answer: no.
  - `scene/click {path}` resolves through the producer as well
    (`resolve_path_to_center` runs it; only the viewport SIZE comes from the
    stored frame). So the demo ends by asking the agent to close the window by
    NAMING its close button. Pre-R1467 that call could not even be made: the tag
    was absent from the producer's scene and the request failed with
    `tag "…" not found in paint scene`.

ZERO-FLAKE: every assertion compares two values produced by the same run — a
producer tree against the stored frame's own layout — and nothing waits on
wall-clock or on pixels. The one timed wait is the app's own exit after it is
told to close, which is the assertion.

Run from the workspace root:
    cargo build -p hello-window-chrome --release
    python3 tools/demos/r1467_producer_dresses_the_window.py
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
    wait_until,
)

EXAMPLE = "hello-window-chrome"
WIN_W, WIN_H = 640, 420
CHROME_H = 32  # WindowChromeStyle::default().height_px
CONTENT_TAG = "content"

STRIP = "ai-overlay/window-chrome"
CONTROLS = [f"{STRIP}#{name}" for name in ("close", "maximize", "minimize", "grip")]
RESIZE = [
    f"ai-overlay/window-resize#{edge}"
    for edge in ("north", "south", "west", "east", "south-east")
]


def walk(node: Any):
    """Yield every node of a `scene/snapshot` or `scene/layout` tree."""
    if not isinstance(node, dict):
        return
    yield node
    for child in node.get("children") or []:
        yield from walk(child)


def tags(tree: Any) -> set[str]:
    return {t for n in walk(tree) if (t := n.get("tag"))}


def rect_of(tree: Any, tag: str) -> Optional[dict[str, int]]:
    for node in walk(tree):
        if node.get("tag") == tag:
            r = node["rect"]
            return {k: int(r[k]) for k in ("x", "y", "w", "h")}
    return None


def produced(tf: RpcSubprocess) -> Any:
    """The PRODUCER's tree: `scene/layout` with an explicit viewport runs the
    produce closure at that size (`viewport: None` would read the stored
    frame's layout cache instead)."""
    resp = tf.request(
        "scene/layout", {"viewport": {"width": WIN_W, "height": WIN_H}}
    )
    assert resp is not None
    return resp.result


def painted(tf: RpcSubprocess) -> Any:
    """The STORED frame's layout — what the window actually shows."""
    resp = tf.request("scene/layout", {})
    assert resp is not None
    return resp.result


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) premise: a chromed window that has painted ──────────────────
        def stored_frame() -> Any:
            snap = tf.snapshot(source="paint")
            # An un-painted window answers from the producer; the strip is in
            # both since R1467, so gate on the CONTENT the view fn emits.
            return snap if rect_of(snap, CONTENT_TAG) else None

        stored = wait_until(stored_frame, desc="the window paints and stores a frame")
        stored_tags = tags(stored)
        assert STRIP in stored_tags, f"A: the painted frame carries the strip: {stored_tags}"
        for tag in CONTROLS:
            assert tag in stored_tags, f"A: the painted frame carries {tag}"
        for tag in RESIZE:
            assert tag in stored_tags, f"A: the painted frame carries {tag}"
        content_painted = rect_of(stored, CONTENT_TAG)
        assert content_painted is not None, f"A: content is in the painted frame"
        assert_eq(
            content_painted["y"],
            CHROME_H,
            "A: THE PREMISE — the painted content is inset below the strip, so a "
            "producer that forgets the inset is off by exactly this much",
        )

        # ── (B) the producer dresses the same window ────────────────────────
        # `scene/layout {viewport}` is the hypothetical-size query, so it runs
        # the produce closure rather than reading the stored frame.
        prod = produced(tf)
        prod_tags = tags(prod)
        assert STRIP in prod_tags, f"★B: the PRODUCER carries the strip too: {prod_tags}"
        for tag in CONTROLS:
            assert tag in prod_tags, f"★B: the producer carries {tag}"
        for tag in RESIZE:
            assert tag in prod_tags, f"★B: the producer carries {tag}"
        content_produced = rect_of(prod, CONTENT_TAG)
        assert content_produced is not None, "★B: content is in the producer tree"
        assert_eq(
            content_produced["y"],
            CHROME_H,
            "★B: and the producer insets it below the strip — pre-R1467 this was "
            "0, and every coordinate derived here was that far above the widget",
        )

        # ── (C) …at the same rects, node for node ───────────────────────────
        # The R1466 carry asked whether `from: paint` (chromed) and a
        # viewport-specified read may legitimately differ. They may not: an
        # agent that sizes a window and an agent that reads it must be told the
        # same thing about it.
        live = painted(tf)
        for tag in [CONTENT_TAG, STRIP, *CONTROLS, *RESIZE]:
            assert_eq(
                rect_of(prod, tag),
                rect_of(live, tag),
                f"★C: producer and painted frame agree on {tag}",
            )

        # ── (D) so the controls are addressable BY NAME ─────────────────────
        # This is the half a user feels. `scene/hover {path}` resolves its
        # coordinate through the producer; before R1467 the tag was simply not
        # in that scene and the call failed.
        tf.hover(path=CONTROLS[0])
        tf.hover(path=CONTENT_TAG)
        for tag in CONTROLS[1:]:
            tf.hover(path=tag)

        # Negative control: resolution is real, not "always succeeds".
        try:
            tf.hover(path="ai-overlay/window-chrome#no-such-control")
        except RpcError as err:
            assert "not found in paint scene" in str(err), (
                f"D: an unknown tag is rejected by the same resolver: {err}"
            )
        else:
            raise AssertionError("D: an unknown tag must NOT resolve")

        # ── (E) the promise, exercised: close the window by naming it ───────
        # R1121's whole argument for client-side chrome is that its buttons are
        # in the scene tree where an agent can reach them. Reaching one through
        # the produce path is what R1467 made possible.
        tf.click(path=f"{STRIP}#close")
        code = tf.wait_self_exit(timeout=10.0)
        assert_eq(
            code,
            0,
            "★E: the app closed itself because the agent clicked the close "
            "button BY NAME — the R1121 promise, now true of the produce path",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1467 the produce path dresses the window", body))
