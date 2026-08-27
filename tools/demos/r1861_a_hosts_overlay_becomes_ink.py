#!/usr/bin/env python3
"""R1861 §5.32 §5.12 §2 #7 — **the host's overlay is ink a reader can read.**

A reader ran the assembled tool at its shipping size, pressed the rail to reach
the node lab, and reported: *the toast floats over the hint strip and I cannot
read the toast's letters*. Every gate in this tree was green, and this script is
why: **they all ask the SCENE.** Measured in process before a line of this was
written, at 1440x900 with the lab showing, the toast is the 420th of 426 tagged
marks — so nothing is painted after it — its box is opaque, its run sits inside
that box, and its ink-on-surface contrast is 13.79. Every scene question about
that toast answers *fine*.

What no gate in this tree asks is whether a mark that is in the scene **became
ink in the frame**. That is the only question left between a green gate and a
reader who cannot read something, and it is the question this script puts.

Run from the workspace root:
    cargo build -p hello-analyzer-shell --release
    python3 tools/demos/r1861_a_hosts_overlay_becomes_ink.py
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
)

SHELL = "hello-analyzer-shell"
EXT = "/external"
# The words the reader used. Found by them rather than by any tag or constant of
# the mounted screen, which is what the reader had.
NAMED = "drag a pin = author a link"
CHECKS: list[str] = []


def meets(box: tuple[int, int, int, int], run: dict) -> bool:
    """Whether a window-coordinate box shares a pixel with a painted run."""
    x, y, w, h = box
    return not (
        x >= run["x"] + run["w"]
        or run["x"] >= x + w
        or y >= run["y"] + run["h"]
        or run["y"] >= y + h
    )


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def note(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def runs_of(app: RpcSubprocess) -> list[dict]:
    """Every text run the frame painted, with the box each one sits in."""
    answer = app.request("scene/text_painted")
    assert answer is not None and isinstance(answer.result, dict)
    return answer.result["runs"]


def ink_in(img: Png, rect: tuple[int, int, int, int]) -> tuple[int, int]:
    """(samples carrying ink, samples taken) inside a window-coordinate box.

    Scanned rather than glanced at, and every pixel rather than a lattice: a
    sentence is thin, and a stride can walk between its strokes.
    """
    x, y, w, h = rect
    inked = 0
    taken = 0
    seen: dict[tuple[int, int, int], int] = {}
    for row in range(max(y, 0), min(y + h, img.height)):
        for col in range(max(x, 0), min(x + w, img.width)):
            r, g, b, _ = png_pixel(img, col, row)
            seen[(r, g, b)] = seen.get((r, g, b), 0) + 1
            taken += 1
    if not taken:
        return (0, 0)
    # The box's own fill is whatever colour most of it is. Ink is everything
    # that is not that — derived from the frame rather than named here, so a
    # theme change does not turn this into a check about a constant.
    ground = max(seen.items(), key=lambda kv: kv[1])[0]
    for (r, g, b), n in seen.items():
        if abs(r - ground[0]) + abs(g - ground[1]) + abs(b - ground[2]) > 24:
            inked += n
    return (inked, taken)


def body() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        drive(Path(tmp))
    print(f"\n[demo] {len(CHECKS)} named check(s)")


def drive(tmp: Path) -> None:
    banner("the assembled tool at the node lab, with the host's toast alive")
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        app.intervene(f"{EXT}/nav", "lab")
        app.tick_ms(16)
        assert_eq(app.query(f"{EXT}/nav"), "lab", "the walk reached the node lab")

        rects = abs_rects_of(app.snapshot(source="paint"))
        note("the host paints a toast", "shell.toast" in rects)
        toast = rects["shell.toast"]
        print(f"[demo] shell.toast box {toast}")

        said = [r for r in runs_of(app) if r.get("owner") == "shell.toast"]
        assert_eq(len(said), 1, "the toast paints exactly one sentence")
        run = said[0]
        print(f"[demo] the toast says {run['content']!r} at {run}")

        png = tmp / "lab-with-toast.png"
        app.request("scene/screenshot", {"path": "", "out_path": str(png)})
        assert png.exists(), "no screenshot was written"
        img = read_png_rgba8(png)
        print(f"[demo] photograph {img.width}x{img.height}")

        # ── A: the overlay's own sentence is ink ───────────────────────────
        #
        # ★ The half of the report that turned out to be FALSE, and it is kept
        # because a refutation nobody can re-run is a claim.
        run_box = (run["x"], run["y"], run["w"], run["h"])
        inked, taken = ink_in(img, run_box)
        print(f"[demo] A: the toast's own sentence is {inked} of {taken} samples of ink")
        note(
            f"A: the toast's sentence became ink ({inked} of {taken})",
            taken > 0 and inked * 10 > taken,
        )

        # ── B: and it is not on top of the guest's ─────────────────────────
        #
        # ★★★★★ The half that was TRUE, measured before the repair at 6 pixels
        # of a 13-pixel run. Asked of the frame the reader saw rather than of
        # the scene: a scene answers where marks are, and a reader loses letters.
        covered = [
            r
            for r in runs_of(app)
            if r.get("owner") != "shell.toast" and meets(toast, r)
        ]
        print(f"[demo] B: sentences under the overlay: {[r['content'][:40] for r in covered]}")
        assert_eq(
            [r["content"] for r in covered],
            [],
            "B: the host's overlay is painted over the screen's own words",
        )
        note("B: nothing the screen says is under the host's overlay", True)

        # ── C: and the sentence the reader named is whole in the photograph ──
        named = next(
            (r for r in runs_of(app) if NAMED in r["content"]),
            None,
        )
        assert named is not None, f"no run says {NAMED!r}"
        named_box = (named["x"], named["y"], named["w"], named["h"])
        inked, taken = ink_in(img, named_box)
        print(f"[demo] C: {NAMED!r} is {inked} of {taken} samples of ink")
        note(
            f"C: the sentence the reader named became ink ({inked} of {taken})",
            taken > 0 and inked * 20 > taken,
        )


run_demo("R1861 a host's overlay becomes ink", body)
