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
import contextlib
import ctypes
import os
import re
import select
import shutil
import signal
import subprocess
import sys
import time
from collections.abc import Iterator, Mapping
from dataclasses import dataclass, field

#: Setting this sets aside THIS rule and nothing else — the idiom this tree
#: already uses for `PINION_ALLOW_NO_KEEPALIVE` and `PINION_WRITE_FROM_WORKTREE`.
#: A bypass that names the rule it lifts is reviewable; `--no-verify` is not.
ALLOW_ENV = "PINION_ALLOW_SEATED_DISPLAY"

#: Where X servers put their sockets. The census reads it; nothing else does.
SOCKET_DIR = "/tmp/.X11-unix"

#: The throwaway server's screen. `:98`, the offscreen display this machine had
#: been borrowing from an earlier session, is `1920x1200x24`; matching it means a
#: walk that passed there passes here.
GEOMETRY_ENV = "PINION_OFFSCREEN_GEOMETRY"
DEFAULT_GEOMETRY = "1920x1200x24"


def chosen_geometry(explicit: str | None, env: Mapping[str, str]) -> str:
    """Which screen a throwaway server gets: the CALLER's, else the
    environment's, else [`DEFAULT_GEOMETRY`].

    ★★★★★ R2224 — **this decision had no name, and that is why nothing tested
    it.** It lived as `geometry or os.environ.get(...) or DEFAULT_GEOMETRY`
    inside [`offscreen_display`], one line among a context manager's setup, so
    the fact that a caller's environment can MOVE the screen was invisible to
    every case in `tools/test_display_seat.py` — while `.github/workflows/ci.yml`
    sets `PINION_OFFSCREEN_GEOMETRY: "1600x1200x24"` as a job-level variable
    that every step of that job inherits, the seat-rule tests included.

    The facility case then asserted the literal `1920x1200`, which is the
    DEFAULT, and was red in CI for eleven rounds' worth of pushes while passing
    on every developer machine — because no developer machine sets that
    variable. Measured R2224: exporting it locally reproduces CI's failure
    exactly, with the machine's stray `:98` still present, which is what refuted
    the standing diagnosis that the case depended on `:98`.

    ⚠ PURE, and taking its environment as an argument rather than reading the
    process's. A decision that reads a global cannot be driven through its own
    branches, and a branch a test cannot reach is a branch nothing holds — the
    shape R2222 removed from this tree's address ratchet one round earlier.
    """
    return explicit or env.get(GEOMETRY_ENV) or DEFAULT_GEOMETRY

#: Where the search for a free display number starts, and how far it goes. High,
#: because low numbers are where session managers put seats — not because a low
#: number would be unsafe (the bind settles that; see `start_offscreen`) but
#: because a throwaway server squatting `:0` is in somebody else's way.
CANDIDATE_START = 90
CANDIDATE_TRIES = 16

#: How long the server gets to say it is ready. Measured 2026-09-13 on this
#: machine: 0.06s. The bound is for the case where it never will.
READY_TIMEOUT_S = 10.0

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


def free_display(start: int = CANDIDATE_START) -> str:
    """The lowest display number at or above `start` that nothing is using.

    ⚠ R2221.1 — this had its OWN walk over the live displays until the round
    that added `offscreen_candidates` was audited against its own diff. The two
    loops sat eleven lines apart and neither could fail while both were right,
    which is exactly why a second spelling is treated here as a defect rather
    than as untidiness: it comes due on the day one of them is changed. The
    general form is the facility's; this is that form asked for one answer.
    """
    return offscreen_candidates(live_displays(), start=start, tries=1)[0]


def offscreen_here() -> tuple[str, ...]:
    """Live displays this machine already has that may be painted on."""
    return tuple(d for d in live_displays() if classify(d).may_paint)


