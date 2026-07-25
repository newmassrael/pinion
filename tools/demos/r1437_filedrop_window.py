#!/usr/bin/env python3
"""R1437 §5.15 §5.16 — a file drop lands in the window it was dropped on.

winit's file-DnD carries a path but no coordinate, so the drop TARGET is
the window. Pre-R1437 the shell knew that target (it redrew exactly that
window) but the three `WidgetView` hooks did not receive it — the id died
one call short of the binding. A multi-window binding could therefore only
guess, typically by routing to the focused window, which mis-aims whenever
the drop lands on an unfocused one (X11 / Wayland DND does not focus a
window before delivering the drop).

hello-filedrop is now two PEER windows — `main` (Inbox) and `archive` —
each with its own drop list. The `scene/hover_file`,
`scene/hover_file_cancel`, and `scene/drop_file` RPC peers take the shared
`{window: "<id>"}` param, so every claim below is drivable head-less and
read back as scene data (§2 #2 + §2 #7, no pixels).

What this proves:

1. **Boot isolation** — both windows exist, both idle, both empty.
2. **Drop routing** — a drop addressed to `archive` appears in the
   archive and NOT in the inbox; the reverse likewise.
3. **The unfocused-window case** — the sprag tear-off case: drops keep
   landing on the addressed window no matter which window was addressed
   last, so nothing "follows" a focus-shaped fallback.
4. **Hover isolation** — lighting one zone leaves the peer idle; a cancel
   clears only the window it names.
5. **Cross-window drag** — cancel on A + hover on B (a drag crossing
   between windows) leaves exactly one zone lit.
6. **Counts + order** — each window's count reflects only its own drops,
   append order preserved per window.
7. **Default addressing** — a frame with NO window param still addresses
   `main` (DEFAULT_WINDOW), so the R770 single-window arc is unchanged.

Run from the workspace root:
    cargo build -p hello-filedrop --release
    python3 tools/demos/r1437_filedrop_window.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

MAIN = "main"
ARCHIVE = "archive"
ZONE_TAG = "drop_zone"
VIEWPORT = (480, 320)


def _walk(node, out):
    out.append(node)
    for c in node.get("children", []) or []:
        _walk(c, out)
    if isinstance(node.get("content"), dict):
        _walk(node["content"], out)
    return out


def window_texts(ta: RpcSubprocess, window: str | None) -> list[str]:
    """Every `Text` content in one window's paint scene."""
    snap = ta.snapshot(source="paint", viewport=VIEWPORT, window=window)
    return [
        n["content"]
        for n in _walk(snap, [])
        if n.get("type") == "Text" and isinstance(n.get("content"), str)
    ]


def zone_texts(ta: RpcSubprocess, window: str | None) -> list[str]:
    """The `Text` contents inside one window's drop-zone container."""
    snap = ta.snapshot(source="paint", viewport=VIEWPORT, window=window)
    zone = find_by_tag(snap, ZONE_TAG)
    assert zone is not None, f"drop zone present in the {window} paint scene"
    return [
        n["content"]
        for n in _walk(zone, [])
        if n.get("type") == "Text" and isinstance(n.get("content"), str)
    ]


def paths_in(ta: RpcSubprocess, window: str | None) -> list[str]:
    return [t for t in zone_texts(ta, window) if t.startswith("/")]


def status_of(ta: RpcSubprocess, window: str | None) -> str:
    for t in window_texts(ta, window):
        if "dropped" in t:
            return t
    return ""


def hovering(ta: RpcSubprocess, window: str | None) -> bool:
    return "Release to drop" in zone_texts(ta, window)


def hover_file(ta: RpcSubprocess, path: str, window: str | None = None) -> None:
    params = {"path": path}
    if window is not None:
        params["window"] = window
    ta.request("scene/hover_file", params)


def hover_cancel(ta: RpcSubprocess, window: str | None = None) -> None:
    params = {}
    if window is not None:
        params["window"] = window
    ta.request("scene/hover_file_cancel", params)


def drop_file(ta: RpcSubprocess, path: str, window: str | None = None) -> None:
    params = {"path": path}
    if window is not None:
        params["window"] = window
    ta.request("scene/drop_file", params)


