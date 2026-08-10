#!/usr/bin/env python3
"""R1642 — a conditional argument declares its cases, and the declaration is the
only thing this demo knows.

R1638 made an action say what it takes. It could not say that one argument's
value decides the rest of the call, so `arrange` declared its third segment as
`{"type": "string", "domain": {"kind": "open"}, "optional": true}` — the only
sentence a flat argument list can produce about a slot that is a closed edge
vocabulary after `align`, an integer after `stack`, and absent after the other
two. Measured over this very wire before the fix, that declaration admitted nine
calls and the dispatcher refused three of them (`align:horizontal` elided,
`align:horizontal:17`, `stack:horizontal:start`) and silently ignored the tail on
a fourth (`distribute:horizontal:start`). Silence would have been honest; that
was a false statement carrying a schema's authority.

`item` was worse and could not have been fixed by a flat list at all: `add:in:1`,
`remove:in:0` and `move:in:2:0` are three different ARITIES, so no positional
argument list describes them. It published no `args` at all.

What this demo checks, and why each check discriminates:

* **(A) the case table reaches a client whole.** The discriminant publishes
  `one_of_with`, and each case carries the arguments choosing it brings —
  including an empty `then`, which is the affirmative "this one adds nothing" and
  is what makes the refusals in (C) legitimate rather than pedantic.
* **(B) the declaration is sufficient — every admitted call lands.** The
  generator below expands a field into the calls its declaration says are
  well-formed, from the wire alone: separator and segment order from `arg_form`,
  vocabularies from each `domain`, per-case arguments from `cases`, and both the
  present and absent forms of anything `optional`. Nothing in this file spells a
  payload. Before the fix this sweep FAILS on `arrange` — which is the point: the
  same generator run against R1638's declaration produces `align:horizontal` and
  `align:horizontal:<int>`, and the surface refuses both.
* **(C) the declaration is tight — every excluded call is refused.** Acceptance
  alone passes against a declaration that admits everything, so each way of
  stepping outside it is driven too: a value the discriminant does not list, a
  wrong vocabulary in a case's own argument, an omitted argument the case says is
  required, and a trailing segment for a case that declared none. The last is the
  one that used to be *accepted and ignored*.
* **(D) published == accepted, in both directions, for every vocabulary.** Every
  value any closed domain publishes is exercised, so a list that is too short
  fails (B) and one that is too long fails (C).
* **(E) past the reference.** The meta-object generates a parameter list from one
  C++ signature, so a conditional argument there has to become separate methods —
  which is what the toolkit does with the eleven alignment commands R1631 folded
  into parameters. Its `Cloned` attribute enumerates the arities a default
  argument produces but cannot say two of them belong to different values of the
  same parameter. Asserted here as the shape of the answer: one verb, four cases,
  and the fact that they are one operation.

Run from the workspace root:
    cargo build -p hello-node-groups --release
    python3 tools/demos/r1642_a_conditional_argument_declares_its_cases.py
"""

from __future__ import annotations

import itertools
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_action_refused,
    assert_eq,
    run_demo,
)

EXT = "/external"
SCHEMA = f"{EXT}/$schema"

#: FIXTURE data, not declaration data: a value per argument whose domain is
#: `open`.
#:
#: An `open` domain is the surface saying it publishes nothing to enumerate, so
#: there is nothing here for a generator to read and anything it invented would
#: be this file's imagination rather than the declaration. Supplying them by NAME
#: rather than by type is deliberate: a new open argument then has no sample and
#: the sweep refuses to run, which forces whoever adds it to decide whether its
#: bound really is unpublishable — the excuse `ArgDomain::Open` documents — or
#: whether it should have pointed at a path.
#:
#: `item`'s two are positions in a variadic run, so they are chosen to be legal
#: on the demo's fixture node; that legality is a precondition of the surface,
#: not a claim of the schema.
OPEN_SAMPLES = {
    "gap": ["0", "7"],
    "index": ["0"],
    "to": ["0"],
    "label": ["Underlay"],
}


def schema_of(tf: RpcSubprocess, ext: str = EXT) -> dict[str, dict]:
    return {f["path"]: f for f in tf.query(f"{ext}/$schema")}


