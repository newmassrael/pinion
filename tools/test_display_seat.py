#!/usr/bin/env python3
"""R2220 — the seat rule, and the probe that feeds it, both tested.

Two gates, not one. `tools/display_seat.py` is a pure rule sitting behind a
parse of three programs' output, and this project has paid for testing only the
memorable half: R1884 recorded that a pure-rules oracle is *a second gate nobody
tests*. So the cases below come in two kinds.

**The rule**, against `Probe` records built by hand — every combination of the
two signs, plus the three not-proven-offscreen kinds.

**The parse**, against text CAPTURED FROM REAL SERVERS on 2026-09-13:

* `:1`   — the gdm Xorg the owner is looking at (2560x1600, `DP-4` connected);
* `:98`  — an `Xvfb :98 -screen 0 1920x1200x24` left running by an earlier
           session;
* `:99`  — a throwaway `Xvfb :99 -screen 0 1024x768x24` started for the
           measurement and reaped after it.

⚠ The fixtures are real because a fixture written to match the parse tests the
author's idea of the output rather than the output
([[r1845-two-faults-that-always-travel-together]] is the general shape: when the
fault and its fixture always travel together, a green means nothing).

And **the oracle runs**: the last case probes whatever displays this machine
actually has and asserts the verdict is self-consistent with its own evidence.
It asserts nothing about WHICH displays exist, so it is silent on a runner that
has none and still falsifiable wherever one does.

    python3 tools/test_display_seat.py
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import display_seat  # noqa: E402
from display_seat import (  # noqa: E402
    OFFSCREEN,
    SEATED,
    UNPROBED,
    UNREACHABLE,
    Output,
    Probe,
    SeatedDisplay,
    judge,
    parse_extensions,
    parse_outputs,
    refuse_seated,
    seat_signs,
)

PASSED = 0
FAILED: list[str] = []
SKIPPED: list[str] = []
CURRENT = ""


def check(condition: bool, label: str) -> None:
    global PASSED
    if condition:
        PASSED += 1
    else:
        FAILED.append(f"{CURRENT}: {label}")


def skip(reason: str) -> None:
    """This case asserted NOTHING, and why — named, so the summary can say so.

    ★★★★★ R2224 — **the denominator was invisible, and that is how the red this
    round repaid stayed unexplained.** Four cases here return early when a
    dependency is absent (`Xvfb`, a live display), and they did it silently. So
    the suite printed `92 passed` on a developer machine and `85 passed, 1
    failed` in CI, and nothing said the six missing assertions were missing
    rather than removed: a case that quietly stops asserting is indistinguishable
    from a case that quietly stops existing.

    `tools/sweep_headless.sh` already draws this distinction for demos — it
    tallies SKIP separately and prints *"env dep absent, coverage NOT
    exercised"*, on R1333's reasoning that a vacuous pass must not read as a
    green. This is that discipline, one harness over.
    """
    SKIPPED.append(f"{CURRENT}: {reason}")


# ---------------------------------------------------------------------------
# Captured output. Trimmed to the blocks the parse reads; the ellipses stand
# for lines that carry no evidence, and the parse must not need them.
# ---------------------------------------------------------------------------

#: `xdpyinfo -display :1` on the owner's seat. ⚠ The `dimensions:` line is kept
#: DELIBERATELY: it carries millimetres, and a parse that read the screen's size
#: instead of an output's would pass every other case here.
XDPYINFO_SEAT = """\
name of display:    :1
version number:    11.0
vendor string:    The X.Org Foundation
X.Org version: 21.1.11
number of extensions:    31
    BIG-REQUESTS
    Composite
    DAMAGE
    DOUBLE-BUFFER
    DPMS
    DRI2
    DRI3
    GLX
    RANDR
    RENDER
    XFree86-DGA
    XFree86-VidModeExtension
    XInputExtension
    XKEYBOARD
    XTEST
    XVideo
default screen number:    0
number of screens:    1

