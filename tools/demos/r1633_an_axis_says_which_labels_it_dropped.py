#!/usr/bin/env python3
"""R1633 — an axis says which labels it dropped.

A category axis asked for one tick per slot and got one label per slot, whatever
the room. Thirty endpoint names in a 690-pixel panel — the shape an
analyzer-class dashboard's axis actually has — drew thirty names on top of one
another, and nothing said so.

The two references do two different things and neither is enough (measured at
6.11 and at `8cf50599`):

* The toolkit draws every label and hides the colliders — but only on its
  VERTICAL axis. Its horizontal axis hides a colliding label only when the text
  has already been elided down to `"..."`, and its newer graphs module has no
  overlap pass at all. Both hide with `setVisible(false)`, so nothing can ask.
* The DCC picks the grid STEP from a measured label width instead, so no label
  is ever dropped — better, and it only works on an axis with a ladder. Its
  editors have no categorical axis.

`pinion_chart` does both, chosen by what the axis is, measures with the real
face through the §5.36 `TextMetrics` seam, and **publishes what it did**.

What each check discriminates:

* **The picture and the wire cannot drift.** Every check reads the painted label
  count AND the derivation, and asserts the arithmetic between them: labels
  drawn + `labels_omitted` = slots. A fit that computed the right answer and
  never reached the canvas is the failure this is written against — and this
  round hit one, in three charts that drew category labels outside the fit.
* **The marks are untouched.** Thinning the LABELS must not thin the data; an
  axis that dropped slots would be a different chart.
* **Both ends keep their names.** The reference's greedy first-wins scan drops
  whichever end it reaches last, so the axis stops saying where it ends.
* **The report is an ABSENCE when nothing was dropped.** Publishing `0` would
  make "did this axis hide a label" answer yes for every chart ever drawn.
* **The same axis, fewer categories, no omission.** Without that pair the
  assertions above would hold for a pass that dropped everything.

What this demo does NOT show, and where it is proven instead: the **ladder**
half (a linear / log / time axis lowering its tick target rather than dropping a
label) needs an axis whose pixel span changes, and no example in the tree builds
a labelled ladder chart into a resizable frame. It is asserted in
`pinion_chart`'s own tests — `r1633_a_ladder_axis_coarsens_and_drops_no_label`,
`r1633_a_vertical_axis_fits_by_line_height` — and its *report* is asserted here,
because that half is the one that omits nothing and so has no paint to disagree
with.

Run from the workspace root:
    cargo build -p hello-category-axis --release
    python3 tools/demos/r1633_an_axis_says_which_labels_it_dropped.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    texts_of,
)

#: The bar chart's tag prefix in `hello-category-axis`.
BARS = "bars"

#: The external the axis mode hangs off.
DENSE = "/category_window/external/dense"

#: The window this example opens at.
WIN = (720, 560)


def derivations(tf: RpcSubprocess, kind: str | None = None) -> list[dict]:
    params: dict = {"tag": BARS, "viewport": list(WIN)}
    if kind is not None:
        params["kind"] = kind
    return list(tf.request("scene/derivations", params).result.get("derivations", []))


def named(entries: list[dict], name: str, subject: str | None = None) -> list[dict]:
    out = [e for e in entries if e.get("name") == name]
    if subject is not None:
        out = [e for e in out if e.get("subject") == subject]
    return out


def value(entry: dict):
    """The evidence a derivation carries, whatever shape it took."""
    evidence = entry.get("evidence")
    if isinstance(evidence, dict):
        for key in ("name", "count", "real", "flag"):
            if key in evidence:
                return evidence[key]
    return evidence


def painted(tf: RpcSubprocess) -> tuple[int, int, list[str]]:
    """(labels drawn, bars drawn, the label strings) off the painted frame.

    The label tags carry the SLOT index, so a strided axis paints
    `bars.xlabel.0`, `.4`, `.8`… — the names are gathered by tag and ordered by
    that index, which is also what makes "the last slot is named" checkable.
    """
    snap = tf.snapshot(source="paint", viewport=list(WIN))
    rects = abs_rects_of(snap)
    prefix = f"{BARS}.xlabel."
    slots = sorted(
        int(tag[len(prefix) :]) for tag in rects if tag.startswith(prefix)
    )
    bars = sum(1 for t in rects if t.startswith(f"{BARS}.bar."))
    texts = []
    for slot in slots:
        node = find_by_tag(snap, f"{prefix}{slot}")
        assert node is not None, f"slot {slot} was in the rects and is not in the tree"
        found = texts_of(node)
        assert found, f"slot {slot} paints a label node with no text"
        texts.append(found[0])
    return len(slots), bars, texts


def a_sparse_axis_labels_every_slot(tf: RpcSubprocess) -> tuple[int, int]:
    entries = derivations(tf)
    rule = named(entries, "label_fit", "axis.x")
    assert len(rule) == 1, f"the axis says how it fitted, once: {rule}"
    assert_eq(value(rule[0]), "fits", "twelve short months fit this panel")
    assert_eq(
        named(entries, "labels_omitted", "axis.x"),
        [],
        "★ and 'nothing was dropped' is an ABSENCE, not a zero — a client "
        "filtering for omissions must get an empty answer exactly when the "
        "picture leaves nothing off",
    )
    assert_eq(
        named(entries, "label_crowding", "axis.x"),
        [],
        "and it is not crowded either",
    )
    labels, bars, _ = painted(tf)
    assert_eq(labels, bars, "every slot is named")
    print(f"[demo] months: label_fit=fits, {labels} names for {bars} bars")
    return labels, bars


def a_dense_axis_strides_and_says_which(tf: RpcSubprocess) -> None:
    slots = int(str(tf.invoke(DENSE, True)))
    assert slots > 20, f"the dense axis is the analyzer's shape: {slots}"
    tf.tick(0.016)

    entries = derivations(tf)
    rule = named(entries, "label_fit", "axis.x")
    assert_eq(
        value(rule[0]),
        "strided",
        f"★ a CATEGORY axis strides — it has no ladder to coarsen: {rule}",
    )
    omitted = named(entries, "labels_omitted", "axis.x")
    assert len(omitted) == 1, f"one count for the axis, not one per label: {omitted}"
    left_off = int(value(omitted[0]))
    assert left_off > 0, omitted

    labels, bars, texts = painted(tf)
    assert_eq(bars, slots, "★ every endpoint still has its BAR — only names thinned")
    assert_eq(
        labels + left_off,
        slots,
        "★ the count on the wire is exactly the labels the canvas stopped "
        "drawing; the report and the picture cannot drift",
    )
    print(
        f"[demo] endpoints: label_fit=strided, {labels} names + {left_off} omitted "
        f"= {slots} bars"
    )

    # ★ Both ends keep their names. The reference's first-wins scan drops
    # whichever end it reaches last, and an axis missing its final label does
    # not say where it stops.
    assert texts, f"the painted names are readable: {texts[:5]}"
    assert_eq(texts[0], "/health", "the first endpoint is named")
    assert_eq(texts[-1], "/version", "★ and so is the LAST one")
    assert len(set(texts)) == len(texts), f"and no name is drawn twice: {texts}"
    print(f"[demo] both ends pinned: {texts[0]} .. {texts[-1]}")


def going_back_restores_every_label(tf: RpcSubprocess, before: tuple[int, int]) -> None:
    assert_eq(int(str(tf.invoke(DENSE, False))), 12, "back to the sparse axis")
    tf.tick(0.016)
    entries = derivations(tf)
    assert_eq(value(named(entries, "label_fit", "axis.x")[0]), "fits")
    assert_eq(
        named(entries, "labels_omitted", "axis.x"),
        [],
        "★ and the omission is GONE — not a stale count, and not a zero",
    )
    assert_eq(painted(tf)[:2], before, "the picture is exactly what it was")
    print("[demo] back to twelve: fits, no omission entry at all")


def the_other_axis_answers_for_itself(tf: RpcSubprocess) -> None:
    entries = derivations(tf)
    subjects = sorted(
        e["subject"] for e in entries if e.get("name") == "label_fit"
    )
    assert_eq(
        subjects,
        ["axis.x", "axis.y"],
        "★ both axes publish, separately — the toolkit's two axes are two "
        "implementations with different BEHAVIOUR, and here the difference is "
        "one value: a horizontal label takes room by its width, a vertical one "
        "by its line height",
    )
    y = named(entries, "label_fit", "axis.y")[0]
    assert value(y) in {"fits", "coarsened"}, y
    assert_eq(
        named(entries, "labels_omitted", "axis.y"),
        [],
        "★ a LADDER axis omits nothing whatever it does — it has fewer ticks, "
        "not fewer labels. That is the assertion that tells the two rules "
        "apart; 'fewer labels' alone would pass either one",
    )
    print(f"[demo] the y axis answers on its own: {value(y)}, omitting nothing")


def the_vocabulary_is_closed(tf: RpcSubprocess) -> None:
    """Every fit entry is one of the three names, under the right kind."""
    tf.invoke(DENSE, True)
    tf.tick(0.016)
    chosen = {e["name"] for e in derivations(tf, kind="chosen")}
    omitted = {e["name"] for e in derivations(tf, kind="omitted")}
    assert "label_fit" in chosen, f"the RULE is a choice a reader can act on: {chosen}"
    assert "labels_omitted" in omitted, (
        f"and what was left off is an omission: {omitted}"
    )
    assert "label_fit" not in omitted, "a rule is not an omission"
    # A `Discarded` crowding entry exists only when the fit could not succeed,
    # which this axis is not — so its absence here is the same "only when there
    # is something" rule, checked through the kind filter.
    assert "label_crowding" not in {e["name"] for e in derivations(tf, kind="discarded")}
    print("[demo] the three names sit under the kinds a reader filters by")


def body() -> None:
    with RpcSubprocess("hello-category-axis", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        sparse = a_sparse_axis_labels_every_slot(tf)
        a_dense_axis_strides_and_says_which(tf)
        going_back_restores_every_label(tf, sparse)
        the_other_axis_answers_for_itself(tf)
        the_vocabulary_is_closed(tf)


if __name__ == "__main__":
    run_demo("R1633 — an axis says which labels it dropped", body)
