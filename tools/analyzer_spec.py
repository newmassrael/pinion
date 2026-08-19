#!/usr/bin/env python3
"""R1730 — **the analysis tool's written specifications, read once.**

## What forced this module

R1728 made `docs/analyzer-rail-spec.json` the reviewed statement of what the
tool's navigation is, and two demos began reading it instead of carrying
literals. R1730 paid a divergence off — it built `keys` — and **five** demos
broke at once, every one of them on a seat list somebody had written out by
hand.

The lift is what stopped that repair from being the disease. Fixing those five
by teaching each of them to read the pin would have left **six** copies of the
loader and four of "which seats does the specification say are shut", which is
this project's mechanical-duplication case arrived at while repairing the
consequences of not having done it.

That is the mechanical-duplication case this project lifts on sight, and the
sharper reason is what the copies were for: two demos disagreeing about which
seats are shut would disagree about the same build, and the one that was never
run would be the one that was wrong. R1695's `elsewhere` expectation had in fact
been stale for two rounds and nothing said so.

## What lives here

The loaders and the three derived rosters every consumer wants. Nothing about a
running application — these read files in this repository, so a demo comparing
a running screen with one of them is comparing two things written by different
hands, which is the whole point of the specifications being separate artifacts.
"""

from __future__ import annotations

import json
from pathlib import Path

DOCS = Path(__file__).resolve().parent.parent / "docs"
RAIL_SPEC_PATH = DOCS / "analyzer-rail-spec.json"
KEYS_SPEC_PATH = DOCS / "analyzer-keys-spec.json"


def rail_spec() -> dict:
    """The tool's navigation, as `docs/analyzer-rail-spec.json` states it."""
    return json.loads(RAIL_SPEC_PATH.read_text(encoding="utf-8"))


def keys_spec() -> dict:
    """The key-pattern section's three surfaces, as their pin states them."""
    return json.loads(KEYS_SPEC_PATH.read_text(encoding="utf-8"))


def rail_keys() -> list[str]:
    """Every seat the reference draws, in the reference's order."""
    return [seat["key"] for seat in rail_spec()["canon"]]


def owed_keys() -> list[str]:
    """The seats the reference has working and this build has not written.

    Sorted, because every consumer compares it against a sorted census.
    """
    return sorted(entry["key"] for entry in rail_spec()["owed"])


def reserved_keys() -> list[str]:
    """The seats the reference itself draws locked, booked under a requirement
    of a release that has not shipped."""
    return sorted(
        seat["key"] for seat in rail_spec()["canon"] if seat.get("kind") == "reserved"
    )


def closed_keys() -> list[str]:
    """Everything the specification says is shut, for either reason."""
    return sorted(owed_keys() + reserved_keys())


def open_keys() -> list[str]:
    """What is left: the seats this build is expected to open."""
    shut = set(closed_keys())
    return sorted(key for key in rail_keys() if key not in shut)


def surfaces(spec: dict) -> list[str]:
    """The surfaces a section's specification fixes, by the name it gives them.

    Takes the document rather than reading one, because there is more than one:
    R1730 wrote this bound to the key-pattern pin and R1731's section grew a
    copy of it within the round — the same duplication, one level down, and
    caught by the close audit rather than by the next round.

    Keys beginning with `$` are the document's own commentary and are not
    surfaces, which is the rule `pinion_core::conformance::SpecDocument` takes.
    """
    return sorted(key for key in spec if not key.startswith("$"))
