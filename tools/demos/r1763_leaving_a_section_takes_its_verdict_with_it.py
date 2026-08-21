#!/usr/bin/env python3
"""R1763 §5.27 §5.40 §2 #7 — **leaving a section takes its painted marks with
it, so what an application claims to have reproduced is what a reader can see.**

# What this demo exists for

Leaving a screen already took its externals, its windows and its accessibility
tree with it — that is `pinion_screen`'s central rule, *the screen the journey
is at is the only one anything reaches*. Its **marks** were the one thing it
left behind, and every verdict in this tree is read from those.

Measured on this application at R1763, before the repair, by walking every
section once and returning to the first:

```text
packets  showing=false  25 of 26  away=0  reconciles=true
keys     showing=false  21 of 21  away=0  reconciles=true
logs     showing=false  15 of 15  away=0  reconciles=true
headline                88 of 133 reproduced
```

Three sections reporting a reproduced specification about frames nobody could
see, and a headline built out of them. R1742 published `showing` beside every
row so a reader could TELL — the honest half. This is the other half.

⚠ It is not only a stale number. `ApplicationConformance::conforms` is
`unjudged == 0 && declared == 0 && every judged report reconciles`, and R1762
brought the first two to zero — so without this, an application could report
conformance earned entirely by frames that had left it.

What this drives:

* **A** — at boot, every section but the one showing is away and the headline is
  the showing section's.
* **B** — walk every section once and come back. The headline is **the same
  number**, because the sections walked through gave their marks back when the
  reader left them.
* **C** — and it is not amnesia: standing in a section, its surfaces are on the
  frame and its verdict is full. The rule is *leaving*, not *forgetting*.
* **D** — the two pages the host paints itself were never affected and still are
  not: their judge is told where the reader is
  (`pinion_screen::Showing`), which is a different mechanism reaching the same
  honesty. Both are checked, because a repair that fixed one and broke the other
  would look like this one from the headline alone.

# Floor

Measured against the reference toolkit 6.11.1 at R1738 and R1758: nothing there
names a verdict, a specification or evidence, so the question *is this verdict
about the frame in front of me* cannot be asked of it at all. The narrower
question this round asks — does leaving a page discard what it painted — has an
answer there and it is the wrong one: a page removed from its container keeps
its rendered surface until the container is destroyed, which is why a screenshot
of a page nobody has opened comes back populated (R1758's probe).

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1763_leaving_a_section_takes_its_verdict_with_it.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, assert_eq, run_demo  # noqa: E402

SHELL = "hello-analyzer-shell"
EXT = "/external"

#: Every section a reader can arrive at, in the order this demo walks them.
WALK = ["packets", "keys", "logs", "lab", "settings"]

#: The two pages the host paints itself — judged by a `SectionJudge` that is
#: TOLD where the reader is, rather than by a store that is emptied.
INLINE = ["dashboard", "settings"]

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def report(app: RpcSubprocess) -> dict:
    return app.query(f"{EXT}/sections")


def judged(said: dict) -> list[dict]:
    return [row for row in said["rows"] if row["standing"] == "judged"]


def stale(said: dict) -> list[str]:
    return [
        row["key"]
        for row in judged(said)
        if not row["showing"] and row["conformance"]["reconciles"]
    ]


def section_a(app: RpcSubprocess) -> int:
    banner("A — at boot, only the section showing has a verdict about a frame")
    said = report(app)
    assert_eq(app.query(f"{EXT}/nav"), "dashboard", "A: the tool opens here")
    ok(
        "A: every section the reader is not looking at is away and reproduces "
        "nothing",
        all(
            row["conformance"]["away"] == len(row["conformance"]["surfaces"])
            and row["conformance"]["reproduced"] == 0
            for row in judged(said)
            if not row["showing"]
        ),
    )
    assert_eq(
        said["reproduced"],
        sum(row["conformance"]["reproduced"] for row in judged(said) if row["showing"]),
        "A: ★★ so the headline is the showing section's, whole",
    )
    print(
        f"  [boot] {said['reproduced']} of {said['specified']} reproduced, "
        f"{len(stale(said))} stale"
    )
    return said["reproduced"]


def section_b(app: RpcSubprocess, at_boot: int) -> None:
    banner("B — walking every section and returning changes NOTHING")
    for key in WALK:
        app.intervene_painted(f"{EXT}/nav", key)
    app.intervene_painted(f"{EXT}/nav", "dashboard")
    said = report(app)
    assert_eq(
        stale(said),
        [],
        "B: ★★★★★ no section reports a reconciled specification about a frame "
        "the reader has left. Before this round three did",
    )
    assert_eq(
        said["reproduced"],
        at_boot,
        "B: ★★★★★ and the headline is the number it was at boot -- what this "
        "application claims to have reproduced is what a reader can SEE, "
        "whatever they walked through to get here. It read 88 of 133 before",
    )
    ok(
        "B: ★ every section is still judged, so this is marks being given back "
        "rather than a section falling out of the population",
        said["unjudged"] == 0 and len(judged(said)) == len(judged(report(app))),
    )
    print(f"  [walked] {said['reproduced']} of {said['specified']} reproduced, 0 stale")


def section_c(app: RpcSubprocess) -> None:
    banner("C — and it is leaving, not forgetting: standing in one fills it")
    for key in WALK:
        app.intervene_painted(f"{EXT}/nav", key)
        row = next(r for r in report(app)["rows"] if r["key"] == key)
        ok(
            f"C: standing in `{key}` its surfaces are on the frame",
            row["showing"] and row["conformance"]["standing"] > 0,
        )
        others = [
            r["key"]
            for r in judged(report(app))
            if r["key"] != key and r["conformance"]["away"] == 0
        ]
        assert_eq(
            others,
            [],
            f"C: ★★ and while standing in `{key}`, no OTHER section has a frame "
            f"-- one at a time is what a verdict about a frame means",
        )
    app.intervene_painted(f"{EXT}/nav", "dashboard")


def section_d(app: RpcSubprocess) -> None:
    banner("D — the host's own pages reach the same honesty by another road")
    said = report(app)
    for key in INLINE:
        row = next(r for r in said["rows"] if r["key"] == key)
        ok(
            f"D: `{key}` is a page the host paints, and it has no tag to address",
            "tag" not in row,
        )
        if row["showing"]:
            ok(
                f"D: ★ showing, `{key}` has its surfaces",
                row["conformance"]["away"] == 0,
            )
        else:
            ok(
                f"D: ★★ not showing, `{key}` is away with a reason -- its judge "
                f"is TOLD where the reader is, which is the other mechanism",
                row["conformance"]["away"] == len(row["conformance"]["surfaces"])
                and all(s.get("why") for s in row["conformance"]["surfaces"].values()),
            )
    ok(
        "D: ★★★★★ and the application still refuses to call any of this "
        "conformance, because a verdict is about a frame and one frame shows "
        "one section",
        said["conforms"] is False,
    )


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        at_boot = section_a(app)
        section_b(app, at_boot)
        section_c(app)
        section_d(app)

    print(f"\n=== {len(CHECKS)} named check(s) ===")
    for line in CHECKS:
        print(f"  - {line}")


if __name__ == "__main__":
    run_demo("r1763 leaving a section takes its verdict with it", body)
