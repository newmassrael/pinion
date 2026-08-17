#!/usr/bin/env python3
"""R1708 §5.16 §5.41 §2 #7 — **a drag is answered once, and the frame says what
it stood in for**, on all three screens of the analysis tool, in real windows.

# What this exists for

A person reported it while using the tool: grab a window edge, drag, and the
*inside* of the window falls behind the frame. "Is this the quality of the
reference toolkit?"

Measured before this round, driving a real mapped window with real window-system
resizes — not the RPC path, which mixes in a signal write — and counting the
shell's own frame counter:

| resize events | frames painted | catch-up after the LAST event |
|--:|--:|--:|
| 16 | 16 | 340 ms |
| 40 | 41 | 1,119 ms |
| 80 | 81 | 2,693 ms |

Nothing was slow. A resize frame measured *cheaper* than an idle one. There were
simply eighty of them, seventy-nine superseded microseconds after they began,
each blocking the event loop on a full paint of a shape already gone.

Measured on the reference toolkit at 6.11.1, by building a probe and running it
offscreen rather than reading headers, forty resizes queued before its loop
drains them:

* its **window** layer delivers all forty, every intermediate size observable;
* its **widget** layer receives **one**, carrying the final size, and paints
  **once**, in 4.4 ms.

So it does not throttle the facts, it folds the work — and it publishes no count
of the fold anywhere. A consumer there learns it only by instrumenting both
layers and subtracting one from the other.

This tree now folds at the same seam (`pinion_core::resize_batch`, drained in
the shell's `about_to_wait`, which winit reaches only once the pending batch is
exhausted — so R1219's guarantee that a newly-exposed region is filled before
the compositor composites is kept, per batch instead of per event) and
**publishes the fold**: `scene/frame_timings`.`resize` says how many resizes
arrived, how many became frames, how many were superseded, how many merely
repeated a pending size, and whether those four account for every one.

# What it asserts

* **I** — ★★ the REAL path, and the only section that can see the fold's frame
  being load-bearing. Everything else drives `scene/resize`, which also writes
  the windows signal and so arms a redraw of its own; a counterfactual that
  deleted the drain's paint left every other section green because something
  else repainted the window anyway. This one resizes the mapped X window
  directly, through a libX11 helper compiled on the spot.
* **A** — the analyser's published specification is on screen before the drag:
  every pane each screen DECLARES is painted, at the width it declares, tiling
  the body. Read out of the screen's own spec, never written down here.
* **B** — ★ the drag is answered once. A flood of resizes produces far fewer
  frames than events, and the screen's own counter says so.
* **C** — the frame that answers a fold carries the LAST size, not an
  intermediate one, and the painted root really is that size.
* **D** — ★ every event is accounted for: `painted + superseded + repeated +
  pending == events`, published as `balanced` so a reader need not re-derive it.
* **E** — nothing is hidden from a reader. Every resize the window system
  delivered is counted, including the ones no frame answered — the property the
  reference has at its window layer and never publishes.
* **F** — a repeat is not a supersede. Re-announcing the size already pending
  discards nothing, and does not inflate the drag count.
* **A2 / G2** — the general half of A and G, and the one that reaches the
  screen whose specification is not a list of panes: everything the
  specification NAMES and the paint draws at the opening size is still drawn
  after the fold. The population is asserted non-empty, so it cannot become the
  reason nothing was checked.
* **G** — the specification is still on screen AFTER the fold: the same panes,
  at the same declared widths, tiling the same body, at the size the drag
  landed on.
* **H** — ★ PIXELS. The window really is the folded size and really painted
  there — scanned out of a screenshot rather than eyeballed.

Run from the workspace root:
    cargo build -p hello-node-lab -p hello-packet-view -p hello-analyzer-shell --release
    python3 tools/demos/r1708_a_drag_is_answered_once.py
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    Png,
    RpcSubprocess,
    assert_eq,
    abs_rects_of,
    png_pixel,
    read_png_rgba8,
    resize_and_settle,
    run_demo,
    wait_until,
)

EXT = "/external"

#: The three screens of the tool, in the order the specification names them.
SCREENS = [
    ("node lab", "hello-node-lab"),
    ("capture viewer", "hello-packet-view"),
    ("shell", "hello-analyzer-shell"),
]

#: How many resizes a flood sends. Eighty is about one second of a real drag at
#: a normal pointer report rate, and it is the row of the pre-round measurement
#: with the worst catch-up (2.7 s).
FLOOD = 80

#: The widest a fold may be allowed to grow before this stops being a fold. The
#: floor is derived, not chosen: one frame per BATCH is the contract, and a
#: batch boundary is whenever the loop drains, so a flood may legitimately span
#: a few batches. Ten frames for eighty events still means at least an 8x fold,
#: while the pre-round behaviour (81) fails by a mile.
MOST_FRAMES = 10

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def timings(app: RpcSubprocess) -> dict:
    resp = app.request("scene/frame_timings", {})
    assert resp is not None and resp.result is not None
    return resp.result


def resize_tally(app: RpcSubprocess) -> dict:
    t = timings(app)
    assert "resize" in t, "scene/frame_timings publishes the resize account"
    return t["resize"]


def panes_of(app: RpcSubprocess) -> list[dict]:
    """The panes this screen DECLARES, or `[]` if it is not organised in panes.

    ★ Two spellings: one screen declares `spec` as json and another as a string
    holding json. Read both — which a screen chose is not what this is about.
    """
    spec = app.query(f"{EXT}/spec")
    if isinstance(spec, str):
        spec = json.loads(spec)
    panes = spec.get("panes") if isinstance(spec, dict) else None
    return panes or []


def design_size(app: RpcSubprocess) -> tuple[int, int]:
    """This screen's own opening size, ASKED FOR rather than written down."""
    rect = app.snapshot(source="paint")["rect"]
    return (rect["w"], rect["h"])


