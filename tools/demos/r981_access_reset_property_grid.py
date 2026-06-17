#!/usr/bin/env python3
"""R981 §5.40 §2 #7 — AT-reachable property-grid controls over `scene/access`.

R980 made the data grid's reset AT-reachable; the R980 audit found the property
grid had the mirror gap — its reset arrow + the R931 remove / add-element buttons
were ANNOUNCED (emitted by `access_node` via the lifted `attach_child_button`, so
`scene/access` already lists them) but NOT ACTIVATABLE: the view had no
`access_child_invoke`, so an AT Click fell through to the parent grid's Enter.

R981 adds that hook: an AT Click / Default on a control button (reset / remove /
add) routes through the SAME `send` funnel a pointer click drains, so AT
activation == pointer activation. This demo verifies it end to end over the R979
`scene/access` surface, activating through the AT wire twin
(`send "<sub>:PointerUp"`):

  (A) boot — the always-present array controls are announced: the array branch
      carries an "Add element" button child, each element row a "Remove" button.
  (B) a modified row advertises a reset `button` child of its `treeitem`.
  (C) an AT Click on the reset button (its send-wire twin) restores the row, and
      the button leaves the access tree.
  (D) an AT Click on a "Remove" / "Add" button shrinks / grows the array.

Run from the workspace root:
    cargo build -p hello-property-grid --release
    python3 tools/demos/r981_access_reset_property_grid.py

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
EXT = f"/{GRID}/external"
VIEWPORT = (460, 900)


def access(tf):
    return tf.request("scene/access").result


def node_by_tag(result, tag):
    for n in result["nodes"]:
        if n.get("tag") == tag:
            return n
    return None


def buttons(result):
    return [n for n in result["nodes"] if n.get("role") == "button"]


def child_button_named(result, host_tag, name_prefix):
    """The button child of `host_tag` whose name starts with `name_prefix`."""
    host = node_by_tag(result, host_tag)
    if host is None:
        return None
    for c in host.get("children", []):
        btn = node_by_tag(result, c)
        if btn and btn.get("role") == "button" and btn.get("name", "").startswith(name_prefix):
            return btn
    return None


def gq(tf, slot):
    return tf.query(f"{EXT}/{slot}")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, GRID) is not None, "grid painted"
        assert_eq(gq(tf, "elem_count"), 3, "the array sub-model boots with 3 elements")
        assert_eq(gq(tf, "any_modified"), False, "the grid boots clean (no reset buttons yet)")

        acc = access(tf)
        assert acc["count"] > 0, "the access tree is non-empty"

        # ── (A) the always-present array controls are announced ──────
        add_btn = child_button_named(acc, f"{GRID}#arr.weights", "Add ")
        assert add_btn is not None, "the array branch advertises an Add-element button child"
        assert_eq(add_btn["tag"], f"{GRID}#addelem", "the add button has the addelem tag")
        rm_btn = child_button_named(acc, f"{GRID}#elem.0", "Remove ")
        assert rm_btn is not None, "the first array element advertises a Remove button child"
        assert rm_btn["tag"].startswith(f"{GRID}#rmelem"), "the remove button has an rmelem tag"
        # No reset button is advertised while everything is at its default.
        assert not any("#reset" in b.get("tag", "") for b in buttons(acc)), \
            "a clean grid advertises no reset buttons"

        # ── (B) a modified row advertises a reset button child ───────
        tf.intervene(f"{EXT}/value.4", 17)  # "Layer" (Int, default 3)
        assert_eq(gq(tf, "modified.4"), True, "the edit marked row 4 modified")
        wait_until(lambda: child_button_named(access(tf), f"{GRID}#4", "Reset ") is not None,
                   timeout=4.0, interval=0.03, desc="the modified row advertises a reset button")
        acc = access(tf)
        reset_btn = child_button_named(acc, f"{GRID}#4", "Reset ")
        assert_eq(reset_btn["role"], "button", "the reset affordance is a button (AT-reachable)")
        assert_eq(reset_btn["name"], "Reset Layer to default", "the reset button is named for the row")

        # ── (C) an AT Click on the reset button restores the row ─────
        reset_sub = reset_btn["tag"].split("#", 1)[1]  # "reset4"
        tf.invoke(f"{EXT}/send", f"{reset_sub}:PointerUp")
        wait_until(lambda: gq(tf, "modified.4") is False,
                   timeout=4.0, interval=0.03, desc="the AT reset restored the row")
        assert_eq(gq(tf, "value.4"), 3, "row 4 is back to its Int default")
        acc = access(tf)
        assert child_button_named(acc, f"{GRID}#4", "Reset ") is None, \
            "the reset button leaves the access tree once the row is default"

        # ── (D) an AT Click on Remove / Add shrinks / grows the array ─
        rm_sub = rm_btn["tag"].split("#", 1)[1]  # "rmelem0"
        tf.invoke(f"{EXT}/send", f"{rm_sub}:PointerUp")
        wait_until(lambda: gq(tf, "elem_count") == 2,
                   timeout=4.0, interval=0.03, desc="the AT Remove shrank the array to 2")
        assert_eq(gq(tf, "elem_count"), 2, "one element removed via the AT wire")
        # Add brings it back; the add button is always present.
        acc = access(tf)
        add_sub = node_by_tag(acc, f"{GRID}#addelem")["tag"].split("#", 1)[1]  # "addelem"
        tf.invoke(f"{EXT}/send", f"{add_sub}:PointerUp")
        wait_until(lambda: gq(tf, "elem_count") == 3,
                   timeout=4.0, interval=0.03, desc="the AT Add grew the array back to 3")
        assert_eq(gq(tf, "elem_count"), 3, "one element added via the AT wire")


if __name__ == "__main__":
    sys.exit(run_demo("R981 §5.40 — AT-reachable property-grid controls over scene/access", body))
