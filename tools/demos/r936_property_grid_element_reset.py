#!/usr/bin/env python3
"""R936 §5.38 §5.40 — property-grid array-element modified / reset.

Drives `hello-property-grid` via JSON-RPC. R931 made the array (`TArray<f32>` —
the engine "Spawn Weights") editable but left its **modified / reset** axis deferred:
an edited element showed no reset arrow, and the array branch had no roll-up. R936
clears that carry by extending the SAME modified / reset machinery the scalar
leaves + struct branches already use to the array sub-model — no new primitive,
just the element peers of `leaf_modified` / `struct_is_modified` / `reset_*`:

  * an element is modified when it differs from the frozen `default_array`
    baseline (`modified.elem.<k>`); a per-element reset arrow paints one slot left
    of its remove button, and `reset` (a node-id payload) / a click on the arrow
    restore it through the shared `set_value_at` Elem funnel;
  * the array branch rolls those up (`array_modified.<id>` = length or any element
    differs), paints a reset arrow left of its add button, and `reset_array`
    restores the whole list (length + content) in one wholesale step — the array
    peer of `struct_modified` / `reset_struct`;
  * an ADDED element (no class counterpart) is array-modified (length) but never
    per-element resettable — the array-level reset truncates it instead;
  * `reset_all` / `any_modified` now account for the array, so a dirty list never
    hides behind a "clean" object readout.

  (A) boot — the array is clean: no element / array reset arrows, object clean.
  (B) an element edit lights its reset arrow + the array-branch roll-up + dirties
      the object (`modified.elem.<k>` / `array_modified.<id>` / `any_modified`).
  (C) `reset` (element node id) restores it and clears every indicator.
  (D) clicking an element's reset arrow routes to the same funnel (the GUI twin).
  (E) a length change dirties the array; an added element has NO per-element reset
      arrow; `reset_array` (RPC + a click on the branch arrow) restores length +
      content.
  (F) `reset_all` returns the WHOLE object — scalars AND the array — to default.

>= 35 assertions.
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
VIEWPORT = (460, 900)

ARR = "arr.weights"
ARR_MODIFIED = f"array_modified.{ARR}"


def gq(tf, slot):
    return tf.query(f"/{GRID}/external/{slot}")


def painted(tf, tag: str) -> bool:
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    return find_by_tag(snap, tag) is not None


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # The array branch sits at the bottom of the tall (non-scrolling) grid, so
        # collapse the categories above it up front to lift the array's element
        # rows + buttons + reset arrows into the window for the pixel-click steps.
        # (RPC / scene-as-data reads work regardless of on-screen position.)
        for cat in ("cat.Identity", "cat.Appearance", "cat.Physics", "cat.Stats", "cat.Transform"):
            tf.intervene(f"/{GRID}/external/expanded.{cat}", False)
        wait_until(lambda: painted(tf, f"{GRID}#elem.0"), timeout=4.0, interval=0.03,
                   desc="the array elements are visible after collapsing the categories above")

        # ── (A) boot — the array is clean ───────────────────────────
        assert_eq(gq(tf, "elem_count"), 3, "the array boots with 3 elements [1.0, 0.5, 0.25]")
        assert_eq(gq(tf, "modified.elem.0"), False, "boot: element 0 is at its default")
        assert_eq(gq(tf, "modified.elem.2"), False, "boot: element 2 is at its default")
        assert_eq(gq(tf, ARR_MODIFIED), False, "boot: the array branch is clean")
        assert_eq(gq(tf, "any_modified"), False, "boot: the whole object is clean")
        assert not painted(tf, f"{GRID}#resetelem.0"), "no element reset arrow while clean"
        assert not painted(tf, f"{GRID}#reset{ARR}"), "no array-branch reset arrow while clean"

        # ── (B) an element edit lights every indicator ──────────────
        tf.intervene(f"/{GRID}/external/value.elem.0", 9.0)
        assert_eq(gq(tf, "modified.elem.0"), True, "the edited element is modified")
        assert_eq(gq(tf, "modified.elem.1"), False, "a sibling element stays clean")
        assert_eq(gq(tf, ARR_MODIFIED), True, "the array branch rolls up the element edit")
        assert_eq(gq(tf, "any_modified"), True, "the object is now dirty")
        wait_until(lambda: painted(tf, f"{GRID}#resetelem.0"), timeout=4.0, interval=0.03,
                   desc="the modified element paints its reset arrow")
        assert painted(tf, f"{GRID}#rmelem0"), "the element keeps its remove button beside the reset arrow"
        assert painted(tf, f"{GRID}#reset{ARR}"), "the array branch paints its reset arrow too"
        assert painted(tf, f"{GRID}#addelem"), "the array branch keeps its add button beside the reset arrow"

        # ── (C) `reset` (element node id) restores it ───────────────
        assert_eq(tf.invoke(f"/{GRID}/external/reset", "elem.0"), True, "reset restored the element")
        assert_eq(gq(tf, "value.elem.0"), 1.0, "the element is back at its default")
        assert_eq(gq(tf, "modified.elem.0"), False, "the element is clean again")
        assert_eq(gq(tf, ARR_MODIFIED), False, "the array roll-up clears")
        assert_eq(gq(tf, "any_modified"), False, "the object is clean again")
        assert_eq(tf.invoke(f"/{GRID}/external/reset", "elem.0"), False, "reset of a clean element is a no-op")
        wait_until(lambda: not painted(tf, f"{GRID}#resetelem.0"), timeout=4.0, interval=0.03,
                   desc="the reset arrow disappears once the element is clean")

        # ── (D) clicking an element's reset arrow routes the same ───
        tf.intervene(f"/{GRID}/external/value.elem.2", 4.0)
        wait_until(lambda: painted(tf, f"{GRID}#resetelem.2"), timeout=4.0, interval=0.03,
                   desc="element 2's reset arrow paints")
        tf.click(path=f"{GRID}#resetelem.2")
        wait_until(lambda: gq(tf, "value.elem.2") == 0.25, timeout=4.0, interval=0.03,
                   desc="clicking the element reset arrow restores its default")
        assert_eq(gq(tf, "modified.elem.2"), False, "the clicked element is clean")

        # ── (E) a length change + the added-element rule + reset_array ─
        assert_eq(tf.invoke(f"/{GRID}/external/add_elem", None), 3, "add_elem appends a 4th element")
        assert_eq(gq(tf, "elem_count"), 4, "the list grew to 4")
        assert_eq(gq(tf, ARR_MODIFIED), True, "a longer list is array-modified")
        assert_eq(gq(tf, "any_modified"), True, "the longer list dirties the object")
        assert_eq(gq(tf, "modified.elem.3"), False, "an added element has no class default -> not per-element modified")
        wait_until(lambda: painted(tf, f"{GRID}#reset{ARR}"), timeout=4.0, interval=0.03,
                   desc="the array-branch reset arrow paints on a length change")
        assert not painted(tf, f"{GRID}#resetelem.3"), "an added element paints NO per-element reset arrow"
        # Dirty an in-range element too, then reset the whole list in one step.
        tf.intervene(f"/{GRID}/external/value.elem.1", 6.0)
        assert_eq(tf.invoke(f"/{GRID}/external/reset_array", None), True, "reset_array restored the whole list")
        assert_eq(gq(tf, "elem_count"), 3, "reset_array restored the length")
        assert_eq(gq(tf, "value.elem.1"), 0.5, "reset_array restored the content")
        assert_eq(gq(tf, ARR_MODIFIED), False, "the array is clean after reset_array")
        assert_eq(tf.invoke(f"/{GRID}/external/reset_array", None), False, "reset_array on a default list is a no-op")
        # The GUI twin: a length change, then a click on the branch reset arrow.
        tf.invoke(f"/{GRID}/external/add_elem", None)
        wait_until(lambda: painted(tf, f"{GRID}#reset{ARR}"), timeout=4.0, interval=0.03,
                   desc="the branch reset arrow paints after re-growing the list")
        tf.click(path=f"{GRID}#reset{ARR}")
        wait_until(lambda: gq(tf, "elem_count") == 3, timeout=4.0, interval=0.03,
                   desc="clicking the array-branch reset arrow restores the list")

        # ── (F) reset_all returns the WHOLE object to default ───────
        tf.intervene(f"/{GRID}/external/value.4", 17)      # a scalar (Layer Int)
        tf.intervene(f"/{GRID}/external/value.elem.0", 9.0)  # the array
        assert_eq(gq(tf, "any_modified"), True, "a scalar + the array dirty the object")
        assert_eq(tf.invoke(f"/{GRID}/external/reset_all", None), 2, "reset_all counts the scalar + the array")
        assert_eq(gq(tf, "value.4"), 3, "reset_all restored the scalar")
        assert_eq(gq(tf, "value.elem.0"), 1.0, "reset_all restored the array")
        assert_eq(gq(tf, "any_modified"), False, "the object is clean after reset_all")


if __name__ == "__main__":
    sys.exit(run_demo("R936 §5.38 §5.40 — property-grid array-element modified / reset", body))
