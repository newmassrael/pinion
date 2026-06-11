#!/usr/bin/env python3
"""R887 §5.49 §5.53 — `scene/click {button: "right"}`: the right-click wire.

Pre-R887 a physical right-click reached `apply_secondary_click` on any
binding, but the RPC surface had no secondary-button arc at all — an AI
client could only open `hello-contextmenu`'s popup through the
binding-specific `invoke("open_at", ...)` action the example hand-wired
(the r772 demo's documented asymmetry: "the native secondary-button path
is verified live by XTEST separately"). That is a §2 invariant #2 gap
(every input a human makes has an RPC peer), carried since R881.1.

R887 closes it on the *existing* wire: `scene/click` gains an optional
`button` axis (`left` default | `right`; `middle` rejected with a
redirect to `scene/drag`, which owns the moved/in-place judgement). A
right click enqueues `DeferredInput::SecondaryClick` and the embedder
drains it through the exact physical arc — cursor seed, then the
press-edge one-shot `secondary_click_for_window` (W3C `contextmenu`
fires on press; no release half).

Verification scope (35 assertions, counted exactly — gates per
[[zero-flake-policy]]: every action→assert edge is wait_query /
wait_until on observed state, no fixed sleeps):

  (A) boot — popup closed, no panel/barrier paint, active none.       (5)
  (B) right-click at a coordinate opens the popup AT the press point
      over the universal input arc (no open_at invoke anywhere in
      this demo); panel + barrier paint; pointer-open leaves the
      highlight clear; press-edge one-shot fires no command.          (9)
  (C) R885 cross-check — `scene/input_state.cursor` reports the
      right-click press point (the cursor-seed leg is observable).    (2)
  (D) the opened popup is live: hover highlights, a plain left click
      on an item activates it (command intent) and closes.            (5)
  (E) path-form right-click — `{path, button:"right"}` re-anchors at
      the resolved node centre (shared at/path selector taxonomy).    (4)
  (F) re-anchor clears the highlight; barrier left-click dismisses.   (4)
  (G) wire vocabulary — "middle" rejected with a scene/drag redirect,
      typo / non-string rejected, all side-effect-free.               (6)
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_query,
    wait_until,
)

VIEWPORT = (520, 360)
PRESS = (200.0, 150.0)


def _paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def _wait_paint(tf, predicate, desc):
    return wait_until(
        lambda: (lambda s: s if predicate(s) else None)(_paint(tf)),
        desc=desc,
    )


def _no_command_logged(tf) -> None:
    tail = tf.stderr_tail(120)
    assert not any("ctx.command" in ln for ln in tail), (
        f"press-edge one-shot must not activate an item; stderr={tail[-6:]!r}"
    )


def body() -> None:
    with RpcSubprocess("hello-contextmenu", boot_grace=1.5) as tf:
        # ── (A) boot: closed ─────────────────────────────────────────
        snap = _paint(tf)
        assert find_by_tag(snap, "ctx") is None, "no popup panel while closed"          # 1
        assert find_by_tag(snap, "ctx#barrier") is None, "no barrier while closed"      # 2
        assert_eq(tf.query("/external/open"), False, "boot: open = false")              # 3
        assert_eq(tf.query("/external/active"), None, "boot: active = none")            # 4
        r0 = tf.request("scene/input_state", {}).result
        assert_eq(r0["cursor"], None, "boot: no cursor event landed yet")               # 5

        # ── (B) right-click opens at the press point (universal arc) ─
        tf.click(at=PRESS, button="right")
        wait_query(tf, "/external/open", True, desc="right-click opens the popup")      # 6
        assert_eq(tf.query("/external/open_x"), PRESS[0], "anchored at press x")        # 7
        assert_eq(tf.query("/external/open_y"), PRESS[1], "anchored at press y")        # 8
        assert_eq(
            tf.query("/external/active"), None, "pointer open pre-highlights nothing"
        )                                                                               # 9
        snap = _wait_paint(tf, lambda s: find_by_tag(s, "ctx") is not None,
                           "popup paints after right-click")                            # 10
        px, py, _pw, _ph = abs_rects_of(snap)["ctx"]
        assert (px, py) == (200, 150), f"panel rect at press point; got ({px},{py})"    # 11
        assert find_by_tag(snap, "ctx#barrier") is not None, "barrier paints"           # 12
        assert find_by_tag(snap, "ctx#i0") is not None, "item rows paint"               # 13
        # Press-edge one-shot: a right-click has no release half, so the
        # open must NOT be followed by any item activation.
        _no_command_logged(tf)                                                          # 14

        # ── (C) R885 read-peer cross-check: cursor followed the press ─
        r1 = tf.request("scene/input_state", {}).result
        assert_eq(r1["cursor"], {"x": PRESS[0], "y": PRESS[1]},
                  "input_state.cursor reports the right-click press point")             # 15
        assert_eq(r1["held_keys"], [], "right-click holds no chord key")                # 16

        # ── (D) the popup opened by the wire is fully live ───────────
        tf.hover(path="ctx#i1")
        wait_query(tf, "/external/active", 1, desc="hover highlights item 1")           # 17
        assert_eq(tf.query("/external/open"), True, "hover does not activate")          # 18
        tf.click(path="ctx#i1")
        wait_query(tf, "/external/open", False, desc="left-click activates + closes")   # 19
        tail = tf.stderr_tail(80)
        assert any('shell: intent ctx.command payload=Text("1")' in ln for ln in tail), (
            f"item activation logs the command intent; stderr={tail[-6:]!r}"
        )                                                                               # 20
        # The left click parked the OS-less cursor over the popup region;
        # clear it so reopened popups start hover-free (mirrors r772).
        tf.pointer_leave()

        # ── (E) path-form right-click re-anchors at the node centre ──
        tf.click(at=(60.0, 40.0), button="right")
        wait_query(tf, "/external/open", True, desc="reopen at (60,40)")                # 21
        snap = _wait_paint(tf, lambda s: find_by_tag(s, "ctx#i0") is not None,
                           "items paint after reopen")                                  # 22
        ix, iy, iw, ih = abs_rects_of(snap)["ctx#i0"]
        cx, cy = ix + iw // 2, iy + ih // 2
        tf.click(path="ctx#i0", button="right")
        wait_query(tf, "/external/open_x", float(cx),
                   desc="path-form right-click re-anchors at item centre x")            # 23
        assert_eq(tf.query("/external/open_y"), float(cy), "re-anchor y = node centre") # 24

        # ── (F) re-anchor cleared highlight; barrier click dismisses ─
        assert_eq(tf.query("/external/active"), None, "re-anchor clears highlight")     # 25
        assert_eq(tf.query("/external/open"), True, "still open before barrier")        # 26
        tf.pointer_leave()
        tf.click(at=(5.0, 5.0))
        wait_query(tf, "/external/open", False,
                   desc="left-click outside (barrier) dismisses")                       # 27
        snap = _wait_paint(tf, lambda s: find_by_tag(s, "ctx") is None,
                           "panel unpaints after dismiss")                              # 28

        # ── (G) wire vocabulary edges (all side-effect-free) ─────────
        # Command-count baseline: the (D) activation legitimately logged
        # one `ctx.command`; the rejections below must not add another.
        commands_before = sum(
            "ctx.command" in ln for ln in tf.stderr_tail(10_000)
        )
        try:
            tf.click(at=PRESS, button="middle")
            raise AssertionError("middle button must be rejected on scene/click")
        except RpcError as err:
            assert_eq(err.code, -32602, "middle button -> invalid params")              # 29
            assert "scene/drag" in str(err.data), (
                f"rejection redirects to the owning method; data={err.data!r}"
            )                                                                           # 30
        try:
            tf.click(at=PRESS, button="rigth")
            raise AssertionError("typo button must be rejected")
        except RpcError as err:
            assert_eq(err.code, -32602, "typo button -> invalid params")                # 31
        try:
            tf.request("scene/click", {"at": {"x": 1.0, "y": 2.0}, "button": 2})
            raise AssertionError("non-string button must be rejected")
        except RpcError as err:
            assert_eq(err.code, -32602, "non-string button -> invalid params")          # 32
        assert_eq(tf.query("/external/open"), False,
                  "rejected requests enqueue nothing (popup stays closed)")             # 33
        r2 = tf.request("scene/input_state", {}).result
        assert_eq(r2["cursor"], {"x": 5.0, "y": 5.0},
                  "rejected requests do not move the cursor cache")                     # 34
        commands_after = sum(
            "ctx.command" in ln for ln in tf.stderr_tail(10_000)
        )
        assert_eq(commands_after, commands_before,
                  "vocabulary rejections fire no commands")                             # 35


if __name__ == "__main__":
    sys.exit(run_demo("R887 §5.49 §5.53 — scene/click button:right (secondary-click wire)", body))
