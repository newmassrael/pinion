#!/usr/bin/env python3
"""R1710 §5.16 §5.12 §2 #2 §2 #3 §2 #7 — **a resize says what it granted**, on
all three screens of the analysis tool.

# What this exists for

Every one of these screens declares a floor for its own window. Nothing enforced
it and nothing reported it, so `scene/resize` answered with the size it was
handed regardless of what the window then did. Measured on this host before the
repair, driving the dashboard (floor 1440x900) over this very method:

| display | asked | window afterwards | what the wire answered |
|---|---|---|---|
| bare offscreen server (all of CI) | 1560x880 | **1560x880** | `height: 880` |
| real desktop, window manager      | 1560x880 | **1560x900** | `height: 880` |

Two defects, and they hide each other. On the display every gate in this tree
runs on, the declared floor is enforced by NOTHING — so no test could fail. On a
display with a window manager the floor is enforced by the MANAGER, and the wire
then says a number the window does not have: ask for 880, be told 880, read a
scene that is 900, and have no way to ask why. That is the §2 #2 path an AI agent
drives.

The repair resolves the ask against the declared bounds inside the framework, so
both displays now answer the same, and publishes the resolution per axis.

# What it asserts

Nothing here writes a screen's floor down. Section **A** MEASURES it, through
the dry run, and every later section is computed from what it found — so the
same file drives three screens whose floors genuinely differ (one screen's floor
is not its design size, which a demo that assumed they were equal would have
mistaken for a pass).

* **A** — the floor is readable in one call: a dry-run ask of `1x1` comes back
  with the whole floor, both axes reporting `floor` at the extent they hold.
  Asserted non-degenerate (a floor of `1x1` would make every later section
  vacuous).
* **B** — ★★ **the wire agrees with the window.** For five asks per screen
  — above the floor, below in width, below in height, below in both, exactly at
  the floor — the granted size the response publishes is the size the frame is
  actually painted at. This is the assertion whose absence let the defect live:
  no caller anywhere compared the two.
* **C** — the report names the bound, per axis: `floor` at exactly the declared
  extent for an axis that was raised, `as_asked` for one that was not, and
  `as_asked` for BOTH when the ask was legal. Both directions, because a report
  that always says `floor` is as useless as one that never does.
* **D** — §2 #3: a dry run changes nothing (the painted rectangle is untouched)
  and answers exactly what the real call then answers, differing in `applied`
  and in nothing else.
* **E** — a malformed ask is still refused BY NAME (`InvalidSize`) rather than
  rescued by the floor: zero is a typo, not a small window.
* **F** — the specification survives a clamped resize: every region each screen
  DECLARES and paints at its design size is still painted after a resize that
  was raised to the floor. Read out of each screen's own spec, never listed here.
* **G** — ★ pixels. At the clamped size the live surface hands back an image of
  the GRANTED size, with ink in it — scanned, not glanced at. Without this,
  "granted" could be bookkeeping that never reached a screen.

★ Every section is falsifiable on both displays: the framework now owns the
resolution, so removing it fails here on the bare offscreen server too — which
is the whole point, because that is the only display CI has.
"""

from __future__ import annotations

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
    RpcError,
    RpcSubprocess,
    assert_declared_panes_on_screen,
    assert_eq,
    declared_and_painted,
    design_size,
    png_pixel,
    read_png_rgba8,
    resize_and_settle,
    run_demo,
    wait_until,
)

#: The three screens of the tool, in the order the specification names them.
SCREENS = [
    ("node lab", "hello-node-lab"),
    ("capture viewer", "hello-packet-view"),
    ("dashboard", "hello-analyzer-shell"),
]

CHECKS: list[str] = []


def ok(what: str, condition: bool) -> None:
    assert condition, f"FAILED: {what}"
    CHECKS.append(what)


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


# ── the two reads every section is built out of ─────────────────────────────


def outcome_of(resp: object, size: tuple[int, int]) -> dict:
    result = getattr(resp, "result", None)
    assert isinstance(result, dict), f"scene/resize {size} answers an object; got {resp!r}"
    return result


def resolve(app: RpcSubprocess, size: tuple[int, int]) -> dict:
    """What the window WOULD grant, without touching it (§2 #3).

    ★ The params are a literal here rather than built above and passed in,
    because the harness's own gate reads `dry_run` off the call to know this
    send is not a resize — an exempting fact belongs where a reader sees it.
    """
    return outcome_of(
        app.request(
            "scene/resize", {"width": size[0], "height": size[1], "dry_run": True}
        ),
        size,
    )


def granted(outcome: dict) -> tuple[int, int]:
    return (outcome["width"], outcome["height"])


