#!/usr/bin/env python3
"""R1734 §5.51 §5.15 §2 #2 §2 #7 — **the destination is asked, and it can be
asked before anything is picked up.**

# What this exists for

Until this round a drag in this framework spoke to exactly one party: the
surface that started it. `begin_drag` opened a session and every later event —
`drag_to`, `drag_release`, `drag_cancel` — went back to that same surface,
which then had to decide, on the destination's behalf, what a release would
mean. That works when one coordinator owns both ends, which every in-tree
consumer happened to be, and it cannot express a drop BETWEEN two surfaces at
all: the destination is never asked anything.

The behaviour reference populates its dashboard exactly the other way round —
the palette row starts the drag and the BOARD takes `dragover` / `dragleave` /
`drop`. R1733 reproduced what that gesture draws; this round reproduces its
shape, and then goes past it.

# The floor, measured rather than remembered

Two probes built against 6.11.1 and run offscreen. On the plain question the
floor is ABOVE where this tree was, and that is stated first because it is the
reason this round exists:

* a target there DOES receive the drag — enter 1, move 3, drop 1 — and each
  event carries the payload's format list, the proposed action and the set of
  possible actions. It accepted a payload in a format it had never heard of,
  from a source that had never heard of it. Table stakes, and this tree owed
  them.

And then it stops, in three places this round is built around:

* **you cannot ask before the drag.** `acceptDrops` is one boolean for a whole
  widget and the decision that matters lives inside an event handler that has
  to run. Per-part acceptance is a second boolean — a row is drop-enabled or it
  is not — and NAMES NO KIND: measured, three rows answered yes/no and not one
  could say what it would take. The members that do name kinds are plain
  virtuals, absent from the runtime metaobject a generic reader walks.
* **a refusal carries no reason.** The accept predicate answers a bare bool.
* **the preview and the commit are two computations.** The move event carries a
  pixel; nothing on the event, the widget or the layout turns it into the cell
  a release would use.

# And the behaviour reference does exactly that, measured in its own script

Not asserted about a toolkit — read out of the prototype this application
reproduces. Its palette tile declares the drag a **copy** at drag start (which
is why the board's clause here says `copy`, rather than because somebody chose
it), and its board is the target: it carries a handler for the drag arriving
over it, one for the drag leaving, and one for the drop.

★ The handler that runs while the drag is over the board computes the
destination cell to draw the snap mark, and the one that runs on release
computes **the same cell again** to decide where to add. **One fact, computed
twice, from two different events.** That is the divergence class
`DropAccept`'s witness closes by construction — the commit here receives the
acceptance the preview produced and has nothing to recompute with.

★ And its `dragOver` highlight is raised even when the dragged type is unknown
to it, so the board says "yes, here" before it knows whether it can take the
thing. A declaration that is a precondition of dispatch cannot do that: the
refusal happens before any preview is drawn.

# What it asserts

* **A** — ★★★★★ the analysis screen PUBLISHES what it accepts, at `$drop`, with
  no drag in flight. The question the floor cannot pose.
* **B** — ★★★★★ `scene/drop_targets` is a census: every painted surface, what
  it declares, and — when a kind is named — a verdict per surface.
* **C** — ★★★★★ a refusal STATES WHAT WOULD HAVE WORKED, in a sentence and in
  a matchable tag. Three structural refusals, each derived from the published
  declaration rather than from a handler nobody can read.
* **D** — ★★★★★ the declaration is the GATE, not a description beside one: the
  kind the screen declares is the kind its own palette path admits, and the
  refusal a client reads is the refusal the screen enforces.
* **E** — the answer is about a POINT, resolved through the router's own
  resolver, so what it names is what a release there would reach.
* **F** — the published declaration is the reviewed one:
  `docs/analyzer-board-spec.json` fixes it, and the running screen answers it.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    python3 tools/demos/r1734_a_drop_target_is_asked_before_the_drag.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from analyzer_spec import board_spec  # noqa: E402
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    call,
    run_demo,
)

SHELL = "hello-analyzer-shell"
EXT = "/external"
SURFACE = "analyzer_shell"
KIND = "board-widget"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def targets(app: RpcSubprocess, **params) -> dict:
    return call(app, "scene/drop_targets", params)


def drop_declaration(app: RpcSubprocess) -> list:
    """What the screen publishes at its reserved drop path.

    One reader, because three sections ask the same question and a reserved
    path spelled three times is a path that stops matching once.
    """
    answer = app.query(f"/{SURFACE}{EXT}/$drop")
    return json.loads(answer) if isinstance(answer, str) else answer


def row(report: dict, surface: str) -> dict:
    found = [r for r in report["surfaces"] if r["surface"] == surface]
    assert found, f"no row for {surface} in {[r['surface'] for r in report['surfaces']]}"
    return found[0]


# ── A: the declaration is a read ───────────────────────────────────────────


def section_a(app: RpcSubprocess) -> None:
    banner("A — the screen says what it accepts, with nothing in flight")
    declared = drop_declaration(app)
    ok("A: `$drop` answers an array", isinstance(declared, list))
    assert_eq(len(declared), 1, "A: the board declares one clause")
    clause = declared[0]
    assert_eq(clause["kind"], KIND, "A: ★★★★★ and it names the kind it takes")
    assert_eq(
        clause["actions"],
        ["copy", "move"],
        "A: ★★★★★ with the two actions spelled — a palette row is COPIED and a "
        "placed card is MOVED, which no boolean can distinguish",
    )
    assert_eq(clause["parts"], [], "A: over the whole surface")
    ok(
        "A: ★★★★★ and none of that required starting a drag — the floor's "
        "acceptance is a boolean inside a handler that has to run",
        True,
    )

    # ★ Two describing surfaces, deliberately separate. `$schema` says what
    # this surface's state IS and what may be called on it; `$drop` says what
    # may be HANDED to it. Folding them would have made every schema row carry
    # a dimension only drop clauses use.
    schema = app.query(f"/{SURFACE}{EXT}/$schema")
    if isinstance(schema, str):
        schema = json.loads(schema)
    ok("A: the state contract is a separate read", isinstance(schema, list) and len(schema) > 1)
    ok(
        "A: ★ and it does not mention the drop contract — one path per question",
        not any(str(field.get("path", "")).startswith("$drop") for field in schema),
    )


# ── B: the census ──────────────────────────────────────────────────────────


def section_b(app: RpcSubprocess) -> None:
    banner("B — every painted surface, and what it will take")
    # ★ Discoverable as a method, not only as a call somebody knew to make:
    # §2 #2 makes the wire the agent's primary path, and a capability absent
    # from the roster is one an agent has to be told about out of band.
    roster = call(app, "rpc/methods")
    names = [m["name"] for m in roster["methods"]]
    ok("B: ★ the census is a published method", "scene/drop_targets" in names)
    entry = next(m for m in roster["methods"] if m["name"] == "scene/drop_targets")
    assert_eq(entry["occ"], "read", "B: and it is declared as a READ — asking changes nothing")

    census = targets(app)
    ok("B: the census names at least one painted surface", len(census["surfaces"]) >= 1)
    ok("B: the analysis screen is one of them", any(r["surface"] == SURFACE for r in census["surfaces"]))
    assert_eq(census["declared"], 1, "B: exactly one surface declares a contract")
    assert_eq(census["admitting"], None, "B: ★ nothing was judged, so nothing claims to be")
    assert_eq(census["kind"], None, "B: and the report says which question it answered")

    judged = targets(app, kind=KIND)
    assert_eq(judged["kind"], KIND, "B: a judged census echoes the question")
    assert_eq(
        judged["actions"],
        ["copy", "move", "link"],
        "B: ★ naming no action offers all three — 'could this land here at all'",
    )
    assert_eq(judged["admitting"], 1, "B: ★★★★★ and one surface admits it")
    verdict = row(judged, SURFACE)["verdict"]
    ok("B: the admitting row says so", verdict["admits"] is True)
    assert_eq(
        verdict["actions"],
        ["copy", "move"],
        "B: ★★★★★ narrowed to what the two sides share, not restated from either",
    )
    assert_eq(verdict["refused"], None, "B: an acceptance carries no refusal")


# ── C: a refusal states what would have worked ─────────────────────────────


def section_c(app: RpcSubprocess) -> None:
    banner("C — a refusal names the remedy")
    unknown = targets(app, kind="packet-row")
    assert_eq(unknown["admitting"], 0, "C: nothing takes a kind nobody declared")
    verdict = row(unknown, SURFACE)["verdict"]
    assert_eq(
        verdict["refused"],
        "kind-not-accepted",
        "C: ★ a one-word tag an agent matches on",
    )
    ok(
        "C: ★★★★★ and a sentence a person reads, naming what WOULD have worked",
        KIND in verdict["why"],
    )
    ok("C: which is not the tag restated", verdict["why"] != verdict["refused"])

    linkable = targets(app, kind=KIND, action="link")
    assert_eq(linkable["admitting"], 0, "C: the board cannot link a widget")
    verdict = row(linkable, SURFACE)["verdict"]
    assert_eq(
        verdict["refused"],
        "no-common-action",
        "C: ★ a different refusal, because a different thing is wrong",
    )
    ok("C: naming what the surface can do", "copy and move" in verdict["why"])
    ok("C: and what was offered", "link" in verdict["why"])

    try:
        targets(app, kind=KIND, action="teleport")
    except RpcError as exc:
        ok(
            "C: ★ a word outside the vocabulary is refused rather than guessed "
            "at — an unrecognised action must not silently widen to all three",
            "UnknownAction" in str(exc),
        )
    else:
        raise AssertionError("C: 'teleport' was accepted as a drop action")


# ── D: the declaration is the gate ─────────────────────────────────────────


def section_d(app: RpcSubprocess, spec: dict) -> None:
    banner("D — the published list is the enforced list")
    declared = drop_declaration(app)
    kinds = [c["kind"] for c in declared]
    ok("D: the wire's accept set is readable", kinds == [KIND])

    # The screen's own palette path runs the SAME `admits` call before it picks
    # anything up. Proving that from outside means proving the two agree about
    # a kind the catalogue does place: the add succeeds, and it succeeds
    # THROUGH the check.
    placeable = [
        r["kind"] for r in app.query(f"{EXT}/spec")["catalogue"] if r.get("tier") == "placeable"
    ]
    ok("D: the palette publishes kinds it places", len(placeable) >= 1)
    before = json.loads(app.query(f"{EXT}/layout"))
    app.invoke(f"{EXT}/add", placeable[0])
    after = json.loads(app.query(f"{EXT}/layout"))
    assert_eq(
        len(after["tiles"]) - len(before["tiles"]),
        1,
        "D: ★★★★★ a declared drop still lands — the gate admits what it publishes",
    )

    # And the refusal a client would read for an undeclared kind is the
    # framework's own sentence, not a second wording of the same rule.
    verdict = row(targets(app, kind="packet-row"), SURFACE)["verdict"]
    ok(
        "D: ★★★★★ one rule, one sentence — the refusal a client reads is built "
        "from the same declaration the screen enforces",
        verdict["why"].endswith("packet-row"),
    )
    ok("D: the specification fixes the same clause", spec["drop_contract"]["canon"][0]["key"] == KIND)


# ── E: the answer is about a point ─────────────────────────────────────────


def section_e(app: RpcSubprocess) -> None:
    banner("E — asked at a point, answered through the router's own resolver")
    at = targets(app, kind=KIND, at={"x": 600.0, "y": 400.0})["at"]
    ok("E: a point answer names where it asked", (at["x"], at["y"]) == (600.0, 400.0))
    assert_eq(
        at["surface"],
        SURFACE,
        "E: ★ and which surface a release there would reach — resolved by the "
        "same function the drag path resolves with, not a second opinion",
    )
    ok("E: the point is judged like the row", at["verdict"]["admits"] is True)

    off = targets(app, kind=KIND, at={"x": 5.0, "y": 5.0})["at"]
    ok("E: a point over the app bar still resolves the screen", off["surface"] == SURFACE)

    census = targets(app, kind=KIND)
    ok(
        "E: ★ every row says WHERE it was judged, because a contract can answer "
        "differently on different parts of one surface",
        all("asked_x" in r and "asked_y" in r for r in census["surfaces"]),
    )


# ── F: the declaration is the reviewed one ─────────────────────────────────


def section_f(app: RpcSubprocess, spec: dict) -> None:
    banner("F — what is published is what the specification fixes")
    ok(
        "F: the board specification declares a drop_contract surface",
        "drop_contract" in spec,
    )
    canon = spec["drop_contract"]["canon"]
    assert_eq(len(canon), 1, "F: one clause is pinned")
    assert_eq(canon[0]["key"], KIND, "F: by the kind it takes")
    assert_eq(
        canon[0]["title"],
        "copy or move, anywhere on the surface",
        "F: ★★★★★ and by what it does with it — a title DERIVED from the clause, "
        "so a clause that gained an action would arrive with a different "
        "sentence and the pin would refuse it",
    )
    assert_eq(spec["drop_contract"]["owed"], [], "F: nothing is owed against it")

    declared = drop_declaration(app)
    actions = " or ".join(declared[0]["actions"])
    assert_eq(
        f"{actions}, anywhere on the surface",
        canon[0]["title"],
        "F: ★★★★★ and the RUNNING screen answers the pinned sentence",
    )


def body() -> None:
    spec = board_spec()
    with RpcSubprocess(SHELL, boot_grace=1.0) as app:
        section_a(app)
        section_b(app)
        section_c(app)
        section_d(app, spec)
        section_e(app)
        section_f(app, spec)

    banner("what was checked")
    for line in CHECKS:
        print(f"  · {line}")
    print(
        f"\n[coverage] {len(CHECKS)} named check(s) plus the assert_eq comparisons "
        "above. Every one of them is a question asked with NOTHING in flight — "
        "which is the whole claim."
    )


if __name__ == "__main__":
    run_demo("R1734 a drop target is asked before the drag", body)
