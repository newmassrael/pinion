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

R1588 — the population is DERIVED (`tools/binding_focus.py`), and so is the
build line, because a hand-written `cargo build -p …` is the same curated list
in another costume: it drifts the moment the population grows, and then a run
measures whichever binaries are lying in `target/release`. Measured while
widening this sweep, every example binary in the tree predated the `pinion-rpc`
change the new assertion reads, except the handful rebuilt since — so the first
run compared binaries from two commits and reported a field as missing that the
serializer always emits.

Run from the workspace root:
    python3 tools/binding_focus.py      # prints the population and its build line
    eval "$(python3 tools/binding_focus.py | tail -1)"
    python3 tools/demos/r1518_at_focus_one_answer.py

Cost, measured at R1588 on one box against the sweep's 180s per-demo budget:
29s fully warm and 138s cold, the spread being page cache over ~2.3 GB of
freshly linked binaries. A focus MOVE is the expensive thing (~1s, the pre-existing R1561 stall in
front of the next RPC), so the walk is bounded twice — the ring closes rather
than running to a fixed count, and how far a binding is walked follows what it
declares. Fourteen hand-picked composites became ninety-two derived ones for
roughly five times the assertions.

>= 30 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from binding_focus import interactive_bindings  # noqa: E402
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    access_focus_flags,
    assert_eq,
    run_demo,
)

#: R1588 — the population is DERIVED, not written down.
#:
#: This list used to be fourteen composite names chosen by hand, and a curated
#: population cannot find an absence: a binding with no focus stop is simply
#: never walked, which is how `debt-interactive-role-without-focus-stop` stayed
#: invisible in seventeen bindings for some fifty rounds. R1570.1 did not fix
#: this list — it added a second sweep with a derived population — so the tree
#: carried one curated sweep and one derived one, asking different questions of
#: different sets. `tools/binding_focus.py` is the merge, and its module docs
#: carry what a source scan can and cannot see.
POPULATION = interactive_bindings()
WALKED = POPULATION.walkable

#: The bindings whose widget attribute names the tag that carries their role.
#: These are walked to the cap; the rest are observed once. See the walk.
DECLARED_TAGS = {d.name for d in POPULATION.declared}

#: R1588 — no fixed sleep in front of the readiness handshake, the decision
#: R1570.1 measured: `RpcSubprocess` polls `scene/cache_stats` until the first
#: windowed paint, so `boot_grace` only widens the window in which an instant
#: crash reads as a boot failure. At 1.5s it was 2.06s per boot and 1.00s at
#: zero — and this sweep grew from 14 bindings to 92, where that
#: padding alone would be most of the 180s budget.
BOOT_GRACE = 0.0

#: The most focus moves any binding is walked. A CAP, not a count: R1588 walks
#: the ring until it closes and stops there.
#:
#: A fixed four was affordable over fourteen hand-picked composites and is not
#: over ninety-two, because a focus move is the one thing in this walk that
#: costs real time — the ~1s stall a mutating call leaves in front of the next
#: RPC (pre-existing, R1561). Measured at R1588: a fixed four took 310s against
#: the sweep's 180s per-demo budget, and most of the widened population is
#: single-control bindings whose ring closes on the second move.
#:
#: Walking the ring is also the more honest sample. The old comment conceded
#: that "a one-stop binding revisits its stop — still a real observation", which
#: is true and is three observations of one stop dressed as four of a ring.
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


def check(tf, label: str) -> tuple[int, str | None]:
    """Assert the claimed-focus set is exactly what the target names.

    Returns `(assertions_made, why_the_bearer_was_absent)`. The demo reports its
    own coverage rather than a docstring claiming a count nobody checks, and
    reports the absences rather than absorbing them — see the caller.

    R1588 — widening the population from fourteen hand-picked composites to the
    derived ninety-two brought in the multi-window bindings, and with them a
    distinction this axis could not previously make. A target naming a tag the
    tree does not contain has two causes and only one of them is a defect:

      * the bearer is in **another window**. `scene/access` answers about one
        window, and `AccessTreeBuilder` folds a focus tag it does not hold onto
        that window's root — so the target is honest and the tag is simply
        elsewhere. R1583 made that visible by publishing `resolved`, and this
        is the first gate to read it.
      * the bearer is **missing outright** — a focus stop its binding never puts
        in the AT tree, where AccessKit collapses focus onto the window root and
        a screen-reader user hears nothing about the control.

    Before `resolved` existed the two were one bucket, so widening the
    population would have reported four multi-window observations as the defect
    they are not.
    """
    acc = access(tf)
    claimed = access_focus_flags(acc)
    focus = acc.get("focus") or {}
    want = bearer(acc)
    resolved = focus.get("resolved")
    absent = want is not None and want not in tags_of(acc)
    why = None
    if absent:
        why = "other-window" if resolved == "window_root" else "missing"
    expected = set() if want is None or absent else {want}
    assert_eq(
        sorted(claimed),
        sorted(expected),
        f"{label}: the nodes claiming AT focus are exactly the target's bearer",
    )
    # A fold is a STATEMENT, not an absence of one: whenever the tree folded
    # onto the window root it must say so, and whenever it did not it must name
    # the tag it resolved. R1583 published both words; this asserts the wire
    # never leaves the question unanswered.
    if want is not None:
        assert resolved in ("tag", "window_root"), (
            f"{label}: the focus target must say how it resolved, got "
            f"{resolved!r}"
        )
        return 2, why
    return 1, why


