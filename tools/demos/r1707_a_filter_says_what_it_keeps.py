#!/usr/bin/env python3
"""R1707 §5.40 §5.41 §2 #2 §2 #7 — **a filter says what it keeps, and why it
dropped the rest**, driven through the real window of the analysis tool's
capture viewer.

# What this exists for

Screen B of the reference tool is a capture viewer: a filter bar over a
session-context strip over a three-pane body. Measured on the built binary
before this round, through the wire: the bar painted a three-clause query, three
saved-filter chips and a `12,418 / 184,392` matched count — and the list held
sixteen messages whatever any of them said. There was no `filter` verb, no text
field anywhere on the screen, and pressing a saved chip flipped a boolean,
announced "applied units only", and moved nothing. Every check in the example
was green throughout, because nothing had ever asked what the filter DID.

The framework half is why it cannot come back. `pinion_core::widgets::row_query`
holds the query a person WRITES — parsed against a roster of column names,
keeping each clause's own source text, compiling to the `GridFilter` that does
the work — and `GridFilter::admit` answers which facet dropped a row rather than
a bare boolean.

Measured on the reference toolkit at 6.11.1, built as a probe and run offscreen
rather than read out of headers, its row-filtering proxy:

* filters on **one** column at a time (`filterKeyColumn` is a single `int`;
  setting a second predicate replaces the first — measured 3 rows and then 2,
  where a conjunction would have given 1), so a three-clause query is not
  expressible without subclassing;
* addresses that column by **ordinal**, so a saved filter changes meaning the
  day a column moves, and a fact not laid out as a column cannot be filtered on
  at all;
* offers **three** operator setters (fixed string, wildcard, regexp) and no set
  membership or inequality among them;
* publishes **12 properties and 101 methods**, of which six and eight
  respectively name a filter and **none names a reason** — a dropped row maps to
  an invalid index, which is the same answer for every way a row can be absent;
* and **loses the query the person typed**: `setFilterWildcard("sensors/unit/*")`
  reads back as `(?s:sensors/unit/[^/]*)`.

# What it asserts

* **A** — the bar is a real text field: a Tab stop, a press puts the caret in
  it, and typing through the key channel narrows the list AS IT IS TYPED.
* **B** — the reference's own query, sent by an agent, keeps exactly the
  messages it should, and the paint draws exactly those.
* **C** — what is drawn is what is pressed: no point in the list resolves to a
  message the query hid.
* **D** — ★ PIXELS. The list is visibly shorter, scanned out of a real
  screenshot rather than eyeballed.
* **E** — the count is DERIVED and says the number the list actually shows.
* **F** — the semantic tree narrows with the paint, and the query box announces
  what it holds.
* **G** — ★ the reason: every hidden message names the CLAUSE that dropped it,
  and that clause is one of the running query's own.
* **H** — a malformed query is refused on the wire with the fault in words,
  while a half-typed one keeps the capture on screen instead of flashing it away.
* **I** — a saved chip runs the query it names, and pressing it again clears.
* **J** — the person's own words survive the round trip.

Run from the workspace root:
    cargo build -p hello-packet-view --release
    python3 tools/demos/r1707_a_filter_says_what_it_keeps.py
"""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    Png,
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    png_pixel,
    read_png_rgba8,
    run_demo,
    text_of_tag,
)

EXT = "/external"

#: How many messages the capture holds. Written here rather than read off the
#: wire: a test that asks the screen how many rows it has and then checks the
#: screen against that answer checks nothing.
HELD = 16

#: Which messages the reference's own opening query keeps — worked out from the
#: capture the specification states, not read back.
#:
#: ★ R2041 — 4, where this said 6, and the row is the same message. That round
#: put the capture's fragment run the right way up: `First` had been arriving
#: after `Last` in a newest-first table, so righting it moved the reassembled
#: message (`sensors/unit-1/depth`, the one this query matches) two rows up.
#: Still worked out from the specification rather than read back, which is what
#: makes this list an expectation instead of an echo.
EXPECTED_KEPT = [0, 4, 10, 11]


