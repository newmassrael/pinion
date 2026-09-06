#!/usr/bin/env python3
"""R1999 §5.2 §5.11 — **a graph says what kind it is, and what it will take.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
R1981-R1983 gave the assembled tool the subgraph verbs, R1985-R1986 gave it the
definition register, and R1988 aimed the graph from a selection. This is the
axis those all sit on and none of them names: **what a graph IS**.

The engine's graph schema publishes a hook answering *what type of graph is
this*, and the three things measured at its own header are what this walk is
written against:

  * the vocabulary is a **fixed five-member enumeration** declared beside the
    schema base class, and the comment directly above the hook says in its own
    words that it is too specific to one editor to belong there;
  * the supplied body **ignores the graph it is handed** and answers the first
    member, so *this is a function graph* and *I could not classify this* are
    one value a caller cannot tell apart;
  * the largest group of its **53** consumers is the per-node-type *are you
    compatible with this graph* test — **sixteen** calls in **fifteen** node
    classes, four times the next largest group, each re-writing the same
    comparison, so a node type added afterwards is compatible with everything
    until somebody edits one more.

# What this walk holds

  (A) the journey reaches the node lab, at the top of the assembled tool, and
      the graph a person opens on says what it is — a deployment, which refuses
      NOTHING. Without this the refusal in (C) would be indistinguishable from
      a screen that refuses routers everywhere.
  (B) ★ a folded part is a PATTERN from the moment it exists, said from OUT
      HERE by the definition register before anybody steps inside.
  (C) ★★★★★ inside it, the screen names what this kind of graph will not take
      **before anything is pressed** — where the reference's palette filter and
      its per-node refusal are two unrelated pieces of code and a person there
      finds out by being refused.
  (D) ★★★★★ the counterfactual, in the same graph: the other seven roles are
      still offered, so the rule is the router's and not a pane that shuts on
      descent.
  (E) ★★★★★ the OFFER and the REFUSAL are one predicate — the placement the
      list refuses is actually refused, and the refusal is SAID.
  (F) ★★★★★ a person may re-classify the graph they are in; nothing is deleted,
      what the change left out of place is named, and a word this screen does
      not have is refused by naming the ones that work.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1999_a_graph_says_what_kind_it_is.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, run_demo  # noqa: E402

SHELL = "hello-analyzer-shell"
EXT = "/external"
SEAT = "lab"
PART = "edge-side"

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


def kind(app: RpcSubprocess, surface: str) -> dict:
    return js(app.query(f"{surface}/graph_kind"))


def definitions(app: RpcSubprocess, surface: str) -> list[dict]:
    return js(app.query(f"{surface}/definitions"))["definitions"]


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

        banner("A — the graph a person opens on says what it is, and refuses nothing")
        here = kind(app, surface)
        ok(f"A: ★ the opening graph is a deployment — {here['kind']}", here["kind"] == "deployment")
        ok(
            f"A: ★ and it says what that is FOR, which the reference's own hook "
            f"has no member for — {here['gist']!r}",
            here["gist"] != "",
        )
        ok(
            f"A: ★★★★★ it refuses NOTHING, so the refusal below is the "
            f"PATTERN's rule — a screen that refused a router everywhere would "
            f"pass (C) just as well — {here['refuses']}",
            here["refuses"] == [],
        )
        ok(
            f"A: ★ the router among what it takes — {here['takes']}",
            "Router" in here["takes"],
        )

        banner("B — ★ a folded part is a PATTERN, said before anybody steps in")
        opening = cards(app, surface)
        app.invoke(f"{surface}/select", opening[0])
        app.tick_ms(16)
        app.invoke(f"{surface}/select_also", opening[1])
        app.tick_ms(16)
        app.invoke(f"{surface}/group", PART)
        app.tick_ms(16)
        rows = definitions(app, surface)
        part = next(row for row in rows if row["definition"] == PART)
        ok(
            f"B: ★ the register says what KIND the definition is, from out "
            f"here — {part}",
            part["kind"] == "pattern",
        )
        ok(
            f"B: ★ and the graph the person is STILL standing in has not "
            f"changed kind — {kind(app, surface)['kind']}",
            kind(app, surface)["kind"] == "deployment",
        )

        banner("C — ★★★★★ inside it, what this graph will not take, named before a press")
        app.invoke(f"{surface}/enter", PART)
        app.tick_ms(16)
        inside = kind(app, surface)
        ok(f"C: ★ the screen says where the person is — {inside['kind']}", inside["kind"] == "pattern")
        ok(
            f"C: ★★★★★ and NAMES what it will not take, before anything is "
            f"pressed — {inside['refuses']}",
            inside["refuses"] == ["Router"],
        )

        banner("D — ★★★★★ the counterfactual: everything else is still offered")
        ok(
            f"D: ★★★★★ seven of the eight roles are still offered inside a "
            f"pattern, so the rule is the ROUTER's and not a pane that shuts on "
            f"descent — {inside['takes']}",
            len(inside["takes"]) == 7,
        )
        ok(
            f"D: ★ a peer among them, which is the router's own family — "
            f"{inside['takes']}",
            "Peer" in inside["takes"],
        )

        banner("E — ★★★★★ the offer and the refusal are ONE predicate")
        # ★ Through the PRESS a person makes, not a wire verb: the palette has
        # no verb of its own here, so what is being asserted is that the row a
        # person can put a cursor on refuses — a greying that were only a colour
        # would take the card anyway.
        held = cards(app, surface)
        # ★ R2049 — the addresses the screen publishes, not ones spelled here.
        row_of = {
            role["name"]: role["tag"] for role in js(app.query(f"{surface}/spec"))["roles"]
        }
        app.click(path=row_of["Router"])
        app.tick_ms(16)
        ok(
            f"E: ★★★★★ nothing was added, whatever the row looked like — "
            f"{len(cards(app, surface))} card(s), was {len(held)}",
            cards(app, surface) == held,
        )
        said = js(app.query(f"{surface}/said"))
        ok(
            f"E: ★★★★★ and the refusal is SAID — a press that did nothing and "
            f"reported nothing is what this replaced — {said}",
            said is not None,
        )
        ok(
            f"E: ★ naming the role and the kind of graph, in this screen's own "
            f"words rather than the crate's identity tokens — {said}",
            "Router" in said["clause"] and "pattern" in said["clause"],
        )
        ok(
            f"E: ★ and it is framed as a REFUSAL, not as a thing that was done "
            f"— {said['tone']}",
            said["tone"] == "refused",
        )
        # ★ And a role the pattern DOES take still lands, so (E) is not a screen
        # that stopped taking cards on descent.
        app.click(path=row_of["Peer"])
        app.tick_ms(16)
        ok(
            f"E: ★★★★★ while a peer still lands — {len(cards(app, surface))} "
            f"card(s), was {len(held)}",
            len(cards(app, surface)) == len(held) + 1,
        )

        banner("F — ★★★★★ a person may re-classify the graph they are in")
        before = cards(app, surface)
        answer = app.invoke(f"{surface}/set_graph_kind", "deployment")
        ok(f"F: ★ the answer says what it is now — {answer!r}", "deployment" in answer)
        ok(
            f"F: ★ and it now refuses nothing, so the greying follows the KIND "
            f"rather than the tree — {kind(app, surface)['refuses']}",
            kind(app, surface)["refuses"] == [],
        )
        ok(
            f"F: ★★★★★ and re-classifying DELETED NOTHING — the crate reports "
            f"what a narrowing left behind rather than removing it, because "
            f"removing a card takes its wires with it — {len(cards(app, surface))}",
            cards(app, surface) == before,
        )
        why = refusal(app, f"{surface}/set_graph_kind", "ubergraph")
        ok(
            f"F: ★ a word this screen does not have is refused BY NAMING the "
            f"ones that work — {why!r}",
            "deployment" in why and "pattern" in why,
        )
        app.invoke(f"{surface}/exit", "")
        app.tick_ms(16)
        ok(
            f"F: ★ and back out, where the deployment still says what it is — "
            f"{kind(app, surface)['kind']}",
            kind(app, surface)["kind"] == "deployment",
        )
        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1999 a graph says what kind it is", body)
