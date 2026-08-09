#!/usr/bin/env python3
"""R1416 §5.35 §5.15 — a raw multi-button pointer sink.

The forcing consumer for the new `External::wants_raw_pointer_buttons` seam (the
sprag PR-72 pointer-capture gap). A widget that owns the raw pointer stream —
`RawPointerSink`, standing in for a terminal-pane oracle — receives EVERY mouse
button (left / middle / right) on BOTH the press and release edge, each carrying
the held modifiers with the button identified, through the typed
`raw_pointer_button` method and the new single-edge `scene/pointer_button` RPC.

The three gaps the PR-72 report named are each closed here, over the wire:

  * RIGHT RELEASE — the shell had no right-release arm (`right:up` never fired);
    the sink now records it, so an SGR app never sees a stuck button.
  * PRESS MODIFIERS — the legacy PointerDown wire dropped press-edge modifiers;
    `right:down:s` carries the Shift on the DOWN edge.
  * BUTTON IDENTITY — the legacy wire named only PointerDown/PointerUp; every
    raw edge names its button.

And it proves the unified seam (`[[r47-class-incident-prevention]]`): the SAME
raw stream is reached by `scene/pointer_button`, by a GUI `scene/click`, and by
`scene/drag` — native and every RPC path share one `pointer_button_for_window`
dispatch, so an injected edge is indistinguishable from a physical one. The
non-capture invariant (every non-raw widget keeps left=focus / middle=paste /
right=context-menu) is covered by the router unit tests
(`r1416_non_raw_widget_is_not_a_raw_sink`) — a raw sink is the ONLY widget that
trades the GUI semantics for the raw edges.

Run from the workspace root:
    cargo build -p hello-raw-pointer --release
    python3 tools/demos/r1416_raw_pointer.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    RpcError,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_query,
    wait_snap,
)

WIN = (720, 440)
PANE = "pane"
EXT = "/external"

# Mirror of the binding's PANE_RECT = Rect::new(16, 48, WIN_W - 32, WIN_H - 104).
RECT_X, RECT_Y, RECT_W, RECT_H = 16, 48, WIN[0] - 32, WIN[1] - 104


def pane_at(fx: float, fy: float) -> tuple[float, float]:
    """The window pixel at fraction `(fx, fy)` across the pane rect.

    The sink surface covers the pane rect exactly, so the router's `x_rel` /
    `y_rel` equals `(fx, fy)` — what the sink stamps onto each report.
    """
    return (RECT_X + fx * RECT_W, RECT_Y + fy * RECT_H)


CENTER = pane_at(0.5, 0.5)


def assert_near(actual, expected: float, label: str, tol: float = 0.03) -> None:
    if actual is None or abs(float(actual) - expected) > tol:
        raise AssertionError(f"{label}: expected ~{expected}, got {actual}")
    print(f"  ok: {label} (~{expected})")


def readout(snap) -> str:
    node = find_by_tag(snap, "raw.readout")
    return node.get("content", "") if node else ""


def body() -> None:
    with RpcSubprocess("hello-raw-pointer") as tf:
        snap = wait_snap(
            tf,
            lambda s: find_by_tag(s, PANE) is not None,
            source="paint",
            viewport=WIN,
            desc="pane surface resolved",
        )

        # --- boot: nothing pressed, empty log ---
        assert_eq(tf.query(f"{EXT}/report_count"), 0, "boot: no reports")
        assert_eq(tf.query(f"{EXT}/last"), None, "boot: no last report")
        assert_eq(tf.query(f"{EXT}/log"), "", "boot: empty log")
        assert "press any mouse button" in readout(snap), "boot: the idle prompt"

        # --- the whole three-button, both-edge stream via scene/pointer_button.
        #     Each edge names its button and its edge (gaps A + C). ---
        count = 0
        for button in ("right", "middle", "left"):
            for edge in ("down", "up"):
                tf.pointer_button(button, edge, at=CENTER)
                count += 1
                wait_query(
                    tf,
                    f"{EXT}/report_count",
                    count,
                    desc=f"{button}:{edge} recorded (#{count})",
                )
                assert_eq(
                    tf.query(f"{EXT}/last"),
                    f"{button}:{edge}:",
                    f"last report is {button}:{edge}",
                )
        # Gap 1 explicitly: the RIGHT RELEASE edge was reached (no stuck button).
        assert "right:up:" in tf.query(f"{EXT}/log"), "the right RELEASE edge fired"
        # The full ordered sequence — one wire vocabulary for all six edges.
        assert_eq(
            tf.query(f"{EXT}/log"),
            "right:down:;right:up:;middle:down:;middle:up:;left:down:;left:up:",
            "the full six-edge sequence, in order",
        )
        # Button identity is distinct per edge (gap C).
        assert_eq(tf.query(f"{EXT}/last_button"), "left", "last_button is identified")
        assert_eq(tf.query(f"{EXT}/last_edge"), "up", "last_edge is identified")

        # --- press-edge modifiers (gap B): the legacy PointerDown wire dropped
        #     them; the raw stream carries them on the DOWN edge. ---
        tf.invoke(f"{EXT}/clear", None)
        wait_query(tf, f"{EXT}/report_count", 0, desc="log cleared for the modifier phase")

        # Every press is balanced by a release so the R1418 implicit grab lifts
        # between probes (a held button would grab the pointer — the toolkit
        # discipline).
        tf.modifiers(shift=True)
        tf.pointer_button("right", "down", at=CENTER)
        wait_query(tf, f"{EXT}/last", "right:down:s", desc="the right PRESS carries Shift")
        assert_eq(tf.query(f"{EXT}/last_mods"), "s", "last_mods reads the Shift token")
        tf.pointer_button("right", "up", at=CENTER)  # release (grab lifts)

        tf.modifiers(shift=True, ctrl=True)
        tf.pointer_button("left", "down", at=CENTER)
        wait_query(tf, f"{EXT}/last", "left:down:sc", desc="Shift+Ctrl on the left PRESS")

        tf.modifiers()  # release the modifiers — mirrors the key-up
        tf.pointer_button("left", "up", at=CENTER)
        wait_query(tf, f"{EXT}/last", "left:up:", desc="the release after clearing carries no mods")

        # --- position stamping: the sink correlates each edge with the live
        #     hover position (a pane oracle → terminal cell). ---
        tf.invoke(f"{EXT}/clear", None)
        wait_query(tf, f"{EXT}/report_count", 0, desc="log cleared for the position phase")
        tf.pointer_button("left", "down", at=pane_at(0.25, 0.75))
        wait_query(tf, f"{EXT}/report_count", 1, desc="a positioned press is recorded")
        assert_near(tf.query(f"{EXT}/last_x"), 0.25, "the report stamps the press x")
        assert_near(tf.query(f"{EXT}/last_y"), 0.75, "the report stamps the press y")
        # Release the button so the implicit grab (R1418) lifts — otherwise the
        # held press would grab the pointer and the hover section below could not
        # leave the pane (which is exactly the grab's job while a button is down).
        tf.pointer_button("left", "up", at=pane_at(0.25, 0.75))
        wait_query(tf, f"{EXT}/report_count", 2, desc="the release lifts the grab")

        # --- the live hover position tracks (wants_hover_move), and leaving the
        #     pane clears it while the recorded log survives (no button held). ---
        tf.hover(at=pane_at(0.6, 0.4))
        wait_snap(
            tf,
            lambda s: abs(float(tf.query(f"{EXT}/x_frac") or -1) - 0.6) < 0.03,
            source="paint",
            viewport=WIN,
            desc="a bare hover forwards the live position",
        )
        assert_near(tf.query(f"{EXT}/x_frac"), 0.6, "live hover x")
        assert_near(tf.query(f"{EXT}/y_frac"), 0.4, "live hover y")
        tf.hover(at=(360.0, 12.0))  # off the pane (above it)
        wait_query(tf, f"{EXT}/x_frac", None, desc="leaving the pane clears the live position")
        assert_eq(tf.query(f"{EXT}/report_count"), 2, "the recorded log survives a leave")

        # --- the unified seam: the SAME raw stream is reached by a GUI
        #     scene/click and by scene/drag, not only scene/pointer_button —
        #     native and every RPC click path share one dispatch. ---
        tf.invoke(f"{EXT}/clear", None)
        wait_query(tf, f"{EXT}/report_count", 0, desc="log cleared for the unified-seam phase")

        # scene/click {left} → the raw sink gets a full down+up pair.
        tf.click(at=CENTER)
        wait_query(tf, f"{EXT}/report_count", 2, desc="a GUI left-click reaches the raw stream")
        assert_eq(
            tf.query(f"{EXT}/log"),
            "left:down:;left:up:",
            "scene/click delivers the same raw down+up as pointer_button",
        )

        # scene/click {right} → the raw sink gets the press-edge one-shot (the
        # GUI context-menu arc has no release half), so balance it with an
        # explicit right-up to lift the grab the press opened.
        tf.click(at=CENTER, button="right")
        wait_query(tf, f"{EXT}/report_count", 3, desc="a GUI right-click reaches the raw stream")
        assert_eq(tf.query(f"{EXT}/last"), "right:down:", "the right GUI click is a raw press edge")
        tf.pointer_button("right", "up", at=CENTER)  # balance the press-only right-click
        wait_query(tf, f"{EXT}/report_count", 4, desc="release lifts the right grab")

        # scene/drag {middle, from == to} → the raw sink gets middle down+up.
        tf.drag(from_at=CENTER, to_at=CENTER, button="middle", steps=0)
        wait_query(tf, f"{EXT}/report_count", 6, desc="a middle drag reaches the raw stream")
        assert "middle:down:" in tf.query(f"{EXT}/log"), "the middle drag press edge is raw"
        assert "middle:up:" in tf.query(f"{EXT}/log"), "the middle drag release edge is raw"

        # --- R1418 IMPLICIT GRAB (the toolkit grabMouse): a press over the pane grabs, so
        #     a release that lands OUTSIDE the pane still pairs on the sink — no
        #     stuck button, the failure PR-72 named. ---
        tf.invoke(f"{EXT}/clear", None)
        wait_query(tf, f"{EXT}/report_count", 0, desc="log cleared for the grab phase")
        tf.pointer_button("left", "down", at=CENTER)
        wait_query(tf, f"{EXT}/report_count", 1, desc="a press over the pane opens the grab")
        # Release far off the pane rect (above-left of x=16, y=48).
        tf.pointer_button("left", "up", at=(5.0, 5.0))
        wait_query(tf, f"{EXT}/last", "left:up:", desc="the release paired on the sink off the pane")
        assert_eq(tf.query(f"{EXT}/report_count"), 2, "both edges recorded — no stuck button")

        # --- R1418 HELD-BUTTON SET (the toolkit buttons()): a chord reports
        #     which buttons are down, not only the one that changed. ---
        tf.invoke(f"{EXT}/clear", None)
        wait_query(tf, f"{EXT}/report_count", 0, desc="log cleared for the chord phase")
        tf.pointer_button("left", "down", at=CENTER)
        wait_query(tf, f"{EXT}/last_buttons", "l", desc="left down -> held {left}")
        tf.pointer_button("right", "down", at=CENTER)
        wait_query(tf, f"{EXT}/last_buttons", "lr", desc="right down while left held -> {left,right}")
        assert_eq(tf.query(f"{EXT}/last_button"), "right", "the single changed button is right")
        tf.pointer_button("left", "up", at=CENTER)
        wait_query(tf, f"{EXT}/last_buttons", "r", desc="left up -> held {right}")
        tf.pointer_button("right", "up", at=CENTER)
        wait_query(tf, f"{EXT}/last_buttons", "", desc="right up -> nothing held")

        # --- the wire contract: scene/pointer_button rejects a bad button /
        #     a missing edge (the vocabulary surfaces the typo at the call). ---
        try:
            tf.pointer_button("scroll", "down", at=CENTER)
            raise AssertionError("an unknown button must be rejected")
        except RpcError as exc:
            print(f"  ok: unknown button rejected ({exc.message!r})")
        try:
            tf.request("scene/pointer_button", {"at": {"x": CENTER[0], "y": CENTER[1]}, "button": "left"})
            raise AssertionError("a missing edge must be rejected")
        except RpcError as exc:
            print(f"  ok: missing edge rejected ({exc.message!r})")

        # --- recovery: a real edge after all that still records ---
        tf.invoke(f"{EXT}/clear", None)
        tf.pointer_button("middle", "down", at=CENTER)
        wait_query(tf, f"{EXT}/last", "middle:down:", desc="a real edge recovers after the error probes")


if __name__ == "__main__":
    sys.exit(run_demo("r1416_raw_pointer", body))
