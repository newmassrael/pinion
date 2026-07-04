#!/usr/bin/env python3
"""R1221 §5.38 §5.22 §5.40 — multi-object inspector INLINE edit gestures.

R922 built the multi-object inspector's model (common-property derivation, the
"Multiple Values" mixed state, and a write-all edit) but edited **only over
RPC** (`intervene value.<i>`); inline click-to-edit cell delegates were the
documented GUI follow-up. R1221 closes that: the Details value cells are now
interactive and write across the ENTIRE selection —

  * a **Bool** cell CLICK toggles the property, resolving a mixed Bool to a
    single uniform state (`inspector#toggle<i>` -> `toggle_property`);
  * an **Int / Float** cell gets `[-] value [+]` STEPPERS that shift each
    selected object RELATIVELY (`inspector#dec<i>` / `inc<i>` ->
    `step_property`), so a mixed numeric stays mixed but moves together (the
    multi-object spinbox that PRESERVES per-object differences).

The GUI gesture and the AI-first verbs (`toggle_property` / `step_property`)
funnel through the SAME write the RPC `intervene value.<i>` uses (invoke-funnel
discipline). Everything is observable over the §5.12 RPC plane (§2 #2), no
pixels: `value.<i>` reads the representative, `mixed.<i>` the disagreement flag.

The three objects share a common actor base Visible(0)=Bool / Layer(1)=Int /
Locked(2)=Bool, with a deliberate value mix (Player/Camera Layer=1, Light=2).

  (A) boot + select_all: the common base rows, all mixed.
  (B) RPC toggle_property resolves a mixed Bool to uniform (write-all).
  (C) RPC step_property shifts each object relatively (mix PRESERVED).
  (D) GUI CLICK the painted Bool cell + the +/- steppers (the paint->route path).
  (E) an AGREEING selection: a step keeps it uniform (both shifted equally).
  (F) rejects: toggle a non-Bool, step a non-numeric, a malformed step spec,
      and edits with no selection are all benign no-ops / typed errors.

Run from the workspace root:
    cargo build -p hello-inspector --release
    python3 tools/demos/r1221_inspector_inline_edit.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_until,
)

INSPECTOR = "inspector"


def q(tf: RpcSubprocess, key: str) -> Any:
    return tf.query(f"/external/{key}")


def select_all(tf: RpcSubprocess) -> None:
    tf.invoke("/external/select_all", None)


def rebaseline(tf: RpcSubprocess) -> None:
    """Restore every object to its class default + select all three, so each
    section starts from the known boot mix (Visible t/t/f, Layer 1/1/2)."""
    select_all(tf)
    tf.invoke("/external/reset_all", None)


def body() -> None:
    with RpcSubprocess("hello-inspector", boot_grace=1.5) as tf:
        # ── (A) boot + select_all: the common base, all mixed ────────
        wait_until(lambda: True if q(tf, "object_count") == 3 else None,
                   desc="inspector ready")
        select_all(tf)
        assert_eq(q(tf, "selection_count"), 3, "all three selected")
        assert_eq(q(tf, "row_count"), 3, "Visible / Layer / Locked are common")
        assert_eq(q(tf, "name.0"), "Visible", "row 0 is the Bool")
        assert_eq(q(tf, "name.1"), "Layer", "row 1 is the Int")
        assert_eq(q(tf, "mixed.0"), True, "Visible is mixed (t, t, f)")
        assert_eq(q(tf, "mixed.1"), True, "Layer is mixed (1, 1, 2)")
        assert_eq(q(tf, "value.0"), True, "value.0 = the representative (Player) Visible")
        assert_eq(q(tf, "value.1"), 1, "value.1 = the representative (Player) Layer")

        # ── (B) RPC toggle_property: resolve a mixed Bool to uniform ──
        assert_eq(tf.invoke("/external/toggle_property", 0), True, "toggle Visible across all")
        assert_eq(q(tf, "mixed.0"), False, "the mix is resolved")
        assert_eq(q(tf, "value.0"), False, "flipped the representative (true) -> all false")
        assert_eq(tf.invoke("/external/toggle_property", 0), True, "toggle back")
        assert_eq(q(tf, "value.0"), True, "all true again")
        assert_eq(q(tf, "mixed.0"), False, "still uniform")

        # ── (C) RPC step_property: relative shift, mix PRESERVED ──────
        rebaseline(tf)
        assert_eq(q(tf, "mixed.1"), True, "Layer mixed again after rebaseline")
        assert_eq(tf.invoke("/external/step_property", "1,1"), True, "step Layer +1 across all")
        assert_eq(q(tf, "value.1"), 2, "representative Layer 1 -> 2")
        assert_eq(q(tf, "mixed.1"), True, "each shifted by 1 -> still mixed (2, 2, 3)")
        assert_eq(tf.invoke("/external/step_property", "1,-1"), True, "step -1 back")
        assert_eq(q(tf, "value.1"), 1, "representative back to 1")

        # ── (D) GUI CLICK the painted cells (paint -> route) ─────────
        rebaseline(tf)
        # Click the Locked(2) Bool value cell: resolves its mix across all.
        tf.click(path=f"{INSPECTOR}#toggle2")
        wait_until(lambda: True if q(tf, "mixed.2") is False else None,
                   desc="clicking the Bool cell toggled Locked across all")
        # Click the Layer(1) +/- steppers: +1, +1, -1 = net +1.
        tf.click(path=f"{INSPECTOR}#inc1")
        tf.click(path=f"{INSPECTOR}#inc1")
        tf.click(path=f"{INSPECTOR}#dec1")
        wait_until(lambda: True if q(tf, "value.1") == 2 else None,
                   desc="stepper clicks netted +1 on the representative Layer")

        # ── (E) an AGREEING selection stays uniform after a step ─────
        tf.intervene("/external/selection", [0, 1])  # Player + Camera, Layer both 1
        assert_eq(q(tf, "mixed.1"), False, "agreeing Layer is not mixed")
        assert_eq(q(tf, "value.1"), 2, "both are 2 (from D's net +1)")
        assert_eq(tf.invoke("/external/step_property", "1,3"), True, "step +3")
        assert_eq(q(tf, "value.1"), 5, "both shifted equally 2 -> 5")
        assert_eq(q(tf, "mixed.1"), False, "still uniform (both moved the same)")

        # ── (F) rejects + no-selection no-ops ────────────────────────
        select_all(tf)
        assert_eq(tf.invoke("/external/toggle_property", 1), False,
                  "toggle on a non-Bool (Layer) is a no-op false")
        assert_eq(tf.invoke("/external/step_property", "0,1"), False,
                  "step on a non-numeric (Visible) is a no-op false")
        assert_eq(tf.invoke("/external/toggle_property", 99), False,
                  "an out-of-range index is a no-op false (never panics)")
        malformed = False
        try:
            tf.invoke("/external/step_property", "not-a-spec")
        except RpcError:
            malformed = True
        assert malformed, "a malformed step spec is a typed Rejected error"
        tf.invoke("/external/clear", None)
        assert_eq(q(tf, "row_count"), 0, "no selection -> no common rows")
        assert_eq(tf.invoke("/external/toggle_property", 0), False, "no-selection toggle is false")
        assert_eq(tf.invoke("/external/step_property", "0,1"), False, "no-selection step is false")


if __name__ == "__main__":
    sys.exit(run_demo("R1221 inspector inline multi-object edit", body))
