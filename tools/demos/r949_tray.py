#!/usr/bin/env python3
"""R949 §3 §5.53 — system-tray substrate, over RPC.

A self-hosted editor needs a system tray for long-running background work.
pinion *owns* the tray model (id / icon / tooltip / status / menu); a
`TrayBackend` exposes it to the OS. R949 lands the substrate — the
`TrayBackend` trait + the headless `InMemoryTrayBackend` (the R665 Storage /
R833 PrintBackend house pattern) — so the whole tray is demonstrable +
RPC-introspectable without a panel. The real StatusNotifierItem D-Bus bridge
(`pinion_platform_tray::SniTrayBackend`, no gtk) is the follow-up platform
round, exactly as FileStorage / CupsPrintBackend followed their in-memory
firsts.

The menu drives app state and re-publishes on every change — the menu state
is app-owned, never backend-guessed. An AI agent drives the whole round-trip
over the §5.12 RPC plane (§2 #2) as pure scene-as-data (§2 #7).

  (A) boot taxonomy — available host, Active status, 5-item menu, nothing
      published yet.
  (B) republish records the live model; published_in_sync flips true.
  (C) the Dark-mode check flips app state + its checkmark + re-publishes.
  (D) toggle-window relabels (Hide <-> Show); the icon click (activate) is
      the same toggle.
  (E) Bake assets sets NeedsAttention status + the syncing icon + relabels.
  (F) a disabled (`ship`) / unknown id is rejected — no dispatch, no publish.
  (G) Quit sets quit_requested.

Run from the workspace root:
    cargo build -p hello-tray --release
    python3 tools/demos/r949_tray.py
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
    wait_query,
)

VIEWPORT = (460, 360)
PANEL = "tray_panel"
E = "/external"


def q(tf, path):
    return tf.query(f"{E}/{path}")


def menu_item(tf, item_id: str) -> bool:
    return tf.invoke(f"{E}/menu_item", item_id)


def body() -> None:
    with RpcSubprocess("hello-tray", boot_grace=1.5) as tf:
        # ── (A) boot taxonomy ────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, PANEL) is not None, "tray panel present"
        assert_eq(q(tf, "available"), True, "an in-memory tray host is available")
        assert_eq(q(tf, "title"), "Pinion", "model title")
        assert_eq(q(tf, "status"), "Active", "boot status is Active")
        assert_eq(q(tf, "icon"), "applications-games", "boot themed icon name")
        assert_eq(q(tf, "menu_count"), 5, "five top-level menu entries")
        assert_eq(q(tf, "publish_count"), 0, "nothing published before an explicit publish")
        assert_eq(q(tf, "published_in_sync"), False, "no published model yet")
        assert_eq(
            q(tf, "menu"),
            "Hide window | Dark mode | — | Build ▸ | Quit",
            "menu summary",
        )

        # ── (B) republish records the live model ─────────────────────
        assert_eq(tf.invoke(f"{E}/republish", None), 1, "first publish")
        wait_query(tf, f"{E}/published_in_sync", True, desc="published matches the live model")
        assert_eq(tf.invoke(f"{E}/republish", None), 2, "republish increments the count")

        # ── (C) Dark-mode check flips state + checkmark + republishes ─
        assert_eq(q(tf, "dark_mode"), False, "dark off at boot")
        assert_eq(q(tf, "item.dark.checked"), False, "the check is clear")
        assert_eq(menu_item(tf, "dark"), True, "the dark menu item activates")
        wait_query(tf, f"{E}/dark_mode", True, desc="the menu flipped app state")
        assert_eq(q(tf, "item.dark.checked"), True, "the checkmark reflects it")
        assert_eq(q(tf, "publish_count"), 3, "the activation re-published")
        assert_eq(q(tf, "published_in_sync"), True, "still in sync after the toggle")
        assert_eq(
            q(tf, "menu"),
            "Hide window | Dark mode ✓ | — | Build ▸ | Quit",
            "the summary shows the dark check",
        )

        # ── (D) toggle-window relabels; activate is the icon click ───
        assert_eq(q(tf, "window_visible"), True, "window shown at boot")
        assert_eq(q(tf, "item.toggle_window.label"), "Hide window", "label while shown")
        assert_eq(menu_item(tf, "toggle_window"), True, "toggle-window activates")
        wait_query(tf, f"{E}/window_visible", False, desc="window now hidden")
        assert_eq(q(tf, "item.toggle_window.label"), "Show window", "label tracks the state")
        # The icon (primary) click toggles the window too.
        assert_eq(tf.invoke(f"{E}/activate", None), True, "icon click")
        wait_query(tf, f"{E}/window_visible", True, desc="activate re-shows the window")

        # ── (E) Bake assets -> NeedsAttention + syncing icon ─────────
        assert_eq(menu_item(tf, "bake"), True, "bake activates")
        wait_query(tf, f"{E}/build_running", True, desc="a build is running")
        assert_eq(q(tf, "status"), "NeedsAttention", "status escalates while baking")
        assert_eq(q(tf, "icon"), "emblem-synchronizing", "the icon shows the syncing state")
        assert_eq(q(tf, "item.bake.label"), "Cancel bake", "the bake item relabels")

        # ── (F) disabled / unknown ids are rejected ──────────────────
        published_before = q(tf, "publish_count")
        assert_eq(q(tf, "item.ship.activatable"), False, "ship is present but disabled")
        assert_eq(menu_item(tf, "ship"), False, "a disabled id is rejected")
        assert_eq(menu_item(tf, "nope"), False, "an unknown id is rejected")
        assert_eq(q(tf, "publish_count"), published_before, "a rejected activation publishes nothing")

        # ── (G) Quit sets quit_requested ─────────────────────────────
        assert_eq(q(tf, "quit_requested"), False, "not quit yet")
        assert_eq(menu_item(tf, "quit"), True, "quit activates")
        wait_query(tf, f"{E}/quit_requested", True, desc="quit was requested")


if __name__ == "__main__":
    sys.exit(run_demo("R949 system-tray substrate", body))
