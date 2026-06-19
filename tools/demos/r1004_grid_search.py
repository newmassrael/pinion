#!/usr/bin/env python3
"""R1004 §5.27 §5.40 — search-and-jump cursor at scale E2E.

Drives `hello-grid-search` via JSON-RPC. A 10,000-row virtualized data-grid
where a GridFilter query drives a match CURSOR (next / prev / jump) that scrolls
each match into view, and EVERY row stays visible — that is what distinguishes
search from the filter axis (`hello-grid-filter`), which removes the rows it
rejects.

  * primary `StubExternal` at `vtbl` — the grid anchor (input + a11y bounds).
  * extra `RowSearchExternal` (R1004) at `vsearch` — the query + the match
    cursor. `query`/`match_count`/`current`/`current_row`/`match.<i>` ARE the
    scene-as-data witness for the search; `set_query`/`next`/`prev`/`jump`
    drive the cursor and (via the attached body scroll) the jump-into-view.

  The search query IS the R997 GridFilter — search is its THIRD consumer after
  the filter axis (removes) and the R998 coloring engine (tints). The wire vocab
  is shared: `0~Delta` (Name contains Delta) reads exactly as a filter would.

  Data (per data row id):
    col 0 Name   = "<Category><id>"          (the substring `~` search column)
    col 1 Score  = (id * 7919) % 1000        (the numeric `>=` search column)
    col 2 Status = ["Idle","Active","Done"][id % 3]

  Seed query: `0~Delta` — Name contains "Delta" (the Delta rows are id % 5 == 3).

  (A) boot — seed query active; cursor on the first match; grid paints.
  (B) match set (the AI-first witness) — match.<i> = the i-th matching source
      row, agreeing with an independent oracle; out-of-range -> Null.
  (C) cursor walk — next/prev/jump move the cursor and return the new source
      row, wrapping; a jump scrolls the match into the visible window.
  (D) search keeps ALL rows — the window holds consecutive source rows incl.
      NON-matches around the cursor (a filter would have removed them).
  (E) numeric query reuse — `1>=990` (R997 numeric `>=`) keeps a sparse set;
      every match really has Score >= 990.
  (F) dynamics — clear empties the search; set_query / intervene restore it;
      a phantom-column query clamps to no search; an out-of-range jump no-ops.

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
    RpcSubprocess,
    WORKSPACE_ROOT,
    abs_rects_of,
    assert_eq,
    read_png_rgba8,
    run_demo,
    sample_png_points,
)

EXAMPLE = "hello-grid-search"
WIN = (400, 480)
N = 10_000
NCOLS = 3
GRID_TAG = "vtbl"
SEARCH_TAG = "vsearch"

CATEGORIES = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"]
STATUS = ["Idle", "Active", "Done"]
SEED_WIRE = "0~Delta"


def score(i: int) -> int:
    return (i * 7919) % 1000


def is_delta(i: int) -> bool:
    return i % len(CATEGORIES) == 3


def delta_rows() -> list[int]:
    return [i for i in range(N) if is_delta(i)]


def q(d, path: str):
    return d.query(f"/{SEARCH_TAG}/external/{path}")


def match_at(d, i: int):
    return d.query(f"/{SEARCH_TAG}/external/match.{i}")


def set_query(d, wire):
    return d.invoke(f"/{SEARCH_TAG}/external/set_query", wire)


def present_rows(snap) -> list[int]:
    """Source ids of the data-row strips currently in the painted window."""
    out: list[int] = []
    for tag in abs_rects_of(snap):
        if tag.startswith("vtbl_row"):
            suffix = tag[len("vtbl_row"):]
            if suffix.isdigit():
                out.append(int(suffix))
    return out


def first_with(pred) -> int:
    for i in range(N):
        if pred(i):
            return i
    raise AssertionError("no matching row")


def body() -> None:
    deltas = delta_rows()
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        rects = abs_rects_of(snap)

        # ── (A) boot: seed query active, cursor on first match ──────
        assert GRID_TAG in rects, "grid container present at boot"
        assert "vtbl_hrow" in rects, "frozen header row present"
        assert_eq(q(tf, "query"), SEED_WIRE, "seed query = Name contains Delta")
        assert_eq(q(tf, "count"), N, "search knows the full row count")
        assert_eq(q(tf, "match_count"), len(deltas), "match_count = the Delta rows")
        assert_eq(q(tf, "current"), 0, "cursor lands on the first match")
        assert_eq(q(tf, "current_row"), deltas[0], "first match is source row 3")
        assert len(present_rows(snap)) < 30, "virtualized: small window"

        # ── (B) match set — the AI-first scene-as-data witness ──────
        for i in range(120):
            assert_eq(match_at(tf, i), deltas[i], f"match.{i} = source {deltas[i]}")
        assert_eq(match_at(tf, len(deltas)), None, "one past the last match -> Null")
        assert_eq(match_at(tf, 10_000_000), None, "far out-of-range match -> Null")

        # ── (C) cursor walk: next / prev / jump, wrapping ───────────
        assert_eq(tf.invoke(f"/{SEARCH_TAG}/external/next", None), deltas[1], "next -> 2nd match")
        assert_eq(tf.invoke(f"/{SEARCH_TAG}/external/next", None), deltas[2], "next -> 3rd match")
        assert_eq(q(tf, "current"), 2, "cursor advanced to position 2")
        assert_eq(tf.invoke(f"/{SEARCH_TAG}/external/prev", None), deltas[1], "prev -> back to 2nd")
        # Jump deep: match position 100 = a source row well below the window.
        target = deltas[100]
        assert_eq(tf.invoke(f"/{SEARCH_TAG}/external/jump", 100), target, "jump -> 101st match row")
        assert_eq(q(tf, "current"), 100, "cursor at position 100")
        snap2 = tf.snapshot(source="paint", viewport=WIN)
        win2 = present_rows(snap2)
        assert target in win2, f"jump scrolled match {target} into view: window {min(win2)}..{max(win2)}"

        # ── (D) search keeps ALL rows (unlike a filter) ─────────────
        # The window around a deep match holds CONSECUTIVE source ids — incl.
        # non-Delta rows a filter would have removed.
        assert target - 1 in win2 or target + 1 in win2, "a non-match neighbour is still present"
        non_matches = [r for r in win2 if not is_delta(r)]
        assert non_matches, f"window has non-matching rows (search != filter): {sorted(win2)}"
        # Wrap-around: jumping back to the first match returns to the top.
        assert_eq(tf.invoke(f"/{SEARCH_TAG}/external/jump", 0), deltas[0], "jump 0 -> first match")
        win0 = present_rows(tf.snapshot(source="paint", viewport=WIN))
        assert deltas[0] in win0 and min(win0) == 0, "first match revealed at the top"
        # prev from the first match wraps to the last.
        assert_eq(tf.invoke(f"/{SEARCH_TAG}/external/prev", None), deltas[-1], "prev wraps to last match")
        assert_eq(tf.invoke(f"/{SEARCH_TAG}/external/next", None), deltas[0], "next wraps to first match")

        # ── (E) numeric query reuse (R997 numeric `>=`) ─────────────
        n_hi = sum(1 for i in range(N) if score(i) >= 990)
        assert_eq(set_query(tf, "1>=990"), n_hi, "set_query returns the sparse match_count")
        assert_eq(q(tf, "query"), "1>=990", "numeric query wire round-trips")
        first_hi = first_with(lambda i: score(i) >= 990)
        assert_eq(q(tf, "current_row"), first_hi, "cursor lands on the first high-Score row")
        # Every match really satisfies the predicate.
        hi_rows = [i for i in range(N) if score(i) >= 990]
        for i in range(0, min(40, len(hi_rows))):
            assert_eq(match_at(tf, i), hi_rows[i], f"match.{i} score>=990")

        # ── (F) dynamics: clear / restore / clamp / out-of-range ────
        assert_eq(tf.invoke(f"/{SEARCH_TAG}/external/clear", None), 0, "clear -> 0 matches")
        assert_eq(q(tf, "query"), "none", "query cleared")
        assert_eq(q(tf, "current"), None, "no cursor after clear")
        assert_eq(q(tf, "current_row"), None, "no current row after clear")
        # set_query restores a search.
        assert_eq(set_query(tf, "2=Done"), sum(1 for i in range(N) if STATUS[i % 3] == "Done"),
                  "set_query 2=Done -> the Done rows")
        assert_eq(q(tf, "current_row"), first_with(lambda i: STATUS[i % 3] == "Done"),
                  "cursor on the first Done row")
        # intervene sets the query too (the admin/restore path).
        tf.intervene(f"/{SEARCH_TAG}/external/query", SEED_WIRE)
        assert_eq(q(tf, "query"), SEED_WIRE, "intervene restores the Delta query")
        assert_eq(q(tf, "match_count"), len(deltas), "restored match set")
        # a phantom-column query (col 9 in a 3-wide grid) clamps to no search.
        assert_eq(set_query(tf, "9~Delta"), 0, "phantom-column query -> no matches")
        assert_eq(q(tf, "query"), "none", "phantom-column query collapsed to none")
        # an out-of-range jump is a no-op (no panic, cursor unchanged).
        set_query(tf, SEED_WIRE)
        tf.invoke(f"/{SEARCH_TAG}/external/jump", 5)
        assert_eq(q(tf, "current"), 5, "cursor at position 5")
        tf.invoke(f"/{SEARCH_TAG}/external/jump", 10_000_000)
        assert_eq(q(tf, "current"), 5, "out-of-range jump left the cursor unchanged")

    # ── Phase 2 — live pixels (boot frame: grid paints) ────────────
    snap, rects = _boot_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == WIN, f"screenshot {img.width}x{img.height} != window {WIN}"
    hx, hy, hw, hh = rects["vtbl_ch0"]
    rx, ry, rw, rh = rects["vtbl#3_0"]  # the seeded first match (source row 3) is in view
    h_col, r_col = sample_png_points(img, [(hx + hw // 2, hy + hh // 2), (rx + rw // 2, ry + rh // 2)])
    assert h_col is not None and r_col is not None, "header + data cell sampled"


def _boot_snapshot_and_rects():
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        return snap, abs_rects_of(snap)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r1004-")) / "search.png"
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
    sys.exit(run_demo("R1004 §5.27 §5.40 — search-and-jump cursor at scale", body))