def painted_size(app: RpcSubprocess, expect: tuple[int, int]) -> tuple[int, int]:
    """The rectangle of the frame that is actually on the window."""
    shot = app.snapshot(source="paint", viewport=expect)
    rect = shot.get("rect", {}) if isinstance(shot, dict) else {}
    return (rect.get("w"), rect.get("h"))


# ── A: the floor, measured rather than written down ─────────────────────────


def the_floor_is_readable(app: RpcSubprocess, name: str) -> tuple[int, int]:
    """Ask for a window one pixel across, without acting, and read the floor.

    ★ This is the reference toolkit's `minimumSize()` in one call, and it comes
    back through the SAME resolution the real path runs — so a client cannot
    learn a floor that the real call would then not honour.
    """
    outcome = resolve(app, (1, 1))
    floor = granted(outcome)
    ok(
        f"A/{name}: a dry-run ask below everything reports the floor ({floor[0]}x{floor[1]})",
        floor[0] > 1 and floor[1] > 1,
    )
    ok(
        f"A/{name}: both axes name the bound that raised them",
        outcome["width_bound"] == {"kind": "floor", "at": floor[0]}
        and outcome["height_bound"] == {"kind": "floor", "at": floor[1]},
    )
    ok(f"A/{name}: a raised ask is not `as_asked`", outcome["as_asked"] is False)
    ok(f"A/{name}: a dry run is not applied", outcome["applied"] is False)
    print(f"[demo] A/{name}: floor {floor[0]}x{floor[1]}, design {design_size(app)}")
    return floor


# ── B / C: the wire agrees with the window, and names the bound ─────────────


def the_wire_agrees_with_the_window(
    app: RpcSubprocess, name: str, floor: tuple[int, int]
) -> None:
    """Five asks per screen, each one checked against the painted frame.

    The asks are DERIVED from the measured floor, so a screen whose floor is
    not its design size is driven correctly rather than accidentally.
    """
    cases = [
        ("above both axes", (floor[0] + 120, floor[1] + 60), (False, False)),
        ("below in width", (floor[0] - 100, floor[1] + 60), (True, False)),
        ("below in height", (floor[0] + 120, floor[1] - 20), (False, True)),
        ("below in both", (floor[0] - 100, floor[1] - 20), (True, True)),
        ("exactly the floor", (floor[0], floor[1]), (False, False)),
    ]
    for label, size, (w_raised, h_raised) in cases:
        # The REAL send, and this function waits for it below — the harness's
        # own gate requires the two to live together, because a resize whose
        # wait is in a sibling is a bet on the render arriving (R1686).
        outcome = outcome_of(
            app.request("scene/resize", {"width": size[0], "height": size[1]}), size
        )
        want = (max(size[0], floor[0]), max(size[1], floor[1]))
        got = granted(outcome)
        ok(
            f"B/{name}/{label}: the grant resolves {size} to {want} (said {got})",
            got == want,
        )
        # ★ THE assertion of this file: the published grant is the frame's own
        # size. The wait is on the GRANTED pair, so a framework that forwarded
        # the raw ask would hang here rather than pass — and the wait is on an
        # OUTCOME, never on an elapsed interval.
        wait_until(
            lambda want=want: painted_size(app, want) == want,
            timeout=8.0,
            desc=f"the window settles at the granted {want}",
        )
        on_screen = painted_size(app, want)
        ok(
            f"B/{name}/{label}: the window is painted at the granted size "
            f"({on_screen[0]}x{on_screen[1]})",
            on_screen == want,
        )
        # C — the reason, per axis, in both directions.
        ok(
            f"C/{name}/{label}: the width bound is {'floor' if w_raised else 'as_asked'}",
            outcome["width_bound"]
            == ({"kind": "floor", "at": floor[0]} if w_raised else {"kind": "as_asked"}),
        )
        ok(
            f"C/{name}/{label}: the height bound is {'floor' if h_raised else 'as_asked'}",
            outcome["height_bound"]
            == ({"kind": "floor", "at": floor[1]} if h_raised else {"kind": "as_asked"}),
        )
        ok(
            f"C/{name}/{label}: `as_asked` agrees with the two bounds",
            outcome["as_asked"] == (not w_raised and not h_raised),
        )
        ok(f"C/{name}/{label}: the ask is echoed unaltered", tuple(outcome["asked"]) == size)


# ── D: the dry run is the same answer without the act ───────────────────────


