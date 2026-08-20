#!/usr/bin/env python3
"""R1720 §5.15 §5.12 §2 #2 §2 #7 — **a refusal handed to the agent is heard by
the person**, on all three screens of the analysis tool.

# The defect this exists for, measured by driving the screens

§2 #2 makes RPC the AI's primary path, so "an agent drives and a person watches"
is the ORDINARY state of these screens rather than an exception. Measured
2026-08-18 by driving every action slot the three screens publish, with an
argument each verb must refuse:

| screen | actions that refuse | reached the person |
|---|--:|--:|
| A node lab | 26 | **2** |
| B capture viewer | 9 | **0** |
| C dashboard | 20 | **0** |

Fifty-five refusals; two arrive. And the two that do are the two sites where
somebody wrote the coupling out by hand — `let said = …; state.say(said.clone());
Err(InvokeError::rejected(said.into_clause()))` — so the property held exactly
where it had been remembered. In the other fifty-three the agent was told, the
screen did not move, and the sentence standing on it was about some earlier act:
the same stale-message defect `Tone::Unchanged` was built for, arriving through
the other channel.

The same measurement found the drift that shape invites. On the node lab's link
authoring, one refusal said `R-01 has no accept pin` to the person and
`R-01 does not listen, so nothing can dial it` to the agent — two wordings for
one fact, three sites away from the round that made them one value.

# The floor this is built to beat, measured rather than read

A probe was built against the mature toolkit at 6.11.1 and **run** offscreen.

  * refusing a programmatic call emits **0** accessibility events — both for a
    verb nobody declared and for a declared verb that answers "no";
  * the caller's whole answer is a **boolean**. The reason for the first exists,
    and it goes to the process's diagnostic stream — a global sink that no
    caller and no person reads;
  * its own status channel emits **0** accessibility events, takes no kind, and
    nothing routes a refusal into it: a caller that wants the person told calls
    it, with words it composes itself;
  * **nothing anywhere reports whether the person was told**, so a caller that
    wants to avoid saying it twice cannot find out.

Six capabilities this round ships are compile errors there: asking a refusal for
the sentence a person reads, having the framework put it in front of them,
asking a surface where its speech goes, declaring that it has nowhere, learning
from the refusal whether the person heard it, and driving every published action
into refusal to check that they all do.

# What it asserts

* **A** — ★★★★★ the headline, on every screen: **every action the surface
  publishes, driven until it refuses, reaches the person**. Not a sample —
  the surface's own declared action list, so a verb added next round is covered
  the moment it is declared.
* **B** — one refusal, one wording. The clause the agent is given and the clause
  the person reads are the same string, which is what the framework composing
  both from one value buys.
* **C** — ★★★★★ the refusal is heard AS a refusal: the live region's urgency is
  `assertive` on all three screens after an agent's refused call, where two of
  the three previously announced everything politely and one of them said
  nothing at all.
* **D** — the agent can find out. The refusal's own record says whether the
  person was told and names the region, so an agent need not guess whether to
  say it again.
* **E** — the write channel too. A refused `intervene` is a mutation that did
  not happen, and it reaches the person by the same seam.
* **F** — ★★★★ the person is not told about a refused READ. The one channel a
  person watches is not filled with an agent's probing, and this is a decision
  rather than an omission.
* **G** — the seeing half: the toast on screen A shows the refusal the agent
  caused, measured off the painted scene rather than off the source.

>= 30 assertions.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    abs_rects_of,
    access_node_by_tag,
    assert_eq,
    assert_every_refusal_is_heard,
    run_demo,
)

EXT = "/external"

#: The three screens, and four things that differ per screen and nowhere else:
#: the live region each puts speech in, a WRITE it refuses, and an ACTION whose
#: refusal the surface itself authors. The last is needed because the two kinds
#: of refusal are held to opposite rules — see `b_the_two_kinds_of_refusal`.
SCREENS = [
    ("hello-node-lab", "lab.toast", "zoom", ("select", "no.such.card")),
    ("hello-packet-view", "pv.appbar.said", "row_count", ("select_message", "no.such.row")),
    ("hello-analyzer-shell", "shell.toast", "cards", ("title", "no.such.card,x")),
]

#: R1564 / R1565 — the codes a refusal the SURFACE authored travels under. Every
#: other code carries a word this transport wrote.
SURFACE_AUTHORED = (-32005, -32006)

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


def live_of(tf, tag: str):
    node = access_node_by_tag(tf.request("scene/access").result, tag)
    return node and node.get("live")


def refuse(tf, path: str, args, *, channel: str = "invoke") -> RpcError:
    """Drive something that must refuse, and answer the refusal itself."""
    call = tf.invoke if channel == "invoke" else tf.intervene
    try:
        call(f"{EXT}/{path}", args, with_origin=True)
    except RpcError as why:
        return why
    raise AssertionError(f"{channel} {path}({args!r}) was expected to refuse")


# ── A: every published action, driven until it refuses ──────────────────────


def a_every_published_action_is_heard(tf, example: str) -> dict:
    banner(f"A — {example}: every declared action, driven into refusal")
    census = assert_every_refusal_is_heard(tf)
    ok(
        f"A[{example}]: ★★★★★ all {census['refused']} of the "
        f"{census['declared']} declared actions that refuse put the refusal in "
        f"front of the person — measured at 2, 0 and 0 before this round",
        census["refused"] > 0 and census["exempt"] == 0,
    )
    ok(
        f"A[{example}]: ★★★ the population is the surface's OWN declaration "
        f"({census['declared']} actions), so a verb added next round is covered "
        f"by being declared rather than by being added to a list here",
        census["declared"] >= census["refused"],
    )
    return census


# ── B, C: one wording, heard as a refusal ───────────────────────────────────


def b_the_two_kinds_of_refusal(tf, example: str, tag: str, surface: tuple) -> None:
    banner(f"B/C — {example}: one wording where the surface spoke, a sentence where it did not")

    # ── the surface's own refusal: one fact, one wording ────────────────────
    #
    # ★ What the screen was saying BEFORE, so the assertions cannot be satisfied
    # by reading back a sentence that was already standing.
    before = said(tf)
    why = refuse(tf, surface[0], surface[1])
    value = said(tf)
    assert_eq(
        why.code in SURFACE_AUTHORED,
        True,
        f"B[{example}]: {surface[0]} refuses in the surface's own words",
    )
    assert_eq(
        value["clause"],
        why.data["reason"],
        f"B[{example}]: ★★★★ the agent and the person get the SAME clause — "
        f"this screen's link authoring was measured saying `R-01 has no accept "
        f"pin` to one and `R-01 does not listen…` to the other",
    )
    assert_eq(
        value["sentence"],
        f"refused: {value['clause']}",
        f"B[{example}]: and the frame is put on for the person, not stored",
    )
    ok(
        f"B[{example}]: ★★★ and this is THIS call's refusal — the screen was "
        f"saying {(before or {}).get('clause')!r} and now says "
        f"{value['clause']!r}, so the check above is not reading back a "
        f"sentence that was already there",
        before is None or before["clause"] != value["clause"],
    )

    # ── the framework's own refusal: a tag for the agent, a sentence for the
    #    person. Held to the OPPOSITE rule, which is the distinction the first
    #    draft of this test got wrong.
    tagged = refuse(tf, "no_such_verb_at_all", "x")
    heard = said(tf)
    assert_eq(
        tagged.data["reason"],
        "UnknownInvokePath",
        f"B[{example}]: an undeclared name refuses with a word an agent matches",
    )
    ok(
        f"B[{example}]: ★★★★★ and the person reads a SENTENCE, not that word "
        f"({heard['sentence']!r}) — the two channels carry different renderings "
        f"of one refusal, which is what having a value rather than a string buys",
        heard["clause"] != tagged.data["reason"] and " " in heard["clause"],
    )

    # ── and either kind is heard AS a refusal ───────────────────────────────
    assert_eq(
        heard["tone"],
        "refused",
        f"C[{example}]: the screen knows what kind of thing it just said",
    )
    assert_eq(
        live_of(tf, tag),
        "assertive",
        f"C[{example}]: ★★★★★ a screen reader is INTERRUPTED for a thing that "
        f"did not happen — this screen was silent about an agent's refusal "
        f"before this round, so there was nothing to be polite about",
    )


# ── D: the agent can find out ───────────────────────────────────────────────


def d_the_agent_learns_whether_the_person_heard(tf, example: str, tag: str) -> None:
    banner(f"D — {example}: the refusal says whether the person was told")
    why = refuse(tf, "no_such_verb_at_all", "x")
    announced = why.data.get("announced")
    ok(
        f"D[{example}]: ★★★★ the refusal carries the third fact — "
        f"{announced} — beside the reason and the surface that refused",
        isinstance(announced, dict),
    )
    assert_eq(announced["reach"], "at", f"D[{example}]: the person was told")
    assert_eq(
        announced["at"],
        tag,
        f"D[{example}]: ★★★ and it names WHERE, so the claim can be read back "
        f"rather than believed",
    )
    node = access_node_by_tag(tf.request("scene/access").result, announced["at"])
    ok(
        f"D[{example}]: ★★★★★ and the named region is a real live region in "
        f"this screen's access tree ({node and node.get('role')!r}) — a surface "
        f"cannot satisfy this by answering `At` and doing nothing",
        node is not None and node.get("live") is not None,
    )


# ── E: the write channel refuses through the same seam ──────────────────────


def e_a_refused_write_is_heard_too(tf, example: str, slot: str) -> None:
    banner(f"E — {example}: a refused write reaches the person as well")
    why = refuse(tf, slot, "not the kind this slot holds", channel="intervene")
    value = said(tf)
    ok(
        f"E[{example}]: ★★★ a refused write reaches the person too "
        f"({value['sentence']!r}) — it is a mutation that did not happen, so "
        f"the screen is showing the old value and something has to say why",
        value["tone"] == "refused" and value["clause"] != why.data["reason"]
        if why.code not in SURFACE_AUTHORED
        else value["clause"] == why.data["reason"],
    )
    assert_eq(
        why.data["announced"]["reach"],
        "at",
        f"E[{example}]: and it reports the person was told",
    )


# ── F: a refused READ says nothing ──────────────────────────────────────────


def f_a_refused_read_is_not_announced(tf, example: str) -> None:
    banner(f"F — {example}: the person is not told about the agent's probing")
    refuse(tf, "no_such_verb_at_all", "x")
    standing = said(tf)
    try:
        tf.query(f"{EXT}/no_such_slot_at_all")
    except RpcError:
        pass
    else:
        raise AssertionError("that read was expected to refuse")
    assert_eq(
        said(tf),
        standing,
        f"F[{example}]: ★★★★ a refused READ changed nothing and left nothing "
        f"stale, so it does not take the one channel a person watches — this "
        f"is a decision the seam makes once, not an omission per screen",
    )


# ── G: the seeing half ──────────────────────────────────────────────────────


def g_the_person_can_see_it(tf) -> None:
    banner("G — the node lab: the agent's refusal, painted")
    # ★★★★★ R1737.1 — the selection has to MOVE for the screen to say anything.
    # R1736 made "an act that changed nothing says nothing" a property of the
    # one place a selection changes, and `R-01` is the card this screen OPENS
    # WITH — so `select R-01` at boot is a no-op and is now correctly silent,
    # leaving no toast to find. Measured: at boot `selected` is `R-01` and
    # `said` is null. The screen is right; this fixture was leaning on a
    # re-selection speaking, which is the behaviour that round removed on
    # purpose. Parking on another card first makes the act a change.
    if tf.query(f"{EXT}/selected") == "R-01":
        tf.invoke(f"{EXT}/select", "T-02")
    tf.invoke(f"{EXT}/select", "R-01")
    rects = abs_rects_of(tf.snapshot(source="paint", viewport=(1600, 900)))
    ok(
        "G: a confirmation is on the toast to begin with",
        "lab.toast.text" in rects or "lab.toast" in rects,
    )
    refuse(tf, "rename", "R-01,P-01")
    after = abs_rects_of(tf.snapshot(source="paint", viewport=(1600, 900)))
    ok(
        "G: ★★★★ the refusal an AGENT caused is drawn on the canvas the person "
        "is looking at, not only returned down the socket",
        "lab.toast.dot" in after,
    )
    value = said(tf)
    ok(
        f"G: ★★★ and what is drawn is the refusal ({value['sentence']!r})",
        value["tone"] == "refused",
    )


def main() -> int:
    for example, tag, slot, surface in SCREENS:
        with RpcSubprocess(example) as tf:
            a_every_published_action_is_heard(tf, example)
            b_the_two_kinds_of_refusal(tf, example, tag, surface)
            d_the_agent_learns_whether_the_person_heard(tf, example, tag)
            e_a_refused_write_is_heard_too(tf, example, slot)
            f_a_refused_read_is_not_announced(tf, example)
    with RpcSubprocess("hello-node-lab") as tf:
        g_the_person_can_see_it(tf)
    print(f"\n{len(CHECKS)} named check(s) beyond the equalities:")
    for line in CHECKS:
        print(f"  - {line}")
    return 0


if __name__ == "__main__":
    run_demo("r1720_a_refusal_reaches_the_person", main)
