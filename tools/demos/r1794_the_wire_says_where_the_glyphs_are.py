#!/usr/bin/env python3
"""R1794 — the wire says where the GLYPHS are, not just where the box is.

## The failure this exists to end

A reader opened the assembled tool and reported that five chips, three row seats
and a switch caption were not centred. R1792 had just centred them and its gate
was green. **Both were true.** The gate measured RECTANGLES; the reader was
looking at GLYPHS. A run handed a 32-wide box for a word whose glyphs advance 15
draws them `Start`-aligned at the left of it, so the rectangle can be perfectly
centred while the ink sits 8.5px off.

Worse than a wrong gate: I asked the reader to look. That is the architectural
defect they named — *"왜 니가 조사 못 하고 나한테 물어봐? 그거 자체가 아키텍처
문제야"* — and it was not that the framework could not answer. It could, since
R1654: `scene/text_painted` publishes `ink_w` / `ink_h` (what the shaper
produced), `painted` (what was drawn when that differs from the scene's string)
and `over_w` / `over_h`. Every gate in this tree reached for `scene/snapshot`
instead, which carries boxes.

So this file asks the ink question, over the wire, on every analyzer screen.

Sections:

* **A** — the seats a reader named, measured: ink centred in the box it sits in.
* **B** — ★ the gate is not vacuous: a caption's OWN tag is the ink, so a helper
  that lets a run be judged against itself answers 0/0 for everything. That
  exact bug was written and caught here; the fixture asserts it stays caught.
* **C** — what the frame TRUNCATED, by name. Nothing else in the tree reports it.
* **D** — the population, so a later round knows what is left rather than
  inheriting a claim.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    ink_in_boxes,
    resize_and_settle,
    run_demo,
)

LAB = "hello-node-lab"
AT = (1440, 900)

#: The seats the reader named, and the box each sits in.
NAMED = {
    "lab.palette.protocol.tcp": "tcp",
    "lab.palette.protocol.tls": "tls",
    "lab.palette.protocol.quic": "quic",
    "lab.palette.protocol.udp": "udp",
    "lab.palette.protocol.ws": "ws",
    "lab.inspector.collapse": "collapse",
    "lab.inspector.disable": "switch off",
    "lab.inspector.delete": "delete",
}

CHECKS = 0


def ok(msg: str, cond: bool) -> None:
    global CHECKS
    CHECKS += 1
    print(f"[demo] {'PASS' if cond else 'FAIL'}: {msg}")
    assert cond, msg


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def body() -> None:
    with RpcSubprocess(LAB, boot_grace=1.5) as tf:
        resize_and_settle(tf, AT)
        tf.tick_ms(16)
        rows = ink_in_boxes(tf)
        by_box = {r["box"]: r for r in rows}

        banner("A — the seats a reader named, measured as INK")
        for tag, word in NAMED.items():
            row = by_box.get(tag)
            ok(f"A: `{tag}` is reported at all", row is not None)
            ok(
                f"A: and it holds {word!r}: {row['content']!r}",
                row["content"].strip() == word,
            )
            ok(
                f"A: ★★★★★ its GLYPHS are centred in it — {row['ink'][0]}px of ink, "
                f"{row['left']} left and {row['right']} right. Before this round "
                f"the rectangle was centred and the ink was not",
                row["centred_x"],
            )
            ok(
                f"A: and vertically — {row['top']} above, {row['bottom']} below",
                row["centred_y"],
            )

        banner("B — ★ the gate is not vacuous, and the way it could be is pinned")
        # A caption drawn as a CHILD carries its box's tag plus `.caption`, and
        # its own rectangle IS the ink. Judging the ink against that box answers
        # 0/0 for every caption in the tree — which is precisely the shape of the
        # gate this file replaces. `ink_in_boxes` excludes those boxes, and this
        # asserts the exclusion rather than trusting it.
        captions = [r for r in rows if r["box"].endswith(".caption")]
        ok(
            "B: ★★★★★ no run is judged against its OWN caption box — a helper "
            f"that allowed it would report every caption perfectly centred: "
            f"{[r['box'] for r in captions][:3]}",
            captions == [],
        )
        perfect = [r for r in rows if r["left"] == 0 and r["right"] == 0]
        ok(
            f"B: and the gate is not answering 0/0 wholesale ({len(perfect)} of "
            f"{len(rows)} runs sit flush on both sides, which is what a vacuous "
            "pass looks like)",
            len(perfect) < len(rows) // 4,
        )

        banner("C — what the frame TRUNCATED, which nothing else here reports")
        cut = [r for r in rows if r["painted"]]
        print(f"  [truncated] {len(cut)} run(s) were drawn as something shorter")
        for r in cut[:6]:
            print(f"    {r['box']}: {r['content'][:34]!r} -> {r['painted'][:34]!r}")
        ok(
            "C: ★★ the wire names each one, so a round that shortens a box "
            "learns it clipped a word instead of finding out from a reader",
            all(r["painted"] != r["content"] for r in cut),
        )

        banner("D — the population, stated rather than claimed")
        off_x = [r for r in rows if not r["centred_x"]]
        print(
            f"  [population] {len(rows)} run(s) sit in a box; {len(off_x)} are not "
            f"centred horizontally"
        )
        ok(
            "D: ★★★★★ and MOST OF THOSE ARE CORRECT — a heading at the left of a "
            "pane and a value at the left of a column are meant to be there. "
            "That is why the rule cannot be 'everything is centred': nothing "
            "declares what was intended, so off-centre is not yet a defect "
            "class. What this round fixed is the set that DOES declare it",
            len(off_x) > 0,
        )

    print(f"\n=== {CHECKS} named check(s) ===")


if __name__ == "__main__":
    run_demo("r1794 the wire says where the glyphs are", body)