#: The floor this sweep's population may not silently fall below.
#:
#: R1588 — a counterfactual replacing `WALKED` with a two-name list PASSED, in
#: seven seconds. Removing the curated list is not the same as removing the
#: ability to have one, and the assertion below is the difference: this sweep
#: now states that its population IS the derivation, and that the derivation is
#: still wide. Not an equality against a number, which would need editing every
#: time a binding is added — a FLOOR, which only a narrowing can cross.
MIN_POPULATION = 60


def body() -> None:
    made = 0
    stops_walked = 0
    #: Bindings whose focus ring came back to its first stop within the cap —
    #: reported so "the walk was short" is a fact rather than an assumption.
    rings_closed = 0
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
    #: Stops whose bearer is in ANOTHER window — not a defect, and separated
    #: from the list above only because R1583 published `resolved`.
    elsewhere: list[str] = []

    # ── the population is the DERIVATION, and it is still wide ─────────────
    assert_eq(
        WALKED,
        interactive_bindings().walkable,
        "the sweep walks exactly the derived population — a hand-written list "
        "here is the defect R1588 removed, and this is what stops it coming back",
    )
    assert len(WALKED) >= MIN_POPULATION, (
        f"the derived population is {len(WALKED)}, below the {MIN_POPULATION} "
        f"floor — a scan that has quietly stopped matching reads as a tree with "
        f"fewer bindings, which is how a curated list fails without saying so"
    )
    print(f"[demo] population: {POPULATION.summary()}")
    made += 2

    # ── (A) + (B) every binding's focus ring ────────────────────────────────
    for example in WALKED:
        with RpcSubprocess(example, boot_grace=BOOT_GRACE) as tf:
            # (A) control — nothing focused at boot, so nothing may claim it.
            assert tf.request("focus/get").result.get("focused") is None, (
                f"{example}: nothing is focused at boot"
            )
            boot = access_focus_flags(access(tf))
            assert_eq(
                sorted(boot), [], f"{example}: no node claims focus at boot"
            )
            made += 2

            # (B) walk the ring until it CLOSES — back to the stop it started
            # on — or until the cap. Every stop is sampled exactly once, so a
            # single-control binding is one observation rather than four of the
            # same one, and the assertions below are about the ring rather than
            # about a fixed number of moves.
            # R1588 — how far to walk follows what the binding DECLARES, which
            # is the same derived distinction R1570.1 makes: a binding that
            # names its `tag` can be asked a pointed question about that tag, a
            # hand-written `WidgetA11y` can only be asked about the window. So
            # every binding in the population is observed — none is invisible,
            # which is the property a curated list could not have — and the ones
            # that said enough to be walked are walked.
            #
            # Depth, not membership. Measured: a focus move costs ~1s (the
            # pre-existing R1561 stall in front of the next RPC), so walking all
            # 92 to the cap took 310s against a 180s budget, and walking rings
            # took 224s. This is the one axis left that trades nothing for it.
            cap = STEPS if example in DECLARED_TAGS else 1
            first = None
            for step in range(cap):
                stop = tf.request("focus/next").result.get("focused")
                assert stop is not None, (
                    f"{example}: focus/next found no stop at step {step}; the "
                    f"assertions below would be vacuous"
                )
                made += 1
                if first is None:
                    first = stop
                elif stop == first:
                    # The ring closed. Asserting again here would re-observe a
                    # stop already checked, which is where the old fixed four
                    # spent its time.
                    rings_closed += 1
                    break
                asserted, why = check(tf, f"{example} stop {step} ({stop})")
                made += asserted
                if why == "missing":
                    absent_bearers.append(f"{example}:{stop}")
                elif why == "other-window":
                    elsewhere.append(f"{example}:{stop}")
                stops_walked += 1

    # ── (C) the repaired bindings tell the AT which row ─────────────────────
    for example, stop in REPAIRED.items():
        with RpcSubprocess(example, boot_grace=BOOT_GRACE) as tf:
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

    # Every binding contributes at least one observed stop, and no binding
    # contributes more than the cap. A ring that never closed is not a failure
    # (it has more stops than the cap), but a population that produced FEWER
    # observations than bindings would mean a walk silently did nothing.
    assert len(WALKED) <= stops_walked <= len(WALKED) * STEPS, (
        f"{stops_walked} stops observed across {len(WALKED)} bindings"
    )
    assert made >= 30, f"only {made} assertions made"
    print(
        f"[demo] {made} assertions over {stops_walked} focus stops in "
        f"{len(WALKED)} bindings ({rings_closed} rings closed inside the "
        f"{STEPS}-move cap); {len(REPAIRED)} repaired composites now name "
        f"their active descendant"
    )
    if elsewhere:
        print(
            f"[demo] not a defect — {len(elsewhere)} stop(s) whose bearer is in "
            f"ANOTHER window, so this window's tree folds focus onto its root "
            f"and says so (`resolved: window_root`, R1583): "
            f"{sorted(set(elsewhere))}"
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
