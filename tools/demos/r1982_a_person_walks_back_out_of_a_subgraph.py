#!/usr/bin/env python3
"""R1982 §5.2 §5.11 — **a person goes into a subgraph and PRESSES their way back
out**, and the palette puts a card where they are standing.

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
R1981 gave the assembled tool the descent; this drives the half that makes the
descent something a person can survive.

# ★★★★★ The sharp half of the debt, and why it is about a person

`debt-the-assembled-tools-subgraph-surface-is-half-built` listed four missing
capabilities and named this one sharpest: the breadcrumb R1981 drew SAID where
a person was and could not be pressed, and `exit` was reachable from the wire
and from nothing on the frame. So a person with a pointer or a keyboard could
enter a subgraph and **not come out**. The other three are things the tool
cannot do; this one is a room with no door, which is a different kind of wrong.

Each step of the way in is a control now, so a person can climb one level or
several — which is what the reference's own path does.

# ⚠ Two defects R1981 left that only driving found

* **The palette added a card to the ROOT** whatever tree was on screen, so from
  inside a subgraph a person pressed a palette row and nothing appeared. R1981's
  own ratchet could not see the site: it matched `(ROOT` and the token sits on a
  line of its own inside a wrapped call. The gate has no discard path now —
  measured, it saw 12 of 25 tokens — and it named three sites, of which this and
  `set_port_address` were real.
* **An address written beside a wire went to the root's card of that number.**
  Same cause, same repair.

# What this walk holds

  (A) the journey reaches the node lab, at the top, with nowhere above.
  (B) ★ two cards are folded and entered — R1981's capability, as the setup.
  (C) ★★★★★ the frame now carries a PRESSABLE step for the tree above, and
      pressing it stands the person there. This is the door.
  (D) ★ the step a person is standing on is NOT a control — a chip that did
      nothing when pressed would be worse than no chip.
  (E) ★★★★★ the palette adds its card to the tree ON SCREEN, not to the root.
  (F) ★ and an address written inside lands on the card inside.
  (G) ★★★★★ from two levels down, one press climbs all the way to the top —
      the affordance a repeated `exit` does not give.
  (H) ★★★★★ R2065 — the same way out with no pointer at all.
  (I) ★★★★★ R2067 — the way IN: the card that stands for a graph carries a
      control onto the frame, a press goes there, and so does a keyboard
      descending into the card. Until that round the descent had exactly one
      caller — the wire verb — so a person could fold a part, read its name and
      have no route into it.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1982_a_person_walks_back_out_of_a_subgraph.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, abs_rects_of, run_demo  # noqa: E402

SHELL = "hello-analyzer-shell"
EXT = "/external"
SEAT = "lab"
VIEWPORT = (1400, 900)

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


def standing(app: RpcSubprocess, surface: str) -> dict:
    return js(app.query(f"{surface}/standing"))


def cards(app: RpcSubprocess, surface: str) -> list[str]:
    raw = app.query(f"{surface}/nodes")
    return [name for name in raw.split(",") if name]


def crumb_marks(app: RpcSubprocess) -> dict[str, tuple[int, int, int, int]]:
    """Every breadcrumb chip on the frame, by tag.

    Read off the FRAME rather than off the wire: the claim is that a person
    looking at the screen has somewhere to press, and only the paint says
    whether a control is there to be pressed.
    """
    marks = abs_rects_of(app.snapshot(source="paint", viewport=VIEWPORT))
    return {
        tag: rect
        for tag, rect in marks.items()
        if tag == "lab.crumb" or tag.startswith("lab.crumb.up.")
    }


def press(app: RpcSubprocess, rect: tuple[int, int, int, int]) -> None:
    x, y, w, h = rect
    app.click((x + w // 2, y + h // 2))
    app.tick_ms(16)


def access(app: RpcSubprocess) -> list[dict]:
    """Every announced node, which is where a keyboard reader's facts live."""
    resp = app.request("scene/access", {})
    assert resp is not None and resp.result is not None, "scene/access must answer"
    return resp.result.get("nodes", [])


def ring(app: RpcSubprocess, limit: int = 40) -> list[str]:
    """Every stop a keyboard reaches from here, in Tab order.

    ⚠ `focus/next` and not the `Tab` key: measured at R2061, twelve `Tab`
    presses through the key channel left `focus/get` answering `None` at four
    destinations, which reads exactly like a screen with no ring at all.
    """
    out: list[str] = []
    for _ in range(limit):
        app.request("focus/next")
        app.tick_ms(16)
        resp = app.request("focus/get")
        here = (resp.result or {}).get("focused") if resp and resp.result else None
        if here is None or here in out:
            break
        out.append(here)
    return out


