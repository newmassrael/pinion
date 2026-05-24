#!/usr/bin/env python3
"""todomvc R659 §5.16 §5.45 — filter axis + scrollbar peer wire RPC
verification.

R659 extends R658 with three new axes:

  1. **`FilterMode`** enum (Active / Completed / All) — single-valued
     reactive Signal owned by `TodoFilterExternal` (3rd
     `ExtraExternal` after delete + toggle).
  2. **Segmented filter button row** — 3 buttons tagged
     `todo_filter#0..2`. R51.42 §5.35 composite-tag wire routes
     clicks to the singleton External; the engaged segment uses
     `ColorRole::Accent` fill so the filter is visually obvious.
     `entries.filter(mode.matches)` derives the visible list.
  3. **Visible scrollbar peer** — `pinion_widget_paint::scrollbar`
     substrate (2nd-consumer lift triggered by this round) +
     `ScrollBarExternal` as 4th `ExtraExternal`. Drag-able under
     `InputRouter` capture lock.

R656 carry: `scene/invoke v0` reaches the **primary** External only;
sibling Externals (delete / toggle / filter / scrollbar) are reachable
only through the paint-side composite-tag wire (`scene/click` on the
matching tag). The demo therefore verifies filter state via paint-
snapshot observation (which button has Accent fill, which rows are
visible) rather than direct RPC introspect on the External.

Driven sequence (≥ 25 typed assertions, AI-first introspection
mandate per `[[ai-first-rpc-introspection-obligation]]`):

  1. Confirm initial filter mode (paint: All button has Accent fill).
  2. Confirm scrollbar peer tag + filter row tag + 3 filter buttons.
  3. Add 5 items, toggle 3 to complete via paint clicks.
  4. Click Active filter → 2 visible + header `Active: 2 of 5`.
  5. Click Completed filter → 3 visible + header `Completed: 3 of 5`.
  6. Click All filter → 5 visible + header back to pre-filter shape.
  7. Verify each filter click switches the Accent fill to the
     matching button (mutual-exclusion contract through paint).
  8. Add 5 more items → 10 total → confirm scroll engaged + scrollbar
     peer width = 8 px (M3 default).
  9. R658 stable-id invariant survives the filter cycle.
"""

from __future__ import annotations

import re
import sys
import time
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, run_demo

TF_TAG = "main_textfield"
LIST_TAG = "todo_list"
LIST_SCROLL_KEY = "todomvc.list_scroll"
FILTER_TAG = "todo_filter"
SCROLLBAR_TAG = "todo_scrollbar"
ITEM_TAG_RE = re.compile(r"^todo_item#(\d+)$")
TOGGLE_GLYPH_UNCHECKED = "☐"
TOGGLE_GLYPH_CHECKED = "☑"


def focus_set(tf: RpcSubprocess, tag: str | None) -> None:
    tf.request("focus/set", {"tag": tag})


def type_text(tf: RpcSubprocess, text: str) -> None:
    for ch in text:
        result = tf.invoke("/external/key", ch)
        assert_eq(result, True, f"invoke('key', {ch!r}) recognized")
    time.sleep(0.05)


def submit_enter(tf: RpcSubprocess) -> None:
    tf.key(path=TF_TAG, name="Enter")
    time.sleep(0.1)


def find_node_by_tag(node: dict[str, Any], tag: str) -> dict[str, Any] | None:
    if not isinstance(node, dict):
        return None
    if node.get("tag") == tag:
        return node
    for child in node.get("children") or []:
        found = find_node_by_tag(child, tag)
        if found is not None:
            return found
    content = node.get("content")
    if isinstance(content, dict):
        found = find_node_by_tag(content, tag)
        if found is not None:
            return found
    return None


def find_scroll_with_content_tag(
    node: dict[str, Any], inner_tag: str
) -> dict[str, Any] | None:
    if not isinstance(node, dict):
        return None
    if node.get("type") == "Scroll":
        content = node.get("content")
        if isinstance(content, dict) and find_node_by_tag(content, inner_tag):
            return node
    for child in node.get("children") or []:
        hit = find_scroll_with_content_tag(child, inner_tag)
        if hit is not None:
            return hit
    return None


def list_rows(tf: RpcSubprocess) -> list[dict[str, Any]]:
    snap = tf.snapshot(source="paint", viewport=(480, 720))
    list_node = find_node_by_tag(snap, LIST_TAG)
    assert list_node is not None, f"snapshot must carry {LIST_TAG} tag"
    return list(list_node.get("children") or [])[1:]


def visible_count(tf: RpcSubprocess) -> int:
    return sum(
        1 for row in list_rows(tf)
        if isinstance(row.get("tag"), str) and ITEM_TAG_RE.match(row["tag"])
    )


