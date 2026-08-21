#!/usr/bin/env python3
"""R1758 §5.27 §5.38 §5.40 §2 #2 §2 #7 — **a verdict says what it was read
from, and a screen that has not painted cannot say it reproduced anything.**

# What this demo exists for

R1738 made the analysis tool count the sections it was judged on. R1742 settled
how a section answers — *from the paint; a verdict read from a screen's own
tables is structurally consistent with those tables and cannot fail for the
reason judging exists* — and wrote that rule in one screen's header. Two of the
four judged sections had been written before the rule and nobody went back.

Measured over this application's own wire at R1747 and again at R1758 before any
of this round's code existed, standing on the dashboard so that no other section
had painted a frame in the session:

```text
packets  showing=false  reproduced=  0 of 26  away=[all six surfaces]
lab      showing=false  reproduced=  0 of 15  away=[all three surfaces]
keys     showing=false  reproduced= 21 of 21  away=none      <-- here
logs     showing=false  reproduced= 15 of 15  away=none      <-- and here
```

Nothing was failing. Two sections were answering from tables, two from frames,
and **the report had no room to say which** — so a reader adding them up got a
number that was partly about pixels and partly about a copy of the
specification. A painter drawing nothing at all would have produced the same 21.

What this drives:

* **A** — every judged section's verdict now carries `evidence`, and every one
  of them says `paint`. The application publishes how many judged sections
  answered from their own tables (`declared`), and refuses to call it
  conformance while that is not zero.
* **B** — the row that was wrong: from a page that is not theirs, all four
  sections report **0 reproduced** with every surface away and a reason.
* **C** — and they are not merely silent. Walking to each one puts its surfaces
  back on screen, so the verdict moved when the frame did — which is what makes
  A and B measurements rather than a screen that stopped answering.
* **D** — the verdict NAMES the parts it is about, in the pin's own order, read
  off disk here by a second hand. A verdict saying *twenty-one were specified*
  cannot be checked by anybody.
* **E** — one value, three readers: the section's own slot, the host's row, and
  a second process running the section standalone.

# Floor, measured by building a probe against the reference toolkit 6.11.1 and
# running it

The probe assembles a paged application out of three pages — one on screen, one
never shown, and one that builds two of the three parts it declares — and asks
the toolkit about them.

* **768** members were scanned across the page-stack, the tabbed container, the
  table view, the plain page and two leaf widgets. **0** name a verdict, a
  specification, evidence, a divergence or a canon. There is nowhere to put the
  statement.
* The structural answer — *which parts does this page have* — is **identical**
  for the page on screen and the page never shown. A conformance check written
  against it therefore passes unchanged for a page nobody has opened, which is
  exactly the defect this round repaired here.
* ⚠ And the honest half: the toolkit **can** tell that a page is not visible.
  What it cannot do is connect that to a judgment — the visibility of a widget
  and the evidence behind a verdict are unrelated facts there, and nothing adds
  them up. That is the same shape the defect had in this tree: this application
  already published `showing` per row, and the two sections answering from
  tables published `21 of 21` beside `showing: false` without contradiction.
* **0** members mentioning paint return anything at all. `grab()` hands back
  pixels with no structure, and on a page that was never shown it renders one on
  demand — so even the pixels do not distinguish a page nobody opened.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell -p hello-key-patterns \\
        -p hello-log-view -p hello-packet-view -p hello-node-lab
    DISPLAY=:97 python3 tools/demos/r1758_a_verdict_says_what_it_was_read_from.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from analyzer_spec import DOCS  # noqa: E402
from rpc_verify import RpcSubprocess, assert_eq, run_demo  # noqa: E402

SHELL = "hello-analyzer-shell"
EXT = "/external"

#: The standalone binary of each judged section, so "one build, two placements"
#: is checked against a second PROCESS and not only a second slot of this one.
STANDALONE = {
    "keys": "hello-key-patterns",
    "logs": "hello-log-view",
    "packets": "hello-packet-view",
    "lab": "hello-node-lab",
}

#: Which pin each judged section is judged against, so section D compares the
#: application's canon with a document this process reads off disk.
PINS = {
    "keys": "analyzer-keys-spec.json",
    "logs": "analyzer-logs-spec.json",
    "packets": "analyzer-packets-spec.json",
    "lab": "analyzer-inspector-spec.json",
}

CHECKS: list[str] = []
PARTS_COMPARED = 0
SECTIONS_JUDGED: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def report(app: RpcSubprocess) -> dict:
    return app.query(f"{EXT}/sections")


def judged_rows(said: dict) -> list[dict]:
    return [row for row in said["rows"] if row["standing"] == "judged"]


def pin(section: str) -> dict:
    """The reviewed specification a section is judged against, off disk."""
    return json.loads((DOCS / PINS[section]).read_text(encoding="utf-8"))


def section_a(app: RpcSubprocess) -> None:
    banner("A — every judged section says WHAT ITS VERDICT WAS READ FROM")
    said = report(app)
    judged = judged_rows(said)
    ok("A: the application has judged sections to report on", len(judged) >= 4)
    assert_eq(
        app.query(f"{EXT}/nav"),
        "dashboard",
        "A: this report is being read from the dashboard, so no judged section "
        "has painted a frame in this session",
    )
    assert_eq(
        {row["key"]: row["conformance"]["evidence"] for row in judged},
        {row["key"]: "paint" for row in judged},
        "A: ★★★★★ every judged section's verdict is about a PAINTED FRAME. Two "
        "of these four answered from their own tables until this round, and "
        "nothing in the report said so",
    )
    ok(
        "A: ★★ the application counts the ones that did not, beside the ones it "
        "judged -- a population, not a footnote",
        said["declared"] == 0 and said["judged"] == len(judged),
    )
    ok(
        "A: ★★★★★ and it refuses to call a verdict from a screen's own tables "
        "conformance at all",
        said["conforms"] is False,
    )
    print(
        f"  [population] {said['sections']} section(s): {said['judged']} judged "
        f"({said['declared']} from tables), {said['unjudged']} unjudged, "
        f"{said['closed']} closed"
    )


def section_b(app: RpcSubprocess) -> None:
    banner("B — the row that was wrong: an unpainted section reproduces NOTHING")
    said = report(app)
    for row in judged_rows(said):
        key = row["key"]
        verdict = row["conformance"]
        ok(f"B: `{key}` is not the section showing", row["showing"] is False)
        ok(
            f"B: ★★★★★ `{key}` reproduces 0 of its {verdict['specified']} "
            f"specified part(s) from a page it has not painted",
            verdict["reproduced"] == 0,
        )
        ok(
            f"B: ★★ and every one of `{key}`'s {verdict['away']} surface(s) is "
            f"AWAY rather than absent -- an unopened surface is not a failing one",
            verdict["away"] == len(verdict["surfaces"])
            and all(
                s["standing"] is False and s["why"]
                for s in verdict["surfaces"].values()
            ),
        )
        ok(
            f"B: ★ and `{key}` accuses the build of nothing -- no divergence, "
            f"no unreconciled entry",
            all(
                s["divergences"] == [] and s["unreconciled"] == []
                for s in verdict["surfaces"].values()
            ),
        )
        ok(
            f"B: ★★ declining to be judged is not passing -- `{key}` does not "
            f"reconcile",
            verdict["reconciles"] is False,
        )
    assert_eq(
        said["reproduced"],
        0,
        "B: ★★★★★ so the application's headline is 0 reproduced from here. It "
        "read 36 of 77 before this round -- two sections' worth of tables",
    )


def section_c(app: RpcSubprocess) -> None:
    banner("C — walking to a section puts its surfaces back, so the verdict moves")
    global PARTS_COMPARED
    for key in sorted(STANDALONE):
        app.intervene(f"{EXT}/nav", key)
        app.tick(16)
        row = next(r for r in report(app)["rows"] if r["key"] == key)
        verdict = row["conformance"]
        ok(
            f"C: standing in `{key}`, the report says it is the one showing",
            row["showing"] is True,
        )
        ok(
            f"C: ★★★★★ and its surfaces come back -- {verdict['standing']} of "
            f"{verdict['standing'] + verdict['away']} standing, "
            f"{verdict['reproduced']} of {verdict['specified']} part(s) "
            f"reproduced. A verdict that were always away would be as "
            f"uninformative as one that is never away",
            verdict["standing"] > 0 and verdict["reproduced"] > 0,
        )
        SECTIONS_JUDGED.append(key)
        PARTS_COMPARED += verdict["reproduced"]
    app.intervene(f"{EXT}/nav", "dashboard")
    app.tick(16)
    print(
        f"  [moved] {PARTS_COMPARED} part(s) went from away to reproduced by "
        f"navigating"
    )


def section_d(app: RpcSubprocess) -> None:
    banner("D — the verdict NAMES the parts it is about, in the pin's own order")
    for key in sorted(STANDALONE):
        app.intervene(f"{EXT}/nav", key)
        app.tick(16)
        said = next(r for r in report(app)["rows"] if r["key"] == key)["conformance"]
        declared = pin(key)
        # The pin may nest its surfaces under a name of its own; the population
        # is the verdict's, and every surface of it must be in the pin.
        canon = {
            name: [part["key"] for part in row["canon"]]
            for name, row in said["surfaces"].items()
        }
        wanted = {}
        for name in canon:
            source = declared.get(name)
            if source is None:
                for value in declared.values():
                    if isinstance(value, dict) and name in value:
                        source = value[name]
                        break
            assert source is not None, f"D: the pin for `{key}` has no `{name}` surface"
            wanted[name] = [part["key"] for part in source["canon"]]
        assert_eq(
            canon,
            wanted,
            f"D: ★★★★★ `{key}`'s verdict names exactly the parts "
            f"docs/{PINS[key]} declares, in its order, read off disk here by a "
            f"second hand -- a count with nothing behind it cannot be checked "
            f"from outside",
        )
        ok(
            f"D: ★ and `{key}`'s specified count is the length of that canon, "
            f"so the two halves of the verdict cannot disagree",
            said["specified"] == sum(len(parts) for parts in canon.values()),
        )
    app.intervene(f"{EXT}/nav", "dashboard")
    app.tick(16)


def section_e(app: RpcSubprocess) -> None:
    banner("E — one value, three readers")
    for key in sorted(STANDALONE):
        app.intervene(f"{EXT}/nav", key)
        app.tick(16)
        row = next(r for r in report(app)["rows"] if r["key"] == key)
        own = app.query(f"/{row['tag']}{EXT}/conformance")
        assert_eq(
            own,
            row["conformance"],
            f"E: ★★ the host's row for `{key}` IS the value the section "
            f"publishes on its own wire -- the host aggregates, it does not "
            f"re-derive",
        )
        ok(
            f"E: ★ and the section's own slot carries the qualifier too, so a "
            f"client of the section alone is not reading a bare count",
            own["evidence"] == "paint",
        )
    app.intervene(f"{EXT}/nav", "dashboard")
    app.tick(16)

    # The third reader: a second PROCESS running one of these sections in its
    # own window. `keys` is the one to use — its three surfaces are drawn
    # whenever the section is showing, so two processes in different sessions
    # must agree, which makes the equality a claim about the build.
    app.intervene(f"{EXT}/nav", "keys")
    app.tick(16)
    page = next(r for r in report(app)["rows"] if r["key"] == "keys")["conformance"]
    with RpcSubprocess(STANDALONE["keys"], boot_grace=1.5) as alone:
        assert_eq(
            alone.query(f"{EXT}/conformance"),
            page,
            "E: ★★★★★ and the standalone binary of that section publishes the "
            "SAME verdict as the page does -- a section that conforms in its "
            "own window and is never asked as a page would be two builds "
            "wearing one name",
        )
    app.intervene(f"{EXT}/nav", "dashboard")
    app.tick(16)


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        section_a(app)
        section_b(app)
        section_c(app)
        section_d(app)
        section_e(app)

    print(
        f"\n[coverage] {len(set(SECTIONS_JUDGED))} judged section(s) driven "
        f"through the assembled application; {PARTS_COMPARED} part(s) observed "
        f"moving from away to reproduced."
    )
    print(f"\n=== {len(CHECKS)} named check(s) ===")
    for line in CHECKS:
        print(f"  - {line}")


if __name__ == "__main__":
    run_demo("r1758 a verdict says what it was read from", body)