def a_dry_run_changes_nothing(app: RpcSubprocess, name: str, floor: tuple[int, int]) -> None:
    settled = (floor[0] + 200, floor[1] + 40)
    resize_and_settle(app, settled)
    before = painted_size(app, settled)

    probe = (floor[0] - 60, floor[1] - 30)
    dry = resolve(app, probe)
    after = painted_size(app, settled)
    ok(
        f"D/{name}: a dry run leaves the window where it was "
        f"({before[0]}x{before[1]} -> {after[0]}x{after[1]})",
        after == before,
    )
    ok(f"D/{name}: a dry run reports itself unapplied", dry["applied"] is False)

    real = outcome_of(
        app.request("scene/resize", {"width": probe[0], "height": probe[1]}), probe
    )
    ok(
        f"D/{name}: the real call answers what the dry run promised",
        {k: v for k, v in real.items() if k != "applied"}
        == {k: v for k, v in dry.items() if k != "applied"},
    )
    ok(f"D/{name}: the real call reports itself applied", real["applied"] is True)


# ── E: a malformed ask is named, not rescued ────────────────────────────────


def a_zero_ask_is_refused_by_name(app: RpcSubprocess, name: str) -> None:
    blob = ""
    refused = False
    try:
        app.request("scene/resize", {"width": 0, "height": 400})
    except RpcError as exc:
        refused = True
        blob = f"{exc.message} {exc.data!r}"
    ok(
        f"E/{name}: a zero extent is refused by name, not raised to the floor ({blob})",
        refused and "InvalidSize" in blob,
    )


# ── F / G: the analyser's own specification, and the pixels ─────────────────


def the_specification_survives_a_clamped_resize(
    app: RpcSubprocess, name: str, floor: tuple[int, int]
) -> None:
    design = design_size(app)
    resize_and_settle(app, design)
    made = assert_declared_panes_on_screen(app, design, label=f"F/{name}")
    if made:
        CHECKS.append(f"F/{name}: {made} declared pane(s) tile the body at the design size")
    declared = declared_and_painted(app, design)
    ok(
        f"F/{name}: the specification names things that are on screen ({len(declared)})",
        len(declared) >= 8,
    )
    # A resize the floor RAISES — the case that used to leave the window at a
    # size the caller was never told about.
    probe = (floor[0] - 80, floor[1] - 40)
    landed = granted(resolve(app, probe))
    resize_and_settle(app, probe)
    gone = sorted(declared - declared_and_painted(app, landed))
    assert_eq(
        gone,
        [],
        f"F/{name}: everything the specification names is still painted at the "
        f"granted {landed}",
    )
    CHECKS.append(f"F/{name}: {len(declared)} declared regions survive a clamped resize")


def the_granted_size_is_what_is_on_the_screen(
    app: RpcSubprocess, name: str, floor: tuple[int, int], png: Path
) -> None:
    """★ The grant reached a surface, not just a response envelope."""
    probe = (floor[0] - 100, floor[1] - 30)
    resize_and_settle(app, probe)
    app.request("scene/screenshot", {"path": "", "out_path": str(png)})
    ok(f"G/{name}: the live surface answered after a clamped resize", png.exists())
    img = read_png_rgba8(png)
    ok(
        f"G/{name}: the image is the GRANTED size ({img.width}x{img.height} vs "
        f"{floor[0]}x{floor[1]})",
        (img.width, img.height) == floor,
    )
    inked = inked_samples(img)
    ok(f"G/{name}: the clamped window is painted ({inked} inked samples)", inked > 200)
    print(f"[demo] G/{name}: {img.width}x{img.height}, {inked} inked samples")


# ── H: the floor the wire reports is the floor the shell DECLARED ───────────
#
# ★★★★★ This section exists because a counterfactual PASSED. Every section
# above derives its expectations from the floor section A measured, so a shell
# that stamped the WRONG floor — measured: `initial_logical_size()` instead of
# `min_inner_floor()`, which differ on one of these three screens — was
# self-consistently green everywhere. A gate that asks the system under test
# what to expect cannot catch the system under test being wrong.
#
# So this reads the SECOND channel: the minimum the shell declared to the
# window system at create (`WM_NORMAL_HINTS`), which no part of the RPC path
# touches. The two are the same fact by construction — that is the claim
# `SizeStrategy::window_bounds` makes in its own doc — and this is what checks
# it. Works in the harness's default hidden mode: an unmapped window still
# carries its hints (measured on both a bare server and a managed one).

HINTS_C = r"""
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <stdio.h>
#include <stdlib.h>
/* argv: <window-id>. Prints `min <w> <h>` when the window declares a minimum. */
int main(int argc, char** argv) {
    if (argc < 2) return 2;
    Window w = (Window)strtoul(argv[1], NULL, 0);
    Display* d = XOpenDisplay(NULL);
    if (!d) return 3;
    XSizeHints* h = XAllocSizeHints();
    long supplied = 0;
    int rc = 4;
    if (XGetWMNormalHints(d, w, h, &supplied) && (h->flags & PMinSize)) {
        printf("min %d %d\n", h->min_width, h->min_height);
        rc = 0;
    }
    XFree(h);
    XCloseDisplay(d);
    return rc;
}
"""