def list_header_text(tf: RpcSubprocess) -> str:
    snap = tf.snapshot(source="paint", viewport=(480, 720))
    list_node = find_node_by_tag(snap, LIST_TAG)
    assert list_node is not None
    for ch in list_node.get("children") or []:
        if ch.get("type") == "Text":
            return ch.get("content") or ""
    raise AssertionError("list region missing header Text node")


def get_item_id(tf: RpcSubprocess, index: int) -> int:
    rows = list_rows(tf)
    tag = rows[index].get("tag")
    m = ITEM_TAG_RE.match(tag)
    assert m is not None, f"row {index} tag {tag!r} malformed"
    return int(m.group(1))


def filter_button_fill(
    tf: RpcSubprocess, idx: int
) -> tuple[int, int, int, int]:
    """Return the RGBA fill tuple of filter button `idx`. The active
    button uses ColorRole::Accent; inactive buttons are transparent.
    """
    snap = tf.snapshot(source="paint", viewport=(480, 720))
    btn = find_node_by_tag(snap, f"todo_filter#{idx}")
    assert btn is not None, f"todo_filter#{idx} missing from paint scene"
    style = btn.get("style") or {}
    fill = style.get("fill") or {}
    # Color serializes as {"r": N, "g": N, "b": N, "a": N}.
    return (
        int(fill.get("r") or 0),
        int(fill.get("g") or 0),
        int(fill.get("b") or 0),
        int(fill.get("a") or 0),
    )


def active_filter_index(tf: RpcSubprocess) -> int | None:
    """Identify the engaged filter button via paint-snapshot fill
    inspection. Returns the index whose fill alpha > 0 (Accent),
    or None if all 3 are transparent (impossible by construction)."""
    for i in range(3):
        rgba = filter_button_fill(tf, i)
        if rgba[3] > 0:
            return i
    return None


