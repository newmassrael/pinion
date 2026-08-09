#!/usr/bin/env python3
"""R1621 §5.16 §5.41 §2 #7 — a display says how much of it is usable, and how
well that is known.

The last buildable gap on the analyzer-tool's plane C: a dashboard that tears a
widget off into its own window can say WHERE to put it (R1087 position, R1576
display-relative placement, R1617 `display_home`) but not where it is ALLOWED
to go, so a torn-off window opens under the panel.

The toolkit's peer is `availableGeometry()`, a plain rectangle. Read from the
reference's own X11 source rather than assumed, that rectangle is a guess whose
quality the caller cannot see:

* Its plugin carries an internal comment saying that deriving a per-monitor work
  area from the desktop-wide atom is unreliable, that window managers disagree
  about what the atom means with several monitors attached, and that the
  window-manager specification has no atom for the per-monitor answer.
* Its conclusion is the part that matters: on a multi-head system its accessor
  **returns the full screen bounds** unless an environment variable overrides,
  so on any two-monitor desk it answers "all of it is available" and the caller
  cannot tell that from a measurement.

So this publishes the answer AND its provenance — four arms, one of which is a
measurement — and recovers displays the reference gives up on: a work area that
takes nothing from a display is a real answer for it, however many displays the
desk has.

Run from the workspace root:
    cargo build -p hello-displays --release
    python3 tools/demos/r1621_a_display_says_how_sure_it_is.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    call,
    run_demo,
)

EXAMPLE = "hello-displays"

#: The four arms, mirrored from the model rather than imported — a demo that
#: read its expected vocabulary out of the code under test could not catch that
#: code changing.
PROVENANCE = {"reported", "desktop_wide", "unpublished", "unprobed"}


def displays(tf: RpcSubprocess) -> list[dict]:
    return call(tf, "scene/displays")["displays"]


def run(tf: RpcSubprocess) -> None:
    # ── 0. the surface is discoverable, and so is its vocabulary ─────────
    catalogue = {m["name"] for m in call(tf, "rpc/methods")["methods"]}
    assert "scene/displays" in catalogue
    schema = call(tf, "rpc/schema")
    types = {t["name"]: t for t in schema["types"]}
    assert "UsableRegionWire" in types, (
        "the reply shape is in the census, so an agent reads it before parsing"
    )
    fields = {f["name"]: f for f in types["UsableRegionWire"]["shape"]["fields"]}
    assert set(fields) == {"rect", "provenance"}, (
        f"the rectangle AND its provenance, side by side: {sorted(fields)}"
    )
    declared = set(fields["provenance"]["values"])
    assert_eq(
        declared,
        PROVENANCE,
        "the closed value set is declared (R1616) and derived from the arms, so "
        "a client can match exhaustively without reading pinion's source",
    )
    print(f"[demo] provenance vocabulary on the wire: {sorted(declared)}")

    # ── 1. every display answers, and the answer is usable ───────────────
    desk = displays(tf)
    assert desk, "at least one display"
    for d in desk:
        usable = d["usable"]
        assert set(usable) == {"rect", "provenance"}, usable
        assert usable["provenance"] in PROVENANCE, usable
        rect = usable["rect"]
        assert rect["w"] > 0 and rect["h"] > 0, (
            f"a usable region is never empty — 'none of this display can be "
            f"used' is the one answer certainly wrong: {rect}"
        )
        print(
            f"[demo] {d['id']}: usable {rect['w']}x{rect['h']}"
            f"+{rect['x']}+{rect['y']} ({usable['provenance']})"
        )

    # ── 2. the region never exceeds the bounds ───────────────────────────
    #      It is bounds MINUS panels, so it is contained by construction. A
    #      derivation that returned a rect sticking out of its own display
    #      would place windows off-screen.
    for d in desk:
        b, u = d["bounds"], d["usable"]["rect"]
        assert u["x"] >= b["x"] and u["y"] >= b["y"], (d["id"], b, u)
        assert u["x"] + u["w"] <= b["x"] + b["w"], (d["id"], b, u)
        assert u["y"] + u["h"] <= b["y"] + b["h"], (d["id"], b, u)

    # ── 3. only `reported` is a measurement, and the fallback IS the bounds
    #      This is the check the reference cannot pass: there, a non-answer and
    #      a measurement are the same value.
    for d in desk:
        u = d["usable"]
        if u["provenance"] == "reported":
            continue
        assert_eq(
            u["rect"],
            d["bounds"],
            f"{d['id']}: a non-measurement falls back to the full bounds, and "
            "says which of the three reasons it is",
        )
    measured = [d["id"] for d in desk if d["usable"]["provenance"] == "reported"]
    print(f"[demo] measured: {measured or 'none on this desk'}")

    # ── 4. the answer is STABLE across calls ─────────────────────────────
    #      A probe re-run per dispatch must not flap: a client that reads the
    #      desk twice while deciding where to put a window would otherwise get
    #      two different answers for a desk nobody touched.
    again = displays(tf)
    assert_eq(
        [d["usable"] for d in again],
        [d["usable"] for d in desk],
        "two reads of an untouched desk agree",
    )

    # ── 5. it is per display, and it travels with the display's identity ─
    ids = [d["id"] for d in desk]
    assert_eq(len(ids), len(set(ids)), f"ids are unique: {ids}")
    for d in desk:
        assert "usable" in d, f"{d['id']} carries its own region, not a global one"

    # ── 6. NEGATIVE CONTROL: the provenance is not a constant ────────────
    #      Whatever this desk answers, the vocabulary must contain arms this
    #      run did NOT produce — otherwise "it always says reported" would look
    #      identical to a working derivation.
    produced = {d["usable"]["provenance"] for d in desk}
    assert produced <= PROVENANCE, produced
    assert PROVENANCE - produced, (
        "the declared vocabulary is strictly larger than what this desk "
        f"produced ({sorted(produced)}) — a single-arm enum would make the "
        "provenance decorative"
    )
    print(f"[demo] this desk produced {sorted(produced)} of {len(PROVENANCE)} arms")

    print("[demo] a display says how much of it is usable, and how sure it is")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        run(tf)


if __name__ == "__main__":
    run_demo("R1621 §5.16 — a display says how sure it is", body)
