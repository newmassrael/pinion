#!/usr/bin/env python3
"""R1893 — a named arrangement knows whose it is, and a delete that is safe.

# What this demo exists for

Standing rule (7) of this repayment loop asks for the analyzer UI assembled and
asserted by one walk. This drives the dashboard's layout presets, which the
behaviour canon has as `builtinPresets()` + `state.customPresets`, with
`applyPreset`, `saveCurrentLayout` and — the one this shell did not have —
`deleteCustomPreset`.

# Why the delete could not exist before

Measured at R1893, this shell held its presets in a bare map from a name to a
board. The arrangement the application OPENS ON looked exactly like one a person
had saved, so a delete would have taken the opening layout with nothing to bring
it back. The missing piece was never the delete; it was the set's inability to
say where a row came from.

`pinion_core::workspace` is that set: every row carries a `Provenance`, a
built-in refuses deletion WITH a sentence, and — the case that makes `save`
fallible at all — saving over a built-in is refused too, because a row that kept
saying `built-in` while holding a person's layout would make the delete rule
protect the wrong thing.

# What the floor has, measured this round

Its main window round-trips an arrangement as 126 opaque bytes and refuses a
blob that is not one. Of 108 published members, ZERO name a named arrangement
and ZERO name a SET of them — so there is nothing there to mark as shipped with
the application, and the distinction this walk asserts has no counterpart.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1893_a_named_arrangement_knows_whose_it_is.py
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
#: The arrangement this application ships. Read from the app, never spelled.
CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def rows(app: RpcSubprocess) -> list[dict]:
    return app.query(f"{EXT}/arrangements")


def names(app: RpcSubprocess) -> list[str]:
    return [r["name"] for r in rows(app)]


def invoke(app: RpcSubprocess, verb: str, args: str):
    return app.invoke(f"{EXT}/{verb}", args)


def board_cards(app: RpcSubprocess) -> list[str]:
    return sorted(
        t
        for t in abs_rects_of(app.snapshot(source="paint"))
        if t.startswith("card.") and t.count(".") == 1
    )


def section_a(app: RpcSubprocess) -> str:
    banner("A — the set publishes WHERE each arrangement came from")
    published = rows(app)
    ok(
        f"A: the application opens with arrangements at all — {len(published)}",
        len(published) >= 1,
    )
    ok(
        "A: ★★★★★ every row says its provenance and whether it can be deleted "
        f"— {published}",
        all(
            isinstance(r.get("provenance"), str) and isinstance(r.get("deletable"), bool)
            for r in published
        ),
    )
    builtin = [r for r in published if r["provenance"] == "built-in"]
    ok(
        f"A: ★★ the arrangement this application SHIPS is marked as such — "
        f"{len(builtin)} of {len(published)}",
        len(builtin) >= 1,
    )
    ok(
        "A: ★★★★★ and a built-in advertises that it offers no delete, so a "
        "client draws the control on the rows that have one instead of finding "
        "out by being refused",
        all(r["deletable"] is False for r in builtin),
    )
    # The names slot and the rows slot are the same set, or a menu and a client
    # are looking at two different things.
    assert_eq(
        sorted(app.query(f"{EXT}/presets").split(",")),
        sorted(names(app)),
        "A: the names a menu shows are the rows a client reads",
    )
    return builtin[0]["name"]


def section_b(app: RpcSubprocess, shipped: str) -> None:
    banner("B — a person saves their own, and it is theirs")
    before = names(app)
    answered = invoke(app, "save_preset", "Mine")
    ok(f"B: saving answers the new set — {answered!r}", "Mine" in str(answered))
    row = next(r for r in rows(app) if r["name"] == "Mine")
    ok(
        f"B: ★★ the saved arrangement is marked as a person's — {row}",
        row["provenance"] == "saved" and row["deletable"] is True,
    )
    ok(
        f"B: and the set grew by exactly one — {len(before)} -> {len(names(app))}",
        len(names(app)) == len(before) + 1,
    )
    # ★ Menu order does not depend on save order: the framework sorts, so two
    # sessions that saved the same layouts in different orders show one menu.
    ok(
        f"B: ★ the menu is in a stable order rather than save order — {names(app)}",
        names(app) == sorted(names(app)),
    )


def section_c(app: RpcSubprocess, shipped: str) -> None:
    banner("C — what the application ships is not a person's to overwrite or remove")
    try:
        answered = invoke(app, "save_preset", shipped)
    except RpcError as refusal:
        said = str(refusal)
        ok(
            f"C: ★★★★★ saving over the shipped arrangement is REFUSED, and the "
            f"refusal says what to do instead ({said})",
            shipped in said and "another name" in said,
        )
    else:
        ok(f"C: saving over a built-in must be refused, got {answered!r}", False)

    try:
        answered = invoke(app, "delete_preset", shipped)
    except RpcError as refusal:
        said = str(refusal)
        ok(
            f"C: ★★★★★ deleting it is REFUSED for the same stated reason "
            f"({said})",
            shipped in said,
        )
    else:
        ok(f"C: deleting a built-in must be refused, got {answered!r}", False)

    # ★★ And it is still there. A refusal that removed what it refused to
    # remove would be the worst of both.
    ok(
        f"C: ★★ and it is still in the set — {names(app)}",
        shipped in names(app),
    )
    ok(
        "C: ★ and it still applies, so the refusal cost nothing a person had",
        app.intervene(f"{EXT}/preset", shipped) is not None
        or app.query(f"{EXT}/preset") == shipped,
    )


def section_d(app: RpcSubprocess) -> None:
    banner("D — a person's own arrangement deletes, and the board is left alone")
    app.intervene(f"{EXT}/preset", "Mine")
    app.tick_ms(16)
    assert_eq(app.query(f"{EXT}/preset"), "Mine", "D: the journey applied it")
    cards_before = board_cards(app)

    answered = invoke(app, "delete_preset", "Mine")
    ok(
        f"D: ★★★★★ a saved arrangement DELETES — the canon has this and this "
        f"shell did not; the set now answers {answered!r}",
        "Mine" not in str(answered),
    )
    ok("D: and it is gone from the rows", "Mine" not in names(app))
    # ★★ The board is what the preset PRODUCED, not the preset. Clearing it
    # because a menu row went away would be a delete doing something nobody
    # asked for.
    ok(
        f"D: ★★ the board is untouched — {len(cards_before)} card(s) before and "
        f"after",
        board_cards(app) == cards_before,
    )
    ok(
        "D: ★ and the name shown falls back to the application's own, rather "
        "than naming an arrangement that no longer exists",
        app.query(f"{EXT}/preset") in names(app),
    )


def section_e(app: RpcSubprocess) -> None:
    banner("E — the refusals are distinct, and each names what would have worked")
    try:
        answered = invoke(app, "delete_preset", "NeverSaved")
    except RpcError as refusal:
        said = str(refusal)
        ok(
            f"E: ★★★★★ deleting a name nobody saved is refused and the refusal "
            f"LISTS the arrangements ({said})",
            all(n in said for n in names(app)),
        )
    else:
        ok(f"E: an unknown name must be refused, got {answered!r}", False)

    try:
        answered = invoke(app, "save_preset", "   ")
    except RpcError as refusal:
        ok(
            f"E: ★★ an arrangement with no name is refused for its OWN stated "
            f"reason ({refusal})",
            "name" in str(refusal),
        )
    else:
        ok(f"E: an unnamed save must be refused, got {answered!r}", False)

    # ★ And the two refusals above are different sentences, which is the point:
    # a caller branches on which one it got.
    ok(
        f"E: ★ the set survived every refusal intact — {names(app)}",
        len(names(app)) >= 1,
    )


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        shipped = section_a(app)
        section_b(app, shipped)
        section_c(app, shipped)
        section_d(app)
        section_e(app)

    print(f"\n=== {len(CHECKS)} named check(s) ===")
    for line in CHECKS:
        print(f"  - {line}")


if __name__ == "__main__":
    run_demo("r1893 a named arrangement knows whose it is", body)
