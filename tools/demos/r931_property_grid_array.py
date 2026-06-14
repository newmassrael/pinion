#!/usr/bin/env python3
"""R931 §5.38 §5.40 §5.50 — property-grid array / Vec property editing.

Drives `hello-property-grid` via JSON-RPC. The Details panel now edits a
**dynamic array** property (a `TArray<f32>` — Unreal "Spawn Weights"): a
collapsible `arr.weights` branch whose **element leaves** (`elem.<k>`) the editor
can grow, shrink and reorder at runtime.

The textbook resolution of the "edit-path fork" — array elements live OUTSIDE the
flat value model, so add / remove / reorder never displace a scalar leaf's stable
`value.<i>` address:
  * the elements are a **separate** `Signal<Vec<CellValue>>` sub-model, addressed
    by position (`value.elem.<k>` / `name.elem.<k>` / `kind.elem.<k>`), the same
    `ValueRef` wire vocabulary scalars use — so an element edits, scrubs and
    reads through the identical machinery (no parallel edit path);
  * `add_elem` / `remove_elem` / `move_elem` mutate the sub-model and regenerate
    the synthesized element leaves; `remove_elem` carries the R930.1 discipline
    (it cancels an in-flight edit and re-anchors the cursor).

  (A) boot — the array branch + 3 element leaves sit under a Gameplay category.
  (B) an element reads / writes through the unified `value.elem.<k>` wire.
  (C) add an element (RPC + a click on the "+" button) grows the list.
  (D) remove an element (RPC + a click on its "-" button) shrinks it; later
      elements shift down (array semantics).
  (E) move_elem reorders the sub-model (a Vec remove + insert).
  (F) begin-editing an element reports its node id; remove cancels it (R930.1).
  (G) a scalar leaf's value.<i> address is permanent across array mutation.
  (H) collapsing the array branch hides its element rows.
  (I) the cursor is an id-keyed move onto an element (no edit side effect).

>= 30 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-property-grid"
GRID = "property_grid"
TREE = "property_grid_tree"
VIEWPORT = (460, 900)

ARR = "arr.weights"
GAMEPLAY = "cat.Gameplay"


def gq(tf, slot):
    return tf.query(f"/{GRID}/external/{slot}")


def tq(tf, slot):
    return tf.query(f"/{TREE}/external/{slot}")


def painted(tf, tag: str) -> bool:
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    return find_by_tag(snap, tag) is not None


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot — the array branch + 3 element leaves ──────────
        assert_eq(tq(tf, "row_count"), 28, "6 cats + 2 structs + 1 array + 19 leaves, all expanded")
        assert_eq(gq(tf, "elem_count"), 3, "the array sub-model boots with 3 elements")
        assert_eq(tq(tf, "id_at.23"), GAMEPLAY, "the Gameplay category is appended last")
        assert_eq(tq(tf, "id_at.24"), ARR, "Gameplay nests the array branch")
        assert_eq(tq(tf, "level_at.24"), 2, "the array branch is aria-level 2")
        assert_eq(tq(tf, "id_at.25"), "elem.0", "the array's first element leaf")
        assert_eq(tq(tf, "level_at.25"), 3, "an element is aria-level 3 (category > array > element)")
        assert_eq(tq(tf, "expanded_at.25"), None, "an element leaf has no aria-expanded")
        assert painted(tf, f"{GRID}#{ARR}"), "the array branch header is painted"
        assert painted(tf, f"{GRID}#elem.0"), "an element row is painted when expanded"
        assert painted(tf, f"{GRID}#addelem"), "the array's add button is painted"
        assert painted(tf, f"{GRID}#rmelem0"), "an element's remove button is painted"

        # ── (B) an element reads / writes via the unified value wire ─
        assert_eq(gq(tf, "value.elem.1"), 0.5, "element 1 reads its sub-model value")
        assert_eq(gq(tf, "name.elem.1"), "Spawn Weights [1]", "the qualified element name")
        assert_eq(gq(tf, "kind.elem.1"), "float", "the homogeneous element kind")
        tf.intervene(f"/{GRID}/external/value.elem.1", 2.5)
        assert_eq(gq(tf, "value.elem.1"), 2.5, "intervene writes the element value")

        # The Gameplay/array branch sits at the bottom of the tall 28-row grid
        # (which does not scroll), so collapse the categories above it to lift
        # the array's "+"/"-" buttons into the window for the pixel-click steps
        # below. (RPC / scene-as-data reads above work regardless of position.)
        for cat in ("cat.Identity", "cat.Appearance", "cat.Physics", "cat.Stats", "cat.Transform"):
            tf.intervene(f"/{GRID}/external/expanded.{cat}", False)
        wait_until(lambda: painted(tf, f"{GRID}#elem.0"), timeout=4.0, interval=0.03,
                   desc="the array elements are visible after collapsing the categories above")

        # ── (C) add an element (RPC, then a click on "+") ───────────
        assert_eq(tf.invoke(f"/{GRID}/external/add_elem", None), 3, "add_elem returns the new index")
        assert_eq(gq(tf, "elem_count"), 4, "the list grew to 4")
        assert_eq(gq(tf, "value.elem.3"), 0.0, "a new element seeds 0.0")
        wait_until(lambda: painted(tf, f"{GRID}#elem.3"), timeout=4.0, interval=0.03,
                   desc="the new element row paints")
        # The "+" button click is the GUI twin of the RPC.
        tf.click(path=f"{GRID}#addelem")
        wait_until(lambda: gq(tf, "elem_count") == 5, timeout=4.0, interval=0.03,
                   desc="clicking the + button adds another element")

        # ── (D) remove an element (RPC, then a click on "-") ────────
        # Elements: [1.0, 2.5, 0.25, 0.0, 0.0]. Remove index 0 -> 2.5 shifts to 0.
        assert_eq(tf.invoke(f"/{GRID}/external/remove_elem", 0), True, "remove_elem reports it existed")
        assert_eq(gq(tf, "elem_count"), 4, "the list shrank to 4")
        assert_eq(gq(tf, "value.elem.0"), 2.5, "later elements shift down on remove (array semantics)")
        assert_eq(tf.invoke(f"/{GRID}/external/remove_elem", 9), False, "an out-of-range remove is a no-op")
        # The "-" button on element 0 removes it.
        tf.click(path=f"{GRID}#rmelem0")
        wait_until(lambda: gq(tf, "elem_count") == 3, timeout=4.0, interval=0.03,
                   desc="clicking an element's - button removes it")

        # ── (E) move_elem reorders the sub-model ────────────────────
        # Elements now: [0.25, 0.0, 0.0]. Move 0 -> 2 -> [0.0, 0.0, 0.25].
        assert_eq(gq(tf, "value.elem.0"), 0.25, "before reorder: element 0")
        assert_eq(tf.invoke(f"/{GRID}/external/move_elem", "0,2"), True, "move_elem reports it moved")
        assert_eq(gq(tf, "value.elem.2"), 0.25, "the element moved to the end")
        assert_eq(tf.invoke(f"/{GRID}/external/move_elem", "1,1"), False, "from == to is a no-op")

        # ── (F) begin-edit reports the node id; remove cancels it ────
        assert_eq(tf.invoke(f"/{GRID}/external/begin", "elem.1"), True, "begin starts the element edit")
        assert_eq(gq(tf, "editing"), "elem.1", "the editing leaf is reported by its node id")
        tf.invoke(f"/{GRID}/external/remove_elem", 0)
        assert_eq(gq(tf, "editing"), None, "removing an element cancels the in-flight edit (R930.1)")

        # ── (G) scalar addresses are permanent across array mutation ─
        before = gq(tf, "value.6")  # Position X scalar
        tf.invoke(f"/{GRID}/external/add_elem", None)
        tf.invoke(f"/{GRID}/external/remove_elem", 0)
        assert_eq(gq(tf, "value.6"), before, "the scalar value.6 is unchanged by array add/remove")
        assert_eq(gq(tf, "row_count"), 16, "the flat value-model length is permanent")

        # ── (H) collapsing the array branch hides its element rows ──
        assert_eq(gq(tf, f"expanded.{ARR}"), True, "the array branch boots expanded")
        assert_eq(tf.invoke(f"/{GRID}/external/toggle_branch", ARR), False, "toggle_branch collapses it")
        wait_until(lambda: not painted(tf, f"{GRID}#elem.0"), timeout=4.0, interval=0.03,
                   desc="the element rows are hidden when the array collapses")
        assert painted(tf, f"{GRID}#{ARR}"), "the array header stays painted when collapsed"
        tf.intervene(f"/{GRID}/external/expanded.{ARR}", True)
        wait_until(lambda: painted(tf, f"{GRID}#elem.0"), timeout=4.0, interval=0.03,
                   desc="the elements reappear on re-expand")

        # ── (I) the cursor is an id-keyed move onto an element ──────
        tf.intervene(f"/{GRID}/external/cursor", "elem.1")
        assert_eq(gq(tf, "cursor"), "elem.1", "intervene moved the cursor onto an element")
        assert_eq(gq(tf, "editing"), None, "a pure cursor move has no edit side effect")


if __name__ == "__main__":
    sys.exit(run_demo("R931 §5.38 §5.40 §5.50 — property-grid array / Vec editing", body))
