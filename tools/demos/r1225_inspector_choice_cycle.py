#!/usr/bin/env python3
"""R1225 §5.38 §5.22 §5.40 — inspector Choice/enum cell CYCLE (keyboard + click).

R1221 made Bool + numeric Details cells interactive; R1224 gave them a keyboard
path. The **Choice** (enum) cell stayed GUI-uneditable (RPC `intervene value.<i>`
only). R1225 closes that: a Choice cell cycles prev/next option —

  * over RPC: `invoke cycle_property "<i>,<dir>"` (the enum peer of
    `step_property`), RELATIVE + WRAPPING per object so a mixed enum stays mixed
    but rotates together (the multi-object segmented control);
  * by keyboard: on the Details property cursor (R1224), ArrowRight / ArrowLeft
    cycle a Choice (the same key that numeric-steps a spinbutton — dispatched by
    the cursor cell's kind);
  * by click: the painted cell is `inspector#cycle<i>`, a click advances +1.

The a11y (unit-tested in the crate) makes a Choice row a `combobox` carrying the
selected option as `aria-valuetext`. All observable over the §5.12 RPC plane
(§2 #2), no pixels: `value.<i>` reads the representative Choice, `kind.<i>` its
kind. Player's `Team` (Red / Blue / Neutral) is the demo enum (common row 5).

  (A) boot: Team is a common Choice row; the selected option reads over RPC.
  (B) RPC cycle wraps forward + backward (Red -> Blue -> Neutral -> Red).
  (C) keyboard: focus Details, navigate to Team, ArrowRight/Left cycle it.
  (D) click the painted Choice cell cycles +1.
  (E) rejects: cycle a non-Choice / a malformed spec are benign no-ops / errors.

Run from the workspace root:
    cargo build -p hello-inspector --release
    python3 tools/demos/r1225_inspector_choice_cycle.py
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
TEAM = 5  # Player's `Team` Choice is common row 5 (Visible/Layer/Locked/Health/Speed/Team/Tint).


def q(tf: RpcSubprocess, key: str) -> Any:
    return tf.query(f"/external/{key}")


def team(tf: RpcSubprocess) -> int:
    """The representative Team option index off `value.5` (a Choice JSON)."""
    return q(tf, f"value.{TEAM}")["selected"]


def wait_team(tf: RpcSubprocess, expected: int, desc: str) -> None:
    wait_until(lambda: True if team(tf) == expected else None, desc=desc)


def cycle(tf: RpcSubprocess, spec: str) -> Any:
    return tf.invoke("/external/cycle_property", spec)


def body() -> None:
    with RpcSubprocess("hello-inspector", boot_grace=1.5) as tf:
        # ── (A) boot: Team is a common Choice ────────────────────────
        wait_until(lambda: True if q(tf, "object_count") == 3 else None,
                   desc="inspector ready")
        # Boot selection is Player (object 0) alone -> its 7 props are common.
        assert_eq(q(tf, "selected"), 0, "Player is the boot selection")
        assert_eq(q(tf, "selection_count"), 1, "one object selected")
        assert_eq(q(tf, "row_count"), 7, "Player exposes 7 properties")
        assert_eq(q(tf, "kind.0"), "bool", "row 0 (Visible) is a Bool")
        assert_eq(q(tf, "kind.1"), "int", "row 1 (Layer) is an Int")
        assert_eq(q(tf, f"kind.{TEAM}"), "choice", "row 5 (Team) is a Choice")
        assert_eq(q(tf, f"name.{TEAM}"), "Team", "row 5 is named Team")
        assert_eq(q(tf, f"mixed.{TEAM}"), False, "single selection -> Team not mixed")
        assert_eq(q(tf, f"value.{TEAM}")["options"],
                  ["Red", "Blue", "Neutral"], "Team's option list")
        assert_eq(team(tf), 0, "Team starts on Red (option 0)")

        # ── (B) RPC cycle wraps forward + backward ───────────────────
        assert_eq(cycle(tf, f"{TEAM},1"), True, "cycle +1 on a Choice returns true")
        wait_team(tf, 1, desc="Red -> Blue")
        cycle(tf, f"{TEAM},1")
        wait_team(tf, 2, desc="Blue -> Neutral")
        cycle(tf, f"{TEAM},1")
        wait_team(tf, 0, desc="Neutral wraps forward to Red")
        cycle(tf, f"{TEAM},-1")
        wait_team(tf, 2, desc="Red wraps backward to Neutral")
        cycle(tf, f"{TEAM},-1")
        wait_team(tf, 1, desc="Neutral -> Blue backward")

        # ── (C) keyboard: focus Details, navigate to Team, cycle ─────
        tf.click(path=f"{INSPECTOR}#0")  # focus the widget (re-selects Player)
        assert_eq(q(tf, "focus_region"), "objects", "click keeps the Objects pane")
        tf.key(path=INSPECTOR, name="Tab")
        wait_until(lambda: True if q(tf, "focus_region") == "details" else None,
                   desc="Tab -> Details pane")
        # Seeded at row 0; ArrowDown x5 lands the property cursor on Team (row 5).
        for _ in range(TEAM):
            tf.key(path=INSPECTOR, name="ArrowDown")
        wait_until(lambda: True if q(tf, "prop_cursor") == TEAM else None,
                   desc="ArrowDown x5 -> property cursor on Team")
        before = team(tf)
        tf.key(path=INSPECTOR, name="ArrowRight")
        wait_team(tf, (before + 1) % 3, desc="ArrowRight cycles the Choice +1")
        assert_eq(q(tf, "prop_cursor"), TEAM, "cycling does not move the cursor")
        assert_eq(q(tf, "focus_region"), "details", "still in the Details pane")
        tf.key(path=INSPECTOR, name="ArrowLeft")
        wait_team(tf, before, desc="ArrowLeft cycles the Choice -1 (back)")
        # Home / End jump the cursor off Team and back to bound rows.
        tf.key(path=INSPECTOR, name="Home")
        wait_until(lambda: True if q(tf, "prop_cursor") == 0 else None,
                   desc="Home -> first row")
        tf.key(path=INSPECTOR, name="End")
        wait_until(lambda: True if q(tf, "prop_cursor") == 6 else None,
                   desc="End -> last row (Tint)")
        # Space on the numeric-less Tint (a Color) is a benign no-op; re-seat on Team.
        tf.invoke("/external/focus_property", TEAM)
        assert_eq(q(tf, "prop_cursor"), TEAM, "focus_property re-seats the cursor on Team")

        # ── (D) click the painted Choice cell cycles +1 ──────────────
        before = team(tf)
        tf.click(path=f"{INSPECTOR}#cycle{TEAM}")
        wait_team(tf, (before + 1) % 3, desc="clicking the Choice cell cycles +1")

        # ── (E) rejects + non-Choice no-ops ──────────────────────────
        assert_eq(cycle(tf, "0,1"), False, "cycle on a Bool (Visible) is a no-op false")
        assert_eq(cycle(tf, "1,1"), False, "cycle on an Int (Layer) is a no-op false")
        assert_eq(cycle(tf, "99,1"), False, "an out-of-range index is a no-op false")
        malformed = False
        try:
            cycle(tf, "not-a-spec")
        except RpcError:
            malformed = True
        assert malformed, "a malformed cycle spec is a typed Rejected error"


if __name__ == "__main__":
    sys.exit(run_demo("R1225 inspector Choice cell cycle", body))