def body() -> None:
    with RpcSubprocess("todomvc") as tf:
        # ── (0) Initial posture — textfield Idle ──────────────────
        assert_eq(tf.query("/external/state"), "Idle", "initial textfield state")

        snap0 = tf.snapshot(source="paint", viewport=(480, 720))
        # Scroll wrap intact from R658.
        assert (
            find_scroll_with_content_tag(snap0, LIST_TAG) is not None
        ), "R658: list region still wrapped in Scene::Scroll"
        # R659 — filter row + 3 buttons + scrollbar peer all paint-addressable.
        assert (
            find_node_by_tag(snap0, FILTER_TAG) is not None
        ), "R659: filter row Container tag present"
        for i in range(3):
            assert (
                find_node_by_tag(snap0, f"todo_filter#{i}") is not None
            ), f"R659: todo_filter#{i} button paint-addressable"
        assert (
            find_node_by_tag(snap0, SCROLLBAR_TAG) is not None
        ), "R659: scrollbar peer tag present"

        # ── (1) Initial filter is All (button 2) ──────────────────
        assert_eq(
            active_filter_index(tf),
            2,
            "R659: boot default filter = All (button 2 has Accent fill)",
        )

        # ── (2) Add 5 items, toggle 3 to completed ────────────────
        focus_set(tf, TF_TAG)
        time.sleep(0.05)
        for word in ("alpha", "beta", "gamma", "delta", "epsilon"):
            type_text(tf, word)
            submit_enter(tf)

        assert_eq(
            visible_count(tf),
            5,
            "5 items visible under default All filter",
        )

        ids = [get_item_id(tf, i) for i in range(5)]
        # toggle alpha, gamma, epsilon → 3 completed.
        tf.click(path=f"todo_toggle#{ids[0]}")
        tf.click(path=f"todo_toggle#{ids[2]}")
        tf.click(path=f"todo_toggle#{ids[4]}")
        time.sleep(0.1)

        # Header reflects the pre-filter R658 shape (visible == total).
        assert_eq(
            list_header_text(tf),
            "Todos (3 of 5 completed)",
            "R659: pre-filter header retains R658 shape",
        )

        # ── (3) Click Active filter → 2 uncompleted visible ───────
        tf.click(path="todo_filter#0")
        time.sleep(0.1)

        assert_eq(
            visible_count(tf),
            2,
            "R659: Active filter hides 3 completed",
        )
        assert_eq(
            list_header_text(tf),
            "Active: 2 of 5",
            "R659: header shows <FilterName>: visible of total when hiding rows",
        )
        # Paint-side mutual exclusion: only Active button (0) has Accent.
        assert_eq(
            active_filter_index(tf),
            0,
            "R659: Active button (0) is the engaged segment",
        )

        # ── (4) Click Completed filter → 3 completed visible ──────
        tf.click(path="todo_filter#1")
        time.sleep(0.1)

        assert_eq(
            visible_count(tf),
            3,
            "R659: Completed filter shows 3 completed",
        )
        assert_eq(
            list_header_text(tf),
            "Completed: 3 of 5",
            "R659: header for Completed filter",
        )
        assert_eq(
            active_filter_index(tf),
            1,
            "R659: Completed button (1) is the engaged segment",
        )

        # ── (5) Click All filter → 5 visible again ────────────────
        tf.click(path="todo_filter#2")
        time.sleep(0.1)

        assert_eq(
            visible_count(tf),
            5,
            "R659: All filter restores full list",
        )
        assert_eq(
            list_header_text(tf),
            "Todos (3 of 5 completed)",
            "R659: header back to pre-filter shape under All",
        )
        assert_eq(
            active_filter_index(tf),
            2,
            "R659: All button (2) engaged",
        )

        # ── (6) Active filter again, then All — cycle test ────────
        tf.click(path="todo_filter#0")  # Active
        time.sleep(0.1)
        assert_eq(
            active_filter_index(tf),
            0,
            "R659: filter cycle — back to Active",
        )
        assert_eq(visible_count(tf), 2, "R659: 2 visible under Active again")

        # ── (7) Click already-engaged button (no-op via Signal eq) ─
        tf.click(path="todo_filter#0")
        time.sleep(0.1)
        assert_eq(
            visible_count(tf),
            2,
            "R659: idempotent click on engaged filter (Signal equality-skip)",
        )

        tf.click(path="todo_filter#2")  # back to All
        time.sleep(0.1)

        # ── (8) Add 5 more entries → 10 total → confirm scroll ────
        focus_set(tf, TF_TAG)
        time.sleep(0.05)
        for word in ("zeta", "eta", "theta", "iota", "kappa"):
            type_text(tf, word)
            submit_enter(tf)

        assert_eq(visible_count(tf), 10, "R659: list grew to 10 under All filter")
        assert_eq(
            list_header_text(tf),
            "Todos (3 of 10 completed)",
            "R659: header reflects new count under All filter",
        )

        # ── (9) Scrollbar peer paint shape ────────────────────────
        # The snapshot serializes the **post-layout** `rect` field
        # (computed by the layout pass) rather than the declarative
        # `layout.size`. The lifted helper sets gutter = 8 px (M3
        # canonical) via VerticalScrollbarStyle::material, and the
        # outer Container's intrinsic size pins the width.
        snap_post = tf.snapshot(source="paint", viewport=(480, 720))
        scrollbar = find_node_by_tag(snap_post, SCROLLBAR_TAG)
        assert scrollbar is not None, "R659: scrollbar peer Container present"
        rect = scrollbar.get("rect") or {}
        assert_eq(
            int(rect.get("w") or 0),
            8,
            "R659: scrollbar laid-out width = 8 px (M3 canonical gutter)",
        )

        # Scrollbar holds exactly 1 child (the thumb) per R55.D.6
        # absolute-position composition.
        children = scrollbar.get("children") or []
        assert_eq(
            len(children),
            1,
            "R659: scrollbar peer holds only the thumb (no spacer)",
        )

        # ── (10) R658 stable-id invariant under filter cycle ──────
        tf.click(path="todo_filter#1")  # Completed
        time.sleep(0.1)
        assert_eq(
            visible_count(tf),
            3,
            "R659: 3 completed across the 5-add cycle",
        )

        tf.click(path="todo_filter#2")  # back to All
        time.sleep(0.1)

        ids_after = [get_item_id(tf, i) for i in range(10)]
        assert ids[0] in ids_after, (
            f"R658 stable-id contract: alpha id={ids[0]} survives filter cycle "
            f"(current ids={ids_after})"
        )
        assert_eq(
            len(set(ids_after)),
            10,
            "R658: 10 unique ids preserved across filter / scroll / sibling adds",
        )

        # ── (11) Filter row paint shape ───────────────────────────
        filter_row = find_node_by_tag(snap_post, FILTER_TAG)
        assert filter_row is not None
        f_children = filter_row.get("children") or []
        assert_eq(
            len(f_children),
            3,
            "R659: filter row holds exactly 3 buttons",
        )
        for i, child in enumerate(f_children):
            assert_eq(
                child.get("tag"),
                f"todo_filter#{i}",
                f"R659: filter button {i} carries todo_filter#{i} tag",
            )


if __name__ == "__main__":
    sys.exit(run_demo("todomvc R659", body))