def values_for(tf: RpcSubprocess, arg: dict) -> list[str]:
    """Every value this argument admits, read off its domain — and, where the
    domain is a path, off the surface."""
    domain = arg["domain"]
    kind = domain["kind"]
    if kind == "one_of":
        return list(domain["values"])
    if kind == "one_of_with":
        return [case["value"] for case in domain["cases"]]
    if kind == "values_of":
        # The domain names a live path; following it is the whole point.
        listed = str(tf.query(f"{EXT}/{domain['values_path']}"))
        values = [v for v in listed.split(",") if v]
        assert values, f"{arg['name']} points at {domain['values_path']}, which is empty"
        return values
    if kind == "open":
        sample = OPEN_SAMPLES.get(arg["name"])
        assert sample, (
            f"{arg['name']} is `open`, so the sweep needs a fixture value for it; "
            "decide whether its bound is really unpublishable before adding one"
        )
        return list(sample)
    raise AssertionError(f"this sweep does not enumerate {kind}: {arg}")


def expansions(field: dict) -> list[list[dict]]:
    """The argument lists this field's declaration admits, in wire order.

    One per case of the discriminant, if it has one; otherwise the single flat
    list. The composition rule the schema states is that a case's arguments come
    after every argument the field declares, so that is the concatenation here —
    read from the wire, not assumed.
    """
    args = field["args"]
    discriminants = [a for a in args if a["domain"]["kind"] == "one_of_with"]
    assert len(discriminants) <= 1, f"one discriminant at most: {field}"
    if not discriminants:
        return [args]
    return [args + case["then"] for case in discriminants[0]["domain"]["cases"]]


def payloads(
    tf: RpcSubprocess, field: dict, expansion: list[dict], case_value: str | None
) -> list[str]:
    """Every delimited payload this expansion admits.

    The product over each argument's admissible values, and — for a trailing
    optional argument — both the form that carries it and the form that elides
    it, since `optional` is a claim about the wire that only the short payload
    tests.
    """
    sep = field["arg_form"]["separator"]

    def choices_for(args: list[dict]) -> list[list[str]]:
        return [
            # The discriminant is pinned to the case being expanded; its other
            # values belong to the other expansions.
            [case_value] if a["domain"]["kind"] == "one_of_with" else values_for(tf, a)
            for a in args
        ]

    out = [sep.join(c) for c in itertools.product(*choices_for(expansion))]
    # An optional suffix may be dropped, one trailing argument at a time.
    droppable = 0
    while droppable < len(expansion) and expansion[-1 - droppable].get("optional"):
        droppable += 1
        head = expansion[: len(expansion) - droppable]
        out.extend(sep.join(c) for c in itertools.product(*choices_for(head)))
    return out


def admitted(tf: RpcSubprocess, field: dict) -> list[str]:
    """Every payload the declaration says is well-formed."""
    discriminant = next(
        (a for a in field["args"] if a["domain"]["kind"] == "one_of_with"), None
    )
    if discriminant is None:
        return payloads(tf, field, field["args"], None)
    out: list[str] = []
    for case in discriminant["domain"]["cases"]:
        out.extend(payloads(tf, field, field["args"] + case["then"], case["value"]))
    return out


def declaration(tf: RpcSubprocess, path: str) -> dict:
    field = schema_of(tf)[path]
    assert_eq(field["channel"], "invoke", f"{path} is an action")
    assert_eq(field["arg_form"]["kind"], "delimited", f"{path}'s form")
    return field


def refused(tf: RpcSubprocess, path: str, payload: str, saying: str) -> None:
    assert_action_refused(
        lambda: tf.invoke(f"{EXT}/{path}", payload), saying=saying
    )


def sweep(tf: RpcSubprocess, path: str) -> tuple[int, int]:
    """(B) + (D): every admitted call lands, and every published value is used."""
    field = declaration(tf, path)
    calls = admitted(tf, field)
    # Non-triviality, stated as a property rather than a magic number: more than
    # one case, and more calls than cases — so at least one case's own arguments
    # or optional suffix actually multiplied. The stronger claim, that every
    # published value was sent, is asserted below.
    cases = len(expansions(field))
    assert cases > 1, f"{path} declares {cases} case(s); nothing is being swept"
    assert len(calls) > cases, f"{path}: no case contributed a choice: {calls}"
    seen: set[str] = set()
    for payload in calls:
        answer = tf.invoke(f"{EXT}/{path}", payload)
        assert answer is not None, f"{path} {payload!r} answered nothing"
        seen.update(payload.split(field["arg_form"]["separator"]))
    # Every value of every ENUMERABLE domain in every expansion was actually
    # sent — the direction that fails against a vocabulary too short, while (C)
    # is the direction that fails against one too long.
    published: set[str] = set()
    for expansion in expansions(field):
        for arg in expansion:
            if arg["domain"]["kind"] in ("one_of", "one_of_with", "values_of"):
                published.update(values_for(tf, arg))
    missing = published - seen
    assert not missing, f"{path} publishes values the sweep never sent: {missing}"
    return len(calls), len(published)


