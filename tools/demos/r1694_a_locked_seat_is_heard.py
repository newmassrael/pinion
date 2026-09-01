#!/usr/bin/env python3
"""R1694 — **screen C, announced**: the analyzer's dashboard reproduced against
its own specification, over the wire, for a reader who never sees the drawing.

Before this round `hello-analyzer-shell` painted **128** addressable regions and
announced **five** accessibility nodes: a group for the window and one per card,
each holding nothing. `scene/conform` judged **zero** nodes on this screen — not
"zero faults", zero *judged*, because no announced role carried a structural
requirement at all. The rail, both bars, two tables, the decode tree, the
seventy-two bytes and the whole thirteen-entry palette reached a reader as four
names and a summary sentence.

And with them went the screen's own subject. This is the dashboard whose point
is that **nine catalogue seats are locked and each says what it is booked
under** — *"a second-release item is not a missing thing, it is a locked seat"*.
The framework has computed that reason since R1668 (a kind, a detail, and a
recourse derived from the kind) and published it on `scene/disabled` for all
**eleven** locked regions. Not one of the eleven had an accessibility node, so
the reason was computed, published, painted as faded ink — and inaudible.

## The floor, built and run at 6.11.1 rather than read

The same shape assembled there — a rail of seven seats with two locked, a
thirteen-entry palette in three sections with nine locked, a tab bar with a
locked tab, and a locked plain button — and its accessibility tree read back:

* the locked **palette entries** answer `focusable, selectable` and carry **no
  unavailable state at all**. Both spellings were measured, the convenience item
  and a model whose `flags()` drops the enabled bit; both lose it.
* the locked **tab** answers `focusable, selectable` too. So on that screen a
  reader is invited to activate exactly the seats the screen has closed.
* the locked **plain widget** does carry the bit, which is how we know the probe
  is sound and the loss belongs to collections.
* the palette's three **section headings** come back as list items indis-
  tinguishable in role from the entries: sixteen flat items, no grouping, and
  no way to derive "thirteen entries, nine reserved" from what is announced.
* the one slot that could carry a reason is a free-form description that
  nothing classifies and nothing links to the lock — and on a toolbar action it
  DEFAULTS to the action's own label, which the probe shows directly: every
  unlocked seat comes back with `description == name`.

What this demo drives:

  (A) the SPECIFICATION off the wire — every population below is expanded from
      what the running application publishes, so a table that drifts fails here
      rather than being quietly re-asserted;
  (B) the SCREEN — the reference's own metrics, still true;
  (C) VOICE — every addressable region classified, and the split between what
      speaks and what is deliberately quiet is the specification's, both ways;
  (D) STRUCTURE — every announced collection holds what its role promises, and
      every member is inside the collection its role requires;
  (E) the LOCKED SEATS — each announced, named, unavailable, carrying the kind,
      the detail and the recourse, and **keeping its place in the set**;
  (F) the GRIDS — every cell addressable, saying which row and column it is in,
      under a header row of column headers;
  (G) the TREE — one item per decoded field, carrying its depth AND its value;
  (H) DRIVEN — a real press through the router moves what is announced;
  (I) the DISCRIMINATING read — the censuses track the live tree rather than
      reporting a constant, shown by the numbers moving when the screen does.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from analyzer_spec import (  # noqa: E402
    closed_keys,
    opening_kinds,
    reserved_palette_kinds,
)
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
    settle_saying,
)

EXAMPLE = "hello-analyzer-shell"
VIEW = "analyzer_shell"
EXT = f"/{VIEW}/external"

CHECKS: list[str] = []


#: The rail seats the specification says are shut, read from the reviewed
#: artifact rather than written out — so a round that builds a section moves
#: this demo's expectation by itself.
closed_rail_keys = closed_keys


def banner(what: str) -> None:
    print(f"[demo] -- {what}")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def q(app: RpcSubprocess, path: str):
    return app.query(f"{EXT}/{path}")


def nodes_by_tag(app: RpcSubprocess) -> dict:
    return {n["tag"]: n for n in app.request("scene/access").result["nodes"]}


def voice_by_tag(app: RpcSubprocess) -> dict:
    return {n["tag"]: n for n in app.request("scene/voice").result["nodes"]}


def body() -> None:
    with RpcSubprocess(EXAMPLE) as app:
        # ── (A) the specification, off the wire ────────────────────────────
        banner("A — the screen publishes the specification it is built against")
        spec = q(app, "spec")
        ok("the screen publishes a specification", isinstance(spec, dict))
        voices = spec["voices"]
        silences = spec["silences"]
        locked = spec["locked"]
        ok("the specification declares what owes a voice", len(voices) > 0)
        ok("the specification declares what owes a silence", len(silences) > 0)
        ok("the specification declares which seats are locked", len(locked) > 0)
        # ★ R1695 — sixteen, and the five that joined are that round's finding:
        # three rail destinations this application cannot take you to (painted
        # live and refusing nothing until then) and the settings page's own two
        # booked affordances. Each row now says WHICH destination it belongs to,
        # so this demo can ask about the one the screen opens at.
        # ★★★★★ R1724 — Catalog's page is the node graph lab, mounted
        # (`pinion_screen::Mount<NodeLabView>`), so that rail seat is open and
        # left this table by being open. Nobody edited the table — it is derived
        # from the seat's own standing, which is what "derived rather than
        # listed" was for.
        # ★★★★★ R1730 — and the COUNT is derived now too. It was written out
        # (sixteen, then fifteen, each edited by hand in the round that moved
        # it), and the round that built the key-pattern section broke it: a
        # number in a demo goes stale exactly like a number in prose.
        rail_locked = sorted(r["tag"] for r in locked if r["tag"].startswith("shell.rail."))
        assert_eq(
            rail_locked,
            sorted(f"shell.rail.{k}" for k in closed_rail_keys()),
            "A: the rail seats declared unavailable are the ones the "
            "specification says are shut, and no others",
        )
        # ★★★★★ R1797 — and THIS count is derived too, which the paragraph above
        # argued for and then did not do: it wrote `11` one line below "a number
        # in a demo goes stale exactly like a number in prose", and the round
        # that promoted a palette widget out of the second release broke it —
        # exactly as predicted, by exactly the mechanism named.
        #
        # The rest splits in two and each half has a published count to answer
        # to: the palette's reserved widgets, and the settings page's booked key
        # affordances. Asserting the halves separately also says WHICH one moved
        # when one does, which the single total could not.
        others = [r for r in locked if not r["tag"].startswith("shell.rail.")]
        palette_locked = [r for r in others if r["tag"].startswith("shell.palette.")]
        assert_eq(
            len(palette_locked),
            spec["reserved_count"],
            "A: the palette seats declared unavailable are the ones a later "
            "release brings, and the footer counts the same set",
        )
        assert_eq(
            len(others) - len(palette_locked),
            len(spec["key_rows"]),
            "A: and the rest are the settings page's booked affordances, which "
            "no rail change touches",
        )
        here = spec["rail_active"]
        on_screen = [row["tag"] for row in locked if row["at"] in ("*", here)]
        ok(
            "A: every locked rail seat is one the opening screen shows",
            all(tag in on_screen for tag in rail_locked),
        )
        ok(
            "A: and some locked regions belong to a destination this screen is "
            "not at, which is why the table says WHERE each one lives",
            len(on_screen) < len(locked),
        )
        locked = on_screen
        assert_eq(
            len(spec["catalogue"]),
            13,
            "A: thirteen catalogue entries",
        )
        # ★ R1797 — five and eight since the latency card was promoted. The
        # TOTAL is what a number here can honestly pin: thirteen entries, split
        # by a release decision that moves. Asserting the split as two literals
        # made this demo fail for a change that is the release plan working.
        #
        # ★★★★★ R1846 — AND R1797 WROTE THAT SENTENCE AND LEFT THE TWO LITERALS
        # STANDING ON THE NEXT LINE. R1843 promoted the health card, the split
        # became six and seven, and this demo failed for exactly the reason the
        # comment above names — for two rounds, because nobody ran it. The
        # diagnosis was right and the repair was never made, so the split is now
        # read from the specifications: the board from
        # `docs/analyzer-dashboard-spec.json`, what is still booked from
        # `docs/analyzer-reserved-spec.json`'s *deferred minus built*.
        assert_eq(
            spec["placeable_count"] + spec["reserved_count"],
            len(spec["catalogue"]),
            "A: every catalogue entry is either placed this release or reserved",
        )
        assert_eq(
            spec["placeable_count"],
            len(opening_kinds()),
            "A: the split is the specification's, on the placed side",
        )
        assert_eq(
            spec["reserved_count"],
            len(reserved_palette_kinds()),
            "A: and on the booked side",
        )
        print(
            f"[demo] {len(voices)} region(s) owe a voice, {len(silences)} owe a "
            f"silence, {len(locked)} seat(s) are locked"
        )

        # ── (B) the screen is still the reference's ────────────────────────
        banner("B — the reference's own metrics")
        metrics = spec["metrics"]
        # ★ WINDOW space, not the paint tree's nested space. A rectangle read
        # raw out of the snapshot is relative to whatever clipped it, and a
        # press aimed at one lands somewhere else entirely: the first draft of
        # this demo aimed at a card's close control and hit the application
        # bar, which the screen reported by changing its capture source.
        rects = abs_rects_of(app.snapshot(source="paint"))
        assert_eq(rects["shell.appbar"][3], metrics["app_bar_h"], "B: app bar height")
        assert_eq(rects["shell.subbar"][3], metrics["sub_bar_h"], "B: layout bar height")
        assert_eq(rects["shell.rail"][2], metrics["rail_w"], "B: rail width")
        assert_eq(rects["shell.palette"][2], metrics["palette_w"], "B: palette width")

        # ── (C) every painted region is classified, and the split is the
        #        specification's ─────────────────────────────────────────────
        banner("C — voice: every addressable region is classified")
        voice = app.request("scene/voice").result
        counts = voice["counts"]
        assert_eq(counts["unvoiced"], 0, "C: nothing is left undecided")
        for fault in ("mumbled", "hollow", "dangling", "ghost"):
            assert_eq(counts[fault], 0, f"C: no {fault} region")
        rows = voice_by_tag(app)
        assert_eq(len(rows), voice["total"], "C: one row per addressable region")
        print(
            f"[demo] {voice['total']} region(s) — {counts['announced']} announced, "
            f"{counts['silent']} declared quiet"
        )
        # ★★★★★ R1867 — **and again with the status band's slot IDLE**, because
        # the screen has two resting states and this reading only ever saw one.
        #
        # Arriving says a sentence, so every census this demo has ever taken was
        # taken with a toast in the band. That is why `shell.toast` satisfied a
        # `Chrome` row for 173 rounds and why the sentence beside it could not:
        # exactly one of the slot's two occupants is painted at a time. Settling
        # the toast and asking again is what makes the comparison below a claim
        # about the SCREEN rather than about the moment it was asked.
        cleared = settle_saying(app)
        ok(f"C: the boot sentence settles, and it was {cleared!r}", bool(cleared))
        idle = app.request("scene/voice").result
        idle_counts = idle["counts"]
        assert_eq(idle_counts["unvoiced"], 0, "C: nothing is undecided when idle either")
        for fault in ("mumbled", "hollow", "dangling", "ghost"):
            assert_eq(idle_counts[fault], 0, f"C: no {fault} region when idle")
        idle_rows = voice_by_tag(app)
        ok(
            "C: ★ and the two states are not the same reading — the slot's "
            "occupant changed",
            set(rows) != set(idle_rows),
        )

        # BOTH ways, which is the whole point of holding a specification: a
        # region the table has and the screen does not is as much a failure as
        # a region the screen paints and the table never named.
        # ★ R1695 — filtered to the destination the screen OPENS at. Before that
        # round the application had one page and this table described it; now
        # each row says which destination owns it, and comparing the whole table
        # against one page would demand the settings page's regions on the
        # dashboard. `r1695_the_rail_takes_you_there` is what asks the same
        # question of the other destination.
        shown = ("*", here)
        declared_voices = {v["tag"] for v in voices if v["at"] in shown}
        declared_silences = {s["tag"] for s in silences if s["at"] in shown}
        # ★ R1867 — the UNION over the slot's two occupancies. A region painted
        # in one state and not the other is still one this screen has, and the
        # specification's `at` column says which DESTINATION shows a region, not
        # which moment.
        both = list(rows.items()) + list(idle_rows.items())
        announced = {t for t, r in both if r["voice"] == "announced"}
        quiet = {t for t, r in both if r["voice"] == "silent"}
        assert_eq(
            sorted(announced - declared_voices),
            [],
            "C: nothing speaks that the specification did not say would",
        )
        assert_eq(
            sorted(declared_voices - announced),
            [],
            "C: nothing the specification named is missing its voice",
        )
        assert_eq(
            sorted(quiet ^ declared_silences),
            [],
            "C: the declared silences are exactly the quiet regions",
        )
        # ★★★★★ R1867 — and a quiet region's REASON is the specification's too.
        #
        # The comparison above is over TAGS, so a region declared `layout` in
        # the painter and `part_of` in the published table read as agreeing —
        # two records of one fact with nothing holding them together, which is
        # the defect this project keeps repairing. It was found by a
        # counterfactual: swapping the status slot's kind changed what a reader
        # is promised and every gate stayed green.
        #
        # ⚠ Compared where the region declares itself. A row whose reason came
        # from an ANCESTOR is reporting that ancestor's kind, and demanding the
        # table's own word there would be demanding the wrong one.
        kinds = 0
        for entry in (s for s in silences if s["at"] in shown):
            row = idle_rows.get(entry["tag"]) or rows.get(entry["tag"])
            ok(f"C: {entry['tag']} is a region the census knows", row is not None)
            if row["self_declared"]:
                assert_eq(row["reason"], entry["kind"], f"C: {entry['tag']} kind")
                kinds += 1
        ok(f"C: ★ {kinds} declared silence(s) carry the specified kind", kinds > 0)

        # And the ROLE each one announces is the specification's too — a name is
        # not evidence of the right kind (the R1691 lesson, twice over).
        access = nodes_by_tag(app)
        for entry in (v for v in voices if v["at"] in shown):
            node = access.get(entry["tag"])
            ok(f"C: {entry['tag']} is announced", node is not None)
            assert_eq(node["role"], entry["role"], f"C: {entry['tag']} role")
        CHECKS.extend([f"role {v['tag']}" for v in voices])

        # ── (D) the tree is walkable ───────────────────────────────────────
        banner("D — structure: every collection holds what its role promises")
        conform = app.request("scene/conform").result
        assert_eq(conform["counts"]["empty"], 0, "D: no collection is left empty")
        assert_eq(conform["counts"]["stray"], 0, "D: no member is outside its collection")
        ok("D: the screen carries structural requirements at all", conform["judged"] > 0)
        print(f"[demo] {conform['judged']} node(s) carry a structural requirement")

        # ── (E) ★★★★★ the locked seats ─────────────────────────────────────
        banner("E — a locked seat is heard, and says what it is booked under")
        disabled = {
            row["tag"]: row for row in app.request("scene/disabled").result["disabled"]
        }
        assert_eq(
            sorted(disabled),
            sorted(locked),
            "E: the cascade declares exactly the seats the specification locks",
        )
        catalogue = {entry["kind"]: entry for entry in spec["catalogue"]}
        for tag in locked:
            node = access.get(tag)
            ok(f"E: {tag} is in the accessibility tree", node is not None)
            ok(f"E: {tag} has a name a reader can hear", bool(node.get("name")))
            ok(f"E: {tag} is announced unavailable", is_disabled(node))
            reason = node.get("unavailable")
            ok(f"E: {tag} carries WHY, not only THAT", reason is not None)
            # ★ R1695 — the kind and the recourse come from the CASCADE rather
            # than being written here, because there are two kinds of locked
            # now and a demo naming one would have gone on ignoring the other.
            # The recourse is still derived, never declared.
            assert_eq(reason["kind"], disabled[tag]["reason"], f"E: {tag} kind")
            assert_eq(
                reason["recourse"],
                disabled[tag]["recourse"],
                f"E: {tag} recourse — what a person can do about it",
            )
            ok(
                f"E: {tag} names a recourse that is not nothing",
                reason["recourse"] in ("await_release", "open_elsewhere"),
            )
            # The detail is the SAME string the panel paints and the wire
            # publishes: one declaration, not three spellings of an intention.
            assert_eq(
                reason["detail"],
                disabled[tag]["detail"],
                f"E: {tag} says the same thing to both readers",
            )
        # ★ A locked seat is still a seat. The palette announces thirteen
        # entries and counts every locked one in its set — dropping them would
        # make the tree say four while the panel shows thirteen and its own
        # footer says nine are reserved.
        palette = access["shell.palette"]
        assert_eq(palette["size_of_set"], 13, "E: the palette declares thirteen entries")
        seats = [
            access[f"shell.palette.{kind}"] for kind in catalogue
        ]
        assert_eq(
            sorted(seat["position_in_set"] for seat in seats),
            list(range(1, 14)),
            "E: ★ every entry keeps its place in the set, locked or not",
        )
        # ★ R1797 — eight since the latency card was promoted, and derived from
        # the specification the screen publishes rather than written again. The
        # count above it (`position_in_set` over the whole thirteen) is the one
        # that does not move: a locked seat is still a seat, which is this
        # demo's subject.
        assert_eq(
            sum(1 for seat in seats if is_disabled(seat)),
            spec["reserved_count"],
            "E: the locked seats are the ones a later release brings",
        )
        # ★ …and the four that are NOT locked carry no reason. A reason
        # everywhere would pass every check above and mean nothing.
        for kind, entry in catalogue.items():
            node = access[f"shell.palette.{kind}"]
            if entry["tier"] == "placeable":
                ok(
                    f"E: {kind} is offered, and states no reason",
                    node.get("unavailable") is None and not is_disabled(node),
                )

        # ── (F) the grids ──────────────────────────────────────────────────
        banner("F — every cell is addressable and says where it is")
        for card, columns, rows_spec, suffix in (
            ("packet#0", spec["stream_columns"], spec["stream_rows"], "row"),
            ("keymap#2", None, spec["map_rows"], "map"),
        ):
            grid = access[f"card.{card}.grid"]
            assert_eq(grid["role"], "grid", f"F: {card} is a grid")
            width = len(columns) if columns else 3
            assert_eq(grid["column_count"], width, f"F: {card} column count")
            assert_eq(
                grid["row_count"],
                len(rows_spec) + 1,
                f"F: {card} row count includes the header row",
            )
            header = access[f"card.{card}.head"]
            assert_eq(header["role"], "row", f"F: {card} header is a row")
            assert_eq(
                len(header["children"]),
                width,
                f"F: {card} header holds one column header per column",
            )
            # WAI-ARIA counts from one and the header row is row 1, which is the
            # same reading `aria-rowcount` above takes — so the first data row
            # is row 2 and the last is the row count. A tree that counted the
            # header in one attribute and not the other would announce a last
            # row nobody can reach.
            assert_eq(header["row_index"], 1, f"F: {card} header is row 1")
            for c in range(width):
                head = access[f"card.{card}.head.{c}"]
                assert_eq(head["role"], "columnheader", f"F: {card} head {c}")
                assert_eq(head["column_index"], c + 1, f"F: {card} head {c} column")
            for r in range(len(rows_spec)):
                row = access[f"card.{card}.{suffix}.{r}"]
                assert_eq(row["role"], "row", f"F: {card} row {r}")
                assert_eq(row["row_index"], r + 2, f"F: {card} row {r} index")
                for c in range(width):
                    cell = access[f"card.{card}.cell.{r}_{c}"]
                    assert_eq(cell["role"], "gridcell", f"F: {card} cell {r},{c}")
                    assert_eq(cell["row_index"], r + 2, f"F: {card} cell {r},{c} row")
                    assert_eq(cell["column_index"], c + 1, f"F: {card} cell {r},{c} column")
                    ok(f"F: {card} cell {r},{c} has a word", bool(cell.get("name")))
            assert_eq(
                access[f"card.{card}.{suffix}.{len(rows_spec) - 1}"]["row_index"],
                grid["row_count"],
                f"F: ★ {card}'s last row is the row count, so none is unreachable",
            )
        CHECKS.extend(["grid cells addressable"] * 4)
        # ★ The cell a reader would otherwise hear as punctuation. The map's
        # unresolved row paints an em dash, which is the typographic stand-in
        # for a value that is not knowable; announced as itself it is a
        # character with no word in it.
        unresolved = spec["map_unresolved"]
        assert_eq(
            access[f"card.keymap#2.cell.{unresolved}_2"]["name"],
            "not known",
            "F: ★ the unknowable timestamp is announced as its meaning",
        )

        # ── (G) the decode tree ────────────────────────────────────────────
        banner("G — one item per decoded field, carrying its depth and value")
        tree = access["card.decode#1.tree"]
        assert_eq(tree["role"], "tree", "G: the decode body is a tree")
        for n, field in enumerate(spec["decode_rows"]):
            item = access[f"card.decode#1.tree.{n}"]
            assert_eq(item["role"], "treeitem", f"G: field {n} is a tree item")
            assert_eq(item["name"], field["key"], f"G: field {n} name")
            assert_eq(item["level"], field["depth"] + 1, f"G: field {n} level")
            if field["value"]:
                # ★ The pair the floor cannot express: there a row of two
                # columns comes back as two SIBLING items, so the field and its
                # value are peers and neither says it belongs to the other.
                assert_eq(
                    item["value"]["text"],
                    field["value"],
                    f"G: field {n} carries its value rather than a sibling",
                )
        assert_eq(
            access[f"card.decode#1.tree.{spec['decode_selected']}"]["selected"],
            True,
            "G: the opening field is the selected one",
        )
        # The bytes the selected field was read from are the ones announced as
        # selected — the law screen B was built around, held here too.
        first, last = spec["decode_span"]
        lit = [
            n
            for n in range(len(spec["decode_rows"]) * 0 + 24)
            if access.get(f"card.decode#1.byte.{n}", {}).get("selected")
        ]
        assert_eq(lit, list(range(first, last)), "G: exactly the field's bytes are lit")

        # ── (H) a real press moves what is announced ───────────────────────
        banner("H — driven: a press through the router moves the announcement")
        before = access["shell.rail.dashboard"].get("current")
        assert_eq(before, "page", "H: the dashboard seat is the current one")
        # ★ R1695 — `settings` rather than `stream`, because `stream` is now a
        # destination this application cannot take you to. The swap is forced,
        # and what forced it is the round's finding: this assertion used to be
        # satisfied by a rail that moved the highlight and arrived nowhere.
        app.request(
            "scene/click", {"button": "left", "at": center(rects["shell.rail.settings"])}
        )
        app.tick(16)
        after = nodes_by_tag(app)
        assert_eq(
            after["shell.rail.settings"].get("current"),
            "page",
            "H: the seat pressed is now the current one",
        )
        assert_eq(
            after["shell.rail.dashboard"].get("current"),
            None,
            "H: and the one it left is not",
        )
        # ★ A locked destination cannot be reached even by the wire. The reason
        # says so; the router agrees.
        #
        # ★★★★★ R1953 — WHICH seat is locked is derived. This pressed
        # `shell.rail.topology` by name, and R1947 opened that seat: the press
        # arrived, the reader moved, and the assertion read a screen behaving
        # correctly as a defect.
        #
        # ⚠ And the empty case is asserted rather than skipped. `closed_keys`
        # is empty today — this build opens every seat the reference draws —
        # so there is no locked destination to press, and saying nothing there
        # would let this leg report green for having no population (R1651.1).
        # What is asserted instead is the fact that makes it empty: the live
        # rail declares no seat unavailable at all.
        locked_seats = [k for k in closed_rail_keys() if f"shell.rail.{k}" in rects]
        if locked_seats:
            app.request(
                "scene/click",
                {"button": "left", "at": center(rects[f"shell.rail.{locked_seats[0]}"])},
            )
            app.tick(16)
            assert_eq(
                nodes_by_tag(app)["shell.rail.settings"].get("current"),
                "page",
                f"H: ★ pressing the locked {locked_seats[0]} does not move the reader",
            )
        else:
            live_shut = [
                row["tag"]
                for row in app.request("scene/disabled", {}).result["disabled"]
                if row["tag"].startswith("shell.rail.")
            ]
            assert_eq(
                live_shut,
                [],
                "H: ★ the specification declares no shut seat, so the rail must "
                "declare none either -- there is nothing to press and that is a "
                "fact rather than an absence of one",
            )
        # Back where the rest of this demo expects to be.
        app.intervene(f"{EXT}/nav", "dashboard")
        app.tick(16)

        # ── (I) the censuses read the live tree ────────────────────────────
        banner("I — discriminating: the numbers move when the screen does")
        # Back to the board first: section H moved the reader off it, and a
        # rectangle read before that press is a rectangle of a screen that is no
        # longer there.
        app.request(
            "scene/click", {"button": "left", "at": center(rects["shell.rail.dashboard"])}
        )
        app.tick(16)
        rects = abs_rects_of(app.snapshot(source="paint"))
        ok("I: the board is back", "card.packet#0.close" in rects)
        opened = app.request("scene/voice").result["counts"]["announced"]
        app.request("scene/click", {"button": "left", "at": center(rects["card.packet#0.close"])})
        app.tick(16)
        closed = app.request("scene/voice").result
        ok(
            "I: removing a card removes its regions from the census",
            closed["counts"]["announced"] < opened,
        )
        assert_eq(
            closed["counts"]["unvoiced"], 0, "I: and nothing is left undecided by it"
        )
        assert_eq(
            app.request("scene/conform").result["counts"],
            {"empty": 0, "stray": 0},
            "I: the tree is still well formed with a card gone",
        )
        print(f"[demo] {len(CHECKS)} named check(s)")


def center(rect: tuple) -> dict:
    x, y, w, h = rect
    return {"x": x + w // 2, "y": y + h // 2}


def is_disabled(node: dict) -> bool:
    """`scene/access` omits the state object when every flag is at its default,
    so "no state" and "not disabled" are the same answer."""
    return bool(node.get("state", {}).get("disabled"))


run_demo("R1694 a locked seat is heard", body)
