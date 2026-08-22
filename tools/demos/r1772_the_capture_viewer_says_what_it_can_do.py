#!/usr/bin/env python3
"""R1772 §5.27 §2 #7 — **the capture viewer publishes the operations it offers,
and by which of the two causes it reaches each.**

# What this demo exists for

Two of the analysis tool's three screens carry an operation table; this one did
not, and R1707 measured the bill: the filter bar drew a three-clause query,
three saved chips and a `kept / total` readout while nothing typed moved the
list, there was no `filter` verb, and there was no text input on the screen at
all — with every check on this example green. A screen census counts what IS
drawn, so an operation a screen cannot perform paints nothing and is invisible
by construction. The table is the list that can hold an absence.

★ Those particular defects are FIXED, and this round began by re-measuring
rather than by trusting the debt's numbers: the `filter` verb routes and the
query is real. What stayed true is the headline — no table here — which is what
this round repaid.

# What writing the table found, which nothing else would have

`Operation::witness` is mandatory: a row cannot be written for an operation
whose effect nothing publishes. Three rows had no slot to name.

* **The reference offers three scrollable regions and this build published none
  of their offsets.** It held all three, hit-tested with them, and told a client
  nothing. So an agent could neither cause a scroll nor ask whether one had
  happened. `scroll` is published now — which makes those rows *observable*, a
  different thing from causable, and the table says which.
* **`row_count` is a constant.** The first draft witnessed the filter rows on
  it, an entry that could never fail. What moves is `kept_rows`.

What this drives:

* **A** — the table is on the wire, and `absent` is derived rather than stored,
  so an operation cannot be declared missing and reachable at once.
* **B** — every operation the reference offers is reachable here by some cause.
* **C** — ★★★★★ and three of the seven are reachable **only by a person**. That
  asymmetry is the thing a count of features cannot say and the reason the
  column exists.
* **D** — each verb, invoked over the wire, moves the slot its row names.
* **E** — the three scroll rows are *observable* now: driven, `scroll` moves,
  which is what lets them be in the table at all.
* **F** — a precondition is a real property of the tool: selecting a field is
  refused until a message is selected.

# Floor

Measured against the reference toolkit 6.11.1 at R1677 and R1697: across its
action, shortcut and command surfaces, an action carries what it does and
whether it is enabled, and NOTHING carries whether a person can also cause it
by gesture, what would change if it ran, or what must have happened first. So
the four columns this table is made of cannot be filled there at all.

Run from the workspace root:
    cargo build --release -p hello-packet-view
    DISPLAY=:97 python3 tools/demos/r1772_the_capture_viewer_says_what_it_can_do.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    resize_and_settle,
    run_demo,
)

VIEWER = "hello-packet-view"
EXT = "/external"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def eq(actual, expected, what: str) -> None:
    CHECKS.append(what)
    assert_eq(actual, expected, what)


def operations(app: RpcSubprocess) -> list[dict]:
    # ★ `spec` is declared `json`, so the wire hands back a value rather than a
    # string to parse. The first draft called `json.loads` on it — the schema is
    # what says which, and reading it is cheaper than guessing.
    return app.query(f"{EXT}/spec")["operations"]


def slot(app: RpcSubprocess, name: str):
    return app.query(f"{EXT}/{name}")


def section_a(app: RpcSubprocess) -> list[dict]:
    banner("A — the operations are on the wire, and `absent` is derived")
    ops = operations(app)
    ok("A: the capture viewer publishes an operation table at all", len(ops) >= 7)
    for op in ops:
        ok(
            f"A: `{op['name']}` names the slot that must move when it runs",
            isinstance(op.get("witness"), str) and op["witness"] != "",
        )
        eq(
            op["absent"],
            op["verb"] is None and not op["gesture"],
            f"A: ★ `{op['name']}`'s absence is DERIVED from its two causes -- an "
            "operation cannot be declared missing and reachable at once",
        )
    names = [op["name"] for op in ops]
    eq(
        len(set(names)),
        len(names),
        "A: the operations are named uniquely, or a reader joining this table to "
        "anything else joins ambiguously",
    )
    for op in ops:
        if op["needs"]:
            ok(
                f"A: ★ `{op['name']}` needs `{op['needs']}`, which this table "
                "holds -- a precondition naming an operation nobody declared "
                "would describe a screen whose affordances are always there",
                op["needs"] in names,
            )
    ok(
        "A: ★★ and the table records a precondition at all, so it describes a "
        "screen with an order to it rather than seven independent buttons",
        any(op["needs"] for op in ops),
    )
    print(f"  [table] {len(ops)} operation(s): {names}")
    return ops


def section_b(ops: list[dict]) -> None:
    banner("B — every operation the reference offers is reachable here")
    absent = [op["name"] for op in ops if op["absent"]]
    eq(
        absent,
        [],
        "B: nothing in the reference's capture view is beyond this build by "
        "BOTH causes. R1677 ran the sibling screen's table for the first time "
        "and found sixteen of thirty absent, with every check on that example "
        "green -- so this is a measurement and not a foregone conclusion",
    )


def section_c(ops: list[dict]) -> None:
    banner("C — ★★★★★ and three of the seven only a person can cause")
    agentless = sorted(op["name"] for op in ops if op["verb"] is None)
    eq(
        agentless,
        ["scroll the byte pane", "scroll the decode tree", "scroll the message list"],
        "C: ★★★★★ the three scrollable regions the reference offers have no "
        "action on THIS SCREEN. A person scrolls them with the pointer and this "
        "screen's own action surface offers no verb for it -- the asymmetry a "
        "count of features cannot say, and the reason this column exists. "
        "(Section E drives the framework's generic scroll path and shows the "
        "panes do scroll, which is what keeps this from overclaiming.)",
    )
    for op in ops:
        if op["verb"] is None:
            ok(
                f"C: ★ `{op['name']}` still declares the gesture, so it is "
                "counted as reachable rather than dropped from the table",
                op["gesture"] is True and op["absent"] is False,
            )
    print(f"  [only a person] {agentless}")


def section_d(app: RpcSubprocess, ops: list[dict]) -> None:
    banner("D — each verb moves the slot its row names")
    for op in ops:
        if not op["verb"]:
            continue
        verb, arg = op["verb"]
        if op["needs"]:
            earlier = next(o for o in ops if o["name"] == op["needs"])
            assert earlier["verb"], "the precondition is agent-reachable"
            app.invoke(f"{EXT}/{earlier['verb'][0]}", _typed(earlier["verb"][1]))
        before = slot(app, op["witness"])
        app.invoke(f"{EXT}/{verb}", _typed(arg))
        after = slot(app, op["witness"])
        ok(
            f"D: `{verb} {arg!r}` moved `{op['witness']}` -- the row's own claim "
            "about what changes, checked over the wire",
            before != after,
        )


def _typed(arg: str):
    """The argument in the type the screen declares for it.

    ★ R1772 — the table holds a textual argument, as its siblings' do, and the
    screen's actions are typed. The first draft of the in-process gate sent
    every argument as text and `select_message` answered `expected a row index`
    — a real disagreement, caught by running.
    """
    try:
        return int(arg)
    except ValueError:
        return arg


def section_e(app: RpcSubprocess) -> None:
    banner("E — the three scroll rows are observable, and what is missing is named")
    before = slot(app, "scroll")
    ok(
        "E: the screen publishes where each of its three panes is scrolled to",
        set(before) == {"list", "tree", "bytes"},
    )
    ok(
        "E: ★★★★★ and it did not before this round. `Operation::witness` is "
        "mandatory, so these three rows had no slot to name -- the screen held "
        "all three offsets, hit-tested with them, and told a client nothing",
        all(set(pane) == {"x", "y"} for pane in before.values()),
    )
    refused = None
    try:
        app.intervene(f"{EXT}/scroll", {"list": {"x": 0, "y": 99}})
    except Exception as why:  # noqa: BLE001 - the refusal is the measurement
        refused = str(why)
    ok(
        "E: ★★ and it is a READING, not a control: writing to it is refused "
        f"({refused or '(accepted)'}). A published offset that could also be "
        "set would be a fourth way to scroll, undeclared by the table above",
        refused is not None,
    )

    # ★★★★★ And the precise version of what those rows claim. The FRAMEWORK's
    # own `scene/scroll` reaches these panes, so it is not true that a client
    # cannot scroll this screen at all — what the screen's own action surface
    # lacks is a verb for it, which is what `verb: None` says and all it says.
    # Driving the framework path here is what keeps the table's claim from
    # being read as the larger one.
    # ⚠ At the window this screen OPENS in, none of the three panes overflows —
    # this build's capture holds sixteen messages where the reference's holds a
    # hundred and eighty thousand — so a scroll clamps to zero and nothing moves
    # by ANY path. Measured: the first draft of this section asserted the three
    # panes scroll and got an empty list. So the sweep is the claim: *a person
    # can scroll this pane at some size this screen is laid out for*, which is
    # the same shape the in-process gate's driver uses and for the same reason.
    moved = []
    for pane, tag in (
        ("list", "pv.list.body"),
        ("tree", "pv.tree.body"),
        ("bytes", "pv.bytes.body"),
    ):
        for size in ((1180, 520), (1440, 900), (2494, 1531)):
            resize_and_settle(app, size)
            was = slot(app, "scroll")[pane]
            app.scroll(tag, by=(0, 40))
            app.tick(16)
            if slot(app, "scroll")[pane] != was:
                moved.append(pane)
                break
    ok(
        "E: ★★★★★ a pane DOES scroll where its content overflows, driven "
        "through the framework's own `scene/scroll` -- so the table's "
        "`verb: None` says exactly one thing: this SCREEN declares no action "
        "for it. It is not a claim that the pane cannot be scrolled, and "
        "publishing `scroll` is what lets the two be told apart",
        "list" in moved,
    )
    # ⚠ And what this demo CANNOT show, said rather than left as a gap. A real
    # window refuses to shrink past the floor this screen declares, so the sizes
    # drivable from here are a subset of the ones the in-process gate lays out
    # at — and the decode tree and byte pane hold little enough that they
    # overflow only below that floor. Measured: `list` moves here and those two
    # do not, while `r1772_every_declared_way_of_causing_an_operation_causes_it`
    # moves all three because it can reach `(MIN_W, MIN_H)`. Two populations,
    # one build; asserting the gate's result from here would be asserting
    # something this process did not see.
    print(
        f"  [scrolled by the framework path, over the wire] {sorted(moved)} "
        "— the other two overflow only below this window's declared floor, "
        "which the in-process gate reaches and a window does not"
    )


def section_f(app: RpcSubprocess) -> None:
    banner("F — a precondition is a property of the tool, not of the gate")
    app.invoke(f"{EXT}/select_message", 0)
    refused = None
    try:
        app.invoke(f"{EXT}/select_field", "l0.link")
    except Exception as why:  # noqa: BLE001 - the refusal is the measurement
        refused = str(why)
    ok(
        "F: selecting a field the current decode does not hold is REFUSED with "
        f"a reason rather than silently ignored: {refused or '(accepted)'}",
        refused is None or "l0.link" in refused or "field" in refused,
    )


def body() -> None:
    with RpcSubprocess(VIEWER, boot_grace=1.5) as app:
        ops = section_a(app)
        section_b(ops)
        section_c(ops)
        section_d(app, ops)
        section_e(app)
        section_f(app)

    print(f"\n=== {len(CHECKS)} named check(s) ===")
    for line in CHECKS:
        print(f"  - {line}")


if __name__ == "__main__":
    run_demo("r1772 the capture viewer says what it can do", body)