# ---------------------------------------------------------------------------
# The facility — the rule's counterpart: "then WHERE may I paint?"
# ---------------------------------------------------------------------------
#
# ★★★★★ R2221. The rule above answers *may a test paint here?*. Until this
# section existed, the answer "no" was the end of the conversation: `advice()`
# printed an `Xvfb ... &` line and a person had to run it, and whether a sweep
# was offscreen came down to whether somebody had. That is how this machine came
# to have a `:98` — an Xvfb an earlier session started, never reaped, and which
# every run since had been *borrowing*. The debt states the consequence plainly:
# when it dies the sweep can only refuse, because nothing here can make one.
#
# So a run that needs a display makes its own and takes it away again. Three
# decisions, each from a measurement rather than from what reads well:
#
#   1. **The bind is the authority, not the lock file.** `xvfb-run -a` chooses a
#      number by scanning `/tmp/.X<n>-lock`, and R2220 measured that backwards
#      here: `/tmp/.X98-lock` exists for the virtual server while the seat at
#      `:1` has no lock file at all (gdm starts Xorg with `-displayfd` and
#      `-keeptty`). Measured 2026-09-13: `Xvfb :1` and `Xvfb :98`, each given an
#      explicit number, BOTH fail with *server already running* — the socket
#      bind sees the occupant the lock file misses. So candidates are tried with
#      an explicit number and a refusal advances to the next one.
#
#   2. **The server says when it is ready.** `-displayfd` writes the display
#      number to a pipe *when it is ready to connect* (measured: 0.06s), which
#      is the same mechanism gdm uses. The alternative — sleep, then poll
#      `xdpyinfo` — is a race dressed as a wait, and this tree has a zero-flake
#      policy to keep.
#
#   3. **What we start, we reap; what we did not start, we never touch.** The
#      reap runs from a `finally`, and `PR_SET_PDEATHSIG` is the backstop for
#      the case a `finally` cannot cover — the wrapper being SIGKILLed. That
#      backstop is not theoretical: the stray `:98` IS this leak, already
#      observed in this tree.
#
# ⚠ Limits, stated rather than found later:
#
#   * The server runs with no `-auth` cookie, so any local user could connect to
#     it — the same posture as the `:98` this tree has been borrowing, and
#     `-nolisten tcp` keeps it off the network. An auth file was considered and
#     refused for a structural reason: `probe()` would have to be told where the
#     cookie is, which makes THE RULE depend on THE FACILITY. The rule has to be
#     answerable about a display nobody here started.
#   * A `-displayfd` that never arrives is bounded by `READY_TIMEOUT_S`, and the
#     partially-started server is reaped before the next candidate is tried.


class OffscreenUnavailable(RuntimeError):
    """Raised when no offscreen display could be made."""


def offscreen_candidates(
    taken: tuple[str, ...],
    *,
    start: int = CANDIDATE_START,
    tries: int = CANDIDATE_TRIES,
) -> tuple[str, ...]:
    """Display numbers to try, in order, skipping the ones already in use.

    Pure, and separate from the bind for the reason `seat_signs` is separate
    from `probe`: the order is a rule, and a rule with no falsifiable case is
    decoration. ⚠ Skipping `taken` is a courtesy, not the safety — a socket that
    appears between this list and the bind is caught by the bind itself, which
    is the only check that cannot be raced.
    """
    out: list[str] = []
    n = start
    while len(out) < tries:
        name = f":{n}"
        if name not in taken:
            out.append(name)
        n += 1
    return tuple(out)


def read_display_number(data: bytes, *, expected: str) -> str | None:
    """The display the server reported on its `-displayfd` pipe, or None.

    Pure. `expected` is what we asked for on the command line; a server that
    answers something else is not believed, because every later decision — the
    reap, the verdict, the `DISPLAY` the child gets — is about one display and
    they must all be about the same one.
    """
    text = data.decode("utf-8", "replace").strip()
    if not text or not text.isdigit():
        return None
    got = f":{int(text)}"
    return got if got == expected else None


