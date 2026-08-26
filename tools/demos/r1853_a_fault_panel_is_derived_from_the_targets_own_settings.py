#!/usr/bin/env python3
"""R1853 §5.11 §5.16 §5.21 §2 #7 — **a fault-injection panel derived from the
target's own declaration.**

# What this demo exists for

The analysis-tool census (`tools/analyzer_census.py`) carries `lab.t1.11` —

    a fault injection panel for the faults the target's own settings provide

— as an **app** verdict whose covering sentence was one word: *a form*. That
verdict named **no assembly**, which is what R1807's `UNASSEMBLED` ratchet
records: a claim about a composition nobody had composed. This is the
composition, driven on the wire, on the analysis-tool shell itself — closing a
census row on a demo that never touches the reference screen closes a line
without the screen gaining anything (the R1722 lesson).

# ★★★★★ The claim that makes this a panel rather than a list

**The set of injectable faults is DERIVED from the target's declared settings.**
Not a table of faults per widget kind, maintained beside the declaration and kept
in step by whoever remembers: `pinion_core::widgets::fault_injection::injectable`
walks the form's fields, asks each field's declared shape what values it would
refuse, and **confirms every candidate by putting it through
`FieldType::encode`** — the single place a text becomes a defect. A candidate the
shape was expected to admit and does not is dropped rather than offered.

Section D is the counterfactual, performed as a gesture rather than argued: a row
is added to the declaration and the panel gains that row's faults with **no edit
to the panel**. A hand-maintained list would fail exactly there.

# ★★★★★ Where this is SUPERIOR to the floor, measured rather than asserted

Probed against the reference toolkit at 6.11.1, compiled and run, twice — once
for behaviour and once as a census of the published member surface:

  (1) **a fault-injection panel cannot ask the declaration anything.** The
      bounded-value declaration publishes **10 members over its 3-class chain**,
      of which the two naming the range are the bounds themselves. The input row
      it is attached to publishes **129**, and the fixed-set chooser **123** —
      and of those 252, **zero** name validation at all. The whole vocabulary is
      one call answering *acceptable / intermediate / invalid* for a value the
      caller **already has**: the direction is inward only. There is no member
      that enumerates the faults a declaration admits, none that produces a value
      causing one, and none that says whether such a fault would stop the
      configured program starting — the three things a panel needs. (Class names
      are deliberately absent: a reference toolkit's names must not reach a
      tracked file, and the capability sentence carries the evidence anyway.)
  (2) **which faults are even reachable is a side effect of widget choice.**
      Measured: an integer row bounded 0..10 refuses to hold `abc` at all, so a
      wrong-type fault cannot be produced by typing into it; it *does* hold `99`
      while reporting the input unacceptable; and a fixed-set chooser handed
      `nonsense` silently keeps its previous word. So the answer to *what can go
      wrong here* differs per widget, is never published, and a panel built on
      that floor must encode it by hand — which is the list this round refuses to
      have.

Here a fault is `(key, kind, value, applies, admitted_by)` derived from the
declaration and confirmed by the encoder; a kind is one of a **closed published
vocabulary**; and each offer carries whether it merely warns or **stops a
launch**, which is the framework's verdict and not the panel's opinion.

# ★★★★★ And what it CANNOT offer is named, derived from the same boundary

An absence nobody names is indistinguishable from an oversight, and this panel
sits inside a tool whose subject is a network — a reader will take three
configuration faults as a claim about the faults there are. So `Scope` has three
arms and the screen paints one sentence per arm it cannot offer, filtered by
`Scope::injectable` rather than written out. Section F reads that from the wire
and section A reads it off the assembled screen's paint.

⚠ One of those arms is a measurement rather than a decision: `ConfigForm::adopt`
reports a leaf the declaration has no row for as *unplaceable* and does not take
it, so *a key the target does not know* is a real fault of the settings that a
form **cannot reach**. This round's first draft offered it. The probe of `adopt`
is what removed it.

# What is shown

  (A) the assembled tool: the walk stands in the node lab, whose inspector paints
      the panel, the heading counts what it painted, and both out-of-reach scopes
      are named there in the framework's own words.
  (B) a reader is told: the panel is a list, its rows are its children, and each
      row announces its key, its arm, whether it blocks a launch, and its
      HOT/RESTART badge with a position in the list.
  (C) the derivation: every offered key is a row the form holds, every arm is a
      word of the published vocabulary, and the value is the derivation's.
  (D) THE COUNTERFACTUAL: adding a row to the declaration adds its faults with no
      edit to the panel, and removing it takes them away.
  (E) the refusal, three ways: an arm the field does not admit is refused BY NAME
      with the arms it does; a word outside the vocabulary is refused with the
      vocabulary; and performing an offer really does block the launch.
  (F) the boundary is published: three scopes, exactly one injectable, each with
      the sentence a surface can show.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_action_refused,
    assert_eq,
    resize_and_settle,
    run_demo,
)

SHELL = "hello-analyzer-shell"
LAB = "hello-node-lab"
EXT = "/external"
SEAT = "lab"
LAB_WINDOW = (1800, 900)

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def walk_texts(node: Any) -> list[str]:
    """Every text run's content in a scene tree, wherever it is nested."""
    out: list[str] = []
    if isinstance(node, dict):
        content = node.get("content")
        if isinstance(content, str):
            out.append(content)
        for value in node.values():
            out.extend(walk_texts(value))
    elif isinstance(node, list):
        for value in node:
            out.extend(walk_texts(value))
    return out


