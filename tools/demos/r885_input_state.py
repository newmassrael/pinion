#!/usr/bin/env python3
"""R885 §5.49 — `scene/input_state`: the READ peer of the out-of-band
input writes.

Pre-R885 the three out-of-band input caches were write-only over the
wire — an AI client could SET the modifier state (`scene/modifiers`,
R763), ARM a held-key chord (`scene/key state:"down"`, R882) and MOVE
the cursor (every `scene/click` / `scene/hover` / `scene/drag`), but
could never read any of them back: a violation of the AI-first
introspection obligation (§2 #2 — every state an input write mutates
must be observable). `scene/input_state` closes the gap with one READ
whose every field mirrors its write peer's shape (read = inverse of
write, [[wire-form-read-write-symmetry]]):

  * `modifiers` — the `scene/modifiers` param object
    `{shift, ctrl, alt, meta}`; `null` on backends with no absolute
    modifier cache (the TUI §2 #6 carry).
  * `held_keys` — canonical *named* spellings (`["Space"]`), even when
    the chord was armed via the W3C `" "` character.
  * `cursor` — `{x, y}` of the addressed window's last mouse position;
    `null` before the first cursor event.

Verification scope (≥ 30 assertions, exact count = 34):

  (A) boot: all-false modifiers / empty held set / null cursor.
  (B) `scene/modifiers` write → read returns the same four bits.
  (C) `scene/hover` moves the cursor → read returns the hover point.
  (D) `scene/key Space state:"down"` → held_keys ["Space"], cursor
      follows the key position, the modifier cache is untouched.
  (E) the legacy edgeless `scene/key` never perturbs the held cache
      (the R882 cache-inviolate contract, now READ-provable).
  (F) `scene/key state:"up"` (positionless) clears the chord and does
      NOT move the cursor.
  (G) clearing `scene/modifiers` reads back all-false.
  (H) `scene/drag` leaves the cursor at the drag end point.
  (I) the read is side-effect-free (two consecutive reads identical).
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
)

EXAMPLE = "hello-node-editor"


def read(tf) -> dict:
    resp = tf.request("scene/input_state", {})
    assert resp is not None
    return resp.result


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot: nothing held, no cursor event yet ─────────────
        r0 = read(tf)
        assert_eq(sorted(r0.keys()), ["cursor", "held_keys", "modifiers"],
                  "result carries exactly the three input axes")
        for bit in ("shift", "ctrl", "alt", "meta"):
            assert_eq(r0["modifiers"][bit], False, f"boot: {bit} not held")
        assert_eq(r0["held_keys"], [], "boot: no chord key held")
        assert_eq(r0["cursor"], None, "boot: no cursor event landed yet")

        # ── (B) modifiers write → read mirrors the same shape ───────
        tf.request("scene/modifiers",
                   {"shift": True, "ctrl": True, "alt": False, "meta": False})
        r1 = read(tf)
        assert_eq(r1["modifiers"]["shift"], True, "shift reads back held")
        assert_eq(r1["modifiers"]["ctrl"], True, "ctrl reads back held")
        assert_eq(r1["modifiers"]["alt"], False, "alt reads back released")
        assert_eq(r1["modifiers"]["meta"], False, "meta reads back released")

        # ── (C) hover moves the cursor; the read observes it ────────
        tf.hover(at=(120.0, 80.0))
        r2 = read(tf)
        assert_eq(r2["cursor"], {"x": 120.0, "y": 80.0},
                  "cursor = the hover injection point")

        # ── (D) Space down arms the chord; cursor follows the key ───
        tf.key(at=(10.0, 12.0), name="Space", state="down")
        r3 = read(tf)
        assert_eq(r3["held_keys"], ["Space"], "chord reads back held")
        assert_eq(r3["cursor"], {"x": 10.0, "y": 12.0},
                  "cursor follows the key injection position")
        assert_eq(r3["modifiers"]["ctrl"], True,
                  "held-key arm leaves the modifier cache untouched")

        # ── (E) the legacy edgeless key never touches the cache ─────
        tf.key(at=(10.0, 12.0), name="Space")
        assert_eq(read(tf)["held_keys"], ["Space"],
                  "edgeless atomic press is cache-inviolate (R882)")

        # ── (F) positionless release clears the chord only ──────────
        tf.key(name="Space", state="up")
        r4 = read(tf)
        assert_eq(r4["held_keys"], [], "release reads back cleared")
        assert_eq(r4["cursor"], {"x": 10.0, "y": 12.0},
                  "positionless release must not move the cursor")
        assert_eq(r4["modifiers"]["shift"], True,
                  "release must not disturb the modifier cache")

        # ── (G) clearing modifiers reads back all-false ─────────────
        tf.request("scene/modifiers",
                   {"shift": False, "ctrl": False, "alt": False,
                    "meta": False})
        r5 = read(tf)
        for bit in ("shift", "ctrl", "alt", "meta"):
            assert_eq(r5["modifiers"][bit], False, f"cleared: {bit} released")

        # ── (H) a drag leaves the cursor at its end point ────────────
        tf.drag(from_at=(30.0, 30.0), to_at=(90.0, 60.0), steps=4)
        r6 = read(tf)
        assert_eq(r6["cursor"], {"x": 90.0, "y": 60.0},
                  "cursor = the drag release point")

        # ── (I) the read is side-effect-free ────────────────────────
        assert_eq(read(tf), r6, "two consecutive reads are identical")

        # Exact assertion count: A 7 + B 4 + C 1 + D 3 + E 1 + F 3 +
        # G 4 + H 1 + I 1 = 25 assert_eq calls (A's loop = 4 of the 7)
        # + 9 read() non-None asserts = 34 total checks.


if __name__ == "__main__":
    sys.exit(run_demo("R885 §5.49 — scene/input_state READ peer", body))
