#!/usr/bin/env python3
"""R1927 §5.12 §5.2 — **a card carries a mark when something is wrong with it,
and the mark's colour says whether that something blocks.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability the two reference-census WARNING rows name — *should
this node show a visual warning* and *what does that warning say* — through the
node lab as it is mounted in the shell.

# ★★★★★ The canon gap this round found

The behaviour canon draws a small round mark on any card its validation names,
coloured by whether what it named is an error or a warning. This screen had no
such mark at all: everything it knew about a node's problems lived in the gate
panel, so a reader scanning the canvas could not see WHICH card was the subject
of a line in a list somewhere else. That is a reproduction gap and it is exactly
what the reference's per-node warning badge is.

# ★★★★★ What the reference does, measured at its own header and BOTH overriders

Its graph node publishes two independent overridable answers, and the census's
covering sentence for them was wrong in both clauses:

  * not a STATE — it is a const method computing a bool, asked every layout;
  * not something a KIND attaches to itself — one overrider answers from whether
    one of its own pins is wired plus a setting on its container, the other from
    the RUNNING node it is debugging. The kind supplies the rule; the answer is
    per node and situational.

And the third finding is what shaped the API: the two answers are independent,
so one of the two overriders overrides only the bool and leaves the text empty —
that node shows a badge with no reason in it.

# ★★★★★ What the first draft of this walk got wrong, and why it is written the
# other way round now

It opened by asserting that the register already names a card the model warns
about. Measured, the opening canvas names none: every listening card in the
specification's graph is dialled by something, so the rule fires nowhere. The
draft would have been repaired by loosening the assertion; what it actually
wanted was the stronger thing, which is to **make** the situation and watch the
answer change. A rule that is only ever observed in one state cannot be told
from a constant.

# What this walk holds

  (A) the register publishes one row per card, and on the opening canvas the
      MODEL warns about none of them — the graph as specified dials everything
      that listens.
  (B) ★★★★★ take away the one link that feeds a listening card and the model
      warns about exactly that card, WITH a sentence. There is no arrangement
      of this API in which it warns and says nothing.
  (C) ★★★★★ that card WEARS the mark, and a card with nothing wrong does not.
      Both directions, because a screen that marked everything would satisfy
      the first alone.
  (D) ★★★★★ the mark's colour separates blocking from not — the canon's own
      distinction — asserted as a PARTITION over the marked cards rather than
      against a written-down colour.
  (E) ★★★★★ the mark FOLLOWS the model: dial that card again and its warning
      goes, and the mark goes with it.
  (F) the cards the gate panel names and the cards wearing a mark are one set,
      because both are renderings of one walk.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1927_a_card_says_what_is_wrong_with_it.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, find_by_tag, run_demo  # noqa: E402

SHELL = "hello-analyzer-shell"
EXT = "/external"
SEAT = "lab"
VIEWPORT = (1400, 900)

#: The listening card this walk starves, and the card that feeds it. Named
#: rather than discovered so the walk fails loudly if the specification's
#: opening graph changes shape under it, instead of quietly finding another
#: pair and asserting something else.
STARVED = "P-03"
FEEDER = "R-01"

#: The two cards this walk makes answer to one identifier, to reach the
#: BLOCKING class. `id` is the path the settings schema declares unique.
TWINS = ("P-01", "P-02")

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


def register(app: RpcSubprocess, surface: str) -> dict:
    """What is wrong with each card, keyed by card name."""
    return {row["card"]: row for row in js(app.query(f"{surface}/wrong"))["cards"]}


def mark_fill(app: RpcSubprocess, card: str):
    """The ISSUE MARK's fill, or None when the card wears none.

    The fill and not the rectangle: what a mark means here is its colour, and
    R1919 measured what asking about a rectangle costs when the property lives
    somewhere else on the node.
    """
    snap = app.snapshot(source="paint", viewport=VIEWPORT)
    node = find_by_tag(snap, f"lab.node.{card}.issue")
    if node is None:
        return None
    return as_hex((node.get("style", {}) or {}).get("fill"))


def marks(app: RpcSubprocess, cards) -> dict:
    """One paint snapshot, read for every card — the whole canvas at once."""
    snap = app.snapshot(source="paint", viewport=VIEWPORT)
    found = {}
    for card in cards:
        node = find_by_tag(snap, f"lab.node.{card}.issue")
        found[card] = None if node is None else as_hex((node.get("style", {}) or {}).get("fill"))
    return found


def carries_the_models_sentence(app: RpcSubprocess, surface: str, reg: dict) -> None:
    """The gate panel shows the MODEL's sentence for a card, not a second wording.

    ★★★★★ Written as a helper with a **non-empty population check in front of
    it** because the first draft asked this question at the end of the walk,
    after the one warning had been dialled away — so `all(...)` over nothing
    answered true and a counterfactual that gave the screen its own wording for
    the model's finding went CAUGHT-less. An assertion whose population can be
    empty is not weaker than a wrong one; it is the same thing with a green
    light on it. So the population is asserted first, and this is called at the
    two moments the walk knows a warning is standing.
    """
    told = sorted(name for name, row in reg.items() if row["said"])
    ok(
        f"the model is warning about something right now, so what follows is "
        f"not a question asked of an empty set — {told}",
        told != [],
    )
    panel = js(app.query(f"{surface}/gate"))
    for name in told:
        said = reg[name]["said"]
        ok(
            f"★★★★★ the panel's line for {name} carries the MODEL's sentence "
            f"verbatim — {said!r}",
            any(
                line["sentence"].startswith(f"{name} · ") and said in line["sentence"]
                for line in panel
            ),
        )


def as_hex(colour) -> str | None:
    if isinstance(colour, str):
        text = colour.lstrip("#").upper()
        return text[:6] if len(text) >= 6 else None
    if isinstance(colour, dict):
        try:
            return "{:02X}{:02X}{:02X}".format(
                int(colour["r"]), int(colour["g"]), int(colour["b"])
            )
        except (KeyError, TypeError, ValueError):
            return None
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

        banner("A — what the model says is wrong, on the opening canvas")
        opening = register(app, surface)
        ok(f"A: one row per card — {sorted(opening)}", len(opening) >= 3)
        for name in (STARVED, FEEDER, *TWINS):
            ok(f"A: the walk's subject {name} is on this canvas", name in opening)
        told = sorted(name for name, row in opening.items() if row["said"])
        ok(
            "A: ★★★★★ the MODEL warns about none of them — the specified graph "
            f"dials everything that listens, so the rule fires nowhere: {told}",
            told == [],
        )
        ok(
            f"A: and the SCREEN already has findings of its own — "
            f"{sorted(n for n, r in opening.items() if r['problem'])}",
            any(row["problem"] for row in opening.values()),
        )
        ok(
            f"A: ★ {STARVED} is clean at open, so what happens to it next is "
            "caused by this walk and not inherited",
            not opening[STARVED]["problem"] and not opening[STARVED]["said"],
        )

        banner(f"B — ★★★★★ starve {STARVED}, and the model says so")
        gone = app.invoke(f"{surface}/delete_link", f"{FEEDER}>{STARVED}")
        app.tick_ms(16)
        ok(f"B: the link that fed it is gone — {gone!r}", isinstance(gone, str))
        starved = register(app, surface)
        said = starved[STARVED]["said"]
        ok(
            f"B: ★★★★★ the model now warns about {STARVED} — {said!r}",
            isinstance(said, str) and said.strip() != "",
        )
        ok(
            "B: ★★★★★ and the warning IS the sentence: a card the model warns "
            "about with nothing to say is not a state this API can reach",
            all(
                row["said"] is None or row["said"].strip() != ""
                for row in starved.values()
            ),
        )
        newly = sorted(
            name
            for name, row in starved.items()
            if row["said"] and not opening[name]["said"]
        )
        ok(
            f"B: ★ exactly the card this walk starved, and no other — {newly}",
            newly == [STARVED],
        )
        banner("B2 — the panel says what the MODEL said, not its own wording")
        carries_the_models_sentence(app, surface, starved)

        banner("C — ★★★★★ a card with a problem WEARS the mark")
        worn = marks(app, starved)
        wearing = sorted(name for name, fill in worn.items() if fill is not None)
        bare = sorted(name for name, fill in worn.items() if fill is None)
        ok(f"C: ★ some cards wear it — {wearing}", wearing != [])
        ok(
            f"C: ★★★★★ and some do not — {bare}. Without this a screen that "
            "marked everything would satisfy the line above",
            bare != [],
        )
        for name, row in sorted(starved.items()):
            ok(
                f"C: {name} wears the mark exactly when the screen says it has a "
                f"problem — problem={row['problem']!r}, mark={worn[name]!r}",
                (worn[name] is not None) == bool(row["problem"]),
            )
        ok(
            f"C: ★★★★★ and {STARVED} is one of them, so the model's answer "
            "reached the canvas and not only the wire",
            worn[STARVED] is not None,
        )

        banner("D — ★★★★★ the mark's colour separates blocking from not")
        ok(
            "D: nothing on this canvas blocks yet, so the second colour has to "
            "be made rather than found",
            not any(row["blocks"] for row in starved.values()),
        )
        app.invoke(f"{surface}/select", TWINS[1])
        held = app.invoke(f"{surface}/set_field", "id=b1")
        app.tick_ms(16)
        ok(
            f"D: ★ {TWINS[1]} now answers to the identifier {TWINS[0]} holds — "
            f"{held!r}",
            isinstance(held, str),
        )
        split = register(app, surface)
        worn = marks(app, split)
        blocking = {as_hex(worn[n]) for n, r in split.items() if r["blocks"]}
        warning = {
            as_hex(worn[n])
            for n, r in split.items()
            if r["problem"] and not r["blocks"]
        }
        ok(
            f"D: ★ both classes are present — blocking "
            f"{sorted(n for n, r in split.items() if r['blocks'])}, warning-only "
            f"{sorted(n for n, r in split.items() if r['problem'] and not r['blocks'])}",
            blocking != set() and warning != set(),
        )
        ok(
            f"D: ★★★★★ each class is drawn in ONE colour — blocking {blocking}, "
            f"warning {warning}",
            len(blocking) == 1 and len(warning) == 1,
        )
        ok(
            "D: ★★★★★ and the two colours are DIFFERENT, which is the whole of "
            f"what the canon's dot says — {blocking} vs {warning}",
            blocking.isdisjoint(warning),
        )
        ok(
            f"D: ★ {STARVED}'s own mark is the non-blocking one: the drawing is "
            "partial, not wrong",
            not split[STARVED]["blocks"] and as_hex(worn[STARVED]) in warning,
        )
        # ★ Asked a second time, in a canvas that now also holds a blocking
        # finding: the model's sentence must survive being one line among
        # several rather than the only thing the panel has to say.
        carries_the_models_sentence(app, surface, split)

        banner(f"E — ★★★★★ the mark FOLLOWS the model: dial {STARVED} again")
        app.invoke(f"{surface}/connect", f"{FEEDER},{STARVED}")
        app.tick_ms(16)
        after = register(app, surface)
        ok(
            f"E: ★★★★★ once something dials it, {STARVED} is no longer warned "
            f"about — was {said!r}, now {after[STARVED]['said']!r}",
            after[STARVED]["said"] is None,
        )
        ok(
            f"E: ★ and the screen has nothing else against it either — "
            f"problem={after[STARVED]['problem']!r}",
            not after[STARVED]["problem"],
        )
        ok(
            "E: ★★★★★ so the mark went with it — the paint is a rendering of "
            "the model rather than a second answer",
            mark_fill(app, STARVED) is None,
        )
        ok(
            "E: ★ and the cards this walk did not touch kept their marks, so "
            "the change was the model's and not a repaint of everything",
            mark_fill(app, TWINS[0]) is not None,
        )

        banner("F — the panel and the marks are one walk")
        final = register(app, surface)
        worn = marks(app, final)
        listed = {name for name, row in final.items() if row["problem"]}
        drawn = {name for name, fill in worn.items() if fill is not None}
        ok(
            f"F: ★★★★★ the cards the gate names and the cards wearing a mark are "
            f"the same set — listed {sorted(listed)}, drawn {sorted(drawn)}",
            listed == drawn,
        )
        panel = js(app.query(f"{surface}/gate"))
        named = {row["sentence"].split(" · ", 1)[0] for row in panel}
        ok(
            f"F: ★★★★★ and the gate PANEL's own lines name that same set — "
            f"{sorted(named)}",
            named == listed,
        )
        # ⚠ The sentence-identity question is NOT asked here. By this point the
        # walk has dialled its one model warning away, so the population is
        # empty and the question answers true without looking at anything — a
        # counterfactual that gave the screen its own wording went uncaught
        # exactly here. It is asked at B2 and again in D, where a warning is
        # standing, and `carries_the_models_sentence` refuses an empty set.

    print(f"\n{len(CHECKS)} check(s) held.")


sys.exit(run_demo("r1927_a_card_says_what_is_wrong_with_it", body))