def named_in_the_spec(spec: object) -> set[str]:
    """Every string anywhere in a published specification.

    The three screens organise their specifications differently — panes and
    columns here, a rail roster and a catalogue there — so the general form
    reads all of it rather than knowing any of it. It is what lets the screen
    whose specification is not a list of panes be checked at all.
    """
    if isinstance(spec, str):
        return {spec}
    if isinstance(spec, dict):
        return set().union(*(named_in_the_spec(v) for v in spec.values())) if spec else set()
    if isinstance(spec, list):
        return set().union(*(named_in_the_spec(v) for v in spec)) if spec else set()
    return set()


def declared_and_painted(app: RpcSubprocess, size: tuple[int, int]) -> set[str]:
    """What this screen's specification NAMES and its paint actually draws."""
    spec = app.query(f"{EXT}/spec")
    if isinstance(spec, str):
        spec = json.loads(spec)
    return named_in_the_spec(spec) & set(abs_rects_of(app.snapshot(source="paint", viewport=size)))


# ── A / G: the published specification is on screen at a given size ─────────


def spec_is_on_screen(app: RpcSubprocess, name: str, size: tuple[int, int], when: str) -> None:
    panes = panes_of(app)
    if not panes:
        print(f"[demo] {when}/{name}: the specification is not organised in panes")
        return
    painted = abs_rects_of(app.snapshot(source="paint", viewport=size))
    missing = [p["tag"] for p in panes if p["tag"] not in painted]
    assert_eq(missing, [], f"{when}/{name} {size}: every declared pane is painted")
    CHECKS.append(f"{when}/{name}: panes painted")
    wrong = [
        f"{p['tag']} declares {p['width']} and is painted {painted[p['tag']][2]}"
        for p in panes
        if p["width"] and painted[p["tag"]][2] != p["width"]
    ]
    assert_eq(wrong, [], f"{when}/{name} {size}: a declared pane width is the width it gets")
    CHECKS.append(f"{when}/{name}: declared widths held")
    # The panes TILE: each begins where the last ended. Three rectangles of the
    # right size that overlap would satisfy the width check and be a broken
    # screen.
    row = sorted((painted[p["tag"]] for p in panes), key=lambda r: r[0])
    gaps = [
        f"{b[0]} does not begin where the pane before it ended ({a[0] + a[2]})"
        for a, b in zip(row, row[1:])
        if a[0] + a[2] != b[0]
    ]
    assert_eq(gaps, [], f"{when}/{name} {size}: the panes tile the body")
    CHECKS.append(f"{when}/{name}: panes tile")
    print(f"[demo] {when}/{name}: {len(panes)} declared pane(s) checked at {size}")