#: Screens the second channel actually ran on, printed by `main` so a display
#: that cannot supply it reads as that rather than as a pass.
SECOND_CHANNEL: list[str] = []


def build_hint_reader(tmp: Path) -> Path | None:
    cc = shutil.which("cc")
    if not (cc and os.environ.get("DISPLAY")):
        return None
    src, exe = tmp / "hints.c", tmp / "hints"
    src.write_text(HINTS_C)
    built = subprocess.run([cc, str(src), "-o", str(exe), "-lX11"],
                           capture_output=True, text=True, timeout=120, check=False)
    if built.returncode != 0:
        print(f"[demo] H: the hint reader did not compile:\n{built.stderr.strip()}")
        return None
    return exe


def x_window_id(example: str) -> str | None:
    """The application's own window, by CLASS — under a window manager the
    direct child of root is the manager's frame, which carries the app's title
    (R1709 paid for that once)."""
    if not shutil.which("xwininfo"):
        return None
    for _ in range(40):
        r = subprocess.run(["xwininfo", "-root", "-tree"],
                           capture_output=True, text=True, check=False)
        rows = [ln.strip() for ln in r.stdout.splitlines() if ln.strip().startswith("0x")]
        mine = [ln.split()[0] for ln in rows if f'("{example}"' in ln]
        if mine:
            return mine[-1]
        time.sleep(0.25)
    return None


def the_declared_minimum_is_the_enforced_floor(
    name: str, example: str, floor: tuple[int, int], reader: Path
) -> None:
    wid = x_window_id(example)
    if wid is None:
        print(f"[demo] ★ H/{name}: no window found in the tree; the second channel cannot run")
        return
    out = subprocess.run([str(reader), wid], capture_output=True, text=True,
                         timeout=30, check=False)
    if out.returncode != 0 or not out.stdout.startswith("min "):
        print(f"[demo] ★ H/{name}: the window declares no minimum to the window system")
        return
    parts = out.stdout.split()
    declared = (int(parts[1]), int(parts[2]))
    SECOND_CHANNEL.append(name)
    ok(
        f"H/{name}: the floor the wire reports is the minimum the shell declared "
        f"to the window system ({floor[0]}x{floor[1]} vs {declared[0]}x{declared[1]})",
        declared == floor,
    )
    print(f"[demo] H/{name}: declared minimum {declared[0]}x{declared[1]}")


def inked_samples(img: Png) -> int:
    """Samples carrying ink over the whole surface — scanned, not glanced at."""
    inked = 0
    for row in range(4, img.height, 11):
        for col in range(4, img.width, 7):
            r, g, b, _ = png_pixel(img, col, row)
            if abs(r - g) > 6 or abs(g - b) > 6 or r > 60:
                inked += 1
    return inked


# ── the round ───────────────────────────────────────────────────────────────


def drive(name: str, example: str, tmp: Path, reader: Path | None) -> None:
    banner(f"{name} ({example})")
    with RpcSubprocess(example) as app:
        floor = the_floor_is_readable(app, name)
        # ★ Before anything is resized, while the window still carries the
        # geometry it was created with.
        if reader is not None:
            the_declared_minimum_is_the_enforced_floor(name, example, floor, reader)
        the_wire_agrees_with_the_window(app, name, floor)
        a_dry_run_changes_nothing(app, name, floor)
        a_zero_ask_is_refused_by_name(app, name)
        the_specification_survives_a_clamped_resize(app, name, floor)
        the_granted_size_is_what_is_on_the_screen(app, name, floor, tmp / f"{example}.png")


def main() -> None:
    with tempfile.TemporaryDirectory() as d:
        reader = build_hint_reader(Path(d))
        if reader is None:
            print("[demo] ★ H: NO SECOND CHANNEL — needs cc + libX11 + DISPLAY")
        for name, example in SCREENS:
            drive(name, example, Path(d), reader)
    print(f"\n{len(CHECKS)} assertions across {len(SCREENS)} screens")
    # R1710 — say every run how many screens the independent channel reached. A
    # section a display prevented from running must not read as one that passed.
    print(
        f"[demo] H: the declared minimum was read back on "
        f"{len(SECOND_CHANNEL)}/{len(SCREENS)} screen(s)"
    )
    assert len(CHECKS) >= 40, f"only {len(CHECKS)} assertions"


if __name__ == "__main__":
    run_demo("hello-analyzer-shell", main)