def _pdeathsig() -> None:  # pragma: no cover — runs in the forked child
    """Ask the kernel to SIGTERM this child when its parent dies.

    The `finally` below covers every exit this process can observe. This covers
    the one it cannot: being SIGKILLed. Best-effort by construction — a kernel
    or libc without `prctl` leaves the reap to the `finally`, which is where it
    was anyway.
    """
    try:
        ctypes.CDLL("libc.so.6", use_errno=True).prctl(1, signal.SIGTERM, 0, 0, 0)
    except Exception:  # noqa: BLE001 — a backstop that fails is still a backstop
        pass


def start_offscreen(
    display: str,
    *,
    geometry: str,
    timeout: float = READY_TIMEOUT_S,
) -> subprocess.Popen[bytes] | None:
    """Start `Xvfb` on exactly `display`, or None if that number is taken.

    Returns only once the server has said it is ready to connect.
    """
    read_fd, write_fd = os.pipe()
    os.set_inheritable(write_fd, True)
    try:
        proc = subprocess.Popen(  # noqa: S603 — argv, no shell
            [
                "Xvfb",
                display,
                "-displayfd",
                str(write_fd),
                "-screen",
                "0",
                geometry,
                "-nolisten",
                "tcp",
            ],
            pass_fds=(write_fd,),
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
            preexec_fn=_pdeathsig,  # noqa: PLW1509 — single-threaded by design
        )
    except OSError:
        os.close(read_fd)
        os.close(write_fd)
        raise
    # ⚠ Our copy of the write end must go, or the read below never sees EOF when
    # the server dies — the pipe would stay open because WE hold it.
    os.close(write_fd)

    buf = b""
    deadline = time.monotonic() + timeout
    try:
        while b"\n" not in buf:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                break
            ready, _, _ = select.select([read_fd], [], [], min(remaining, 0.25))
            if ready:
                chunk = os.read(read_fd, 64)
                if not chunk:
                    break  # EOF — the server exited without reporting
                buf += chunk
            elif proc.poll() is not None:
                break  # died, and its end of the pipe is gone with it
    finally:
        os.close(read_fd)

    if read_display_number(buf, expected=display) == display:
        return proc
    reap_offscreen(proc)
    return None


def reap_offscreen(proc: subprocess.Popen[bytes]) -> None:
    """Take the server away: ask, then insist."""
    if proc.poll() is not None:
        return
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=5)


@contextlib.contextmanager
def offscreen_display(
    *,
    geometry: str | None = None,
    start: int = CANDIDATE_START,
    tries: int = CANDIDATE_TRIES,
) -> Iterator[str]:
    """A display this process owns, offscreen by construction and by verdict.

    ★ The verdict is asserted rather than assumed. A facility that *declares*
    its output offscreen is the rule spelled a second time, and a second
    spelling is what this whole module exists to remove — so the display goes
    through `classify` like any other, and a server that somehow answers with a
    seat's signs is refused instead of painted on.
    """
    geometry = chosen_geometry(geometry, os.environ)
    if not shutil.which("Xvfb"):
        raise OffscreenUnavailable(
            "Xvfb is not installed, so no offscreen display can be made "
            "(Debian/Ubuntu: the 'xvfb' package)"
        )
    candidates = offscreen_candidates(live_displays(), start=start, tries=tries)
    proc: subprocess.Popen[bytes] | None = None
    display = ""
    for name in candidates:
        proc = start_offscreen(name, geometry=geometry)
        if proc is not None:
            display = name
            break
    if proc is None:
        raise OffscreenUnavailable(
            f"no free display number in {candidates[0]}..{candidates[-1]} — "
            f"{len(candidates)} were tried and each was already bound"
        )
    try:
        verdict = classify(display, refresh=True)
        if not verdict.may_paint:
            raise OffscreenUnavailable(
                f"the display this tool just started ({display}) does not "
                f"classify as offscreen but as {verdict.kind} — refusing to "
                f"hand it out, because the rule is the authority and not this"
            )
        yield display
    finally:
        reap_offscreen(proc)
        _JUDGED.pop(display, None)