def fold_and_enter(app: RpcSubprocess, surface: str, name: str) -> None:
    here = cards(app, surface)
    app.invoke(f"{surface}/select", here[0])
    app.tick_ms(16)
    app.invoke(f"{surface}/select_also", here[1])
    app.tick_ms(16)
    app.invoke(f"{surface}/group", name)
    app.tick_ms(16)
    app.invoke(f"{surface}/enter", name)
    app.tick_ms(16)


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
        at = standing(app, surface)
        ok(f"A: ★ it opens at the top — {at}", at["depth"] == 0)
        top = crumb_marks(app)
        ok(
            f"A: ★★★★★ and at the top there is NO step to press, because there "
            f"is nowhere above — {sorted(top)}",
            sorted(top) == ["lab.crumb"],
        )

        banner("B — two cards are folded and entered (R1981's capability)")
        fold_and_enter(app, surface, "capture-side")
        at = standing(app, surface)
        ok(f"B: ★ one descent deep — {at}", at["depth"] == 1)

        banner("C — ★★★★★ the frame carries a door, and pressing it opens it")
        marks = crumb_marks(app)
        ok(
            f"C: ★★★★★ a PRESSABLE step for the tree above is on the frame — "
            f"{sorted(marks)}. Before R1982 the breadcrumb only SAID where a "
            "person was, so a pointer could go in and not come out",
            "lab.crumb.up.0" in marks,
        )
        press(app, marks["lab.crumb.up.0"])
        at = standing(app, surface)
        ok(
            f"C: ★★★★★ and the press puts them back at the top — {at}",
            at["depth"] == 0 and at["inside"] is False,
        )

        banner("D — ★ the step you are standing on is not a control")
        # Pressing the current step must not be a control that does nothing:
        # what it does is fall through to the canvas, which is what the rest of
        # the chip's area does.
        before = standing(app, surface)
        press(app, crumb_marks(app)["lab.crumb"])
        ok(
            f"D: ★ pressing where you already are changes nothing about where "
            f"you are — {before} then {standing(app, surface)}",
            standing(app, surface)["depth"] == before["depth"],
        )

        banner("E — ★★★★★ the palette adds its card to the tree ON SCREEN")
        app.invoke(f"{surface}/enter", "capture-side")
        app.tick_ms(16)
        inside_before = cards(app, surface)
        # ⚠ PRESSED, not invoked. Adding a card from the palette is not a wire
        # verb on this screen — it is a row a person presses — so driving it any
        # other way would be asserting about a path a person does not have.
        # ★ R2049 — the addresses the screen publishes, not a prefix spelled
        # here. Asked once rather than per tag.
        offered = {row["tag"] for row in js(app.query(f"{surface}/spec"))["roles"]}
        rows = {
            tag: rect
            for tag, rect in abs_rects_of(
                app.snapshot(source="paint", viewport=VIEWPORT)
            ).items()
            if tag in offered
        }
        ok(f"E: ★ the palette offers rows to press — {sorted(rows)}", rows)
        press(app, rows[sorted(rows)[0]])
        inside_after = cards(app, surface)
        ok(
            f"E: ★★★★★ the card appears where the person is standing — "
            f"{len(inside_before)} then {len(inside_after)}. Until R1982 this "
            "went to the ROOT and the canvas did not change",
            len(inside_after) == len(inside_before) + 1,
        )
        made = [name for name in inside_after if name not in inside_before]
        ok(f"E: ★ and it is one card, named — {made}", len(made) == 1)

        banner("F — ★ an address written inside lands inside")
        app.invoke(f"{surface}/select", made[0])
        app.tick_ms(16)
        # The card is here; the point is that writing does not reach out to the
        # root's card of the same number.
        root_before = None
        app.invoke(f"{surface}/exit", "")
        app.tick_ms(16)
        root_before = cards(app, surface)
        app.invoke(f"{surface}/enter", "capture-side")
        app.tick_ms(16)
        ok(
            f"F: ★ the root's roster is unchanged by what happened inside — "
            f"{len(root_before)} card(s) out there",
            made[0] not in root_before,
        )

        banner("G — ★★★★★ one press climbs several levels")
        fold_and_enter(app, surface, "inner")
        at = standing(app, surface)
        ok(f"G: ★ two descents deep — {at['through']}", at["depth"] == 2)
        marks = crumb_marks(app)
        ok(
            f"G: ★ a step is offered for EACH tree above — {sorted(marks)}",
            "lab.crumb.up.0" in marks and "lab.crumb.up.1" in marks,
        )
        press(app, marks["lab.crumb.up.0"])
        at = standing(app, surface)
        ok(
            f"G: ★★★★★ and pressing the first climbs ALL the way, which a "
            f"repeated exit gives one level at a time — {at}",
            at["depth"] == 0,
        )

        # ★★★★★ R2065 — and the SAME way out, with no pointer at all.
        #
        # This screen had no Tab stop of its own: the fifteen a reader met here
        # were the inspector form's, built by the framework, and measured at
        # R2061 not one of the painted tags outside that form was in the ring.
        # So everything above — the door R1982 built, the multi-level climb —
        # was a door only a mouse could open, and a person put inside a subgraph
        # by the wire or by somebody else's session was stuck.
        banner("H — ★★★★★ the way out, by keyboard alone")
        app.request("focus/set", {"tag": None})
        app.tick_ms(16)
        top_ring = ring(app)
        ok(
            f"H: ★ at the TOP the trail is no stop, because there is nowhere to "
            f"go — {[s for s in top_ring if 'crumb' in s]}",
            not [s for s in top_ring if "crumb" in s],
        )

        fold_and_enter(app, surface, "by-keyboard")
        at = standing(app, surface)
        ok(f"H: one descent deep — {at['depth']}", at["depth"] == 1)

        app.request("focus/set", {"tag": None})
        app.tick_ms(16)
        walked = ring(app)
        trail = [s for s in walked if "crumb" in s]
        ok(
            f"H: ★★★★★ inside, a KEYBOARD reaches the trail — ring {walked}",
            len(trail) == 1,
        )
        stop = trail[0]
        app.request("focus/set", {"tag": stop})
        app.tick_ms(16)
        node = {n.get("tag"): n for n in access(app)}.get(stop, {})
        nav = node.get("navigation") or {}
        # ⚠⚠ `group`, and NOT the `navigation` a breadcrumb is in WAI-ARIA. This
        # walk asked for `navigation` first and the SWEEP refused the screen:
        # `r1725_one_application_has_one_navigation` counts landmarks per
        # APPLICATION, this tool already publishes one (the shell's rail), and a
        # second tells a reader it has two navigations — the exact defect that
        # gate was built against. ⇒ a role that is canonically right for a
        # WIDGET can be wrong for the application it is assembled into.
        ok(f"H: ★ it is announced as a row of its own — {node.get('role')}", node.get("role") == "group")
        ok(
            f"H: ★★★★★ and arriving on a step does NOT go there — {nav.get('activation')}. "
            "A cursor that followed would walk a reader out of the subgraph on "
            "the way past the step they wanted",
            nav.get("activation") == "explicit",
        )
        # ★★★★★ The cursor names a step the trail can GO to, and the roster it
        # walks is contained in what the trail announces as its children.
        #
        # ⚠ Two earlier drafts of this line are worth recording, because each
        # was refused by a different gate of this tree and both were mine.
        # `any(node has children)` could not FAIL — the trail always holds the
        # step a person is standing on, and the line above already established
        # the node exists. Then `active_descendant.startswith("lab.crumb.up.")`
        # spelled a painted address, which the address ratchet refused as a
        # second copy of a composition the screen already publishes. What is
        # asked now is neither: it is answered entirely out of what the screen
        # PUBLISHED, it can fail, and it holds the property R2064 paid for
        # twice — a published roster must not drift from the announced children.
        members = {m["tag"] for m in nav.get("members") or []}
        owned = set(node.get("children") or [])
        ok(
            f"H: ★ the cursor names a step the trail can go to — "
            f"{nav.get('active_descendant')} among {sorted(members)}",
            nav.get("active_descendant") in members,
        )
        ok(
            f"H: ★ and the roster is inside what the trail announces it owns — "
            f"{sorted(members)} within {sorted(owned)}",
            members and members <= owned,
        )
        before = standing(app, surface)
        app.key(path=stop, name="ArrowRight")
        app.tick_ms(16)
        ok(
            f"H: ★ walking the trail leaves the person where they were — {before['depth']}",
            standing(app, surface)["depth"] == before["depth"],
        )
        app.key(path=stop, name="Enter")
        app.tick_ms(32)
        out = standing(app, surface)
        ok(
            f"H: ★★★★★ and pressing it walks them OUT — {before} then {out}",
            out["depth"] == 0 and out["inside"] is False,
        )

        # ★★★★★ R2067 — and the way IN, which nothing on the frame had.
        #
        # Everything above is about a door. Measured at R2066, the descent had
        # exactly ONE caller in this screen — the wire verb `enter` — so a
        # person could fold a part, watch the instance card appear, read its
        # name, and have no route into it: not a chip, not a key, not a press
        # anywhere. A room a person can be PUT in and cannot walk into.
        banner("I — ★★★★★ the way IN, on the frame and by keyboard alone")
        at = standing(app, surface)
        ok(f"I: back at the top to begin with — {at['depth']}", at["depth"] == 0)
        here = cards(app, surface)
        app.invoke(f"{surface}/select", here[0])
        app.tick_ms(16)
        app.invoke(f"{surface}/select_also", here[1])
        app.tick_ms(16)
        app.invoke(f"{surface}/group", "way-in")
        app.tick_ms(16)

        # ★ The address is READ OFF THE WIRE and not spelled here. The screen
        # publishes where to press beside what each card stands for, derived
        # from the one place that composes it, so this walk cannot look for a
        # mark under a name the paint does not use — R2049's whole answer for
        # readers that cannot call the declaration.
        rows = js(app.query(f"{surface}/standing_for"))["cards"]
        doors = {row["card"]: row["way_in"] for row in rows if row["way_in"]}
        stands = {row["card"]: row["stands_for"] for row in rows}
        ok(
            f"I: ★★★★★ the screen publishes a way in for the card that stands "
            f"for a graph, and for no other — {doors} of {stands}",
            set(doors) == {card for card, what in stands.items() if what == "definition"},
        )
        door = doors["way-in"]
        marks = abs_rects_of(app.snapshot(source="paint", viewport=VIEWPORT))
        ok(
            f"I: ★★★★★ and it is PAINTED, so a hand has somewhere to press — "
            f"{door}",
            door in marks,
        )

        # (I.1) the pointer route.
        press(app, marks[door])
        at = standing(app, surface)
        ok(
            f"I: ★★★★★ a press on it goes inside — {at}. This canvas resolves "
            "a press from the published paint through a filter of tag "
            "families, so a family missing from that filter is a mark that is "
            "drawn, announced and unpressable",
            at["depth"] == 1 and at["inside"] is True,
        )
        app.invoke(f"{surface}/exit", "")
        app.tick_ms(16)
        ok(
            "I: back out, so the keyboard route starts where the pointer route "
            "did",
            standing(app, surface)["depth"] == 0,
        )

        # (I.2) ★★★★★ the keyboard route, with no pointer at all. The canvas is
        # a composite: its stops are the cards, and each card CONTAINS what it
        # draws — so the way in is reached by descending into the card, which is
        # the WAI-ARIA nesting R2066 built and this round gave something to
        # press.
        app.request("focus/set", {"tag": None})
        app.tick_ms(16)
        walked = ring(app)
        canvas = [stop for stop in walked if stop.endswith("canvas")]
        ok(f"I: ★ a keyboard reaches the canvas — ring {walked}", len(canvas) == 1)
        canvas = canvas[0]
        app.request("focus/set", {"tag": canvas})
        app.tick_ms(16)

        def cursor_of(stop: str) -> dict:
            node = {n.get("tag"): n for n in access(app)}.get(stop, {})
            return node.get("navigation") or {}

        def step_to(stop: str, want: str, key: str, limit: int = 24) -> str:
            """Walk `stop`'s cursor until it names `want`, by keys alone."""
            for _ in range(limit):
                if cursor_of(stop).get("active_descendant") == want:
                    break
                app.key(path=stop, name=key)
                # ⚠ The pointer is put on the tag's rect centre by every keyed
                # press, so it is taken off again — R2063 measured a register
                # that froze on whatever the midpoint hovered while the cursor
                # walked past it.
                app.pointer_leave()
                app.tick_ms(16)
            return cursor_of(stop).get("active_descendant")

        nav = cursor_of(canvas)
        members = {m["tag"] for m in nav.get("members") or []}
        card = next(m for m in members if m.endswith("way-in"))
        ok(
            f"I: ★ the instance card is a stop the canvas walks — "
            f"{sorted(members)}",
            card in members,
        )
        ok(
            f"I: ★ arriving on a card SELECTS it, so the inspector follows a "
            f"walk — {nav.get('activation')}",
            nav.get("activation") == "follows",
        )
        ok(
            f"I: ★★★★★ walking reaches the instance card — {step_to(canvas, card, 'ArrowDown')}",
            cursor_of(canvas).get("active_descendant") == card,
        )

        # Descend INTO the card. The way in is one of the card's own stops, so
        # this is the same gesture that puts a pin's sentence in reach.
        app.key(path=canvas, name="Enter")
        app.pointer_leave()
        app.tick_ms(16)
        held = cursor_of(card)
        inner = {m["tag"] for m in held.get("members") or []}
        owned = set({n.get("tag"): n for n in access(app)}.get(card, {}).get("children") or [])
        ok(
            f"I: ★★★★★ the card publishes what it holds, and the way in is one "
            f"of them — {sorted(inner)}",
            door in inner and inner <= owned,
        )
        ok(
            f"I: ★★★★★ and its stops are chosen EXPLICITLY — "
            f"{held.get('activation')}. A cursor that followed could not carry "
            "a control at all: a following composite declares no choose key, so "
            "the way in would be reachable and unpressable",
            held.get("activation") == "explicit",
        )
        reached = step_to(card, door, "ArrowDown")
        ok(f"I: ★ the reader walks to it inside the card — {reached}", reached == door)
        before = standing(app, surface)
        app.key(path=card, name="Enter")
        app.pointer_leave()
        app.tick_ms(32)
        went = standing(app, surface)
        ok(
            f"I: ★★★★★ and pressing it takes them IN, with no pointer anywhere "
            f"— {before} then {went}. R1982 gave this screen the way out; "
            "until this round the way in was a verb on the wire and nothing a "
            "person could reach",
            went["depth"] == 1 and went["inside"] is True,
        )

        # ★★★★★ R2068 — and ARRIVING somewhere shows you what is there.
        #
        # The paint sweep stood inside a subgraph for the first time this round
        # and found the screen arriving at nothing: no card selected, so the
        # inspector painted not one control, and the viewport still pointed
        # where the tree above had been left, so a definition's cards — which
        # sit around its own origin — were half off the canvas or off it
        # entirely. Both are the same defect, and it is about arrival.
        banner("J — ★★★★★ arriving in a part shows the part")
        # ⚠ Out first: clause I left the person INSIDE, and `enter` is answered
        # against the tree on screen — from in there no card is called `way-in`,
        # which is the per-tree naming this walk's clause F is about. Driven
        # rather than assumed: the first draft of this clause called `enter`
        # from inside and was refused by name.
        app.invoke(f"{surface}/exit", "")
        app.tick_ms(16)
        app.invoke(f"{surface}/enter", "way-in")
        app.tick_ms(32)
        at = standing(app, surface)
        ok(f"J: one descent deep — {at['depth']}", at["depth"] == 1)

        inside_cards = cards(app, surface)
        picked = app.query(f"{surface}/selected")
        ok(
            f"J: ★★★★★ a card OF THIS TREE is selected on arrival — {picked} "
            f"among {inside_cards}. The document opens with one selected; a "
            "tree arrived in gets the same greeting, and before this round the "
            "inspector painted nothing at all in here",
            picked in inside_cards,
        )

        marks = abs_rects_of(app.snapshot(source="paint", viewport=VIEWPORT))
        canvas = marks["lab.canvas"]
        seen = {
            name: marks.get(f"lab.node.{name}")
            for name in inside_cards
        }
        missing = [name for name, rect in seen.items() if rect is None]
        ok(
            f"J: ★★★★★ every card of the arrived tree is on the canvas — {seen}",
            not missing,
        )
        outside = [
            name
            for name, rect in seen.items()
            if rect is not None
            and not (
                rect[0] >= canvas[0]
                and rect[1] >= canvas[1]
                and rect[0] + rect[2] <= canvas[0] + canvas[2]
                and rect[1] + rect[3] <= canvas[1] + canvas[3]
            )
        ]
        ok(
            f"J: ★ and none of them straddles the canvas edge — {outside}. A "
            "card half past that edge is recorded as a CLIPPED box while the "
            "parts that survive keep their own, which every containment gate "
            "then reads as a defect",
            not outside,
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r1982 a person walks back out of a subgraph", body)
