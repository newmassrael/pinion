#!/usr/bin/env python3
"""R958 §5.38 §5.40 — multi-object inspector: modified-from-default + reset.

Drives hello-inspector over JSON-RPC. R922 made the inspector edit the properties
COMMON to a multi-selection and report "Multiple Values" (`mixed.<i>`) where the
selected objects disagree. R958 adds the other half of an engine/the toolkit inspector:
a per-property **modified-from-default** indicator and **reset to default**.

Each selected object compares its OWN current value to its OWN frozen class
default (`use_object_defaults`) via the NaN-safe `CellValue::value_eq` SSOT:

  (A) `modified.<i>` is true when ANY selected object diverges from its default
      — orthogonal to `mixed` (a property can be uniform-but-modified, or
      mixed-but-default: the base props boot mixed-but-at-default);
  (B) `any_modified` rolls the selection up for a panel-level "reset all";
  (C) `invoke reset <i>` restores property i to each object's OWN default, so a
      selection whose defaults differ (Layer is 1/1/2) reads "Multiple Values"
      yet "not modified" afterward — reset is per object, not to a shared value;
  (D) the Details reset arrow (`inspector#reset<i>`) paints ONLY on a modified
      row and a click on it routes to the same reset (the GUI peer of the RPC);
  (E) `invoke reset_all` clears every modified property in one atomic write and
      returns the count;
  (F) reset is idempotent (a no-op on an at-default row).

Run from the workspace root:
    cargo build -p hello-inspector --release
    python3 tools/demos/r958_inspector_modified_reset.py

>= 30 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    runs_of,
    wait_query,
    wait_snap,
    wait_until,
)

VP = (640, 400)


def _q(tf: RpcSubprocess, key: str) -> Any:
    return tf.query(f"/external/{key}")


def _arrow_present(tf: RpcSubprocess, i: int) -> bool:
    snap = tf.snapshot(source="paint", viewport=VP)
    return find_by_tag(snap, f"inspector#reset{i}") is not None


def body() -> None:
    with RpcSubprocess("hello-inspector", request_timeout=12.0) as tf:
        wait_until(lambda: True if _q(tf, "object_count") == 3 else None, desc="inspector ready")

        # ── (A) boot: one object, nothing modified ──────────────────────
        assert_eq(_q(tf, "selection"), runs_of([0]), "boots with Player selected")
        assert_eq(_q(tf, "any_modified"), False, "boot Player is all at default")
        assert_eq(_q(tf, "modified.1"), False, "Player property 1 at default")

        # ── select all three; common base = Visible / Layer / Locked ────
        tf.invoke("/external/select_all", None)
        wait_query(tf, "/external/selection", runs_of([0, 1, 2]),
                   desc="all three selected")
        assert_eq(_q(tf, "row_count"), 3, "Visible / Layer / Locked are common")
        assert_eq(_q(tf, "name.1"), "Layer", "common property 1 is Layer")

        # ── (A) modified is orthogonal to mixed ─────────────────────────
        # The base props boot MIXED (Layer is 1/1/2) but at DEFAULT.
        assert_eq(_q(tf, "mixed.1"), True, "Layer disagrees across the selection (1/1/2)")
        assert_eq(_q(tf, "modified.1"), False, "...yet Layer is at default in every object")
        assert_eq(_q(tf, "any_modified"), False, "nothing modified at boot")
        assert not _arrow_present(tf, 1), "no reset arrow on an at-default row"

        # ── (B) edit Layer across the selection -> modified ─────────────
        tf.intervene("/external/value.1", 5)
        wait_query(tf, "/external/value.1", 5, desc="Layer set to 5 across the selection")
        assert_eq(_q(tf, "modified.1"), True, "edited Layer is modified from default")
        assert_eq(_q(tf, "any_modified"), True, "the panel is now dirty")
        snap = wait_snap(
            tf,
            lambda s: find_by_tag(s, "inspector#reset1") is not None,
            viewport=VP,
            desc="reset arrow paints on the modified Layer row",
        )
        assert find_by_tag(snap, "inspector#reset0") is None, "Visible (still default) has no arrow"

        # ── (C) RPC reset restores EACH object's own default ────────────
        assert_eq(tf.invoke("/external/reset", 1), True, "reset reports it changed something")
        wait_query(tf, "/external/modified.1", False, desc="Layer no longer modified")
        assert_eq(_q(tf, "value.1"), 1, "representative back to Player's default (1)")
        assert_eq(
            _q(tf, "mixed.1"),
            True,
            "per-object defaults differ (1/1/2) -> mixed again, but not modified",
        )
        assert not _arrow_present(tf, 1), "the reset arrow is gone after reset"

        # ── (F) reset is idempotent on an at-default row ────────────────
        assert_eq(tf.invoke("/external/reset", 1), False, "re-resetting an at-default row is a no-op")

        # ── (D) the GUI reset arrow click drives the same reset ─────────
        tf.intervene("/external/value.0", False)  # Visible -> false (Player+Camera diverge)
        wait_query(tf, "/external/modified.0", True, desc="Visible modified after edit")
        assert _arrow_present(tf, 0), "Visible reset arrow now paints"
        tf.click(path="inspector#reset0")
        wait_query(tf, "/external/modified.0", False, desc="clicking the arrow reset Visible")
        assert_eq(_q(tf, "value.0"), True, "Visible back to Player's default (true)")
        assert_eq(_q(tf, "mixed.0"), True, "Visible defaults differ (true/true/false) -> mixed")

        # ── (E) reset_all clears every modified property at once ────────
        tf.intervene("/external/value.1", 7)  # Layer (all diverge)
        tf.intervene("/external/value.2", True)  # Locked -> true (Player+Camera diverge)
        wait_query(tf, "/external/any_modified", True, desc="two properties modified")
        assert_eq(_q(tf, "modified.1"), True, "Layer modified")
        assert_eq(_q(tf, "modified.2"), True, "Locked modified")
        assert_eq(tf.invoke("/external/reset_all", None), 2, "reset_all reports 2 properties cleared")
        wait_query(tf, "/external/any_modified", False, desc="nothing modified after reset_all")
        assert_eq(_q(tf, "modified.1"), False, "Layer cleared")
        assert_eq(_q(tf, "modified.2"), False, "Locked cleared")

        # ── (G) single-object selection: reset to that object's default ─
        tf.invoke("/external/select", 2)  # Light only
        wait_query(tf, "/external/selection", runs_of([2]),
                   desc="Light selected alone")
        assert_eq(_q(tf, "name.1"), "Layer", "Light's common[1] is Layer")
        assert_eq(_q(tf, "modified.1"), False, "Light Layer at its default (2)")
        tf.intervene("/external/value.1", 9)
        wait_query(tf, "/external/modified.1", True, desc="Light Layer modified")
        assert_eq(tf.invoke("/external/reset", 1), True, "reset Light's Layer")
        wait_query(tf, "/external/value.1", 2, desc="Light Layer back to its own default (2)")
        assert_eq(_q(tf, "modified.1"), False, "Light Layer at default again")


if __name__ == "__main__":
    sys.exit(run_demo("R958 §5.38 §5.40 — inspector modified-from-default + reset", body))