screen #0:
  dimensions:    2560x1600 pixels (677x423 millimeters)
  resolution:    96x96 dots per inch
"""

#: `xdpyinfo -display :98` — the Xvfb. Same vendor, same version, no DPMS, and
#: a `dimensions:` line whose millimetres are computed from 96 DPI.
XDPYINFO_VIRTUAL = """\
name of display:    :98
version number:    11.0
vendor string:    The X.Org Foundation
X.Org version: 21.1.11
number of extensions:    23
    BIG-REQUESTS
    Composite
    DAMAGE
    DOUBLE-BUFFER
    GLX
    RANDR
    RECORD
    RENDER
    XInputExtension
    XKEYBOARD
    XTEST
    XVideo
default screen number:    0
number of screens:    1

screen #0:
  dimensions:    1920x1200 pixels (488x305 millimeters)
  resolution:    96x96 dots per inch
"""

#: `xrandr --display :1`. Six outputs, one connected, with EDID millimetres.
XRANDR_SEAT = """\
Screen 0: minimum 8 x 8, current 2560 x 1600, maximum 32767 x 32767
DP-0 disconnected (normal left inverted right x axis y axis)
DP-1 disconnected (normal left inverted right x axis y axis)
DP-2 disconnected (normal left inverted right x axis y axis)
DP-3 disconnected (normal left inverted right x axis y axis)
HDMI-0 disconnected (normal left inverted right x axis y axis)
DP-4 connected primary 2560x1600+0+0 (normal left inverted right x axis y axis) 344mm x 215mm
   2560x1600     60.00*+ 240.00
"""

#: `xrandr --display :98`. One output, connected, and ZERO millimetres — which
#: is the whole difference.
XRANDR_VIRTUAL = """\
Screen 0: minimum 1 x 1, current 1920 x 1200, maximum 1920 x 1200
screen connected 1920x1200+0+0 0mm x 0mm
   1920x1200      0.00*
"""

#: `xrandr --display :99`, the throwaway. Same shape at another geometry — kept
#: so the virtual case is not one sample.
XRANDR_VIRTUAL_99 = """\
Screen 0: minimum 1 x 1, current 1024 x 768, maximum 1024 x 768
screen connected 1024x768+0+0 0mm x 0mm
   1024x768       0.00*
