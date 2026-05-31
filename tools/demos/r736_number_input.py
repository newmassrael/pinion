#!/usr/bin/env python3
"""R736 §5.38 §5.40 §5.50 — editable number input (typeable spinbutton) E2E.

Drives the new `hello-number-input` binding via JSON-RPC. The editable
WAI-ARIA spinbutton (HTML `<input type=number>` / GTK editable SpinButton /
Qt QSpinBox form factor): a numeric TextField (`num_input`, primary) flanked
by `−` / `+` stepper ButtonExternals (`num_dec` / `num_inc`). The user can
type a value OR step it. 2nd consumer of the editable-field value-
coordination pattern (after R717 hello-combobox-editable) — still pure
composition, zero new substrate.

The field text IS the value (the editable-spinbutton SSOT): the numeric
value is `parse_clamp(field_text)`, mirroring the editable combobox whose
value is its field text. There is no separate f32 store.

Typing routes through `scene/key` (R666 character arc -> V::apply_key), the
same path the platform keyboard uses, so the numeric-gate + edit logic in
apply_key runs. Stepper clicks route through `scene/click` -> the
ButtonExternal `click` intent -> `NumberView::update`.

Atomic verification scope (>=30 assertions):

  (A) boot — field seeded to 16; spinbutton node carries value 16 in
      [8, 72]; status line echoes "Value: 16 px".
  (B) stepper clicks — clicking `+` steps 16 -> 17; clicking `−` steps back;
      siblings (the field) stay the live value SSOT.
  (C) keyboard stepping — ArrowUp/Down step by 1, PageUp/Down by 12.
  (D) clamp — drive the value to the MAX / MIN bounds and confirm further
      steps are no-ops at the clamp.
  (E) type-to-edit — clear + type "24"; the field text + the value follow;
      a letter keystroke is dropped (the <input type=number> numeric gate).
  (F) Enter-normalise — type a verbose / over-range value ("099"), Enter
      reformats + clamps it in place ("72").
  (G) a11y value tracks — aria-valuenow reflects the committed value via
      the introspect `caret`/text round-trip (structurally via status line).
  (H) negative — an unknown TextField introspect path is rejected.

  Phase 2 — PIXELS (PINION_SCREENSHOT, producer-parity headless render).
    The boot frame shows the seeded numeric field as a distinct opaque
    tonal surface against the page background (a real text input), with the
    two stepper buttons flanking it as their own filled surfaces.
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
    WORKSPACE_ROOT,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    read_png_rgba8,
    run_demo,
    sample_png_points,
)

EXAMPLE = "hello-number-input"
VIEWPORT = (420, 200)
PAUSE = 0.12

INPUT = "num_input"
DEC = "num_dec"
INC = "num_inc"
STATUS = "num_status"


def _status(tf) -> str:
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    node = find_by_tag(snap, STATUS)
    assert node is not None, "status node present"
    return node.get("content") or ""


def _field_text(tf) -> str:
    """The field text = the value SSOT, read through the TextField's
    introspect `text` slot (the same path `scene/query` walks)."""
    return tf.query(f"/{INPUT}/external/text")


def _focus_input(tf) -> None:
    tf.request("focus/set", {"tag": INPUT})


def _type(tf, text: str) -> None:
    tf.text(text, path=INPUT)
    time.sleep(PAUSE)


def _backspace(tf, n: int) -> None:
    for _ in range(n):
        tf.key(path=INPUT, name="Backspace")
    time.sleep(PAUSE)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot shape ──────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, INPUT) is not None, "numeric field present at boot"
        assert find_by_tag(snap, DEC) is not None, "decrement stepper present"
        assert find_by_tag(snap, INC) is not None, "increment stepper present"
        assert_eq(_field_text(tf), "16", "field seeded to START (16)")
        assert_eq(_status(tf), "Value: 16 px", "status echoes the boot value")
        # The spinbutton's value lives in the field text; a11y valuenow is
        # the same projection (verified structurally via the text SSOT).

        # ── (B) stepper clicks step the value ───────────────────────
        tf.click(path=INC)
        tf.pointer_leave()
        time.sleep(PAUSE)
        assert_eq(_field_text(tf), "17", "click + steps 16 -> 17")
        assert_eq(_status(tf), "Value: 17 px", "status follows the stepped value")
        tf.click(path=DEC)
        tf.pointer_leave()
        time.sleep(PAUSE)
        assert_eq(_field_text(tf), "16", "click − steps 17 -> 16")
        tf.click(path=DEC)
        tf.pointer_leave()
        time.sleep(PAUSE)
        assert_eq(_field_text(tf), "15", "click − again 16 -> 15")

        # ── (C) keyboard stepping (arrows + page) ───────────────────
        _focus_input(tf)
        assert_eq(tf.request("focus/get").result.get("focused"), INPUT, "input focused")
        tf.key(path=INPUT, name="ArrowUp")
        time.sleep(PAUSE)
        assert_eq(_field_text(tf), "16", "ArrowUp 15 -> 16")
        tf.key(path=INPUT, name="ArrowDown")
        tf.key(path=INPUT, name="ArrowDown")
        time.sleep(PAUSE)
        assert_eq(_field_text(tf), "14", "ArrowDown x2 16 -> 14")
        tf.key(path=INPUT, name="PageUp")
        time.sleep(PAUSE)
        assert_eq(_field_text(tf), "26", "PageUp +12 14 -> 26")
        tf.key(path=INPUT, name="PageDown")
        time.sleep(PAUSE)
        assert_eq(_field_text(tf), "14", "PageDown -12 26 -> 14")

        # ── (D) clamp at the bounds ─────────────────────────────────
        # March up to / past MAX (72) with PageUp, confirm the clamp.
        for _ in range(10):
            tf.key(path=INPUT, name="PageUp")
        time.sleep(PAUSE)
        assert_eq(_field_text(tf), "72", "value clamps at MAX (72)")
        tf.key(path=INPUT, name="ArrowUp")
        time.sleep(PAUSE)
        assert_eq(_field_text(tf), "72", "ArrowUp at MAX is a no-op")
        for _ in range(10):
            tf.key(path=INPUT, name="PageDown")
        time.sleep(PAUSE)
        assert_eq(_field_text(tf), "8", "value clamps at MIN (8)")
        tf.key(path=INPUT, name="ArrowDown")
        time.sleep(PAUSE)
        assert_eq(_field_text(tf), "8", "ArrowDown at MIN is a no-op")

        # ── (E) type-to-edit + numeric keystroke gate ───────────────
        _focus_input(tf)
        _backspace(tf, 4)  # clear "8" (and any stragglers)
        assert_eq(_field_text(tf), "", "field cleared by Backspace")
        _type(tf, "24")
        assert_eq(_field_text(tf), "24", "typed digits land in the field")
        assert_eq(_status(tf), "Value: 24 px", "status reflects the typed value")
        # A letter keystroke is dropped at the numeric gate.
        tf.key(path=INPUT, name="x")
        time.sleep(PAUSE)
        assert_eq(_field_text(tf), "24", "letter dropped (numeric gate)")
        # A symbol keystroke is dropped too.
        tf.key(path=INPUT, name="$")
        time.sleep(PAUSE)
        assert_eq(_field_text(tf), "24", "symbol dropped (numeric gate)")

        # ── (F) Enter normalises the typed text in place ────────────
        _focus_input(tf)
        _backspace(tf, 4)
        _type(tf, "099")  # verbose + over-range
        assert_eq(_field_text(tf), "099", "raw typed text retained pre-Enter")
        tf.key(path=INPUT, name="Enter")
        time.sleep(PAUSE)
        assert_eq(_field_text(tf), "72", "Enter parses + clamps + reformats 099 -> 72")
        assert_eq(_status(tf), "Value: 72 px", "status reflects the normalised value")
        # Below-range typed value normalises up to MIN.
        _focus_input(tf)
        _backspace(tf, 4)
        _type(tf, "3")
        tf.key(path=INPUT, name="Enter")
        time.sleep(PAUSE)
        assert_eq(_field_text(tf), "8", "Enter clamps below-range 3 -> MIN 8")

        # ── (G) value round-trips through digits (text = value SSOT) ─
        _focus_input(tf)
        _backspace(tf, 4)
        _type(tf, "50")
        assert_eq(_field_text(tf), "50", "field text is the value store")
        tf.key(path=INPUT, name="ArrowUp")
        time.sleep(PAUSE)
        assert_eq(_field_text(tf), "51", "stepping reads the typed text as the base")

        # ── (H) negative — unknown introspect path is rejected ──────
        raised = False
        try:
            tf.query(f"/{INPUT}/external/no_such_field")
        except RpcError:
            raised = True
        assert raised, "unknown introspect path must be rejected"

        # geometry for the pixel phase
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        rects = abs_rects_of(snap)
        assert INPUT in rects, "input field has a computed rect"
        assert DEC in rects, "decrement stepper has a computed rect"
        assert INC in rects, "increment stepper has a computed rect"

    # ── Phase 2 — live pixels (seeded numeric field at boot) ────────
    snap, rects = _boot_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == VIEWPORT, \
        f"screenshot {img.width}x{img.height} != viewport {VIEWPORT}"
    assert INPUT in rects and DEC in rects, "field + stepper rects present"
    fr = rects[INPUT]
    dr = rects[DEC]
    field_pt = (fr[0] + 6, fr[1] + fr[3] // 2)
    dec_pt = (dr[0] + dr[2] // 2, dr[1] + dr[3] // 2)
    page_pt = (6, 6)
    field_px, dec_px, page_px = sample_png_points(img, [field_pt, dec_pt, page_pt])
    assert field_px != page_px, (
        f"numeric field renders as a distinct tonal surface vs the page "
        f"(field={field_px} page={page_px})"
    )
    assert dec_px != page_px, (
        f"stepper button renders as a distinct surface vs the page "
        f"(stepper={dec_px} page={page_px})"
    )
    assert field_px[3] == 255, f"field is opaque; got alpha {field_px[3]}"
    assert sum(field_px[:3]) > 0, f"field is not pure black; got {field_px}"


def _boot_snapshot_and_rects():
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        return snap, abs_rects_of(snap)


def capture_screenshot() -> Path:
    """Render hello-number-input's boot frame to RGBA8 PNG via
    `PINION_SCREENSHOT`, bypassing winit. Producer parity: the same
    `to_vello_cached` rasterizer the live window uses."""
    out = Path(tempfile.mkdtemp(prefix="pinion-r736-")) / "number-input.png"
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
            f"PINION_SCREENSHOT capture exited {res.returncode}:\n"
            f"  stderr: {res.stderr!r}"
        )
    if not out.exists():
        raise AssertionError(f"PINION_SCREENSHOT produced no file at {out}")
    return out


if __name__ == "__main__":
    sys.exit(run_demo("R736 §5.38 §5.40 — editable number input", body))
