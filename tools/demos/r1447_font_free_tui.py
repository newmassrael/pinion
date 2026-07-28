#!/usr/bin/env python3
"""R1447 §5.36 §5.41 §2#6 — a pinion TUI paints on a host with no fonts.

`LayoutCache` used to build a `FontContext` in its constructor, and
`FontContext::new()` enumerates every installed font through the platform
font API. The TUI never shapes — `pinion_tui`'s measure arm lays every
`Scene::Text` leaf out on the cell grid and never defers to parley, because
a terminal has no fonts — so the scan was pure waste on that path. R1447
defers the build to the first call that actually shapes.

The waste was the smaller half. The scan is not merely slow on a font-less
host, it **panics**: fontique unwraps a `NoMatch` while populating its
generic-family map (`backend/fontconfig.rs:685`). So the font-less backend
could not run on a font-less machine. This demo is that claim, executed:

  1. PREMISE — the system has fonts; the demo's own fontconfig has zero.
     Both are measured, so a broken fixture cannot read as a pass.
  2. THE CLAIM — `hello-button-tui`, a real terminal binary on a real pty,
     paints its frame under the zero-font config. Before R1447 this exact
     invocation exited 101 with the fontique panic having painted nothing.
  3. FONTS CHANGE NOTHING — the same binary under the system's 635-font
     config paints a **byte-identical** frame. That is §2#6 stated as a
     measurement: the TUI's output does not depend on fonts at all.
  4. THE SUITE — `pinion-tui`'s unit tests pass font-less, with the same
     count as under system fonts, and so do all four TUI examples'
     batteries. (Pre-R1447: 51 of 136 `pinion-tui` tests died.)
  5. THE INSTRUMENT — the parley arm genuinely cannot run in this
     environment. Asserted by running `pinion-runtime`'s arm-comparison
     test font-less and requiring it to FAIL on the fontique panic. This
     is what stops the whole demo from being vacuous: it proves the
     zero-font config really is zero-font, so every pass above is a pass
     against an environment that would catch a regression to eager
     construction.

ZERO-FLAKE: the only wait is a content predicate over the pty (read until
the painted label arrives, bounded by a timeout) — no wall-clock sleeps and
no dependence on key input, which a synthetic pty does not deliver reliably
to a crossterm reader. The app is terminated once it has painted; R1447 is
a claim about painting, not about the exit path.

Run from the workspace root:
    python3 tools/demos/r1447_font_free_tui.py
"""

# R1472 — this is the ONLY demo in the sweep that shells out to `cargo test`,
# which makes it the only one that needs the DEBUG test binaries rather than the
# release binary the sweep pre-builds for everyone else. Measured, because the
# first guess was wrong: with those binaries present the whole demo runs in
# **3.63s**; with them absent the same demo takes **89.85s** on a 16-core box,
# and on a 2-core CI runner it blew through the sweep's 180s per-demo budget and
# was killed at 180.010s. So its cost is compilation, not its own work — the CI
# job now builds the test binaries up front (`cargo test --workspace --no-run`)
# and the ordinary budget is ample.

from __future__ import annotations

import fcntl
import os
import pty
import re
import select
import struct
import subprocess
import sys
import tempfile
import termios
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from build_gate import ensure_built  # noqa: E402
from rpc_verify import (  # noqa: E402
    assert_eq,
    fc_list_count,
    run_demo,
    write_fontconfig,
)

REPO = Path(__file__).resolve().parent.parent.parent

EXAMPLE = "hello-button-tui"
COLS, ROWS = 80, 24
PAINT_TIMEOUT_S = 30.0

# Chrome the view paints — the label, the help line, and the box-drawing
# frame. Asserting on these (not merely on "some bytes arrived") is what
# makes "it painted" mean the frame, not the alternate-screen preamble.
LABEL = b"Click me!"
HELP = b"Space/Enter/click"
BOX_TOP_LEFT = "┌".encode()
BOX_BOTTOM_RIGHT = "┘".encode()
ALT_SCREEN = b"\x1b[?1049h"

# The four TUI examples. Their test batteries drive `render_one_frame`,
# which builds a `LayoutCache` per call — the consumer-reported waste.
TUI_EXAMPLES = [
    "hello-button-tui",
    "hello-toggle-tui",
    "hello-commands-tui",
    "hello-textfield-tui",
]

