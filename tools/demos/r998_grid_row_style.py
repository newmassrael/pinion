#!/usr/bin/env python3
"""R998 §5.40 — per-row style rules at scale E2E.

Drives `hello-grid-row-style` via JSON-RPC. A 10,000-row virtualized data-grid
whose rows paint by declarative coloring rules (Wireshark "coloring rules" /
dlt-viewer filter-colours). Each rule is a GridFilter predicate -> a tint;
first match wins; the AI adds one with `invoke "add_rule"
"<filter>;<bg>;<fg>"`. The match half REUSES the R997 GridFilter, so coloring
and filtering speak the same wire vocab.

  * primary `VirtualSelectExternal` (R746) at `vtbl` — source-indexed selection.
  * extra `GridSortExternal` (R778) at `vsort` — sort over the coloured rows.
  * extra `RowStyleExternal` (R998) at `vrules` — the rule list + per-row match
    introspection (`match.<row>` / `tint.<row>` use the SAME resolver the paint
    body consumes, so they ARE the scene-as-data witness for the coloring).

  Data (per data row id):
    col 0 Name   = "<Category><id>"          (presentational)
    col 1 Score  = (id * 7919) % 1000        (the numeric `>= 900` rule column)
    col 2 Status = ["Idle","Active","Done"][id % 3]   (the exact `=` rule column)

  Seed rules: rule 0 `2=Done` -> red-on-white; rule 1 `1>=900` -> green-on-black.

  (A) boot — 2 rules seeded; rule wires + rows introspectable; grid paints.
  (B) per-row match (the AI-first witness) — a Done row -> rule 0, a high-Score
      non-Done row -> rule 1, neither -> -1; tint.<row> and matched_rows agree
      with an independent oracle.
  (C) first-match precedence — a Done AND high-Score row matches rule 0 (red),
      not rule 1.
  (D) coloring is orthogonal — selecting a row does not change its match; a
      re-sort does not change which rule colours a source row.
  (E) dynamics — `clear` reverts every row to no-tint; `add_rule` re-colours;
      `intervene "rules"` restores the whole list (wire round-trip).

  Phase 2 — PIXELS (PINION_SCREENSHOT): the grid paints (header vs cell bands).
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    WORKSPACE_ROOT,
    abs_rects_of,
    assert_eq,
    indexed_tags,
    read_png_rgba8,
    run_demo,
    sample_png_points,
)

EXAMPLE = "hello-grid-row-style"
WIN = (400, 480)
N = 10_000
NCOLS = 3
GRID_TAG = "vtbl"
SORT_TAG = "vsort"
RULES_TAG = "vrules"

STATUS = ["Idle", "Active", "Done"]
RULE0_WIRE = "2=Done;#c82828;#ffffff"
RULE1_WIRE = "1>=900;#28a03c;#000000"


def score(i: int) -> int:
    return (i * 7919) % 1000


def status_of(i: int) -> str:
    return STATUS[i % 3]


def oracle_rule(i: int) -> int:
    """First seed rule colouring row i (Done -> 0, else Score>=900 -> 1, else -1)."""
    if status_of(i) == "Done":
        return 0
    if score(i) >= 900:
        return 1
    return -1


def rule_count(d):
    return d.query(f"/{RULES_TAG}/external/rule_count")


def match_of(d, row: int):
    return d.query(f"/{RULES_TAG}/external/match.{row}")


def tint_of(d, row: int):
    return d.query(f"/{RULES_TAG}/external/tint.{row}")


def source_at(d, pos: int):
    return d.query(f"/{SORT_TAG}/external/source_at.{pos}")


def present_rows(snap) -> list[int]:
    return indexed_tags(abs_rects_of(snap), "vtbl_row")


def first_with(pred) -> int:
    for i in range(N):
        if pred(i):
            return i
    raise AssertionError("no matching row")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        rects = abs_rects_of(snap)

        # ── (A) boot: 2 rules seeded, grid paints ───────────────────
        assert GRID_TAG in rects, "grid container present at boot"
        assert "vtbl_hrow" in rects, "frozen header row present"
        assert_eq(rule_count(tf), 2, "two seed rules at boot")
        assert_eq(tf.query(f"/{RULES_TAG}/external/rows"), N, "rule proxy knows the row count")
        assert_eq(tf.query(f"/{RULES_TAG}/external/rule.0"), RULE0_WIRE, "rule 0 = Done -> red")
        assert_eq(tf.query(f"/{RULES_TAG}/external/rule.1"), RULE1_WIRE, "rule 1 = Score>=900 -> green")
        assert_eq(
            tf.query(f"/{RULES_TAG}/external/rules"),
            f"{RULE0_WIRE}|{RULE1_WIRE}",
            "whole rule list wire round-trips",
        )
        assert len(present_rows(snap)) < 30, "virtualized: small window"

        # ── (B) per-row match — the AI-first scene-as-data witness ──
        done_row = first_with(lambda i: status_of(i) == "Done")
        high_row = first_with(lambda i: score(i) >= 900 and status_of(i) != "Done")
        none_row = first_with(lambda i: oracle_rule(i) == -1)
        assert_eq(match_of(tf, done_row), 0, f"Done row {done_row} -> rule 0")
        assert_eq(match_of(tf, high_row), 1, f"high-Score row {high_row} -> rule 1")
        assert_eq(match_of(tf, none_row), -1, f"plain row {none_row} -> no rule")
        assert_eq(tint_of(tf, done_row), "#c82828;#ffffff", "Done row paints red-on-white")
        assert_eq(tint_of(tf, high_row), "#28a03c;#000000", "high row paints green-on-black")
        assert_eq(tint_of(tf, none_row), "none", "plain row has no tint")
        # match introspection agrees with the oracle across a sample sweep.
        for row in range(0, 300):
            assert_eq(match_of(tf, row), oracle_rule(row), f"row {row} match")
        # matched_rows = the independently-counted coloured rows.
        oracle_matched = sum(1 for i in range(N) if oracle_rule(i) != -1)
        assert_eq(tf.query(f"/{RULES_TAG}/external/matched_rows"), oracle_matched, "matched_rows count")

        # ── (C) first-match precedence (Done AND high-Score -> red) ─
        both = first_with(lambda i: status_of(i) == "Done" and score(i) >= 900)
        assert_eq(match_of(tf, both), 0, f"row {both} matches both -> earlier Done rule wins")
        assert_eq(tint_of(tf, both), "#c82828;#ffffff", "painted red, not green")

        # ── (D) coloring is orthogonal to selection + sort ──────────
        assert_eq(tf.invoke("/external/select", done_row), done_row, "select the Done row")
        assert_eq(tf.query("/external/selected"), done_row, "row selected")
        assert_eq(match_of(tf, done_row), 0, "selection does not change the row's matched rule")
        # Re-sort by Score: the coloured source rows keep their rule (coloring
        # is per SOURCE row, not visual position).
        tf.invoke(f"/{SORT_TAG}/external/cycle_sort", 1)  # Score ascending
        assert_eq(tf.query(f"/{SORT_TAG}/external/sort"), "1:ascending", "sort applied")
        for pos in range(6):
            src = source_at(tf, pos)
            assert_eq(match_of(tf, src), oracle_rule(src), f"visual {pos} (source {src}) keeps its rule")
        tf.invoke(f"/{SORT_TAG}/external/cycle_sort", 1)  # desc
        tf.invoke(f"/{SORT_TAG}/external/cycle_sort", 1)  # none

        # ── (E) dynamics: clear / add_rule / restore ────────────────
        assert_eq(tf.invoke(f"/{RULES_TAG}/external/clear", None), 0, "clear returns 0 rules")
        assert_eq(rule_count(tf), 0, "no rules after clear")
        assert_eq(match_of(tf, done_row), -1, "no rule matches after clear")
        assert_eq(tf.query(f"/{RULES_TAG}/external/matched_rows"), 0, "no coloured rows after clear")
        assert_eq(tint_of(tf, done_row), "none", "no tint after clear")
        # add_rule re-colours, returning the new count.
        assert_eq(tf.invoke(f"/{RULES_TAG}/external/add_rule", RULE0_WIRE), 1, "add_rule -> 1")
        assert_eq(match_of(tf, done_row), 0, "Done row coloured again")
        assert_eq(match_of(tf, high_row), -1, "high row not yet coloured (rule 1 absent)")
        assert_eq(tf.invoke(f"/{RULES_TAG}/external/add_rule", RULE1_WIRE), 2, "add_rule -> 2")
        assert_eq(match_of(tf, high_row), 1, "high row coloured after rule 1 re-added")
        # intervene restores the whole list from a wire payload.
        tf.intervene(f"/{RULES_TAG}/external/rules", None)
        assert_eq(rule_count(tf), 0, "intervene Null clears")
        tf.intervene(f"/{RULES_TAG}/external/rules", f"{RULE0_WIRE}|{RULE1_WIRE}")
        assert_eq(rule_count(tf), 2, "intervene restores the list")
        assert_eq(match_of(tf, both), 0, "restored rules colour as before")
        # a malformed rule payload is rejected (add_rule needs filter;bg;fg).
        try:
            tf.invoke(f"/{RULES_TAG}/external/add_rule", "garbage")
            raise AssertionError("a non-rule payload must be rejected")
        except RpcError:
            pass
        assert_eq(rule_count(tf), 2, "rule list unchanged after the rejected add")

    # ── Phase 2 — live pixels (boot frame: grid paints) ────────────
    snap, rects = _boot_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == WIN, f"screenshot {img.width}x{img.height} != window {WIN}"
    hx, hy, hw, hh = rects["vtbl_ch0"]
    rx, ry, rw, rh = rects["vtbl#0_0"]
    h_col, r_col = sample_png_points(img, [(hx + hw // 2, hy + hh // 2), (rx + rw // 2, ry + rh // 2)])
    assert h_col is not None and r_col is not None, "header + data cell sampled"


def _boot_snapshot_and_rects():
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        return snap, abs_rects_of(snap)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r998-")) / "rowstyle.png"
    binary = WORKSPACE_ROOT / "target" / "release" / EXAMPLE
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", EXAMPLE, "--quiet", "--release",
    ]
    env = os.environ.copy()
    env["PINION_SCREENSHOT"] = str(out)
    res = subprocess.run(
        cmd, cwd=WORKSPACE_ROOT, env=env,
        capture_output=True, text=True, check=False, timeout=120.0,
    )
    if res.returncode != 0:
        raise AssertionError(
            f"PINION_SCREENSHOT capture exited {res.returncode}:\n  stderr: {res.stderr!r}"
        )
    if not out.exists():
        raise AssertionError(f"PINION_SCREENSHOT produced no file at {out}")
    return out


if __name__ == "__main__":
    sys.exit(run_demo("R998 §5.40 — per-row style rules at scale", body))