def body() -> None:
    with RpcSubprocess("hello-filedrop", request_timeout=12.0, boot_grace=1.5) as ta:
        # ── Phase 1 — boot: two peer windows, both idle and empty ─────
        declared = ta.request("scene/windows", {})
        assert declared is not None
        ids = {w["id"] for w in declared.result["windows"]}
        assert MAIN in ids, "the primary window is declared"
        assert ARCHIVE in ids, "the peer window is declared"

        inbox_titles = window_texts(ta, MAIN)
        archive_titles = window_texts(ta, ARCHIVE)
        assert any("Inbox" in t for t in inbox_titles), "the main window titles itself Inbox"
        assert any("Archive" in t for t in archive_titles), "the peer window titles itself Archive"
        assert not any("Archive" in t for t in inbox_titles), "windows paint different scenes"
        assert "Drag a file here" in zone_texts(ta, MAIN), "inbox idle hint"
        assert "Drag a file here" in zone_texts(ta, ARCHIVE), "archive idle hint"
        assert_eq(status_of(ta, MAIN), "0 file(s) dropped", "inbox empty at boot")
        assert_eq(status_of(ta, ARCHIVE), "0 file(s) dropped", "archive empty at boot")
        assert not hovering(ta, MAIN), "inbox not lit at boot"
        assert not hovering(ta, ARCHIVE), "archive not lit at boot"

        # ── Phase 2 — a drop lands in the window it names ─────────────
        drop_file(ta, "/tmp/old.log", window=ARCHIVE)
        wait_until(
            lambda: "/tmp/old.log" in zone_texts(ta, ARCHIVE),
            desc="the archive received the drop addressed to it",
        )
        assert "/tmp/old.log" not in zone_texts(ta, MAIN), "the inbox received nothing"
        assert_eq(status_of(ta, ARCHIVE), "1 file(s) dropped", "archive counts its own drop")
        assert_eq(status_of(ta, MAIN), "0 file(s) dropped", "inbox count untouched")
        assert_eq(len(paths_in(ta, MAIN)), 0, "inbox list still empty")

        # ── Phase 3 — the reverse direction ───────────────────────────
        drop_file(ta, "/tmp/new.txt", window=MAIN)
        wait_until(
            lambda: "/tmp/new.txt" in zone_texts(ta, MAIN),
            desc="the inbox received the drop addressed to it",
        )
        assert "/tmp/new.txt" not in zone_texts(ta, ARCHIVE), "the archive did not gain it"
        assert_eq(status_of(ta, MAIN), "1 file(s) dropped", "inbox counts its own drop")
        assert_eq(status_of(ta, ARCHIVE), "1 file(s) dropped", "archive count unchanged")

        # ── Phase 4 — the tear-off case: the target does not drift ────
        # Address the archive repeatedly without ever re-addressing the
        # inbox. A focus-shaped fallback would send these wherever focus
        # sits; window-addressed routing keeps them on the archive.
        for p in ("/tmp/a1.bin", "/tmp/a2.bin", "/tmp/a3.bin"):
            drop_file(ta, p, window=ARCHIVE)
        wait_until(
            lambda: len(paths_in(ta, ARCHIVE)) == 4,
            desc="all three follow-up drops stayed on the archive",
        )
        assert_eq(status_of(ta, ARCHIVE), "4 file(s) dropped", "archive counts 1 + 3")
        assert_eq(status_of(ta, MAIN), "1 file(s) dropped", "inbox never drifted")
        archive_paths = paths_in(ta, ARCHIVE)
        assert_eq(
            archive_paths,
            ["/tmp/old.log", "/tmp/a1.bin", "/tmp/a2.bin", "/tmp/a3.bin"],
            "the archive lists its own drops in arrival order",
        )
        assert_eq(paths_in(ta, MAIN), ["/tmp/new.txt"], "the inbox list is exactly its own drop")

        # ── Phase 5 — hover lights only the addressed window ──────────
        hover_file(ta, "/tmp/hover.png", window=ARCHIVE)
        wait_until(lambda: hovering(ta, ARCHIVE), desc="archive zone lit by its own hover")
        assert not hovering(ta, MAIN), "the inbox zone stayed idle"
        assert "Drag a file here" not in zone_texts(ta, ARCHIVE), "archive hint replaced"
        assert "/tmp/new.txt" in zone_texts(ta, MAIN), "the inbox keeps painting its list"

        # ── Phase 6 — cancel clears only the window it names ──────────
        hover_file(ta, "/tmp/hover.png", window=MAIN)
        wait_until(lambda: hovering(ta, MAIN), desc="inbox lit too")
        assert hovering(ta, ARCHIVE), "archive still lit — two independent affordances"
        hover_cancel(ta, window=MAIN)
        wait_until(lambda: not hovering(ta, MAIN), desc="cancel cleared the inbox")
        assert hovering(ta, ARCHIVE), "the archive survived the inbox's cancel"
        hover_cancel(ta, window=ARCHIVE)
        wait_until(lambda: not hovering(ta, ARCHIVE), desc="archive cancel clears the archive")
        assert not hovering(ta, MAIN), "the inbox stayed cleared"

        # ── Phase 7 — a drag crossing windows: exactly one zone lit ───
        hover_file(ta, "/tmp/cross.dat", window=MAIN)
        wait_until(lambda: hovering(ta, MAIN), desc="drag enters the inbox")
        hover_cancel(ta, window=MAIN)
        hover_file(ta, "/tmp/cross.dat", window=ARCHIVE)
        wait_until(lambda: hovering(ta, ARCHIVE), desc="drag crossed into the archive")
        assert not hovering(ta, MAIN), "the window the drag left is dark"
        drop_file(ta, "/tmp/cross.dat", window=ARCHIVE)
        wait_until(
            lambda: "/tmp/cross.dat" in zone_texts(ta, ARCHIVE),
            desc="the crossing drag drops where it ended",
        )
        assert not hovering(ta, ARCHIVE), "the drop cleared the archive affordance"
        assert "/tmp/cross.dat" not in zone_texts(ta, MAIN), "not in the window it started over"
        assert_eq(status_of(ta, ARCHIVE), "5 file(s) dropped", "archive counts the crossing drop")
        assert_eq(status_of(ta, MAIN), "1 file(s) dropped", "inbox unchanged by the crossing drag")

        # ── Phase 8 — no window param still addresses `main` ──────────
        # DEFAULT_WINDOW routing is what keeps every single-window binding
        # (and the R770 demo) working with the same wire frames as before.
        drop_file(ta, "/tmp/default-addressed.txt")
        wait_until(
            lambda: "/tmp/default-addressed.txt" in zone_texts(ta, MAIN),
            desc="a window-less frame lands on the primary window",
        )
        assert "/tmp/default-addressed.txt" not in zone_texts(ta, ARCHIVE), "not on the peer"
        assert_eq(status_of(ta, MAIN), "2 file(s) dropped", "inbox counted the default-addressed")
        assert_eq(status_of(ta, ARCHIVE), "5 file(s) dropped", "archive untouched by it")
        hover_file(ta, "/tmp/default-hover.txt")
        wait_until(lambda: hovering(ta, MAIN), desc="window-less hover lights the primary")
        assert not hovering(ta, ARCHIVE), "window-less hover left the peer idle"
        hover_cancel(ta)
        wait_until(lambda: not hovering(ta, MAIN), desc="window-less cancel clears the primary")

        # ── Phase 9 — unicode + spaces ride through the routed path ───
        unicode_path = "/tmp/내 파일 résumé.txt"
        drop_file(ta, unicode_path, window=ARCHIVE)
        wait_until(
            lambda: unicode_path in zone_texts(ta, ARCHIVE),
            desc="unicode/space path routed to the archive",
        )
        assert unicode_path not in zone_texts(ta, MAIN), "unicode path did not leak to the inbox"
        assert_eq(status_of(ta, ARCHIVE), "6 file(s) dropped", "archive counts the unicode drop")

        # ── Phase 10 — final ledger: every drop is in exactly one list ─
        final_main = paths_in(ta, MAIN)
        final_archive = paths_in(ta, ARCHIVE)
        assert_eq(len(final_main), 2, "the inbox holds exactly its two drops")
        assert_eq(len(final_archive), 6, "the archive holds exactly its six drops")
        assert not set(final_main) & set(final_archive), "no path is listed in both windows"
        assert_eq(
            final_main,
            ["/tmp/new.txt", "/tmp/default-addressed.txt"],
            "the inbox ledger in arrival order",
        )
        assert_eq(
            final_archive[-2:],
            ["/tmp/cross.dat", unicode_path],
            "the archive ledger tail in arrival order",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1437 window-addressed file drop", body))
