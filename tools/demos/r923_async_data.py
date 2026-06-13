#!/usr/bin/env python3
"""R923 §5.22 §5.23 §2 async/lazy data view — `hello-async-data`.

The first framework consumer that renders a view over an **out-of-memory,
page-fetched** data source: a paged asset browser whose rows arrive
asynchronously through the §5.22 `Resource` carrier, an `Effect`-driven
auto-refetch on the `page` / `reload_nonce` Signals, and the shell-polled
`LocalTaskPump`. The three `ResourceState` arms — Loading / Ready / Error —
are all observed as DATA through `scene/snapshot` (§2 #7 scene-as-data); no
pixels needed.

ZERO-FLAKE latency model: each fetch is a deterministic deferred future
(`Pending` N times → resolve), and every `scene/snapshot from=paint`
advances the pump one step (the poll lives in
`compute_paint_scene_internal`). So the demo's own snapshot polling drives
`Loading → Ready/Error` — `wait_snap` on the `Loading` line is guaranteed to
catch it before the terminal state, with no wall-clock race (same discipline
as the R761.1 deferred file-dialog demo).

Run from the workspace root:
    cargo build -p hello-async-data --release
    python3 tools/demos/r923_async_data.py
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
    wait_snap,
)

VIEWPORT = (460, 380)

PREV_TAG = "async_prev"
NEXT_TAG = "async_next"
RELOAD_TAG = "async_reload"
STATUS_TAG = "pager_status"
LIST_TAG = "asset_list"
LIST_NOTE_TAG = "asset_list_note"

# Mirror the binding's non-ASCII glyphs (escaped in Rust source; raw here).
ELLIPSIS = "…"  # …
EMDASH = "—"  # —


def row_tag(global_index: int) -> str:
    return f"asset_row_{global_index}"


# Mirror the binding's `KINDS` rotation + `page_rows` size formula so the
# expected row descriptor stays in lockstep with the Rust SSOT.
KINDS = [
    ("Texture", "png"),
    ("Mesh", "obj"),
    ("Audio", "wav"),
    ("Script", "rs"),
    ("Shader", "wgsl"),
    ("Scene", "pinion"),
]


def expected_label(i: int) -> str:
    kind, ext = KINDS[i % len(KINDS)]
    size = (i * 37 + 11) % 900 + 8
    return f"asset_{i:03d}.{ext} ({kind}, {size} KB)"


def find_text(node, content: str):
    """Depth-first search for a Text node whose content == content."""
    if not isinstance(node, dict):
        return None
    if node.get("type") == "Text" and node.get("content") == content:
        return node
    for child in node.get("children", []) or []:
        hit = find_text(child, content)
        if hit is not None:
            return hit
    return None


def text_under(snap, tag: str):
    """The first Text content found under the container tagged `tag`."""
    node = find_by_tag(snap, tag)
    if node is None:
        return None

    def first_text(n):
        if not isinstance(n, dict):
            return None
        if n.get("type") == "Text":
            return n.get("content")
        for child in n.get("children", []) or []:
            hit = first_text(child)
            if hit is not None:
                return hit
        return None

    return first_text(node)


def status_of(snap):
    return text_under(snap, STATUS_TAG)


def wait_status(d, expected: str, where: str):
    """Poll the paint scene until the pager status line reaches `expected`."""
    return wait_snap(
        d,
        lambda s: status_of(s) == expected,
        viewport=VIEWPORT,
        desc=f"status == {expected!r} ({where})",
    )


def assert_three_buttons(snap, where: str) -> None:
    for tag in (PREV_TAG, NEXT_TAG, RELOAD_TAG):
        assert find_by_tag(snap, tag) is not None, f"{tag} present ({where})"


def assert_rows_present(snap, indices, where: str) -> None:
    for i in indices:
        assert find_by_tag(snap, row_tag(i)) is not None, f"{row_tag(i)} present ({where})"


def assert_rows_absent(snap, indices, where: str) -> None:
    for i in indices:
        assert find_by_tag(snap, row_tag(i)) is None, f"{row_tag(i)} absent ({where})"


def body() -> None:
    with RpcSubprocess("hello-async-data") as d:
        # ── boot: page 0's fetch resolves through the pump (Effect eager
        #    run). The terminal Ready state is deterministic; the demo's
        #    own snapshot polls drive the pump to it. ───────────────────
        snap = wait_status(d, f"Page 1 of 5 {EMDASH} 6 assets", "boot → page 1 ready")
        assert find_text(snap, "Async asset browser (paged, lazy-loaded)") is not None, "title"
        assert_three_buttons(snap, "boot")
        assert find_by_tag(snap, LIST_TAG) is not None, "list panel present (boot)"
        assert_rows_present(snap, range(0, 6), "boot page 1")
        assert_rows_absent(snap, [6], "boot has no page-2 rows")
        # The first row carries the SSOT descriptor (name + kind + size).
        assert find_text(snap, expected_label(0)) is not None, f"row 0 label {expected_label(0)!r}"

        # ── Next → page 2: Loading is observable first, then the rows. ──
        d.click(path=NEXT_TAG)
        wait_status(d, f"Loading page 2 of 5{ELLIPSIS}", "page 2 Loading placeholder")
        snap = wait_status(d, f"Page 2 of 5 {EMDASH} 6 assets", "page 2 ready")
        assert_rows_present(snap, range(6, 12), "page 2 rows")
        assert_rows_absent(snap, [0, 5], "page-1 rows gone on page 2")
        assert find_by_tag(snap, LIST_NOTE_TAG) is None, "no placeholder when Ready"

        # ── Next → page 3 = ERROR_PAGE: scripted-unavailable. Loading then
        #    the Error arm, surfaced both in the pager + the list note. ──
        d.click(path=NEXT_TAG)
        wait_status(d, f"Loading page 3 of 5{ELLIPSIS}", "page 3 Loading")
        snap = wait_status(
            d,
            f"Page 3 of 5 {EMDASH} error: source unavailable for page 3",
            "page 3 Error arm",
        )
        note = text_under(snap, LIST_NOTE_TAG)
        assert note is not None and "Could not load" in note, f"error note ({note!r})"
        assert_rows_absent(snap, range(12, 18), "no rows on the errored page")
        assert_three_buttons(snap, "after error page")

        # ── Next → page 4: recovers (errors are per-page, nav is not
        #    blocked — the page Signal advances regardless). ─────────────
        d.click(path=NEXT_TAG)
        snap = wait_status(d, f"Page 4 of 5 {EMDASH} 6 assets", "page 4 recovers")
        assert_rows_present(snap, range(18, 24), "page 4 rows")

        # ── Next → page 5 (last). ──────────────────────────────────────
        d.click(path=NEXT_TAG)
        snap = wait_status(d, f"Page 5 of 5 {EMDASH} 6 assets", "page 5 ready")
        assert_rows_present(snap, [24, 29], "page 5 first + last rows")

        # ── Next at the last page clamps: the reducer's `.min(LAST)` makes
        #    it a no-op, so the status stays on page 5 (the painted Next is
        #    Disabled; the clamp is the behavioural guard). ──────────────
        d.click(path=NEXT_TAG)
        snap = wait_status(d, f"Page 5 of 5 {EMDASH} 6 assets", "Next clamps at last page")
        assert_rows_present(snap, [24], "still on page 5 after clamped Next")

        # ── Reload → re-fetch the SAME page via the nonce dep: Loading
        #    then the same page 5 rows (the Effect re-fired without a page
        #    change). ─────────────────────────────────────────────────────
        d.click(path=RELOAD_TAG)
        wait_status(d, f"Loading page 5 of 5{ELLIPSIS}", "Reload re-enters Loading")
        snap = wait_status(d, f"Page 5 of 5 {EMDASH} 6 assets", "Reload restores page 5")
        assert_rows_present(snap, range(24, 30), "page 5 rows after reload")

        # ── Prev → page 4: navigation back also re-fetches. ────────────
        d.click(path=PREV_TAG)
        wait_status(d, f"Loading page 4 of 5{ELLIPSIS}", "Prev re-enters Loading")
        snap = wait_status(d, f"Page 4 of 5 {EMDASH} 6 assets", "Prev → page 4")
        assert_rows_present(snap, [18], "page 4 row after Prev")
        assert_rows_absent(snap, [24], "page-5 rows gone after Prev")

        # ── Prev all the way to page 1, then Prev clamps. ──────────────
        d.click(path=PREV_TAG)  # page 3 (error)
        wait_status(
            d,
            f"Page 3 of 5 {EMDASH} error: source unavailable for page 3",
            "Prev revisits the error page",
        )
        d.click(path=PREV_TAG)  # page 2
        wait_status(d, f"Page 2 of 5 {EMDASH} 6 assets", "Prev → page 2")
        d.click(path=PREV_TAG)  # page 1
        wait_status(d, f"Page 1 of 5 {EMDASH} 6 assets", "Prev → page 1")
        # Prev at page 1 clamps (saturating_sub) — status stays page 1.
        d.click(path=PREV_TAG)
        snap = wait_status(d, f"Page 1 of 5 {EMDASH} 6 assets", "Prev clamps at first page")
        assert_rows_present(snap, range(0, 6), "page 1 rows after round-trip")
        assert_three_buttons(snap, "final")
        assert find_by_tag(snap, STATUS_TAG) is not None, "status region persists"


if __name__ == "__main__":
    sys.exit(run_demo("R923 async/lazy paged data view", body))