# ── B..F: the drag ──────────────────────────────────────────────────────────


def flood(app: RpcSubprocess, start: tuple[int, int], n: int) -> tuple[int, int]:
    """Fire `n` resizes without waiting for any of them, and return the last.

    Notifications rather than requests, because the whole point is that they
    arrive faster than frames are produced: a caller that waits for each one
    has hand-serialised the very thing being measured, and would measure one
    frame per event no matter what the shell does.
    """
    last = start
    for i in range(n):
        last = (start[0] + i * 3, start[1])
        app.request("scene/resize", {"width": last[0], "height": last[1]}, notify=True)
    return last


def settled_at(app: RpcSubprocess, size: tuple[int, int]) -> None:
    """Wait for the painted root to BE `size` — an outcome, not an interval."""

    def done():
        shot = app.snapshot(source="paint", viewport=size)
        rect = shot.get("rect", {}) if isinstance(shot, dict) else {}
        return shot if (rect.get("w"), rect.get("h")) == size else None

    wait_until(done, timeout=15.0, desc=f"the window settles at {size}")


def drag_is_answered_once(app: RpcSubprocess, name: str) -> tuple[int, int]:
    design = design_size(app)
    before, start_t = resize_tally(app), timings(app)
    base_frames = start_t["frame_count"]

    start = (design[0], design[1])
    last = flood(app, start, FLOOD)
    settled_at(app, last)

    after = resize_tally(app)
    frames = timings(app)["frame_count"] - base_frames
    events = after["events_total"] - before["events_total"]
    painted = after["painted_total"] - before["painted_total"]
    superseded = after["superseded_total"] - before["superseded_total"]
    repeated = after["repeated_total"] - before["repeated_total"]

    # E — nothing is hidden from a reader: every resize that reached the shell
    # was counted, including the ones no frame answered. `events` can fall short
    # of FLOOD only where the window system itself dropped a no-op resize, so
    # this is a floor rather than an equality.
    ok(
        f"B/{name}: the shell counted the flood ({events} of {FLOOD} events)",
        events >= FLOOD - 2,
    )
    # B — far fewer frames than events. The pre-round behaviour was 81.
    ok(
        f"B/{name}: {events} resizes were answered by {frames} frame(s), not {events}",
        frames <= MOST_FRAMES,
    )
    ok(
        f"B/{name}: the screen's own counter says it folded ({superseded} superseded)",
        superseded >= events - MOST_FRAMES,
    )
    # D — the account balances, and it is PUBLISHED as balanced rather than
    # left for a reader to re-derive. ★ This can now FAIL: a fold counts as
    # painted only after the shell reports the frame, so a drain that took the
    # fold and drew nothing leaves the books open. A counterfactual walked
    # straight through the first draft, where the take did the counting.
    ok(f"D/{name}: the published account balances", after["balanced"] is True)
    # ★ And the two independent counters agree: a fold never claims more
    # frames than the window actually drew.
    ok(
        f"D/{name}: {painted} fold(s) claimed of {frames} frame(s) drawn",
        1 <= painted <= frames,
    )
    ok(
        f"D/{name}: every event is in exactly one bucket "
        f"({events} = {painted} painted + {superseded} superseded + "
        f"{repeated} repeated + {after['pending']} pending)",
        events == painted + superseded + repeated + (after["pending"] - before["pending"]),
    )
    # C — the fold landed on the LAST size, not an intermediate one.
    fold = after["last"]
    ok(f"C/{name}: a fold was published", fold is not None)
    ok(
        f"C/{name}: the frame carries the last size, not an intermediate one "
        f"({fold['width']}x{fold['height']} for a flood ending at {last[0]}x{last[1]})",
        (fold["width"], fold["height"]) == last,
    )
    ok(
        f"C/{name}: the fold remembers where the drag began "
        f"({fold['opened_width']}x{fold['opened_height']})",
        fold["opened_width"] <= last[0],
    )
    print(
        f"[demo] B/{name}: {events} events -> {frames} frame(s); "
        f"folded {superseded}, repeated {repeated}, balanced={after['balanced']}"
    )
    return last