def walk_tags(node: Any) -> list[str]:
    """Every tag in a scene tree, wherever it is nested."""
    out: list[str] = []
    if isinstance(node, dict):
        tag = node.get("tag")
        if isinstance(tag, str):
            out.append(tag)
        for value in node.values():
            out.extend(walk_tags(value))
    elif isinstance(node, list):
        for value in node:
            out.extend(walk_tags(value))
    return out


def as_json(answer: Any) -> Any:
    """A slot's answer as data.

    ⚠ Not cosmetic. A screen may publish a slot as a typed JSON value or as
    TEXT that happens to hold JSON, and `scene/query` hands back exactly what the
    slot produced. This demo reads three slots of one screen and they are not all
    the same kind — `faults` and `fault_scopes` are typed, `form` and `catalogue`
    are text. Iterating the text form without parsing it walks the STRING's
    characters and every membership test quietly answers false, which is how a
    correct screen reads as broken.
    """
    if isinstance(answer, str):
        import json

        return json.loads(answer)
    return answer


def slot(app: RpcSubprocess, name: str) -> Any:
    """Read one of this screen's slots as data. ONE reader, not one per site."""
    return as_json(app.query(f"{EXT}/{name}"))


def announced(app: RpcSubprocess, prefix: str, *, leaves: bool = True) -> dict[str, Any]:
    """The accessibility nodes under `prefix`, by tag."""
    out: dict[str, Any] = {}
    for node in app.request("scene/access").result["nodes"]:
        tag = node.get("tag") or ""
        if not tag.startswith(prefix):
            continue
        if not leaves and "." in tag[len(prefix) :]:
            continue
        out[tag] = node
    return out


def reading(node: dict[str, Any]) -> str:
    """What a reader is told this node's value says, normalised.

    ⚠ `value` on the wire is a typed thing — the access tree carries a value's
    KIND beside its text — so a bare `node["value"]` is a guess at the shape
    rather than a read of it.
    """
    value = node.get("value")
    if isinstance(value, dict):
        for key in ("text", "Text", "value"):
            if isinstance(value.get(key), str):
                return value[key]
        return str(value)
    return value if isinstance(value, str) else ""


def select_card(app: RpcSubprocess, card: str = "P-01") -> None:
    """Press a node card, the way a hand selects one.

    ★ Through the harness's own `abs_rects_of`, which folds every scroll offset
    AND every enclosing clip — R1676 measured what a hand-rolled walk costs here:
    coordinates outside the viewport, and presses that went nowhere.
    """
    rects = abs_rects_of(app.snapshot(source="paint"))
    box = rects.get(f"lab.node.{card}")
    assert box is not None, f"the frame drew nothing at lab.node.{card}"
    x, y, w, h = box
    app.click((x + w / 2, y + h / 2))


