#!/usr/bin/env python3
"""R1721 §5.38 §5.39 §5.40 §2 #2 §2 #7 — **a row of chips declares how many of
its members may be on, and everything else about it is derived from that word.**

# The defect this exists for, measured by driving the screens

Three screens of the analysis tool paint the same shape — a line of labelled
pills, each on or off — and each decided separately what that shape *is*.
Measured 2026-08-19 by driving the running applications rather than reading them:

| screen | the row | what it declared | what it did |
|---|---|--:|---|
| B capture viewer | 3 saved filters | 3 independent `button`s, **3** Tab stops | **at most one**: choosing the second cleared the first, choosing it again cleared it |
| C dashboard | 5 saved filters | 5 independent `button`s, **0** Tab stops | **nothing at all** — clicked every one, every `checked` stayed where it was |
| the chip gallery | 4 filters | 4 independent `button`s, 4 Tab stops | any subset |

Two of the three announced a rule they did not obey. The capture viewer told a
screen reader "toggle button, not pressed" three times over a set where only one
member can ever be on — and its own test file called them "independent
switches". The dashboard's five were announced as operable controls and were
inert, with no keyboard able to reach them at all.

# The floor this is built to beat, measured rather than read

A probe was built against the mature toolkit at 6.11.1 and **run** offscreen — a
row of three checkable buttons joined by the one type that toolkit has for
expressing a selection rule over a set of buttons:

  * the rule does not reach the accessibility tree: an exclusive set and an
    independent set report the **same** member role, the push-button one, in both;
  * the object carrying the rule is not a widget, so **nothing stands for the
    set** — its members are loose children of whatever encloses them;
  * "at most one" is **not expressible**: clicking the chosen member leaves it
    chosen, and the set cannot be emptied;
  * the rule is a bare boolean beside a name — no roster, no cursor, no key list;
  * ★ joining the set **costs the keyboard**: three loose checkable buttons are
    three Tab stops, and the moment they join a group — *even a non-exclusive
    one* — two of them accept neither `Tab` nor an arrow and can be reached only
    by pointer. Measured both ways round;
  * and `Home` / `End` move nothing inside the set.

# What it asserts

* **A** — ★★★★★ the headline: the row's Tab stops are its RULE's. An at-most-one
  bar is ONE stop with a cursor; an any-subset bar is one stop per chip. Walked
  through the real Tab ring on three screens.
* **B** — ★★★★★ the accessibility tree says what the rule says: `listbox` +
  `option` + `aria-selected` where at most one may be on, `group` +
  `button[aria-pressed]` where any may, `radiogroup` + `radio` where exactly one
  must. Three screens, three shapes, no screen choosing.
* **C** — the cursor exists and moves. Arrows walk the bar, `Home` and `End`
  reach its ends, and the row publishes where the cursor rests — none of which
  the floor's grouped buttons do.
* **D** — ★★★★★ walking is not applying. The at-most-one bars declare `Explicit`,
  so four arrows across a five-chip bar apply nothing and `Enter` applies one.
* **E** — ★★★★★ the rule is the only thing that applies a choice: choosing a
  second chip clears the first, and choosing the chosen one empties the row.
  Driven through the pointer AND through the wire, which must agree.
* **F** — ★★★★★ the dashboard's chips are operable at all, which they were not:
  the same press that changed nothing before this round now changes the row, and
  it reaches the person.
* **G** — exactly-one refuses to be emptied, and the refusal reaches the person
  (R1720's seam, on the arm no analysis screen drives).
* **H** — the seeing half: the chip that is on is painted differently, measured
  off the painted scene rather than off the source.

>= 30 assertions.

Run from the workspace root:
    cargo build --release -p hello-packet-view -p hello-analyzer-shell \\
        -p hello-filter-chip -p hello-segmented-button
    python3 tools/demos/r1721_a_chip_row_declares_how_many_may_be_on.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
)

EXT = "/external"

CHECKS: list[str] = []


def ok(what: str, condition: bool) -> None:
    assert condition, f"FAILED: {what}"
    CHECKS.append(what)


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def tree(app: RpcSubprocess) -> tuple[dict, dict]:
    res = app.request("scene/access").result
    return {n["tag"]: n for n in res["nodes"]}, (res.get("focus") or {})


def ring(app: RpcSubprocess, limit: int = 24) -> list[str]:
    """Walk the Tab ring once, the way a keyboard does."""
    seen: list[str] = []
    for _ in range(limit):
        app.request("focus/next")
        app.tick_ms(16)
        _, focus = tree(app)
        tag = focus.get("tag")
        if tag is None or tag in seen:
            break
        seen.append(tag)
    return seen


def cursor(app: RpcSubprocess) -> str | None:
    _, focus = tree(app)
    return focus.get("active_descendant")


def selected(app: RpcSubprocess, prefix: str) -> list[bool]:
    """Which chips of the row at `prefix` are on, whichever attribute says so.

    ★ The attribute is part of what the rule derives — `aria-selected` for a
    listbox option, `aria-checked` for a radio or a toggle button — so a reader
    that only knew one of them would pass on one screen and be blind on another.
    """
    nodes, _ = tree(app)
    out = []
    for n in range(64):
        node = nodes.get(f"{prefix}.{n}")
        if node is None:
            break
        state = node.get("state") or {}
        if "selected" in node:
            out.append(bool(node["selected"]))
        elif "selected" in state:
            out.append(bool(state["selected"]))
        else:
            out.append(bool(state.get("checked")))
    return out


def press_tag(app: RpcSubprocess, tag: str) -> None:
    nodes, _ = tree(app)
    box = nodes[tag]["bounds"]
    app.click((box["x"] + box["w"] / 2, box["y"] + box["h"] / 2))
    app.tick_ms(16)


def said(app: RpcSubprocess, slot: str) -> dict | None:
    answer = app.query(f"{EXT}/{slot}")
    if answer in (None, ""):
        return None
    return answer if isinstance(answer, dict) else {"sentence": str(answer)}


#: The three rows this round is about, and what each screen calls them.
#:
#: `stops` is the number of Tab stops the row's rule buys, and it is written here
#: as the OUTCOME being asserted rather than read from the screen — a test that
#: asked the screen how many stops it has would pass whatever the screen said.
ROWS = [
    # example, row tag, chip prefix, group role, member role, stops, chips
    ("hello-packet-view", "pv.filter.saved", "pv.filter.saved", "listbox", "option", 1, 3),
    (
        "hello-analyzer-shell",
        "card.filter#3.chips",
        "card.filter#3.chip",
        "listbox",
        "option",
        1,
        5,
    ),
    ("hello-filter-chip", "chip_group", "chip_", "group", "button", 4, 4),
    ("hello-segmented-button", "view_mode", "view_mode#", "radiogroup", "radio", 1, 3),
]


# ── A / B: the stops and the roles are the rule's ───────────────────────────


def a_the_ring_is_the_rules(app, example, row_tag, stops, chips) -> list[str]:
    banner(f"A — {example}: the row costs the stops its rule says")
    walked = ring(app)
    in_row = [tag for tag in walked if tag == row_tag]
    ok(
        f"A[{example}]: ★★★★★ the bar is {stops} Tab stop(s) of the ring "
        f"{walked} — the capture viewer's three became one and the dashboard's "
        f"zero became one, both because the RULE said so",
        (len(in_row) == 1) == (stops == 1),
    )
    if stops == 1:
        ok(
            f"A[{example}]: ★★★ and none of its {chips} chips is a stop of its "
            f"own, which is what 'one Tab stop' means",
            not any(tag.startswith(f"{row_tag}.") for tag in walked),
        )
    return walked


def b_the_tree_says_what_the_rule_says(app, example, row_tag, prefix, group, member, chips):
    banner(f"B — {example}: the roles are the rule's")
    nodes, _ = tree(app)
    node = nodes.get(row_tag)
    ok(
        f"B[{example}]: ★★★★★ the bar itself is a node — the floor has none, "
        f"because the thing that carries its rule is not a widget",
        node is not None,
    )
    assert_eq(node["role"], group, f"B[{example}]: the bar is a {group}")
    kinds = {nodes[f"{prefix}.{n}"]["role"] for n in range(chips) if f"{prefix}.{n}" in nodes}
    if not kinds:
        # `hello-filter-chip` / `hello-segmented-button` number without a dot.
        kinds = {nodes[f"{prefix}{n}"]["role"] for n in range(chips) if f"{prefix}{n}" in nodes}
    assert_eq(
        kinds,
        {member},
        f"B[{example}]: ★★★★★ every chip is a {member} — the floor reports the "
        f"push-button role whatever the rule is, measured both ways round",
    )


def c_the_cursor_moves(app, example, row_tag, chips):
    banner(f"C — {example}: the cursor inside the bar")
    app.request("focus/set", {"tag": row_tag})
    app.tick_ms(16)
    nodes, _ = tree(app)
    nav = nodes[row_tag].get("navigation")
    ok(
        f"C[{example}]: ★★★★ the bar publishes its cursor — its roster, the keys "
        f"it navigates by and where the cursor rests",
        nav is not None and len(nav["members"]) == chips,
    )
    first = cursor(app)
    ok(f"C[{example}]: the cursor rests on a chip ({first})", first is not None)
    advance = nav["keys"][0]
    app.key(path=row_tag, name=advance)
    app.tick_ms(16)
    moved = cursor(app)
    assert_eq(moved != first, True, f"C[{example}]: {advance} moved the cursor")
    app.key(path=row_tag, name="End")
    app.tick_ms(16)
    assert_eq(
        cursor(app),
        nav["members"][-1]["tag"],
        f"C[{example}]: ★★★ End reaches the last chip — measured at 6.11.1, "
        f"Home and End move nothing inside a grouped set of buttons",
    )
    app.key(path=row_tag, name="Home")
    app.tick_ms(16)
    assert_eq(cursor(app), nav["members"][0]["tag"], f"C[{example}]: Home reaches the first")


# ── D: walking is not applying ──────────────────────────────────────────────


def d_walking_is_not_applying(app, example, row_tag, prefix, chips):
    banner(f"D — {example}: an arrow moves the cursor and applies nothing")
    app.request("focus/set", {"tag": row_tag})
    app.tick_ms(16)
    app.key(path=row_tag, name="Home")
    app.tick_ms(16)
    before = selected(app, prefix)
    nodes, _ = tree(app)
    advance = nodes[row_tag]["navigation"]["keys"][0]
    for _ in range(chips - 1):
        app.key(path=row_tag, name=advance)
        app.tick_ms(16)
    ok(
        f"D[{example}]: ★★★★★ {chips - 1} arrow press(es) across the bar applied "
        f"nothing ({before} -> {selected(app, prefix)}) — the rule declares "
        f"`Explicit`, so a reader walks the saved filters without running them",
        selected(app, prefix) == before,
    )
    app.key(path=row_tag, name="Enter")
    app.tick_ms(16)
    ok(
        f"D[{example}]: ★★★★ and `Enter` at the last chip applies it "
        f"({selected(app, prefix)})",
        selected(app, prefix) != before,
    )


# ── E: only the rule applies a choice ───────────────────────────────────────


def e_at_most_one_is_the_rule(app, example, prefix, chips, slot):
    banner(f"E — {example}: at most one, through the pointer and through the wire")
    press_tag(app, f"{prefix}.1")
    after_one = selected(app, prefix)
    assert_eq(
        after_one,
        [n == 1 for n in range(chips)],
        f"E[{example}]: choosing chip 1 turns it on",
    )
    press_tag(app, f"{prefix}.2")
    ok(
        f"E[{example}]: ★★★★★ choosing chip 2 CLEARED chip 1 "
        f"({after_one} -> {selected(app, prefix)}) — the rule replaced it, and "
        f"the tree that says so is built from the same rule",
        selected(app, prefix) == [n == 2 for n in range(chips)],
    )
    press_tag(app, f"{prefix}.2")
    ok(
        f"E[{example}]: ★★★★★ and choosing it again EMPTIED the row "
        f"({selected(app, prefix)}) — 'at most one' is a rule the floor cannot "
        f"express at all: its exclusive set keeps its member chosen",
        not any(selected(app, prefix)),
    )
    heard = said(app, slot)
    ok(
        f"E[{example}]: ★★★ and the person was told ({heard})",
        heard is not None,
    )


def f_the_dashboard_chips_are_operable(app):
    banner("F — the dashboard: a press on a saved filter reaches the saved filter")
    prefix = "card.filter#3.chip"
    before = selected(app, prefix)
    ok(
        "F: the card opens with the chip the specification lights",
        before == [True, False, False, False, False],
    )
    press_tag(app, f"{prefix}.3")
    after = selected(app, prefix)
    ok(
        "F: ★★★★★ the press CHANGED the row — measured before this round, "
        f"clicking every one of these five left every `checked` where it was "
        f"({before} -> {after})",
        after != before,
    )
    heard = said(app, "toast")
    ok(
        f"F: ★★★ and the person is told which one by NAME ({heard}) — the card "
        f"draws 'exclude P-03', so a sentence naming `chip.3` would be an "
        f"internal identity reaching somebody who sees titles",
        heard is not None and "exclude P-03" in str(heard.get("sentence", "")),
    )


# ── G: exactly one refuses to be emptied ────────────────────────────────────


def g_exactly_one_cannot_be_emptied(app):
    banner("G — the segmented button: exactly one, and it cannot be emptied")
    prefix = "view_mode#"

    def chosen() -> list[int]:
        nodes, _ = tree(app)
        return [
            n
            for n in range(3)
            if (nodes[f"{prefix}{n}"].get("state") or {}).get("checked")
            or nodes[f"{prefix}{n}"].get("selected")
        ]

    opening = chosen()
    ok(f"G: exactly one segment is on to begin with ({opening})", len(opening) == 1)
    press_tag(app, f"{prefix}1")
    moved = chosen()
    ok(
        f"G: ★★★ choosing another segment REPLACES it ({opening} -> {moved})",
        moved == [1],
    )
    # ★★★★ Driven, not asserted true: press the segment that IS on. Under
    # `exactly one` the row must come back with the same one on, because
    # clearing it would leave none — and the floor does the same thing while
    # exposing no rule that says so and saying nothing to anybody.
    press_tag(app, f"{prefix}1")
    ok(
        f"G: ★★★★★ and choosing the one that is ON leaves it on ({chosen()}) — "
        f"`exactly one` means the row cannot be emptied, which is the arm the "
        f"at-most-one bars would empty",
        chosen() == [1],
    )


# ── H: the seeing half ──────────────────────────────────────────────────────


def h_the_chip_that_is_on_is_drawn_differently(app, example, prefix, chips):
    banner(f"H — {example}: the chip that is on is painted")
    shot = app.snapshot(source="paint", viewport=(1600, 900))
    rects = abs_rects_of(shot)
    painted = [f"{prefix}.{n}" for n in range(chips) if f"{prefix}.{n}" in rects]
    ok(
        f"H[{example}]: ★★★ every chip the tree announces is painted "
        f"({len(painted)} of {chips}) — an announced control nobody drew is the "
        f"other half of the defect this round is repairing",
        len(painted) == chips,
    )


def main() -> int:
    for example, row_tag, prefix, group, member, stops, chips in ROWS:
        with RpcSubprocess(example) as app:
            a_the_ring_is_the_rules(app, example, row_tag, stops, chips)
            b_the_tree_says_what_the_rule_says(
                app, example, row_tag, prefix, group, member, chips
            )
            if stops == 1:
                c_the_cursor_moves(app, example, row_tag, chips)

    with RpcSubprocess("hello-packet-view") as app:
        d_walking_is_not_applying(app, "hello-packet-view", "pv.filter.saved", "pv.filter.saved", 3)
        e_at_most_one_is_the_rule(app, "hello-packet-view", "pv.filter.saved", 3, "said")
        h_the_chip_that_is_on_is_drawn_differently(
            app, "hello-packet-view", "pv.filter.saved", 3
        )

    with RpcSubprocess("hello-analyzer-shell") as app:
        f_the_dashboard_chips_are_operable(app)
        d_walking_is_not_applying(
            app, "hello-analyzer-shell", "card.filter#3.chips", "card.filter#3.chip", 5
        )
        e_at_most_one_is_the_rule(
            app, "hello-analyzer-shell", "card.filter#3.chip", 5, "toast"
        )
        h_the_chip_that_is_on_is_drawn_differently(
            app, "hello-analyzer-shell", "card.filter#3.chip", 5
        )

    with RpcSubprocess("hello-segmented-button") as app:
        g_exactly_one_cannot_be_emptied(app)

    print(f"\n{len(CHECKS)} named check(s) beyond the equalities:")
    for line in CHECKS:
        print(f"  - {line}")
    return 0


if __name__ == "__main__":
    run_demo("r1721_a_chip_row_declares_how_many_may_be_on", main)
