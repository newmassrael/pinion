#!/usr/bin/env python3
"""R1518 §5.39 §5.40 §2 #7 — the AT focus target and the tree's focus flag are
one answer.

`scene/access` states focus twice: `focus` (the `AccessFocus` target — the tag
AccessKit is given, plus the `active_descendant` a roving composite addresses)
and, per node, `state.focused`. Only the first reaches a screen reader:
`lower_access_node` carries `checked` / `mixed` / `disabled` and no focus at all,
so the AT learns focus from the target alone. The flag lives on the wire, where
it is the AI client's spelling of that same fact (§2 #2 — the RPC path is the
primary one).

Two spellings of one fact were two sources. Measured over this wire across 188
examples before the fix (940 observations — boot plus four `focus/next` stops
each), **144 disagreed with the target the same tree published**:

  * 130 MISSING — a focus target no node echoed (`hello-window-focus-multi`'s
    `main_btn` was focused and said nothing).
  * 11 MOVED — a roving cell flagged for the AI while the AT was told only the
    container: `hello-data-grid` named `data_grid#0_0` on the wire and reported
    `{"tag": "data_grid"}` to AccessKit, so a screen reader could not announce
    which cell the cursor was on. Same for `hello-inspector`,
    `hello-model-chart`, `hello-tabbed-chart`.
  * 3 OVERCLAIM — a flag with no focus anywhere (`hello-tree-view`,
    `hello-tree-grid`, `hello-virtual-tree` each claimed a row at boot while
    `focus/get` answered `None`).

R1517 removed that class in the five bindings a demo walked. R1518 removes it
for every binding at once: `pinion_a11y::build_access_tree` — the assembler the
AccessKit emit, the `scene/access` dump and the TUI shell all share — now STAMPS
the flag from the target it just read. A binding cannot claim focus the shell did
not grant, drop one it did, or put it on a node other than the one the AT was
told about, whether or not any demo walks it.

What this demo adds on top of that construction:

  (A) boot — no focus, so no node claims it. The control: it makes every later
      assertion attributable to a focus move rather than to a node that always
      says `true`. `hello-tree-view` is here because it is exactly what it used
      to fail.
  (B) per binding, walk the ring and assert at each stop that the claimed set is
      EXACTLY the bearer the target names — `active_descendant` when it names
      one, else the focus tag, and nothing when that node is not in this frame's
      tree (a scrolled-away roving row). Not "accounted for" (R1517's string
      relation over `{stop}#…`, which a `prop_3` under an `inspector` stop
      cannot satisfy): the target NAMES the bearer, so the exact set is the
      honest assertion.
  (C) the four bindings this round repaired: assert the target now names an
      active descendant. That is the half a screen reader hears, and the half a
      counterfactual can kill — reverting the assembler alone leaves (A)/(B)
      passing here.

Run from the workspace root:
    cargo build --release -p hello-data-grid -p hello-inspector \\
        -p hello-model-chart -p hello-tabbed-chart -p hello-tree-view \\
        -p hello-tree-grid -p hello-virtual-tree -p hello-property-grid \\
        -p hello-combobox -p hello-listbox -p hello-tabs -p hello-toolbar \\
        -p hello-dialog -p settings-panel
    python3 tools/demos/r1518_at_focus_one_answer.py

>= 30 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    access_focus_flags,
    assert_eq,
    run_demo,
)

#: Bindings walked end to end. The three that over-claimed at boot, the four
#: whose roving cursor the AT was never told about, and a spread of ordinary
#: composites + atomics so the assertion is not tuned to the repaired cases.
WALKED = [
    "hello-tree-view",
    "hello-tree-grid",
    "hello-virtual-tree",
    "hello-data-grid",
    "hello-inspector",
    "hello-model-chart",
    "hello-tabbed-chart",
    "hello-property-grid",
    "hello-combobox",
    "hello-listbox",
    "hello-tabs",
    "hello-toolbar",
    "hello-dialog",
    "settings-panel",
]
STEPS = 4

#: The bindings R1518 repaired, and the stop whose roving cursor each one had
#: been keeping from the AT. `focus/next` reaches each of these stops.
REPAIRED = {
    "hello-data-grid": "data_grid",
    "hello-inspector": "inspector",
    "hello-model-chart": "x_field",
    "hello-tabbed-chart": "left_well",
}


def access(tf):
    return tf.request("scene/access").result


def tags_of(acc) -> set[str]:
    return {
        n["tag"]
        for n in acc.get("nodes") or ()
        if isinstance(n, dict) and isinstance(n.get("tag"), str)
    }


def bearer(acc) -> str | None:
    """The one tag the published `AccessFocus` says the AT reports as focused —
    the active descendant of a focused composite, else the focused element."""
    focus = acc.get("focus")
    if not focus:
        return None
    return focus.get("active_descendant") or focus.get("tag")


def check(tf, label: str) -> tuple[int, bool]:
    """Assert the claimed-focus set is exactly what the target names.

    Returns `(assertions_made, bearer_was_absent)`. The demo reports its own
    coverage rather than a docstring claiming a count nobody checks, and reports
    the absences rather than absorbing them — see the caller.
    """
    acc = access(tf)
    claimed = access_focus_flags(acc)
    want = bearer(acc)
    # A target can name a node this frame does not realize: a roving row
    # scrolled out of a windowed list, or — the case measured here — a focus
    # stop its binding never puts in the AT tree at all. Nothing bears the flag
    # then, and the AccessKit side likewise drops the dangling reference.
    absent = want is not None and want not in tags_of(acc)
    expected = set() if want is None or absent else {want}
    assert_eq(
        sorted(claimed),
        sorted(expected),
        f"{label}: the nodes claiming AT focus are exactly the target's bearer",
    )
    return 1, absent


def body() -> None:
    made = 0
    stops_walked = 0
    #: Stops whose focus target names a tag the AT tree does not contain. NOT
    #: what this round is about — the assembler cannot stamp a node that does
    #: not exist — but printing it keeps a real defect class visible instead of
    #: letting the "expect nothing" branch quietly absorb it. Measured 28 such
    #: observations over 8 bindings, two of which ship AT-invisible on purpose
    #: (`settings-panel` "AT-invisible for v1", `hello-window-focus-multi`'s
    #: empty `WidgetA11y` impl); the rest have a populated tree with a focus
    #: stop missing from it, where AccessKit collapses focus onto the window
    #: root and a screen-reader user hears nothing about the control.
    absent_bearers: list[str] = []

    # ── (A) + (B) every binding's focus ring ────────────────────────────────
    for example in WALKED:
        with RpcSubprocess(example, boot_grace=1.5) as tf:
            # (A) control — nothing focused at boot, so nothing may claim it.
            assert tf.request("focus/get").result.get("focused") is None, (
                f"{example}: nothing is focused at boot"
            )
            boot = access_focus_flags(access(tf))
            assert_eq(
                sorted(boot), [], f"{example}: no node claims focus at boot"
            )
            made += 2

            # (B) walk. A one-stop binding revisits its stop — still a real
            # sample of "the flag is where the target says".
            for step in range(STEPS):
                stop = tf.request("focus/next").result.get("focused")
                assert stop is not None, (
                    f"{example}: focus/next found no stop at step {step}; the "
                    f"assertions below would be vacuous"
                )
                made += 1
                asserted, absent = check(tf, f"{example} stop {step} ({stop})")
                made += asserted
                if absent:
                    absent_bearers.append(f"{example}:{stop}")
                stops_walked += 1

    # ── (C) the repaired bindings tell the AT which row ─────────────────────
    for example, stop in REPAIRED.items():
        with RpcSubprocess(example, boot_grace=1.5) as tf:
            reached = None
            for _ in range(STEPS):
                reached = tf.request("focus/next").result.get("focused")
                if reached == stop:
                    break
            assert_eq(reached, stop, f"{example}: the ring reaches {stop!r}")
            made += 1

            focus = access(tf).get("focus")
            assert focus is not None, f"{example}: {stop!r} focused publishes a target"
            assert_eq(focus.get("tag"), stop, f"{example}: AT focus rests on {stop!r}")
            child = focus.get("active_descendant")
            assert child, (
                f"{example}: {stop!r} is a roving composite, so the target must "
                f"name the row its cursor addresses — without it a screen reader "
                f"hears the container and never the cell (got {focus!r})"
            )
            made += 3

            # The named child is a real node, and it is the one bearing the flag.
            acc = access(tf)
            assert child in tags_of(acc), (
                f"{example}: the active descendant {child!r} is not a node in "
                f"this tree — a dangling aria-activedescendant"
            )
            assert_eq(
                sorted(access_focus_flags(acc)),
                [child],
                f"{example}: the wire flags the very row the AT was told about",
            )
            made += 2

    assert stops_walked == len(WALKED) * STEPS, f"{stops_walked} stops walked"
    assert made >= 30, f"only {made} assertions made"
    print(
        f"[demo] {made} assertions over {stops_walked} focus stops in "
        f"{len(WALKED)} bindings; {len(REPAIRED)} repaired composites now name "
        f"their active descendant"
    )
    if absent_bearers:
        print(
            f"[demo] separate axis, reported not asserted — {len(absent_bearers)} "
            f"stop(s) whose focus target names a tag absent from the AT tree, so "
            f"AccessKit collapses focus onto the window root: "
            f"{sorted(set(absent_bearers))}"
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1518 the AT focus target and the flag are one answer", body))
