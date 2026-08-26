#!/usr/bin/env python3
"""R1851 §5.16 §5.27 §5.40 §2 #2 — **an alarm feed, graded, ordered and windowed.**

# What this demo exists for

The analysis-tool census (`tools/analyzer_census.py`) carries `dashboard.t1.9` —
*an alarm feed* — as an **app** verdict whose covering sentence has always been a
claim about composition:

    a virtualised list with a severity column

That verdict named **no assembly**, which is what R1807's `UNASSEMBLED` ratchet
records: a claim about a composition nobody had composed. This is the
composition, driven on the wire, on the analyzer shell itself — closing a census
row on a demo that never touches the reference screen closes a line without the
screen gaining anything (the R1722 lesson).

# ★★★★★ Measured: the two halves existed and NOTHING composed them

Counted over the whole tree at R1851, by file:

    grep -rl 'view_virtual_list\\|view_variable_virtual_list\\|view_flex_virtual_list\\|view_measured_list' crates/ examples/ --include=*.rs
    grep -rl 'view_header_cell\\|header_label_node' crates/ examples/ --include=*.rs

The first answered 27. The second answered THREE, of which one is a screenshot
harness and one a shell test — so exactly ONE screen in this workspace had ever
drawn a column header. And the only surface holding both is the data grid, where
every row is a row of CELLS. A feed is not that. Its row is a shape: a severity
swatch beside a graded word, a clock reading and a message, which is exactly what
a grid cannot draw. So a screen wanting a sortable header over caller-drawn rows
had to wire the two together by hand, and `pinion_widget_paint::header_feed` is
that wiring, once.

# ★★★★★ Where this is SUPERIOR to the floor, measured rather than asserted

Probed against the reference toolkit at 6.11.1, compiled and run:

  (1) its row filtering is a PREDICATE OVER A STRING. Filtering six rows whose
      severity is spelled `err` by the word `error` answers **0 of 6** and says
      nothing — a correct answer to a question nobody meant to ask, and
      indistinguishable from *no row is that severe*. `at least this severe` has
      to be written there as a pattern enumerating the words by hand
      (`^(err|warn)$`), and one word misspelled inside it silently drops rows:
      **2 of 6** where four should have survived.
  (2) a virtualised tabular view over ten thousand rows reports **ten thousand**
      through its public surface and publishes NO count of the rows it actually
      built. The absence is proved the way this project proves one — a probe that
      asks the view for the set of rows it constructed FAILS TO COMPILE, because
      that view class has no such member. (The class name is left out on purpose:
      a reference toolkit's names must not reach a tracked file, and the
      capability sentence is what carries the evidence anyway.)

And the behaviour prototype this build reproduces ships defect (1) in the
article: its alarm control offers `info / warn / error` over a feed whose rows are
spelled `info / warn / err`, so its most severe setting could never have matched
a row. ⚠ And nothing there could ever have noticed, because the control is never
READ: `grep -c minSev` over its extracted app logic answers 1 — the declaration
that offers it, and no other site.

Here a severity is a word of ONE ordered vocabulary, a threshold is a position in
that order, and a word the vocabulary does not hold is a REFUSAL that names it and
carries the vocabulary — section E drives all three. And the window the feed built
is on the wire, which section C reads.

# What is shown

  (A) the seat is no longer locked — the catalogue offers `alarms` as placeable,
      the deferred register declares requirement 21 BUILT, and the board places
      it as the seventh card.
  (B) the feed has a severity column: three headings, and the sort indicator a
      reader is told about is the order the rows are actually in.
  (C) the virtualisation — the feed holds eighteen alarms and CONSTRUCTED four,
      the rows outside the window are not in the paint at all, and scrolling
      changes which alarms are there without changing how many rows exist.
  (D) sorting works, from the POINTER and from the wire, and the two arrive at
      the same state.
  (E) the severity threshold narrows the feed, and a word outside the vocabulary
      is refused BY NAME with the vocabulary in the refusal.
  (F) every row the feed constructed is announced, and no row it did not.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_action_refused,
    assert_eq,
    run_demo,
)

SHELL = "hello-analyzer-shell"
EXT = "/external"
CARD = "alarms"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


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


def painted_rows(app: RpcSubprocess, card: str) -> list[str]:
    """The row slots the feed actually PAINTED, read from the paint.

    ⚠ From `source="paint"` and not from the state, because the claim in section
    C is about what was CONSTRUCTED — and a virtualised list decides that while
    painting. Reading the state would answer about the table, which never
    narrows.
    """
    # ⚠ The bare ROWS, not their cells. A row owns three tagged cells (WAI-ARIA:
    # a `row` owns members of a cell role), so a plain prefix test answers
    # sixteen where four rows were built — the predicate has to say which of the
    # two families it means.
    stem = f"card.{card}.feed.row."
    return sorted(
        {
            t
            for t in walk_tags(app.snapshot(source="paint"))
            if t.startswith(stem) and "." not in t[len(stem) :]
        }
    )


def announced(
    app: RpcSubprocess, prefix: str, *, leaves: bool = True
) -> dict[str, dict[str, Any]]:
    """The accessibility nodes under `prefix`, by tag.

    `leaves=False` keeps only the nodes whose tag ends AT this family — the bare
    rows rather than the cells they own.
    """
    out: dict[str, dict[str, Any]] = {}
    for node in app.request("scene/access").result["nodes"]:
        tag = node.get("tag") or ""
        if not tag.startswith(prefix):
            continue
        if not leaves and "." in tag[len(prefix) :]:
            continue
        out[tag] = node
    return out


def reading(node: dict[str, Any]) -> str:
    """What a reader is told this row says, normalised.

    ⚠ `value` on the wire is a typed thing — the access tree carries a value's
    KIND beside its text — so a bare `node["value"]` is a guess about the shape
    rather than a read of it.
    """
    value = node.get("value")
    if isinstance(value, dict):
        for key in ("text", "Text", "value"):
            if isinstance(value.get(key), str):
                return value[key]
        return str(value)
    return value if isinstance(value, str) else ""


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as d:
        # ── (A) the seat is not locked any more, and says so three times ─────
        banner("A — the alarm seat is placeable, placed, and declared built")
        catalogue = d.query(f"{EXT}/catalogue").split(",")
        ok("the catalogue offers an alarm seat", CARD in catalogue)

        spec: Any = d.query(f"{EXT}/spec")
        reserved = {w["kind"] for w in spec["catalogue"] if w.get("reserved_for")}
        ok("and it is NOT among the seats a later release brings", CARD not in reserved)

        placed = [tile["kind"] for tile in spec["board"]]
        ok("the opening board places the alarm card", CARD in placed)
        assert_eq(len(placed), len(set(placed)), "no kind is placed twice")
        # ⚠ A card's id is `kind#n` where n is its INDEX on the board — derived,
        # not stored, which is why the board publishes no `id` field. R1843
        # learned this the expensive way; a new placement goes LAST and its id is
        # computed the way the shell computes it.
        card = f"{CARD}#{placed.index(CARD)}"
        print(f"    the alarm card is {card}, {len(placed)} card(s) on the board")

        # ★ Promoting a seat changes the RELEASE structure the reference defines,
        # so it is DECLARED rather than made to look like a seat that was never
        # locked. R1797 set that pattern; R1843 followed it.
        register = Path("docs/analyzer-reserved-spec.json")
        ok("the deferred register is where the promotion is recorded", register.exists())
        import json

        built = {row["requirement"] for row in json.loads(register.read_text())["built"]}
        ok("and requirement 21 is declared built there", 21 in built)

        # ── (B) a severity column, whose indicator IS the order ─────────────
        banner("B — the feed has a severity column and the arrow cannot lie")
        feed = d.query(f"{EXT}/alarms")
        print(f"    vocabulary {feed['vocabulary']}, sort {feed['sort']!r}")
        ok("the severity vocabulary is published, in order", feed["vocabulary"] == ["info", "warn", "error"])
        heads = announced(d, f"card.{card}.feed.head.col#")
        print(f"    headings: {[(t, n.get('name'), n.get('sort')) for t, n in sorted(heads.items())]}")
        assert_eq(len(heads), 3, "three headings")
        ok(
            "the first heading is the severity column",
            heads[f"card.{card}.feed.head.col#0"].get("name") == "Severity",
        )
        # ★★★★★ THE CLAIM. The feed opens sorted by time descending, and the
        # indicator a reader is told about must be that same order — on the
        # toolkit floor these are separate properties of separate objects.
        sorted_heads = {t: n.get("sort") for t, n in heads.items() if n.get("sort")}
        print(f"    the indicator sits on {sorted_heads}")
        assert_eq(len(sorted_heads), 1, "exactly one heading carries an indicator")
        assert_eq(feed["sort"], "1:descending", "and the rows are in that order")
        ok(
            "the indicator says the direction the rows are in",
            list(sorted_heads.values())[0].lower().startswith("desc"),
        )
        rows = feed["rows"]
        seconds = [row["seconds"] for row in rows]
        ok("and the rows really are newest first", seconds == sorted(seconds, reverse=True))
        # The prototype's own quirk, kept: its first two rows are OUT of time
        # order, so an unsorted feed puts the newest alarm second. That is what
        # makes "the sort did something" observable at all.
        ok("the table itself is not in time order", feed["total"] > feed["in_reference"])

        # ── (C) the virtualisation, read off the paint ───────────────────────
        banner("C — eighteen alarms, four rows constructed")
        built_now = feed["built"]
        drawn = painted_rows(d, card)
        print(f"    total {feed['total']}, shown {feed['shown']}, built {built_now}")
        print(f"    painted row slots: {drawn}")
        assert_eq(len(drawn), len(built_now), "every constructed row is in the paint")
        ok("and there are fewer of them than there are alarms", len(drawn) < feed["total"])
        ok("but not none", len(drawn) > 0)
        # ★★★★★ The claim the reference cannot make about itself: the rows outside
        # the window were never constructed. Read as an ABSENCE from the paint,
        # which is the only place it can be read.
        ok(
            "the alarms outside the window are not in the paint at all",
            all(int(t.rsplit(".", 1)[1]) < len(built_now) for t in drawn),
        )
        first_readings = [reading(n) for _, n in sorted(announced(d, f"card.{card}.feed.row.", leaves=False).items())]
        print(f"    at the top: {first_readings[0]!r}")

        # Scrolling moves the WINDOW, not the row count: the slots stay, the
        # alarms in them change. That is what a virtualised feed is.
        d.scroll("card.alarms.feed.scroll", to=(0, 96))
        d.request("scene/snapshot", {"path": "", "source": "paint"})
        after = d.query(f"{EXT}/alarms")
        scrolled = painted_rows(d, card)
        print(f"    after scrolling: built {after['built']}, slots {scrolled}")
        assert_eq(len(scrolled), len(drawn), "the same number of row slots")
        ok("but a different window of alarms", after["built"] != built_now)
        later = [reading(n) for _, n in sorted(announced(d, f"card.{card}.feed.row.", leaves=False).items())]
        ok("and different alarms in them", later != first_readings)
        d.scroll("card.alarms.feed.scroll", to=(0, 0))

        # ── (D) sorting, from the pointer and from the wire ──────────────────
        banner("D — the severity column sorts, by pointer and by verb")
        d.click(path=f"card.{card}.feed.head.col#0")
        by_press = d.query(f"{EXT}/alarms")
        print(f"    after pressing the Severity heading: sort {by_press['sort']!r}")
        assert_eq(by_press["sort"], "0:ascending", "a press starts that column's cycle")
        ranks = [feed["vocabulary"].index(row["severity"]) for row in by_press["rows"]]
        ok("and the rows are ordered by severity, least severe first", ranks == sorted(ranks))
        # ★ The indicator moved WITH the order — it is derived from it.
        moved = {t: n.get("sort") for t, n in announced(d, f"card.{card}.feed.head.col#").items() if n.get("sort")}
        assert_eq(list(moved), [f"card.{card}.feed.head.col#0"], "the arrow moved to that column")
        ok("pointing the way the rows now run", list(moved.values())[0].lower().startswith("asc"))

        # The wire reaches the same state. Not a second implementation — the
        # press goes THROUGH this verb, which is why they cannot drift.
        assert_eq(d.invoke(f"{EXT}/sort_alarms", "severity:descending"), "0:descending", "the verb answers the state it set")
        by_wire = d.query(f"{EXT}/alarms")
        ranks = [by_wire["vocabulary"].index(row["severity"]) for row in by_wire["rows"]]
        ok("and the rows are most severe first", ranks == sorted(ranks, reverse=True))
        assert_eq(d.invoke(f"{EXT}/sort_alarms", "time:descending"), "1:descending", "and back to newest first")

        # ── (E) the threshold, and the refusal that is the point ────────────
        banner("E — a severity threshold is an order, and an unknown word is refused")
        assert_eq(d.invoke(f"{EXT}/filter_alarms", "warn"), "warn", "the threshold is set")
        warned = d.query(f"{EXT}/alarms")
        print(f"    floor warn: {warned['shown']} of {warned['total']}")
        ok("the feed narrowed", warned["shown"] < warned["total"])
        ok("and kept nothing below the floor", all(r["severity"] != "info" for r in warned["rows"]))
        # ★★★★★ *warnings* means warnings AND errors. Three independent flags
        # could not say this, which is why the vocabulary is ordered.
        ok("keeping warnings AND errors", {r["severity"] for r in warned["rows"]} == {"warn", "error"})
        assert_eq(d.invoke(f"{EXT}/filter_alarms", "error"), "error", "the strictest floor")
        strict = d.query(f"{EXT}/alarms")
        print(f"    floor error: {strict['shown']} of {strict['total']}")
        ok("errors are a SUBSET of warnings — that is what an order means", strict["shown"] < warned["shown"])

        # ★★★★★ THE MEASURED SUPERIORITY. On the toolkit floor at 6.11.1 the same
        # request answers `0 of 6` and says nothing; the prototype ships exactly
        # that mismatch. Here the word is refused, by name, with the vocabulary.
        said = assert_action_refused(
            lambda: d.invoke(f"{EXT}/filter_alarms", "err"),
            saying="not a severity",
        )
        print(f"    refused: {said}")
        ok("the refusal names the word the caller wrote", '"err"' in said)
        ok("and carries the whole vocabulary, in order", "info < warn < error" in said)
        unchanged = d.query(f"{EXT}/alarms")
        assert_eq(unchanged["floor"], "error", "a refused threshold changed nothing")
        # And the declared domain is exactly what is accepted, which is R1642's
        # rule: a declaration admitting a call the surface refuses is worse than
        # silence.
        declared = {f["path"]: f for f in d.query(f"{EXT}/$schema")}
        args = declared["filter_alarms"]["args"]
        print(f"    filter_alarms declares {args}")
        domain = args[0].get("domain", {})
        ok(
            "the declared domain is a CLOSED set, which the reference's string "
            "predicate has no way to state",
            "one_of" in str(domain).lower() or isinstance(domain, dict) and domain,
        )
        assert_eq(d.invoke(f"{EXT}/filter_alarms", "all"), "all", "and *all* puts every alarm back")
        assert_eq(d.query(f"{EXT}/alarms")["shown"], strict["total"], "every one of them")

        # ── (F) announced is exactly constructed ────────────────────────────
        banner("F — a reader is told about the rows that exist, and no others")
        final = d.query(f"{EXT}/alarms")
        drawn = painted_rows(d, card)
        heard = announced(d, f"card.{card}.feed.row.", leaves=False)
        print(f"    {len(drawn)} painted, {len(heard)} announced")
        assert_eq(sorted(heard), drawn, "the announced rows ARE the painted rows")
        for tag in drawn:
            said = reading(heard[tag])
            ok(f"{tag} is announced with its whole reading", said.count(",") >= 2)
        # And the group says how much of the feed a reader is looking at, which
        # is the fact a window withholds.
        group = announced(d, f"card.{card}.feed")[f"card.{card}.feed"]
        print(f"    the feed announces itself as {group.get('name')!r}")
        ok(
            "the feed says how many of how many it shows",
            f"{final['shown']} of {final['total']}" in (group.get("name") or ""),
        )

    print(f"\n{len(CHECKS)} named check(s) passed")


run_demo("r1851 an alarm feed is graded, ordered and windowed", body)
