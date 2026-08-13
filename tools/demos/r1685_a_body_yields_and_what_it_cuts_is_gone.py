#!/usr/bin/env python3
"""R1685 §5.21 §5.45 §5.12 §2 #6 §2 #7 — **a box can say that what does not fit
is cut here, and every surface agrees about it.**

CSS says this in one word:

    .body { flex: 1 1 auto; overflow: hidden; }

and until this round pinion could express only half of it. The layout half was
reachable — `LayoutStyle::min_size = Px(0)` produces the same relaxation of the
CSS automatic minimum size — but the paint half existed nowhere: `Scene::Scroll`
was the only node that clipped, so a region that had to cut its overflow had to
become scrollable, or its pixel budget had to be balanced by hand. A consumer
reported exactly that, having read the `min_size` documentation (which names the
CSS rule), concluded `overflow: hidden` had been ported, and hit the wall at the
far end of the work.

`LayoutStyle::overflow` is the one declaration, and this drives all of it on the
real pipeline:

  (A) the declaration is ON THE WIRE — `scene/snapshot` publishes `clips` per
      container, so a client reading a child rect that leaves its parent can
      tell whether that ink reaches the screen at all.
  (B) the LAYOUT half — the chrome rows keep their declared heights at every
      window size and the body absorbs the whole difference.
  (C) the PAINT half — `scene/containment` reports the cut entries as `clipped`
      rather than `smeared`, and the count moves with the window.
  (D) REACHABILITY — `scene/scroll_reach` calls them `lost`: a hidden box has no
      range, so nothing brings them back. This is the assertion that made the
      workaround worth refusing rather than documenting: a pinned `Scene::Scroll`
      used as a clip would have answered `scrollable` with an offset no gesture
      in the application could reach.
  (E) the POINTER agrees with the picture — `scene/locate` at a cut entry's
      coordinates lands on the chrome painted there, not on the entry.

Run from the workspace root:
    cargo build -p hello-overflow-clip --release
    python3 tools/demos/r1685_a_body_yields_and_what_it_cuts_is_gone.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    call,
    resize_and_settle,
    run_demo,
)

#: The size the screen opens at, and the two the demo drives it to. The rows
#: are 9 x 56 = 504 tall against a body that gets 404 at the opening size, so
#: the interesting state is already on screen at boot.
WIN = (420, 560)
TALL = (420, 760)
SHORT = (420, 360)

HEADER_H = 48
ACTION_H = 64
TABBAR_H = 44
ROWS = 9
ROW_H = 56


def containers(node: Any, out: dict[str, dict]) -> dict[str, dict]:
    """Every tagged container in a `scene/snapshot`, keyed by tag."""
    if isinstance(node, dict):
        if node.get("type") == "Container" and node.get("tag"):
            out[node["tag"]] = node
        for child in node.get("children", []) or []:
            containers(child, out)
        content = node.get("content")
        if content:
            containers(content, out)
    return out


def paint(tf: RpcSubprocess, size: tuple[int, int]) -> dict[str, dict]:
    return containers(tf.snapshot(source="paint", viewport=size), {})


def body_height(size: tuple[int, int]) -> int:
    return size[1] - HEADER_H - ACTION_H - TABBAR_H


def resized_to(tf: RpcSubprocess, size: tuple[int, int]) -> dict[str, dict]:
    """The tagged containers of the frame that landed after a resize.

    ★★★★★ R1686 — this was `resize` then one `tick(0.05)`, and that is a sleep
    wearing a tick's clothes: `scene/snapshot from=paint` reads the last
    RENDERED frame, so a resize that has not repainted yet answers with the
    previous window's rectangles. It reported the opening body height (404) at
    the tall size (604) once under load and passed three times idle — a flake,
    which this project does not carry at any rate ([[zero-flake-policy]]).
    The wait itself is `rpc_verify.resize_and_settle`, lifted there in the same
    round because four demos had written it and three had written it wrong.
    """
    return containers(resize_and_settle(tf, size), {})


def run(tf: RpcSubprocess) -> None:
    # ── A. the declaration rides on the wire ─────────────────────────────────
    tagged = paint(tf, WIN)
    for tag in ("overflow_clip", "chrome.header", "body", "body.content",
                "chrome.action", "chrome.tabbar"):
        assert tag in tagged, f"{tag} is painted: {sorted(tagged)}"
    for tag, node in sorted(tagged.items()):
        assert "clips" in node, (
            f"every container publishes whether it cuts its children; {tag} "
            f"does not: {sorted(node)}"
        )
        assert isinstance(node["clips"], bool), node["clips"]
    assert_eq(tagged["body"]["clips"], True, "the body declares the cut")
    for tag in ("overflow_clip", "chrome.header", "body.content",
                "chrome.action", "chrome.tabbar"):
        assert_eq(tagged[tag]["clips"], False, f"{tag} declares no cut")

    # ── B. the layout half: the chrome holds, the body yields ────────────────
    for size in (WIN, TALL, SHORT):
        at = resized_to(tf, size)
        assert_eq(at["chrome.header"]["rect"]["h"], HEADER_H, f"header at {size}")
        assert_eq(at["chrome.action"]["rect"]["h"], ACTION_H, f"action at {size}")
        assert_eq(at["chrome.tabbar"]["rect"]["h"], TABBAR_H, f"tabbar at {size}")
        assert_eq(at["body"]["rect"]["h"], body_height(size), f"the body absorbs it at {size}")
        tabbar = at["chrome.tabbar"]["rect"]
        assert_eq(tabbar["y"] + tabbar["h"], size[1], f"the tab bar ends at the bottom at {size}")
        # The content keeps its full height whatever the window does — it is
        # the BODY that gives way, which is what makes the cut observable.
        assert_eq(
            at["body.content"]["rect"]["h"],
            ROWS * ROW_H,
            f"the entries never shrink at {size}",
        )

    # ── C. the paint half: what the body cut is reported as CUT ──────────────
    # Settled, not ticked: `scene/containment` is derived from the painted
    # scene, so it answers about the previous window until the resize has been
    # rendered — the same race section B was flaking on.
    resized_to(tf, WIN)
    contained = call(tf, "scene/containment")
    for key in ("escapes", "smeared", "clipped", "marks"):
        assert key in contained, f"scene/containment must report {key}"
    assert contained["marks"] > 20, (
        f"only {contained['marks']} mark(s) examined — an empty escape list on "
        f"a surface that painted nothing is not the same answer as one on a "
        f"screen that painted a body full of entries"
    )
    from_body = [e for e in contained["escapes"] if e["owner"] == "body"]
    assert from_body, (
        f"the entries are taller than the body, so the body has escapes: "
        f"{contained['escapes'][:3]}"
    )
    for escape in from_body:
        assert_eq(escape["fate"], "clipped", f"the body cut it: {escape}")
        assert escape["over"]["bottom"] > 0, f"and it went off the bottom: {escape}"
    assert_eq(
        contained["smeared"],
        0,
        "nothing on this screen is painted over its neighbours",
    )
    assert contained["clipped"] >= len(from_body), contained

    # More room, less cut: the report follows the window rather than a constant.
    resized_to(tf, TALL)
    roomier = call(tf, "scene/containment")
    assert roomier["clipped"] <= contained["clipped"], (
        f"a taller window cuts no more than a shorter one: {roomier['clipped']} "
        f"vs {contained['clipped']}"
    )

    # ── D. reachability: a hidden box has no range, so the cut is LOST ───────
    resized_to(tf, WIN)
    reach = call(tf, "scene/scroll_reach")
    for key in ("window", "marks", "scrollable", "lost", "out_of_sight"):
        assert key in reach, f"scene/scroll_reach must report {key}"
    assert_eq(reach["window"]["h"], WIN[1], "the read follows the window")
    assert_eq(
        reach["scrollable"] + reach["lost"],
        len(reach["out_of_sight"]),
        "every out-of-sight mark is one of the two verdicts",
    )
    lost = [o for o in reach["out_of_sight"] if o["reach"] == "lost"]
    assert lost, f"the entries past the body are unreachable: {reach}"
    assert_eq(
        reach["scrollable"],
        0,
        "★ and none of them is 'one scroll away' — nothing on this screen "
        "scrolls, which is exactly what a pinned Scroll used as a clip would "
        "have claimed the opposite of",
    )
    in_body = [o for o in lost if o["viewport"]["name"] == "body"]
    assert in_body, f"the body is named as the box they were judged against: {lost[:2]}"
    for o in in_body:
        v = o["viewport"]
        # ★ The frame the numbers are in. A scroll's content has its own frame
        # with the origin at the top-left, so this is 0 there; a hidden box
        # introduces no frame, so its window starts where the box does — and a
        # client holding `rect` and this viewport could not otherwise say where
        # one sits inside the other.
        for key in ("origin_x", "origin_y"):
            assert key in v, f"a viewport names its frame: {v}"
        assert_eq(v["origin_y"], HEADER_H, "the body starts below the header")
        assert o["rect"]["y"] >= v["origin_y"] + v["h"], (
            f"and the cut mark is past the bottom of that window: {o}"
        )
        assert_eq(v["max_x"], 0, f"a hidden box has no horizontal range: {v}")
        assert_eq(v["max_y"], 0, f"and none vertically: {v}")
        assert_eq(v["fits"], True, "so it reports that it fits, by CSS's own rule")
        assert_eq(v["h"], body_height(WIN), "and its size is the body's")
        assert o["short_by"] is not None, f"a lost mark says how far past it sits: {o}"
        assert o["to_y"] is None, f"and offers no offset, because there is none: {o}"

    # ── E. the pointer agrees with the picture ───────────────────────────────
    #      A cut entry's coordinates belong to whatever is painted there now.
    # A TAGGED one: the untagged rows are the entries' own labels, and a tag is
    # what makes the last assertion say something (an empty string is a
    # substring of every path, so an untagged mark would pass it vacuously).
    entries = [o for o in in_body if (o.get("tag") or "").startswith("overflow_clip#")]
    assert entries, f"the cut entries name themselves: {in_body[:3]}"
    cut = max(entries, key=lambda o: o["rect"]["y"])
    x = cut["rect"]["x"] + max(cut["rect"]["w"] // 2, 1)
    y = cut["rect"]["y"] + max(cut["rect"]["h"] // 2, 1)
    assert y >= body_height(WIN) + HEADER_H, (
        f"the probe point is below the body, or it is not testing the cut: {cut}"
    )
    if y < WIN[1]:
        # `from: "paint"` — the state scene of a view-fn binding has never been
        # through the layout pass, so it answers `OutOfBounds` for every
        # coordinate (R1188's doc-honesty note, R1654's two-scene basis).
        found = call(tf, "scene/locate", {"x": x, "y": y, "from": "paint"})
        assert "path" in found, found
        tag = cut["tag"]
        assert tag not in found["path"], (
            f"the pointer must not reach ink the scene says is not drawn: "
            f"{found['path']} at ({x}, {y}) for {tag}"
        )
        assert "chrome." in found["path"], (
            f"and it lands on the band that IS painted there: {found['path']}"
        )

    print(
        f"  overflow: body {body_height(WIN)}px holds {ROWS}x{ROW_H}px of "
        f"entries; {contained['clipped']} mark(s) cut and {contained['smeared']} "
        f"smeared; {len(lost)} unreachable, {reach['scrollable']} scrollable"
    )


def main() -> None:
    with RpcSubprocess("hello-overflow-clip") as tf:
        run(tf)


if __name__ == "__main__":
    run_demo("hello-overflow-clip", main)
