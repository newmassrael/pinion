#!/usr/bin/env python3
"""R1888 §5.38 §5.40 §2 #7 — **a section's row carries its own reason, and a
section that has not given one is counted rather than excused.**

# What this demo exists for

`docs/analyzer-sections-spec.json` has carried an entry since R1738 saying that
*an unjudged section's row carries the host's reason, not the screen's*. It was
true: `SectionStanding::Unspecified` was a unit variant for 150 rounds, so every
such row published one constant sentence — *a screen is here and it publishes no
verdict about a specification* — which is true of every unjudged section and
therefore tells a reader nothing the word `unspecified` had not already told
them. The fact it stood in for is one only the screen has: *nobody wrote a
specification* and *one exists where the assembled application cannot reach it*
are different gaps with different repairs.

R1742 built the same thing one level down for a SURFACE (`Built::Away`, a
declared absence carrying its reason) and left a SECTION without it. This is
that other half, and this walk is what holds the assembled tool to it.

# The distinction it drives, and the population it drives it on

Every row of the application's report carries either a **reason** or an
**admission**, and until this round nothing could tell them apart:

* a reason comes from the subject and says something only the subject knows —
  a verdict from the screen that painted the section, a closure from the
  destination that is shut;
* an admission comes from the host and says nobody answered — a page painted
  inline with nothing registered for it, or a screen handing back the framework
  default, which is deliberately worded as an admission so that this walk can
  find it.

Both read like sentences, which is the difficulty. So `accounts` is published
per row and `unaccounted` beside `unjudged`, and this demo asserts the finer
count rather than inferring it from a string.

What this drives:

* **A** — the report is one row per rail seat and every row publishes whether it
  accounts for itself; `unaccounted` is a subset of `unjudged`.
* **B** — nothing in this application carries an admission. The framework's
  admission constant is read out of `pinion-shell`'s own source by a second
  hand rather than copied here, and it appears in no published sentence.
* **C** — ★★★★★ a published reason is the SUBJECT's, shown where a subject can
  be asked twice: navigating to a closed seat refuses **with the same sentence
  the row published**. A host writing that string could not have produced it.
* **D** — the reasons are per-subject and not one constant: as many distinct
  sentences as there are rows carrying one.
* **E** — ★ a WALK. The three above are asserted from every open section of the
  tool in turn, because a reason computed from the page a reader happens to be
  standing on would pass any of them read once.

# Floor

The floor for *a container that says why one of its pages was not checked* was
measured against the reference toolkit at 6.11.1 in R1738 and R1758 by building
a probe and running it: nothing in its paged container or its widget base names
a specification, a verdict, evidence or a divergence, so there is no member to
compare this against — the question cannot be put to it. This round adds the
narrower row: there, a page that answers nothing is indistinguishable from a
page nobody asked, and the container has no count either way.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1888_a_silent_section_says_why_in_its_own_words.py
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from analyzer_spec import closed_keys, open_keys, rail_keys  # noqa: E402
from rpc_verify import RpcSubprocess, assert_eq, run_demo  # noqa: E402

SHELL = "hello-analyzer-shell"
EXT = "/external"
ROOT = Path(__file__).resolve().parent.parent.parent

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def report(app: RpcSubprocess) -> dict:
    return app.query(f"{EXT}/sections")


def admission() -> str:
    """The framework's admission constant, read from `pinion-shell`'s own
    source.

    ⚠ Read rather than copied, and that is the whole reason the constant is
    public. A demo carrying its own copy of this string would go green after the
    framework changed the sentence — it would be comparing the application with
    a literal this file remembered, which is the failure every pin in
    `analyzer_spec` exists to prevent, arrived at one layer down.
    """
    source = (ROOT / "crates" / "pinion-shell" / "src" / "lib.rs").read_text(
        encoding="utf-8"
    )
    found = re.search(r'pub const UNSTATED: &str = "([^"]+)";', source)
    assert found, (
        "`pinion_shell::UNSTATED` was not found in its own source, so this "
        "demo read nothing and every check below would have passed vacuously"
    )
    return found.group(1)


def section_a(app: RpcSubprocess) -> None:
    banner("A — one row per seat, and every row says whether it accounts for itself")
    said = report(app)

    assert_eq(
        sorted(row["key"] for row in said["rows"]),
        sorted(rail_keys()),
        "A: the population is every seat the specification names -- a section "
        "can be missing from this report only by not being in the application",
    )
    ok(
        "A: ★ every row publishes `accounts`, so a reader is never left to "
        "recognise a framework constant inside a sentence",
        all(isinstance(row.get("accounts"), bool) for row in said["rows"]),
    )
    ok(
        "A: ★★ and the count agrees with the rows it is made of",
        said["unaccounted"] == sum(1 for row in said["rows"] if not row["accounts"]),
    )
    ok(
        "A: ★★★ `unaccounted` is a SUBSET of `unjudged` -- saying why is not "
        "being judged, so explaining a silence must not move the coarser count",
        said["unaccounted"] <= said["unjudged"],
    )
    print(
        f"  [population] {said['sections']} section(s): {said['judged']} judged, "
        f"{said['unjudged']} unjudged, of which {said['unaccounted']} unaccounted, "
        f"{said['closed']} closed"
    )


def section_b(app: RpcSubprocess) -> None:
    banner("B — nothing here carries an admission")
    said = report(app)
    unstated = admission()

    silent = sorted(row["key"] for row in said["rows"] if not row["accounts"])
    assert_eq(
        silent,
        [],
        "B: ★★★★★ no section of this application carries an admission where a "
        "reason should be. A mounted screen says why through "
        "`WidgetView::unjudged_because`; a page the host paints itself gets a "
        "verdict through `ScreenRoster::judging`",
    )
    assert_eq(said["unaccounted"], 0, "B: and the published count says so too")
    ok(
        f"B: ★★ the framework's admission -- {unstated!r} -- is in no "
        "published sentence, and it was read from `pinion-shell`'s source "
        "rather than copied into this demo",
        all(row.get("why") != unstated for row in said["rows"]),
    )


def section_c(app: RpcSubprocess) -> None:
    banner("C — a published reason is the SUBJECT's, asked a second way")
    said = report(app)
    shut = [row for row in said["rows"] if row["standing"] == "closed"]

    assert_eq(
        sorted(row["key"] for row in shut),
        sorted(closed_keys()),
        "C: the seats the report calls closed are the ones the specification "
        "says are shut",
    )
    ok(
        "C: a closed seat is one this demo can ask twice, so the population "
        "this section stands on is not empty",
        len(shut) > 0,
    )

    for row in shut:
        ok(f"C: `{row['key']}` carries a reason at all", bool(row.get("why")))
        ok(
            f"C: ★ and it accounts for itself -- a closure is the destination "
            f"speaking for itself, not the host admitting nobody looked",
            row["accounts"] is True,
        )
        try:
            app.intervene(f"{EXT}/nav", row["key"])
        except Exception as refusal:  # noqa: BLE001
            ok(
                f"C: ★★★★★ going to `{row['key']}` refuses with THAT SAME "
                f"sentence, so the row is the subject's words and not a second "
                f"wording of them -- {row['why']}",
                row["why"] in str(refusal),
            )
        else:
            ok(f"C: navigating to the closed seat `{row['key']}` must refuse", False)


def section_d(app: RpcSubprocess) -> None:
    banner("D — the reasons are per-subject, not one constant")
    said = report(app)
    reasons = [row["why"] for row in said["rows"] if row.get("why")]

    ok(
        "D: there are rows carrying a reason, so this is a claim rather than "
        "an empty one",
        len(reasons) > 1,
    )
    assert_eq(
        len(set(reasons)),
        len(reasons),
        "D: ★★★★★ as many DISTINCT sentences as there are rows carrying one. "
        "The arm this round replaced published one constant for every unjudged "
        "section, and a check that only asked whether a sentence was present "
        "would have passed on it",
    )


def section_e(app: RpcSubprocess) -> None:
    banner("E — ★ and all of it from every section in turn, which is the walk")
    seats = open_keys()
    ok("E: the tool opens more than one section, so a walk has somewhere to go", len(seats) > 1)

    baseline: dict[str, str] | None = None
    for seat in seats:
        app.intervene_painted(f"{EXT}/nav", seat)
        said = report(app)
        silent = sorted(row["key"] for row in said["rows"] if not row["accounts"])
        assert_eq(
            silent,
            [],
            f"E: standing on `{seat}`, no section carries an admission",
        )
        here = {row["key"]: row["why"] for row in said["rows"] if row.get("why")}
        if baseline is None:
            baseline = here
        else:
            assert_eq(
                here,
                baseline,
                f"E: ★★★★★ and standing on `{seat}` the reasons are the same "
                f"ones. A reason is a fact about its own section, so one that "
                f"moved with the reader's position would be the host inferring "
                f"it again -- which is the defect this round closed and which "
                f"no single reading of this report can see",
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
    run_demo("r1888 a silent section says why in its own words", body)
