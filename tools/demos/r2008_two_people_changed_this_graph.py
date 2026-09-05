#!/usr/bin/env python3
"""R2008 §5.2 §5.11 — **two people changed this graph, and the screen says
where their changes meet.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability the reference-census row
`script_editor::BeginBlueprintMerge` names, on **screen A** — the node lab,
mounted whole into `hello-analyzer-shell`. It is the campaign's LAST absent
row: with it closed, both reference trees report `absent 0`.

# ★★★★★ Rule (9) — the row's own sentence was wrong in one clause

It read: *a three-way merge of two versions of a graph … no diff or merge
derivation is built on that.* Re-measured this round, a diff derivation was
already here — the link-layer answer, which says *in both / in the first only /
in the second only* over two link sets. That is this same existence question
one dimension narrower, and it is why the merge's per-tree answer reuses its
shape. **What was absent was the THIRD version and the join**, not diffing.

# ★★★★★ Three measured defects in the reference's conflict rule, one of which
its own source states

1. **The same change made by both is a conflict.** Its comment, beside the pin
   case: *given the wide variety of changes that can be made to a pin it is
   difficult to identify the change as identical, for now I'm just flagging all
   changes to the same pin as a conflict.*
2. **The harmless-change exclusion is asymmetric** — it asks whether the REMOTE
   difference is a move or a comment and never asks the local one, so *they
   moved it, I rewrote it* is excused and the mirror is a conflict.
3. **A change conflicts with at most one other.** The search stops at its first
   match and the map is keyed by a pointer to that one, so a second clash on
   the same subject passes as a clean change.

# ★★★★★ And a fourth this round found in ITSELF before the tests did

The first draft compared four of a card's eleven fields, so a bypass, a
switch-off, a re-parent, an authored value and an item edit — every one of them
meaning-bearing by its own field's documentation — met as *nothing changed*.
The difference functions now DESTRUCTURE, so a field added to a card cannot
compile until somebody places it on one side of the meaning/looks split.

# What this walk holds

  (A) the journey reaches the node lab, and the merge register answers.
  (B) ★★★★★ *nothing has been compared* and *the comparison found nothing* are
      different answers — the register says which.
  (C) ★★★★★ a peer with no base is REFUSED, with the verb that fixes it named.
  (D) ★★★★★ the same change on both sides is AGREEMENT, which the reference
      says out loud that it cannot give.
  (E) ★★★★★ and it is not passing on the KIND of change alone: the same kind
      with a different outcome is not agreement.
  (F) ★★★★★ a rewrite against a rename is a CONFLICT, a restyle is not, and
      each change publishes which it is.
  (G) ★★★★★ the merge is DERIVED, not latched: undoing the local edit on the
      canvas makes the conflict go away in front of the person.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r2008_two_people_changed_this_graph.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo  # noqa: E402

SHELL = "hello-analyzer-shell"
EXT = "/external"
SEAT = "lab"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def js(value):
    return json.loads(value) if isinstance(value, str) else value


def surface_of(app: RpcSubprocess, seat: str) -> str:
    published = js(app.query(f"{EXT}/destinations"))
    row = next(row for row in published["destinations"] if row["key"] == seat)
    return row["screen"]["address"]


def merging(app: RpcSubprocess, surface: str):
    return js(app.query(f"{surface}/merging"))


def meeting_on(report, card: str):
    """The one meeting whose subject is this card, or `None`."""
    for met in report["meetings"] or []:
        if met["at"]["name"] == card:
            return met
    return None


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        app.intervene(f"{EXT}/nav", SEAT)
        app.tick_ms(16)
        ok(
            "the journey reaches the node lab, so what follows is about the "
            "ASSEMBLED tool",
            app.query(f"{EXT}/nav") == SEAT,
        )
        surface = surface_of(app, SEAT)

        banner("A — the merge register answers")
        report = merging(app, surface)
        ok(
            f"A: it carries the three versions' standing and the join — "
            f"{sorted(report)}",
            {"base", "peer", "trees", "remote", "local", "meetings", "clean"}
            <= set(report),
        )

        banner("B — ★★★★★ nothing compared is not the same as nothing found")
        ok(
            "B: ★★★★★ before anybody names a base, the join is `null` rather "
            "than an empty list — a screen that spelled those the same way "
            "would tell a person their graphs agree when nobody has looked",
            report["meetings"] is None and report["clean"] is None,
        )
        ok(
            f"B: ★ and it says which of the two versions is missing — "
            f"base={report['base']} peer={report['peer']}",
            report["base"] is False and report["peer"] is False,
        )

        banner("C — ★★★★★ a peer with no base is refused, and the refusal is actionable")
        try:
            app.invoke(f"{surface}/take_peer", "")
            refusal = None
        except RpcError as exc:
            refusal = str(exc)
        ok(
            f"C: ★★★★★ refused, because a peer with no base is two documents "
            f"and no way to tell who changed what — {refusal!r}",
            refusal is not None,
        )
        ok(
            "C: ★ and it names the verb that fixes it, so the refusal reaches "
            "somebody who can act on it",
            refusal is not None and "keep_base" in refusal,
        )

        banner("D — ★★★★★ the same change on both sides is AGREEMENT")
        # The base is the canon topology as it stands. The peer is the same
        # bytes, so `take_peer` gives two sides that started level.
        app.invoke(f"{surface}/keep_base", "")
        app.tick_ms(16)
        saved = app.query(f"{surface}/archive")
        app.invoke(f"{surface}/take_peer", saved)
        app.tick_ms(16)
        report = merging(app, surface)
        ok(
            f"D: three versions named, and nothing differs yet — "
            f"remote={len(report['remote'])} local={len(report['local'])}",
            report["base"] and report["peer"] and report["clean"] is True,
        )
        ok(
            "D: ★ every tree is held by all three, which is the existence axis "
            "the reference's view draws as three boxes per graph",
            all(
                row["in_base"] and row["in_peer"] and row["here"]
                for row in report["trees"]
            ),
        )

        # Now write a note on one card and hand THAT over as the peer, so the
        # two sides carry the same change against the base they share.
        cards = [row["card"] for row in js(app.query(f"{surface}/stand_ins"))["cards"]]
        subject, other = cards[0], cards[1]
        app.invoke(f"{surface}/note", f"{subject},what we both wrote")
        app.tick_ms(16)
        app.invoke(f"{surface}/take_peer", app.query(f"{surface}/archive"))
        app.tick_ms(16)
        report = merging(app, surface)
        met = meeting_on(report, subject)
        ok(
            f"D: ★ one subject, touched by both sides — {met}",
            met is not None,
        )
        ok(
            f"D: ★★★★★ both wrote the same thing, so it is AGREED — the "
            f"reference reports this as a conflict and its own comment says "
            f"why it cannot do better — meet={met['meet']}",
            met["meet"] == "agreed",
        )
        ok(
            f"D: ★ and nobody has to decide anything — conflicts="
            f"{report['conflicts']} clean={report['clean']}",
            report["conflicts"] == 0 and report["clean"] is True,
        )

        banner("E — ★★★★★ the same KIND of change with a different outcome is not agreement")
        app.invoke(f"{surface}/note", f"{subject},what only I wrote")
        app.tick_ms(16)
        report = merging(app, surface)
        met = meeting_on(report, subject)
        ok(
            f"E: ★★★★★ two notes, one kind, two sentences — reading the word "
            f"alone would have called this agreement — peer={met['peer']} "
            f"here={met['here']} meet={met['meet']}",
            met["peer"] == "renamed"
            and met["here"] == "renamed"
            and met["meet"] == "harmless",
        )
        ok(
            "E: ★ and neither side changed what the graph MEANS, so it is "
            "still nothing a person has to adjudicate",
            report["clean"] is True,
        )

        banner("F — ★★★★★ a rewrite against a rename is a conflict")
        # The peer renamed this card; this side now switches it off, which the
        # card's own field documentation calls the one fact other than its body
        # and its links that changes what the graph means.
        app.invoke(f"{surface}/disable", subject)
        # ★ And a second card touched only HERE, so `local` carries a change
        # with no meeting — the looks-only arm, which is the half of the split
        # that must NOT reach a conflict.
        app.invoke(f"{surface}/tint", f"{other},#c08a3e")
        app.tick_ms(16)
        report = merging(app, surface)
        met = meeting_on(report, subject)
        ok(
            f"F: ★★★★★ the local change is STRUCTURAL — a switch-off is one of "
            f"the five fields the first draft of this merge could not see at "
            f"all, and it met them as nothing changed — here={met['here']}",
            met["here"] == "rewritten",
        )
        ok(
            f"F: ★★★★★ so a rename against it is a CONFLICT — where the "
            f"reference excuses exactly this pair whenever the harmless half "
            f"happens to be on the remote side — meet={met['meet']}",
            met["meet"] == "conflict",
        )
        ok(
            f"F: ★ and the merge is not clean — conflicts={report['conflicts']}",
            report["conflicts"] == 1 and report["clean"] is False,
        )
        by_card = {row["at"]["name"]: row for row in report["local"]}
        ok(
            f"F: ★★★★★ the change list publishes WHICH changes carry meaning, "
            f"so a client need not keep a second copy of the rule that decides "
            f"every conflict — {subject}={by_card[subject]['what']} "
            f"{other}={by_card[other]['what']}",
            by_card[subject]["structural"] is True
            and by_card[other]["structural"] is False
            and by_card[other]["what"] == "restyled",
        )
        ok(
            f"F: ★ and the card only this side touched has no meeting at all, "
            f"because a meeting is a subject BOTH sides changed",
            meeting_on(report, other) is None,
        )

        banner("G — ★★★★★ the merge is derived, so resolving it is visible as it happens")
        app.invoke(f"{surface}/disable", subject)
        app.tick_ms(16)
        report = merging(app, surface)
        met = meeting_on(report, subject)
        ok(
            f"G: ★★★★★ switching the card back on made the conflict go away "
            f"with no merge verb run at all — a latched report would still be "
            f"showing it — meet={met['meet'] if met else None}",
            met is not None and met["meet"] == "harmless",
        )
        ok(
            f"G: ★ and the whole merge is clean again — conflicts="
            f"{report['conflicts']}",
            report["conflicts"] == 0 and report["clean"] is True,
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r2008 two people changed this graph", body)
