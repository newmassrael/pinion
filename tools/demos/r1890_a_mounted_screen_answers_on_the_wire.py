#!/usr/bin/env python3
"""R1890 — a mounted screen answers on the wire, at the address it publishes.

# What this demo exists for

Standing rule (7) of this repayment loop says every round's deliverable is the
analyzer UI *assembled* — one shell, its sections mounted, asserted by one
walk. R1889 found that half of that was impossible: it asked the assembled tool
for the node lab's own introspect paths, got `UnknownIntrospectPath` seven
times, and had to open a SECOND PROCESS (a standalone lab) to reach the guest's
verb at all. It registered that as a debt: *a mounted screen's wire surface does
not survive mounting.*

# The re-measurement, which disproved it

Re-run at R1890 against the same two binaries, the surface was there the whole
time:

    /external/graph            -> UnknownIntrospectPath
    /node_lab/external/graph   -> "mesh-failover"
    /node_lab/external/$schema -> 82 fields

`/external/<path>` is the ROOT short-circuit, and in an assembled application
the root surface is the HOST's — so those seven refusals were true statements
about the shell. What was missing was the **address**: the row published a
`tag`, and turning a tag into an address needed a grammar that lived as a
`const` inside the transport's parser and appeared in no published value.

⚠ And one of the eight paths ANSWERED, which is what made the wrong conclusion
survive: `spec` is a name both surfaces carry, so the host's document came back
looking like the guest's. Section C is that trap, asserted.

# What this walk holds

Every mounted section of the assembled analysis tool, driven in ONE process:
the address is published, it answers, it answers that screen's OWN schema, the
host's identically-named path answers something else, the guest's ACTION runs
over it with its refusal carrying the range, and a screen the journey has left
cannot be addressed at all.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1890_a_mounted_screen_answers_on_the_wire.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    run_demo,
)

SHELL = "hello-analyzer-shell"
#: The host's own root surface — the address R1889 asked on.
HOST = "/external"
#: The seat whose screen publishes an action, so section D can drive one.
LAB_SEAT = "lab"
#: The pane that verb sizes, and the arguments it takes.
PANE = "inspector"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def js(value):
    """A published value, whether the surface handed back JSON or a string."""
    return json.loads(value) if isinstance(value, str) else value


def rows(app: RpcSubprocess) -> list[dict]:
    return js(app.query(f"{HOST}/destinations"))["destinations"]


def schema_paths(app: RpcSubprocess, surface: str) -> list[str] | None:
    """What `surface` declares, or `None` when the address does not resolve.

    ★ The refusal is returned rather than raised, because the whole claim of
    this walk is that a PUBLISHED address answers — so an address that does not
    has to fail a *named* assertion saying which property broke, not escape as
    a transport exception the reader has to reconstruct.

    Measured: mutating the roster to publish a bare `tag` (the state before
    R1890) killed the first draft of this walk here, with `UnsupportedPath` and
    no check named. A gate that dies rather than judges reports its own crash,
    not the defect.
    """
    try:
        return [field["path"] for field in js(app.query(f"{surface}/$schema"))]
    except RpcError:
        return None


def section_a(app: RpcSubprocess) -> dict[str, str]:
    banner("A — every mounted destination publishes an address; a host page does not")
    published = rows(app)
    mounted = [row for row in published if row["mounted"]]
    unmounted = [row for row in published if not row["mounted"]]

    # ★ The population floor. Without it every assertion below is vacuous on a
    # build where nothing is mounted, and "nothing is mounted" is exactly the
    # state a regression here would produce.
    ok(
        f"A: the assembled tool mounts screens at all — {len(mounted)} of "
        f"{len(published)} destinations",
        len(mounted) >= 4,
    )

    addressed = {
        row["key"]: row["screen"]["address"]
        for row in mounted
        if isinstance(row.get("screen"), dict) and isinstance(row["screen"].get("address"), str)
    }
    ok(
        f"A: ★★★★★ every mounted destination publishes an address — "
        f"{len(addressed)} of {len(mounted)}",
        len(addressed) == len(mounted),
    )
    ok(
        "A: and each address names its own screen's tag, so an address read on "
        "one row cannot reach another's screen",
        all(row["screen"]["tag"] in addressed[row["key"]] for row in mounted),
    )
    ok(
        f"A: the addresses are distinct — {len(set(addressed.values()))} for "
        f"{len(addressed)} screens",
        len(set(addressed.values())) == len(addressed),
    )
    ok(
        f"A: a page the host paints itself publishes no address, so 'ask the "
        f"screen' and 'this is the host's own page' are different values — "
        f"{len(unmounted)} such destination(s)",
        len(unmounted) > 0 and all(row["screen"] is None for row in unmounted),
    )
    return addressed


def section_b(app: RpcSubprocess, addressed: dict[str, str]) -> None:
    banner("B — the published address ANSWERS, with that screen's own schema")
    host = schema_paths(app, HOST)
    ok(
        f"B: the host's own root surface answers, so the comparison below has "
        f"two sides — {len(host or [])} declared field(s)",
        host is not None and len(host) > 0,
    )
    host = set(host or ())
    seen: dict[str, frozenset] = {}

    for seat, surface in sorted(addressed.items()):
        app.intervene(f"{HOST}/nav", seat)
        app.tick_ms(16)
        assert_eq(app.query(f"{HOST}/nav"), seat, f"the journey reached {seat}")

        declared = schema_paths(app, surface)
        ok(
            f"B: {seat}: the published address {surface!r} answers $schema — "
            f"{'no answer at all' if declared is None else str(len(declared)) + ' declared field(s)'}",
            declared is not None and len(declared) > 0,
        )
        if not declared:
            # Named, so a run that lost the address says which seat lost it and
            # keeps going to the others rather than reporting one exception.
            seen[seat] = frozenset()
            continue
        # Read a declared path THROUGH the published address. Reading `$schema`
        # alone would only prove the surface exists; this proves the address the
        # roster hands out is the one the surface's own paths hang off.
        first = declared[0]
        try:
            answered = app.query(f"{surface}/{first}")
        except RpcError as refusal:
            answered = None
            ok(f"B: {seat}: reading {first!r} at {surface!r} — {refusal}", False)
        ok(
            f"B: {seat}: a path this screen declares ({first!r}) is readable at "
            f"its published address",
            answered is not None,
        )
        ok(
            f"B: {seat}: ★★ the schema is the SCREEN's and not the host's — "
            f"{len(set(declared) - host)} field(s) the host does not declare",
            set(declared) != host and len(set(declared) - host) > 0,
        )
        seen[seat] = frozenset(declared)

    ok(
        f"B: ★ and the screens do not share one surface — "
        f"{len({paths for paths in seen.values()})} distinct schema(s) over "
        f"{len(seen)} screens",
        len({paths for paths in seen.values()}) == len(seen),
    )


def section_c(app: RpcSubprocess, addressed: dict[str, str]) -> None:
    banner("C — the trap: a name both surfaces carry answers about DIFFERENT subjects")
    surface = addressed[LAB_SEAT]
    app.intervene(f"{HOST}/nav", LAB_SEAT)
    app.tick_ms(16)

    host_said = js(app.query(f"{HOST}/spec"))
    guest_said = js(app.query(f"{surface}/spec"))

    ok(
        "C: ★★★★★ both answer `spec`, and they are different documents — this "
        "is the single answer that made R1889's seven refusals read as 'the "
        "surface survived'",
        host_said != guest_said,
    )
    ok(
        "C: the host's `spec` is the SHELL's — it describes the board the shell "
        "paints",
        "board" in host_said,
    )
    ok(
        "C: the guest's `spec` is the SCREEN's — it describes the panes the lab "
        "lays out",
        "panes" in guest_said and "board" not in guest_said,
    )


def section_d(app: RpcSubprocess, addressed: dict[str, str]) -> None:
    banner("D — the guest's ACTION runs over the wire, in this one process")
    surface = addressed[LAB_SEAT]
    app.intervene(f"{HOST}/nav", LAB_SEAT)
    app.tick_ms(16)

    # The bounds come from the screen's own published specification rather than
    # from this file — R1889's second hand, kept, now read through the mounted
    # address instead of out of a second process.
    declared = {pane["tag"]: pane for pane in js(app.query(f"{surface}/spec"))["panes"]}
    bounds = declared[f"lab.{PANE}"]["resize"]

    for asked in (bounds["min"] - 1, bounds["max"] + 1):
        try:
            answered = app.invoke(f"{surface}/place", f"{PANE},width={asked}")
        except RpcError as refusal:
            said = str(refusal)
            ok(
                f"D: ★★★★★ the guest's verb REFUSES width={asked} through the "
                f"assembled application, and the refusal names the range",
                str(bounds["min"]) in said and str(bounds["max"]) in said,
            )
        else:
            ok(
                f"D: width={asked} must be refused, not answered {answered!r}",
                False,
            )

    inside = (bounds["min"] + bounds["max"]) // 2
    answered = app.invoke(f"{surface}/place", f"{PANE},width={inside}")
    ok(
        f"D: ★★ and a width inside the range goes through, the verb saying what "
        f"it changed — {answered!r}",
        str(inside) in str(answered),
    )
    ok(
        "D: ★ the change is READABLE back at the same address, so the verb and "
        "the state are one surface and not two",
        str(inside)
        in str(
            {pane["tag"]: pane for pane in js(app.query(f"{surface}/spec"))["panes"]}[
                f"lab.{PANE}"
            ]["at"]
        ),
    )


def section_e(app: RpcSubprocess, addressed: dict[str, str]) -> None:
    banner("E — an address reaches a screen only while the journey is at it")
    surface = addressed[LAB_SEAT]
    elsewhere = next(seat for seat in addressed if seat != LAB_SEAT)

    app.intervene(f"{HOST}/nav", elsewhere)
    app.tick_ms(16)
    try:
        answered = app.query(f"{surface}/spec")
    except RpcError as refusal:
        ok(
            f"E: ★★★★★ standing at {elsewhere!r}, the node lab's address does "
            f"not resolve — a screen you are not at is not addressable "
            f"({refusal})",
            True,
        )
    else:
        ok(
            f"E: a screen the journey has left must not answer, and it answered "
            f"{str(answered)[:60]!r}",
            False,
        )

    # And going back restores it, so E is measuring the journey and not a
    # surface that died.
    app.intervene(f"{HOST}/nav", LAB_SEAT)
    app.tick_ms(16)
    ok(
        "E: and returning to the seat makes the same address answer again",
        "panes" in js(app.query(f"{surface}/spec")),
    )


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        addressed = section_a(app)
        section_b(app, addressed)
        section_c(app, addressed)
        section_d(app, addressed)
        section_e(app, addressed)

    print(f"\n=== {len(CHECKS)} named check(s) ===")
    for line in CHECKS:
        print(f"  - {line}")


if __name__ == "__main__":
    run_demo("r1890 a mounted screen answers on the wire", body)
