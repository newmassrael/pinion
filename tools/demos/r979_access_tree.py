#!/usr/bin/env python3
"""R979 §5.40 §2 #7 — `scene/access`: the accessibility tree as data.

Every pinion widget already builds a WAI-ARIA `AccessNode` tree (the
`WidgetView::access_node` family), which the shell enriches with names and
resolves bounds for, then hands to the platform AccessKit adapter for a
screen reader. Until R979 that tree was reachable ONLY through a live AT
client or an in-process unit test — the AI-first JSON-RPC path (§2 #7
"scene-as-data, queryable as text") could see the paint scene and the
introspect schema but NOT the accessibility projection. That is why the
recurring a11y carry (R966 a slider's value range, R967.1 a focusable reset
control) was deferred as "RPC-invisible": there was no wire to verify it on.

`scene/access` closes the gap. It serializes the same enriched,
bounds-resolved node list (plus the `AccessFocus` target) the AccessKit
adapter receives, so an AI client introspects the a11y tree exactly as a
screen reader would. The wire vocabulary IS the WAI-ARIA vocabulary (each
role / sort token is the type's own `aria_name`).

Two consumers prove the method against the REAL shell wiring (not a
synthetic producer):

  (A) hello-property-grid — the widget the carry lived on. The boot dump is
      a rich tree: a `tree` root named "Inspector", `treeitem` rows carrying
      `aria-level`, a `textbox` search box, a live `status` region. The
      access-tree levels are cross-checked against the existing
      `property_grid_tree` introspect (two independent wires, one model).
      THE HEADLINE: editing a property makes the row's reset `button` — the
      exact node the R967.1 session-review found was AT/RPC-invisible — appear
      in the dump as a named `button` child of its row; resetting removes it.

  (B) hello-slider — closes the R966 value-range carry over the real wire: a
      `slider` node whose `value.float` carries `valuenow` / `valuemin` /
      `valuemax`, cross-checked against the slider's own `scene/query` value,
      and shown tracking an `intervene`.

Run from the workspace root:
    cargo build -p hello-property-grid -p hello-slider --release
    python3 tools/demos/r979_access_tree.py

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

# ── hello-property-grid wiring (mirrors r921 constants) ──────────────
GRID = "property_grid"
TREE = "property_grid_tree"
SEARCH_TF = "property_grid_search"
PG_VIEWPORT = (460, 820)
IDENTITY = "cat.Identity"
POS_X = 6  # value index of the Position X struct field (visible at boot)

# ── hello-slider wiring ─────────────────────────────────────────────
SLIDER_TAG = "main_slider"
SLIDER_VIEWPORT = (480, 320)


def access_nodes(tf):
    """`scene/access` -> the node list (the enriched a11y tree)."""
    return tf.request("scene/access").result


def node_by_tag(result, tag):
    for n in result["nodes"]:
        if n.get("tag") == tag:
            return n
    return None


def pg_query(tf, slot):
    return tf.query(f"/{GRID}/external/{slot}")


def tree_query(tf, slot):
    return tf.query(f"/{TREE}/external/{slot}")


def property_grid_body() -> None:
    with RpcSubprocess("hello-property-grid", boot_grace=1.5) as tf:
        # ── (A) boot: force a paint so names enrich + bounds resolve ──
        snap = tf.snapshot(source="paint", viewport=PG_VIEWPORT)
        assert find_by_tag(snap, GRID) is not None, "grid painted"

        acc = access_nodes(tf)
        assert acc["count"] > 0, "the access tree is non-empty"
        assert "focus" in acc, "the result carries a focus target (or null)"

        # The root is the WAI-ARIA `tree`, explicitly named.
        root = acc["nodes"][0]
        assert_eq(root["tag"], GRID, "root node is the grid")
        assert_eq(root["role"], "tree", "the inspector root is a `tree`")
        assert_eq(root["name"], "Inspector", "the tree carries its explicit name")

        # Bounds resolved from the paint scene prove enrich + resolve ran
        # (not just the raw V::access_node list).
        bounded = [n for n in acc["nodes"] if "bounds" in n]
        assert len(bounded) > 0, "at least one node has paint-resolved bounds"

        treeitems = [n for n in acc["nodes"] if n.get("role") == "treeitem"]
        assert len(treeitems) >= 20, "the inspector advertises its many rows as treeitems"

        # Levels: a category is aria-level 1, a struct field is level 3.
        identity = node_by_tag(acc, f"{GRID}#{IDENTITY}")
        assert identity is not None, "the Identity category row is in the tree"
        assert_eq(identity["role"], "treeitem", "a category is a treeitem")
        assert_eq(identity["level"], 1, "a category is aria-level 1")

        pos_x = node_by_tag(acc, f"{GRID}#{POS_X}")
        assert pos_x is not None, "the Position X leaf row is in the tree"
        assert_eq(pos_x["level"], 3, "a struct field is aria-level 3 (category > struct > field)")

        # Cross-check: `scene/access` agrees with the existing tree introspect
        # (two independent wires reading one model).
        assert_eq(identity["level"], tree_query(tf, "level_at.0"),
                  "access level == tree introspect level for the first category")
        assert_eq(pos_x["level"], tree_query(tf, "level_at.16"),
                  "access level == tree introspect level for Position X")

        # The live search box is a named textbox; the filter count is a status.
        search = node_by_tag(acc, SEARCH_TF)
        assert search is not None, "the search box is in the a11y tree"
        assert_eq(search["role"], "textbox", "the search box is a textbox")
        assert_eq(search["name"], "Filter properties", "the search box is named")
        status = node_by_tag(acc, "pg_search_status")
        assert status is not None, "the live filter-count region is present"
        assert_eq(status["role"], "status", "the count is an aria-live status")
        assert status["name"].endswith("properties"), "the status names the property count"

        # ── (B) THE HEADLINE: a reset button becomes RPC-visible ─────
        reset_tag = f"{GRID}#reset{POS_X}"
        assert node_by_tag(acc, reset_tag) is None, "a clean row advertises no reset button"

        boot_val = pg_query(tf, f"value.{POS_X}")
        pg_name = pg_query(tf, f"name.{POS_X}")
        tf.intervene(f"/{GRID}/external/value.{POS_X}", 99.0)
        assert_eq(pg_query(tf, f"value.{POS_X}"), 99.0, "the field edit applied")

        wait_until(lambda: node_by_tag(access_nodes(tf), reset_tag) is not None,
                   timeout=4.0, interval=0.03,
                   desc="the modified row's reset button appears in the access tree")
        acc = access_nodes(tf)
        reset = node_by_tag(acc, reset_tag)
        assert_eq(reset["role"], "button", "the reset affordance is a button (AT-reachable)")
        assert_eq(reset["name"], f"Reset {pg_name} to default", "the button is named for the property")
        row = node_by_tag(acc, f"{GRID}#{POS_X}")
        assert reset_tag in row.get("children", []), "the reset button is the row's child"

        # Restoring the default removes the reset button (one gate: paint + a11y).
        tf.intervene(f"/{GRID}/external/value.{POS_X}", boot_val)
        wait_until(lambda: node_by_tag(access_nodes(tf), reset_tag) is None,
                   timeout=4.0, interval=0.03,
                   desc="the reset button disappears once the row is default again")
        assert_eq(pg_query(tf, f"value.{POS_X}"), boot_val, "the field restored to default")


def slider_body() -> None:
    with RpcSubprocess("hello-slider", boot_grace=1.0) as tf:
        # Force a paint, then dump the a11y tree.
        tf.snapshot(source="paint", viewport=SLIDER_VIEWPORT)
        acc = access_nodes(tf)
        slider = node_by_tag(acc, SLIDER_TAG)
        assert slider is not None, "the slider is in the access tree"
        assert_eq(slider["role"], "slider", "the node is a slider")

        # The R966 value-range carry: valuenow / valuemin / valuemax over the wire.
        fval = slider["value"]["float"]
        assert_eq(fval["min"], 0.0, "aria-valuemin is RPC-visible")
        assert_eq(fval["max"], 1.0, "aria-valuemax is RPC-visible")
        assert 0.0 <= fval["value"] <= 1.0, "aria-valuenow sits within the range"

        # Cross-check: access value == the slider's own introspect value.
        # The sole primary external is addressed as `/external/<slot>` (no
        # tag prefix — the R666 v1 path syntax for the primary).
        own = tf.query("/external/value")
        assert abs(fval["value"] - own) < 1e-6, "access valuenow == scene/query value"

        # It tracks an intervene through the same set_value funnel.
        tf.intervene("/external/value", 0.25)
        wait_until(
            lambda: abs(node_by_tag(access_nodes(tf), SLIDER_TAG)["value"]["float"]["value"] - 0.25) < 1e-6,
            timeout=4.0, interval=0.03,
            desc="aria-valuenow tracks the new slider value over scene/access",
        )


def body() -> None:
    property_grid_body()
    slider_body()


if __name__ == "__main__":
    sys.exit(run_demo("r979_access_tree", body))
