#!/usr/bin/env python3
"""R714 §5.38 §5.40 §5.50 — select-only ComboBox end-to-end.

Drives the new `hello-combobox` binding via JSON-RPC and verifies it as a
pure composition (no new coordinator) of three proven externals:

  * trigger  — a `ButtonExternal` (`combo_trigger`, primary) painted as
    the field showing the current value + chevron;
  * popup    — a single-select `ListBoxExternal` (`combo_options`, extra)
    of N=5 T-shirt sizes;
  * barrier  — a click-only `ButtonExternal` (`combo_barrier`, extra)
    bound to a TRANSPARENT `scrim_backdrop` (the R702/R703 scrim primitive
    at alpha 0, no focus trap) = the click-outside dismiss substrate.

R714 also adds the `AriaRole::ComboBox` + `aria-controls` a11y axis.

Atomic verification scope (>=30 assertions):

  (A) closed shape — trigger + placeholder value + "(none)" status, no
      panel / options painted, chevron points DOWN.
  (B) open via trigger click — barrier + panel + N options appear, chevron
      glyph flips UP; the dropdown panel floats at MD3 menu elevation (L2)
      and — because the trigger sits low — the panel FLIPS ABOVE it (R1378
      `flip_y`), its bottom flush against the trigger top, inside the viewport.
  (C) keyboard roving — focus the trigger, ArrowDown/End move the popup's
      `focused_index` active descendant (queried off the listbox external).
  (D) commit via click — clicking an option closes the popup, updates the
      trigger value + status, and the listbox `selected_index` matches.
  (E) click-OUTSIDE dismiss — re-open, click the transparent barrier away
      from the panel: the popup closes WITHOUT changing the selection.
  (F) Escape dismiss — re-open, Escape closes without changing selection.
  (G) keyboard open + commit — ArrowDown opens a closed combobox; Enter
      commits the active option.
  (H) negative — querying an unknown listbox path returns null.

  Phase 2 — PIXELS (PINION_SCREENSHOT, producer-parity headless render).
    The boot frame is the CLOSED combobox; the trigger field renders as a
    distinct tonal surface against the page background (a real, visible
    widget). The open dropdown cannot be boot-captured (RPC-opened
    overlay, the R711 limitation [[introspection-from-paint-not-screen]]);
    its shadow/scrim pixel fidelity rides the shared R710 elevation +
    R703 scrim rasterizer already guarded — so structure (RPC read-back)
    covers the open state, pixels cover the visible closed field.
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
    abs_rects_of,
    assert_eq,
    find_by_tag,
    read_png_rgba8,
    run_demo,
    sample_png_points,
    wait_snap,
    wait_until,
)

WORKSPACE_ROOT = Path(__file__).resolve().parent.parent.parent
VIEWPORT = (560, 420)

TRIGGER = "combo_trigger"
OPTIONS = "combo_options"
BARRIER = "combo_barrier"
PANEL = "combo_panel"
STATUS = "combo_status"
LABELS = ["Extra small", "Small", "Medium", "Large", "Extra large"]
OPT = [f"{OPTIONS}#{i}" for i in range(5)]

# A point well outside the dropdown panel (top-left), used for the
# click-outside (barrier) dismiss.
OUTSIDE_POINT = (40.0, 40.0)


def _present(snap, tag) -> bool:
    return find_by_tag(snap, tag) is not None


def _text_of(snap, tag) -> str:
    node = find_by_tag(snap, tag)
    assert node is not None, f"{tag} text node present"
    return node.get("content") or ""


def _value(tf) -> str:
    """The trigger's displayed value = the first Text under the trigger
    subtree (the value precedes the chevron). Untagged like a button
    label so the open click falls through to the container external."""
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    node = find_by_tag(snap, TRIGGER)
    out: list[str] = []

    def walk(n):
        if isinstance(n, dict):
            if n.get("type") == "Text" and isinstance(n.get("content"), str):
                out.append(n["content"])
            for c in n.get("children") or []:
                walk(c)

    walk(node)
    # First text = value; second = chevron glyph.
    return out[0] if out else ""


def _status(tf) -> str:
    return _text_of(tf.snapshot(source="paint", viewport=VIEWPORT), STATUS)


def _focused_index(tf):
    return tf.query(f"/{OPTIONS}/external/focused_index")


def _selected_index(tf):
    return tf.query(f"/{OPTIONS}/external/selected_index")


def body() -> None:
    with RpcSubprocess("hello-combobox", boot_grace=1.5) as tf:
        # ── (A) closed shape ────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert _present(snap, TRIGGER), "trigger present when closed"
        assert not _present(snap, PANEL), "no dropdown panel when closed"
        assert not _present(snap, BARRIER), "no barrier when closed"
        for t in OPT:
            assert not _present(snap, t), f"option {t} not painted when closed"
        assert_eq(_value(tf), "Select a size", "placeholder value when nothing committed")
        assert_eq(_status(tf), "Committed: (none)", "status shows no commit")
        # Chevron down (collapsed) renders somewhere in the trigger.
        assert "▾" in _trigger_text(snap), "collapsed chevron points down"
        assert_eq(_selected_index(tf), None, "listbox selection empty at boot")

        # ── (B) open via trigger click ──────────────────────────────
        tf.click(path=TRIGGER)
        snap = wait_snap(
            tf, lambda s: _present(s, BARRIER), viewport=VIEWPORT,
            desc="transparent barrier appears on open",
        )
        assert _present(snap, PANEL), "dropdown panel appears on open"
        for i, t in enumerate(OPT):
            assert _present(snap, t), f"option {i} painted on open"
        ptext = _trigger_text(snap)
        assert "▴" in ptext, "expanded chevron flips UP"
        # The barrier is a real but fully transparent scrim (alpha 0).
        barrier = find_by_tag(snap, BARRIER)
        fill = (barrier.get("style") or {}).get("fill")
        assert fill is not None, "barrier carries a fill"
        assert_eq(int(fill.get("a", -1)), 0, "barrier scrim is fully transparent (non-modal, no dim)")
        # MD3 menu elevation (L2 = key+ambient) on the dropdown panel.
        panel = find_by_tag(snap, PANEL)
        shadows = (panel.get("style") or {}).get("shadows") or []
        assert_eq(len(shadows), 2, "dropdown panel casts key + ambient (MD3 L2)")
        assert abs(shadows[0]["blur"] - 3.0) < 1e-3, f"L2 key blur 3.0; got {shadows[0]}"
        # R1378 — the trigger sits low in the window, so the dropdown FLIPS
        # ABOVE it (opening below would overflow the window bottom). The
        # shared `flip_y` positioner lands the panel bottom flush at the
        # trigger top and keeps the whole panel inside the viewport.
        rects = abs_rects_of(snap)
        tr, pn = rects[TRIGGER], rects[PANEL]
        assert pn[1] < tr[1], (
            f"dropdown flips ABOVE the low trigger (panel top {pn[1]} < trigger top {tr[1]})"
        )
        assert_eq(pn[1] + pn[3], tr[1], "panel bottom flush against the trigger top (flip_y)")
        assert pn[1] + pn[3] <= VIEWPORT[1], "flipped panel stays within the viewport bottom"

        # ── (C) keyboard roving active descendant ───────────────────
        tf.request("focus/set", {"tag": TRIGGER})
        assert_eq(tf.request("focus/get").result.get("focused"), TRIGGER, "trigger focused")
        tf.key(path=TRIGGER, name="ArrowDown")  # fresh open: lands on 0
        wait_until(
            lambda: _focused_index(tf) == 0,
            desc="first ArrowDown lands active descendant on 0",
        )
        tf.key(path=TRIGGER, name="ArrowDown")
        wait_until(
            lambda: _focused_index(tf) == 1,
            desc="ArrowDown steps active descendant to 1",
        )
        tf.key(path=TRIGGER, name="End")
        wait_until(
            lambda: _focused_index(tf) == 4,
            desc="End jumps active descendant to last",
        )
        tf.key(path=TRIGGER, name="Home")
        wait_until(
            lambda: _focused_index(tf) == 0,
            desc="Home jumps active descendant to first",
        )

        # ── (D) commit an option via click ──────────────────────────
        tf.click(path=OPT[2])  # "Medium"
        snap = wait_snap(
            tf, lambda s: not _present(s, PANEL), viewport=VIEWPORT,
            desc="committing an option closes the popup",
        )
        assert not _present(snap, BARRIER), "barrier gone after commit"
        assert_eq(_selected_index(tf), 2, "listbox records the committed index")
        assert_eq(_value(tf), "Medium", "trigger value reflects the committed option")
        assert_eq(_status(tf), "Committed: Medium", "status echoes the committed value")
        # Chevron back to down (collapsed).
        assert "▾" in _trigger_text(snap), "chevron points down again after close"

        # ── (E) click-OUTSIDE (barrier) dismiss ─────────────────────
        tf.click(path=TRIGGER)
        wait_snap(
            tf, lambda s: _present(s, PANEL), viewport=VIEWPORT,
            desc="re-opened",
        )
        tf.click(at=OUTSIDE_POINT)  # lands on the transparent barrier
        snap = wait_snap(
            tf, lambda s: not _present(s, PANEL), viewport=VIEWPORT,
            desc="click outside the panel dismisses the popup",
        )
        assert not _present(snap, BARRIER), "barrier gone after click-outside dismiss"
        assert_eq(_selected_index(tf), 2, "click-outside does NOT change the selection")
        assert_eq(_value(tf), "Medium", "value unchanged by click-outside dismiss")

        # ── (F) Escape dismiss ──────────────────────────────────────
        tf.click(path=TRIGGER)
        wait_snap(
            tf, lambda s: _present(s, PANEL), viewport=VIEWPORT,
            desc="re-opened for Escape",
        )
        tf.request("focus/set", {"tag": TRIGGER})
        tf.key(path=TRIGGER, name="Escape")
        snap = wait_snap(
            tf, lambda s: not _present(s, PANEL), viewport=VIEWPORT,
            desc="Escape dismisses the popup",
        )
        assert_eq(_selected_index(tf), 2, "Escape does NOT change the selection")

        # ── (G) keyboard open + commit ──────────────────────────────
        tf.request("focus/set", {"tag": TRIGGER})
        tf.key(path=TRIGGER, name="ArrowDown")  # opens (closed -> open)
        wait_snap(
            tf, lambda s: _present(s, PANEL), viewport=VIEWPORT,
            desc="ArrowDown opens closed combobox",
        )
        tf.key(path=TRIGGER, name="ArrowDown")  # active descendant -> step
        tf.key(path=TRIGGER, name="ArrowDown")
        idx_before = _focused_index(tf)
        tf.key(path=TRIGGER, name="Enter")  # commit the active option
        snap = wait_snap(
            tf, lambda s: not _present(s, PANEL), viewport=VIEWPORT,
            desc="Enter commits + closes",
        )
        assert_eq(_selected_index(tf), idx_before, "Enter committed the active descendant")
        assert_eq(_value(tf), LABELS[idx_before], "value reflects keyboard commit")

        # ── (H) negative — unknown listbox path is rejected ─────────
        raised = False
        try:
            tf.query(f"/{OPTIONS}/external/no_such_field")
        except RpcError:
            raised = True
        assert raised, "unknown introspect path must be rejected"

    # ── Phase 2 — live pixels (closed trigger field) ────────────────
    snap, rects = _boot_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    tr = rects[TRIGGER]
    # A point inside the left of the trigger field (before the value text)
    # vs the page background top-left corner.
    field_pt = (tr[0] + 6, tr[1] + tr[3] // 2)
    page_pt = (8, 8)
    field_px, page_px = sample_png_points(img, [field_pt, page_pt])
    assert field_px != page_px, (
        f"trigger field renders as a distinct tonal surface vs the page "
        f"background (field={field_px} page={page_px})"
    )
    # The field is a non-transparent, non-black surface (a real control).
    assert field_px[3] == 255, f"trigger field is opaque; got alpha {field_px[3]}"
    assert sum(field_px[:3]) > 0, f"trigger field is not pure black; got {field_px}"


def _trigger_text(snap) -> str:
    """Concatenate every Text content under the trigger subtree."""
    node = find_by_tag(snap, TRIGGER)
    out: list[str] = []

    def walk(n):
        if isinstance(n, dict):
            if n.get("type") == "Text":
                c = n.get("content")
                if isinstance(c, str):
                    out.append(c)
            for c in n.get("children") or []:
                walk(c)

    walk(node)
    return "".join(out)


def _boot_snapshot_and_rects():
    with RpcSubprocess("hello-combobox", boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        return snap, abs_rects_of(snap)


def capture_screenshot() -> Path:
    """Render hello-combobox's boot (closed) frame to RGBA8 PNG via
    `PINION_SCREENSHOT`, bypassing winit. Producer parity: same
    `to_vello_cached` rasterizer the live window uses."""
    out = Path(tempfile.mkdtemp(prefix="pinion-r714-")) / "combobox.png"
    binary = WORKSPACE_ROOT / "target" / "release" / "hello-combobox"
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", "hello-combobox", "--quiet", "--release",
    ]
    env = os.environ.copy()
    env["PINION_SCREENSHOT"] = str(out)
    res = subprocess.run(
        cmd, cwd=WORKSPACE_ROOT, env=env,
        capture_output=True, text=True, check=False, timeout=120.0,
    )
    if res.returncode != 0:
        raise AssertionError(
            f"PINION_SCREENSHOT capture exited {res.returncode}:\n"
            f"  stderr: {res.stderr!r}"
        )
    if not out.exists():
        raise AssertionError(f"PINION_SCREENSHOT produced no file at {out}")
    return out


if __name__ == "__main__":
    sys.exit(run_demo("R714 §5.38 §5.40 — select-only ComboBox", body))
