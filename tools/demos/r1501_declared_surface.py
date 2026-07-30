#!/usr/bin/env python3
"""R1501 §5.27 §2#2 §2#7 — a surface declares what it answers.

`$schema` (R825) is the discovery primitive under the whole introspection
surface: §2 #2 makes RPC the AI's primary path, and `$schema` is how a client
learns what it may ask for without hard-coded knowledge. A path that answers but
is not declared is therefore not a documentation gap — it is a feature no client
can find.

Measured on this very binding before the round, over the real wire:

    $schema                                 -> 50 fields
    query stretch_last_section              -> answers   (R1498, undeclared)
    query effective_resize_modes            -> answers   (R1498, undeclared)
    query effective_resize_mode.0           -> answers   (R1498, undeclared)
    query resize_contents_precision         -> answers   (R1496, undeclared)
    invoke reset_default_section_size       -> exists    (R1493, undeclared)
    $schema logical_index_at.<x>            -> {"path": ..., "type": "int"}

Five surfaces the wire answered and the contract denied, plus a sixth that was
declared as a plain scalar although its path spells an argument. Every one was
added by a round that edited `column_layout.rs` — the module that answers them —
while the declaration lived in a hand-written literal down here, which those
rounds had no reason to open.

Nothing failed, because nothing checked this direction. R1353.1's audit runs
declarations against reality; an omission declares nothing to run. And its
dynamic half only reaches widgets `pinion-core` links, which an example is not.

So the declaration moved to the surface that answers: `ColumnLayout::SCHEMA_FIELDS`,
composed here with `SchemaField::concat` instead of restated. `ColumnLayout::query`
is gated on it, so an arm added without a field answers nothing and the round that
adds it fails its own tests.

What this asserts:

  (A) THE CONTRACT COVERS THE SURFACE — the five measured above are declared,
      and every declared read path answers, driven over the wire.
  (B) A FAMILY DECLARES ITS ARGUMENT — `logical_index_at.<x>` carries a typed
      arg and a bound the surface itself publishes (`visible_total`), where it
      used to render exactly like a scalar.
  (C) THE DECLARED DOMAIN HOLDS — inside it every family answers; outside it
      none of them produces a value. Five used to answer `0`, `false` and
      `"interactive"` for a column that does not exist.
  (D) COMPOSED, NOT COPIED — the reorder model's paths appear verbatim at the
      tail, and the binding contributes exactly the one path it answers itself.
  (E) A READ-ONLY PATH SAYS SO — having a declaration is what lets a refusal
      tell "not writable" from "not a path".
  (F) THE HEADER STILL WORKS — the surface under all this is unchanged.

ZERO-FLAKE: every action->assert edge polls published state; no wall-clock
sleeps.

Run from the workspace root:
    python3 tools/demos/r1501_declared_surface.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    assert_rpc_error,
    find_by_tag,
    run_demo,
    wait_until,
)

VIEWPORT = (700, 420)

HDR = "colhdr"
HEADERS = ["Name", "Type", "Size", "Modified", "Owner"]
NCOLS = len(HEADERS)
BOOT_W = [150, 90, 100, 130, 100]
BOOT_TOTAL = sum(BOOT_W)

# Measured before R1501: answered by the wire, absent from `$schema`.
FORMERLY_UNDECLARED = [
    "stretch_last_section",
    "effective_resize_modes",
    "effective_resize_mode.<logical>",
    "resize_contents_precision",
    "reset_default_section_size",
]

# `SchemaField` does not separate a readable value from an `invoke` channel yet
# (its own doc says so), so the write channels are named rather than probed.
ACTIONS = {
    "swap_sections",
    "resize_section",
    "interactive_resize_section",
    "set_section_hidden",
    "set_resize_mode",
    "set_all_resize_modes",
    "set_sort_indicator",
    "cycle_sort_indicator",
    "clear_sort_indicator",
    "reset_default_section_size",
    "send",
    "move",
    "move_section",
    "grab",
    "grab_cancel",
}

# The embedded `ReorderModel`'s own declaration, in its own order — borrowed by
# the layout, which is borrowed by the binding.
REORDER_TAIL = [
    "order",
    "preview",
    "focused_index",
    "grabbed",
    "send",
    "move",
    "move_section",
    "grab",
    "grab_cancel",
]


def _paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def _h(tf, path: str):
    return tf.query(f"/external/{path}")


def _schema(tf) -> list[dict]:
    got = _h(tf, "$schema")
    return got if isinstance(got, list) else got["fields"]


def body() -> None:
    with RpcSubprocess("hello-column-reorder", boot_grace=1.5) as tf:
        wait_until(lambda: find_by_tag(_paint(tf), f"{HDR}#0") is not None,
                   desc="the strip paints")

        fields = _schema(tf)
        declared = {f["path"] for f in fields}

        # ── (A) the contract covers the surface ───────────────────────
        assert_eq(len(fields), len(declared),
                  "no path is declared twice by the composition")            # 1
        for path in FORMERLY_UNDECLARED:
            assert path in declared, \
                f"{path!r} answers but $schema still denies it"              # 2-6

        # Driven, not just named: the three readable ones really do read.
        assert_eq(_h(tf, "stretch_last_section"), False)                     # 7
        assert_eq(_h(tf, "effective_resize_modes"), ["interactive"] * NCOLS) # 8
        assert_eq(_h(tf, "resize_contents_precision"), 1000)                 # 9
        assert_eq(_h(tf, "effective_resize_mode.0"), "interactive")          # 10

        # And the whole declaration is walked: every non-action field answers
        # at a real address. A family is addressed by its MEMBERS, so the
        # template's placeholder is filled rather than sent as spelled.
        # "Answers" means the read did not raise `UnknownIntrospectPath`. It is
        # NOT "returned something non-null": `sort_indicator_section` reads null
        # while no section carries the indicator, and `preview` reads null while
        # no drag is live. Both are answers — treating null as absence would
        # make this walk fail on two honest paths and, worse, would pass on a
        # surface that answered null to everything.
        probed = 0
        for f in fields:
            if f["path"] in ACTIONS:
                continue
            probe = f["path"]
            if f.get("args"):
                probe = probe[:probe.index("<")] + "0"
            try:
                _h(tf, probe)
            except Exception as exc:  # noqa: BLE001 - any refusal is a failure here
                raise AssertionError(
                    f"{f['path']!r} is declared but {probe!r} refused: {exc}"
                ) from exc
            probed += 1
        assert_eq(probed, len(fields) - len(ACTIONS),
                  "every declared field is probed or a named action")        # 11
        assert probed >= 34, f"the walk is not vacuous: {probed} paths"      # 12

        # ── (B) a family declares its argument ────────────────────────
        at_x = next(f for f in fields if f["path"].startswith("logical_index_at"))
        assert_eq(at_x["path"], "logical_index_at.<x>")                      # 13
        assert_eq(len(at_x.get("args") or []), 1,
                  "it takes an argument — it used to declare none")          # 14
        assert_eq(at_x["args"][0]["name"], "x")                              # 15
        assert_eq(at_x["args"][0]["domain"]["kind"], "index_of")             # 16
        assert_eq(at_x["args"][0]["domain"]["count_path"], "visible_total",
                  "bounded by pixels along the row, not by section count")   # 17
        # The bound is a path THIS surface publishes, so the client can follow
        # it — the whole promise an `index_of` domain makes.
        assert_eq(_h(tf, "visible_total"), BOOT_TOTAL)                       # 18
        assert_eq(_h(tf, "logical_index_at.0"), 0, "inside the bound")       # 19
        assert_eq(_h(tf, f"logical_index_at.{BOOT_TOTAL}"), None,
                  "and at the bound the row has no section")                 # 20

        # Every section-keyed family names a domain the surface publishes too.
        assert_eq(_h(tf, "count"), NCOLS,
                  "answered by the layout that declares IndexOf(count)")     # 21
        by_count = [f["path"] for f in fields
                    if any(a["domain"].get("count_path") == "count"
                           for a in (f.get("args") or []))]
        assert_eq(len(by_count), 8, f"the section-keyed families: {by_count}")  # 22

        # ── (C) the declared domain holds ─────────────────────────────
        for path, inside in (("section_size", 100), ("section_hidden", False),
                             ("resize_mode", "interactive"),
                             ("effective_resize_mode", "interactive"),
                             ("visual_index", 2), ("logical_index", 2)):
            assert_eq(_h(tf, f"{path}.2"), inside, f"{path}.2 is a real section")
                                                                             # 23-28
        # Outside it, nothing plausible comes back. Measured before the round:
        # `0`, `false` and `"interactive"` for a column that is not there.
        for path in ("section_size", "section_hidden", "resize_mode",
                     "effective_resize_mode", "content_width",
                     "visual_index", "logical_index", "section_position"):
            assert_eq(_h(tf, f"{path}.{NCOLS}"), None,
                      f"{path}.{NCOLS} is outside the declared domain")      # 29-36

        # ── (D) composed, not copied ──────────────────────────────────
        assert_eq([f["path"] for f in fields[-len(REORDER_TAIL):]], REORDER_TAIL,
                  "the reorder model's declaration, borrowed verbatim")      # 37
        assert_eq(fields[0]["path"], "labels",
                  "and the binding contributes the one path it answers")     # 38
        assert_eq(_h(tf, "labels"), HEADERS)                                 # 39
        assert not any(f["path"] == "" for f in fields), \
            "a blank row is a composed length that stopped matching"         # 40

        # ── (E) a read-only path says so ──────────────────────────────
        assert_rpc_error(lambda: tf.intervene("/external/placements", None),
                         data="ReadOnly")                                    # 41
        assert_rpc_error(lambda: tf.intervene("/external/visible_total", 1),
                         data="ReadOnly")                                    # 42
        assert_rpc_error(lambda: tf.intervene("/external/count", 9),
                         data="ReadOnly")                                    # 43
        assert_rpc_error(lambda: tf.intervene("/external/labels", []),
                         data="ReadOnly")                                    # 44
        assert_rpc_error(lambda: tf.intervene("/external/no_such_path", 1),
                         data="UnknownIntervenePath")                        # 45
        assert_rpc_error(lambda: tf.query("/external/no_such_path"),
                         data="UnknownIntrospectPath")                       # 46

        # ── (F) the header still works ────────────────────────────────
        tf.invoke("/external/move_section", "0:2")
        wait_until(lambda: _h(tf, "order")[2] == 0, desc="Name dragged right")
        assert_eq(_h(tf, "labels"), ["Type", "Size", "Name", "Modified", "Owner"])
                                                                             # 47
        assert_eq(_h(tf, "section_size.0"), BOOT_W[0],
                  "and its width travelled with it, keyed logically")        # 48
        tf.intervene("/external/stretch_last_section", True)
        wait_until(lambda: _h(tf, "stretch_last_section"), desc="the rule is on")
        assert_eq(_h(tf, "visible_total"), 640, "the R1498 rule still fills") # 49
        assert_eq(_h(tf, "effective_resize_mode.4"), "stretch",
                  "and the path that was undeclared reports it")             # 50
        assert find_by_tag(_paint(tf), f"{HDR}#0") is not None, \
            "the strip is still painted"                                     # 51


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1501 §5.27 §2#2 §2#7 — a surface declares what it answers", body,
    ))
