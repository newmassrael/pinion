#!/usr/bin/env python3
"""R880 §5.35 §5.38 §5.49 — node-editor marquee multi-select gesture.

Drives `hello-node-editor` via JSON-RPC. The R879 selection *model* gains
its *gesture*: an LMB background-drag sweeps a rubber-band marquee whose
rect-hit node set lands on the selection at release. The release modifiers
pick the area form of the click policy (one shared decode): plain
*replaces* (an empty sweep clears — the background-click deselect
generalised to an area), `Ctrl` *toggles* each hit member, `Shift`
*unions*. The framework axis behind it: the bare (non-composite) send wire
carries the R781 modifier token as `":PointerUp:<token>"` for an External
that opts in via `wants_bare_send_modifiers` (`scene/modifiers` +
`scene/drag` therefore drive a Ctrl-marquee with zero new RPC surface),
and the click-vs-drag dead zone is the lifted `pinion_core::DragLatch`
contract predicate (router + node drag + marquee all advance the same
latch). Plus `Ctrl`+`A` / `invoke select_all`.

  (A) boot — empty selection; `select_all` in the schema.
  (B) plain marquee over two nodes replaces the selection with the pair.
  (C) marquee selection is not an undo step (selection is transient).
  (D) an empty sweep clears.
  (E) Ctrl-marquee toggles membership (in AND out, one sweep);
      Cmd/meta is a command chord too (R880.1 command_key).
  (F) Shift-marquee unions the hit set in.
  (G) an in-place background click still edge-selects (the R839 probe).
  (H) a *moved* background gesture never consumes the edge probe.
  (I) a marquee-built selection group-drags rigidly (R879 regression).
  (J) Ctrl+A selects every node; Escape clears; the status line counts.
  (K) `invoke select_all` is the AI-first twin.

>= 30 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-node-editor"
G = "node_graph"
VIEWPORT = (772, 420)
PALETTE_W = 132  # canvas x-offset inside the window


def W(gx: float, gy: float) -> tuple[float, float]:
    """Graph units -> window-absolute logical px (zoom 1, pan 0)."""
    return (PALETTE_W + gx, gy)


def ids(tf) -> str:
    return tf.query("/external/selected_ids")


def marquee(tf, frm: tuple[float, float], to: tuple[float, float], *,
            ctrl: bool = False, shift: bool = False, meta: bool = False) -> None:
    """A background sweep in graph coordinates, optionally modified."""
    if ctrl or shift or meta:
        tf.modifiers(ctrl=ctrl, shift=shift, meta=meta)
    tf.drag(from_at=W(*frm), to_at=W(*to))
    if ctrl or shift or meta:
        tf.modifiers()


def texts_of(snap) -> list[str]:
    """Every Scene::Text content under the paint snapshot."""
    out: list[str] = []

    def walk(node) -> None:
        if isinstance(node, dict):
            content = node.get("content")
            if isinstance(content, str):
                out.append(content)
            for v in node.values():
                walk(v)
        elif isinstance(node, list):
            for v in node:
                walk(v)

    walk(snap)
    return out


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot ────────────────────────────────────────────────
        assert_eq(ids(tf), "", "boot: empty selected_ids")
        assert_eq(tf.query("/external/selected"), None, "boot: no single selection")
        assert_eq(tf.query("/external/selected_edge"), None, "boot: no edge selection")
        schema = tf.query("/external/$schema")
        slots = {entry["path"] for entry in schema}
        assert "select_all" in slots, f"select_all advertised, got {sorted(slots)}"
        assert_eq(tf.query("/node_undo/external/undo_label"), None,
                  "boot: clean undo history")

        # ── (B) plain marquee replaces with the rect-hit set ────────
        # Sweep (20,50)->(200,260): nodes 0 (40,70) and 1 (40,210) are
        # inside; nodes 2 (x 250) and 3 (x 470) are not.
        marquee(tf, (20, 50), (200, 260))
        wait_until(lambda: ids(tf) == "0,1", timeout=4.0, interval=0.03,
                   desc="plain marquee selects the swept pair")
        assert_eq(tf.query("/external/selected"), None,
                  "a marquee set has no single `selected`")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert "#marquee" not in str(snap), "rubber band cleared from the paint after release"

        # ── (C) selection is transient — never journaled ────────────
        assert_eq(tf.query("/node_undo/external/undo_label"), None,
                  "the marquee did not journal an undo step")

        # ── (D) an empty sweep clears ───────────────────────────────
        marquee(tf, (610, 330), (700, 400))
        wait_until(lambda: ids(tf) == "", timeout=4.0, interval=0.03,
                   desc="empty sweep clears the selection")

        # ── (E) Ctrl-marquee toggles ────────────────────────────────
        marquee(tf, (20, 50), (200, 260))
        wait_until(lambda: ids(tf) == "0,1", timeout=4.0, interval=0.03,
                   desc="re-arm the pair")
        # Sweep y 145..260 covers node 1 (210..278) + node 2 (110..206)
        # but not node 0 (70..138): 1 toggles out, 2 toggles in.
        marquee(tf, (30, 145), (390, 260), ctrl=True)
        wait_until(lambda: ids(tf) == "0,2", timeout=4.0, interval=0.03,
                   desc="Ctrl-marquee toggles membership")

        # ── (E2) Cmd/meta is a command chord too (R880.1 command_key) ──
        marquee(tf, (460, 140), (620, 230), meta=True)
        wait_until(lambda: ids(tf) == "0,2,3", timeout=4.0, interval=0.03,
                   desc="meta-marquee toggles node 3 in (macOS Cmd)")
        marquee(tf, (460, 140), (620, 230), meta=True)
        wait_until(lambda: ids(tf) == "0,2", timeout=4.0, interval=0.03,
                   desc="meta-marquee toggles node 3 back out")

        # ── (F) Shift-marquee unions ────────────────────────────────
        marquee(tf, (460, 140), (620, 230), shift=True)
        wait_until(lambda: ids(tf) == "0,2,3", timeout=4.0, interval=0.03,
                   desc="Shift-marquee unions node 3 in")

        # ── (G) in-place background click still edge-selects ────────
        tf.click(at=W(210, 134))  # edge 0's midpoint
        wait_until(lambda: tf.query("/external/selected_edge") == 0,
                   timeout=4.0, interval=0.03,
                   desc="background click on the wire selects it")
        assert_eq(ids(tf), "", "edge selection cleared the node set (sum type)")

        # ── (H) a moved gesture never consumes the edge probe ───────
        # Press ON the wire, sweep away: the marquee applies (rect hits
        # node 2 only), the edge stays unselected.
        marquee(tf, (210, 134), (600, 30))
        wait_until(lambda: ids(tf) == "2", timeout=4.0, interval=0.03,
                   desc="moved gesture applies the marquee, not the edge click")
        assert_eq(tf.query("/external/selected_edge"), None,
                  "the press-point wire was not selected")

        # ── (I) marquee-built selection group-drags rigidly ─────────
        tf.intervene("/external/selected_ids", "0,1")
        x0, y0 = tf.query("/external/node.0.x"), tf.query("/external/node.0.y")
        x1, y1 = tf.query("/external/node.1.x"), tf.query("/external/node.1.y")
        tf.drag(from_at=W(x0 + 65, y0 + 15), to_at=W(x0 + 105, y0 + 35))
        wait_until(lambda: tf.query("/external/node.0.x") == x0 + 40,
                   timeout=4.0, interval=0.03, desc="grabbed member moved +40")
        assert_eq(tf.query("/external/node.0.y"), y0 + 20, "grabbed member moved +20")
        assert_eq((tf.query("/external/node.1.x"), tf.query("/external/node.1.y")),
                  (x1 + 40, y1 + 20), "the other member moved rigidly")
        assert_eq(ids(tf), "0,1", "the moved release kept the set intact")

        # ── (J) Ctrl+A + Escape + status line ───────────────────────
        tf.modifiers(ctrl=True)
        tf.key(path=G, name="a")
        tf.modifiers()
        wait_until(lambda: ids(tf) == "0,1,2,3", timeout=4.0, interval=0.03,
                   desc="Ctrl+A selects every node")
        assert_eq(tf.query("/external/selected"), None,
                  "the full set has no single `selected`")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        status = [t for t in texts_of(snap) if "selected:" in t]
        assert status and "selected: 4 nodes" in status[0], \
            f"status line reports the full set, got {status}"
        assert_eq(tf.query("/node_undo/external/undo_label"), "Move 2 nodes",
                  "selection gestures stacked nothing on the journal "
                  "(scene I's group move is still the top entry)")
        tf.key(path=G, name="Escape")
        wait_until(lambda: ids(tf) == "", timeout=4.0, interval=0.03,
                   desc="Escape clears the set")

        # ── (K) the AI-first invoke twin ────────────────────────────
        assert_eq(tf.invoke("/external/select_all", None), True, "invoke twin")
        assert_eq(ids(tf), "0,1,2,3", "every node selected via RPC")
        assert_eq(tf.invoke("/external/select_all", None), True,
                  "select_all is idempotent on a full selection")


if __name__ == "__main__":
    sys.exit(run_demo("r880_node_marquee_select", body))