"""


def _probe_from(info: str, layout: str, display: str = ":n") -> Probe:
    return Probe(
        display=display,
        reachable=True,
        extensions=parse_extensions(info),
        outputs=parse_outputs(layout),
        ran=("xdpyinfo", "xrandr"),
    )


# ---------------------------------------------------------------------------
# The parse
# ---------------------------------------------------------------------------


def test_the_extension_block_is_read_and_bounded() -> None:
    seat = parse_extensions(XDPYINFO_SEAT)
    check("DPMS" in seat, "the seat's DPMS is read")
    check("RANDR" in seat, "an ordinary extension is read")
    virtual = parse_extensions(XDPYINFO_VIRTUAL)
    check("DPMS" not in virtual, "the virtual server advertises no DPMS")
    check("RANDR" in virtual, "and the block was read, not skipped")
    # The block ends at the first unindented line. Without that bound the lines
    # under `screen #0:` would join the set, and `dimensions:` would become an
    # "extension" — harmless here, and exactly the kind of drift that makes a
    # later membership test answer about the wrong text.
    check(
        not any(name.startswith("dimensions") for name in virtual),
        "the block stops at the first unindented line",
    )
    check(
        parse_extensions("") == frozenset(),
        "output with no block yields no extensions",
    )


def test_the_output_lines_are_read_and_the_screen_line_is_not() -> None:
    seat = parse_outputs(XRANDR_SEAT)
    check(len(seat) == 6, f"six outputs, not {len(seat)}")
    connected = [o for o in seat if o.connected]
    check(len(connected) == 1, "one of them is connected")
    check(
        connected[0] == Output("DP-4", True, 344, 215),
        f"the panel is read whole, got {connected[0]}",
    )
    check(
        all(o.width_mm == 0 for o in seat if not o.connected),
        "a disconnected output carries no size",
    )
    # ⚠ `Screen 0: ... current 2560 x 1600` is not an output. If it were read as
    # one it would be named `Screen` and carry no millimetres, so it would not
    # change a verdict — which is why this is asserted rather than assumed.
    check(
        not any(o.name == "Screen" for o in seat),
        "the Screen header is not an output",
    )
    for label, text in (("98", XRANDR_VIRTUAL), ("99", XRANDR_VIRTUAL_99)):
        virtual = parse_outputs(text)
        check(len(virtual) == 1, f":{label} has one output")
        check(virtual[0].connected, f":{label}'s output says connected")
        check(
            virtual[0].width_mm == 0 and virtual[0].height_mm == 0,
            f":{label}'s output reports no physical size",
        )


# ---------------------------------------------------------------------------
# The rule
# ---------------------------------------------------------------------------


def test_the_real_seat_is_refused_and_the_virtual_ones_are_not() -> None:
    seat = judge(_probe_from(XDPYINFO_SEAT, XRANDR_SEAT, ":1"))
    check(seat.kind == SEATED, f"the owner's display is seated, got {seat.kind}")
    check(not seat.may_paint, "and nothing may paint on it")
    check(len(seat.signs) == 2, f"both signs fired, got {len(seat.signs)}")
    for display, layout in ((":98", XRANDR_VIRTUAL), (":99", XRANDR_VIRTUAL_99)):
        virtual = judge(_probe_from(XDPYINFO_VIRTUAL, layout, display))
        check(
            virtual.kind == OFFSCREEN,
            f"{display} is offscreen, got {virtual.kind}",
        )
        check(virtual.may_paint, f"{display} may be painted on")
        check(not virtual.signs, f"{display} shows no sign of a seat")


def test_either_sign_alone_refuses() -> None:
    """The signs are independent, and a guard that needed both would be weaker."""
    dpms_only = Probe(
        display=":n",
        reachable=True,
        extensions=frozenset({"DPMS", "RANDR"}),
        outputs=(Output("screen", True, 0, 0),),
        ran=("xdpyinfo", "xrandr"),
    )
    check(judge(dpms_only).kind == SEATED, "DPMS alone refuses")
    panel_only = Probe(
        display=":n",
        reachable=True,
        extensions=frozenset({"RANDR"}),
        outputs=(Output("eDP-1", True, 344, 215),),
        ran=("xdpyinfo", "xrandr"),
    )
    check(judge(panel_only).kind == SEATED, "a panel alone refuses")
    # ⚠ And `connected` is NOT a sign: the virtual server's one output says
    # connected. This is the case a reasonable implementation gets wrong.
    connected_only = Probe(
        display=":n",
        reachable=True,
        extensions=frozenset({"RANDR"}),
        outputs=(Output("screen", True, 0, 0),),
        ran=("xdpyinfo", "xrandr"),
    )
    check(
        judge(connected_only).kind == OFFSCREEN,
        "a connected output with no millimetres is not a panel",
    )
    # A size on a DISCONNECTED output is stale EDID, not a seat.
    stale = Probe(
        display=":n",
        reachable=True,
        extensions=frozenset({"RANDR"}),
        outputs=(Output("HDMI-0", False, 520, 320),),
        ran=("xdpyinfo", "xrandr"),
    )
    check(judge(stale).kind == OFFSCREEN, "a disconnected panel is not a seat")


def test_one_millimetre_in_either_axis_is_a_panel() -> None:
    for w, h in ((1, 0), (0, 1)):
        one = Probe(
            display=":n",
            reachable=True,
            outputs=(Output("odd", True, w, h),),
            ran=("xrandr",),
        )
        check(judge(one).kind == SEATED, f"{w}mm x {h}mm refuses")


def test_what_cannot_be_proven_offscreen_is_refused() -> None:
    unreachable = judge(Probe(display=":7", reachable=False))
    check(unreachable.kind == UNREACHABLE, "a display that did not answer")
    check(not unreachable.may_paint, "and it may not be painted on")
    unprobed = judge(
        Probe(display=":7", reachable=True, ran=(), absent=("xdpyinfo is not installed",))
    )
    check(unprobed.kind == UNPROBED, "a display no probe reached")
    check(not unprobed.may_paint, "and it may not be painted on either")
    # ★ The fail-closed direction, stated as a case: evidence-free is NOT clean.
    check(
        not judge(Probe(display=":7", reachable=True)).may_paint,
        "an empty probe never allows",
    )


def test_partial_evidence_still_judges_and_says_so() -> None:
    """xrandr is absent on some runners; the DPMS sign must still carry."""
    seat = judge(
        Probe(
            display=":1",
            reachable=True,
            extensions=parse_extensions(XDPYINFO_SEAT),
            outputs=(),
            ran=("xdpyinfo",),
            absent=("xrandr is not installed",),
        )
    )
    check(seat.kind == SEATED, "the seat is still refused without xrandr")
    check(
        any("xrandr" in gap for gap in seat.absent),
        "and the verdict says which evidence was missing",
    )


# ---------------------------------------------------------------------------
# The guard
# ---------------------------------------------------------------------------


def _seat_judged(display: str = ":1") -> None:
    display_seat._JUDGED[display] = judge(
        _probe_from(XDPYINFO_SEAT, XRANDR_SEAT, display)
    )


def _virtual_judged(display: str = ":98") -> None:
    display_seat._JUDGED[display] = judge(
        _probe_from(XDPYINFO_VIRTUAL, XRANDR_VIRTUAL, display)
    )


def test_the_guard_refuses_a_seat_and_lets_an_offscreen_display_through() -> None:
    saved = dict(display_seat._JUDGED)
    allow = os.environ.pop(display_seat.ALLOW_ENV, None)
    try:
        _seat_judged()
        _virtual_judged()
        raised = ""
        try:
            refuse_seated(":1", what="a demo")
        except SeatedDisplay as exc:
            raised = str(exc)
        check(bool(raised), "a seated display raises")
        check("DISPLAY=:1" in raised, "the exception names the display")
        check("a demo" in raised, "and what was about to run")
        check(display_seat.ALLOW_ENV in raised, "and the way to overrule it")
        # ★ The requirement the debt states in as many words: the refusal has to
        # say what to DO. It is asserted on the LONG form, which is what a person
        # reads — the exception is deliberately one line, because the harness
        # reports an unexpected exception through `repr` and the advice would
        # arrive as `\n` escapes.
        long_form = display_seat.refusal(
            display_seat.classify(":1"), "a demo"
        )
        check("Xvfb" in long_form or "DISPLAY=" in long_form, "it names a command")
        check(
            "344mm x 215mm" in long_form and "DPMS" in long_form,
            "and the evidence it judged on",
        )
        check(
            "2026-09-11" in long_form,
            "and whose instruction it is enforcing",
        )
        ok = refuse_seated(":98", what="a demo")
        check(ok is not None and ok.may_paint, "an offscreen display goes through")
        check(refuse_seated("", what="a demo") is None, "no display is not a refusal")
        check(refuse_seated(None, what="a demo") is None, "nor is an unset one")
    finally:
        display_seat._JUDGED.clear()
        display_seat._JUDGED.update(saved)
        if allow is not None:
            os.environ[display_seat.ALLOW_ENV] = allow


def test_the_override_lifts_this_rule_and_only_for_the_asking() -> None:
    saved = dict(display_seat._JUDGED)
    allow = os.environ.get(display_seat.ALLOW_ENV)
    try:
        _seat_judged()
        os.environ[display_seat.ALLOW_ENV] = "1"
        verdict = refuse_seated(":1", what="a demo")
        check(verdict is not None, "the override lets a seated display through")
        check(
            verdict is not None and verdict.kind == SEATED,
            "and the verdict is still SEATED — the override does not relabel it",
        )
        os.environ.pop(display_seat.ALLOW_ENV)
        refused = False
        try:
            refuse_seated(":1", what="a demo")
        except SeatedDisplay:
            refused = True
        check(refused, "and removing it restores the refusal")
    finally:
        display_seat._JUDGED.clear()
        display_seat._JUDGED.update(saved)
        if allow is None:
            os.environ.pop(display_seat.ALLOW_ENV, None)
        else:
            os.environ[display_seat.ALLOW_ENV] = allow


def test_the_advice_names_a_display_that_is_actually_free() -> None:
    live = display_seat.live_displays()
    spare = display_seat.free_display()
    check(spare not in live, f"{spare} is not one of the live displays {live}")
    check(spare.startswith(":"), "and it is spelled as a display")
    # ★ R2221.1 — the falsifiable form of "these are the same walk". The two
    # assertions above pass under two INDEPENDENT walks, which is how a second
    # spelling survived the round that added the general one; this one goes red
    # the moment they can disagree.
    first_candidate = display_seat.offscreen_candidates(live, tries=1)[0]
    check(
        spare == first_candidate,
        f"free_display agrees with the facility's first candidate "
        f"({spare} vs {first_candidate})",
    )
    lines = "\n".join(display_seat.advice())
    check(display_seat.ALLOW_ENV in lines, "the advice carries the override")
    # ★ R2221 — and it names the FACILITY, not an `Xvfb ... &` the reader has to
    # remember to take away again. Asserted unconditionally, because the branch
    # that used to carry the hand-typed server was the one taken on a machine
    # with no offscreen display — exactly where the advice matters most.
    check(
        "--with-offscreen" in lines,
        "the advice points at the thing that starts and reaps a display",
    )
    if display_seat.offscreen_here():
        check("DISPLAY=" in lines, "and offers the live one as an alternative")


# ---------------------------------------------------------------------------
# The facility — a display made on purpose, and taken away again
# ---------------------------------------------------------------------------


def test_the_candidate_order_skips_what_is_in_use() -> None:
    """Pure: the order is a rule, so it is asserted without starting anything."""
    got = display_seat.offscreen_candidates((":90", ":92"), start=90, tries=4)
    check(got == (":91", ":93", ":94", ":95"), f"taken numbers are skipped: {got}")
    check(
        display_seat.offscreen_candidates((), start=7, tries=3) == (":7", ":8", ":9"),
        "and with nothing taken it counts up from the start",
    )
    check(
        len(display_seat.offscreen_candidates((":90",) * 1, start=90, tries=5)) == 5,
        "the count asked for is the count returned, skips notwithstanding",
    )


def test_the_number_the_server_reports_is_the_one_we_asked_for() -> None:
    """Pure: a server that answers about a different display is not believed."""
    check(
        display_seat.read_display_number(b"90\n", expected=":90") == ":90",
        "the reported number is read",
    )
    check(
        display_seat.read_display_number(b"91\n", expected=":90") is None,
        "a different number is refused rather than followed",
    )
    for junk in (b"", b"\n", b"x\n", b"9 0\n", b"-1\n"):
        check(
            display_seat.read_display_number(junk, expected=":90") is None,
            f"{junk!r} is not a display number",
        )


#: The screen THIS case asks for — deliberately neither `DEFAULT_GEOMETRY`
#: (1920x1200) nor the value CI pins (1600x1200), so that a pass proves the
#: request actually reached the server rather than coinciding with whatever
#: the environment would have supplied anyway.
WANT_W, WANT_H = 1400, 1050


def test_a_skipped_case_is_named_rather_than_silent() -> None:
    """The skip ACCOUNTING, driven — because on this host nothing skips.

    ★★★★★ R2224 — this machine has `Xvfb` and a live display, so all four skip
    branches are unreachable here and the tally would read `0 skipped` whether
    the accounting worked or not. R2222 met exactly that shape in this tree's
    address ratchet — a freshly pinned gate whose rise branch no case could
    reach, so its green said nothing — and the answer is the same: drive the
    decision directly instead of hoping an environment reaches it.

    ⚠ The `pop` is deliberate and is why this is honest: this case did not
    actually skip anything, so it must not leave a row claiming coverage was
    not exercised. Without it the suite would report a skip that never happened,
    which is the reporting defect this round is repaying, inverted.
    """
    before = len(SKIPPED)
    skip("a reason this case invents")
    check(len(SKIPPED) == before + 1, "a skip is recorded rather than dropped")
    check(SKIPPED[-1].endswith("a reason this case invents"), "carrying its reason")
    check(CURRENT in SKIPPED[-1], "and naming the case that skipped")
    SKIPPED.pop()
    check(len(SKIPPED) == before, "and this case leaves no row of its own")


def test_the_geometry_is_the_argument_then_the_environment_then_the_default() -> None:
    """Pure: who decides a throwaway server's screen, in order.

    ★★★★★ R2224 — **the branch that was red in CI and green everywhere else.**
    `.github/workflows/ci.yml` sets `PINION_OFFSCREEN_GEOMETRY` as a JOB-level
    variable, so every step of the sweep job inherits it — the seat-rule tests
    included. No developer machine sets it. The facility case below therefore
    asserted the default's literal size and passed here forever while failing
    there, and nothing in this file said the environment could move that value
    at all.

    ⚠ The standing diagnosis for that red was that the case depended on the
    machine's borrowed `:98`. Measured R2224 and REFUTED: exporting the
    variable reproduces the failure exactly with `:98` still present, and
    unsetting it passes with `:98` still present. `:98` is not in the causal
    path; this decision is.
    """
    check(
        display_seat.chosen_geometry(None, {}) == display_seat.DEFAULT_GEOMETRY,
        "with nothing said, the default",
    )
    check(
        display_seat.chosen_geometry(None, {display_seat.GEOMETRY_ENV: "800x600x24"})
        == "800x600x24",
        "the environment beats the default — the branch CI takes",
    )
    check(
        display_seat.chosen_geometry("640x480x24", {display_seat.GEOMETRY_ENV: "800x600x24"})
        == "640x480x24",
        "an explicit argument beats the environment — the branch a test takes",
    )
    check(
        display_seat.chosen_geometry("", {display_seat.GEOMETRY_ENV: "800x600x24"})
        == "800x600x24",
        "an empty argument is not a choice, so the environment still wins",
    )


def test_a_display_is_started_proven_offscreen_and_then_gone() -> None:
    """The whole facility, for real — and the reap is the half that rots.

    ⚠ This case does what the milestone claims and then checks the claim from
    outside: the socket is there while the block runs and gone after it. A test
    that only asserted the display worked would pass forever on a tree that
    leaked one server per sweep, which is the defect this facility exists to
    stop ([[debt-the-demo-sweep-paints-on-the-users-own-screen]] records the
    stray `:98` that leak already produced).
    """
    if not shutil.which("Xvfb"):
        skip("Xvfb is not installed, so the facility cannot be exercised")
        return
    with display_seat.offscreen_display(
        start=140, geometry=f"{WANT_W}x{WANT_H}x24"
    ) as display:
        number = display[1:]
        check(display.startswith(":"), f"a display was handed out: {display}")
        check(
            os.path.exists(f"{display_seat.SOCKET_DIR}/X{number}"),
            f"{display} has a socket while the block runs",
        )
        verdict = display_seat.classify(display, refresh=True)
        check(verdict.kind == OFFSCREEN, f"{display} classifies offscreen")
        check(verdict.may_paint, "so a test may paint on it")
        probed = subprocess.run(  # noqa: S603 — argv, no shell
            ["xdpyinfo", "-display", display],
            capture_output=True,
            text=True,
            check=False,
        )
        check(probed.returncode == 0, "and a real X client can connect to it")
        check(
            f"{WANT_W}x{WANT_H}" in probed.stdout,
            f"with the geometry THIS CASE asked for ({WANT_W}x{WANT_H}), "
            f"whatever the environment pins",
        )
    check(
        not os.path.exists(f"{display_seat.SOCKET_DIR}/X{number}"),
        f"{display} is gone once the block ends — the reap actually runs",
    )
    check(display not in display_seat.live_displays(), "and the census agrees")


def test_a_number_already_bound_is_stepped_over() -> None:
    """The bind is the authority — measured against a server we put in the way.

    ⚠ The occupant is started WITHOUT a lock file being the reason it is found:
    `xvfb-run -a` chooses by scanning `/tmp/.X<n>-lock`, and R2220 measured that
    backwards on this machine (the seat has no lock file, the virtual server
    does). Here the first candidate is genuinely taken and the facility must
    move past it because the *bind* failed.
    """
    if not shutil.which("Xvfb"):
        skip("Xvfb is not installed, so no occupant can be put in the way")
        return
    occupant = display_seat.start_offscreen(":150", geometry="640x480x24")
    check(occupant is not None, "an occupant was started on :150")
    if occupant is None:
        return
    try:
        with display_seat.offscreen_display(start=150) as display:
            check(display != ":150", f"the facility stepped over :150, got {display}")
            check(
                os.path.exists(f"{display_seat.SOCKET_DIR}/X{display[1:]}"),
                "and what it handed out is live",
            )
        check(
            os.path.exists(f"{display_seat.SOCKET_DIR}/X150"),
            "the occupant it did not start is still there — never reaped",
        )
    finally:
        display_seat.reap_offscreen(occupant)


def test_a_server_that_answers_like_a_seat_is_not_handed_out() -> None:
    """The facility feeds the rule; it does not get to declare its own output.

    ⚠ The rule is replaced rather than the server, because there is no Xvfb
    flag that makes one look like a panel — and a check with no reachable
    failing case is decoration, which is the thing
    [[an-assertion-that-cannot-stand-moves-rather-than-dies]] names. What this
    pins is the direction of authority: if `classify` ever says seat, the
    facility refuses and still takes the server away.
    """
    if not shutil.which("Xvfb"):
        skip("Xvfb is not installed, so the refusal path cannot be driven")
        return
    seen: list[str] = []

    def pretend_seated(display: str, *, refresh: bool = False) -> object:
        seen.append(display)
        return display_seat.Verdict(
            display=display,
            kind=SEATED,
            signs=("a sign this test invented",),
            ran=("fixture",),
        )

    real = display_seat.classify
    display_seat.classify = pretend_seated  # type: ignore[assignment]
    try:
        refused = False
        try:
            with display_seat.offscreen_display(start=160):
                check(False, "a display that classifies as a seat was handed out")
        except display_seat.OffscreenUnavailable:
            refused = True
        check(refused, "the facility refused its own server")
    finally:
        display_seat.classify = real  # type: ignore[assignment]
    check(len(seen) == 1, f"the rule was asked exactly once: {seen}")
    if seen:
        check(
            not os.path.exists(f"{display_seat.SOCKET_DIR}/X{seen[0][1:]}"),
            f"and {seen[0]} was reaped even though the block raised",
        )


def test_the_wrapper_runs_a_command_there_and_forwards_its_verdict() -> None:
    """`--with-offscreen` end to end, including the exit code.

    The exit code is asserted because a wrapper that swallows it turns every red
    sweep green, and nothing else in this tree would notice.
    """
    if not shutil.which("Xvfb"):
        skip("Xvfb is not installed, so the wrapper cannot be run end to end")
        return
    tool = str(Path(display_seat.__file__).resolve())
    ok = subprocess.run(  # noqa: S603 — argv, no shell
        [sys.executable, tool, "--with-offscreen", "--", "sh", "-c",
         'test -n "$DISPLAY" && xdpyinfo >/dev/null'],
        capture_output=True,
        text=True,
        check=False,
    )
    check(ok.returncode == 0, f"the child got a working DISPLAY (rc={ok.returncode})")
    check(
        "offscreen server this tool started" in ok.stderr,
        "and the wrapper said which display it made",
    )
    red = subprocess.run(  # noqa: S603 — argv, no shell
        [sys.executable, tool, "--with-offscreen", "--", "sh", "-c", "exit 7"],
        capture_output=True,
        text=True,
        check=False,
    )
    check(red.returncode == 7, f"a failing child fails the wrapper (rc={red.returncode})")
    empty = subprocess.run(  # noqa: S603 — argv, no shell
        [sys.executable, tool, "--with-offscreen"],
        capture_output=True,
        text=True,
        check=False,
    )
    check(empty.returncode == 2, "and with no command it refuses rather than runs")


# ---------------------------------------------------------------------------
# The oracle — the probe itself, against whatever this machine has
# ---------------------------------------------------------------------------


def test_the_probe_agrees_with_itself_on_this_machine() -> None:
    """Self-consistency, asserted wherever a display exists and silent where not.

    ⚠ Deliberately asserts nothing about WHICH displays are here — that would
    be a test of the machine rather than of the probe, and it would go red the
    first time somebody's Xvfb exited. What it does assert is falsifiable on any
    machine with an X server: a verdict must be explained by its own evidence.
    """
    displays = display_seat.live_displays()
    if not displays:
        skip("no live display on this host, so the probe has nothing to read")
        return
    for name in displays:
        verdict = display_seat.classify(name, refresh=True)
        check(
            verdict.kind in (SEATED, OFFSCREEN, UNREACHABLE, UNPROBED),
            f"{name} got a known verdict",
        )
        if verdict.kind == SEATED:
            check(bool(verdict.signs), f"{name} is seated ON evidence")
        if verdict.kind == OFFSCREEN:
            check(not verdict.signs, f"{name} is offscreen WITHOUT a sign")
            check(bool(verdict.ran), f"{name} was actually probed")
        print(f"[display] {name}: {verdict.kind} ({len(verdict.signs)} sign(s))")


def main() -> int:
    # The population is derived from this module, not listed beside it — a case
    # added and not registered would otherwise not fail, it would not exist.
    cases = [
        (name, fn)
        for name, fn in list(globals().items())
        if name.startswith("test_") and callable(fn)
    ]
    if not cases:
        print("[display] the case scan found nothing — it is broken, not clean")
        return 1
    global CURRENT
    for name, fn in cases:
        CURRENT = name
        try:
            fn()
        except BaseException as exc:  # noqa: BLE001 — a raise IS the verdict
            check(False, f"raised {exc!r}")
    CURRENT = ""
    # ★★★★★ R2224 — the SKIPPED tally is the denominator, printed whether or
    # not anything failed. CI ran six fewer assertions than a developer machine
    # and the summary said only `85 passed`, so the gap read as normal. A count
    # that cannot say what it did not do is not a measurement.
    print(
        f"[display] {len(cases)} case(s): {PASSED} passed, {len(FAILED)} failed, "
        f"{len(SKIPPED)} skipped"
    )
    for reason in SKIPPED:
        print(f"[display] SKIPPED (dep absent, coverage NOT exercised): {reason}")
    if FAILED:
        # ★★★★★ R2220 — this exact SENTENCE is what makes a red here readable to
        # `tools/counterfactual.py`. Measured this round: five counterfactuals
        # that really caught, all five reported UNREADABLE, because none of this
        # tree's five python case suites spoke a vocabulary that driver knows.
        # The marker is the sentence and not the `[display]` prefix, so the other
        # four adopt it by printing this line — see the `python case suite` entry.
        # ⚠ Lower case: cargo's own vocabulary carries the bare word `FAILED`,
        # and the driver's selftest refuses a sentence two harnesses can read.
        for label in FAILED:
            print(f"[display] a case failed: {label}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
