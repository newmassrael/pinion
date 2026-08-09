#!/usr/bin/env python3
"""R918 §5.38 §5.40 — node-editor Details panel in-panel click-to-edit.

Drives hello-node-editor over JSON-RPC. R916 gave the node graph a Details
panel that reflects the single selected node's properties; R918 makes every
panel row click-to-edit (the engine Details affordance). A click on a
`node_graph#detail_<key>` row routes to the coordinator (the palette precedent:
a view sibling carrying the primary's tag prefix) and opens the ONE shared
inline field IN THE PANEL ROW (surface = Panel) — the same field, focus, keymap,
blur-commit, and undo machinery the node card uses (R878 / R901), now with an
editing-SURFACE dimension so the card and the panel coexist. Position rows
(PosX / PosY) are new edit targets; a panel commit routes through the SAME
`apply_set_pos` funnel an `intervene node.<id>.{x,y}` uses, so a panel edit and
an RPC move are one undoable mutation (x then y coalesce into one step). Unlike
the card, the panel edits a WIRED port's default — the row always paints its
value, so the field has a painted anchor even for a pin the card hides.

  (A) select node 2 — the property rows paint.
  (B) click the Position X row — the field opens IN THE PANEL (surface=panel),
      seeded from node.x, owning focus; its rect sits in the right sidebar.
  (C) type + Enter — the node moves through the shared move funnel; a panel
      x then y edit coalesce into one undo step (the `intervene x/y` funnel).
  (D) the panel edits the Title row.
  (E) the panel edits a WIRED port default the card refuses.
  (F) `query editing` reports the kind + the `panel` surface for each.
  (G) Escape cancels without touching the value.

>= 30 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-node-editor"
G = "node_graph"
EDIT = "node_edit"  # the shared inline edit field's tag
UNDO = "/node_undo/external"
VIEWPORT = (964, 420)  # palette(132) + canvas(640) + details(192)
PANEL_X0 = 772  # palette + canvas: the Details panel starts here


def editing(tf):
    return tf.query("/external/editing")


def detail(tf, field):
    return tf.query(f"/external/detail.{field}")


def editor_text(tf):
    return tf.query(f"/{EDIT}/external/text")


def field_rect(tf):
    return abs_rects_of(tf.snapshot(source="paint", viewport=VIEWPORT)).get(EDIT)


def has_tag(tf, tag) -> bool:
    return find_by_tag(tf.snapshot(source="paint", viewport=VIEWPORT), tag) is not None


def retype(tf, new_text: str) -> None:
    """Erase the seeded value (caret parks at the end) and type a new one."""
    for _ in range(len(editor_text(tf))):
        tf.key(path=EDIT, name="Backspace")
    tf.text(new_text, path=EDIT)


def open_row(tf, key: str, expect: dict) -> None:
    """Click a Details-panel row and wait for its editor to open."""
    tf.click(path=f"{G}#detail_{key}")
    wait_until(
        lambda: editing(tf) == expect,
        timeout=4.0,
        interval=0.03,
        desc=f"clicking the {key} row opens its editor (surface=panel)",
    )
    wait_until(
        lambda: field_rect(tf) is not None,
        timeout=4.0,
        interval=0.03,
        desc=f"the shared field paints in the {key} panel row",
    )


def commit(tf, value: str) -> None:
    """Retype `value` into the open field and Enter-commit it."""
    retype(tf, value)
    wait_until(lambda: editor_text(tf) == value, timeout=4.0, interval=0.03,
               desc=f"keystrokes reach the field ({value})")
    tf.key(path=EDIT, name="Enter")
    wait_until(lambda: editing(tf) is None, timeout=4.0, interval=0.03,
               desc="Enter leaves edit mode")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) select node 2 (Multiply, at 250,110) — rows paint ────
        assert_eq(editing(tf), None, "boot: no edit in flight")
        tf.intervene("/external/selected_ids", "2")
        wait_until(lambda: detail(tf, "node") == 2, timeout=4.0, interval=0.03,
                   desc="node 2 becomes the single selection")
        assert has_tag(tf, f"{G}#detail_x"), "the Position X row paints"
        assert has_tag(tf, f"{G}#detail_title"), "the Title row paints"
        assert has_tag(tf, f"{G}#detail_in_0"), "the input-0 row paints"
        assert field_rect(tf) is None, "no inline field while idle"

        # ── (B) click Position X — the field opens IN THE PANEL ──────
        open_row(tf, "x", {"kind": "pos_x", "node": 2, "surface": "panel"})
        assert_eq(editor_text(tf), "250", "the field is seeded from node.x")
        assert_eq(tf.request("focus/get").result.get("focused"), EDIT,
                  "the panel field owns keyboard focus")
        rx = field_rect(tf)[0]
        assert rx >= PANEL_X0, f"the field paints in the Details panel (x={rx} >= {PANEL_X0}), not the canvas"
        # The selected node's card hosts no inline field (the card-surface gate
        # is closed while the panel hosts the edit).
        card = find_by_tag(tf.snapshot(source="paint", viewport=VIEWPORT), f"{G}#node_2")
        assert card is not None, "the node 2 card still paints"
        assert find_by_tag(card, EDIT) is None, "the field is not on the node card"

        # ── (C) type + Enter — move through the shared funnel ────────
        commit(tf, "400")
        assert_eq(detail(tf, "x"), 400, "the panel edit moved the node")
        assert_eq(tf.query("/external/node.2.x"), 400, "node.2.x == detail.x (the selection alias)")
        assert_eq(tf.query(f"{UNDO}/undo_label"), "Move node", "the panel move is one undoable step")
        assert field_rect(tf) is None, "the field unpaints after the commit"
        assert_eq(tf.request("focus/get").result.get("focused"), G, "focus returns to the canvas")
        # A panel y edit coalesces with the x edit: one undo reverts both axes,
        # exactly like `intervene x` then `intervene y` (the shared funnel).
        open_row(tf, "y", {"kind": "pos_y", "node": 2, "surface": "panel"})
        assert_eq(editor_text(tf), "110", "the y field is seeded from node.y")
        commit(tf, "180")
        assert_eq(detail(tf, "y"), 180, "the panel edit moved the node in y")
        assert_eq(tf.invoke(f"{UNDO}/undo", None), True, "undo the coalesced move")
        assert_eq(tf.query("/external/node.2.x"), 250, "x reverted")
        assert_eq(tf.query("/external/node.2.y"), 110, "y reverted in the SAME undo step (coalesced)")

        # ── (D) the panel edits the Title row ───────────────────────
        open_row(tf, "title", {"kind": "title", "node": 2, "surface": "panel"})
        assert_eq(editor_text(tf), "Multiply", "the title field is seeded from node.title")
        commit(tf, "Albedo")
        assert_eq(detail(tf, "title"), "Albedo", "the panel renamed the node")
        assert_eq(tf.query("/external/node.2.title"), "Albedo", "node.2.title == detail.title")

        # ── (E) the panel edits a WIRED port default the card refuses ─
        assert_eq(tf.invoke("/external/begin_edit_default", "2.0"), False,
                  "the card refuses a wired pin (its label is hidden)")
        open_row(tf, "in_0", {"kind": "port_default", "node": 2, "port": 0, "surface": "panel"})
        ri = field_rect(tf)[0]
        assert ri >= PANEL_X0, "the wired-port edit also paints in the panel"
        commit(tf, "#3366cc")
        d = tf.query("/external/node.2.input_default.0")
        assert_eq(d["r"], 0x33, "the typed hex parsed into the wired port's colour default (r)")
        assert_eq(d["b"], 0xCC, "the typed hex parsed into the wired port's colour default (b)")

        # ── (F) the RPC twin: begin_edit_detail opens the panel editor ─
        # (the AI-first surface symmetry — the panel inline editor a human
        # reaches by clicking is also RPC-reachable, like the card begins).
        assert_eq(tf.invoke("/external/begin_edit_detail", "x"), True,
                  "begin_edit_detail opens the panel editor over RPC")
        wait_until(lambda: editing(tf) == {"kind": "pos_x", "node": 2, "surface": "panel"},
                   timeout=4.0, interval=0.03, desc="the RPC-opened edit reads as the panel surface")
        assert_eq(tf.invoke("/external/begin_edit_detail", "bogus"), False, "an unknown field key is rejected")

        # ── (G) Escape cancels without touching the value ───────────
        retype(tf, "999")
        tf.key(path=EDIT, name="Escape")
        wait_until(lambda: editing(tf) is None, timeout=4.0, interval=0.03, desc="Escape leaves edit mode")
        assert_eq(detail(tf, "x"), 250, "a cancelled panel edit never touches the value")
        assert_eq(editing(tf), None, "no edit in flight after the cancel")


if __name__ == "__main__":
    sys.exit(run_demo("R918 §5.38 §5.40 — node Details panel in-panel edit", body))
