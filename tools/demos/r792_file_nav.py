#!/usr/bin/env python3
"""R792 §5.15 §5.40 — keyboard navigation over the file list.

Drives the `hello-file-manager` binding via JSON-RPC. The own-rendered file
UI gains the Files/Explorer keyboard model: the entry list is a single tab
stop with an internal **roving cursor**, the arrow keys move it, the shell
rings the active row (`aria-activedescendant`), and `Enter` activates it
(navigate a folder / pick a file). The new substrate is small — a `cursor`
holder on `DirectoryState` + the `dir_nav_key` controller — and reuses the
shared `clamp_nav` policy + `scroll_offset_to_reveal` the virtualized
list/grid navigation already uses (R777).

The decisive witness is scene-as-data (§2 #7): a key moves the cursor, the
injected focus-ring overlay box (`ai-overlay/focus-ring`, the R705 ring
substrate) tracks the cursor row, and `Enter` changes `cwd` / `selected` —
all observable over RPC with no pixels required. R792 adds no new paint: the
focus ring's on-screen rendering is the pre-existing R705 / R694 pixel
guarantee, and this round's contribution (the ring framing the *cursor* row,
the cursor moving on every key) is fully scene-as-data, exactly as the R694
ring demo verifies the ring RPC-only.

  (A) boot — list rendered; the effective cursor defaults to the first row;
      nothing focused yet, so no ring.
  (B) focus + roving — focus the list; the ring frames the cursor row;
      ArrowDown/Up step (clamped, no wrap); Home/End jump; the ring follows.
  (C) Enter activates — Enter on a folder row navigates in; in a tall folder
      End scrolls a never-materialized row into view; Enter on a file picks
      it.
  (D) AI-first cursor axis — `query cursor` reports the active row;
      `intervene cursor` moves it (the aria-activedescendant write peer).
  (E) pointer + RPC unaffected — a real `scene/click` still selects a row.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
    wait_query,
    wait_snap,
    wait_until,
)

EXAMPLE = "hello-file-manager"
WIN = (480, 540)
DIR = "fb_dir"
RING = "ai-overlay/focus-ring"


def dpath(slot: str) -> str:
    return f"/{DIR}/external/{slot}"


def present_rows(snap) -> set[int]:
    out: set[int] = set()
    for tag in abs_rects_of(snap):
        if tag.startswith(f"{DIR}#") and tag.split("#", 1)[1].isdigit():
            out.add(int(tag.split("#", 1)[1]))
    return out


def cursor(tf) -> int | None:
    return tf.query(dpath("cursor"))


def ring_row_aligned(snap, row: int) -> bool:
    """The injected focus-ring box frames the given file row (same y band)."""
    rects = abs_rects_of(snap)
    ring = rects.get(RING)
    rowr = rects.get(f"{DIR}#{row}")
    if ring is None or rowr is None:
        return False
    # The ring is positioned over the cursor row; allow a couple px of
    # M3 ring-offset slack between the two y origins.
    return abs(ring[1] - rowr[1]) <= 4


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        rects = abs_rects_of(snap)

        # ── (A) boot: list rendered, cursor defaults to first row ───
        assert f"{DIR}#0" in rects, "file rows rendered at boot"
        assert DIR in rects, "the list region is a tagged (focusable) node"
        assert_eq(tf.query(dpath("cwd")), "/proj", "boot directory is /proj")
        assert_eq(cursor(tf), 0, "effective cursor defaults to the first row")
        assert_eq(tf.query(dpath("selected")), None, "nothing selected at boot")
        assert_eq(tf.query(dpath("count")), 4, "four entries in /proj")
        assert RING not in rects, "no focus ring before the list is focused"

        # ── (B) focus the list → ring on the cursor row, then rove ──
        tf.request("focus/set", {"tag": DIR})
        wait_until(
            lambda: tf.request("focus/get").result.get("focused") == DIR,
            desc="the list owns shell focus",
        )
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert RING in abs_rects_of(snap), "focusing the list paints a focus ring"
        assert ring_row_aligned(snap, 0), "the ring frames the cursor row (row 0)"

        tf.key(path=DIR, name="ArrowDown")
        wait_query(tf, dpath("cursor"), 1, desc="ArrowDown advances the cursor")
        tf.key(path=DIR, name="ArrowDown")
        tf.key(path=DIR, name="ArrowDown")
        wait_query(tf, dpath("cursor"), 3, desc="two more ArrowDowns reach the last /proj row")
        tf.key(path=DIR, name="ArrowDown")
        # Clamp no-op: the dispatch commits before the response.
        assert_eq(cursor(tf), 3, "ArrowDown at the last row clamps (no wrap)")
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert ring_row_aligned(snap, 3), "the ring followed the cursor to row 3"

        tf.key(path=DIR, name="ArrowUp")
        wait_query(tf, dpath("cursor"), 2, desc="ArrowUp steps back")
        tf.key(path=DIR, name="Home")
        wait_query(tf, dpath("cursor"), 0, desc="Home jumps to the first row")
        tf.key(path=DIR, name="ArrowUp")
        # Clamp no-op: the dispatch commits before the response.
        assert_eq(cursor(tf), 0, "ArrowUp at the top clamps (no wrap)")
        tf.key(path=DIR, name="End")
        wait_query(tf, dpath("cursor"), 3, desc="End jumps to the last row")

        # ── (C) Enter activates: navigate a folder, then pick a file ─
        # /proj sorts dirs-first: row 0 = assets, row 1 = src (the tall
        # folder). Move to src and Enter into it.
        tf.key(path=DIR, name="Home")
        wait_query(tf, dpath("cursor"), 0, desc="Home returns to the first /proj row")
        tf.key(path=DIR, name="ArrowDown")  # row 1 = src
        wait_query(tf, dpath("cursor"), 1, desc="cursor on the src row")
        tf.key(path=DIR, name="Enter")
        wait_query(tf, dpath("cwd"), "/proj/src", desc="Enter on a folder navigates in")
        assert_eq(cursor(tf), 0, "the new folder's cursor resets to its first row")
        src_count = tf.query(dpath("count"))
        assert src_count > 9, f"src has more rows ({src_count}) than the 9-row viewport"

        # End scrolls a never-materialized deep row into view.
        tf.key(path=DIR, name="End")
        wait_query(tf, dpath("cursor"), src_count - 1, desc="End moves to the last src row")
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert (src_count - 1) in present_rows(snap), "the deep cursor row is now rendered"
        assert 0 not in present_rows(snap), "the first row scrolled out of view"
        assert ring_row_aligned(snap, src_count - 1), "the ring tracks the deep cursor row"
        tf.key(path=DIR, name="Home")
        snap = wait_snap(
            tf, lambda s: 0 in present_rows(s), viewport=WIN,
            desc="Home scrolled the cursor back to the top",
        )

        # Enter on a file row picks it (src row 0 is a file — lib.rs).
        tf.key(path=DIR, name="Enter")
        wait_query(
            tf, dpath("selected"), "/proj/src/lib.rs",
            desc="Enter on a file row picks it",
        )
        assert_eq(cursor(tf), 0, "picking a file leaves the cursor on its row")

        # ── (D) AI-first cursor axis (query + intervene) ────────────
        tf.intervene(dpath("cursor"), 2)
        wait_query(tf, dpath("cursor"), 2, desc="intervene moved the cursor (aria-activedescendant write)")
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert ring_row_aligned(snap, 2), "the ring follows an RPC-set cursor"
        tf.intervene(dpath("cursor"), None)
        wait_query(tf, dpath("cursor"), 0, desc="intervene Null resets the cursor to the first row")

        # ── (E) pointer click still selects (unaffected by keyboard) ─
        # Back up to /proj and click a file row.
        tf.invoke(dpath("up"), None)
        wait_query(tf, dpath("cwd"), "/proj", desc="up() navigates back to /proj")
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert 2 in present_rows(snap), "row 2 (Cargo.toml) visible for the click test"
        tf.click(path=f"{DIR}#2")
        wait_query(
            tf, dpath("selected"), "/proj/Cargo.toml",
            desc="a pointer click still selects, alongside the keyboard cursor",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R792 §5.15 §5.40 — keyboard navigation over the file list", body))
