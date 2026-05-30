#!/usr/bin/env python3
"""R702 §5.16 §5.39 §5.40 §5.50 — modal navigation Drawer end-to-end.

Drives the new `hello-drawer` binding via JSON-RPC + verifies it as the
2nd consumer of the R693 modal focus-trap substrate
(`pinion_runtime::FocusManager` push/pop modal scope via
`pinion_core::modal_scope_request`) and the new
`pinion_widget_paint::drawer` chrome — plus the scrim-click light-dismiss
this round adds (the R693 carry).

The drawer is a left-anchored modal navigation sheet built from real
`ButtonExternal`s: the trigger `drawer_trigger` (primary) plus the
click-only scrim `drawer_scrim` and N=3 nav items `drawer_item_<i>`
(extra externals). Opening installs a focus trap over the nav items; the
scrim dims + blocks the background AND light-dismisses on tap; Escape /
scrim / selecting a destination close and restore focus to the trigger.

R702 atomic verification scope (>=30 assertions):

  (A) closed shape — trigger present, no scrim / panel, "Current: Home".
  (B) open via trigger click — scrim + panel + title + N nav items appear.
  (C) auto-focus — focus moves onto the first nav item.
  (D) Tab trap — focus/next + focus/prev cycle ONLY the nav members + wrap.
  (E) trap confinement — focus/set to the trigger behind the scrim rejected.
  (F) scrim LIGHT-DISMISS — a backdrop tap CLOSES the drawer without
      navigating (the new behaviour vs the inert dialog scrim), and
      restores focus to the trigger.
  (G) selecting a destination sets it active AND closes the drawer.
  (H) Escape dismisses without navigating (active unchanged).
  (I) keyboard activate — re-open, Enter on the focused item navigates.
  (J) negative — focus/set to an unknown tag is rejected.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
)

VIEWPORT = (560, 380)
PAUSE = 0.12

TRIGGER = "drawer_trigger"
SCRIM = "drawer_scrim"
PANEL = "drawer_panel"
STATUS = "drawer_status"
NAV = [f"drawer_item_{i}" for i in range(3)]
DEST = ["Home", "Search", "Settings"]

# A point inside the visible scrim region (right of the 280px-wide panel).
SCRIM_POINT = (500.0, 190.0)


def _present(snap, tag) -> bool:
    return find_by_tag(snap, tag) is not None


def _all_text(node) -> list[str]:
    out: list[str] = []

    def walk(n):
        if isinstance(n, dict):
            if n.get("type") == "Text":
                content = n.get("content")
                if isinstance(content, str):
                    out.append(content)
            for c in n.get("children") or []:
                walk(c)

    walk(node)
    return out


def _active(tf) -> str:
    """The active destination from the `drawer_status` text node."""
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    node = find_by_tag(snap, STATUS)
    assert node is not None, "status text node present"
    content = node.get("content") or ""
    assert content.startswith("Current: "), f"status shape; got {content!r}"
    return content[len("Current: ") :]


def _focused(tf) -> str | None:
    return tf.request("focus/get").result.get("focused")


def _tab_order(tf) -> list:
    return tf.request("focus/get").result.get("tab_order") or []


def _focus_next(tf) -> str | None:
    return tf.request("focus/next").result.get("focused")


def _focus_prev(tf) -> str | None:
    return tf.request("focus/prev").result.get("focused")


def body() -> None:
    with RpcSubprocess("hello-drawer", boot_grace=1.5) as tf:
        # ── (A) closed shape ────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert _present(snap, TRIGGER), "trigger present when closed"
        assert not _present(snap, SCRIM), "no scrim when closed"
        assert not _present(snap, PANEL), "no panel when closed"
        assert_eq(_active(tf), "Home", "initial active destination")

        # ── (B) open via trigger click ──────────────────────────────
        tf.click(path=TRIGGER)
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert _present(snap, SCRIM), "scrim appears on open"
        assert _present(snap, PANEL), "panel appears on open"
        for i, tag in enumerate(NAV):
            assert _present(snap, tag), f"nav item {i} present on open"
        ptext = _all_text(snap)
        assert "Navigation" in ptext, f"drawer title renders; got {ptext!r}"
        for dest in DEST:
            assert dest in ptext, f"destination label {dest!r} renders"
        scrim = find_by_tag(snap, SCRIM)
        scrim_alpha = int(((scrim.get("style") or {}).get("fill") or {}).get("a") or 0)
        assert scrim_alpha > 0, f"scrim paints a dim backdrop; alpha={scrim_alpha}"

        # R711 MD3 elevation — the modal drawer panel floats at Level 1
        # (key+ambient drop-shadow from the shared `elevation(1)`). Read it
        # back as data (§2 #7); pixel fidelity is R710's guard.
        panel = find_by_tag(snap, PANEL)
        ps = (panel.get("style") or {}).get("shadows") or []
        assert len(ps) == 2, f"drawer panel casts key + ambient (MD3 L1); got {ps}"
        # MD3 modal drawer = Level 1 -> key blur = 1 * 1.5 = 1.5, offset.y = 1.
        assert abs(ps[0]["blur"] - 1.5) < 1e-3, f"L1 key blur 1.5; got {ps[0]}"
        assert abs(ps[0]["offset"]["y"] - 1.0) < 1e-4, "L1 key offset.y 1"

        # ── (C) auto-focus into the drawer ──────────────────────────
        assert_eq(_focused(tf), NAV[0], "open auto-focuses the first nav item")
        assert_eq(_tab_order(tf), NAV, "focus/get reports the trap enumeration")

        # ── (D) Tab trap: cycle + wrap within members ───────────────
        assert_eq(_focus_next(tf), NAV[1], "focus/next: item0 -> item1")
        assert_eq(_focus_next(tf), NAV[2], "focus/next: item1 -> item2")
        assert_eq(_focus_next(tf), NAV[0], "focus/next wraps item2 -> item0")
        assert_eq(_focus_prev(tf), NAV[2], "focus/prev wraps item0 -> item2")

        # ── (E) trap confinement ────────────────────────────────────
        raised = False
        try:
            tf.request("focus/set", {"tag": TRIGGER})
        except RpcError:
            raised = True
        assert raised, "focus/set to the trigger behind the scrim is rejected"
        assert _focused(tf) in NAV, "focus stays trapped on a nav item"
        moved = tf.request("focus/set", {"tag": NAV[1]}).result.get("focused")
        assert_eq(moved, NAV[1], "focus/set to a trap member succeeds")

        # ── (F) scrim LIGHT-DISMISS (the new behaviour) ─────────────
        tf.click(at=SCRIM_POINT)
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert not _present(snap, PANEL), "scrim tap dismisses the drawer (light-dismiss)"
        assert not _present(snap, SCRIM), "scrim gone after light-dismiss"
        assert_eq(_active(tf), "Home", "scrim tap does NOT navigate (active unchanged)")
        assert_eq(_focused(tf), TRIGGER, "focus restored to the trigger after dismiss")
        assert_eq(_tab_order(tf), [TRIGGER], "base tab order restored after close")

        # ── (G) selecting a destination navigates + closes ──────────
        tf.click(path=TRIGGER)
        time.sleep(PAUSE)
        assert_eq(_focused(tf), NAV[0], "re-open auto-focuses item0 again")
        tf.click(path=NAV[2])
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert not _present(snap, PANEL), "selecting a destination closes the drawer"
        assert_eq(_active(tf), "Settings", "selected destination becomes active")
        assert_eq(_focused(tf), TRIGGER, "focus restored after selection")

        # ── (H) Escape dismisses without navigating ─────────────────
        tf.click(path=TRIGGER)
        time.sleep(PAUSE)
        assert _present(tf.snapshot(source="paint", viewport=VIEWPORT), PANEL), "re-opened"
        tf.key(path=PANEL, name="Escape")
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert not _present(snap, PANEL), "Escape dismisses the drawer"
        assert_eq(_active(tf), "Settings", "Escape does NOT navigate (still Settings)")
        assert_eq(_focused(tf), TRIGGER, "focus restored after Escape")

        # ── (I) keyboard activate: Enter on the focused item ────────
        tf.click(path=TRIGGER)
        time.sleep(PAUSE)
        assert_eq(_focused(tf), NAV[0], "re-open focuses item0")
        tf.key(path=PANEL, name="Enter")
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert not _present(snap, PANEL), "Enter on the focused item navigates + closes"
        assert_eq(_active(tf), "Home", "keyboard activate selected item0 = Home")
        assert_eq(_focused(tf), TRIGGER, "focus restored after keyboard activate")

        # ── (J) negative: focus/set to an unknown tag is rejected ───
        raised = False
        try:
            tf.request("focus/set", {"tag": "no_such_widget"})
        except RpcError:
            raised = True
        assert raised, "focus/set to an unknown tag must be rejected"


if __name__ == "__main__":
    sys.exit(run_demo("R702 §5.16 §5.39 §5.40 — modal navigation Drawer", body))
