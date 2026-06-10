#!/usr/bin/env python3
"""R715 §5.16 — shared click-outside dismiss barrier (SSOT lift).

R714 introduced a transparent click-catcher inline in `hello-combobox`
(a modal scrim painted at zero opacity). R715 lifts that shape into one
home, `pinion_widget_paint::barrier::dismiss_barrier`, and adds
`hello-menu` as the 2nd consumer per [[abstraction-needs-second-consumer]]:
clicking outside an open menu now dismisses it. Only the *paint
construction* is shared — dispatch stays per-binding (ComboBox routes the
outside click to a `combo_barrier` ButtonExternal; the menubar routes
`menu#barrier` to the one MenuBarExternal via the R51.42 composite-tag
router, whose new `barrier:PointerUp` arm closes the dropdown).

Verification axes (>=30 assertions):

  hello-menu (1st new consumer)
  (A) closed     — no barrier painted; open = none.
  (B) open       — clicking a title opens it AND paints the barrier: a
                   transparent (alpha 0), childless container anchored
                   below the title strip (origin (0, bar_height)).
  (C) click-out  — a click on the barrier region (by tag and by bare
                   coordinate) dismisses the menu; no `command` intent.
  (D) menu-switch — the title strip stays clickable ABOVE the barrier,
                   so clicking a sibling title switches menus instead of
                   dismissing (the region-not-full-window invariant).
  (E) substrate  — `send("barrier:PointerEnter")` is inert; only
                   `barrier:PointerUp` closes (the dispatch arm directly).
  (F) Escape     — the pre-existing Escape dismiss still closes.

  hello-combobox (R714 consumer, retrofitted to the shared barrier)
  (G) the combobox still opens and click-outside still dismisses through
      the lifted `dismiss_barrier` — proving the 2nd-consumer lift kept
      the 1st consumer's behaviour byte-for-byte.

Pixels. The barrier is invisible *by design* (alpha 0) and only exists
in the RPC-opened state, so — exactly the R710/R711 boot-closed-overlay
situation — it has no dedicated pixel witness; its transparency is
verified structurally above, and the visible menu chrome keeps its R691
/ R711 pixel guards. Phase 2 adds a boot-render guard (the menubar title
text rasterises as pixels distinct from the page background) so a blank
/ broken render still fails loudly with producer parity (same
`to_vello_cached` rasterizer as the live window).
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
    abs_rects_of,
    assert_eq,
    find_by_tag,
    png_pixel,
    read_png_rgba8,
    run_demo,
    wait_query,
    wait_snap,
    wait_until,
)

WORKSPACE_ROOT = Path(__file__).resolve().parent.parent.parent
VIEWPORT = (520, 320)
BAR_HEIGHT = 40
MENU_BARRIER = "menu#barrier"
DROPDOWN = "menu_dropdown"
# A point well below the title strip and clear of menu 0's left-aligned
# dropdown — a bare click here lands on the transparent barrier.
OUTSIDE_POINT = (480.0, 280.0)

COMBO_VIEWPORT = (560, 420)
COMBO_TRIGGER = "combo_trigger"
COMBO_PANEL = "combo_panel"
COMBO_BARRIER = "combo_barrier"


def _present(snap, tag) -> bool:
    return find_by_tag(snap, tag) is not None


def _open(tf):
    return tf.query("/external/open")


def _menu_body() -> None:
    with RpcSubprocess("hello-menu", boot_grace=1.5) as tf:
        # ── (A) closed: no barrier ──────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert_eq(_open(tf), None, "boot: no menu open")
        assert not _present(snap, MENU_BARRIER), "closed: no dismiss barrier painted"
        assert not _present(snap, DROPDOWN), "closed: no dropdown painted"

        # ── (B) open paints the transparent barrier below the bar ───
        tf.click(path="menu#t0")
        wait_query(tf, "/external/open", 0, desc="click File -> open 0")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert _present(snap, DROPDOWN), "open: dropdown painted"
        barrier = find_by_tag(snap, MENU_BARRIER)
        assert barrier is not None, "open: dismiss barrier painted"
        fill = (barrier.get("style") or {}).get("fill") or {}
        assert_eq(int(fill.get("a", -1)), 0, "barrier is fully transparent (non-modal, no dim)")
        assert not (barrier.get("children") or []), "barrier is a passive click-catcher (no children)"
        # Post-layout rect: the barrier anchors below the title strip so a
        # sibling-title click above it switches menus instead of dismissing.
        rect = barrier.get("rect") or {}
        assert_eq(int(rect.get("x", -1)), 0, "barrier x origin = 0")
        assert_eq(int(rect.get("y", -1)), BAR_HEIGHT, "barrier starts below the title strip")
        assert int(rect.get("h", 0)) > 0, "barrier covers a non-empty region below the bar"

        # ── (C) click-outside dismiss (by tag, then by coordinate) ──
        tf.click(path=MENU_BARRIER)  # routed: menu#barrier -> send("barrier:PointerUp")
        wait_query(tf, "/external/open", None, desc="click on barrier tag dismisses the menu")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert not _present(snap, MENU_BARRIER), "barrier gone after click-outside"
        assert not _present(snap, DROPDOWN), "dropdown gone after click-outside"

        tf.click(path="menu#t1")
        wait_query(tf, "/external/open", 1, desc="re-open Edit for the coordinate click-outside")
        tf.click(at=OUTSIDE_POINT)  # bare coordinate over the barrier region
        wait_query(tf, "/external/open", None, desc="bare click outside the dropdown dismisses")

        # Barrier dismiss is NOT an activation: it fires no `command`.
        needle = "shell: intent menu.command"
        tail = tf.stderr_tail(40)
        recent = [ln for ln in tail if needle in ln]
        assert not recent, f"click-outside must not emit a command; saw {recent[-3:]!r}"

        # ── (D) the title strip stays clickable ABOVE the barrier ───
        tf.click(path="menu#t0")
        wait_query(tf, "/external/open", 0, desc="open File")
        tf.click(path="menu#t1")  # a sibling title sits above the barrier
        wait_query(tf, "/external/open", 1, desc="clicking a sibling title SWITCHES menus, not dismiss")
        tf.click(path="menu#t2")
        wait_query(tf, "/external/open", 2, desc="switch again to View")
        tf.click(path="menu#t2")  # re-click open title toggles closed
        wait_query(tf, "/external/open", None, desc="re-click the open title still toggle-closes")

        # ── (E) substrate arm: only PointerUp on the barrier closes ─
        tf.click(path="menu#t0")
        wait_query(tf, "/external/open", 0, desc="open File for the substrate-arm probe")
        # The two inert sends are no-change verifications (open stays 0)
        # — no gate is possible; the dispatch commits before the response.
        tf.invoke("/external/send", "barrier:PointerEnter")
        assert_eq(_open(tf), 0, "PointerEnter over the barrier is inert")
        tf.invoke("/external/send", "barrier:PointerDown")
        assert_eq(_open(tf), 0, "PointerDown over the barrier is inert")
        out = tf.invoke("/external/send", "barrier:PointerUp")
        wait_query(tf, "/external/open", None, desc="barrier:PointerUp closes the menu")
        assert_eq(out, None, "send returns the now-closed open index (null)")

        # ── (F) Escape dismiss still works ──────────────────────────
        tf.request("focus/set", {"tag": "menu"})
        wait_until(
            lambda: tf.request("focus/get").result.get("focused") == "menu",
            desc="menubar owns shell focus",
        )
        tf.key(path="menu", name="ArrowDown")  # open focused menu
        wait_query(tf, "/external/open", 0, desc="ArrowDown reopens File")
        tf.key(path="menu", name="Escape")
        wait_query(tf, "/external/open", None, desc="Escape still closes (unchanged by R715)")


def _combobox_body() -> None:
    """R714 consumer retrofit — the combobox's click-outside now flows
    through the shared `dismiss_barrier`; prove it is unchanged."""
    with RpcSubprocess("hello-combobox", boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=COMBO_VIEWPORT)
        assert not _present(snap, COMBO_BARRIER), "combobox closed: no barrier"

        tf.click(path=COMBO_TRIGGER)
        snap = wait_snap(
            tf, lambda s: _present(s, COMBO_PANEL), viewport=COMBO_VIEWPORT,
            desc="combobox opens on trigger click",
        )
        barrier = find_by_tag(snap, COMBO_BARRIER)
        assert barrier is not None, "combobox open: shared dismiss barrier painted"
        fill = (barrier.get("style") or {}).get("fill") or {}
        assert_eq(int(fill.get("a", -1)), 0, "combobox barrier is transparent (shared shape)")

        tf.click(path=COMBO_BARRIER)
        snap = wait_snap(
            tf, lambda s: not _present(s, COMBO_PANEL), viewport=COMBO_VIEWPORT,
            desc="combobox click-outside dismisses (retrofit intact)",
        )
        assert not _present(snap, COMBO_BARRIER), "combobox barrier gone after dismiss"


def _pixel_guard() -> None:
    """Boot-render guard: the menubar title text rasterises as pixels
    distinct from the page background (producer parity). The barrier
    itself is invisible by design and has no pixel witness — see the
    module docstring."""
    with RpcSubprocess("hello-menu", boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        rects = abs_rects_of(snap)
    img = read_png_rgba8(_capture_screenshot())
    assert_eq((img.width, img.height), VIEWPORT, "screenshot matches the viewport")

    bg = png_pixel(img, 4, 4)  # page background (Surface), above the bar gap
    tr = rects["menu#t0"]  # the "File" title slot
    cy = tr[1] + tr[3] // 2
    # Scan across the slot; the centred "File" glyphs (OnSurface) must
    # produce at least a few pixels distinct from the Surface background.
    distinct = 0
    for k in range(1, 24):
        x = tr[0] + tr[2] * k // 24
        if 0 <= x < img.width and png_pixel(img, x, cy) != bg:
            distinct += 1
    assert distinct >= 1, (
        f"menubar title 'File' must rasterise distinct glyph pixels vs the "
        f"page background {bg}; scanned slot {tr} found none"
    )


def _capture_screenshot() -> Path:
    """Render hello-menu's boot frame to RGBA8 PNG via PINION_SCREENSHOT
    (bypassing winit; same `to_vello_cached` rasterizer the live window
    uses)."""
    out = Path(tempfile.mkdtemp(prefix="pinion-r715-")) / "menu.png"
    binary = WORKSPACE_ROOT / "target" / "release" / "hello-menu"
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", "hello-menu", "--quiet", "--release",
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


def body() -> None:
    _menu_body()
    _combobox_body()
    _pixel_guard()


if __name__ == "__main__":
    sys.exit(run_demo("R715 §5.16 — shared click-outside dismiss barrier", body))