def a_repeat_is_not_a_supersede(app: RpcSubprocess, name: str, at: tuple[int, int]) -> None:
    """F — re-announcing the size already on screen discards nothing.

    Distinguishing the two is what keeps an idle window that re-states its size
    from reading like a drag. The reference cannot express the difference: its
    widget layer never hears either one.
    """
    before = resize_tally(app)
    for _ in range(4):
        app.request("scene/resize", {"width": at[0], "height": at[1]}, notify=True)
    settled_at(app, at)
    after = resize_tally(app)
    ok(
        f"F/{name}: resizing to the size already on screen superseded nothing "
        f"({after['superseded_total'] - before['superseded_total']} superseded)",
        after["superseded_total"] == before["superseded_total"],
    )
    ok(f"F/{name}: the account still balances", after["balanced"] is True)


# ── H: pixels ───────────────────────────────────────────────────────────────


def shoot(app: RpcSubprocess, png: Path) -> Png:
    app.request("scene/screenshot", {"path": "", "out_path": str(png)})
    assert png.exists(), "the screenshot was not written"
    return read_png_rgba8(png)


def inked_in_band(img: Png, x0: int, x1: int) -> int:
    """Samples carrying ink in the column band `[x0, x1)`, over the whole height.

    ★ SCANNED rather than looked at. Three rounds running, a screen that drew
    nothing (or drew two visibly different things) passed a by-eye screenshot
    comparison. One row is not enough either — the first draft of this sampled
    the single middle row and the capture viewer answered 1, because that row
    crosses a pane of uniform background. A window is painted or not; the
    question has to be asked of the window.
    """
    inked = 0
    for row in range(4, img.height, 11):
        for col in range(x0, min(x1, img.width), 5):
            r, g, b, _ = png_pixel(img, col, row)
            if abs(r - g) > 6 or abs(g - b) > 6 or r > 60:
                inked += 1
    return inked


def the_pixels_are_the_folded_size(
    app: RpcSubprocess, name: str, size: tuple[int, int], opened_width: int
) -> None:
    with tempfile.TemporaryDirectory() as d:
        img = shoot(app, Path(d) / f"{name.replace(' ', '-')}-folded.png")
    ok(
        f"H/{name}: the real window is the folded size "
        f"({img.width}x{img.height} vs {size[0]}x{size[1]})",
        (img.width, img.height) == size,
    )
    # ★ The sharp claim, not merely "something is on screen": the band of
    # columns that ONLY the widened window has is painted. A fold that kept the
    # old image, or that painted the pre-drag width into a wider surface, leaves
    # this band empty and passes every scene-level check in this file.
    added = inked_in_band(img, opened_width, img.width)
    whole = inked_in_band(img, 0, img.width)
    ok(
        f"H/{name}: the columns the drag ADDED are painted "
        f"({added} inked samples in x=[{opened_width},{img.width}))",
        added > 20,
    )
    ok(f"H/{name}: the window as a whole is painted ({whole} inked samples)", whole > 200)
    print(
        f"[demo] H/{name}: {img.width}x{img.height}, {added} inked in the added "
        f"band, {whole} over the window"
    )


# ── I: the REAL path, the one a person drags ────────────────────────────────
#
# ★★★★★ Everything above drives `scene/resize`, and that RPC also writes the
# windows signal — which arms a redraw of its own. So on that path the drain's
# paint is REDUNDANT: a counterfactual that deleted it left every assertion
# above green, because something else repainted the window anyway. A real drag
# writes no signal, and nothing else repaints. The section below is the only one
# that can see the fold's frame being load-bearing, and it is the path the
# person who reported this was on.
#
# `XResizeWindow` rather than a synthetic pointer drag on the window border:
# what is under test is the resize EVENT stream, not the window manager's
# border hit-testing, and the offscreen display CI uses runs no window manager.

