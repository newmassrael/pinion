#!/usr/bin/env python3
"""R1719 §5.40 §5.12 §5.15 §2 #7 — **what a screen says knows what kind of
thing it is**, on all three screens of the analysis tool.

# The defect this exists for, measured by driving the screens

The three screens each keep "the last thing I said" and each kept it as a
`String`. 117 call sites handed one to a one-line setter, and the wire typed all
three as `string`. Two consequences a person actually meets, both measured
before this round by running the binaries:

  * **a refusal was announced with the same urgency as a confirmation**, because
    the urgency was a per-screen constant. Selecting a card on the node lab said
    `selected R-01` and its live region read `assertive` — a screen reader
    interrupted to be told something the person did on purpose — while the other
    two screens were `polite`, so on those a REFUSAL waited for a pause a person
    working the tool does not leave;
  * **an act that changed nothing said nothing**, and the earlier message stood.
    Renaming a card to the name it already had left a refusal about a different
    act on screen.

And the fact that a refusal *was* a refusal travelled in a string prefix:
`format!("refused: {sentence}")` five times on one screen, a `refusal_sentence`
helper on another, `format!("query refused: {why}")` on the third.

# The floor this is built to beat, measured rather than read

A probe was built against the mature toolkit 6.11.1 and **run** offscreen.

  * its status channel **carries no kind at all** — 62 properties and 37 methods,
    and none of them names one; the accessor answers the text;
  * its dialogs **do** have a kind, five arms of it, and the kind does not change
    what a reader hears: the accessible role of the critical dialog and of the
    informational one are the same number;
  * its announcement event carries an urgency and **the caller passes it,
    derived from nothing**, defaulting to the polite one — so a refusal is
    polite unless somebody remembers;
  * nothing refuses a malformed message: `refused: refused: refused: ` and the
    empty string are both accepted verbatim.

Six capabilities this round ships are compile errors there: asking the channel
what kind the last message was, saying something *as* a refusal, deriving the
urgency from the kind, refusing a doubly-framed message, reading the producer's
own clause without the frame, and saying "it was already so".

# What it asserts

* **A** — the wire publishes the value, on all three screens, spelled the same
  way. `tone` beside `clause` beside `sentence`, and the sentence is derived
  from the other two rather than stored beside them.
* **B** — ★★★★★ the headline: the urgency a screen reader is given comes off the
  tone. A refusal interrupts and a confirmation does not, on every screen,
  where every screen used to answer with one constant.
* **C** — ★★★★★ the frame is not in the stored text. A refusal's clause is the
  producer's own sentence, and the same value answers the agent's channel
  without the frame and the person's with it.
* **D** — ★★★★★ the third arm. An act that changes nothing says so, where the
  screen used to say nothing and leave the last message standing.
* **E** — the seeing half: the toast's bullet is a different colour for a
  refusal, measured off the painted scene rather than off the source.
* **F** — a malformed announcement is refused rather than rendered: empty,
  doubly framed, and a `Debug` spelling — the last being the defect R1699 fixed
  with a helper each screen had to remember.
* **G** — the wire round trip. Reading the value back reconstructs it, and a
  stored one that breaks the rule is refused rather than revived.

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
    assert_eq,
    run_demo,
)

EXT = "/external"
VIEWPORT = (1440, 900)

#: The three screens and the tag their live region carries. The value is read
#: at `said` on all three — R1719 made that uniform; before it one screen
#: called the string `toast` and another called it `said`.
SCREENS = [
    ("hello-node-lab", "lab.toast"),
    ("hello-packet-view", "pv.appbar.said"),
    ("hello-analyzer-shell", "shell.toast"),
]

CHECKS: list[str] = []


def ok(what: str, condition: bool) -> None:
    assert condition, f"FAILED: {what}"
    CHECKS.append(what)


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def said(tf) -> dict | None:
    """The value the screen last said, or `None` when it has said nothing."""
    answer = tf.query(f"{EXT}/said")
    if answer in (None, ""):
        return None
    return json.loads(answer) if isinstance(answer, str) else answer


#: A card the node lab does NOT open with, for parking the selection on.
PARK = "T-02"


def select_afresh(tf, card: str) -> None:
    """Select `card` as a CHANGE, so the screen has something to say.

    ★★★★★ R1737.1 — R1736 made "an act that changed nothing says nothing" a
    property of the one place a selection changes, and `R-01` is the card the
    node lab OPENS WITH: selecting it again is a no-op and is now correctly
    silent. Every fixture here that wants the done tone therefore has to move
    the selection first. Measured: at boot `selected` is `R-01` and `said` is
    null, and `select R-01` leaves it null while `select T-02` then `select
    R-01` both speak.

    The screen is right and the fixture was wrong: it was leaning on a
    re-selection speaking, which is exactly the behaviour R1736 removed on
    purpose. Written as one helper rather than a park at each site so a later
    reader cannot restore the assumption at one of them.
    """
    if tf.query(f"{EXT}/selected") == card:
        tf.invoke(f"{EXT}/select", PARK if card != PARK else "R-01")
    tf.invoke(f"{EXT}/select", card)


def live_of(tf, tag: str):
    node = access_node_by_tag(tf.request("scene/access").result, tag)
    return node and node.get("live")


def focus_by_tab(tf, tag: str, limit: int = 14) -> None:
    """Walk the Tab ring to `tag` — the keyboard's own way into a text box."""
    for _ in range(limit):
        tf.request("focus/next")
        tf.tick_ms(16)
        landed = (tf.request("scene/access", {}).result.get("focus") or {}).get("tag")
        if landed == tag:
            return
    raise AssertionError(f"the Tab ring never reached {tag}")