def centre(rect: tuple[int, int, int, int]) -> tuple[int, int]:
    x, y, w, h = rect
    return (x + w // 2, y + h // 2)


def drawn_rows(tf: RpcSubprocess) -> list[int]:
    """Which message rows the screen is painting, by source index."""
    rects = abs_rects_of(tf.snapshot(source="paint"))
    return [n for n in range(HELD) if f"pv.list.row.{n}" in rects]


def kept(tf: RpcSubprocess) -> list[int]:
    return list(tf.query(f"{EXT}/kept_rows"))


def access(tf: RpcSubprocess) -> tuple[dict[str, dict], dict]:
    answer = tf.request("scene/access", {}).result
    nodes = {n["tag"]: n for n in answer.get("nodes", []) if "tag" in n}
    return nodes, (answer.get("focus") or {})


def focus_tag(tf: RpcSubprocess) -> str | None:
    return access(tf)[1].get("tag")


def walk_tab_ring(tf: RpcSubprocess, limit: int = 12) -> list[str]:
    """Walk the Tab ring once, the way a keyboard does."""
    seen: list[str] = []
    for _ in range(limit):
        tf.request("focus/next")
        tf.tick(16)
        tag = focus_tag(tf)
        if tag is None or tag in seen:
            break
        seen.append(tag)
    return seen


def main() -> None:
    with RpcSubprocess("hello-packet-view") as tf:
        body(tf)


def body(tf: RpcSubprocess) -> None:
    checks = 0

    # ── A. the query bar is a field a person can type in ───────────────────
    print("\n== A. the bar is a real text field ==")
    assert_eq(tf.query(f"{EXT}/query"), "", "the screen opens unfiltered")
    assert_eq(len(kept(tf)), HELD, "and holds the whole capture")
    rects = abs_rects_of(tf.snapshot(source="paint"))
    box = rects["pv.filter.query"]
    print(f"  the query box is painted at {box}")

    stops = walk_tab_ring(tf)
    assert "pv.filter.query" in stops, f"the box has to be a Tab stop; ring={stops}"
    checks += 1

    tf.click(centre(box))
    tf.tick(16)
    assert_eq(focus_tag(tf), "pv.filter.query", "a press focuses the box")
    checks += 1

    # Typed one character at a time, through the key channel a keyboard uses —
    # not through a setter of our own, which is the door R1698 measured every
    # gate on this screen was skipping.
    typed = ""
    narrowed_at = None
    for ch in "type = Query":
        # A single codepoint routes through the character path, which is the
        # door a real keystroke comes in by — a space included.
        tf.key(path="pv.filter.query", name=ch)
        typed += ch
        if narrowed_at is None and len(kept(tf)) < HELD:
            narrowed_at = typed
    assert_eq(tf.query(f"{EXT}/query"), "type = Query", "the box holds what was typed")
    checks += 1
    print(f"  the list first narrowed at {narrowed_at!r} — live, as the reference is")
    assert narrowed_at is not None, "typing a whole query never narrowed the list"
    checks += 1
    live = kept(tf)
    assert 0 < len(live) < HELD, f"`type = Query` kept {live}"
    checks += 1

    # ── B. the reference's own query, sent by an agent ─────────────────────
    print("\n== B. the canon query, through the wire ==")
    answer = tf.invoke(f"{EXT}/filter", tf.query(f"{EXT}/spec")["example_query"])
    print(f"  filter answered {answer}")
    assert_eq(kept(tf), EXPECTED_KEPT, "the canon query keeps the messages it should")
    checks += 1
    assert_eq(answer["clauses"], 3, "and it is three clauses")
    checks += 1
    assert_eq(drawn_rows(tf), EXPECTED_KEPT, "the paint draws exactly what was kept")
    checks += 1

    # ── C. what is drawn is what is pressed ────────────────────────────────
    print("\n== C. no press reaches a hidden message ==")
    rects = abs_rects_of(tf.snapshot(source="paint"))
    list_rect = rects["pv.list"]
    hidden = [n for n in range(HELD) if n not in EXPECTED_KEPT]
    reached = set()
    x = list_rect[0] + list_rect[2] // 2
    for y in range(list_rect[1], list_rect[1] + list_rect[3], 3):
        tf.click((x, y))
        reached.add(tf.query(f"{EXT}/selected_row"))
    stray = sorted(n for n in reached if n in hidden)
    assert not stray, f"presses inside the list reached hidden message(s) {stray}"
    checks += 1
    print(f"  swept {list_rect[3] // 3} points; reached only {sorted(reached)}")
    # Put the canon query back — the sweep moved the selection.
    tf.invoke(f"{EXT}/filter", tf.query(f"{EXT}/spec")["example_query"])
    # ★★★★★ R1831 — and put the ORDER back, which the sweep now also moves.
    #
    # This line is a regression R1829 caused and CI caught: that round made the
    # column headers pressable, and the sweep above walks the list rect from its
    # TOP in three-pixel steps — straight through the header band, which until
    # then answered nothing. Each of those presses cycles the sort, so the sweep
    # left one active and `kept_rows` came back permuted:
    # `expected [0, 6, 10, 11], got [10, 0, 6, 11]` — the same set in a
    # different order, which is exactly what an ordering feature is supposed to
    # be able to do.
    #
    # ⇒ ★★★★★ MAKING AN INERT REGION INTERACTIVE REACHES EVERY SWEEP THAT RELIED
    # ON IT BEING INERT. Nothing in R1829's own round could see this: it ran its
    # own demo and this screen's suites, and the caller that broke is a
    # different demo whose subject is the filter. The full sweep is what found
    # it, one push later.
    #
    # Restored here rather than by narrowing the sweep, because the sweep's
    # claim — *no press inside the list reaches a hidden message* — is about the
    # whole pane, header included, and starting below the header would stop
    # testing a band that now answers presses. The demo already restores the
    # query for this reason; the order is the second thing the sweep perturbs.
    tf.invoke(f"{EXT}/order", "none")

    # ── D. pixels ──────────────────────────────────────────────────────────
    print("\n== D. the list is visibly shorter ==")
    out_dir = Path(tempfile.mkdtemp(prefix="r1707-"))
    px_filtered = ink_rows(shoot(tf, out_dir / "filtered.png"), rects["pv.list"])
    tf.invoke(f"{EXT}/filter", "")
    px_all = ink_rows(shoot(tf, out_dir / "unfiltered.png"), rects["pv.list"])
    print(f"  rows of ink in the list pane: filtered {px_filtered}, unfiltered {px_all}")
    assert px_filtered < px_all, (
        "★ the screenshot shows the same amount of list either way — every "
        "other check here reads the wire, and a screen can be right on the "
        "wire and blank on the glass"
    )
    checks += 1

    # ── E. the count is derived ────────────────────────────────────────────
    print("\n== E. the count says what the list shows ==")
    tf.invoke(f"{EXT}/filter", tf.query(f"{EXT}/spec")["example_query"])
    count = text_of_tag(tf, "pv.filter.count")
    assert_eq(count, f"{len(EXPECTED_KEPT)} of {HELD} shown", "the count is derived")
    checks += 1
    tf.invoke(f"{EXT}/filter", "")
    assert_eq(
        text_of_tag(tf, "pv.filter.count"),
        "12,418 / 184,392",
        "unfiltered it is the capture's own scale",
    )
    checks += 1

    # ── F. the semantic tree narrows with the paint ────────────────────────
    print("\n== F. a reader hears the list the query kept ==")
    tf.invoke(f"{EXT}/filter", tf.query(f"{EXT}/spec")["example_query"])
    nodes, _ = access(tf)
    rows = [t for t in nodes if t.startswith("pv.list.row.") and t.count(".") == 3]
    assert_eq(
        sorted(rows),
        sorted(f"pv.list.row.{n}" for n in EXPECTED_KEPT),
        "the accessibility tree holds the kept rows and no others",
    )
    checks += 1
    box_node = nodes["pv.filter.query"]
    assert_eq(box_node["role"], "textbox", "the query box announces itself a text box")
    assert box_node.get("value"), f"and says what it holds: {box_node}"
    checks += 1

    # ── G. the reason ──────────────────────────────────────────────────────
    print("\n== G. every hidden message names the clause that dropped it ==")
    why = tf.query(f"{EXT}/why_hidden")
    clauses = [c["text"] for c in tf.query(f"{EXT}/query_clauses")]
    assert_eq(sorted(int(k) for k in why), hidden, "every hidden message has a reason")
    checks += 1
    for row, reason in sorted(why.items(), key=lambda kv: int(kv[0])):
        assert reason in clauses, f"row {row} blames {reason!r}, not a clause of the query"
    checks += 1
    print(f"  {len(why)} hidden message(s), each attributed to one of {len(clauses)} clauses")
    for row in ("1", "7", "8"):
        print(f"    message {row} is not shown because: {why[row]}")

    # ── H. a refusal says why ──────────────────────────────────────────────
    print("\n== H. a wrong query is refused in words ==")
    try:
        tf.invoke(f"{EXT}/filter", "nod = n1")
        raise AssertionError("an unknown column must be refused")
    except Exception as exc:  # noqa: BLE001
        message = str(exc)
    assert "nod" in message and "time" in message, message
    checks += 1
    print(f"  refused: {message.split('—')[-1].strip()[:88]}")
    assert_eq(
        kept(tf),
        EXPECTED_KEPT,
        "a refused query leaves the running one alone",
    )
    checks += 1

    # Half-typed, through the box: kept on screen rather than flashed away.
    tf.invoke(f"{EXT}/filter", "")
    tf.click(centre(abs_rects_of(tf.snapshot(source="paint"))["pv.filter.query"]))
    for ch in "type":
        tf.key(path="pv.filter.query", name=ch)
    assert_eq(
        len(kept(tf)),
        HELD,
        "a half-typed query keeps the capture on screen",
    )
    checks += 1
    fault = tf.query(f"{EXT}/query_fault")
    assert fault, "and says, in the bar, that it is not a query yet"
    print(f"  half-typed `type` -> the bar says: {fault}")
    checks += 1
    assert "pv.filter.fault" in abs_rects_of(tf.snapshot(source="paint")), (
        "the fault is PAINTED — a reason only the wire carries is a reason "
        "the person at the screen does not get"
    )
    checks += 1

    # ── I. the saved chips ─────────────────────────────────────────────────
    print("\n== I. a saved chip runs the query it names ==")
    tf.invoke(f"{EXT}/filter", "")
    spec = tf.query(f"{EXT}/spec")
    for n, saved in enumerate(spec["saved_filters"]):
        rects = abs_rects_of(tf.snapshot(source="paint"))
        tf.click(centre(rects[f"pv.filter.saved.{n}"]))
        assert_eq(tf.query(f"{EXT}/query"), saved["query"], f"chip {n} runs its query")
        narrowed = kept(tf)
        assert 0 < len(narrowed) < HELD, f"{saved['name']} kept {narrowed}"
        print(f"  {saved['name']:<18} -> {len(narrowed)} of {HELD}: {narrowed}")
        rects = abs_rects_of(tf.snapshot(source="paint"))
        tf.click(centre(rects[f"pv.filter.saved.{n}"]))
        assert_eq(len(kept(tf)), HELD, f"pressing {saved['name']} again clears")
        checks += 2

    # ── J. the person's own words ──────────────────────────────────────────
    print("\n== J. the query survives the round trip ==")
    written = 'name ~= "sensors/unit-*/**"'
    tf.invoke(f"{EXT}/filter", written)
    assert_eq(tf.query(f"{EXT}/query"), written, "read back exactly as written")
    checks += 1
    clause = tf.query(f"{EXT}/query_clauses")[0]
    assert_eq(clause["operand"], "sensors/unit-*/**", "the pattern is not compiled away")
    assert_eq(clause["column"], "name", "and the column is named, not numbered")
    checks += 1
    print(f"  wrote {written!r}, read back {tf.query(f'{EXT}/query')!r}")
    print("  (the floor answers `(?s:sensors/unit-[^/]*/.*)` here — the words are gone)")

    # ── K. the screen says what can be asked of it ─────────────────────────
    print("\n== K. the screen publishes its roster and its gestures ==")
    spec = tf.query(f"{EXT}/spec")
    roster = spec["query_columns"]
    columns = [c["title"] for c in spec["columns"]]
    assert set(roster) - {c.replace(" -> ", "") for c in columns}, (
        "★ the roster has to be WIDER than the drawn columns or this screen is "
        "no better addressed than the floor, which can only filter a column"
    )
    assert "note" in roster and "fragment" in roster, roster
    checks += 1
    print(f"  {len(roster)} askable names vs {len(columns)} drawn columns: {roster}")

    gestures = spec["gestures"]
    assert gestures, (
        "★ a screen that advertises nothing keeps every promise — the empty "
        "population this round's gate refuses"
    )
    # ★★★★★ R1831 — a PROPERTY rather than a count, and the count is why.
    #
    # This line pinned 3 and R1829 added a fourth gesture ("click a column
    # header"), so it broke. The pin was not protecting anything a reader
    # needs: what this section is about is that the screen's advertised
    # gestures are a NON-EMPTY set the screen can be held to, which the
    # assertion above already says — and `r1707_every_gesture_this_screen_
    # advertises_is_answered` in the crate's own suite is what holds each of
    # them to a driver. A number here is a second population that has to be
    # updated by hand every time the screen gains an affordance, and it is the
    # kind of number this tree has recorded going stale over and over.
    #
    # What IS worth pinning is that the advertised set stays a set of DISTINCT
    # promises, each with a phrase on both sides — that cannot go stale as the
    # screen grows, and it fails on the duplicate a copy-paste would introduce.
    assert_eq(
        len({g["gesture"] for g in gestures}),
        len(gestures),
        "every advertised gesture is a distinct promise",
    )
    for g in gestures:
        assert g["gesture"] and g["effect"], f"an advertised gesture with a missing half: {g}"
    checks += 1
    checks += 1
    for g in gestures:
        print(f"  {g['gesture']:<24} -> {g['effect']}")

    print(f"\n{checks} assertions")
    assert checks >= 30, f"only {checks} assertions"


def shoot(tf: RpcSubprocess, png: Path) -> Png:
    """Capture the real window and decode it."""
    tf.request("scene/screenshot", {"path": "", "out_path": str(png)})
    assert png.exists(), "the screenshot was not written"
    return read_png_rgba8(png)


def ink_rows(img: Png, rect: tuple[int, int, int, int]) -> int:
    """How many pixel rows inside `rect` carry list ink.

    ★ SCANNED rather than looked at. Twice in the three rounds before this one a
    screen that drew nothing at all passed a by-eye screenshot comparison, and
    once a screen that drew two visibly different things was judged identical.
    """
    x, y, w, h = rect
    rows = 0
    for row in range(y + 26, min(y + h, img.height)):
        inked = 0
        for col in range(x + 4, min(x + w, img.width), 3):
            r, g, b, _ = png_pixel(img, col, row)
            if abs(r - g) > 6 or abs(g - b) > 6 or r > 150:
                inked += 1
        if inked > 3:
            rows += 1
    return rows


if __name__ == "__main__":
    run_demo("hello-packet-view", main)