RESIZE_C = r"""
#include <X11/Xlib.h>
#include <stdlib.h>
#include <unistd.h>
/* argv: <window-id> <w0> <h0> <count> <step>
   Resize the window `count` times, growing by `step` each time, flushing every
   request but never waiting: the point is to deliver them faster than frames
   can be produced. */
int main(int argc, char** argv) {
    if (argc < 6) return 2;
    Window w = (Window)strtoul(argv[1], NULL, 0);
    unsigned int w0 = (unsigned int)atoi(argv[2]);
    unsigned int h0 = (unsigned int)atoi(argv[3]);
    int count = atoi(argv[4]);
    int step = atoi(argv[5]);
    Display* d = XOpenDisplay(NULL);
    if (!d) return 3;
    for (int i = 0; i < count; i++) {
        XResizeWindow(d, w, w0 + (unsigned int)(i * step), h0);
    }
    XFlush(d);
    XCloseDisplay(d);
    return 0;
}
"""


def build_resizer(tmp: Path) -> Path | None:
    cc = shutil.which("cc")
    if not (cc and os.environ.get("DISPLAY")):
        return None
    src, exe = tmp / "xresize.c", tmp / "xresize"
    src.write_text(RESIZE_C)
    r = subprocess.run([cc, str(src), "-o", str(exe), "-lX11"],
                       capture_output=True, text=True, timeout=120, check=False)
    if r.returncode != 0:
        print(f"[demo] I: the resize helper did not compile:\n{r.stderr.strip()}")
        return None
    return exe


def x_window_id(app: RpcSubprocess) -> str | None:
    """The mapped X window's id, read with the tooling CI already installs."""
    if not shutil.which("xwininfo"):
        return None
    for _ in range(40):
        r = subprocess.run(["xwininfo", "-root", "-children"],
                           capture_output=True, text=True, check=False)
        ids = [ln.split()[0] for ln in r.stdout.splitlines()
               if ln.strip().startswith("0x") and "pinion" in ln.lower()]
        if ids:
            return ids[-1]
        time.sleep(0.25)
    return None


def a_real_drag_is_answered_once(app: RpcSubprocess, name: str, exe: Path) -> None:
    wid = x_window_id(app)
    if wid is None:
        print(f"[demo] I/{name}: no mapped window found; the real-path section needs one")
        ok(f"I/{name}: a mapped window was found for the real-path section", False)
        return

    before = resize_tally(app)
    base = design_size(app)
    n, step = FLOOD, 3
    subprocess.run([str(exe), wid, str(base[0]), str(base[1]), str(n), str(step)],
                   check=False, timeout=60)
    last = (base[0] + (n - 1) * step, base[1])

    def landed():
        shot = app.snapshot(source="paint", viewport=last)
        rect = shot.get("rect", {}) if isinstance(shot, dict) else {}
        return shot if (rect.get("w"), rect.get("h")) == last else None

    # ★ THE assertion of this section: with nothing else repainting, the window
    # reaches the size the drag ended at only because the drain painted it.
    wait_until(landed, timeout=20.0,
               desc=f"the real drag's last size {last} is what got painted")
    after = resize_tally(app)
    events = after["events_total"] - before["events_total"]
    painted = after["painted_total"] - before["painted_total"]
    superseded = after["superseded_total"] - before["superseded_total"]
    ok(f"I/{name}: real window-system resizes reached the shell ({events})", events >= 2)
    ok(
        f"I/{name}: {events} real resizes were answered by {painted} frame(s)",
        1 <= painted <= MOST_FRAMES,
    )
    ok(f"I/{name}: the real drag folded ({superseded} superseded)", superseded >= 1)
    ok(f"I/{name}: the published account balances after a real drag",
       after["balanced"] is True)
    fold = after["last"]
    ok(
        f"I/{name}: the frame carries the size the real drag ended at "
        f"({fold['width']}x{fold['height']} vs {last[0]}x{last[1]})",
        (fold["width"], fold["height"]) == last,
    )
    print(f"[demo] I/{name}: {events} real events -> {painted} frame(s), "
          f"{superseded} folded, landed {last}")