def body() -> None:
    with RpcSubprocess("hello-node-groups", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)

        # ── (A) the case table reaches a client whole ────────────────────
        arrange = declaration(tf, "arrange")
        names = [a["name"] for a in arrange["args"]]
        assert_eq(names, ["pass", "axis"], "the shared segments, in wire order")
        assert "edge_or_gap" not in names, "the one merged slot is gone"
        pass_arg = arrange["args"][0]
        assert_eq(pass_arg["domain"]["kind"], "one_of_with", "the pass CHOOSES")
        cases = {c["value"]: c["then"] for c in pass_arg["domain"]["cases"]}
        assert_eq(
            sorted(cases),
            ["align", "distribute", "stack", "straighten"],
            "one case per pass, and the vocabulary is still enumerable",
        )
        assert_eq([a["name"] for a in cases["align"]], ["edge"], "align reads an edge")
        assert_eq(
            cases["align"][0]["domain"]["values"],
            ["start", "center", "end"],
            "★ from the crate's own enum, not a literal in the schema",
        )
        assert not cases["align"][0].get("optional"), "and it is required"
        assert_eq([a["name"] for a in cases["stack"]], ["gap"], "stack reads a gap")
        assert_eq(cases["stack"][0]["type"], "int", "which is a number, not a word")
        assert cases["stack"][0].get("optional"), "and may be left out"
        assert_eq(cases["distribute"], [], "★ an empty case is a CLAIM: adds nothing")
        assert_eq(cases["straighten"], [], "and so is this one")
        print("[demo] one verb, four cases, and each says what it brings")

        item = declaration(tf, "item")
        verb = item["args"][0]
        assert_eq(verb["domain"]["kind"], "one_of_with", "the verb CHOOSES too")
        item_cases = {c["value"]: c["then"] for c in verb["domain"]["cases"]}
        arities = {v: len(t) for v, t in item_cases.items()}
        assert_eq(
            arities,
            {"add": 1, "remove": 0, "move": 1},
            "★ three arities under one name — what no flat list can describe",
        )
        assert item_cases["add"][0].get("optional"), "a label may be left out"
        assert not item_cases["move"][0].get("optional"), "a destination may not"
        assert_eq(
            [a["name"] for a in item["args"]],
            ["verb", "side", "index"],
            "and the segments every edit shares",
        )
        # And the side is a LIVE bound, not the type's two words: which side
        # answers is the selected node's kind's business.
        side = item["args"][1]
        assert_eq(side["domain"]["kind"], "values_of", "★ side points at a path")
        assert_eq(side["domain"]["values_path"], "item_sides")
        assert_eq(
            str(tf.query(f"{EXT}/item_sides")),
            "",
            "nothing is selected yet, so no side answers — and the path says so",
        )
        print("[demo] item publishes three arities under one verb")

        # ── (B) + (D) every admitted call lands, every value gets sent ───
        calls, published = sweep(tf, "arrange")
        print(f"[demo] {calls} calls built from arrange's declaration alone, all "
              f"accepted, covering {published} published values")

        # `item` needs exactly one selected node with a variadic run, which is
        # the surface's own precondition and nothing to do with the declaration.
        layers = int(str(tf.invoke(f"{EXT}/add", "layers")))
        assert_eq(str(tf.invoke(f"{EXT}/select", str(layers))), "1", "one selected")
        tf.tick(0.016)
        assert_eq(
            str(tf.query(f"{EXT}/item_sides")),
            "in",
            "★ and now it says which side answers — this kind repeats its INPUTS, "
            "so the closed pair would have advertised a side `item` refuses",
        )
        item_calls, item_published = sweep(tf, "item")
        print(f"[demo] {item_calls} calls built from item's declaration alone, all "
              f"accepted, covering {item_published} published values")

        # A live domain has to go EMPTY when no call is well formed, or it is
        # advertising calls the surface refuses — the same defect as the closed
        # pair, one condition along. `item` needs exactly one selected node, so
        # a second selection empties the domain. Found by counterfactual: the
        # single-selection fixture could not tell this rule from `first()`.
        second = int(str(tf.invoke(f"{EXT}/add", "layers")))
        assert_eq(str(tf.invoke(f"{EXT}/select", f"{layers},{second}")), "2", "two")
        tf.tick(0.016)
        assert_eq(
            str(tf.query(f"{EXT}/item_sides")),
            "",
            "★ two selected, so NO side answers — and the domain says so rather "
            "than naming a side the call would be refused for",
        )
        refused(tf, "item", "add:in:0", "exactly one selected node")
        assert_eq(str(tf.invoke(f"{EXT}/select", str(layers))), "1", "one again")
        tf.tick(0.016)
        assert_eq(str(tf.query(f"{EXT}/item_sides")), "in", "and it comes back")
        print("[demo] the live domain empties when no call is well formed")

        # ── (C) and nothing outside the declaration is accepted ─────────
        # A value the discriminant does not list.
        refused(tf, "arrange", "tidy:horizontal:start", "is not one of")
        refused(tf, "item", "insert:in:0", "is not one of")
        # A side the live path does not list. The refusal comes from the MODEL,
        # which owns the fact `item_sides` reads — deliberately not from a second
        # vocabulary check in the dispatcher, which would be a copy of
        # `Document::variadic` and could drift from it. The declaration's job is
        # to let a client avoid this call; the model's job is to refuse it.
        refused(tf, "item", "add:out:0", "NotVariadic")
        # A wrong vocabulary inside a case's own argument: an integer where the
        # case declared a closed edge set, and a word where it declared an int.
        refused(tf, "arrange", "align:horizontal:17", "arrange edge")
        refused(tf, "arrange", "stack:horizontal:start", "gap is not an integer")
        # A required case argument omitted. `align` says required, `stack` says
        # optional, and the pair is why this discriminates: the same position is
        # refused under one case and accepted under another.
        refused(tf, "arrange", "align:horizontal", "is missing")
        assert tf.invoke(f"{EXT}/arrange", "stack:horizontal") is not None
        refused(tf, "item", "move:in:0", "item destination")
        assert tf.invoke(f"{EXT}/item", "add:in:0") is not None
        # A trailing segment for a case that declared none. THIS is the one that
        # used to be accepted and silently dropped, which is the failure mode an
        # author reads as a broken tool rather than as a rejected call.
        refused(tf, "arrange", "distribute:horizontal:start", "no third segment")
        refused(tf, "arrange", "straighten:vertical:7", "no third segment")
        refused(tf, "item", "remove:in:0:extra", "no fourth segment")
        # And a segment past every case's own arguments.
        refused(tf, "arrange", "align:horizontal:start:more", "no further segment")
        refused(tf, "item", "move:in:0:0:more", "no further segment")
        print("[demo] every call the declaration excludes is refused, including "
              "the one that used to be accepted and ignored")

        # ── (E) the shape of the answer, against the reference ──────────
        # The reference would spell these as separate methods, so a client there
        # discovers unrelated verbs. Here the count of verbs and the count of
        # cases are both readable, and the second is what says they are one
        # operation.
        conditional = [
            f
            for f in schema_of(tf).values()
            if any(a["domain"]["kind"] == "one_of_with" for a in f.get("args", []))
        ]
        assert_eq(len(conditional), 2, "the two conditional verbs on this surface")
        total_cases = sum(
            len(a["domain"]["cases"])
            for f in conditional
            for a in f["args"]
            if a["domain"]["kind"] == "one_of_with"
        )
        assert_eq(total_cases, 7, "four passes and three item edits")
        print(f"[demo] ★ {len(conditional)} verbs carry {total_cases} cases; the "
              "reference would publish 7 unrelated method names and no way to "
              "learn they are 2 operations")


if __name__ == "__main__":
    run_demo("r1642-a-conditional-argument-declares-its-cases", body)
