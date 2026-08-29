#!/usr/bin/env python3
"""R1891 — a detached card is in ONE place, and the application says which.

# What this demo exists for

Standing rule (7) of this repayment loop asks for the analyzer UI assembled —
one shell, its sections mounted, asserted by one walk. This drives the board's
tear-off, which the behaviour canon has (`detachWidget` / `redock`, over
`state.widgets` and `state.floats`) and which this tree gave a real OS window at
R1826.

# The fork this closes, measured before the round

Driving the assembled tool at R1891, tearing ONE card off produced:

    windows: ['main', 'torn-packet#0']
    float.* in the MAIN window: ['float.packet#0', 'float.packet#0.badge',
                                 'float.packet#0.close', 'float.packet#0.redock',
                                 'float.packet#0.resize']

Two pictures of one card, from two models that did not track each other — a
window topology keyed on which windows exist, and a float carrying live
geometry. The debt registered at R1826 said which of the two a person
manipulates was an unmade design decision.

# What was decided, and why not "always a window"

§2 #6 makes GUI and TUI one scene over two dispatch paths, and a terminal
backend has no window server. Deleting the canvas form would make tear-off a
gesture that silently does nothing on half of this framework's surfaces — and
the behaviour canon itself uses the canvas form, being a web page that cannot
open a window. So the choice is kept and a card carries WHICH:
`pinion_core::detach::DetachHome`, one value, no representation for "both".

# What this walk holds

That the two pictures are two disjoint sets in the running application: a
window-homed card has a window and paints nothing on the canvas, a canvas-homed
one paints a panel and has no window, moving between them moves BOTH facts at
once, and the host publishes which homes it can offer before anyone asks for
one.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1891_a_detached_card_is_in_one_place.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
)

SHELL = "hello-analyzer-shell"
EXT = "/external"
CARD = "packet#0"
TORN = f"torn-{CARD}"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def windows(app: RpcSubprocess) -> list[str]:
    return [w["id"] for w in app.request("scene/windows", {}).result["windows"]]


def invoke(app: RpcSubprocess, verb: str, args: str):
    return app.invoke(f"{EXT}/{verb}", args)


def settle(app: RpcSubprocess) -> None:
    """Let the topology Effect run and the shell reconcile its windows."""
    for _ in range(6):
        app.tick_ms(16)


def canvas_marks(app: RpcSubprocess) -> list[str]:
    """Everything the MAIN window paints for a detached panel."""
    return sorted(
        t for t in abs_rects_of(app.snapshot(source="paint")) if t.startswith("float.")
    )


def homes(app: RpcSubprocess) -> dict[str, str]:
    return {f["id"]: f["home"] for f in app.query(f"{EXT}/floats")}


def section_a(app: RpcSubprocess) -> None:
    banner("A — the host publishes the homes it can offer, before anyone asks")
    policy = app.query(f"{EXT}/detach_policy")
    ok(
        f"A: ★★ the host declares which homes it has — {policy}",
        isinstance(policy, dict) and policy.get("homes"),
    )
    ok(
        "A: and the home it picks by default is one it offers, so the two "
        "cannot disagree",
        policy["preferred"] in policy["homes"],
    )
    ok(
        "A: ★ this build opens windows, so it offers both — the canvas form is "
        "not a fallback that disappears where it is not needed",
        set(policy["homes"]) == {"window", "canvas"},
    )
    # The population floor: nothing is detached yet, so every later assertion
    # about a detached card is about something this walk actually made.
    assert_eq(app.query(f"{EXT}/floats"), [], "A: nothing is detached at boot")
    assert_eq(windows(app), ["main"], "A: and the application opens one window")


def section_b(app: RpcSubprocess) -> None:
    banner("B — a torn-off card takes the preferred home, and is there ONLY")
    invoke(app, "act", f"{CARD},tear_off")
    settle(app)

    assert_eq(homes(app), {CARD: "window"}, "B: the card took the window home")
    ok(
        f"B: ★★ a window opened for it — {windows(app)}",
        windows(app) == ["main", TORN],
    )
    # ★★★★★ The claim of the round. Before it, this list had five entries.
    marks = canvas_marks(app)
    ok(
        f"B: ★★★★★ and the canvas paints NOTHING for it — {len(marks)} "
        f"`float.*` region(s) in the main window, was five",
        marks == [],
    )
    ok(
        "B: ★ the published correspondence names its window, because a "
        "window-homed card is the only kind that has one",
        app.query(f"{EXT}/detached") == [{"card": CARD, "window": TORN}],
    )


def section_c(app: RpcSubprocess) -> None:
    banner("C — moving it to the canvas moves BOTH facts, in one call")
    invoke(app, "detach_home", f"{CARD},canvas")
    settle(app)

    assert_eq(homes(app), {CARD: "canvas"}, "C: the card took the canvas home")
    ok(
        f"C: ★★★★★ the window is GONE — {windows(app)}",
        windows(app) == ["main"],
    )
    marks = canvas_marks(app)
    ok(
        f"C: ★★ and the canvas now paints the panel — {len(marks)} region(s), "
        f"including its grip and its re-dock",
        f"float.{CARD}" in marks
        and f"float.{CARD}.resize" in marks
        and f"float.{CARD}.redock" in marks,
    )
    ok(
        "C: ★★★★★ and `detached` is EMPTY, because a canvas-homed panel has no "
        "window to name — a correspondence to a window that does not exist is "
        "worse than the silence",
        app.query(f"{EXT}/detached") == [],
    )
    ok(
        "C: ★ the card is still detached either way — the board does not have "
        "it back",
        CARD not in [t.split(".")[1] for t in abs_rects_of(app.snapshot(source="paint")) if t.startswith("card.") and t.count(".") == 1],
    )


def section_d(app: RpcSubprocess) -> None:
    banner("D — the canvas panel is a panel a hand can drive, which is why it stays")
    before = app.query(f"{EXT}/floats")[0]
    grip = abs_rects_of(app.snapshot(source="paint"))[f"float.{CARD}.resize"]
    at = (grip[0] + grip[2] // 2, grip[1] + grip[3] // 2)
    app.drag(from_at=at, to_at=(at[0] + 60, at[1] + 40))
    app.tick_ms(16)
    after = app.query(f"{EXT}/floats")[0]
    ok(
        f"D: ★★ a drag on its corner sizes it — {before['w']}x{before['h']} -> "
        f"{after['w']}x{after['h']}",
        (after["w"], after["h"]) != (before["w"], before["h"]),
    )
    ok(
        "D: ★ and it is still on the canvas, because sizing a panel is not a "
        "decision about where it lives",
        after["home"] == "canvas",
    )


def section_e(app: RpcSubprocess) -> None:
    banner("E — a home this host cannot offer is refused, and the refusal names one it can")
    try:
        answered = invoke(app, "detach_home", f"{CARD},elsewhere")
    except RpcError as refusal:
        said = str(refusal)
        ok(
            f"E: ★★★★★ an unknown home is REFUSED and the refusal names the "
            f"ones that exist ({said})",
            "window" in said and "canvas" in said,
        )
    else:
        ok(f"E: an unknown home must be refused, not answered {answered!r}", False)

    # And a card that is not detached has no home to move — a different refusal,
    # named, so the two cannot be confused for one another.
    try:
        answered = invoke(app, "detach_home", "decode#1,canvas")
    except RpcError as refusal:
        said = str(refusal)
        ok(
            f"E: ★★ a card still on the board is refused for a DIFFERENT stated "
            f"reason ({said})",
            "not detached" in said,
        )
    else:
        ok(f"E: a card on the board has no home to move, got {answered!r}", False)

    # ★ And re-docking ends it from the canvas home too, so the round did not
    # strand a panel in a home nothing can leave.
    invoke(app, "redock", CARD)
    settle(app)
    ok(
        "E: ★★ re-docking from the canvas puts the card back on the board and "
        "leaves no panel behind",
        app.query(f"{EXT}/floats") == [] and canvas_marks(app) == [],
    )


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        section_a(app)
        section_b(app)
        section_c(app)
        section_d(app)
        section_e(app)

    print(f"\n=== {len(CHECKS)} named check(s) ===")
    for line in CHECKS:
        print(f"  - {line}")


if __name__ == "__main__":
    run_demo("r1891 a detached card is in one place", body)