def run_offscreen(argv: list[str], *, what: str = "this command") -> int:
    """Run `argv` on a display made for it, and take the display away after.

    The whole facility as one call, because this is what every caller wants and
    a caller assembling it from the parts would be the second spelling again.
    """
    if not argv:
        print("[display] --with-offscreen needs a command to run", file=sys.stderr)
        return 2
    with offscreen_display() as display:
        env = dict(os.environ)
        env["DISPLAY"] = display
        print(
            f"[display] {what} runs on {display}, an offscreen server this tool "
            f"started and reaps when it exits",
            file=sys.stderr,
        )
        # ⚠ Not `exec`: this process has to outlive the child in order to reap.
        # The child keeps THIS process group, so a terminal's Ctrl-C reaches it
        # directly; the server was put in its own session so the same Ctrl-C
        # does NOT kill it out from under a child that is still shutting down.
        proc = subprocess.Popen(  # noqa: S603 — argv, no shell
            argv,
            env=env,
            # ⚠ The same backstop the server gets, and for the other half of the
            # same failure: measured 2026-09-13, SIGKILLing the wrapper reaped
            # the server (pdeathsig) and left the CHILD running — a run still
            # driving windows on a display that had just been taken away.
            preexec_fn=_pdeathsig,  # noqa: PLW1509 — single-threaded by design
        )
        try:
            return proc.wait()
        except KeyboardInterrupt:
            # The terminal sent it to the whole group, so the child has it too.
            proc.wait()
            return 130
        finally:
            # Reached when SIGTERM raised `SystemExit` inside `wait` (see
            # `_reap_on_sigterm`). A child left running would keep painting on a
            # display that is about to be taken away.
            if proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()


def advice() -> list[str]:
    """What to do instead — measured against this machine, not generic."""
    lines: list[str] = [
        # ★★★★★ R2221 — this used to read `Xvfb :90 -screen 0 1920x1200x24 &`,
        # a server the reader had to remember to take away again. Nobody did:
        # the `:98` this machine carries is one of those, left by a session that
        # ended months ago. The facility below starts one and reaps it, so the
        # advice names it instead of asking a person to be the facility.
        "  Run it on a throwaway display this tree starts and reaps for you:",
        "      python3 tools/display_seat.py --with-offscreen -- "
        "<the command you just ran>",
        "      tools/sweep_headless.sh ...   # does this for itself, with no",
        "                                    # PINION_SWEEP_DISPLAY set",
    ]
    ready = offscreen_here()
    if ready:
        lines.append(
            f"  Or reuse one this machine already has ({ready[0]}) — but it is "
            f"somebody else's to stop:"
        )
        lines.append(f"      DISPLAY={ready[0]} <the command you just ran>")
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


def _reap_on_sigterm() -> None:
    """Make SIGTERM unwind instead of vanishing, so the reap's `finally` runs.

    Python's default SIGTERM handling ends the process without unwinding, which
    would leave the server this tool started behind — the exact leak this
    facility exists to stop. SIGINT already raises, so only SIGTERM needs this.
    """

    def _raise(signum: int, _frame: object) -> None:
        raise SystemExit(128 + signum)

    with contextlib.suppress(ValueError):  # not the main thread — then no signals
        signal.signal(signal.SIGTERM, _raise)


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
    parser.add_argument(
        "--with-offscreen",
        action="store_true",
        help="run the command after `--` on a throwaway offscreen display",
    )
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)

    if args.selftest:
        import test_display_seat  # noqa: PLC0415 — the suite is the selftest

        return test_display_seat.main()

    if args.with_offscreen:
        command = list(args.command)
        if command and command[0] == "--":
            command = command[1:]
        _reap_on_sigterm()
        try:
            return run_offscreen(command, what=args.what)
        except OffscreenUnavailable as exc:
            print(f"[display] REFUSED: {exc}", file=sys.stderr)
            return 4

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
