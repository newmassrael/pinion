#!/usr/bin/env python3
"""R1550 §5.16 §5.36 §5.7 — every arena states what it is holding, in BYTES.

Before this round nothing in this tree could state its memory. A census of
the RPC surface found **not one field in bytes**: `scene/cache_stats` answers
with `entries`, `scene/text_cache_stats` with `entries` / `capacity` /
`max_capacity`, `scene/frame_timings` with node counts. Every one of those is
a count of *things*, and a count of things is not a footprint — the shape
cache holds one entry for "OK" and one for a 10,000-character paragraph and
reports `2` for both.

`scene/memory` is the memory axis: one row per arena per owner, in bytes,
plus what the OS says the process is resident for.

Four things this proves that Qt 6.11 cannot answer:

  1. USAGE, NOT JUST THE BUDGET. `QPixmapCache::setCacheLimit(int kb)` sets a
     byte budget and `cacheLimit()` reads it back — and there is **no
     accessor at all** for how much of it is in use. A Qt application cannot
     tell whether its pixmap cache sits at 1% or 99% of its own ceiling.
     `QFontCache`, the closer analogue of the shape cache, is private in its
     entirety. Here every arena answers with both.

  2. THE ACCOUNTING STATES ITS OWN BASIS. `basis: "partial"` on the shape
     cache is not hedging: the measured bytes are exact, and the row NAMES
     what it could not reach (`parley::Layout`, MOST of whose buffers are
     behind a `pub(crate)` field — the two it hands out as slices of public
     types are counted, R1550.1) with a count. Qt's `QImage::sizeInBytes()`
     is a formula over two members with nothing tying it to the object's
     fields.

  3. MEMORY IS ATTRIBUTED PER WINDOW, AND SHARED ARENAS EXACTLY ONCE. The
     producer image store is held by every window's cache and counted in
     none of them — it gets its own shell-wide row. Asserted here by
     registering an image and watching the shell-wide row move while the
     per-window rows do not.

  4. THE PROCESS TOTAL SITS BESIDE THE ARENAS, so the unattributed remainder
     is a subtraction rather than an implication. Qt publishes no
     process-memory API of any kind.

And the claim that makes it a PERFORMANCE fact rather than a readout: **held
bytes are bounded by what is visible, not by how big the model is.** The
model grows 100 -> 1,000,000 rows and the arenas do not move; the eager arm
is the negative control, where they do. Same shape as R1538's node census,
on the other resource.

Run from the workspace root:
    cargo build -p hello-scene-scale -p hello-memory-image --release
    python3 tools/demos/r1550_arena_states_its_bytes.py
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_until,
)

APP = "hello-scene-scale"
LIST_TAG = "scale"
EXT = f"/{LIST_TAG}/external"

IMAGE_APP = "hello-memory-image"
IMAGE_ORACLE = "/memory_image/external"
# The binding registers one 16x16 RGBA8 image, so the store holds exactly
# this many pixel bytes while it is present.
IMAGE_PIXEL_BYTES = 16 * 16 * 4

LADDER = [100, 1_000, 10_000, 100_000, 1_000_000]

REPO = Path(__file__).resolve().parent.parent.parent

# Arena names, as published. Stable strings an agent matches on.
FRAGMENTS = "paint-fragments"
SHAPES = "text-shapes"
IMAGES = "images"


def census(tf: RpcSubprocess) -> dict:
    return tf.request("scene/memory", {}).result


def rows_of(cen: dict, arena: str, window: str | None = ...) -> list[dict]:
    """Rows for `arena`, optionally filtered by owner (`None` = shell-wide)."""
    out = [r for r in cen["arenas"] if r["arena"] == arena]
    if window is not ...:
        out = [r for r in out if r["window"] == window]
    return out


def one(cen: dict, arena: str, window: str | None = ...) -> dict:
    found = rows_of(cen, arena, window)
    assert len(found) == 1, f"expected exactly one {arena} row, got {found}"
    return found[0]


def drive_frame(tf: RpcSubprocess, baseline: int, desc: str) -> None:
    """Drive real paints until `frame_count` passes `baseline`.

    An arena is filled by PAINTING. A census read off a producer pass would
    describe caches nobody populated.
    """

    def advanced() -> bool:
        try:
            if int(tf.frame_timings()["frame_count"]) > baseline:
                return True
        except RpcError:
            pass
        tf.request("scene/screenshot", {"path": ""})
        return False

    wait_until(advanced, desc=desc)


def assert_row_shape(row: dict, label: str) -> None:
    """Present, typed, and internally coherent — before any of it is believed."""
    for key in ("arena", "window", "bytes", "entries", "basis", "unmeasured"):
        assert key in row, f"{label}: row is missing {key!r}: {row}"
    assert isinstance(row["bytes"], int) and row["bytes"] >= 0, label
    assert isinstance(row["entries"], int) and row["entries"] >= 0, label
    assert row["basis"] in ("exact", "partial"), f"{label}: {row['basis']!r}"
    assert isinstance(row["unmeasured"], list), label
    # The basis is DERIVED from `unmeasured`, so the two cannot disagree.
    expected = "partial" if row["unmeasured"] else "exact"
    assert_eq(row["basis"], expected, f"{label}: basis follows unmeasured")
    for un in row["unmeasured"]:
        assert set(un) == {"type", "count"}, f"{label}: {un}"
        assert un["count"] > 0, f"{label}: a zero count is not published: {un}"
        assert "::" in un["type"], f"{label}: name the type: {un}"
    budget = row["budget_bytes"]
    assert budget is None or (isinstance(budget, int) and budget > 0), label


def body() -> None:
    # ── (A) the census exists, is complete, and is coherent ─────────────────
    with RpcSubprocess(APP, boot_grace=1.5) as tf:
        base = int(tf.frame_timings()["frame_count"])
        drive_frame(tf, base, "boot frame")

        cen = census(tf)
        assert set(cen) == {"arenas", "total_bytes", "process_rss_bytes"}, cen

        # A GUI shell holds three arena kinds: one window's fragments and
        # images, plus the shell-wide shape cache and producer image store.
        assert_eq(len(rows_of(cen, FRAGMENTS)), 1, "one paint-fragment arena")
        assert_eq(len(rows_of(cen, SHAPES)), 1, "one shape arena")
        assert_eq(len(rows_of(cen, IMAGES)), 2, "one per window + the store")
        for row in cen["arenas"]:
            assert_row_shape(row, row["arena"])

        # Ownership: the per-window arenas name their window, the shell-wide
        # ones do not. That distinction is what makes a three-window shell's
        # memory attributable at all.
        assert_eq(one(cen, FRAGMENTS)["window"], "main", "fragments are per window")
        assert_eq(one(cen, SHAPES)["window"], None, "one shape cache per shell")
        assert_eq(len(rows_of(cen, IMAGES, "main")), 1, "a window's decode cache")
        assert_eq(len(rows_of(cen, IMAGES, None)), 1, "the shell-wide store")

        # ── (B) the numbers are real ────────────────────────────────────────
        frag = one(cen, FRAGMENTS)
        shapes = one(cen, SHAPES)
        assert frag["bytes"] > 0, f"a painted window holds encoded fragments: {frag}"
        assert frag["entries"] > 0, f"and they are cached: {frag}"
        assert shapes["bytes"] > 0, f"a window with text holds shaped layouts: {shapes}"
        assert shapes["entries"] > 0, shapes
        # Bytes are not entries — the whole point of the round. A cache
        # holding text costs far more than one machine word per entry.
        assert shapes["bytes"] > 16 * shapes["entries"], (
            f"an entry that cost {shapes['bytes'] / shapes['entries']:.0f} bytes "
            f"is not being priced by content: {shapes}"
        )
        assert_eq(
            cen["total_bytes"],
            sum(r["bytes"] for r in cen["arenas"]),
            "the total is the sum of the rows",
        )

        # ── (C) the basis, published rather than footnoted ──────────────────
        # The two vello-side arenas are measured to the byte; the shape cache
        # is not, and says which foreign values it could not reach.
        assert_eq(frag["basis"], "exact", "a vello encoding reports its buffers")
        assert_eq(one(cen, IMAGES, "main")["basis"], "exact", "pixels are countable")
        assert_eq(shapes["basis"], "partial", "parley's Layout is opaque")
        named = {u["type"] for u in shapes["unmeasured"]}
        assert "parley::Layout" in named, f"name the opaque type: {shapes}"
        assert_eq(
            next(u["count"] for u in shapes["unmeasured"] if u["type"] == "parley::Layout"),
            shapes["entries"],
            "one opaque layout per cached entry",
        )

        # ── (D) the budget, and the usage against it ────────────────────────
        # This is Qt's `QPixmapCache::cacheLimit()` — with the number Qt has
        # no accessor for, beside it.
        img = one(cen, IMAGES, "main")
        assert_eq(img["budget_bytes"], 10 * 1024 * 1024, "QPixmapCache's own default")
        assert img["bytes"] <= img["budget_bytes"], "and the arena is inside it"
        # The arenas bounded by something other than bytes say so with null,
        # rather than publishing a fictitious ceiling.
        assert_eq(shapes["budget_bytes"], None, "the shape cache bounds entries")
        assert_eq(frag["budget_bytes"], None, "fragments are bounded by the paint")

        # ── (E) the process total ───────────────────────────────────────────
        rss = cen["process_rss_bytes"]
        assert isinstance(rss, int) and rss > 8 * 1024 * 1024, (
            f"a running GPU shell is resident for more than 8 MiB; got {rss!r}"
        )
        assert rss > cen["total_bytes"], (
            "the arenas are a subset of the process — the widget tree, taffy's "
            f"nodes and the driver's buffers are outside them: {cen['total_bytes']} "
            f"vs {rss}"
        )

        # ── (F) SCALE INVARIANCE — the round's performance claim ────────────
        # Held bytes are bounded by what is VISIBLE. The model grows four
        # orders of magnitude and the arenas do not move.
        held: dict[int, tuple[int, int]] = {}
        for rows in LADDER:
            tf.intervene(f"{EXT}/rows", rows)
            assert_eq(tf.query(f"{EXT}/rows"), rows, f"the model took rows={rows}")
            count = int(tf.frame_timings()["frame_count"])
            drive_frame(tf, count, f"paint at rows={rows}")
            cen = census(tf)
            held[rows] = (one(cen, SHAPES)["entries"], one(cen, FRAGMENTS)["entries"])

        # The two arenas make the claim differently, and the difference is a
        # property of what each one IS rather than a weaker assertion.
        #
        # `paint-fragments` is mark-and-swept at the end of every paint, so it
        # holds exactly the live set: EQUAL across four orders of magnitude.
        #
        # `text-shapes` is an LRU, so it retains the union of what has been
        # painted — changing the model paints labels that read "row 999999"
        # where they read "row 99", and those are distinct keys. What it must
        # not do is grow WITH the model, and it does not: measured across a
        # 10,000x model, entries move by two.
        first_shapes, first_frags = held[LADDER[0]]
        for rows in LADDER:
            shapes_held, frags_held = held[rows]
            assert_eq(
                frags_held,
                first_frags,
                f"rows={rows} paints what rows={LADDER[0]} paints "
                f"({rows // LADDER[0]}x the model, same live fragment set)",
            )
            assert shapes_held < 2 * first_shapes, (
                f"the shape cache is bounded by the visible window, not by the "
                f"model: rows={rows} ({rows // LADDER[0]}x) holds {shapes_held} "
                f"against {first_shapes}"
            )

        # ── (G) the negative control ───────────────────────────────────────
        # A guard that can only measure the passing case cannot fail. The
        # eager arm builds every row, so its arenas MUST grow with the model.
        cap = int(tf.query(f"{EXT}/max_eager_rows"))
        assert cap >= 10 * LADDER[0], f"the cap must span a decade: {cap}"
        # Back under the cap first: the binding REFUSES the eager arm while
        # the model is larger than it can eagerly build, which is the point of
        # having a cap at all.
        tf.intervene(f"{EXT}/rows", LADDER[0])
        tf.intervene(f"{EXT}/eager", True)
        assert_eq(tf.query(f"{EXT}/eager"), True, "the eager arm is entered")

        eager: dict[int, int] = {}
        for rows in (LADDER[0], min(cap, LADDER[1])):
            tf.intervene(f"{EXT}/rows", rows)
            count = int(tf.frame_timings()["frame_count"])
            drive_frame(tf, count, f"eager paint at rows={rows}")
            eager[rows] = one(census(tf), SHAPES)["entries"]

        small, large = LADDER[0], min(cap, LADDER[1])
        assert eager[large] > 4 * eager[small], (
            f"the eager arm builds every row, so {large} rows must hold far "
            f"more shaped text than {small}: {eager}"
        )

        # ── (H) the method is discoverable and described ────────────────────
        methods = {m["name"] for m in tf.request("rpc/methods", {}).result["methods"]}
        assert "scene/memory" in methods, "the axis is on the published surface"
        schema = tf.request("rpc/schema", {}).result
        types = {t["name"] for t in schema["types"]}
        for name in ("MemoryOutcome", "MemoryArena", "MemoryUnmeasured"):
            assert name in types, f"{name} must be in the published census"

    # ── (I) a SHARED arena is counted exactly once ──────────────────────────
    # Every window's image cache holds a handle to the producer store. If the
    # store were counted through those handles, a three-window shell would
    # report one registered image three times. Here it has its own row, and
    # the per-window rows stay at zero while it moves.
    with RpcSubprocess(IMAGE_APP, boot_grace=1.5) as tf:
        base = int(tf.frame_timings()["frame_count"])
        drive_frame(tf, base, "image boot frame")

        assert_eq(tf.query(f"{IMAGE_ORACLE}/registered"), 1, "boot: one image held")
        cen = census(tf)
        store = one(cen, IMAGES, None)
        window_images = one(cen, IMAGES, "main")
        assert store["bytes"] >= IMAGE_PIXEL_BYTES, (
            f"the store owns a 16x16 RGBA8 image: {store}"
        )
        assert_eq(store["entries"], 1, "one registered key")
        assert_eq(window_images["bytes"], 0, "the window decoded nothing from disk")
        assert_eq(window_images["entries"], 0, "so its own arena is empty")

        # Remove it: the bytes go away, and they go away from the row that
        # owned them. A live memory measurement over the wire.
        assert_eq(tf.invoke(f"{IMAGE_ORACLE}/send", "remove"), "absent", "removed")
        assert_eq(tf.query(f"{IMAGE_ORACLE}/registered"), 0, "store is empty")
        count = int(tf.frame_timings()["frame_count"])
        drive_frame(tf, count, "paint after removal")
        after = one(census(tf), IMAGES, None)
        assert_eq(after["entries"], 0, "the store row follows the store")
        assert after["bytes"] < store["bytes"], (
            f"and its bytes fell with it: {store['bytes']} -> {after['bytes']}"
        )

        # Restore, and the bytes come back. A one-way assertion could be
        # satisfied by an arena that simply stopped answering.
        # The palette it comes back as is the binding's business (whatever was
        # last registered); that the BYTES come back is this round's.
        tf.invoke(f"{IMAGE_ORACLE}/send", "restore")
        assert_eq(tf.query(f"{IMAGE_ORACLE}/present"), True, "restored")
        count = int(tf.frame_timings()["frame_count"])
        drive_frame(tf, count, "paint after restore")
        back = one(census(tf), IMAGES, None)
        assert_eq(back["entries"], 1, "registered again")
        assert back["bytes"] >= IMAGE_PIXEL_BYTES, f"and priced again: {back}"

    # ── (J) COVERAGE — every arena in the tree is in the census ─────────────
    # The compiler stops a cached struct from growing a field the accounting
    # forgets (every `Footprint` impl destructures its type). It cannot stop
    # someone adding a whole new cache. This is that gate: a source parse for
    # cache types, each of which must implement `MeasuredArena` or carry a
    # written reason for being outside the census.
    outside = {
        # Consumer-owned and generic over the consumer's own value type, so
        # its footprint is not computable without a `V: Footprint` bound the
        # binding would have to satisfy. The shell holds none of these; an
        # arena registry for binding-owned caches is the extension point, and
        # is not built.
        "ResourceCache": "consumer-owned, generic over user values",
    }
    declared = set()
    implemented = set()
    for src in (REPO / "crates").rglob("*.rs"):
        text = src.read_text(encoding="utf-8", errors="replace")
        declared |= set(re.findall(r"^pub struct (\w*Cache)\b", text, re.M))
        implemented |= set(re.findall(r"^\s*impl MeasuredArena for (\w+)", text, re.M))
    assert declared, "the parse found no cache types at all — it is broken"
    uncovered = declared - implemented - set(outside)
    assert not uncovered, (
        f"these arenas are memory nothing reports: {sorted(uncovered)}. Either "
        f"implement MeasuredArena and publish a row, or add a written reason."
    )
    for name in outside:
        assert name in declared, (
            f"{name} is excused from the census but no longer exists — an "
            f"exclusion list that outlives its subject excuses the next type "
            f"to take the name"
        )


if __name__ == "__main__":
    run_demo("R1550 an arena states what it is holding", body)