TEST_RESULT_RE = re.compile(
    r"test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored"
)


_T0 = time.monotonic()


def phase(label: str) -> str:
    """R1474 — seconds since the demo started, for the CI log.

    This demo was killed twice by the sweep's 180s budget on a runner while
    finishing in 3.6s here, and neither of the two obvious explanations
    survived measurement: after the job pre-builds the test binaries a
    `cargo test -p ...` recompiles nothing (0 crates, 0.35s), and pinning the
    whole demo to two cores reproduces nothing (3.61s). What is left is
    something about the runner this machine cannot reproduce — so the demo
    reports its own elapsed time per phase and lets the failing machine say
    where the time goes. R1472 is what makes that readable: a killed demo now
    keeps its output.
    """
    return f"[demo t+{time.monotonic() - _T0:6.1f}s] {label}"


def font_count(fontconfig: Path | None) -> int:
    """Fonts `fc-list` reports under `fontconfig` (None = the system's)."""
    return fc_list_count(fontconfig)



def write_empty_fontconfig(root: Path) -> Path:
    """A valid fontconfig whose font directory is empty — a font-less host,
    without needing one. Every generic family then resolves to no font."""
    return write_fontconfig(root)


def paint_on_pty(
    binary: Path, fontconfig: Path | None
) -> tuple[bytes, bytes, int | None]:
    """Run `binary` on a real pty; return `(painted bytes, stderr, exit code)`.

    Reads until the painted label arrives, then terminates the app. On the
    pre-R1447 build the label never arrives: the process dies in fontique
    first, so the read ends on EOF and the exit code carries the panic.

    The pty is made the child's **controlling** terminal (`setsid` +
    `TIOCSCTTY`): crossterm opens `/dev/tty`, not stdin, so without this the
    app would attach to the session's real terminal instead of ours.
    """
    env = dict(os.environ, TERM="xterm-256color")
    if fontconfig is None:
        env.pop("FONTCONFIG_FILE", None)
    else:
        env["FONTCONFIG_FILE"] = str(fontconfig)

    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))

    def attach_ctty() -> None:  # runs in the forked child, pre-exec
        os.setsid()
        fcntl.ioctl(0, termios.TIOCSCTTY, 0)

    proc = subprocess.Popen(
        [str(binary)],
        stdin=slave,
        stdout=slave,
        stderr=subprocess.PIPE,
        env=env,
        close_fds=True,
        preexec_fn=attach_ctty,  # noqa: PLW1509 — deliberate, see docstring
    )
    os.close(slave)

    painted = b""
    deadline = time.monotonic() + PAINT_TIMEOUT_S
    try:
        while time.monotonic() < deadline:
            ready, _, _ = select.select([master], [], [], 0.2)
            if not ready:
                continue
            try:
                chunk = os.read(master, 65536)
            except OSError:  # the slave side closed — the app is gone
                break
            if not chunk:
                break
            painted += chunk
            if LABEL in painted and HELP in painted:
                break
    finally:
        os.close(master)

    if proc.poll() is None:
        proc.terminate()
    # Read stderr before waiting: it is where the pre-R1447 fontique panic
    # went, so its emptiness is part of the claim, not incidental cleanup.
    errors = proc.stderr.read()
    proc.stderr.close()
    try:
        code = proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        proc.kill()
        code = None
    return painted, errors, code


