#!/usr/bin/env python3
"""R2220 — **is a person sitting in front of this display?**, asked once.

The owner asked on 2026-09-11, looking at their own screen: *"사용자 화면에 뜨는
테스트가 있어? 전부 오프스크린이어야 해"*. The honest answer was not "no": this
tree had the policy written down in several places and **enforced in none**, so
what stopped a run from painting on the seat was whoever remembered to export
the right `DISPLAY`.

## What generated the defect

Not one site — the ABSENCE of a site. Display selection was spelled wherever a
run happened to need it:

* `tools/sweep_headless.sh` defaulted to `:0` and its mode is `realgpu`, i.e.
  the host's own X server. Its own header records the price it pays ("that
  reintroduces the host cursor the R720 Xvfb move removed") — a measured cost,
  accepted in prose, gated by nothing;
* fifteen demo docstrings tell a reader to run on `DISPLAY=:1`, which on this
  machine *is* the seat;
* sixty-four tell them `:97`, a number **nothing in this repository starts**
  (`tools/worktree.sh` says in a comment that it "must be started separately");
* the repayment loop's own briefs carried `:97` for nineteen rounds.

A display number is a CONVENTION, and a convention cannot refuse. So this module
exists to make "may a test paint here?" a question with ONE answer, derived from
what the X server says about itself, and to give the refusal a home the callers
share. It is the rule; the facility that *provides* an offscreen display is its
sequel (see `debt-the-demo-sweep-paints-on-the-users-own-screen`).

## The rule

A display is **seated** when the X server shows any sign of physical display
hardware. Two signs, each measured on this machine on 2026-09-13 against three
live displays — `:1` (the gdm Xorg the owner is looking at), `:98` (an Xvfb a
previous session left running) and a throwaway `Xvfb :99` started for the
measurement:

  1. the **DPMS** extension. Screen power management is a thing an X server has
     because there is a monitor to power down. Measured: present on `:1`,
     absent on `:98` and `:99`.
  2. a **connected RANDR output reporting a non-zero physical size**. Those
     millimetres come off the panel's EDID. Measured: `:1` reports
     `DP-4 connected ... 344mm x 215mm`; `:98` and `:99` report
     `screen connected ... 0mm x 0mm`.

Either sign alone refuses. That is deliberate: this is a safety rule, so its
error direction is set toward refusing a display it is unsure about, and the
cost of a wrong refusal is one message while the cost of a wrong allowance is
the thing the owner reported.

For the same reason **anything not provably offscreen is refused** — a display
that cannot be reached, or one no probe could run against.

## What was measured and REJECTED, so nobody re-derives it

Each of these reads like a criterion and is not one:

* `XDG_SESSION_TYPE` — describes the *caller's* session, not the display being
  asked about. Measured `tty` in the very pane that can drive `:1`.
* `loginctl show-session <id> -p Display` — **empty** for this machine's
  `Seat=seat0 Type=x11` session, so logind cannot name the seat's display.
* the screen's size in millimetres from `xdpyinfo` — ⚠ **the trap.** It is
  non-zero on a virtual server too (`:98` reports `488x305 millimeters`)
  because X computes it from a default 96 DPI. Only the *output's* size comes
  from EDID. A guard built on the screen line would have passed every review
  and refused nothing.
* `/tmp/.X<n>-lock` → the server's own `/proc/<pid>/cmdline` — **backwards on
  this machine.** `/tmp/.X98-lock` exists and names `Xvfb :98`; there is no
  `/tmp/.X1-lock` at all, because gdm starts Xorg with `-displayfd` and
  `-keeptty`. The route finds the virtual server and misses the real one.
* a window manager (`_NET_SUPPORTING_WM_CHECK`) — discriminates here, for the
  wrong reason. A WM on an Xvfb is legitimate, and a bare Xorg with no WM is
  still somebody's screen.
* the display NUMBER — what the whole tree used until now, and not a fact about
  the display at all.

## The limits, stated rather than discovered later

* A **nested** server (Xephyr, Xnest) draws inside another server's window: it
  reports no physical output and no DPMS, so it classifies as offscreen while
  being visible. Nothing in this tree starts one. If that changes, the sign to
  add is the nesting server's parent display.
* **Xdummy** (Xorg with the dummy driver, a legitimate headless option) has
  DPMS and would be refused. That is the fail-closed direction working as
  intended; the message says how to proceed.
* This classifies a DISPLAY, not a Wayland socket. An XWayland display mirrors
  the compositor's outputs and so classifies as seated, which is right.

Run it:

    python3 tools/display_seat.py --check [:N]   # exit 0 offscreen, 3 refused
    python3 tools/display_seat.py --census       # every live display, judged
    python3 tools/display_seat.py --selftest     # the rule, against fixtures
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass, field

#: Setting this sets aside THIS rule and nothing else — the idiom this tree
#: already uses for `PINION_ALLOW_NO_KEEPALIVE` and `PINION_WRITE_FROM_WORKTREE`.
#: A bypass that names the rule it lifts is reviewable; `--no-verify` is not.
ALLOW_ENV = "PINION_ALLOW_SEATED_DISPLAY"

#: Where X servers put their sockets. The census reads it; nothing else does.
SOCKET_DIR = "/tmp/.X11-unix"

#: Verdict kinds. Only `OFFSCREEN` may be painted on.
SEATED = "seated"
OFFSCREEN = "offscreen"
UNREACHABLE = "unreachable"
UNPROBED = "unprobed"

#: Extensions that mean "there is display hardware here". One entry today; a
#: list because the next sign belongs beside it rather than in a second `if`.
SEAT_EXTENSIONS = ("DPMS",)


@dataclass(frozen=True)
class Output:
    """One RANDR output, as the server describes it."""

    name: str
    connected: bool
    width_mm: int
    height_mm: int


@dataclass(frozen=True)
class Probe:
    """What the probes could learn about a display. Evidence, not judgment.

    Kept separate from the verdict on purpose: the rule below is a pure
    function of this record, so it can be tested against captured output from
    real servers, and a defect in the rule is distinguishable from a defect in
    the parse. [[an-assertion-that-cannot-stand-moves-rather-than-dies]] is the
    other half of that: a rule with no falsifiable case is decoration.
    """

    display: str
    reachable: bool
    extensions: frozenset[str] = frozenset()
    outputs: tuple[Output, ...] = ()
    #: Probe names that produced evidence.
    ran: tuple[str, ...] = ()
    #: Probe names that could not run, and why — carried into the verdict so a
    #: reader can see WHICH evidence a judgment rests on.
    absent: tuple[str, ...] = ()


@dataclass(frozen=True)
class Verdict:
    """The answer, with the evidence that produced it."""

    display: str
    kind: str
    signs: tuple[str, ...] = ()
    ran: tuple[str, ...] = ()
    absent: tuple[str, ...] = ()
    detail: str = ""

    @property
    def may_paint(self) -> bool:
        """True only for a display proven to have no seat."""
        return self.kind == OFFSCREEN


# ---------------------------------------------------------------------------
# The rule — pure, and the only place the criterion is spelled
# ---------------------------------------------------------------------------


def seat_signs(probe: Probe) -> tuple[str, ...]:
    """Every sign of physical display hardware this probe found.

    Empty means the evidence gathered shows no seat — which is not the same as
    "there is no seat", and `judge` is where that difference is honoured.
    """
    signs: list[str] = []
    for ext in SEAT_EXTENSIONS:
        if ext in probe.extensions:
            signs.append(
                f"the {ext} extension is advertised — screen power management "
                f"is something a server has because there is a monitor"
            )
    for out in probe.outputs:
        # ⚠ `connected` alone is not the sign: a virtual server reports its one
        # output as connected too (`screen connected 1920x1200+0+0 0mm x 0mm`).
        # The millimetres are what come off a panel's EDID.
        if out.connected and (out.width_mm > 0 or out.height_mm > 0):
            signs.append(
                f"output {out.name} reports a physical size of "
                f"{out.width_mm}mm x {out.height_mm}mm — that is a panel"
            )
    return tuple(signs)


def judge(probe: Probe) -> Verdict:
    """Turn evidence into a verdict, refusing everything not proven offscreen."""
    if not probe.reachable:
        return Verdict(
            display=probe.display,
            kind=UNREACHABLE,
            ran=probe.ran,
            absent=probe.absent,
            detail="the display did not answer",
        )
    if not probe.ran:
        # Fail CLOSED. An absent probe is not evidence of absence — and this is
        # the opposite call from the CI stop-the-line gate, which fails open
        # because infrastructure absence is not evidence of BREAKAGE. The
        # difference is what a wrong answer costs: there, a missed red; here,
        # a window on the owner's screen.
        return Verdict(
            display=probe.display,
            kind=UNPROBED,
            absent=probe.absent,
            detail="no probe could run, so nothing here is proven",
        )
    signs = seat_signs(probe)
    return Verdict(
        display=probe.display,
        kind=SEATED if signs else OFFSCREEN,
        signs=signs,
        ran=probe.ran,
        absent=probe.absent,
    )


# ---------------------------------------------------------------------------
# The parses — also pure, and tested against captured output from real servers
# ---------------------------------------------------------------------------

_EXT_COUNT = re.compile(r"^number of extensions:\s+\d+\s*$")
_OUTPUT_LINE = re.compile(
    r"^(?P<name>[^\s]+)\s+(?P<state>connected|disconnected)\b(?P<rest>.*)$"
)
_PHYSICAL = re.compile(r"(?P<w>\d+)mm\s*x\s*(?P<h>\d+)mm")


def parse_extensions(text: str) -> frozenset[str]:
    """The extension names out of `xdpyinfo` output.

    They are listed one per indented line after `number of extensions: N`, and
    the block ends at the next unindented line.
    """
    names: list[str] = []
    inside = False
    for line in text.splitlines():
        if _EXT_COUNT.match(line):
            inside = True
            continue
        if not inside:
            continue
        if not line.startswith(" "):
            break
        name = line.strip()
        if name:
            names.append(name)
    return frozenset(names)


def parse_outputs(text: str) -> tuple[Output, ...]:
    """The outputs out of `xrandr` output.

    ⚠ The `Screen 0:` header line is NOT an output and must not be read as one
    — it carries pixel dimensions that look like a size and no millimetres. It
    is rejected by the regex itself (`Screen` is followed by `0:`, not by
    `connected`), measured rather than assumed: the first draft carried a
    `startswith("Screen ")` skip as well, and a probe showed that line could
    never reach it. A filter with no path to firing is one more thing a reader
    has to believe; the property it was defending is asserted in the tests
    instead, where it guards the regex that actually does the work.
    """
    outputs: list[Output] = []
    for line in text.splitlines():
        if line.startswith(" ") or line.startswith("\t"):
            continue  # a mode line under an output
        match = _OUTPUT_LINE.match(line)
        if not match:
            continue
        physical = _PHYSICAL.search(match.group("rest"))
        outputs.append(
            Output(
                name=match.group("name"),
                connected=match.group("state") == "connected",
                width_mm=int(physical.group("w")) if physical else 0,
                height_mm=int(physical.group("h")) if physical else 0,
            )
        )
    return tuple(outputs)


# ---------------------------------------------------------------------------
# The probes — the only part that touches the machine
# ---------------------------------------------------------------------------


def _run(argv: list[str], display: str) -> str | None:
    """Run a probe against one display, returning its stdout or None."""
    if shutil.which(argv[0]) is None:
        return None
    env = dict(os.environ)
    env["DISPLAY"] = display
    try:
        done = subprocess.run(
            argv,
            env=env,
            capture_output=True,
            text=True,
            timeout=10,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    if done.returncode != 0:
        return None
    return done.stdout


def probe(display: str) -> Probe:
    """Ask a display about itself with every probe available here."""
    ran: list[str] = []
    absent: list[str] = []

    info = _run(["xdpyinfo", "-display", display], display)
    if info is None:
        if shutil.which("xdpyinfo") is None:
            absent.append("xdpyinfo is not installed")
            # ⚠ Not reachable/unreachable — unknown. A display that exists and
            # one that does not are indistinguishable without a probe, so this
            # goes to `judge` as "reachable, nothing ran" and is refused there.
            return Probe(
                display=display,
                reachable=True,
                ran=(),
                absent=tuple(absent),
            )
        return Probe(display=display, reachable=False, absent=("xdpyinfo said no",))
    ran.append("xdpyinfo")
    extensions = parse_extensions(info)

    layout = _run(["xrandr", "--display", display], display)
    if layout is None:
        absent.append(
            "xrandr is not installed — the physical-size sign could not be read"
            if shutil.which("xrandr") is None
            else "xrandr failed on this display"
        )
        outputs: tuple[Output, ...] = ()
    else:
        ran.append("xrandr")
        outputs = parse_outputs(layout)

    return Probe(
        display=display,
        reachable=True,
        extensions=extensions,
        outputs=outputs,
        ran=tuple(ran),
        absent=tuple(absent),
    )


#: One verdict per display per process. A display does not grow a monitor
#: mid-run, and `RpcSubprocess` launches once per demo — one of which opens its
#: screen 59 times.
_JUDGED: dict[str, Verdict] = {}


def classify(display: str, *, refresh: bool = False) -> Verdict:
    """The verdict for a display, probing it at most once per process."""
    if refresh or display not in _JUDGED:
        _JUDGED[display] = judge(probe(display))
    return _JUDGED[display]


# ---------------------------------------------------------------------------
# The census, and the advice the refusal gives
# ---------------------------------------------------------------------------


def live_displays() -> tuple[str, ...]:
    """Every display with a socket on this machine, lowest first."""
    try:
        names = os.listdir(SOCKET_DIR)
    except OSError:
        return ()
    numbers = sorted(
        int(name[1:]) for name in names if name.startswith("X") and name[1:].isdigit()
    )
    return tuple(f":{n}" for n in numbers)


def free_display(start: int = 90) -> str:
    """The lowest display number at or above `start` that nothing is using."""
    taken = {d for d in live_displays()}
    n = start
    while f":{n}" in taken:
        n += 1
    return f":{n}"


def offscreen_here() -> tuple[str, ...]:
    """Live displays this machine already has that may be painted on."""
    return tuple(d for d in live_displays() if classify(d).may_paint)


def advice() -> list[str]:
    """What to do instead — measured against this machine, not generic."""
    ready = offscreen_here()
    lines: list[str] = []
    if ready:
        lines.append(f"  This machine already has an offscreen display: {ready[0]}")
        lines.append(f"      DISPLAY={ready[0]} <the command you just ran>")
        lines.append(
            f"      PINION_SWEEP_DISPLAY={ready[0]} tools/sweep_headless.sh ..."
        )
    else:
        spare = free_display()
        lines.append("  Start a throwaway display and point the run at it:")
        lines.append(f"      Xvfb {spare} -screen 0 1920x1200x24 &")
        lines.append(f"      DISPLAY={spare} <the command you just ran>")
        lines.append(
            f"      PINION_SWEEP_DISPLAY={spare} tools/sweep_headless.sh ..."
        )
    lines.append(
        f"  To watch a window on purpose, set {ALLOW_ENV}=1 — it sets aside"
    )
    lines.append("  this rule and nothing else.")
    return lines


class SeatedDisplay(RuntimeError):
    """Raised when something would paint where a person is sitting."""


def refusal(verdict: Verdict, what: str) -> str:
    """The whole refusal, which has to say what to do as well as what is wrong."""
    head = {
        SEATED: "is a display somebody is sitting in front of",
        UNREACHABLE: "did not answer, so it cannot be shown to be offscreen",
        UNPROBED: "could not be probed, so it cannot be shown to be offscreen",
    }.get(verdict.kind, "was not proven offscreen")
    lines = [
        f"[display] REFUSED: {what} would run on DISPLAY={verdict.display}, "
        f"which {head}.",
    ]
    for sign in verdict.signs:
        lines.append(f"    evidence: {sign}")
    for gap in verdict.absent:
        lines.append(f"    note: {gap}")
    if verdict.detail:
        lines.append(f"    note: {verdict.detail}")
    lines.append(
        "  Every test in this tree runs offscreen (owner's instruction, "
        "2026-09-11)."
    )
    lines.extend(advice())
    return "\n".join(lines)


def refuse_seated(display: str | None, *, what: str) -> Verdict | None:
    """Refuse unless `display` is proven to have no seat.

    Returns the verdict when the run may proceed, and `None` when there is no
    display to protect at all. Raises `SeatedDisplay` otherwise.
    """
    if not display:
        return None  # nothing to paint on; the run's own problem if it needs one
    verdict = classify(display)
    if verdict.may_paint:
        return verdict
    if os.environ.get(ALLOW_ENV):
        print(
            f"[display] {ALLOW_ENV} is set — running on {display} "
            f"({verdict.kind}) anyway, which was asked for explicitly.",
            file=sys.stderr,
        )
        return verdict
    # ★ The long form goes to stderr and the exception carries a short one.
    #
    # Measured R2220 against the real harness: `rpc_verify.run_demo` reports an
    # unexpected exception with `repr`, which turned the whole refusal — the
    # evidence, the rule, the two commands that fix it — into one line of `\n`
    # escapes. The advice is the half of this that does any good, so it is
    # printed where a person reads it rather than encoded into a traceback.
    print(refusal(verdict, what), file=sys.stderr)
    raise SeatedDisplay(
        f"{what} may not run on DISPLAY={display} ({verdict.kind}) — see the "
        f"refusal above, or set {ALLOW_ENV}=1"
    )


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def _describe(verdict: Verdict) -> str:
    parts = [f"{verdict.display:>5}  {verdict.kind}"]
    if verdict.ran:
        parts.append(f"(probed by {', '.join(verdict.ran)})")
    for sign in verdict.signs:
        parts.append(f"\n         - {sign}")
    for gap in verdict.absent:
        parts.append(f"\n         ! {gap}")
    return " ".join(parts)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--check",
        nargs="?",
        const="",
        metavar="DISPLAY",
        help="refuse unless DISPLAY (default $DISPLAY) is proven offscreen",
    )
    parser.add_argument(
        "--what",
        default="this command",
        metavar="SUBJECT",
        help="what the refusal should say is about to run",
    )
    parser.add_argument(
        "--census", action="store_true", help="judge every live display here"
    )
    parser.add_argument(
        "--selftest", action="store_true", help="run the rule against fixtures"
    )
    args = parser.parse_args(argv)

    if args.selftest:
        import test_display_seat  # noqa: PLC0415 — the suite is the selftest

        return test_display_seat.main()

    if args.census:
        displays = live_displays()
        if not displays:
            print("[display] no live display on this machine")
            return 0
        for name in displays:
            print(_describe(classify(name)))
        return 0

    if args.check is not None:
        display = args.check or os.environ.get("DISPLAY", "")
        if not display:
            print("[display] no DISPLAY to check — nothing can be painted on")
            return 0
        try:
            verdict = refuse_seated(display, what=args.what)
        except SeatedDisplay:
            # The whole refusal is already on stderr — `refuse_seated` puts it
            # there so a caller that reports exceptions through `repr` still
            # shows a person the advice. Printing it again would double it.
            return 3
        if verdict is not None:
            print(f"[display] {_describe(verdict)}")
        return 0

    parser.print_help()
    return 2


if __name__ == "__main__":
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    sys.exit(main())