# ─────────────────────────────────────────────────────────────────────────────
# (A) the assembled tool
# ─────────────────────────────────────────────────────────────────────────────
def section_a(app: RpcSubprocess) -> None:
    banner("A — the panel is a surface of the assembled tool, not a binary of its own")
    # ★ `sections` is the conformance report, keyed by seat — the same structure
    # the shell's own walk reads. Asked for the seat rather than for a string, so
    # a section that gained a field cannot break this read.
    # ⚠ `rows`, not `sections`: `sections` is the COUNT that rides beside the
    # rows so a client cannot disagree with the application about how much was
    # judged. Reading it as the list is the mistake it exists to prevent.
    sections: Any = app.query(f"{EXT}/sections")
    seats = [row["key"] for row in sections["rows"]]
    print(f"    {sections['sections']} section(s): {seats}")
    ok("the node lab is a section of this application", SEAT in seats)
    app.intervene_painted(f"{EXT}/nav", SEAT)
    assert_eq(app.query(f"{EXT}/nav"), SEAT, "the lab section opens")

    select_card(app)
    tags = set(walk_tags(app.snapshot(source="paint")))
    ok("its inspector paints the fault panel", "lab.faults" in tags)
    rows = sorted(
        t
        for t in tags
        if t.startswith("lab.faults.row.") and "." not in t[len("lab.faults.row.") :]
    )
    print(f"    {len(rows)} fault row(s) painted inside the assembled tool")
    ok("with rows in it", len(rows) >= 4)

    texts = walk_texts(app.snapshot(source="paint"))
    heads = [t for t in texts if t.startswith("fault injection")]
    assert_eq(len(heads), 1, "exactly one heading — two would be two accounts of one panel")
    counted = next(int(w) for w in heads[0].split() if w.isdigit())
    print(f"    the heading reads {heads[0]!r}")
    ok(
        "★ the heading counts the rows the panel painted, so a reader who reads "
        "the heading and a reader who counts the rows are told the same thing",
        counted == len(rows),
    )

    boundary = [t for t in texts if "faults are not offered" in t]
    print(f"    {len(boundary)} boundary sentence(s) on the assembled screen")
    for said in boundary:
        print(f"      {said[:96]}...")
    ok(
        "★★★★★ and the faults it CANNOT offer are named on the assembled screen "
        "-- an absence nobody names is indistinguishable from an oversight",
        len(boundary) >= 2,
    )


# ─────────────────────────────────────────────────────────────────────────────
# (B) what a reader is told
# ─────────────────────────────────────────────────────────────────────────────
def section_b(app: RpcSubprocess) -> None:
    banner("B — a reader walks the offers one at a time, with a position and a count")
    select_card(app)
    panel = announced(app, "lab.faults", leaves=False)
    ok("the panel itself is announced", "lab.faults" in panel)
    node = panel["lab.faults"]
    print(f"    role {node.get('role')!r}, name {node.get('name')!r}")
    ok(
        "★ as a LIST, which is what a reader navigates offers as -- folding them "
        "into the panel's value would make eight faults one paragraph",
        (node.get("role") or "").lower() in {"list", "arialist"},
    )
    ok(
        "and its value is the boundary, so a reader who never reaches the "
        "sentences still hears them",
        "not offered" in reading(node),
    )

    rows = announced(app, "lab.faults.row.")
    rows = {t: n for t, n in rows.items() if "." not in t[len("lab.faults.row.") :]}
    print(f"    {len(rows)} row(s) announced")
    assert_eq(
        sorted(node.get("children") or []),
        sorted(rows),
        "★ the panel's children ARE its rows -- a list whose children are a "
        "different set from its rows is two accounts of one panel",
    )
    for tag, row in sorted(rows.items()):
        name = row.get("name") or ""
        print(f"      {tag}: {name}")
        ok(
            f"{tag} says whether it blocks a launch",
            "blocks launch" in name or "warning" in name,
        )
        ok(
            f"{tag} carries the badge the FIELD declares",
            "hot" in name.lower() or "restart" in name.lower() or "form" in name.lower(),
        )
        ok(f"{tag} says which declaration admitted it", "admitted by" in name)
        pos = row.get("set_position") or row.get("position") or {}
        if isinstance(pos, dict) and pos:
            ok(f"{tag} knows where it is in the list", bool(pos))