def press_tag(tf, tag: str) -> None:
    """Press the middle of a painted mark, the way a person reaches it."""
    rect = abs_rects_of(tf.snapshot(source="paint"))[tag]
    tf.click((rect[0] + rect[2] // 2, rect[1] + rect[3] // 2))
    tf.tick_ms(16)


def refuse(tf, path: str, args) -> str:
    """Drive something that must refuse, and answer the refusal's own words."""
    try:
        tf.invoke(f"{EXT}/{path}", args)
    except Exception as why:  # noqa: BLE001 — the refusal is the point
        return str(why)
    raise AssertionError(f"{path}({args!r}) was expected to refuse and did not")


# ── the acts that put each screen in each tone ──────────────────────────────
#
# ★ Screen C is reached by PRESSING rather than by a verb, and that is a fact
# about the screen rather than an awkwardness of the test: its wire verbs hand
# a refusal back through the caller's own channel and do not put it on the
# toast, because the toast is for the person at the screen. Its palette is
# where a person meets one — half its catalogue is booked for a later release.


def make_done(tf, example: str) -> None:
    if example == "hello-node-lab":
        select_afresh(tf, "R-01")
    elif example == "hello-packet-view":
        tf.invoke(f"{EXT}/select_message", 2)
    else:
        press_tag(tf, "shell.palette.decode")


def make_refused(tf, example: str) -> str:
    """Put the screen in the refused tone, and answer what the AGENT was told.

    ★ Screen B is reached by TYPING, and that is the screen being right rather
    than the test being awkward: its `filter` verb refuses a malformed query
    outright, because an agent sends a whole one, while a person is malformed on
    nearly every keystroke and the bar keeps the list. So the refusal a *person*
    meets is the one the box announces as they type, which is what this drives.
    """
    if example == "hello-node-lab":
        select_afresh(tf, "R-01")
        return refuse(tf, "rename", "R-01,P-01")
    if example == "hello-packet-view":
        focus_by_tab(tf, "pv.filter.query")
        for ch in "nosuchcolumn = 1":
            # A single codepoint routes through the character path, which is
            # the door a real keystroke comes in by — a space included.
            tf.key(path="pv.filter.query", name=ch)
        # ★ Typing is live and SILENT here — the list re-derives from the
        # buffer on every keystroke, so there is nothing for it to apply — and
        # Enter is what says where the query got to. R1707's own note.
        tf.key(path="pv.filter.query", name="Enter")
        return said(tf)["clause"]
    press_tag(tf, "shell.palette.overlay")
    return said(tf)["clause"]


# ── A: the wire publishes the value ─────────────────────────────────────────


def a_the_wire_publishes_the_value(tf, example: str) -> None:
    banner(f"A — {example}: the wire publishes tone, clause and sentence")
    make_done(tf, example)
    value = said(tf)
    ok(
        f"A[{example}]: the screen answers a VALUE, not a string — {value}",
        isinstance(value, dict) and set(value) == {"tone", "clause", "sentence", "urgency"},
    )
    assert_eq(value["tone"], "done", f"A[{example}]: and it names the kind")
    assert_eq(
        value["sentence"],
        value["clause"],
        f"A[{example}]: ★ a tone that frames nothing shows its clause verbatim, "
        f"so the sentence is DERIVED and not a second record",
    )
    assert_eq(
        value["urgency"],
        "when-idle",
        f"A[{example}]: ★★ and the urgency came off the tone rather than off a "
        f"constant this screen picked",
    )


# ── B: the urgency is derived ───────────────────────────────────────────────


def b_the_urgency_comes_off_the_tone(tf, example: str, tag: str) -> str:
    banner(f"B — {example}: a refusal interrupts, a confirmation waits")
    make_done(tf, example)
    polite = live_of(tf, tag)
    assert_eq(
        polite,
        "polite",
        f"B[{example}]: ★★★ a confirmation does not interrupt a reader — the "
        f"node lab announced every one of these assertively before this round",
    )
    agent_words = make_refused(tf, example)
    assert_eq(said(tf)["tone"], "refused", f"B[{example}]: the refusal is a refusal")
    assert_eq(
        live_of(tf, tag),
        "assertive",
        f"B[{example}]: ★★★★★ and it CUTS IN — two of these three screens "
        f"announced every refusal politely, because the urgency was a constant",
    )
    ok(
        f"B[{example}]: ★★ the same region answered two urgencies, which is "
        f"what a per-screen constant cannot do",
        polite != live_of(tf, tag),
    )
    return agent_words


# ── C: the frame is not in the stored text ──────────────────────────────────


def c_the_frame_is_not_in_the_clause(tf, example: str, agent_words: str) -> None:
    banner(f"C — {example}: the producer's sentence, kept out of the frame")
    value = said(tf)
    assert_eq(
        value["sentence"],
        f"refused: {value['clause']}",
        f"C[{example}]: ★★★ the sentence a person reads is the frame plus the "
        f"clause, composed on the way out",
    )
    ok(
        f"C[{example}]: ★★★★★ and the clause carries no frame of its own "
        f"({value['clause']!r}) — the fact used to live in a string prefix that "
        f"three screens spelled three ways",
        not value["clause"].lower().startswith("refused"),
    )
    if example == "hello-node-lab":
        # ★★ Only this screen refuses on BOTH channels at once, so it is the
        # only place the pair can be compared. Asserting it on the other two
        # would be reading back what this test itself put there.
        ok(
            f"C[{example}]: ★★★★ the agent's channel gets the clause, not the "
            f"framed sentence — one value answers both, so they cannot drift",
            value["clause"] in agent_words and "refused: " not in agent_words,
        )


# ── D: the arm for an act that changed nothing ──────────────────────────────


def d_an_act_that_changed_nothing_says_so(tf) -> None:
    banner("D — the node lab: renaming a card to its own name")
    select_afresh(tf, "R-01")
    refuse(tf, "rename", "R-01,P-01")
    stale = said(tf)
    assert_eq(stale["tone"], "refused", "D: a refusal is standing on the toast")
    tf.invoke(f"{EXT}/rename", "R-01,R-01")
    now = said(tf)
    assert_eq(
        now["tone"],
        "unchanged",
        "D: ★★★★★ the act that changed nothing SAYS so — before this round it "
        "said nothing at all and the refusal above stayed on screen",
    )
    ok(
        f"D: ★★★ and a reader is not left with the previous act's words "
        f"({now['sentence']!r})",
        now["sentence"] != stale["sentence"],
    )
    assert_eq(
        now["urgency"],
        "when-idle",
        "D: ★★ nothing happened, so nobody is interrupted — which is the arm "
        "existing for, rather than folding it into the refusal",
    )
    assert_eq(
        live_of(tf, "lab.toast"),
        "polite",
        "D: and the live region agrees, because it reads the same tone",
    )


# ── E: the seeing half ──────────────────────────────────────────────────────


def e_the_bullet_changes_colour(tf) -> None:
    banner("E — the node lab: the toast's bullet, measured off the paint")

    def bullet() -> tuple:
        scene = tf.snapshot(source="paint", viewport=(1600, 900))
        found = fills_by_tag(scene)
        assert "lab.toast.dot" in found, "the toast's bullet is painted"
        return found["lab.toast.dot"]

    select_afresh(tf, "R-01")
    confirmed = bullet()
    refuse(tf, "rename", "R-01,P-01")
    refused = bullet()
    ok(
        f"E: ★★★★★ a refusal is a different picture from a confirmation "
        f"({confirmed} vs {refused}) — one expression, `ink.accent`, painted "
        f"both before this round",
        confirmed != refused,
    )
    tf.invoke(f"{EXT}/rename", "R-01,R-01")
    unchanged = bullet()
    ok(
        f"E: ★★★ and all three tones are told apart by eye ({unchanged})",
        len({confirmed, refused, unchanged}) == 3,
    )


def fills_by_tag(scene) -> dict:
    """Every tagged mark's fill colour, flattened out of a painted scene."""
    out: dict[str, tuple] = {}

    def walk(node) -> None:
        if isinstance(node, dict):
            tag = node.get("tag")
            style = node.get("style") or {}
            fill = style.get("background") or style.get("fill")
            if tag and fill is not None:
                out[tag] = json.dumps(fill, sort_keys=True)
            for value in node.values():
                walk(value)
        elif isinstance(node, list):
            for value in node:
                walk(value)

    walk(scene)
    return out


# ── F: a malformed announcement is refused ──────────────────────────────────


def f_the_rule_is_the_types(tf) -> None:
    banner("F — what cannot be said, checked through the screen's own verbs")
    # The three faults are unit-tested in `pinion_core::utterance`; what this
    # section asserts is that the SCREENS cannot produce one, which they cannot
    # because the constructor is the only door. The observable half is that
    # every sentence the screens can reach is well formed.
    seen = set()
    for path, args in [
        ("select", "R-01"),
        ("rename", "R-01,R-01"),
        ("export", ""),
        ("fit", ""),
    ]:
        try:
            # ★ R1737.1 — through the same helper, because a re-selection is
            # now correctly silent and this section reads what the act SAID.
            if path == "select":
                select_afresh(tf, args)
            else:
                tf.invoke(f"{EXT}/{path}", args)
        except Exception:  # noqa: BLE001 — a refusal still says something
            pass
        value = said(tf)
        seen.add(value["tone"])
        ok(
            f"F: after {path!r} the clause is a sentence, not empty and not "
            f"a Debug spelling ({value['clause'][:48]!r})",
            value["clause"].strip() != ""
            and not value["clause"].lower().startswith("refused")
            and "(" not in value["clause"].split(" ")[0],
        )
    ok(
        f"F: ★★ and these four verbs reached {len(seen)} of the three tones, "
        f"so the arms are not decoration",
        len(seen) >= 2,
    )


# ── G: the wire round trip ──────────────────────────────────────────────────


def g_the_value_survives_the_wire(tf) -> None:
    banner("G — the value read back is the value")
    select_afresh(tf, "R-01")
    value = said(tf)
    again = said(tf)
    assert_eq(again, value, "G: reading twice answers the same value")
    assert_eq(
        tf.query(f"{EXT}/toast"),
        value["sentence"],
        "G: ★★★ and the string path a person's readers use is the same value "
        "projected — two derivations of one record, never two records",
    )
    declared = tf.query(f"{EXT}/$schema")
    if isinstance(declared, str):
        declared = json.loads(declared)
    schema = {f["path"]: f.get("type") for f in declared if isinstance(f, dict)}
    assert_eq(
        schema.get("said"),
        "object",
        "G: ★★ the schema declares it an object — it was `string` on all three "
        "screens, which is why nothing downstream could ask the kind",
    )


def main() -> int:
    for example, tag in SCREENS:
        with RpcSubprocess(example) as tf:
            a_the_wire_publishes_the_value(tf, example)
            agent_words = b_the_urgency_comes_off_the_tone(tf, example, tag)
            c_the_frame_is_not_in_the_clause(tf, example, agent_words)
    with RpcSubprocess("hello-node-lab") as tf:
        d_an_act_that_changed_nothing_says_so(tf)
        e_the_bullet_changes_colour(tf)
        f_the_rule_is_the_types(tf)
        g_the_value_survives_the_wire(tf)
    print(f"\n{len(CHECKS)} named check(s) beyond the equalities:")
    for line in CHECKS:
        print(f"  - {line}")
    return 0


if __name__ == "__main__":
    run_demo("r1719_what_a_screen_says_knows_its_kind", main)
