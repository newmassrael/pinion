#!/usr/bin/env python3
"""R921 §5.38 §5.40 §5.50 — property-grid nested struct properties.

Drives `hello-property-grid` via JSON-RPC. The Details panel is now a **tree**:
scalar properties nest under collapsible **category** branches (Identity /
Appearance / Transform / …) and **struct** branches — a `Vector3` like Position
expands into its X / Y / Z field rows (the engine / the toolkit Details core depth). The
row backbone migrated off the 2-level `GroupOrderState` group-by proxy onto the
arbitrary-depth WAI-ARIA Tree substrate, so one visible-row sequence drives the
paint, the id-keyed cursor, the collapse and the a11y `tree`.

Two externals carry the structure (the value model stays flat, keyed by value
index, so `value.<i>` / `reset` / scrub are unchanged):
  property_grid_tree  (TreeViewIntrospect, read-only) — the visible-row flatten:
    /external/row_count            -> visible row count
    /external/id_at.<pos>          -> a row's node id (leaf "6" / branch cat:/struct:)
    /external/level_at.<pos>       -> aria-level (depth + 1)
    /external/expanded_at.<pos>    -> aria-expanded (Bool branch / Null leaf)
  property_grid       (the coordinator) — collapse + the struct aggregate:
    /external/expanded.<branch_id> -> read / intervene a branch's collapse
    /external/toggle_branch        -> invoke(text id): flip a branch's collapse
    /external/struct_summary.<id>  -> a struct's "(x, y, z)" tuple
    /external/struct_modified.<id> -> any field modified?
    /external/reset_struct         -> invoke(text id): reset every field
    /external/cursor               -> read / intervene the roving cursor node id

  (A) boot — the tree structure: 5 categories + 2 structs + 16 leaves.
  (B) the Position struct nests X / Y / Z at aria-level 3 under Transform.
  (C) editing a field marks the struct modified; its reset arrow paints.
  (D) reset_struct restores every field through the shared funnel.
  (E) collapse a struct (RPC + a click on its header) hides its fields.
  (F) collapse a category hides its leaves; the row count drops.
  (G) the cursor is an id-keyed read/write (a pure move, no click side effect).

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
VIEWPORT = (460, 820)

POS = "struct.Position"
SCALE = "struct.Scale"
IDENTITY = "cat.Identity"
# Value indices: Position X / Y / Z fields.
POS_X, POS_Y, POS_Z = 6, 7, 12


def gq(tf, slot):
    return tf.query(f"/{GRID}/external/{slot}")


def tq(tf, slot):
    return tf.query(f"/{TREE}/external/{slot}")


def painted(tf, tag: str) -> bool:
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    return find_by_tag(snap, tag) is not None


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot — the tree structure ───────────────────────────
        assert_eq(tq(tf, "row_count"), 28, "6 categories + 2 structs + 1 array + 19 leaves, all expanded")
        assert_eq(tq(tf, "id_at.0"), IDENTITY, "the first row is the Identity category")
        assert_eq(tq(tf, "level_at.0"), 1, "a category is aria-level 1")
        assert_eq(tq(tf, "expanded_at.0"), True, "categories boot expanded")
        assert_eq(gq(tf, "row_count"), 16, "value model = 12 scalars + 4 struct fields")
        assert_eq(gq(tf, "name.6"), "Position X", "value 6 is the qualified Position X field")

        # ── (B) the Position struct nests X / Y / Z at level 3 ──────
        assert_eq(tq(tf, "id_at.15"), POS, "the Transform category nests the Position struct")
        assert_eq(tq(tf, "level_at.15"), 2, "a struct is aria-level 2")
        assert_eq(tq(tf, "id_at.16"), "6", "the struct's first field is Position X (value 6)")
        assert_eq(tq(tf, "level_at.16"), 3, "a struct field is aria-level 3 (category > struct > field)")
        assert_eq(tq(tf, "expanded_at.16"), None, "a leaf has no aria-expanded")
        summary = gq(tf, "struct_summary.struct.Position")
        assert summary.startswith("(") and summary.endswith(")") and "12.5" in summary, \
            f"the struct summary is a tuple of its field values, got {summary!r}"
        assert_eq(gq(tf, "struct_modified.struct.Position"), False, "boot: the struct is clean")
        assert painted(tf, f"{GRID}#{POS}"), "the Position struct header is painted"
        assert painted(tf, f"{GRID}#{POS_X}"), "the Position X field row is painted when expanded"

        # ── (C) edit a field -> the struct reads modified + paints its arrow ─
        tf.intervene(f"/{GRID}/external/value.{POS_X}", 99.0)
        assert_eq(gq(tf, f"value.{POS_X}"), 99.0, "the field edit applied")
        assert_eq(gq(tf, "struct_modified.struct.Position"), True, "a modified field makes the struct modified")
        assert_eq(gq(tf, "struct_modified.struct.Scale"), False, "the untouched Scale struct stays clean")
        wait_until(lambda: painted(tf, f"{GRID}#reset{POS}"), timeout=4.0, interval=0.03,
                   desc="the modified struct paints its reset arrow")

        # ── (D) reset_struct restores every field (the shared funnel) ─
        assert_eq(tf.invoke(f"/{GRID}/external/reset_struct", POS), 1, "reset_struct reports the field count reset")
        assert_eq(gq(tf, f"value.{POS_X}"), 12.5, "Position X restored to its default")
        assert_eq(gq(tf, "struct_modified.struct.Position"), False, "the struct reads clean after reset_struct")
        wait_until(lambda: not painted(tf, f"{GRID}#reset{POS}"), timeout=4.0, interval=0.03,
                   desc="the struct reset arrow disappears once default")

        # ── (E) collapse the struct (RPC + a click) hides its fields ─
        assert_eq(gq(tf, "expanded.struct.Position"), True, "the struct boots expanded")
        assert_eq(tf.invoke(f"/{GRID}/external/toggle_branch", POS), False, "toggle_branch collapses it")
        assert_eq(gq(tf, "expanded.struct.Position"), False, "the struct is now collapsed")
        assert_eq(tq(tf, "row_count"), 25, "collapsing Position hides its 3 fields (28 - 3)")
        wait_until(lambda: not painted(tf, f"{GRID}#{POS_X}"), timeout=4.0, interval=0.03,
                   desc="the Position X field is hidden when its struct collapses")
        assert painted(tf, f"{GRID}#{POS}"), "the struct header stays painted when collapsed"
        # A click on the struct header re-expands it (the disclosure affordance).
        tf.click(path=f"{GRID}#{POS}")
        wait_until(lambda: gq(tf, "expanded.struct.Position") is True, timeout=4.0, interval=0.03,
                   desc="clicking the struct header re-expands it")
        wait_until(lambda: painted(tf, f"{GRID}#{POS_X}"), timeout=4.0, interval=0.03,
                   desc="the field reappears after the click expand")

        # ── (F) collapse a category hides its leaves ────────────────
        tf.intervene(f"/{GRID}/external/expanded.{IDENTITY}", False)
        assert_eq(gq(tf, f"expanded.{IDENTITY}"), False, "intervene collapsed the Identity category")
        assert_eq(tq(tf, "row_count"), 25, "collapsing Identity hides its 3 leaves (28 - 3)")
        wait_until(lambda: not painted(tf, f"{GRID}#0"), timeout=4.0, interval=0.03,
                   desc="the Name leaf is hidden when Identity collapses")
        tf.intervene(f"/{GRID}/external/expanded.{IDENTITY}", True)
        assert_eq(tq(tf, "row_count"), 28, "re-expanding restores the full tree")

        # ── (G) the cursor is an id-keyed read/write (a pure move) ───
        # (The (E) header click already moved the cursor onto the struct — a
        # click moves the roving cursor; clearing then re-driving shows the wire.)
        tf.intervene(f"/{GRID}/external/cursor", None)
        assert_eq(gq(tf, "cursor"), None, "Null clears the cursor")
        tf.intervene(f"/{GRID}/external/cursor", POS)
        assert_eq(gq(tf, "cursor"), POS, "intervene moved the cursor onto the struct")
        assert_eq(tq(tf, "cursor"), POS, "the tree introspection reports the same cursor")
        assert_eq(gq(tf, "editing"), None, "a pure cursor move has no edit side effect")
        tf.intervene(f"/{GRID}/external/cursor", str(POS_Z))
        assert_eq(gq(tf, "cursor"), str(POS_Z), "the cursor moved onto the Position Z leaf")


if __name__ == "__main__":
    sys.exit(run_demo("R921 §5.38 §5.40 §5.50 — property-grid nested struct properties", body))