# ── the round ───────────────────────────────────────────────────────────────


def drive(name: str, example: str, resizer: Path | None) -> None:
    banner(f"{name} ({example})")
    # ★★ MAPPED, not the harness's default hidden window, and the reason is a
    # defect this round measured rather than a preference.
    #
    # Resizing an UNMAPPED window leaves the GPU surface configured for a size
    # the X drawable never took, so every subsequent swapchain acquire returns
    # `Outdated` — for ever. Measured on the stashed pre-round binary, so it is
    # not this round's doing: one resize takes `present_ok` from `true` to
    # `false` and three ordinary ticks afterwards are `false` too, while
    # `scene/screenshot` answers `RenderBackendUnavailable` on every attempt
    # including the third. The same drive against a MAPPED window on the same
    # host presents fine, twice, with zero warnings.
    #
    # Nothing had seen it because no demo in the tree both resizes and looks at
    # pixels — the paint snapshot reads the ENCODED scene, which is built before
    # the acquire, so every introspection surface reports a healthy window while
    # nothing reaches the screen. Registered as its own debt; this demo maps the
    # window so that section H is a real pixel check rather than a check of a
    # path that cannot work.
    with RpcSubprocess(example, visible_window=True) as app:
        design = design_size(app)
        # ★ The real path FIRST, while the window is still at its opening size:
        # it is the section the whole round is about, and running it after the
        # RPC flood would measure a window someone already moved.
        if resizer is not None:
            a_real_drag_is_answered_once(app, name, resizer)
        resize_and_settle(app, design)
        spec_is_on_screen(app, name, design, "A")
        # ★ The general half of A, and the one that reaches the screen whose
        # specification is not a list of panes. The population is the
        # intersection of "named in the specification" with "painted at the
        # opening size" — self-calibrating across three differently-shaped
        # specifications, and asserted non-empty so it cannot become the reason
        # nothing was checked.
        declared = declared_and_painted(app, design)
        ok(
            f"A2/{name}: the specification names things that are on screen "
            f"({len(declared)})",
            len(declared) >= 8,
        )
        landed = drag_is_answered_once(app, name)
        spec_is_on_screen(app, name, landed, "G")
        # ★★ And the whole of it survives the FOLD. This is the section that
        # would fail if a drag's one answering frame painted a stale tree, or
        # if the frame that answered the fold were laid out for a superseded
        # size: a declared region that is on screen at the opening size and
        # gone after the drag.
        gone = sorted(declared - declared_and_painted(app, landed))
        assert_eq(
            gone,
            [],
            f"G2/{name}: everything the specification names is still painted "
            f"after the fold, at {landed}",
        )
        CHECKS.append(f"G2/{name}: {len(declared)} declared regions survive the fold")
        a_repeat_is_not_a_supersede(app, name, landed)
        the_pixels_are_the_folded_size(app, name, landed, design[0])
        # And the screen still answers at the size the drag landed on, through
        # the ordinary path a later resize takes.
        resize_and_settle(app, (landed[0] - 40, landed[1] - 20))
        ok(f"G/{name}: the screen still resizes normally after a fold", True)


def main() -> None:
    with tempfile.TemporaryDirectory() as d:
        resizer = build_resizer(Path(d))
        if resizer is None:
            # Loud, and not silent-vacuous: this section is the only one that
            # drives the path the person reported, so its absence has to be
            # visible in the round's record rather than read as a pass.
            print("[demo] ★ I: NO REAL-PATH SECTION — needs cc + libX11 + DISPLAY")
        for name, example in SCREENS:
            drive(name, example, resizer)
    print(f"\n{len(CHECKS)} assertions across {len(SCREENS)} screens")
    assert len(CHECKS) >= 40, f"only {len(CHECKS)} assertions"
    assert resizer is not None, "the real-path section did not run"


if __name__ == "__main__":
    run_demo("hello-analyzer-shell", main)
