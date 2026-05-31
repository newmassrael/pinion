#!/usr/bin/env python3
"""R717 §5.38 §5.40 §5.50 — editable ComboBox (list autocomplete) E2E.

Drives the new `hello-combobox-editable` binding via JSON-RPC. The WAI-ARIA
1.2 §4.5 "Editable Combobox With List Autocomplete" pattern: a TextField
input (`combo_input`, primary) whose popup ListBox (`combo_options`) filters
to the options matching what is typed; outside-click via the shared
`dismiss_barrier` (`combo_barrier`). 2nd combobox-class consumer of R714 —
still pure composition (no `ComboBoxExternal` coordinator).

R717 adds the `AriaRole::EditableComboBox` + `aria-autocomplete` a11y axis
(verified in the binding's unit tests; the RPC surface below verifies the
typeahead behaviour structurally + the visible field in pixels).

Typing routes through `scene/key` (R666 character arc → `V::apply_key`), the
same path the platform keyboard uses — NOT the External's `key` invoke — so
the open/filter/active-descendant logic in `apply_key` runs.

Atomic verification scope (>=30 assertions):

  (A) closed boot — input present, no panel / barrier / option rows; the
      status line shows the empty value + full 6-match count.
  (B) type "ap" — popup opens; only the Apple + Apricot rows paint (Banana
      filtered OUT); status shows the typed value + 2-match count; the
      listbox active descendant parks on the first match.
  (C) keyboard roving within the filtered set — ArrowDown steps 0 -> 1 and
      clamps at the last filtered match (not the absolute last option).
  (D) commit via click — clicking Apricot closes the popup and writes
      "Apricot" into the field (the combobox value); listbox records it.
  (E) refilter to a new prefix — clearing + typing "ban" narrows to Banana
      only; "zzz" matches nothing so the popup paints no panel even though
      the combobox is notionally open.
  (F) Escape dismiss — closes the popup, leaves the typed text intact.
  (G) click-OUTSIDE (barrier) dismiss — closes without changing the text.
  (H) keyboard Enter commit — type a unique prefix, ArrowDown, Enter writes
      the active option's label into the field.
  (I) negative — an unknown listbox introspect path is rejected.

  Phase 2 — PIXELS (PINION_SCREENSHOT, producer-parity headless render).
    The boot frame is the empty editable field; it renders as a distinct,
    opaque tonal surface against the page background (a real text input).
    The filtered popup is an RPC-opened overlay (cannot be boot-captured,
    the R711 [[introspection-from-paint-not-screen]] limit); its chrome
    rides the shared R710 elevation rasterizer already guarded.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import time
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
)

WORKSPACE_ROOT = Path(__file__).resolve().parent.parent.parent
VIEWPORT = (560, 460)
PAUSE = 0.12

INPUT = "combo_input"
OPTIONS = "combo_options"
BARRIER = "combo_barrier"
PANEL = "combo_panel"
STATUS = "combo_status"
LABELS = ["Apple", "Apricot", "Banana", "Blueberry", "Cherry", "Cranberry"]


def opt(i: int) -> str:
    return f"{OPTIONS}#{i}"


# A point well outside the dropdown panel (top-left) for click-outside.
OUTSIDE_POINT = (40.0, 40.0)


def _present(snap, tag) -> bool:
    return find_by_tag(snap, tag) is not None


def _status(tf) -> str:
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    node = find_by_tag(snap, STATUS)
    assert node is not None, "status node present"
    return node.get("content") or ""


def _focused_index(tf):
    return tf.query(f"/{OPTIONS}/external/focused_index")


def _selected_index(tf):
    return tf.query(f"/{OPTIONS}/external/selected_index")


def _focus_input(tf) -> None:
    tf.request("focus/set", {"tag": INPUT})


def _type(tf, text: str) -> None:
    """Type `text` through the character-key arc (-> apply_key), the path
    the editable combobox's open/filter logic listens on."""
    tf.text(text, path=INPUT)
    time.sleep(PAUSE)