# ─────────────────────────────────────────────────────────────────────────────
# (C) the derivation
# ─────────────────────────────────────────────────────────────────────────────
def section_c(app: RpcSubprocess) -> list[dict[str, Any]]:
    banner("C — every offer is a row of the declaration, confirmed by the encoder")
    select_card(app)
    faults: list[dict[str, Any]] = slot(app, "faults")
    print(f"    {len(faults)} offer(s) on the wire")
    ok("the panel publishes its offers", len(faults) >= 4)

    scopes: list[dict[str, Any]] = slot(app, "fault_scopes")
    arms = {row["scope"] for row in scopes}
    print(f"    scopes {sorted(arms)}")

    form: Any = slot(app, "form")
    held = {row["key"] for row in form}
    for one in faults:
        print(
            f"      {one['key']} · {one['kind']} = {one['value']!r} "
            f"[{one['applies']}] {'blocks' if one['blocks'] else 'warns'}"
        )
        ok(
            f"{one['key']} is a row the declaration HOLDS -- an offer at a key "
            "the form lacks would be an act the panel cannot perform",
            one["key"] in held,
        )
        ok(f"{one['key']}:{one['kind']} carries the value to use", bool(one["value"]))
        ok(
            f"{one['key']}:{one['kind']} says which declaration admitted it",
            bool(one["admitted_by"]),
        )
        ok(
            f"{one['key']}:{one['kind']} says whether it stops a launch",
            isinstance(one["blocks"], bool),
        )
    ok(
        "★★★★★ every offer this panel makes BLOCKS a launch, and that is not a "
        "coincidence: the only arm that merely warns is a key the declaration "
        "lacks, which the boundary puts out of a form's reach",
        all(one["blocks"] for one in faults),
    )
    return faults


# ─────────────────────────────────────────────────────────────────────────────
# (D) the counterfactual
# ─────────────────────────────────────────────────────────────────────────────
def section_d(app: RpcSubprocess, before: list[dict[str, Any]]) -> None:
    banner("D — ★★★★★ a row added to the declaration brings its faults with it")
    # ★ `offered` and not `known`: the first is what the form will ACCEPT right
    # now (`ConfigForm::addable`), the second the whole catalogue including rows
    # already on it. Adding a row the form already holds is refused, so the
    # counterfactual has to pick from the half that can move.
    catalogue: Any = slot(app, "catalogue")
    offered = [k for k in catalogue["offered"] if k]
    held = {one["key"] for one in before}
    print(f"    the form will accept {offered}")
    fresh = next((k for k in offered if k not in held), None)
    ok(
        "the declaration's catalogue offers a row the panel is not offering yet",
        fresh is not None,
    )
    assert fresh is not None
    print(f"    adding {fresh}")

    app.invoke(f"{EXT}/add_field", fresh)
    after: list[dict[str, Any]] = slot(app, "faults")
    mine = [one for one in after if one["key"] == fresh]
    print(f"    {fresh} brought {[one['kind'] for one in mine]}")
    ok(
        "★★★★★ THE COUNTERFACTUAL: the panel offers the new row's faults with no "
        "edit to the panel. A hand-maintained list of faults per widget kind "
        "would offer nothing here, which is what makes this the proof",
        len(mine) >= 1,
    )
    for one in mine:
        ok(
            f"and {fresh}:{one['kind']} carries the same five facts every other "
            "offer does",
            bool(one["value"]) and bool(one["admitted_by"]) and one["applies"],
        )

    # ⚠ The TOTAL is deliberately not asserted to have grown: measured at R1853,
    # adopting a row can make ANOTHER row worked out from it, and a row with no
    # written half cannot receive a value -- so its faults stop being offered and
    # the list can get SHORTER while gaining a key. That is the derivation
    # working, and an assertion on the count would demand it be wrong.
    print(f"    {len(before)} offer(s) -> {len(after)} while gaining {fresh}")

    app.invoke(f"{EXT}/remove_field", fresh)
    back: list[dict[str, Any]] = slot(app, "faults")
    ok(
        "and taking the row off the declaration takes its faults with it -- the "
        "panel follows the declaration in BOTH directions",
        not any(one["key"] == fresh for one in back),
    )


