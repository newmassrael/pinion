#!/usr/bin/env python3
"""R1717 §5.21 §5.12 §2 #7 — **one configuration key, two contributors**, on the
analysis tool's node-graph screen.

# What this exists for

R1716 gave a settings row a provenance: somebody wrote it, or the screen worked
it out. That vocabulary has two answers and the behaviour canon's own screen
needs three. A node may be told to dial an already-running peer this canvas does
not draw *and* be wired to the peers it does draw, and those are not competing
answers to one question — they are two contributions to one list.

R1716 could not say that, so a written address took the whole row and the drawn
links stopped reaching the configuration. It paid for the loss with a gate
warning naming each drawn address the card no longer dialled. That warning was
compensation, and this round removes the thing it was compensating for.

# The floor this is built to beat, measured rather than read

A probe was built against the mature toolkit 6.11.1 and **run** offscreen. It
does compose two contributors — a settings store falls back to a second store,
a key only the second one holds still answers, and taking the written half away
brings the other back — so the capability is parity, not a gap. Four things it
does not do, every one of them measured:

  * the composition is **whole-key**: a written one-element list beside a
    worked-out three-element list answered **one** element. One store wins the
    key entire; there is no union;
  * **no reader can ask which store answered.** The file name is the written
    store's whatever answered and the scope is the asking object's; telling them
    apart needs a second object with fallbacks switched off, and it answers
    about the KEY, not about the value;
  * the binding half **ends on a contribution**: writing into a derived value
    cleared its derivation, and after the source moved 2 -> 3 elements the value
    did not follow;
  * a cell holding a composed value answers **2 of 256** standard roles, and
    none of them is how much of it is not the reader's.

Six capabilities this round ships are compile errors there: naming which
contributor answered a key, composing a list key element-wise, reading both
halves of one key at once, contributing to a derived value without ending the
derivation, marking part of a cell's value not the reader's, and counting the
elements that came from elsewhere.

# What it asserts

* **A** — the wire publishes both halves. `written` beside `source` is the whole
  answer: neither alone, and neither recoverable from the shown value.
* **B** — ★★★★★ the headline: a written address and every drawn one stand in
  one row, written first, and the exported configuration ships both.
* **C** — ★★★★★ the canvas keeps reaching a row somebody owns half of. Drawing
  a link moves it; undrawing takes the address back out; the written half never
  moves.
* **D** — ★★★★★ `edited` is a question about the written half. A row that moved
  because the canvas moved was not edited by anybody.
* **E** — the third act. The seat gives the written half back, the row STAYS,
  and the router resolves that rectangle to `disown:<key>` and not to `remove`.
* **F** — the badges. A shared row keeps the applies badge AND names its source
  with the count, and a reader who cannot see them hears the same.
* **G** — what the gate says now: the fact underneath R1716's warning, which is
  an address nothing in this graph listens on.
* **H** — a single-valued row refuses to have two contributors at all.

>= 30 assertions.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    access_node_by_tag,
    assert_action_refused,
    assert_eq,
    run_demo,
)

EXAMPLE = "hello-node-lab"
EXT = "/external"
VIEWPORT = (1440, 900)

#: An address at a host nothing in the opening graph listens on. It is the one
#: the behaviour canon's own warning is about.
OUTSIDE = "tcp/10.0.0.21:7449"

CHECKS: list[str] = []


def ok(what: str, condition: bool) -> None:
    assert condition, f"FAILED: {what}"
    CHECKS.append(what)


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def rows(tf) -> list[dict]:
    return json.loads(tf.query(f"{EXT}/form"))


def row(tf, key: str) -> dict:
    found = [r for r in rows(tf) if r["key"] == key]
    assert found, f"no row {key!r} on the selected card"
    return found[0]


def parts(text: str) -> list[str]:
    return [piece.strip() for piece in text.split(",") if piece.strip()]


def rects(tf) -> dict:
    return abs_rects_of(tf.snapshot(source="paint", viewport=VIEWPORT))


def document(tf) -> dict:
    return json.loads(tf.query(f"{EXT}/document"))


def gate_text(tf) -> str:
    return json.dumps(json.loads(tf.query(f"{EXT}/gate")), ensure_ascii=False)


def share_the_connect_row(tf) -> list[str]:
    """Reach the shared state the way a person does, and answer the drawn set.

    Two acts, both the screen's own: take the wire's row over — which seeds it
    with what the canvas was saying — then write an address of your own over
    that seed. The wires come straight back as the other half.
    """
    tf.invoke(f"{EXT}/select", "R-01")
    drawn = parts(row(tf, "connect.endpoints")["value"])
    tf.invoke(f"{EXT}/author_field", "connect.endpoints")
    tf.invoke(f"{EXT}/set_field", f"connect.endpoints={OUTSIDE}")
    return drawn


# ── A: both halves are on the wire ──────────────────────────────────────────


def a_the_wire_publishes_both_halves(tf) -> None:
    banner("A — the wire publishes what was written and what was worked out")
    opening = row(tf, "connect.endpoints")
    assert_eq(opening["source"], "wire", "A: the opening row is the canvas's alone")
    assert_eq(
        opening["written"],
        None,
        "A: ★ and nobody wrote any of it — which `source` alone could not say, "
        "because a shared row names a source too",
    )
    assert_eq(
        opening["derived_elements"],
        0,
        "A: a row with one contributor counts nothing as somebody else's",
    )
    written = row(tf, "id")
    assert_eq(written["source"], None, "A: a row somebody wrote names no source")
    assert_eq(written["written"], written["value"], "A: and its halves are the same thing")
    drawn = share_the_connect_row(tf)
    shared = row(tf, "connect.endpoints")
    assert_eq(shared["source"], "wire", "A: ★★ a shared row names what it shares with")
    assert_eq(shared["written"], OUTSIDE, "A: ★★ and what somebody wrote, exactly")
    ok(
        f"A: ★★★★★ neither half is recoverable from the shown value "
        f"({shared['value']!r}) — the floor publishes one string and calls it "
        f"the answer",
        shared["value"] != shared["written"] and OUTSIDE in shared["value"],
    )
    assert_eq(
        shared["derived_elements"],
        len(drawn),
        "A: ★★ and the count says how many of them are the canvas's — the "
        "number a cell there answers none of 256 roles with",
    )


# ── B: the composition, and what ships ──────────────────────────────────────


def b_one_row_holds_both_contributions(tf) -> None:
    banner("B — a written address and every drawn one, in one row")
    tf.invoke(f"{EXT}/select", "R-01")
    shown = parts(row(tf, "connect.endpoints")["value"])
    ok(
        f"B: ★★★★★ the row holds both contributions ({shown})",
        shown[0] == OUTSIDE and len(shown) == 3,
    )
    ok(
        "B: written first — the half a person is looking for is the half they "
        "meet first",
        shown[0] == OUTSIDE,
    )
    assert_eq(
        document(tf)["connect"]["endpoints"],
        shown,
        "B: ★★★★★ and the exported configuration ships both — the picture and "
        "the file say the same thing, which is what R1716 could not do",
    )
    tf.invoke(f"{EXT}/export", "")
    produced = json.loads(tf.query(f"{EXT}/produced"))
    assert_eq(
        produced["config"]["nodes"]["R-01"]["connect"]["endpoints"],
        shown,
        "B: ★★★ and the PLAN ships the same list — two derivations of one fact "
        "is the failure this screen keeps finding",
    )
    # An address said twice is one address. The canvas draws to `shown[1]`, so
    # writing it as well must not double it.
    tf.invoke(f"{EXT}/set_field", f"connect.endpoints={OUTSIDE}, {shown[1]}")
    both = parts(row(tf, "connect.endpoints")["value"])
    assert_eq(
        both,
        [OUTSIDE, shown[1], shown[2]],
        "B: ★★ an address both halves name appears once, in the place the "
        "written half puts it",
    )
    assert_eq(
        row(tf, "connect.endpoints")["derived_elements"],
        1,
        "B: and only the one the canvas alone says is counted as its",
    )
    tf.invoke(f"{EXT}/set_field", f"connect.endpoints={OUTSIDE}")


# ── C: the canvas keeps reaching it ─────────────────────────────────────────


def c_the_canvas_keeps_reaching_a_row_somebody_owns_half_of(tf) -> None:
    banner("C — drawing a link still moves a row somebody owns half of")
    tf.invoke(f"{EXT}/select", "R-01")
    before = row(tf, "connect.endpoints")
    # Give the card this dials a second address to land on, then draw the link.
    tf.invoke(f"{EXT}/select", "P-02")
    tf.invoke(f"{EXT}/set_field", "listen.endpoints=tcp/0.0.0.0:7449, tcp/0.0.0.0:7450")
    tf.invoke(f"{EXT}/select", "R-01")
    tf.invoke(f"{EXT}/connect", "R-01,P-02")
    grown = row(tf, "connect.endpoints")
    ok(
        f"C: ★★★★★ the drawn link reaches a row somebody owns half of "
        f"({parts(grown['value'])})",
        len(parts(grown["value"])) == len(parts(before["value"])) + 1,
    )
    assert_eq(
        grown["written"],
        before["written"],
        "C: ★★★ and their half did not move — R1716's take-over froze the "
        "canvas into it and this does not",
    )
    assert_eq(
        grown["derived_elements"],
        before["derived_elements"] + 1,
        "C: the count follows the drawing",
    )
    assert_eq(
        document(tf)["connect"]["endpoints"],
        parts(grown["value"]),
        "C: and the configuration follows in the same act",
    )
    drawn = [
        link
        for link in json.loads(tf.query(f"{EXT}/links"))
        if link["from"] == "R-01"
        and link["to"] == "P-02"
        and link["endpoint"].endswith(":7450")
    ]
    assert drawn, "the link this section drew is in the model"
    tf.invoke(f"{EXT}/delete_link", str(drawn[0]["id"]))
    assert_eq(
        parts(row(tf, "connect.endpoints")["value"]),
        parts(before["value"]),
        "C: ★ and undrawing it takes the address back out of the row",
    )


# ── D: edited is about the written half ─────────────────────────────────────


def d_a_moving_derivation_is_not_somebody_editing(tf) -> None:
    banner("D — a row that moved because the canvas moved was not edited")
    tf.invoke(f"{EXT}/select", "R-01")
    # Settle the form, so what follows is measured from a clean baseline.
    tf.invoke(f"{EXT}/run", "")
    tf.invoke(f"{EXT}/run", "")
    settled = row(tf, "connect.endpoints")
    assert_eq(settled["edited"], False, "D: nothing is pending after a launch")
    tf.invoke(f"{EXT}/select", "P-02")
    tf.invoke(f"{EXT}/set_field", "listen.endpoints=tcp/0.0.0.0:7449, tcp/0.0.0.0:7460")
    tf.invoke(f"{EXT}/select", "R-01")
    tf.invoke(f"{EXT}/connect", "R-01,P-02")
    moved = row(tf, "connect.endpoints")
    ok(
        f"D: the shown value moved ({len(parts(moved['value']))} addresses, was "
        f"{len(parts(settled['value']))})",
        len(parts(moved["value"])) > len(parts(settled["value"])),
    )
    assert_eq(
        moved["edited"],
        False,
        "D: ★★★★★ and that is not an edit — a form that said it was would offer "
        "a 'put it back' that puts nothing back",
    )
    tf.invoke(f"{EXT}/set_field", f"connect.endpoints={OUTSIDE}, tcp/mine:1")
    assert_eq(
        row(tf, "connect.endpoints")["edited"],
        True,
        "D: ★★ this is — the written half moved",
    )
    drawn = [
        link
        for link in json.loads(tf.query(f"{EXT}/links"))
        if link["from"] == "R-01"
        and link["to"] == "P-02"
        and link["endpoint"].endswith(":7460")
    ]
    assert drawn, "the link this section drew is in the model"
    tf.invoke(f"{EXT}/delete_link", str(drawn[0]["id"]))
    tf.invoke(f"{EXT}/set_field", f"connect.endpoints={OUTSIDE}")


# ── E: the third act ────────────────────────────────────────────────────────


def e_the_seat_gives_the_written_half_back(tf) -> None:
    banner("E — the seat gives their half back, and the row stays")
    tf.invoke(f"{EXT}/select", "R-01")
    seats = rects(tf)
    ok(
        "E: ★★ the seat on a shared row is neither of the other two acts",
        "lab.form.disown.connect.endpoints" in seats
        and "lab.form.remove.connect.endpoints" not in seats
        and "lab.form.author.connect.endpoints" not in seats,
    )
    seat = seats["lab.form.disown.connect.endpoints"]
    centre = (seat[0] + seat[2] // 2, seat[1] + seat[3] // 2)
    assert_eq(
        tf.invoke(f"{EXT}/point", f"{centre[0]},{centre[1]}"),
        "disown:connect.endpoints",
        "E: ★★★ the router resolves that rectangle to giving the half back — a "
        "press answered 'remove' over a row that stays would read as a tool "
        "that ignored it",
    )
    node = access_node_by_tag(
        tf.request("scene/access").result, "lab.form.disown.connect.endpoints"
    )
    assert node is not None, "the seat is in the accessibility tree"
    ok(
        f"E: and a reader hears the act rather than the glyph ({node['name']!r})",
        "give back" in node["name"],
    )
    tf.click(centre)
    back = row(tf, "connect.endpoints")
    assert_eq(
        back["written"],
        None,
        "E: ★★★★★ their half is gone",
    )
    assert_eq(
        back["source"],
        "wire",
        "E: ★★ and the row STAYS, worked out from the wires — the derivation "
        "was still true, so a removed row would be back one render later",
    )
    # ★★★ The WHOLE sentence, not a substring of it. This round's own defect
    # was a launch-panel line that read as nonsense while every check over it
    # asked whether a word was present — so a sentence a person reads is asserted
    # entire, and a reworded one has to come back here.
    assert_eq(
        tf.query(f"{EXT}/toast"),
        "connect.endpoints is the wire's again",
        "E: ★★★ the screen says which act happened, in the words it happened in",
    )
    assert_action_refused(
        lambda: tf.invoke(f"{EXT}/remove_field", "connect.endpoints"),
        saying="wire",
    )
    ok("E: and a second press has nothing left to take", True)


# ── F: the badges say both things ───────────────────────────────────────────


def f_a_shared_row_carries_both_badges(tf) -> None:
    banner("F — a shared row says what an edit costs AND where the rest came from")
    tf.invoke(f"{EXT}/select", "R-01")
    derived_only = rects(tf)
    ok(
        "F: a row with one contributor that nobody can edit shows its source",
        "lab.form.source.connect.endpoints" in derived_only,
    )
    drawn = share_the_connect_row(tf)
    shared = rects(tf)
    ok(
        "F: ★★★★★ a shared row shows BOTH — a reader may still type here, so "
        "what an edit costs is news, and part of what they read is not theirs, "
        "so where it came from is news too",
        "lab.form.applies.connect.endpoints" in shared
        and "lab.form.source.connect.endpoints" in shared,
    )
    access = tf.request("scene/access").result
    # ★ R2050 — the address the screen publishes for that row's control.
    control = access_node_by_tag(
        access,
        next(
            row["control"]
            for row in json.loads(tf.query(f"{EXT}/form"))
            if row["key"] == "connect.endpoints"
        ),
    )
    assert control is not None, "the control is in the tree"
    assert_eq(
        control.get("state", {}).get("read_only", False),
        False,
        "F: ★★★ a shared row is writable, so it is NOT announced read-only — "
        "which a row nobody wrote is",
    )
    # ★★★★★ And the line-by-line half of the same fact: an element the canvas
    # contributed is a read-out and says what worked it out, because a reader
    # who cannot see that its box has no fill has no other way to learn that
    # this one line will not take an edit.
    # One address was written, so line 0 is theirs and every line after it is
    # the canvas's.
    mine = access_node_by_tag(access, "lab.form.item.connect.endpoints.0")
    theirs = access_node_by_tag(access, "lab.form.item.connect.endpoints.1")
    assert mine is not None and theirs is not None, "both lines are in the tree"
    assert_eq(
        mine.get("state", {}).get("read_only", False),
        False,
        "F: ★★ the line they wrote is theirs to type in",
    )
    assert_eq(
        theirs.get("state", {}).get("read_only"),
        True,
        "F: ★★★★★ and the line the canvas drew is not — provenance reaches the "
        "ELEMENT, because editing does",
    )
    ok(
        f"F: which the reader is told in the same breath ({theirs['name']!r})",
        "worked out from the wire" in theirs["name"],
    )
    runs = json.dumps(tf.snapshot(source="paint", viewport=VIEWPORT), ensure_ascii=False)
    ok(
        f"F: ★★ and the source badge carries the COUNT ({len(drawn)}) — the "
        f"figure the floor answers none of 256 roles with",
        f"wire {len(drawn)}" in runs,
    )


# ── G: what the gate says now ───────────────────────────────────────────────


def g_the_gate_says_the_fact_underneath(tf) -> None:
    banner("G — the surviving warning: this card dials outside the graph")
    tf.invoke(f"{EXT}/select", "R-01")
    findings = gate_text(tf)
    drawn = parts(row(tf, "connect.endpoints")["value"])
    ok(
        f"G: ★★★ the address nothing in this graph listens on is named ({OUTSIDE})",
        OUTSIDE in findings and "nothing here listens on" in findings,
    )
    # ★★★★★ And the sentence READS — the card named once, and a wording about
    # the graph rather than about unknown keys. R1716's version said
    # "R-01 · R-01 · … is outside this graph is not a key the target knows"
    # for a whole round with every gate green, because nothing asked this.
    said = [
        line["sentence"]
        for line in json.loads(tf.query(f"{EXT}/gate"))
        if OUTSIDE in line["sentence"]
    ]
    assert_eq(len(said), 1, "G: exactly one line is about it")
    assert_eq(
        said[0].count("R-01"),
        1,
        f"G: ★★★★★ and the card is named ONCE in it: {said[0]!r}",
    )
    ok(
        "G: with a sentence about the graph, not about a key the target does "
        "not know",
        "is not a key the target knows" not in said[0],
    )
    ok(
        "G: ★★★★★ and no drawn address is — they are in the row by "
        "construction, so R1716's warning could never fire again",
        not any(address in findings for address in drawn if address != OUTSIDE),
    )
    verdict = json.loads(tf.query(f"{EXT}/verdict"))
    ok(
        f"G: ★★ it warns and does not block — a node may legitimately be told "
        f"to reach an already-running peer ({verdict})",
        verdict["may_launch"],
    )
    tf.invoke(f"{EXT}/remove_field", "connect.endpoints")
    ok(
        "G: and giving the half back takes the warning away",
        OUTSIDE not in gate_text(tf),
    )


# ── H: a single value cannot have two contributors ──────────────────────────


def h_a_single_valued_row_refuses_two_contributors(tf) -> None:
    banner("H — two contributions to one mode contradict; they do not compose")
    tf.invoke(f"{EXT}/select", "R-01")
    mode = row(tf, "mode")
    assert_eq(mode["source"], "role", "H: it is worked out from the role")
    assert_eq(mode["written"], None, "H: and nobody wrote any of it")
    tf.invoke(f"{EXT}/author_field", "mode")
    taken = row(tf, "mode")
    assert_eq(
        taken["source"],
        None,
        "H: ★★★★★ taking a single-valued row over leaves NOTHING deriving it — "
        "a mode that was the role's and somebody's at once would be a third "
        "value neither of them said",
    )
    assert_eq(taken["written"], taken["value"], "H: the whole row is theirs")
    assert_eq(
        taken["derived_elements"],
        0,
        "H: with nothing in it from anywhere else",
    )
    seats = rects(tf)
    ok(
        "H: ★★ and its seat is the remove, not the give-back — there is nobody "
        "to give it back to",
        "lab.form.remove.mode" in seats and "lab.form.disown.mode" not in seats,
    )
    tf.invoke(f"{EXT}/remove_field", "mode")
    assert_eq(
        row(tf, "mode")["source"],
        "role",
        "H: ★ and removing it hands the row back to the role",
    )


def main() -> int:
    with RpcSubprocess(EXAMPLE) as tf:
        a_the_wire_publishes_both_halves(tf)
        b_one_row_holds_both_contributions(tf)
        c_the_canvas_keeps_reaching_a_row_somebody_owns_half_of(tf)
        d_a_moving_derivation_is_not_somebody_editing(tf)
        e_the_seat_gives_the_written_half_back(tf)
        f_a_shared_row_carries_both_badges(tf)
        g_the_gate_says_the_fact_underneath(tf)
        h_a_single_valued_row_refuses_two_contributors(tf)
    print(f"\n{len(CHECKS)} named check(s) beyond the equalities:")
    for line in CHECKS:
        print(f"  - {line}")
    return 0


if __name__ == "__main__":
    run_demo("r1717_a_row_can_have_two_contributors", main)