def cargo_test(args: list[str], fontconfig: Path | None) -> tuple[int, int, int]:
    """Run `cargo test <args>` and return `(passed, failed, exit code)`.

    Counts come from cargo's own `test result:` line, never from grepping
    for the word "error" — an exit code and a parsed tally, so a build
    failure cannot read as "0 failed".
    """
    env = dict(os.environ, CARGO_BUILD_JOBS="2")
    if fontconfig is None:
        env.pop("FONTCONFIG_FILE", None)
    else:
        env["FONTCONFIG_FILE"] = str(fontconfig)
    out = subprocess.run(
        ["cargo", "test", *args],
        cwd=REPO,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    passed = failed = 0
    for match in TEST_RESULT_RE.finditer(out.stdout):
        passed += int(match.group(2))
        failed += int(match.group(3))
    combined = out.stdout + out.stderr
    if "error: could not compile" in combined:
        raise AssertionError(f"build failed for {args}:\n{out.stderr[-2000:]}")
    return passed, failed, out.returncode


def body() -> None:
    binary = ensure_built(EXAMPLE)

    with tempfile.TemporaryDirectory(prefix="r1447-nofonts-") as tmp:
        no_fonts = write_empty_fontconfig(Path(tmp))

        # ---- 1. premise: two genuinely different font environments ----
        system_fonts = font_count(None)
        assert system_fonts > 0, (
            f"premise: this host has fonts installed (fc-list = {system_fonts}); "
            "with none, the 'fonts change nothing' comparison below would be "
            "two identical environments"
        )
        print(phase(f"system fontconfig: {system_fonts} fonts"))
        assert_eq(font_count(no_fonts), 0, "fonts visible under the demo's config")

        # ---- 2. the claim: a real TUI binary paints font-less ----
        dark, dark_err, dark_code = paint_on_pty(binary, no_fonts)
        assert ALT_SCREEN in dark, (
            "premise: the app really drove a terminal — it switched to the "
            "alternate screen, so what follows is a painted frame"
        )
        assert LABEL in dark, (
            "the app painted its button label with zero fonts installed; "
            f"pre-R1447 this exited 101 in fontique having painted nothing "
            f"(got {len(dark)} bytes, exit={dark_code})"
        )
        assert HELP in dark, "the help line painted too — a whole frame, not a fragment"
        assert BOX_TOP_LEFT in dark, "the box-drawing frame painted (top-left corner)"
        assert BOX_BOTTOM_RIGHT in dark, "the box-drawing frame closed (bottom-right)"
        assert dark_code != 101, f"the app did not panic font-less (exit={dark_code})"
        assert b"panicked" not in dark_err, (
            f"nothing panicked on the font-less host: {dark_err[:400]!r}"
        )
        assert b"NoMatch" not in dark_err, (
            "no fontique NoMatch — the exact failure R1447 removes: "
            f"{dark_err[:400]!r}"
        )
        print(phase(f"painted {len(dark)} bytes with 0 fonts (exit={dark_code})"))

        # ---- 3. fonts change nothing (§2 #6, as a measurement) ----
        lit, lit_err, lit_code = paint_on_pty(binary, None)
        assert LABEL in lit, "the same binary paints under the system font config"
        assert b"panicked" not in lit_err, f"clean run with fonts: {lit_err[:400]!r}"
        assert_eq(lit_code, dark_code, "exit code across the two font environments")
        assert_eq(
            len(lit),
            len(dark),
            "painted byte count across 635 fonts and 0 fonts",
        )
        assert lit == dark, (
            "the painted frame is byte-identical with 635 fonts and with none: "
            "a TUI frame is cells, and cells do not consult a font"
        )
        print(phase(f"byte-identical paint across both font environments"))

        # ---- 4. the suite runs font-less ----
        # `pinion-tui` is the crate whose whole point is that it has no fonts.
        # Pre-R1447: 51 of 136 unit tests died here on the fontique panic.
        tui_pass_dark, tui_fail_dark, tui_rc_dark = cargo_test(
            ["-p", "pinion-tui", "--lib", "-j2"], no_fonts
        )
        assert_eq(tui_fail_dark, 0, "pinion-tui failures with zero fonts")
        assert_eq(tui_rc_dark, 0, "pinion-tui cargo exit code with zero fonts")
        assert tui_pass_dark > 100, (
            f"premise: the suite is substantial, not a stub ({tui_pass_dark} tests)"
        )
        tui_pass_lit, tui_fail_lit, tui_rc_lit = cargo_test(
            ["-p", "pinion-tui", "--lib", "-j2"], None
        )
        assert_eq(tui_fail_lit, 0, "pinion-tui failures with system fonts")
        assert_eq(tui_rc_lit, 0, "pinion-tui cargo exit code with system fonts")
        assert_eq(
            tui_pass_dark,
            tui_pass_lit,
            "pinion-tui tests passing — the count must not depend on fonts",
        )
        print(phase(f"pinion-tui: {tui_pass_dark} tests pass in both environments"))

        # Every TUI example's battery — these drive `render_one_frame`, the
        # entry point the consumer field report measured.
        for example in TUI_EXAMPLES:
            passed, failed, code = cargo_test(["-p", example, "-j2"], no_fonts)
            assert_eq(failed, 0, f"{example} failures with zero fonts")
            assert_eq(code, 0, f"{example} cargo exit code with zero fonts")
            assert passed > 0, f"premise: {example} has tests that ran ({passed})"
            print(phase(f"{example}: {passed} tests pass with 0 fonts"))

        # The mechanism itself, not only its effect: `pinion-text` pins that a
        # fresh cache has NOT scanned and that the first shape builds the
        # context. That crate legitimately needs fonts (shaping is its job), so
        # it runs under the system config.
        text_pass, text_fail, text_rc = cargo_test(
            ["-p", "pinion-text", "--lib", "-j2", "r1447_"], None
        )
        assert_eq(text_fail, 0, "pinion-text deferral test failures")
        assert_eq(text_rc, 0, "pinion-text deferral cargo exit code")
        assert_eq(text_pass, 3, "pinion-text deferral tests that ran")

        # The TUI guarantee itself, asserted font-less: a real terminal layout
        # pass over real text leaves the cache's font scan unrun.
        guard = "r1447_terminal_layout_pass_builds_no_font_context"
        guard_pass, guard_fail, guard_rc = cargo_test(
            ["-p", "pinion-tui", "--lib", "-j2", guard], no_fonts
        )
        assert_eq(guard_fail, 0, "TUI font-free guarantee failures with zero fonts")
        assert_eq(guard_rc, 0, "TUI font-free guarantee cargo exit code")
        assert_eq(guard_pass, 1, "the TUI font-free guarantee ran")

        # ---- 5. the instrument: this environment really is font-less ----
        # Every claim above is only worth as much as the zero-font config, so
        # this measures the config itself FROM INSIDE a pinion process rather
        # than trusting `fc-list`.
        #
        # R1460 replaced what used to stand here. The old instrument required
        # the parley path to FAIL font-less — and R1448 then taught pinion to
        # run font-less on purpose ("a window opens on a host with no fonts"),
        # so that failure stopped happening and this control started asserting
        # pre-R1448 behaviour. The same fact is now measured positively through
        # R1448's own typed status: with no fonts reachable, a probe answers
        # `Unavailable`.
        probe = "r1460_a_zero_font_config_is_font_less_from_inside_the_process"
        probe_args = ["-p", "pinion-text", "--lib", "-j2", probe, "--", "--ignored"]
        probe_pass, probe_fail, probe_rc = cargo_test(probe_args, no_fonts)
        assert_eq(probe_fail, 0, "font-less probe failures under the demo's config")
        assert_eq(probe_rc, 0, "font-less probe cargo exit code")
        assert_eq(probe_pass, 1, "the font-less probe ran")

        # And the half that makes it an instrument rather than a tautology: the
        # SAME probe must fail under the system config. If it passed in both,
        # it would be measuring nothing about which environment it is in.
        _, _, probe_lit_rc = cargo_test(probe_args, None)
        assert probe_lit_rc != 0, (
            "premise: the probe distinguishes the two font environments — if "
            "it passes with the system config too, it is not reading the "
            "config at all and the zero-font claims above rest on nothing"
        )

        # The measure-arm comparison stays, on its own terms: with fonts
        # present, an answering arm reaches no shaper while the parley arm
        # does. That is a claim about the ARMS and it is checked where it is
        # true, rather than being pressed into service as a font-environment
        # detector — which is what R1448 broke by making the parley arm survive
        # font-lessly.
        arm_test = "r1447_answering_measure_arm_leaves_the_font_scan_unrun"
        arm_pass_lit, arm_fail_lit, arm_rc_lit = cargo_test(
            ["-p", "pinion-runtime", "--lib", "-j2", arm_test], None
        )
        assert_eq(arm_fail_lit, 0, "parley-arm test failures with system fonts")
        assert_eq(arm_rc_lit, 0, "parley-arm cargo exit code with system fonts")
        assert_eq(arm_pass_lit, 1, "the arm-comparison test ran and passed")
        print(
            phase(
                "instrument verified: the probe reads the font environment, "
                "and the TUI needs no fonts either way"
            )
        )


if __name__ == "__main__":
    sys.exit(run_demo("r1447 font-free TUI paint", body))