# ─────────────────────────────────────────────────────────────────────────────
# (E) the refusals, and the performance
# ─────────────────────────────────────────────────────────────────────────────
def section_e(app: RpcSubprocess, offers: list[dict[str, Any]]) -> None:
    banner("E — an offer is performed, and everything that is not an offer is refused")
    kinds = {row["kind"] for row in slot(app, "faults")}
    one = offers[0]

    before = slot(app, "verdict")
    print(f"    before: {before}")
    answer = app.invoke(f"{EXT}/inject", f"{one['key']}:{one['kind']}")
    print(f"    injected {answer!r}")
    ok(
        "the wire echoes what it injected, value included -- the value is the "
        "DERIVATION's and never the caller's",
        one["key"] in str(answer) and one["kind"] in str(answer),
    )
    after = slot(app, "verdict")
    print(f"    after:  {after}")
    ok(
        "★★★★★ and the pre-launch check now REFUSES the launch -- the refusal "
        "comes from the encoder, the single authority, not from the panel",
        after["blocking"] == before["blocking"] + 1 and not after["may_launch"],
    )

    # An arm the field does not admit, refused BY NAME with the arms it does.
    absent = next(
        (k for k in ("wrong_type", "out_of_range") if k not in {
            row["kind"] for row in slot(app, "faults") if row["key"] == one["key"]
        }),
        None,
    )
    if absent is not None:
        said = assert_action_refused(
            lambda: app.invoke(f"{EXT}/inject", f"{one['key']}:{absent}"),
            saying=one["key"],
        )
        print(f"    refused: {said}")
        ok(
            "★ an arm this field does not admit is refused BY NAME, and the "
            "refusal names the arms it DOES -- 'not offered' and 'no such field' "
            "are different facts and a caller has to tell them apart",
            "admits" in said,
        )

    # A word outside the published vocabulary, refused with the vocabulary.
    said = assert_action_refused(
        lambda: app.invoke(f"{EXT}/inject", f"{one['key']}:nonsense"),
        saying="nonsense",
    )
    print(f"    refused: {said}")
    ok(
        "★ a word outside the closed vocabulary is refused WITH the vocabulary, "
        "so an agent discovers the arms from the surface instead of guessing",
        all(kind in said for kind in kinds),
    )

    # A key the declaration does not hold. ★★★★★ THIS DEMO FOUND THE DEFECT
    # HERE: the refusal used to say the key's *declared shape accepts every
    # value*, which is a sentence about a declaration that is not there — the
    # exact conflation of *not offered* with *no such field* that the verb's own
    # comment promises not to make. A caller has to be able to tell them apart,
    # so the third refusal is now its own sentence.
    held = {row["key"] for row in slot(app, "form")}
    absent_key = "no.such.key"
    ok("the premise: the declaration really does not hold it", absent_key not in held)
    said = assert_action_refused(
        lambda: app.invoke(f"{EXT}/inject", f"{absent_key}:wrong_type"),
        saying=absent_key,
    )
    print(f"    refused: {said}")
    ok(
        "★★★★★ a key the declaration does not hold is refused as NOT A ROW, and "
        "is not told about a shape it has no declaration for",
        "not a row" in said and "accepts every value" not in said,
    )
    ok(
        "and the refusal says where the keys that ARE rows can be found",
        "faults" in said,
    )

    # Malformed argument.
    assert_action_refused(
        lambda: app.invoke(f"{EXT}/inject", "not-a-pair"), saying="not-a-pair"
    )
    ok("a malformed argument is refused rather than half-performed", True)


# ─────────────────────────────────────────────────────────────────────────────
# (F) the boundary, published
# ─────────────────────────────────────────────────────────────────────────────
def section_f(app: RpcSubprocess) -> None:
    banner("F — the boundary is a published vocabulary, not a sentence on a screen")
    scopes: list[dict[str, Any]] = slot(app, "fault_scopes")
    for row in scopes:
        print(f"    {row['scope']}: injectable={row['injectable']}")
        print(f"      {row['because']}")
    ok("every scope publishes whether it is injectable", all("injectable" in r for r in scopes))
    ok("and why, in a sentence a surface can show", all(r["because"] for r in scopes))
    injectable = [r["scope"] for r in scopes if r["injectable"]]
    print(f"    injectable: {injectable}")
    ok(
        "★★★★★ exactly ONE scope is injectable, which is what makes the other "
        "sentences load-bearing rather than decorative",
        len(injectable) == 1,
    )
    ok(
        "the two that are not are exactly the ones the screen names",
        len(scopes) - len(injectable) == 2,
    )


def body() -> None:
    # ★ The assembled tool first (rule 7): the capability belongs to a SECTION of
    # this application, reached the way every other section is.
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        resize_and_settle(app, LAB_WINDOW)
        section_a(app)

    # ★ Then the section's own process for the WIRE half. The mounted screen's
    # verbs are its own surface, and the shell does not forward them -- a host
    # that answered for a guest's actions would be a second implementation of
    # them. Two processes is the honest shape, which is R1742's arrangement.
    with RpcSubprocess(LAB, boot_grace=1.5) as lab:
        section_b(lab)
        offers = section_c(lab)
        section_d(lab, offers)
        section_e(lab, offers)
        section_f(lab)

    print(f"\n=== {len(CHECKS)} named check(s) ===")
    for line in CHECKS:
        print(f"  - {line}")


if __name__ == "__main__":
    run_demo("r1853 a fault panel is derived from the target's own settings", body)