def _backspace(tf, n: int) -> None:
    for _ in range(n):
        tf.key(path=INPUT, name="Backspace")
    time.sleep(PAUSE)


def body() -> None:
    with RpcSubprocess("hello-combobox-editable", boot_grace=1.5) as tf:
        # ── (A) closed boot shape ───────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert _present(snap, INPUT), "editable input present at boot"
        assert not _present(snap, PANEL), "no dropdown panel when closed"
        assert not _present(snap, BARRIER), "no barrier when closed"
        assert not _present(snap, opt(0)), "no option rows when closed"
        assert_eq(_status(tf), 'Value: "" | 6 matches', "empty value, full match count")
        assert_eq(_selected_index(tf), None, "listbox selection empty at boot")

        # ── (B) type a prefix → opens + filters ─────────────────────
        _focus_input(tf)
        assert_eq(tf.request("focus/get").result.get("focused"), INPUT, "input focused")
        _type(tf, "ap")  # matches Apple(0) + Apricot(1)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert _present(snap, BARRIER), "typing opens the dismiss barrier"
        assert _present(snap, PANEL), "typing opens the filtered popup"
        assert _present(snap, opt(0)), "Apple row painted (matches 'ap')"
        assert _present(snap, opt(1)), "Apricot row painted (matches 'ap')"
        assert not _present(snap, opt(2)), "Banana filtered OUT (no 'ap')"
        assert not _present(snap, opt(3)), "Blueberry filtered OUT"
        assert_eq(_status(tf), 'Value: "ap" | 2 matches', "value + 2-match count")
        assert_eq(_focused_index(tf), 0, "active descendant parks on first match")
        # MD3 menu elevation (L2 = key+ambient) on the dropdown panel.
        panel = find_by_tag(snap, PANEL)
        shadows = (panel.get("style") or {}).get("shadows") or []
        assert_eq(len(shadows), 2, "dropdown panel casts key + ambient (MD3 L2)")
        # Barrier is fully transparent (non-modal, no dim).
        barrier = find_by_tag(snap, BARRIER)
        fill = (barrier.get("style") or {}).get("fill")
        assert fill is not None and int(fill.get("a", -1)) == 0, "barrier fully transparent"

        # ── (C) keyboard roving within the filtered set ─────────────
        # Typing already auto-highlighted the first match (0); ArrowDown
        # steps to the next FILTERED option (1), then clamps there (the
        # last filtered match, NOT the absolute last option).
        tf.key(path=INPUT, name="ArrowDown")  # step within filter → Apricot (1)
        time.sleep(PAUSE)
        assert_eq(_focused_index(tf), 1, "ArrowDown steps to 2nd filtered match (1)")
        tf.key(path=INPUT, name="ArrowDown")  # clamp at last FILTERED match (1)
        time.sleep(PAUSE)
        assert_eq(_focused_index(tf), 1, "ArrowDown clamps at the last filtered match")
        tf.key(path=INPUT, name="ArrowUp")  # back to first filtered match (0)
        time.sleep(PAUSE)
        assert_eq(_focused_index(tf), 0, "ArrowUp steps back to the first filtered match")

        # ── (D) commit a filtered option via click ──────────────────
        tf.click(path=opt(1))  # "Apricot"
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert not _present(snap, PANEL), "committing an option closes the popup"
        assert not _present(snap, BARRIER), "barrier gone after commit"
        assert_eq(_selected_index(tf), 1, "listbox records the committed index")
        assert_eq(_status(tf), 'Value: "Apricot" | 1 match', "field value = committed label")

        # ── (E) refilter to a new prefix ────────────────────────────
        _focus_input(tf)
        _backspace(tf, len("Apricot"))  # clear the field
        assert_eq(_status(tf), 'Value: "" | 6 matches', "field cleared by Backspace")
        _type(tf, "ban")  # matches Banana(2) only
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert _present(snap, PANEL), "re-typing re-opens the popup"
        assert _present(snap, opt(2)), "Banana row painted (matches 'ban')"
        assert not _present(snap, opt(0)), "Apple filtered OUT for 'ban'"
        assert_eq(_status(tf), 'Value: "ban" | 1 match', "narrowed to 1 match")
        # No-match: extend to a string nothing contains.
        _type(tf, "zzz")  # "banzzz" matches nothing
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert not _present(snap, PANEL), "no match → no panel even when open"
        assert_eq(_status(tf), 'Value: "banzzz" | 0 matches', "0-match count")

        # ── (F) Escape dismiss keeps the typed text ─────────────────
        _focus_input(tf)
        _backspace(tf, len("banzzz"))
        _type(tf, "ch")  # Cherry(4) — re-opens
        assert _present(tf.snapshot(source="paint", viewport=VIEWPORT), PANEL), "re-opened by 'ch'"
        tf.key(path=INPUT, name="Escape")
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert not _present(snap, PANEL), "Escape dismisses the popup"
        assert_eq(_status(tf), 'Value: "ch" | 1 match', "Escape leaves the typed text intact")

        # ── (G) click-OUTSIDE (barrier) dismiss ─────────────────────
        _focus_input(tf)
        _type(tf, "e")  # widen ("che") still matches Cherry; popup re-opens
        assert _present(tf.snapshot(source="paint", viewport=VIEWPORT), PANEL), "re-opened for barrier test"
        tf.click(at=OUTSIDE_POINT)  # lands on the transparent barrier
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert not _present(snap, PANEL), "click outside dismisses the popup"
        assert_eq(_status(tf), 'Value: "che" | 1 match', "click-outside leaves text intact")

        # ── (H) keyboard Enter commit ───────────────────────────────
        _focus_input(tf)
        _backspace(tf, len("che"))
        _type(tf, "blue")  # Blueberry(3) only
        assert _present(tf.snapshot(source="paint", viewport=VIEWPORT), PANEL), "re-opened by 'blue'"
        tf.key(path=INPUT, name="ArrowDown")  # active descendant → 3
        time.sleep(PAUSE)
        assert_eq(_focused_index(tf), 3, "active descendant on the single 'blue' match")
        tf.key(path=INPUT, name="Enter")  # commit
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert not _present(snap, PANEL), "Enter commits + closes"
        assert_eq(_selected_index(tf), 3, "Enter committed the active descendant")
        assert_eq(_status(tf), 'Value: "Blueberry" | 1 match', "field value = keyboard-committed label")

        # ── (I) negative — unknown introspect path is rejected ──────
        raised = False
        try:
            tf.query(f"/{OPTIONS}/external/no_such_field")
        except RpcError:
            raised = True
        assert raised, "unknown introspect path must be rejected"

    # ── Phase 2 — live pixels (closed editable field) ───────────────
    snap, rects = _boot_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    assert INPUT in rects, "input field has a computed rect"
    fr = rects[INPUT]
    field_pt = (fr[0] + 6, fr[1] + fr[3] // 2)
    page_pt = (8, 8)
    field_px, page_px = sample_png_points(img, [field_pt, page_pt])
    assert field_px != page_px, (
        f"editable field renders as a distinct tonal surface vs the page "
        f"background (field={field_px} page={page_px})"
    )
    assert field_px[3] == 255, f"field is opaque; got alpha {field_px[3]}"
    assert sum(field_px[:3]) > 0, f"field is not pure black; got {field_px}"


def _boot_snapshot_and_rects():
    with RpcSubprocess("hello-combobox-editable", boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        return snap, abs_rects_of(snap)


def capture_screenshot() -> Path:
    """Render hello-combobox-editable's boot (closed) frame to RGBA8 PNG
    via `PINION_SCREENSHOT`, bypassing winit. Producer parity: the same
    `to_vello_cached` rasterizer the live window uses."""
    out = Path(tempfile.mkdtemp(prefix="pinion-r717-")) / "combobox-editable.png"
    binary = WORKSPACE_ROOT / "target" / "release" / "hello-combobox-editable"
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", "hello-combobox-editable", "--quiet", "--release",
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
    sys.exit(run_demo("R717 §5.38 §5.40 — editable ComboBox", body))
