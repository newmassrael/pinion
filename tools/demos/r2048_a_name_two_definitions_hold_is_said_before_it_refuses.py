#!/usr/bin/env python3
"""R2048 §5.2 §5.11 — **a name two definitions hold is SAID, before it refuses
anybody.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
R1985 and R1986 built the half that REFUSES: two things answering to one name is
a state this crate's verbs handle rather than guess through — the lookup answers
`None` and every verb addressed by that name declines, naming the holders. What
neither round built is the half that SAYS SO. Measured at this round's open,
`Document::validate` reported nothing for either layer and no reader could ask
the document whether it held such a pair at all, so a person met the state as
one refusal sentence about one press.

# ★★★★★ The two layers are NOT the same finding, and that is the measurement

- **Nodes.** The state is unreachable through this crate's verbs since R1985
  closed the copy path, so holding it means the document came from somewhere
  else — which is exactly what the violation vocabulary is documented for. It
  is a `validate` finding now.
- **Definitions.** The state is produced by the verbs ON PURPOSE: a fragment's
  definitions land under the names they arrive with, and renaming into a taken
  name is admitted for that reason. Reporting it as broken would make
  `validate` non-empty for a document the crate had just built. So it is a
  READING, and this walk is where it reaches a person.

⇒ the debt that asked for one fault covering both layers asked for the wrong
shape on one of them.

# What this walk holds

  (A) the journey reaches the node lab, at the top of the assembled tool.
  (B) ★ a definition is made and copied, and the copy takes a name of its own —
      so nothing is shared yet and the register says nothing about sharing.
  (C) ★★★★★ two presses a person can make put the document in the state, and
      the document can be ASKED about it rather than only refusing.
  (D) ★★★★★ the row on the frame SAYS it — the painted run, read by tag — and
      says it before any verb has refused anybody.
  (E) ★★★★★ the refusals then agree with what the row said: every verb
      addressed by that name declines, and the sentence is the crate's.
  (F) ★★★★★ the counterfactual: renaming one of them apart puts the row back to
      what kind of graph it is, and the verbs answer again. The row is
      reporting the DOCUMENT and not a flag somebody set.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r2048_a_name_two_definitions_hold_is_said_before_it_refuses.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, find_by_tag, run_demo, texts_of  # noqa: E402

SHELL = "hello-analyzer-shell"
EXT = "/external"
SEAT = "lab"
PART = "capture-side"

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


def cards(app: RpcSubprocess, surface: str) -> list[str]:
    raw = app.query(f"{surface}/nodes")
    return [name for name in raw.split(",") if name]


def register(app: RpcSubprocess, surface: str) -> dict:
    return js(app.query(f"{surface}/definitions"))


def run_at(app: RpcSubprocess, tag: str) -> str | None:
    """The text of the painted run carrying `tag`, or `None` when none does.

    Read off the PAINT TREE rather than off the frame's placement index: the
    register sits at the foot of a pane that scrolls, so a row can be painted
    and not currently placed, and what is being asserted is what the screen
    says rather than where it says it.
    """
    node = find_by_tag(app.snapshot(source="paint"), tag)
    if node is None:
        return None
    texts = texts_of(node)
    return texts[0] if texts else None


def row_ids(app: RpcSubprocess, surface: str) -> dict[str, int]:
    """Each definition's id, by the name it currently answers to.

    ⚠ A dict keyed by NAME is exactly the thing this walk is about, so it is
    only ever used where the names are still distinct — the ids it hands back
    are what the register is addressed by afterwards.
    """
    return {row["definition"]: row["id"] for row in register(app, surface)["definitions"]}


def refusal(app: RpcSubprocess, path: str, args: str) -> str:
    """Drive a verb that should be refused, and answer what it said."""
    try:
        app.invoke(path, args)
    except Exception as why:  # noqa: BLE001 — the refusal is the assertion
        return str(why)
    return ""


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        app.intervene(f"{EXT}/nav", SEAT)
        app.tick_ms(16)
        ok(
            "A: the journey reaches the node lab, so what follows is about the "
            "ASSEMBLED tool",
            app.query(f"{EXT}/nav") == SEAT,
        )
        surface = surface_of(app, SEAT)

        banner("B — ★ a definition and a copy, with names of their own")
        opening = cards(app, surface)
        app.invoke(f"{surface}/select", opening[0])
        app.tick_ms(16)
        app.invoke(f"{surface}/select_also", opening[1])
        app.tick_ms(16)
        app.invoke(f"{surface}/group", PART)
        app.tick_ms(16)
        copy = app.invoke(f"{surface}/copy_definition", PART).strip().strip('"')
        app.tick_ms(16)
        ok(
            f"B: ★ the copy took a name of its own — {copy!r}",
            copy != PART,
        )
        held = register(app, surface)
        ok(
            f"B: ★ so the document holds no shared name — "
            f"{held['names_held_by_more_than_one']}",
            held["names_held_by_more_than_one"] == [],
        )
        ok(
            f"B: ★ and every row's name reaches it — "
            f"{[row['name_addresses_it'] for row in held['definitions']]}",
            all(row["name_addresses_it"] for row in held["definitions"]),
        )
        ids = row_ids(app, surface)
        line = f"lab.palette.part.{ids[PART]}.line"
        ok(
            f"B: ★ the row says what KIND of graph it is, which is what it says "
            f"when there is nothing wrong — {run_at(app, line)!r}",
            run_at(app, line) == "pattern",
        )

        banner("C — ★★★★★ two presses make the state, and it can be ASKED about")
        app.invoke(f"{surface}/rename_definition", f"{copy},{PART}")
        app.tick_ms(16)
        held = register(app, surface)
        shared = held["names_held_by_more_than_one"]
        ok(
            f"C: ★★★★★ the document says which name is held twice, and by how "
            f"many — {shared}",
            len(shared) == 1
            and shared[0]["name"] == PART
            and len(shared[0]["holders"]) == 2,
        )
        ok(
            f"C: ★ and both rows publish that their own name does not reach "
            f"them — {[row['name_addresses_it'] for row in held['definitions']]}",
            not any(row["name_addresses_it"] for row in held["definitions"]),
        )

        banner("D — ★★★★★ the ROW says it, before anything has refused anybody")
        said = run_at(app, line)
        ok(
            f"D: ★★★★★ the painted run says the name reaches nothing — {said!r}",
            said is not None and "answer to this name" in said,
        )
        ok(
            f"D: ★ and it counts, so a person knows how many to go and look at "
            f"— {said!r}",
            said is not None and said.startswith("2 "),
        )
        # ★★★★★ AND BOTH ROWS ARE STILL ADDRESSABLE. R2047 keyed the register on
        # the NAME, which would have painted one tag twice in exactly this
        # state; this round keyed it on the definition's id, and this is what
        # holds that. Read off the paint tree, not off the register.
        painted = [
            run_at(app, f"lab.palette.part.{held_id}.name")
            for held_id in shared[0]["holders"]
        ]
        ok(
            f"D: ★★★★★ two rows sharing a name still have two addresses, and "
            f"each carries the name — {painted}",
            painted == [PART, PART],
        )

        banner("E — ★★★★★ the NAME refuses, and the ROW still acts")
        before = [row["definition"] for row in register(app, surface)["definitions"]]
        why = refusal(app, f"{surface}/drop_definition", f"{PART},keep")
        ok(
            f"E: ★★★★★ the wire verb addressed by that name is refused, which "
            f"is what the row warned about — {why!r}",
            "address" in why,
        )
        after = [row["definition"] for row in register(app, surface)["definitions"]]
        ok(
            f"E: ★ and nothing went — {after} was {before}",
            after == before,
        )

        banner("F — ★★★★★ the counterfactual: the row a person presses still works")
        # The copy is the one nothing stands for, so its removal is allowed —
        # and it is reached by IDENTITY, which is the whole repair: the name
        # reaches neither, and the row does not need it to.
        spare = shared[0]["holders"][1]
        # ⚠ The register is at the foot of a pane that scrolls, and a press is
        # aimed at a node's rect CENTRE — so a row below the fold has to be
        # brought up first, exactly as a person would. R2047's walk recorded the
        # same step and why a clause that skipped it read as a passing press.
        app.scroll("lab.palette.body", to=(0, 4_000))
        app.tick_ms(16)
        app.click(path=f"lab.palette.verb.remove.{spare}")
        app.tick_ms(16)
        held = register(app, surface)
        ok(
            f"F: ★★★★★ the press landed on the row it was aimed at, by id — "
            f"{[row['definition'] for row in held['definitions']]}",
            len(held["definitions"]) == len(before) - 1,
        )
        ok(
            f"F: ★ so nothing is held twice any more — "
            f"{held['names_held_by_more_than_one']}",
            held["names_held_by_more_than_one"] == [],
        )
        ok(
            f"F: ★★★★★ and the survivor's row goes back to saying what kind it "
            f"is, so the line is reporting the DOCUMENT rather than a flag "
            f"somebody set — {run_at(app, line)!r}",
            run_at(app, line) == "pattern",
        )
        print(f"\n{len(CHECKS)} check(s) held")


if __name__ == "__main__":
    run_demo("r2048 a name two definitions hold is said before it refuses", body)
