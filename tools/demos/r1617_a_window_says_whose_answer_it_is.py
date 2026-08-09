#!/usr/bin/env python3
"""R1617 §5.16 §5.41 §2 #7 — a window says which display it is on, and WHOSE
answer that is.

Two questions look like one. "Where was this window asked to go" is the
declaration, and R1576 answered it with `anchored`. "Where is this window" is a
different fact: a window-manager-placed window declares nothing at all and is
still somewhere, and a window the user drags has moved without any declaration
changing.

That second question has two answerers, and until this round only one of them
was ever consulted. This framework derives a home from the window's live
rectangle — the display holding the largest share. The window system has its own
opinion, and across the four desktop backends underneath there are FOUR rules:
two resolve by largest intersection, one caches an answer refreshed only when
the window's scale factor changes, and one reports the first compositor output
the surface entered, which is not geometric at all. One of them answers with the
first enumerated monitor for a window that is on no monitor, where this
framework answers "nowhere".

So the two can disagree without either being wrong, which is exactly why this is
a REPORT and not a check: a gate would have to invent a rule overriding a
platform's own.

What this script checks, and why each check discriminates:

* **`scene/windows` publishes `display_home` for every window** — with the
  derived answer, the platform's answer, and the named relation between them.
* **The vocabulary is on the wire.** `rpc/schema` publishes the closed set
  `display_home.kind` can carry, so a client knows what to match on rather than
  collecting spellings by observation. Every published spelling is reachable
  from the framework's own type, which the binding mirrors.
* **PAST THE USUAL SHAPE (1): both answers are readable.** The conventional
  toolkit has both too — a public accessor returning the platform plugin's
  stored answer, and a geometric resolver that is PRIVATE, consulted only when
  the application itself moves the window, and deciding by CENTRE POINT rather
  than by largest share. Nothing there puts the two side by side and an
  application cannot, because one of them is unreachable.
* **PAST THE USUAL SHAPE (2): silence is not agreement.** The backend accessor
  is an option and a hidden window can get nothing from it. `platform_silent`
  is its own answer, distinct from `agreed`.
* **PAST THE USUAL SHAPE (3): the binding reads the same fact.** The panel
  PAINTS its home through the in-process hook while the wire derives it from the
  same stamp, so the two are one derivation with two callers.
* **The home is not the declaration.** Moving the panel with the framework's own
  write verb moves the home; changing its LEVEL does not. Two fields, two
  questions, and this drives both directions.
* **The level outcome's three vocabularies are published too** — `kind`,
  `declared` and `backend` all carry closed sets, and R1610 published none of
  them. Knowing a match is safe is worth nothing without knowing what to match.

Everything here holds on any host: no assertion depends on how many monitors
there are or on which windowing system is running. Where the answer legitimately
differs the script asserts the RELATION, which is the fact worth pinning.

Run from the workspace root:
    cargo build -p hello-displays --release
    python3 tools/demos/r1617_a_window_says_whose_answer_it_is.py
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

PANEL = "panel"
MAIN = "main"

#: Mirrored from the framework rather than imported — a demo that read its
#: expected answers out of the code under test could not catch that code
#: changing.
HOME_KINDS = [
    "agreed",
    "diverged",
    "platform_silent",
    "derived_nowhere",
    "nowhere",
]
OUTCOME_KINDS = ["applied", "unsupported", "unknown"]
LEVELS = ["always_on_bottom", "normal", "always_on_top"]
BACKENDS = ["x11", "wayland", "macos", "windows", "other"]

#: A home whose derived side named a display. The two arms where it did not are
#: the two that report the window as being on no display at all.
DERIVED_SOMEWHERE = {"agreed", "diverged", "platform_silent"}
#: ...and where the platform named one.
PLATFORM_SPOKE = {"agreed", "diverged", "derived_nowhere"}


def q(tf: RpcSubprocess, path: str):
    """One `query` against the primary External."""
    return tf.query(f"/external/{path}")


def windows(tf: RpcSubprocess) -> dict[str, dict]:
    return {w["id"]: w for w in call(tf, "scene/windows")["windows"]}


def declare(tf: RpcSubprocess, **patch) -> dict:
    return call(tf, "scene/window_declare", patch)


def settle(tf: RpcSubprocess, frames: int = 2) -> None:
    for _ in range(frames):
        tf.tick(0.016)


def field_of(schema: dict, type_name: str, field_name: str) -> dict:
    for t in schema["types"]:
        if t["name"] != type_name:
            continue
        for f in t.get("shape", {}).get("fields") or []:
            if f["name"] == field_name:
                return f
        raise AssertionError(f"{type_name} publishes no {field_name} key")
    raise AssertionError(f"the census holds no type {type_name}")


def run(tf: RpcSubprocess) -> None:
    # ---- 1. every window publishes a home, and it is a well-formed report ----
    specs = windows(tf)
    assert MAIN in specs and PANEL in specs, f"both windows declared: {list(specs)}"
    for wid, w in specs.items():
        assert "display_home" in w, f"{wid} must carry a display_home key: {w!r}"
    homes = {wid: w["display_home"] for wid, w in specs.items()}
    for wid, home in homes.items():
        assert home is not None, (
            f"{wid} has a live window on a real windowing system, so somebody "
            f"looked — a null here would mean the stamp never ran"
        )
        assert set(home) == {"kind", "derived", "platform"}, (
            f"{wid} home is exactly three fields on every kind: {home!r}"
        )
        assert home["kind"] in HOME_KINDS, f"{wid} kind {home['kind']!r}"

    panel_home = homes[PANEL]
    print(
        f"[demo] panel home: kind={panel_home['kind']!r} "
        f"derived={panel_home['derived']!r} platform={panel_home['platform']!r}"
    )

    # ---- 2. the KIND and the two ids agree with each other -------------------
    #        The arms are not free-standing labels: each one says exactly which
    #        answerers spoke, and a report whose kind disagreed with its own
    #        fields would be unusable. This is the relation, asserted rather
    #        than assumed, on whatever the host actually answers.
    for wid, home in homes.items():
        kind = home["kind"]
        assert_eq(
            home["derived"] is not None,
            kind in DERIVED_SOMEWHERE,
            f"{wid}: kind {kind!r} must agree with whether WE named a display",
        )
        assert_eq(
            home["platform"] is not None,
            kind in PLATFORM_SPOKE,
            f"{wid}: kind {kind!r} must agree with whether the WINDOW SYSTEM "
            f"named one",
        )
        if kind == "agreed":
            assert_eq(
                home["derived"],
                home["platform"],
                f"{wid}: 'agreed' means one display, so the two ids are one id",
            )
        if kind == "diverged":
            assert home["derived"] != home["platform"], (
                f"{wid}: 'diverged' with two equal ids is not a divergence"
            )

    # ---- 3. an id a home names is a display the desk actually holds ----------
    desk = call(tf, "scene/displays")
    attached = {d["id"] for d in desk["displays"]}
    print(f"[demo] attached displays: {sorted(attached)}")
    for wid, home in homes.items():
        for side in ("derived", "platform"):
            if home[side] is not None:
                assert home[side] in attached, (
                    f"{wid}.{side} names {home[side]!r}, which scene/displays "
                    f"does not list — the two reads must be one desk"
                )

    # ---- 4. the vocabulary is PUBLISHED, not learnt by observation ----------
    schema = call(tf, "rpc/schema")
    kind_field = field_of(schema, "DisplayHomeWire", "kind")
    assert_eq(
        kind_field.get("values"),
        HOME_KINDS,
        "a key whose value set is closed says which values — where the only "
        "way to learn a spelling otherwise is to see one arrive",
    )
    for side in ("derived", "platform"):
        f = field_of(schema, "DisplayHomeWire", side)
        assert f.get("nullable"), (
            f"{side} is nullable: an answerer that named nothing must be "
            f"representable, and null is how"
        )
    home_field = field_of(schema, "DeclaredWindow", "display_home")
    assert_eq(home_field.get("of"), "DisplayHomeWire", "and it nests by name")
    assert home_field.get("nullable"), (
        "a window nobody looked at claims nothing, which is a third thing "
        "again from a home that resolved to nowhere"
    )
    # The kind actually being reported is in the published set — the check that
    # makes publishing worth anything.
    for wid, home in homes.items():
        assert home["kind"] in kind_field["values"], (
            f"{wid} reports {home['kind']!r}, which the published set omits"
        )

    # ---- 5. the binding's own enumeration mirrors the framework's -----------
    assert_eq(
        [s for s in str(q(tf, "home_kinds")).split(",") if s],
        HOME_KINDS,
        "the application surface is a MIRROR of the framework vocabulary, not "
        "a second source of it",
    )

    # ---- 6. the binding and the wire read ONE derivation --------------------
    #        The panel paints its home through the in-process hook; the wire
    #        derives it from the same stamp. Two copies would be two things to
    #        disagree.
    def flat(home: dict | None) -> str:
        if home is None:
            return "unstamped"
        return f"{home['kind']}:{home['derived'] or ''}:{home['platform'] or ''}"

    assert_eq(
        str(q(tf, "panel_home")),
        flat(homes[PANEL]),
        "the binding's read and the wire's read are one derivation",
    )
    assert_eq(str(q(tf, "main_home")), flat(homes[MAIN]), "and for the other window")
    snap = tf.snapshot(source="paint", window=PANEL)
    assert f"home: {flat(homes[PANEL])}" in str(snap), (
        "the panel PAINTS its home, so what is drawn and what the wire says "
        "come from one fact"
    )

    # ---- 7. the home is the WINDOW, not the declaration ---------------------
    #        The two fields answer different questions, and this drives both
    #        directions rather than asserting it in prose.
    before = windows(tf)[PANEL]
    declare(tf, window_id=PANEL, level="always_on_top")
    settle(tf)
    after = windows(tf)[PANEL]
    assert_eq(after["level"], "always_on_top")
    assert_eq(
        after["display_home"],
        before["display_home"],
        "pinning a window to the front does not move it between monitors",
    )

    # A window-manager-placed window declares no placement at all — and still
    # has a home. That is the case `anchored` structurally cannot cover.
    declare(tf, window_id=PANEL, position=None)
    settle(tf)
    unplaced = windows(tf)[PANEL]
    assert_eq(
        unplaced["anchored"],
        None,
        "handing the window back to the window manager clears the declaration",
    )
    assert unplaced["display_home"] is not None, (
        "...and it is still on a monitor. A window that declares nothing is "
        "exactly the window whose home nothing else could report"
    )
    assert_eq(unplaced["display_home"]["kind"], before["display_home"]["kind"])

    # ---- 8. moving it is visible in the home, through the framework verb ----
    declare(tf, window_id=PANEL, position=[60, 60])
    settle(tf)
    moved = windows(tf)[PANEL]
    assert_eq(moved["position"], [60, 60])
    assert moved["display_home"] is not None
    assert_eq(
        moved["display_home"]["kind"],
        before["display_home"]["kind"],
        "a move within one display does not change WHOSE answer it is",
    )
    assert_eq(
        moved["anchored"]["kind"],
        "on_declared",
        "and the declaration resolves on the desk it was measured in",
    )

    # ---- 9. the LEVEL outcome's three vocabularies are published too --------
    #        R1610 published the fields and none of their value sets. A client
    #        told which windowing system decided its level's fate can only
    #        branch on that word if it knows the word list.
    outcome = specs[PANEL]["level_outcome"]
    assert outcome is not None, "a windowed surface stamped a backend"
    assert_eq(
        field_of(schema, "LevelOutcomeWire", "kind").get("values"),
        OUTCOME_KINDS,
        "what `kind` may say is now readable off the wire",
    )
    assert_eq(
        field_of(schema, "LevelOutcomeWire", "declared").get("values"),
        LEVELS,
        "and it is the SAME set the write side accepts",
    )
    assert_eq(
        field_of(schema, "LevelOutcomeWire", "backend").get("values"),
        BACKENDS,
        "and the backends are enumerable, so 'is this the one with no "
        "stacking protocol?' is answerable without reading pinion's source",
    )
    assert_eq(
        field_of(schema, "WindowDeclareParams", "level").get("values"),
        LEVELS,
        "R1616's write-side set is unchanged, and the read side now matches it",
    )
    assert outcome["kind"] in OUTCOME_KINDS
    assert outcome["backend"] in BACKENDS, (
        f"the running backend {outcome['backend']!r} is in the published set — "
        f"a vocabulary the wire can step outside of is not one"
    )
    # `level` itself now publishes its set on the READ side as well, so an
    # agent reading a window and writing one back speaks one vocabulary.
    assert_eq(
        field_of(schema, "DeclaredWindow", "level").get("values"),
        LEVELS,
    )

    # ---- 10. the whole census stays well-formed ----------------------------
    #        A closed value set is only a contract if it is one: distinct,
    #        non-empty spellings on a key that carries strings.
    declaring = 0
    for t in schema["types"]:
        for f in t.get("shape", {}).get("fields") or []:
            values = f.get("values")
            if values is None:
                continue
            declaring += 1
            assert f["ty"] == "string", (
                f"{t['name']}.{f['name']} declares a value set on a "
                f"{f['ty']!r} key"
            )
            assert values, f"{t['name']}.{f['name']} publishes an empty set"
            assert len(set(values)) == len(values), (
                f"{t['name']}.{f['name']} lists a spelling twice"
            )
    assert declaring >= 5, (
        f"only {declaring} field(s) publish a value set — R1616 shipped the "
        f"slot with one consumer and this round is the rest"
    )

    # ---- 11. and it all reaches assistive technology ------------------------
    access = call(tf, "scene/access")
    assert "display(s)" in str(access), "the desk still reaches AT"
    print("[demo] the home is published, named, mirrored in-process, and painted")


def body() -> None:
    with RpcSubprocess("hello-displays", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        run(tf)


if __name__ == "__main__":
    run_demo("r1617 a window says whose answer it is", body)
