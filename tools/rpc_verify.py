#!/usr/bin/env python3
"""AI-first RPC self-verification harness (§5.49 R59, R51.193).

Spawns a pinion `WidgetView` example as a subprocess, drives it over
JSON-RPC 2.0 framed by newline-delimited stdin / stdout (see
`pinion-shell::spawn_stdin_rpc_reader`), and exposes typed convenience
wrappers for `scene/query`, `scene/invoke`, `scene/snapshot`,
`scene/intents`, and the generic envelope.

The harness is the Claude-side dogfood of §2 invariant #2 ("RPC headless
as AI primary path") + §2 invariant #7 ("scene-as-data"): every visual
round should end with a `tools/demos/*.py` that proves the change by
typed RPC, not by asking the human reader to describe a screenshot.

R640 §5.7 — gained the [`read_png_rgba8`] / [`sample_png_points`]
helpers so any demo that pairs `PINION_SCREENSHOT` capture with the
9-point pixel sample mandated by `[[center-only-pixel-sample-anti-pattern]]`
no longer needs PIL / Pillow as a third-party dep. The decoder
handles the subset the wgpu + vello + `png` substrate emits (RGBA
8-bit, non-interlaced, all five filter types) — enough for every
pinion design-parity binding.

Python 3.9+ stdlib only — no third-party deps. Run from the workspace
root so `cargo run -p <example>` resolves.
"""

from __future__ import annotations

import collections
import json
import queue
import shutil
import signal
import socket
import struct
import subprocess
import sys
import threading
import time
import zlib
import os
import shutil
import tempfile
from contextlib import AbstractContextManager, contextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Iterable, Iterator, NoReturn, Optional, Sequence

from build_gate import BuildError, ensure_built
import driven_binaries


WORKSPACE_ROOT = Path(__file__).resolve().parent.parent

#: ★★★★★ R1984 — the example binaries THIS demo process launched, and where
#: each one was on disk.
#:
#: Filled in by `RpcSubprocess._enter_inner`, drained by `run_demo` when the
#: body passes. It is the evidence half of the push gate that refuses to publish
#: an edit to an example nothing has driven: R1981, R1982 and R1983 each edited
#: `examples/hello-node-lab/src/lib.rs`, each ran the walks it called its blast
#: radius, and all of those walks launched `hello-analyzer-shell` — so CI failed
#: the STANDALONE lab's walk three commits later. One screen, two binaries.
#:
#: A list rather than a set because the order a demo launched things in is
#: readable evidence, and a demo legitimately launches one package twice.
_DRIVEN: list[tuple[str, Optional[Path]]] = []

# R1330 — when set, the caller has ALREADY built every example it will drive
# (the sweep does one workspace build up front), so a per-launch rebuild is
# redundant. Left unset for interactive / one-off runs, where `RpcSubprocess`
# rebuilds on entry so a source edit can never be silently outrun by a stale
# binary. See `tools/build_gate.py`.
_ASSUME_BUILT_ENV = "PINION_ASSUME_BUILT"


@contextmanager
def isolated_storage_dir(prefix: str) -> Iterator[Path]:
    """R666 §3 §5.15 — per-demo `PINION_STORAGE_DIR` isolation.

    Sets the env var to a fresh tempdir for the duration of the
    context, restoring (or unsetting) the previous value on exit and
    `shutil.rmtree`-ing the dir. Use to keep any todomvc demo from
    bleeding its typed rows into the developer's real
    `$XDG_DATA_HOME/pinion-todomvc/` blob (R665 introduced
    persistence-by-default, so R655-R664 demos predated and would
    contaminate each other without isolation).

    The yielded `Path` is the tempdir root; callers usually do not
    need it (the env var is the wire), but it's handy for asserting
    against the on-disk blob inside the demo body.
    """
    storage_dir = Path(tempfile.mkdtemp(prefix=prefix))
    prev = os.environ.get("PINION_STORAGE_DIR")
    os.environ["PINION_STORAGE_DIR"] = str(storage_dir)
    try:
        yield storage_dir
    finally:
        if prev is None:
            os.environ.pop("PINION_STORAGE_DIR", None)
        else:
            os.environ["PINION_STORAGE_DIR"] = prev
        shutil.rmtree(storage_dir, ignore_errors=True)


class HostObservation:
    """R1476 — a reading of whatever machine happens to be running the demo:
    printable, and deliberately NOT comparable.

    R1471 and R1473 each shipped a demo whose premise asserted something about
    the HOST — that it could draw Hangul, that it had any fonts at all — when
    the claim under test only ever needed the demo's own fixture. Both passed on
    a developer box and failed on a CI runner provisioned differently, which
    made a host property read as a defect in pinion. R1474 named it a class and
    recorded that nothing enforced the distinction.

    This is the enforcement, and it is a type rather than a review habit: the
    host arm of any comparison is worth *reporting* (a reader wants to know the
    real machine had 635 faces) and is never worth *asserting*. So `str`/format
    work and every comparison raises. `assert host_font_count() > 0` now fails
    the moment it is written, on every host, instead of only on the one runner
    that would have disproved it.
    """

    __slots__ = ("_value", "_what")

    def __init__(self, value: int, what: str) -> None:
        self._value = value
        self._what = what

    def __str__(self) -> str:
        return str(self._value)

    def __format__(self, spec: str) -> str:
        return format(self._value, spec)

    def __repr__(self) -> str:
        return f"<host {self._what}={self._value}, report-only>"

    def _refuse(self, op: str) -> "NoReturn":
        raise AssertionError(
            f"a premise must be about the fixture, not the host: this {op} "
            f"reads the developer's machine ({self._what}={self._value}). "
            "Build the environment the claim needs with `write_fontconfig(root, "
            "faces=...)` and assert on that; print this value instead."
        )

    def __bool__(self) -> bool:
        self._refuse("truth test")

    def __eq__(self, other: object) -> bool:
        self._refuse("comparison")

    def __ne__(self, other: object) -> bool:
        self._refuse("comparison")

    def __lt__(self, other: object) -> bool:
        self._refuse("comparison")

    def __le__(self, other: object) -> bool:
        self._refuse("comparison")

    def __gt__(self, other: object) -> bool:
        self._refuse("comparison")

    def __ge__(self, other: object) -> bool:
        self._refuse("comparison")

    __hash__ = None  # type: ignore[assignment]



#: R1660 — the faces every measurement in this tree shapes against.
#:
#: EXACTLY ONE, and that is load-bearing rather than minimal. Measured: adding
#: the sibling `NanumGothic-Regular.ttf` to the pinned set takes
#: `hello-tabbed-chart` from 6 escapes to 7, and the marks that move are LATIN
#: tick labels. The app's styles name families neither face provides, so with
#: two faces fontconfig's own ordering decides the fallback and the ink follows
#: it. With one face there is nothing to decide.
#:
#: Adding a face here is therefore a re-measurement of both budget files, not a
#: convenience — which is why the set is a named constant and not a directory.
PINNED_FACES: tuple[str, ...] = (
    "crates/pinion-text-font/tests/fonts/NotoSans-Regular.ttf",
)

_PINNED_FONTCONFIG: Path | None = None


def pinned_fontconfig() -> Path | None:
    """R1660 — a fontconfig exposing exactly [`PINNED_FACES`], or `None`.

    # The defect this exists for

    R1656 built a ratchet on painted INK: `scene/containment` reports which
    marks left the box that owns them, every demo pays it at boot against a
    per-example budget, and `scene/text_painted` feeds the text-smear peer. Ink
    is a function of the shaped face; the face is a function of the host's font
    database; the budgets were measured on one machine. So the ratchet gave a
    different verdict on CI, and 33 sweep runs went red on 8 examples that pass
    locally — a green local gate saying nothing about CI, which is the exact
    shape [[zero-flake-policy]] forbids.

    Reproduced rather than argued: pointing `FONTCONFIG_FILE` at a config
    exposing only DejaVu reproduces the CI failure on this machine
    (`hello-tabbed-chart` 6 escapes -> 7, and the marks that move are the
    chart's tick labels — pure ink extent).

    R1573 measured the same class one layer down — **40 of 94** unit tests read
    the host — and closed it with `LayoutCache::with_own_fonts`. R1656 then
    built the demo gate on a host-reading cache, which reopened it.

    # Why the font DATABASE and not the cache

    Tried first, and measured wrong: pinning the shell's `LayoutCache` to an
    own-fonts cache made three different hosts agree, and made
    `scene/text_painted` report **zero runs** — the screen had no text on it at
    all. `0 escapes` earned that way is green for the wrong reason, which is
    worse than the red it replaces. An own-fonts cache has no fallback, so a
    style naming a family the registered face does not provide resolves to
    nothing.

    Replacing the DATABASE keeps every fallback path intact and simply gives it
    one thing to find, so text still shapes (measured: 16/16 and 63/63 runs
    inked, and the numbers equal to this host's own).

    # What it does not cover

    fontconfig is the Linux font path. macOS and Windows resolve through Core
    Text / DirectWrite and ignore this, so a measurement taken there is still
    host-dependent. The sweep this gates is Linux-only; a second platform
    needs the cache-level pin above, done properly.

    Returns `None` when a caller has already set `FONTCONFIG_FILE` — the
    font-source demos (`r1447_font_free_tui`, `r1448_app_font`,
    `r1473_app_default_font`, `r1474_*`) drive that variable themselves and
    are ABOUT what the host has; pinning it under them would test nothing.
    """
    global _PINNED_FONTCONFIG
    if "FONTCONFIG_FILE" in os.environ:
        return None
    if _PINNED_FONTCONFIG is not None:
        return _PINNED_FONTCONFIG
    root = Path(tempfile.mkdtemp(prefix="pinion-pinned-fonts-"))
    faces = root / "faces"
    faces.mkdir()
    for rel in PINNED_FACES:
        src = WORKSPACE_ROOT / rel
        if not src.is_file():
            raise AssertionError(
                f"pinned face {rel} is missing — refusing to fall through to "
                "the host, which is what the pin exists to prevent"
            )
        shutil.copy2(src, faces / src.name)
    conf = root / "fonts.conf"
    conf.write_text(
        "<?xml version='1.0'?>\n"
        "<!DOCTYPE fontconfig SYSTEM 'fonts.dtd'>\n"
        "<fontconfig>\n"
        f"  <dir>{faces}</dir>\n"
        f"  <cachedir>{root / 'cache'}</cachedir>\n"
        "</fontconfig>\n",
        encoding="utf-8",
    )
    _PINNED_FONTCONFIG = conf
    return conf


def _fc_list(fontconfig: Path | None, pattern: str | None) -> int:
    """Faces `fc-list` reports under `fontconfig` (`None` = the host's own)."""
    env = dict(os.environ)
    if fontconfig is None:
        env.pop("FONTCONFIG_FILE", None)
    else:
        env["FONTCONFIG_FILE"] = str(fontconfig)
    args = ["fc-list"]
    if pattern is not None:
        args.append(pattern)
    out = subprocess.run(args, env=env, capture_output=True, text=True, check=False)
    return len([line for line in out.stdout.splitlines() if line.strip()])


def fc_list_count(fontconfig: Path, pattern: str | None = None) -> int:
    """R1474 — how many faces `fc-list` reports under a fontconfig the demo
    BUILT, optionally matching `pattern` (e.g. `":charset=ac00"` for Hangul).

    Lifted at its third copy (`r1447_font_free_tui`, `r1448_app_font`,
    `r1473_app_default_font`). The `pattern` argument is what makes one helper
    serve all three: counting *fonts* answers "are there any", while counting a
    charset answers "can this draw the script" — and R1474 landed because a demo
    asserted the first when it meant the second.

    R1476 — `fontconfig` is no longer optional, and the check is at runtime
    rather than in the annotation, because an annotation is not a gate: passing
    `None` here used to hand back an ordinary `int` that an `assert` would
    happily consume. The host is a different question with a different answer
    type; ask it through [`host_font_count`].
    """
    if fontconfig is None:
        raise AssertionError(
            "fc_list_count reads a fontconfig the DEMO built. For the machine "
            "running the demo call host_font_count(), whose value prints and "
            "refuses to be compared — a premise must be about the fixture."
        )
    return _fc_list(fontconfig, pattern)


def host_font_count(pattern: str | None = None) -> HostObservation:
    """R1476 — what the machine running this demo happens to have installed.

    The return value formats and refuses to compare, because a claim resting on
    it is a claim about the developer's box. See [`HostObservation`].
    """
    return HostObservation(_fc_list(None, pattern), f"fc-list {pattern or 'faces'}")


def write_fontconfig(root: Path, faces: tuple[str, ...] = ()) -> Path:
    """R1473 — a well-formed fontconfig over a directory holding exactly `faces`.

    Not "a broken config": a valid one describing a host whose font inventory
    the demo chose. With no faces that is a slim container; with a Latin-only
    face it is an ordinary CI runner that cannot draw CJK. Both are real hosts
    a binding meets, and both are states a demo needs to be able to construct.

    Lifted here at its THIRD copy (`r1447_font_free_tui`,
    `r1448_app_font`, `r1473_app_default_font` each wrote the same XML over the
    same two directories). The variable part is only which faces go in, so the
    duplication was mechanical and carried no per-demo opinion.

    R1476 — `root` is created if absent, along with `fonts/` and `cache/`. It
    used to have to exist, which meant every caller that wanted two environments
    in one temp dir wrote the same `mkdir` first; that reached four copies the
    moment a second demo grew a populated arm.
    Returns the config path, for `FONTCONFIG_FILE`.
    """
    fonts = root / "fonts"
    cache = root / "cache"
    fonts.mkdir(parents=True, exist_ok=True)
    cache.mkdir(parents=True, exist_ok=True)
    for face in faces:
        shutil.copy(face, fonts / Path(face).name)
    conf = root / "fontconfig.conf"
    conf.write_text(
        '<?xml version="1.0"?>\n'
        '<!DOCTYPE fontconfig SYSTEM "fonts.dtd">\n'
        "<fontconfig>\n"
        f"  <dir>{fonts}</dir>\n"
        f"  <cachedir>{cache}</cachedir>\n"
        "</fontconfig>\n"
    )
    return conf


class SocketClient(AbstractContextManager["SocketClient"]):
    """One line-framed JSON-RPC 2.0 connection straight to an app's `AF_UNIX`
    socket — the *client* channel, as distinct from `RpcSubprocess`'s
    out-of-band stdin *observer* channel.

    R1478 obligation-3b lift: r1393 (`SockClient`) and r1469
    (`connect_socket` + `rpc_over_socket`) had already wired this by hand, and
    the R1478 demo was the third site. The wiring is mechanical — an `AF_UNIX`
    stream, a bounded connect retry, one JSON line out, one JSON line back —
    so it is shared; what a *closed* connection means is the caller's opinion
    and stays at the call site (see `rpc`).

    The bounded connect retry is not politeness: `serve` returns before the
    accept thread has necessarily reached its first `accept()`, and an app
    binds during boot, so a connect can legitimately arrive early. Bounded and
    deterministic — never a fixed sleep.
    """

    def __init__(self, path: str | Path, *, timeout: float = 5.0) -> None:
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(timeout)
        deadline = time.monotonic() + timeout
        while True:
            try:
                self.sock.connect(str(path))
                break
            except OSError:
                if time.monotonic() >= deadline:
                    self.sock.close()
                    raise
                time.sleep(0.02)
        self._buf = b""
        # R1552 — server-initiated frames read past while awaiting a response.
        self._notifications: list[dict] = []

    def rpc(self, method: str, params: Any = None, rid: int = 1) -> Optional[dict]:
        """Send one frame and return the response, or `None` if the endpoint
        refused service — closed or reset the connection without answering.

        `None` is a *mechanical* report ("nothing came back"), not a verdict: a
        demo asserting the endpoint serves writes `assert resp is not None`, and
        one asserting it refuses (a withdrawn endpoint, R-PR48) writes
        `assert resp is None`. Raising here instead would make the refusing
        demos catch exceptions to express their own passing case.
        """
        env: dict[str, Any] = {"jsonrpc": "2.0", "id": rid, "method": method}
        if params is not None:
            env["params"] = params
        try:
            self.sock.sendall((json.dumps(env) + "\n").encode())
        except OSError:
            # The refusal beat our write. There is nothing left to read on a
            # socket that is already gone.
            return None
        # R1552 PINION-PR83 — read until the frame that answers THIS request.
        # Before R1552 the next line was necessarily the response, because
        # nothing else could arrive; now a subscription's `scene/changed`
        # notification can land between the request and its answer, and
        # returning it as the response would silently corrupt every caller.
        while True:
            frame = self._next_frame()
            if frame is None:
                return None
            if frame.get("id") is None and "method" in frame:
                self._notifications.append(frame)
                continue
            return frame

    def _next_frame(self) -> Optional[dict]:
        """One line-delimited frame off the socket, or `None` on EOF / reset."""
        while b"\n" not in self._buf:
            try:
                chunk = self.sock.recv(4096)
            except (ConnectionResetError, TimeoutError):
                return None
            if not chunk:  # EOF: closed with no response.
                return None
            self._buf += chunk
        line, self._buf = self._buf.split(b"\n", 1)
        return json.loads(line.decode())

    def notifications(self, method: Optional[str] = None) -> list[dict]:
        """R1552 — server-initiated frames this connection has received.

        Collected by `rpc` as it reads past them. A caller expecting a stream
        with no request to make uses `await_notifications`.
        """
        if method is None:
            return list(self._notifications)
        return [n for n in self._notifications if n.get("method") == method]

    def await_notifications(
        self,
        method: str,
        count: int,
        *,
        timeout: float = 5.0,
    ) -> list[dict]:
        """Block on the socket until `count` notifications of `method` have
        arrived, and return them.

        Bounded by the socket's own timeout rather than by a sleep. Raises
        `RpcError` naming how many were actually seen, so a failure is a count
        and not a hang.
        """
        deadline = time.monotonic() + timeout
        while len(self.notifications(method)) < count:
            if time.monotonic() >= deadline:
                raise RpcError(
                    -32099,
                    f"timed out awaiting {count} {method} notification(s); "
                    f"saw {len(self.notifications(method))}",
                )
            remaining = max(0.01, deadline - time.monotonic())
            self.sock.settimeout(remaining)
            frame = self._next_frame()
            if frame is None:
                break
            if frame.get("id") is None and "method" in frame:
                self._notifications.append(frame)
        return self.notifications(method)

    def close(self) -> None:
        """Close the connection. A plain close is the crash analog: the
        server's reader hits EOF and must fire `on_disconnect` for THIS
        connection's id."""
        self.sock.close()

    def __exit__(self, *exc: Any) -> None:
        self.close()


class RpcError(Exception):
    """JSON-RPC 2.0 error response or transport-level failure."""

    def __init__(self, code: int, message: str, data: Any = None) -> None:
        super().__init__(f"[{code}] {message}" + (f" — {data!r}" if data else ""))
        self.code = code
        self.message = message
        self.data = data


@dataclass
class Response:
    """JSON-RPC 2.0 response envelope (success branch only — error is raised)."""

    id: Any
    result: Any


class LeakedProcess(Exception):
    """R1570.3 — a driven binary outlived the demo that launched it.

    Raised at teardown so a leak fails the demo that caused it, instead of
    being left for whatever runs next to trip over.
    """


def terminate_process_tree(
    proc: subprocess.Popen,
    *,
    term_grace: float = 2.0,
    kill_grace: float = 2.0,
) -> Optional[str]:
    """Reap `proc` and everything it spawned. Return `None`, or why it survived.

    R1570.3 — the previous body was `SIGTERM; wait(2); kill(); wait(1)`, and the
    second `wait` could raise `TimeoutExpired` **out of the demo** while leaving
    the process running. Measured consequence, in CI: four binaries R1570 had
    left spinning became four orphans, and the 33 demos that ran after them on
    the same machine failed in a set that DIFFERED between runs. Four
    deterministic failures were reported as thirty-seven non-deterministic ones,
    and the root cause was only reachable by intersecting the two runs.

    Two things changed. Signals go to the process GROUP (the launch asks for a
    fresh session, so the group is exactly this binary and its children), which
    is what `Popen.kill` cannot do — a demo whose binary spawns a helper leaks
    the helper otherwise. And a survivor is REPORTED rather than raised over:
    the caller decides, because at teardown there may already be a more
    interesting exception in flight.

    Returns `None` when the process is reaped. Otherwise a sentence naming the
    pid and what was tried — the fact the sweep needs to attribute the leak to
    the demo that made it.
    """
    if proc.poll() is not None:
        return None

    def signal_group(sig: int) -> None:
        # The group is the fresh session `__enter__` asked for. Falling back to
        # the single process rather than skipping: a caller that launched
        # without `start_new_session` (or a platform without process groups)
        # should still get the old behaviour, not silently no signal at all.
        try:
            os.killpg(os.getpgid(proc.pid), sig)
        except (ProcessLookupError, PermissionError, OSError):
            try:
                proc.send_signal(sig)
            except (ProcessLookupError, OSError):
                pass

    for sig, grace in ((signal.SIGTERM, term_grace), (signal.SIGKILL, kill_grace)):
        signal_group(sig)
        try:
            proc.wait(timeout=grace)
            return None
        except subprocess.TimeoutExpired:
            continue

    return (
        f"pid {proc.pid} survived SIGTERM ({term_grace}s) and SIGKILL "
        f"({kill_grace}s) sent to its process group"
    )


_POINTER_REACH_BUDGET: Optional[dict[str, frozenset[str]]] = None


def _pointer_reach_budget() -> dict[str, frozenset[str]]:
    """R1650 — the measured backlog of widgets no press reaches, by example.

    A ratchet rather than a list of excuses: the boot gate refuses a victim
    absent from this file, so the population can only shrink. The shape is
    `docs/reference-names-budget.tsv`'s, and for the same reason — R1611 met a
    population of 7,999 and a gate that fails everything on day one is a gate
    somebody switches off.
    """
    global _POINTER_REACH_BUDGET
    if _POINTER_REACH_BUDGET is None:
        budget: dict[str, set[str]] = {}
        path = WORKSPACE_ROOT / "docs" / "pointer-reach-budget.tsv"
        if path.is_file():
            for line in path.read_text(encoding="utf-8").splitlines():
                if not line.strip() or line.lstrip().startswith("#"):
                    continue
                example, _, tag = line.partition("\t")
                if tag:
                    budget.setdefault(example.strip(), set()).add(tag.strip())
        _POINTER_REACH_BUDGET = {k: frozenset(v) for k, v in budget.items()}
    return _POINTER_REACH_BUDGET


_POINTER_ROUTING_BUDGET: "Optional[dict[str, str]]" = None


def _pointer_routing_budget() -> dict[str, str]:
    """R1664 — the surfaces whose OPENING screen routes no press, and why.

    A separate file from `_pointer_reach_budget`'s, deliberately, because it is
    a different claim: that one says *this painted widget's own centre answers
    to nobody*, this one says *nothing on this screen answers to anybody*. They
    look adjacent and are not, and folding two claims into one file is how a
    reader ends up unable to tell which one a row is making.

    Measured over all 224 examples before the gate was armed: nine route
    nothing, and every one of the nine was then checked against its own
    `$schema` and its paint. Eight are readouts with no action declared at all;
    the ninth, `hello-contextmenu`, is a live screen whose menu is painted only
    after a right-click — which is the gate's state-blindness rather than the
    screen's defect, and is why this file exists instead of nine repairs.

    The reason travels with the row so the claim is reviewable. A surface not
    listed must route a press.
    """
    global _POINTER_ROUTING_BUDGET
    if _POINTER_ROUTING_BUDGET is None:
        budget: dict[str, str] = {}
        path = WORKSPACE_ROOT / "docs" / "pointer-routing-budget.tsv"
        if path.is_file():
            for line in path.read_text(encoding="utf-8").splitlines():
                if not line.strip() or line.lstrip().startswith("#"):
                    continue
                example, _, reason = line.partition("\t")
                if reason.strip():
                    budget[example.strip()] = reason.strip()
        _POINTER_ROUTING_BUDGET = budget
    return _POINTER_ROUTING_BUDGET


#: What a budget row holds when the surface could not be measured on the host
#: that produced the file, beside the reason it could not.
UNMEASURED = "unmeasured"

_FC_LIST_CACHE: dict[str, Optional[frozenset]] = {}


def _fc_list_files(conf: Path) -> Optional[frozenset]:
    """Every font FILE a fontconfig resolves to, by base name, or `None` when
    `fc-list` is not installed.

    ★ Files and not families. The first draft asked for `family` and compared
    the answer to the pinned faces' file stems, which never match: the pin
    names `NotoSans-Regular.ttf` and the family is `Noto Sans`. It fired on the
    correctly-pinned run, which is how it was caught. A base name is the
    identity the pin is written in, so this is a comparison and not a
    translation.

    Cached per config path: the answer is a property of the file, every demo
    asks it once at boot, and the sweep runs it 223 times.
    """
    key = str(conf)
    if key in _FC_LIST_CACHE:
        return _FC_LIST_CACHE[key]
    answer: Optional[frozenset]
    try:
        out = subprocess.run(
            ["fc-list", ":", "file"],
            env={**os.environ, "FONTCONFIG_FILE": key},
            capture_output=True,
            text=True,
            timeout=20,
            check=False,
        )
        answer = (
            frozenset(
                Path(line.strip().rstrip(":")).name
                for line in out.stdout.splitlines()
                if line.strip()
            )
            if out.returncode == 0
            else None
        )
    except (OSError, subprocess.SubprocessError):
        answer = None
    _FC_LIST_CACHE[key] = answer
    return answer


#: Sentinel for "this file has not been read yet", distinct from `None`, which
#: is the answer "read, and it does not exist". Conflating them made every
#: budget read report the file missing — caught by the test, not by a run.
_MISSING = object()
_READ_BUDGETS: dict[str, object] = {}


#: Header line a producer writes to say its file is a CENSUS — one row per
#: example — rather than a list of the non-zero ones. The strict
#: missing-row rule arms only on a file that says this.
CENSUS_MARKER = "# census: total"


def _read_budget(name: str) -> "Optional[tuple[dict[str, object], bool]]":
    """A ratchet file in its three states (R1661 / R1662).

        <example>\t<count>                  measured, and this is the number
        <example>\tunmeasured\t<reason>     could not be measured there, stated
        (no row)                            AN ERROR — the census is total

    Returns `None` when the FILE does not exist, which is a fourth state and a
    different one: the ratchet has never been produced. R1661 made the missing-
    ROW distinction; the same argument applies to the file, and enforcing zero
    against a population nobody has measured makes the producer's first run
    impossible.

    ★ Why the missing row is an error rather than zero. Before R1661 a surface
    that could not be booted left no row, and no row read as `0` — the strictest
    budget there is, chosen by nobody. `hello-audio-device` cannot boot without
    `snd-dummy`, so it was silently budgeted at 0 while CI, which does load that
    card, measures 1. An absence that reads as the strictest possible claim is
    the shape this project keeps paying for.

    ★ ...and why that rule is CONDITIONAL. It can only be true of a file that
    is a census, and the two files this tree carries were written as lists of
    the non-zero examples: 25 rows for 223 surfaces. Arming the strict rule
    against them would fail 198 boot gates — including the boot gates the
    PRODUCER drives, which is a deadlock where the only way to write a census
    is to already have one. So a producer stamps its output with
    [`CENSUS_MARKER`] and the strict rule reads that stamp. The second value
    returned is that stamp.
    """
    path = WORKSPACE_ROOT / "docs" / name
    if not path.is_file():
        return None
    budget: dict[str, object] = {}
    total = False
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip() == CENSUS_MARKER:
            total = True
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        parts = line.split("\t")
        example = parts[0].strip()
        value = parts[1].strip() if len(parts) > 1 else ""
        if value.isdigit():
            budget[example] = int(value)
        elif value == UNMEASURED:
            budget[example] = (UNMEASURED, parts[2].strip() if len(parts) > 2 else "")
    return budget, total


#: R1669 — surfaces allowed to leave an inert region unexplained, and why.
#:
#: EMPTY, and that is the state worth keeping. An entry here is a claim that a
#: region is inert for a reason its author genuinely cannot name, which is a
#: much rarer thing than it looks: the author wrote the branch that disabled it.
_STATED_REASON_EXCEPTIONS: dict[str, int] = {}


#: R1693 — surfaces allowed to carry an unconditional structural fault, and why.
#:
#: EMPTY, and it stays that way for the same reason the two tables around it do:
#: neither arm needs a per-screen judgment. A collection that owns none of what
#: its role promises is one a reader is told about and cannot enter, and an
#: empty collection has a way to SAY it is empty (`aria-rowcount` /
#: `aria-colcount` / `aria-setsize` of zero), so "it is legitimately empty" is
#: not an exception — it is a declaration the screen makes and the census
#: accepts. A member outside the collection its role requires is a member an
#: assistive technology cannot place, wherever it appears.
_CONFORM_FAULT_EXCEPTIONS: dict[str, int] = {}


#: R1692 — surfaces allowed to carry an unconditional voice defect, and why.
#:
#: EMPTY, and for the same reason the table above is: the four arms this gate
#: judges need no per-screen judgment at all. A node that says nothing, a name
#: that is an address, a redirect to a node that does not speak, a box promising
#: children that do not — none of those is a taste question, so none of them has
#: a legitimate exception. (`unvoiced` DOES need one per screen, which is why it
#: is reported here and not judged.)
_VOICE_DEFECT_EXCEPTIONS: dict[str, int] = {}


def _budget_for(
    file_name: str, example: str, gate: str, cache_key: str
) -> "int | tuple[str, str] | None":
    """This example's allowance, or `None` when the gate must not run.

    `None` covers the two states a gate answers by REPORTING rather than by
    judging — the file has never been produced, or this surface has a stated
    reason it could not be measured — and both print, because a check that
    silently stopped happening is the failure mode these files exist to prevent.
    """
    read = _READ_BUDGETS.get(cache_key, _MISSING)
    if read is _MISSING:
        read = _read_budget(file_name)
        _READ_BUDGETS[cache_key] = read
    if read is None:
        print(f"[{gate}] {example}: UNARMED — docs/{file_name} has not been "
              f"produced (python3 tools/measure_ink_budgets.py)")
        return None
    budget, total = read
    if example not in budget:
        if not total:
            # A pre-census file: it lists the non-zero examples, so an absent
            # row is the zero it was always read as. Silent on purpose — this
            # is the majority case on such a file and printing it 198 times
            # would bury the rows that matter.
            return 0
        raise AssertionError(
            f"{example}: docs/{file_name} declares itself a census "
            f"({CENSUS_MARKER!r}) and has no row for this surface. A missing "
            f"row used to read as a budget of 0, which is the strictest claim "
            f"there is and one nobody made. Re-run "
            f"`python3 tools/measure_ink_budgets.py`, which writes every "
            f"example including the ones it could not measure."
        )
    allowed = budget[example]
    if isinstance(allowed, tuple):
        print(f"[{gate}] {example}: UNMEASURED where this file was produced "
              f"({allowed[1]}) — a host that CAN measure it is owed a number")
        return None
    return allowed


class RpcSubprocess(AbstractContextManager["RpcSubprocess"]):
    """Spawn a pinion example, drive it over JSON-RPC 2.0 stdin/stdout."""

    def __init__(
        self,
        example: str,
        *,
        release: bool = True,
        boot_grace: float = 0.8,
        request_timeout: float = 5.0,
        boot_timeout: float = 30.0,
        visible_window: bool = False,
        env: Optional[dict[str, str]] = None,
        ensure_build: bool = True,
        pointer_reach_exempt: Optional[dict[str, str]] = None,
        measuring: bool = False,
    ) -> None:
        self.example = example
        self.release = release
        # R1330 — rebuild the example before launching so a source edit can never
        # be outrun by a stale on-disk binary (the rig trap that cost the
        # R1327-R1329 session hours). `cargo build` is incremental, so the
        # no-change case is a sub-second fingerprint check. The sweep sets
        # `PINION_ASSUME_BUILT` after its own batch build and turns this off; a
        # caller streaming many launches of a known-fresh binary can pass
        # `ensure_build=False`.
        # R1333 — gate on the value `"1"` (not mere presence), matching the sibling
        # `PINION_SWEEP_NO_BUILD` convention. Presence-checking made `…=0` disable the
        # rebuild — the opposite of what a user setting `0` ("keep the gate on") means.
        self.ensure_build = ensure_build and (
            os.environ.get(_ASSUME_BUILT_ENV) != "1"
        )
        self.boot_grace = boot_grace
        self.request_timeout = request_timeout
        # R881.1 CI fix — deadline for the FIRST request only (the R719
        # boot-baseline `pointer_leave`, which doubles as the readiness
        # handshake). The very first pinion process on a runner with a
        # cold mesa/lavapipe shader cache compiles its pipelines before
        # the event loop services RPC, deterministically exceeding the
        # steady-state `request_timeout` (CI run 27256527715: sweep slot
        # 1 timed out twice at 5s while the 186 cache-warm followers all
        # answered in ~1s). Once the handshake answers, every subsequent
        # request runs under the normal per-request timeout.
        self.boot_timeout = boot_timeout
        # R835 §5.16 — by default the shell window is created UNMAPPED
        # (`PINION_HIDDEN_WINDOW`) so a local verification run renders the
        # full real pipeline (winit + GPU + Vello + present) WITHOUT a
        # window flashing on the developer's display / stealing focus.
        # Demos that screen-capture the real window (ffmpeg x11grab) pass
        # `visible_window=True` so the window is mapped for the grab.
        self.visible_window = visible_window
        # R1319 §5.16 — extra environment for the driven binary, merged over the
        # inherited env. The canonical use is raising the shell's log level
        # (`{"PINION_LOG": "pinion::shell=debug"}`): `init_tracing` filters on
        # `PINION_LOG` (default `warn`) and writes to STDERR, which this harness
        # already buffers into `stderr_tail` — so a demo can assert on a production
        # `tracing` line ([[verify-via-tracing-not-eprintln]]) instead of an example
        # growing an `eprintln!` purely to be observable.
        self.extra_env = dict(env or {})
        # R1650 §5.35 — widgets this surface is knowingly allowed to leave
        # unreachable at boot, as `{tag: reason}`. The escape hatch is not free
        # on purpose (R1640): a bare list would let a red be silenced by adding
        # a name, so each entry has to say WHY in a sentence that ends up in the
        # round's record, and a name that stops being unreachable fails too —
        # an exemption outliving its defect is a claim nobody re-checked.
        self.pointer_reach_exempt = dict(pointer_reach_exempt or {})
        # R1662 — the fontconfig this subprocess was launched with, filled in by
        # `__enter__`; see `_gate_font_pin`.
        self._env_fontconfig: Optional[Path] = None
        # ★ R1899 — the storage root THIS subprocess was given, when the harness
        # is the one that chose it. `None` means the caller owns the variable
        # (`isolated_storage_dir`, or a demo that set it), and teardown leaves
        # that root alone: a walk needing three processes to share one store
        # must outlive any one of them.
        self._own_storage: Optional[Path] = None
        # R1662 — this surface is being MEASURED for the ink ratchets, so the
        # three ink gates stand down.
        #
        # ★ Not a bypass: a producer cannot be judged by the file it is
        # producing. Measured while it was happening — a surface whose count
        # exceeded its committed budget had its boot gate refuse, the producer
        # recorded `unmeasured` with "a boot gate refused" as the reason, and
        # the true number it had just been asked for was the one thing that
        # could not get into the file. The other gates (pointer reach, the font
        # pin) still run: those are preconditions of a measurement being
        # meaningful, not the thing being measured.
        self.measuring = bool(measuring)

        self._proc: Optional[subprocess.Popen] = None
        self._inbox: "queue.Queue[str]" = queue.Queue()
        # R1552 PINION-PR83 — server-initiated frames (a `method`, no `id`).
        # Before R1552 nothing could arrive here unprompted, so `request`'s
        # id-matching loop simply DISCARDED every non-matching line. Now that
        # the server can speak first, discarding would silently drop the whole
        # subscription stream — and, worse, a demo asserting "no notification
        # arrived" would pass whether or not one did.
        self._notifications: list[dict] = []
        self._stderr_lines: list[str] = []
        self._stderr_thread: Optional[threading.Thread] = None
        self._stdout_thread: Optional[threading.Thread] = None
        self._next_id = 1

    def __enter__(self) -> "RpcSubprocess":
        # ★★ R1672 — EVERY exit path from here reaps the child, not only the
        # gate block's.
        #
        # `with` calls `__exit__` only for a context manager whose `__enter__`
        # RETURNED, so a raise anywhere in the body below leaks the subprocess
        # — and the body raises on a boot that times out, on a binary that dies
        # during boot, and on a first paint that never completes. R1650 wrote
        # exactly that sentence and guarded only the block it was adding;
        # R1666 then measured `ai-introspect-demo` alive for an hour and two
        # minutes after its boot baseline timed out, and R1672 measured it again
        # at an hour and eleven. The axis is not "the gate refused", it is
        # "anything left `__enter__` without returning".
        try:
            return self._enter_inner()
        except BaseException:
            self.shutdown()
            raise

    def _enter_inner(self) -> "RpcSubprocess":
        binary = self._resolve_binary()
        # R1984 — registered BEFORE the boot gates, and on purpose: what this
        # records is *which binary this demo drove*, and a demo that dies in a
        # boot gate still drove it. Whether the run counts as evidence is
        # `run_demo`'s decision, taken once, on the body's verdict — putting
        # that judgment here as well would give the property two authors.
        _DRIVEN.append((self.example, binary))
        cmd = [str(binary)] if binary else self._cargo_run_cmd()
        # R835 §5.16 — windowless-by-default env. Hidden unless the demo
        # asked for a visible window (x11grab screen capture) or the caller
        # set PINION_HIDDEN_WINDOW explicitly.
        env = dict(os.environ)
        if self.visible_window:
            env.pop("PINION_HIDDEN_WINDOW", None)
        elif "PINION_HIDDEN_WINDOW" not in env:
            env["PINION_HIDDEN_WINDOW"] = "1"
        # ★★ R1676 — the software rasteriser renders with ONE tile thread, so a
        # picture is a function of the scene and not of how the scheduler
        # interleaved the tiles.
        #
        # This is the same argument as the pinned font DB below, one layer down:
        # a pixel assertion is a claim about paint, and it was being made
        # through a rasteriser that answers differently for reasons paint has no
        # part in. R1664 measured the disagreement and made the assertions
        # tolerate it, which is the right move for noise you cannot remove.
        # R1676 measured whether it can be removed — ten captures of one
        # unchanged screen under software Vulkan, 45 pairs:
        #
        #   default          3 of 45 pairs byte-identical, worst 1
        #   LP_NUM_THREADS=1 45 of 45 pairs byte-identical, worst 0
        #
        # So it is thread interleaving, and it goes away. That matters twice.
        # It removes a FLAKE the tolerance could not: the floor is measured from
        # the control pair, and a control pair agreeing by luck — 3 times in 45
        # — reports 0 and then fails a tested pair that noised by 1. And it
        # restores the assertion's STRENGTH: with a tolerance of 1 a stale
        # fragment differing by one least-significant bit is invisible, and with
        # a deterministic rasteriser it is not.
        #
        # Ignored by every other driver, so this costs nothing where it does not
        # apply, and measured at no cost where it does (2.53s vs 2.57s for the
        # same demo). It is set HERE rather than in the CI job because a repro
        # on this machine has to have the property CI has — a local run that is
        # deterministic for a different reason than CI's is the green-local /
        # red-CI shape [[zero-flake-policy]] exists to forbid.
        env.setdefault("LP_NUM_THREADS", "1")
        # R1660 — every measurement in this tree shapes against ONE pinned face,
        # so a budget measured here means the same thing on CI. See
        # `pinned_fontconfig` for the defect and for why this replaces the font
        # DATABASE rather than the shell's cache.
        pinned = pinned_fontconfig()
        if pinned is not None:
            env["FONTCONFIG_FILE"] = str(pinned)
        # R1662 — remembered so `_gate_font_pin` can verify the database this
        # child actually resolves. `None` means the CALLER owns the variable
        # (the font-source demos), and the gate stands down there rather than
        # asserting about a database it did not choose.
        self._env_fontconfig = pinned
        # ★★★★★ R1899 §3 §5.15 — **a demo does not write to the developer's
        # home directory**, and a sweep's verdict does not depend on run order.
        #
        # Set here rather than in each demo because it is a property of every
        # driven binary, including ones nobody has written yet. A caller that
        # has ALREADY chosen a storage root keeps it: `isolated_storage_dir`
        # sets this variable in `os.environ` (so it is already in `env`), which
        # is how the walks that need several processes to share one store — a
        # save outliving the run that made it — still work.
        #
        # ⚠ THE CLASS WAS WRITTEN DOWN AND NOT MADE A PROPERTY. The docstring
        # on `isolated_storage_dir` has said since R666 that R665 introduced
        # persistence-by-default and that the demos predating it "would
        # contaminate each other without isolation". R1897 gave a SECOND
        # application the same persistence and only its own walk isolated:
        # `r1649` saves an arrangement, that save now lands on disk, and
        # `r1893` — which asserts the application ships FOUR arrangements —
        # found five and went red in CI. Measured on this machine, the file was
        # sitting in the real data directory with `Layout 5` in it. ⇒ a
        # prescription recorded as advice is one every later app has to
        # rediscover; this makes it the harness's answer instead.
        if "PINION_STORAGE_DIR" not in env:
            self._own_storage = Path(
                tempfile.mkdtemp(prefix=f"pinion-demo-{self.example}-")
            )
            env["PINION_STORAGE_DIR"] = str(self._own_storage)
        # R1319 — demo-supplied env wins (e.g. `PINION_LOG`), so a demo can observe a
        # level-gated `tracing` line the default `warn` filter would drop.
        env.update(self.extra_env)
        self._proc = subprocess.Popen(
            cmd,
            cwd=WORKSPACE_ROOT,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            bufsize=1,
            env=env,
            # R1570.3 — a fresh session makes this binary its own process-group
            # leader, so teardown can signal the GROUP and reap anything it
            # spawned. `Popen.kill` reaches only the direct child, which is how
            # a leaked helper survives a demo that looked cleanly shut down.
            start_new_session=True,
        )
        self._stdout_thread = threading.Thread(
            target=self._pump_stdout, daemon=True
        )
        self._stderr_thread = threading.Thread(
            target=self._pump_stderr, daemon=True
        )
        self._stdout_thread.start()
        self._stderr_thread.start()
        time.sleep(self.boot_grace)
        if self._proc.poll() is not None:
            raise RpcError(
                -32099,
                f"subprocess exited during boot (rc={self._proc.returncode})",
                "\n".join(self._stderr_lines[-20:]),
            )
        # R719 §5.35 §5.49 — establish a deterministic no-hover baseline.
        # A real X / Wayland server maps the window wherever the WM
        # places it — frequently under the host's physical cursor — and
        # winit delivers a genuine boot-time `CursorMoved`, leaving a
        # widget reporting `Hover` before any RPC ran (flaky boot-state
        # assertions, e.g. the accordion sweep). A headless CI with the
        # cursor parked never sees that event; `pointer_leave()` clears
        # the ambient hover so a cursor-occupied desktop matches that
        # clean baseline. Tolerate ONLY a genuinely-absent method
        # (-32601), i.e. a stale pre-R719 `target/release/` binary that
        # predates the `scene/pointer_leave` peer; the harness still
        # drives it (just without the baseline). Any other error fails
        # loud — a swallowed real failure would silently reintroduce the
        # very boot-hover flakiness this baseline exists to remove.
        # R881.1 — the baseline is also the readiness handshake: first
        # contact gets the generous `boot_timeout` (cold shader-cache
        # compile, see __init__), then the steady-state timeout resumes.
        #
        # R882.2 (zero-flake) — "the RPC answered" is NOT readiness:
        # `pointer_leave` is a paint-free ACK, and the expensive cold
        # path (lavapipe compiling the vello shader pipelines, several
        # seconds on a cold CI runner) runs inside the FIRST redraw on
        # the same event-loop thread. If that redraw starts AFTER the
        # ACK, the demo's first real request queues behind the compile
        # and dies on the 5s steady timeout (CI 27271079427, sweep
        # slot 1). Readiness therefore = "the first windowed paint
        # completed": poll `scene/cache_stats`, which the shell
        # publishes only after a paint cycle, under the same boot
        # budget — a poll that lands mid-compile simply waits, and a
        # data response proves the loop survived a full frame.
        steady_timeout = self.request_timeout
        self.request_timeout = max(steady_timeout, self.boot_timeout)
        try:
            stale_binary = False
            try:
                self.pointer_leave()
            except RpcError as exc:
                if exc.code != -32601:
                    raise
                # Pre-R719 target/release binary — no baseline, and no
                # cache_stats method either; drive it best-effort.
                stale_binary = True
            if not stale_binary:
                deadline = time.monotonic() + self.boot_timeout
                while True:
                    try:
                        self.request("scene/cache_stats", {})
                        break
                    except RpcError as exc:
                        if exc.code == -32601:
                            break  # stale pre-R682 binary — best-effort
                        not_painted = exc.code == -32602 and "unavailable" in (
                            f"{exc.data or ''}{exc.message or ''}".lower()
                        )
                        if not not_painted:
                            raise
                        if time.monotonic() >= deadline:
                            raise RpcError(
                                -32099,
                                "first paint never completed within "
                                f"boot_timeout={self.boot_timeout}s",
                                "\n".join(self._stderr_lines[-20:]),
                            ) from exc
                        time.sleep(0.05)
        finally:
            self.request_timeout = steady_timeout
        # R1650 wrote these gates their own teardown guard; R1672 moved that
        # guard out to `__enter__` itself, where it covers the boot baseline
        # above as well. What a leaked process does is on record: R1570.3
        # measured 14 refusals leaving 14 live binaries holding GPU contexts,
        # and the next launch died with "Not enough memory left" instead of its
        # own verdict.
        self._gate_pointer_reach()
        self._gate_text_smear()
        self._gate_font_pin()
        self._gate_containment()
        self._gate_stated_reasons()
        self._gate_scroll_reach()
        self._gate_tag_rects_agree()
        self._gate_voice_census()
        self._gate_aria_structure()
        self._gate_pointer_targets()
        return self

    def _gate_pointer_reach(self) -> None:
        """R1650 §5.35 — refuse to drive a surface a real pointer cannot.

        Every demo pays this, at boot, because the failure it catches is
        invisible to everything a demo does afterwards: `scene/click`,
        `scene/invoke` and `send` reach a widget's handler by name, while a
        mouse reaches it by POSITION through the §5.35 router — so a screen
        whose widgets are covered by tagged decoration is dead to a person and
        green to a script. Measured twice before anything checked it (R1497's
        header cells, R1649.1's whole shell), and a third time by this gate on
        its first run.

        Two kinds, and only one is fatal here:

        * `blocked_by: <tag>` — a painted node is covering the widget. The
          repair is one declaration (`pointer_transparent`) at the covering
          node, so this FAILS the demo: it is a defect with a known fix and no
          design question attached.
        * `blocked_by: null` — the widget is painted outside the window with
          nothing at that point at all. The repair is a scroll region, which is
          a layout decision rather than a slip, so this is REPORTED and does not
          fail. `debt-the-analyzer-canvas-does-not-scroll` is that class.

        A binary too old to answer the method is driven without the gate, the
        same tolerance the boot baseline gives.
        """
        try:
            resp = self.request("scene/pointer_reach")
        except RpcError as exc:
            if exc.code == -32601:
                return  # stale binary, predates the method
            raise
        assert resp is not None
        reach = resp.result
        unreachable = reach.get("unreachable", [])
        covered = [u for u in unreachable if u.get("blocked_by") is not None]
        offscreen = [u for u in unreachable if u.get("blocked_by") is None]
        budgeted = _pointer_reach_budget().get(self.example, frozenset())
        allowed = budgeted | set(self.pointer_reach_exempt)
        fatal = [u for u in covered if u["tag"] not in allowed]
        if fatal:
            rows = "; ".join(
                f"{u['tag']} is covered by {u['blocked_by']} (at {u['path'] or '/'})"
                for u in fatal
            )
            raise AssertionError(
                f"{self.example}: {len(fatal)} widget(s) cannot be pressed where "
                f"they are painted, so this screen is dead to a real mouse while "
                f"every wire-driven assertion below would pass — {rows}. "
                "Two repairs, and which one is right depends on what the "
                "covering node IS: pure decoration over a widget takes "
                "`pointer_transparent` (the reference toolkit's "
                "WA_TransparentForMouseEvents, CSS's pointer-events: none), "
                "while a real sub-region of the widget takes a COMPOSITE tag "
                "`widget#sub`, which keeps it hit-testable so a cursor hint on "
                "it still works. `docs/pointer-reach-budget.tsv` carries the "
                "measured backlog; adding a row to silence this is the one use "
                "that turns the ratchet back into a suggestion."
            )
        # ★ R1664 — the half this gate PRINTED and never enforced.
        #
        # R1663 shipped `hello-packet-view` with `deliverable=0 inert=30`, this
        # gate printed exactly that line, the sweep went green, and a person
        # opened the window, pressed things, and reported that nothing happened.
        # `deliverable=0` was not information; it was the diagnosis. What made it
        # unreadable is that it is byte-identical to the honest report of a screen
        # with no widgets on it — the same "no entries reads as zero" collapse
        # R1661/R1662 fixed in the budget files, one layer up.
        #
        # `dead_to_a_pointer` is the runtime's own verdict over the roster of
        # registered widgets (all unrouted, roster non-empty), so this gate reads
        # a decision rather than re-deriving one from a count it can misread. A
        # binary too old to publish it is driven without the check, the same
        # tolerance the rest of this method gives.
        if reach.get("dead_to_a_pointer") and not self.measuring:
            unrouted = [
                e["tag"] for e in reach.get("externals", []) if e.get("routed_by") is None
            ]
            declared = _pointer_routing_budget().get(self.example)
            if declared is not None:
                print(
                    f"[pointer-routing] {self.example}: routes no press at boot, "
                    f"declared — {declared}"
                )
                return
            raise AssertionError(
                f"{self.example}: this screen is dead to a real mouse — it paints "
                f"{reach.get('deliverable', 0) + reach.get('inert', 0)} tagged node(s) "
                f"and registers {len(unrouted)} widget(s), and NOT ONE of them can "
                f"receive a press anywhere in the window: {', '.join(unrouted[:6])}. "
                "The router resolves the deepest tagged node under the cursor and "
                "looks its primary half up as an `External`; when no painted tag "
                "carries a registered name, every press is dropped in silence while "
                "`scene/click {path}`, `scene/invoke` and `send` all keep working, "
                "because those call the handler by name and never ask the router. "
                "The repair is to paint the widget's surface under the tag it is "
                "registered with (`scene/pointer_reach`.externals names both sides). "
                "This is not budgetable: a screen nobody can press is not a backlog "
                "item."
            )
        note = ""
        if reach.get("shadows"):
            note += f" shadows={len(reach['shadows'])}"
        if offscreen:
            note += f" off-window={len(offscreen)} ({', '.join(u['tag'] for u in offscreen[:3])}…)"
        if allowed:
            note += f" budgeted={len(allowed)}"
        unrouted = [e["tag"] for e in reach.get("externals", []) if e.get("routed_by") is None]
        if unrouted:
            note += f" unrouted={len(unrouted)}"
        print(
            f"[pointer-reach] {self.example}: "
            f"deliverable={reach.get('deliverable', 0)} inert={reach.get('inert', 0)}{note}"
        )

    def _gate_pointer_targets(self) -> None:
        """★★★★★ R1700 §5.15 §5.35 — refuse a surface that disagrees with its
        own paint about what is where.

        `_gate_pointer_reach` above covers the REGISTERED widgets, and §2 #7
        makes a pinion screen ONE `External`, so on the analyser's capture
        viewer that gate vouches for 1 of the 291 tagged rectangles on screen.
        The other 290 are resolved by the screen's own hit test, and nothing
        held that against what the screen actually painted.

        `scene/pointer_target` asks the surface twice — what the rectangle
        under a tag addresses, and what a press inside that rectangle addresses
        — and classifies the pair. One of the five verdicts has no benign
        reading and is refused here:

        * `unreachable` — addressable by name, and addressable at NO point
          inside its own painted rectangle. This is what a screen looks like
          when its paint and its hit test have come to read different facts.

        `deliverable` (pressable at its centre), `handle` (a group gripped by
        its tab strip), `covering` (decoration over what it decorates) and
        `inert` (a caption) all pass, because all four are correct. ★ `handle`
        exists because the first draft of this gate probed only the centre and
        its first run called the node lab's two host frames defective — a group
        whose grip is its top strip and which holds its members everywhere else
        is right, and it was the rule that was wrong.

        A surface that does not implement the pair is listed in `unanswered`
        and printed rather than counted as clean — "did not answer" is not
        "answered nothing", and collapsing the two would let a screen nobody
        checked read as a screen that checked out.

        ★ This runs at the size the demo booted at, so on its own it cannot see
        the defect it was written for: at the design size the capture viewer
        agreed with itself, and it was a resize that made 166 rectangles
        unreachable. `assert_targets_survive_resize` is the other half, and it
        is a helper rather than a gate because resizing a screen is not
        something every demo should have done to it on the way in.

        A binary too old to answer the method is driven without the gate, the
        same tolerance the boot baseline gives.
        """
        try:
            resp = self.request("scene/pointer_target")
        except RpcError as exc:
            if exc.code == -32601:
                return  # stale binary, predates the method
            raise
        assert resp is not None
        report = resp.result
        answered = [s for s in report.get("surfaces", []) if s.get("answers")]
        if not answered and not report.get("unanswered"):
            return  # nothing painted yet
        if report.get("defects"):
            # ★★★★★ R1953 — the rows come from the report's OWN `defect` field.
            #
            # This filtered `verdict == "unreachable"`, which was the whole rule
            # when it was written and stopped being one round later: R1736 added
            # `astray` and taught `TargetVerdict::is_defect` about it, and this
            # spelling stayed. Measured at R1953 on the two sections R1947 and
            # R1948 built — three astray rectangles, and this gate refused with
            # "3 painted rectangle(s) ... — .", naming none of them, because
            # every defect was of the kind the filter did not know.
            #
            # A gate that re-spells its publisher's rule is a second oracle, and
            # the one that goes stale is the copy nobody runs the mutation on.
            rows = [
                f"{row['tag']} is {row['verdict']}: says {row['by_name']!r} by "
                f"name, {row['at_centre']!r} at its centre, and "
                f"{row.get('astray_to')!r} at ({row['x']},{row['y']})"
                for surface in answered
                for row in surface["rows"]
                # `.get` for a binary older than the field, which the boot
                # baseline already tolerates two lines up; falling back to the
                # stale spelling keeps such a binary reporting SOMETHING rather
                # than reporting nothing quietly.
                if row.get("defect", row["verdict"] == "unreachable")
            ]
            raise AssertionError(
                f"{self.example}: {report['defects']} painted rectangle(s) where "
                f"this screen disagrees with its own paint about what is there — "
                f"{'; '.join(rows[:6])}. A rectangle that is addressable by name "
                f"and addressable at no point inside itself is dead to a real "
                f"mouse while every wire-driven assertion below keeps passing, "
                f"because those address the handler by name and never ask the "
                f"geometry. The usual cause is the paint and the hit test "
                f"reading two facts — most often two window sizes; "
                f"`pinion_core::external::layout_size` is the one spelling of "
                f"that question."
            )
        if not answered:
            return
        line = " ".join(
            f"{s['surface']}={s['deliverable']}+{s['handle']}/{s['painted']}"
            for s in answered
        )
        note = f" unanswered={len(report['unanswered'])}" if report.get("unanswered") else ""
        # ★ R1700 — reported, not judged, and the distinction is deliberate.
        # `assert_targets_survive_resize` FAILS on this; here it is printed,
        # because this gate runs on every demo in the tree and the agreement
        # between what a surface is painted at and what it is told has been
        # measured on three of them. A surface that disagrees is named on every
        # run of every demo that mounts it, which is how the population gets
        # measured rather than assumed.
        told = [
            s["surface"]
            for s in report.get("surfaces", [])
            if s.get("announced") is not None
            and tuple(s["announced"]) != tuple(s["painted_size"])
        ]
        if told:
            note += f" MISANNOUNCED={','.join(told)}"
        print(f"[pointer-target] {self.example}: {line} deliverable+handle{note}")

    def _gate_text_smear(self) -> None:
        """R1654 §5.36 — refuse a screen whose text is painted over itself.

        Two runs of ONE widget landing on each other is the signature of a run
        that flowed instead of being placed, and of a box too small for the
        string it promised. Both shipped: R1649 stacked a whole shell's card
        text down the left edge with 118 wire assertions passing, and R1653
        found the same thing in a screen four rounds had called a reproduction.
        Neither was visible to anything else, because **a text run carries no
        tag** and every other gate in this tree is tag-keyed.

        Grouped by the run's OWNER — its nearest tagged ancestor — because a
        floating annotation over a diagram is a design and two labels of one row
        on top of each other is a defect. The read is `scene/text_painted`,
        which is the only surface that reports a run's window rectangle at all.

        A binary too old to answer the method is driven without the gate, the
        same tolerance the boot baseline gives.
        """
        if self.measuring:
            return  # the producer is not judged by the file it produces
        try:
            resp = self.request("scene/text_painted")
        except RpcError as exc:
            if exc.code in (-32601, -32602):
                return  # stale binary, or an embedder that cannot shape
            raise
        assert resp is not None
        runs = resp.result.get("runs", [])
        by_owner: dict[str, list[dict]] = {}
        for run in runs:
            by_owner.setdefault(run.get("owner") or "", []).append(run)
        smeared = []
        for owner, group in by_owner.items():
            for i, a in enumerate(group):
                for b in group[i + 1:]:
                    if (
                        a["x"] < b["x"] + b["w"]
                        and b["x"] < a["x"] + a["w"]
                        and a["y"] < b["y"] + b["h"]
                        and b["y"] < a["y"] + a["h"]
                    ):
                        smeared.append((owner, a["content"], b["content"]))
        allowed = _budget_for(
            "text-smear-budget.tsv", self.example, "text-smear", "smear"
        )
        if allowed is None:
            return
        if len(smeared) > allowed:
            rows = "; ".join(f"{o or '<root>'}: {a!r} over {b!r}" for o, a, b in smeared[:6])
            raise AssertionError(
                f"{self.example}: {len(smeared)} pair(s) of text runs are "
                f"painted on top of each other and the budget allows "
                f"{allowed} — {rows}. A run with no `LayoutStyle` FLOWS (its "
                f"rect reads like a position and is not one), and a run whose "
                f"box is narrower than its string wraps onto the row below "
                f"unless its style declares a `TextOverflow` that shortens it. "
                f"`docs/text-smear-budget.tsv` carries the measured backlog; "
                f"raising a number there to silence this is the one use that "
                f"turns the ratchet back into a suggestion."
            )
        if smeared or allowed:
            print(
                f"[text-smear] {self.example}: {len(smeared)} overlapping "
                f"pair(s), budget {allowed}"
            )

    def _gate_containment(self) -> None:
        """R1656 §5.32 §5.36 — refuse a screen that paints outside its own boxes.

        Every demo pays this at boot, because the class it catches is invisible
        to everything a demo does afterwards. A rectangle in a scene is a
        promise; every other read here reports the promise, and this is the only
        one that reports whether it was kept. Measured on the screen it was
        written for: seven of eight node cards painted their last field row
        three to five pixels below their own border, at the size the app opens
        in, with the round's own 8-state painted-spec sweep green — because the
        card's parts were SIBLINGS of the card, so "is this run inside its
        owner" was being asked about the canvas.

        `clipped` counts too. An overhang a clip cuts away loses the reader the
        content with nothing saying so, which is a different repair (elide,
        wrap, scroll) and the same defect.

        A binary too old to answer the method is driven without the gate, the
        same tolerance the boot baseline gives.
        """
        if self.measuring:
            return  # the producer is not judged by the file it produces
        try:
            resp = self.request("scene/containment")
        except RpcError as exc:
            if exc.code in (-32601, -32602):
                return  # stale binary, or a host that cannot shape
            raise
        assert resp is not None
        out = resp.result
        escapes = out.get("escapes", [])
        allowed = _budget_for(
            "containment-budget.tsv", self.example, "containment", "containment"
        )
        if allowed is None:
            return
        if len(escapes) > allowed:
            rows = "; ".join(
                f"{(e.get('content') or e.get('tag') or '<a mark>')!r} is "
                f"{max(e['over'].values())}px outside {e['owner']} ({e['fate']})"
                for e in escapes[:6]
            )
            raise AssertionError(
                f"{self.example}: {len(escapes)} painted mark(s) are outside "
                f"the box that owns them and the budget allows {allowed} — "
                f"{rows}. Two shapes cause almost all of it: a box whose size "
                f"is a CONSTANT while its contents are derived (the repair is "
                f"to derive the box from the rows it paints, so a row that does "
                f"not fit is not expressible), and a text box authored at the "
                f"font size rather than at its LINE box (the shaper's line for "
                f"a 12px face is 21px). `docs/containment-budget.tsv` carries "
                f"the measured backlog; raising a number there to silence this "
                f"is the one use that turns the ratchet back into a suggestion."
            )
        if escapes or allowed:
            print(
                f"[containment] {self.example}: {len(escapes)} escape(s) "
                f"(smeared {out.get('smeared', 0)} / clipped "
                f"{out.get('clipped', 0)}) of {out.get('marks', 0)} marks, "
                f"budget {allowed}"
            )

    def _gate_stated_reasons(self) -> None:
        """R1669 — every inert region on the opening screen says WHY it is inert.

        `scene/disabled` has published a reason since R1668, and
        `UnavailableKind::Unstated` is deliberately an ARM rather than an
        absence so that "declared inert, said nothing" is a number somebody can
        count. Nothing counted it, which is the debt this closes: an arm nobody
        reports is an `Option::None` with extra steps.

        The count is taken from the RUNNING screen rather than from a source
        scan, and that difference is the whole reliability of it. A grep for the
        reasonless builder answers 23 in this tree, of which 18 are test
        fixtures, 3 are prose and one is the builder's own definition — the real
        production population is 1, and a text census could not tell those
        apart. What paints is what is asked.

        Held at zero rather than ratcheted from a backlog: the measurement was
        taken with the one production site repaired, so there is nothing to
        carry. A surface that legitimately cannot say why is added to
        `_STATED_REASON_EXCEPTIONS` with the reason written beside it, and that
        list is empty.

        A binary too old to answer the method is driven without the gate, the
        same tolerance the boot baseline gives.
        """
        if self.measuring:
            return  # the producer is not judged by the file it produces
        try:
            resp = self.request("scene/disabled")
        except RpcError as exc:
            if exc.code in (-32601, -32602):
                return  # stale binary
            raise
        assert resp is not None
        rows = resp.result.get("disabled", [])
        # A row whose reason is `unstated` was declared inert by something that
        # knew why and did not say. A row with any other kind is fine at any
        # count -- this gate is about the SILENCE, not about how much is inert.
        silent = [r for r in rows if r.get("reason") == "unstated"]
        # ★ The floor is ZERO for every surface, and the file is an EXCEPTION
        # list rather than a census. The other budgets carry a measured backlog
        # so they have to be total -- a missing row there would hide work. This
        # one has no backlog to carry (the population was measured at one and
        # repaired in the same round), so a default of zero is the honest floor
        # and a file that has to be generated over 224 demos would only be a way
        # for the gate to go UNARMED.
        allowed = _STATED_REASON_EXCEPTIONS.get(self.example, 0)
        if len(silent) > allowed:
            named = "; ".join(
                f"{r['tag']} (declared by {r.get('declared_by') or 'itself'})"
                for r in silent[:6]
            )
            raise AssertionError(
                f"{self.example}: {len(silent)} inert region(s) say nothing "
                f"about why, and the budget allows {allowed} — {named}. "
                f"`with_disabled(true)` states the fact and no reason; "
                f"`with_unavailable(..)` / `with_availability(..)` state both, "
                f"and the reason is what reaches `scene/disabled`, the "
                f"accessibility tree's state description, and the person "
                f"looking at a greyed control wondering what to do about it."
            )
        unheard = self._unheard_reasons(rows)
        if unheard:
            named = "; ".join(
                f"{tag} ({why})" for tag, why in unheard[:6]
            )
            raise AssertionError(
                f"{self.example}: {len(unheard)} region(s) state WHY they are "
                f"inert and no reader is told — {named}. A reason on "
                f"`scene/disabled` that never reaches `scene/access` is a "
                f"sentence written for somebody who cannot receive it. A region "
                f"the cascade declares unavailable owes an accessibility node: "
                f"announce it (`WidgetA11y::access_node`) and the assembler "
                f"relays the declared reason onto it — there is nothing to "
                f"restate. Measured tree-wide before this gate was armed: "
                f"13 stated reasons over 215 surfaces, 11 of them unheard, all "
                f"on one screen."
            )
        if rows:
            kinds = sorted({r.get("reason", "?") for r in rows})
            print(
                f"[stated-reason] {self.example}: {len(rows)} inert region(s), "
                f"{len(silent)} silent (budget {allowed}), {len(rows)} heard, "
                f"kinds {kinds}"
            )

    def _unheard_reasons(self, rows: list) -> "list[tuple[str, str]]":
        """★★★★★ R1694 — every stated reason that reaches no reader.

        R1668 gave the framework a reason a disabled region can carry — a kind,
        a detail, and a recourse derived from the kind — and R1669 counted the
        regions that state one. Neither asked the next question: **does the
        reason reach the accessibility tree**, which is the audience it was
        built for and which the gate above names in its own failure text as a
        destination. It did not, and nothing said so.

        Measured over 215 running surfaces the day this was written: **13**
        regions state a reason, and **11** of them had no accessibility node at
        all — every one on the dashboard, whose whole subject is that nine seats
        are locked and each says what it is booked under. The reason was
        computed, published on the wire, painted as faded ink, and inaudible.

        Two ways to fail, because the repairs differ: a region with no node
        wants announcing, and a node that lost the reason wants the assembler
        looked at. The relay itself is automatic — `stamp_inherited_disabled`
        copies the resolved reason onto whatever node carries the tag — so a
        screen never restates it, and this gate is what notices when a region
        is missing from the tree rather than when somebody forgot to copy it.

        Zero for every surface, with no exception list: the population is the
        regions a screen ITSELF declared unavailable, so a surface that declares
        none is silent here.

        A row whose reason is `unstated` is **not** in the population. It is
        already the other half of this gate's failure, and reporting one region
        under two arms would make a single repair look like two — the mistake
        R1693 wrote down about adding an arm for a defect an existing arm
        already names.
        """
        rows = [r for r in rows if r.get("reason") not in (None, "unstated")]
        if not rows:
            return []
        try:
            resp = self.request("scene/access")
        except RpcError as exc:
            if exc.code in (-32601, -32602):
                return []  # stale binary
            raise
        assert resp is not None
        nodes = {n.get("tag"): n for n in resp.result.get("nodes", [])}
        out: list[tuple[str, str]] = []
        for row in rows:
            node = nodes.get(row["tag"])
            if node is None:
                out.append((row["tag"], "no accessibility node at all"))
            elif node.get("unavailable") is None:
                out.append((row["tag"], f"announced as {node.get('role')}, reason dropped"))
        return out

    def _gate_font_pin(self) -> None:
        """R1662 — the font pin is actually ON, and the screen still has text.

        ★ The defect this closes was written down by the round that created the
        pin and then not built: nothing asks whether the pin took. If
        `FONTCONFIG_FILE` is unset, or points somewhere else, or the config is
        malformed, every ink measurement quietly goes back to being a fact about
        this host's font database — the exact failure R1660 spent a round
        reproducing, silently reintroduced.

        Two halves, because R1660 measured that one is not enough. Its FIRST
        repair pinned the shape cache instead of the database and made three
        hosts agree by painting **no text at all**: zero runs, zero escapes,
        green for the wrong reason. So this asserts both that the database is
        the pinned one and that a surface which shapes text still shapes some.

        Skipped when the caller pinned its own fontconfig (the font-source
        demos, which are ABOUT the database and set it deliberately) and when
        `fc-list` is absent — an infrastructure absence is not evidence, but it
        prints, because a check that silently stopped happening is the thing
        these gates exist to prevent.
        """
        conf = self._env_fontconfig
        if conf is None:
            return  # the caller owns the database; this is its subject
        listed = _fc_list_files(conf)
        if listed is None:
            print(f"[font-pin] {self.example}: fc-list is absent — the database "
                  f"cannot be verified from here")
        else:
            want = {Path(rel).name for rel in PINNED_FACES}
            extra = sorted(f for f in listed if f not in want)
            if extra or not listed:
                raise AssertionError(
                    f"{self.example}: the pinned fontconfig at {conf} resolves "
                    f"the face file(s) {sorted(listed) or 'nothing'} and the pin "
                    f"is {sorted(want)}. Every ink budget in this tree is measured "
                    f"through that database, so a pin that did not take turns "
                    f"the ratchets back into facts about whichever machine ran "
                    f"them — which is the failure R1660 spent a round "
                    f"reproducing."
                )
        try:
            painted = self.request("scene/text_painted")
        except RpcError as exc:
            if exc.code in (-32601, -32602):
                return  # stale binary
            raise
        assert painted is not None
        runs = painted.result.get("runs", [])
        if not runs:
            return  # a surface with no text is allowed to have none
        inked = [r for r in runs if r.get("w") and r.get("h")]
        if not inked:
            raise AssertionError(
                f"{self.example}: {len(runs)} text run(s) and not one of them "
                f"has ink. A pin that leaves the shaper with nothing to find "
                f"makes every ink gate pass by painting an empty screen — "
                f"R1660's first repair did exactly this and read as green."
            )

    def _gate_scroll_reach(self) -> None:
        """R1662 — nothing on the opening screen is out of the reader's reach.

        For every painted mark the framework answers one of four things: it is
        on screen, some offset of an enclosing viewport brings it there, some
        offset brings PART of it there (R1713 `clipped`), or nothing brings any
        of it. Only the last is a defect, and until this gate existed it was
        indistinguishable from the second — a control below the fold of a pane
        that scrolls and a control below the fold of a pane that does not both
        simply stopped being painted.

        ★ R1713 — the third answer is why the count this gate reads moved. A
        row whose right edge a narrowed pane cuts off used to be `lost`; it is
        `clipped` now, and a glyph nothing reaches at all is still `lost`. The
        budget therefore counts a strictly sharper thing than it did.

        The window is a viewport whose range is zero, so a screen with no
        scrolling panes is judged too: anything it paints past the window edge
        is lost, which is exactly the finding this gate was built from.

        A binary too old to answer the method is driven without the gate, the
        same tolerance the boot baseline gives.
        """
        if self.measuring:
            return  # the producer is not judged by the file it produces
        try:
            resp = self.request("scene/scroll_reach")
        except RpcError as exc:
            if exc.code in (-32601, -32602):
                return  # stale binary
            raise
        assert resp is not None
        out = resp.result
        lost = [o for o in out.get("out_of_sight", []) if o.get("reach") == "lost"]
        allowed = _budget_for(
            "scroll-reach-budget.tsv", self.example, "scroll-reach", "reach"
        )
        if allowed is None:
            return
        if len(lost) > allowed:
            rows = "; ".join(
                f"{(o.get('content') or o.get('tag') or '<a mark>')!r} is past "
                f"{o['viewport']['name']} by {max(o.get('short_by') or [0])}px "
                f"(viewport {o['viewport']['w']}x{o['viewport']['h']}, content "
                f"{o['viewport']['content_w']}x{o['viewport']['content_h']})"
                for o in lost[:6]
            )
            raise AssertionError(
                f"{self.example}: {len(lost)} painted mark(s) cannot be brought "
                f"into view by any gesture and the budget allows {allowed} — "
                f"{rows}. The repair is a scrolling pane whose range is derived "
                f"from its content (`pinion_widget_paint::pane::scroll_pane`), "
                f"not a bigger window: a window a person can resize down puts "
                f"the content back outside. `docs/scroll-reach-budget.tsv` "
                f"carries the measured backlog; raising a number there to "
                f"silence this is the one use that turns the ratchet back into "
                f"a suggestion."
            )
        # ★★★★★ R1971 — the class that reached NO report before this round, and
        # which this gate would have waved through for the same reason it was
        # invisible: a filter on `reach == "lost"` does not see `"unplaced"`.
        #
        # Budgeted at ZERO with no per-example allowance, deliberately. The
        # backlog file exists because `lost` had a measured population when its
        # ratchet was built; this one's population was measured at 0 on the
        # screen that had eight of them a round ago, so a budget would only be a
        # place for the next one to be filed instead of fixed.
        unplaced = [
            o for o in out.get("out_of_sight", []) if o.get("reach") == "unplaced"
        ]
        if unplaced:
            # ★ R1971 — the PATH is the fallback, not a placeholder. Measured on
            # the shell screen with a placement removed: every one of the eleven
            # was untagged, so a message that stopped at tag-or-content printed
            # `'<a mark>'` six times and told the reader nothing about WHICH.
            # `scene/locate` answers on this address, so the reader can go there.
            # ★★★★★ R2030 — AND THE BOX IT SAW, which this line did not carry.
            #
            # R2025 wrote this refusal and named the mark; R2030 tried to repair
            # one of its findings and could not even ask what the rectangle was,
            # because the gate raises inside `__enter__` and the answer it had
            # measured never reached the reader. Two wrong repairs were shipped
            # and reverted before that was noticed. A refusal that names a mark
            # and withholds the measurement makes every reader re-derive it —
            # and here the axis IS the diagnosis: a zero width and a zero height
            # come from different halves of a layout.
            # ★★★★★ R2034 — and the NAME says which FIELD it came from.
            #
            # The first draft folded tag / content / path into one `or` chain,
            # so a mark printed as `'1'` could be a node tagged "1", a text run
            # reading "1", or the second child of something — three different
            # repairs. R2031 lost three rounds to exactly that ambiguity one
            # field over (`"tag": null` on a path named after a scroll), and
            # the round that read this line next had to neuter the gate to find
            # out which it was. A refusal that hides which question it answered
            # sends its reader to re-measure what the gate already knew.
            def named(o: dict) -> str:
                parts = [f"tag {o['tag']!r}" if o.get("tag") else "UNTAGGED"]
                if o.get("content"):
                    parts.append(f"reading {o['content']!r}")
                parts.append(f"at path {o.get('path')!r}")
                return " ".join(parts)

            rows = "; ".join(
                f"{named(o)}"
                f" (rect {o.get('rect')}, in viewport {o.get('viewport', {}).get('name')!r}"
                f" {o.get('viewport', {}).get('w')}x{o.get('viewport', {}).get('h')})"
                for o in unplaced[:6]
            )
            # ★★★★★ R2025 — REFUSED, and this line is the whole of
            # `debt-a-zero-box-does-not-say-who-made-it-zero`.
            #
            # R1971 could only PRINT here. Its first draft raised, on the
            # measured claim that the population was zero; run over ONE DEMO
            # PER EXAMPLE — 113 of them, derived from the demos' own `EXAMPLE`
            # declarations — it was not zero, and reading the thirteen split
            # into idioms rather than defects. After the layout pass a box the
            # author DECLARED zero and one the pass DENIED were the same
            # rectangle, and nothing on this wire told them apart, so a gate
            # that failed anyway would have reported a convention as a defect.
            #
            # R2025 put the difference where it can be read — on the NODE,
            # before the rectangle is all that is left — and `reach` now
            # carries two words. `unplaced` is the arm the walk is willing to
            # call a defect; `unjudged` below is the one it is not. So this
            # refuses, and the class stops being advisory.
            raise AssertionError(
                f"[unplaced] {self.example}: {len(unplaced)} mark(s) carry a "
                f"name and NO BOX that nobody asked to have none — {rows}. A "
                f"primitive whose own `rect` holds its geometry must be placed "
                f"with `absolute(rect)`; put in flow, the layout pass "
                f"overwrites that rect with the flow box and every index built "
                f"from `absolute_rect` drops it. A zero the author DECLARED, or "
                f"a node this walk cannot see inside, is reported as "
                f"`unjudged` instead and does not reach here."
            )
        # ★★★★★ R2025 — the half the walk DECLINED to judge, kept visible.
        #
        # An admission rather than an excuse: `opaque` says the framework could
        # not ask a foreign surface whether it had anything to draw, so a real
        # defect can sit in this list. Printing it is what keeps that owed
        # number in front of a reader instead of turning the split into a
        # silence — which is the failure mode a class getting its own arm
        # invites, and the one R1971's own report was written against.
        unjudged = [
            o for o in out.get("out_of_sight", []) if o.get("reach") == "unjudged"
        ]
        if unjudged:
            causes = collections.Counter(o.get("why") or "?" for o in unjudged)
            print(
                f"[unjudged] {self.example}: {len(unjudged)} boxless mark(s) "
                f"this walk did not judge — "
                + ", ".join(f"{n} {why}" for why, n in sorted(causes.items()))
                + ". `declared` is the author's own zero; `opaque` is a node "
                "whose content the framework cannot see into, and a defect can "
                "hide there. `unshaped` is a run on a process that PROVED it "
                "holds no face — every run measures nothing there, which is one "
                "fact about the host and not a placement defect per mark."
            )
        if lost or allowed or out.get("scrollable") or out.get("clipped"):
            print(
                f"[scroll-reach] {self.example}: {len(lost)} lost, "
                f"{len(unplaced)} unplaced, {len(unjudged)} unjudged, "
                f"{out.get('clipped', 0)} reachable in part, "
                f"{out.get('scrollable', 0)} one scroll away, of "
                f"{out.get('marks', 0)} marks, budget {allowed}"
            )

    def _gate_tag_rects_agree(self) -> None:
        """★★ R1676 — the harness and the framework answer the same geometry.

        `abs_rects_of` deliberately re-derives where every tagged mark is
        painted rather than asking, and that independence is load-bearing: it
        is what makes a focus-ring assertion a comparison instead of a
        tautology (R705.1). What was missing is the other half of independence
        — nothing ever checked that the second implementation AGREED with the
        first.

        It did not, in two ways at once, for as long as both existed. The
        mirror folded each `Scroll`'s offset and not its clip, so a mark the
        viewport cuts was reported at its full width and a mark scrolled out of
        sight was reported at a rectangle off-screen; and it let a LATER
        duplicate tag overwrite an earlier one where the framework keeps the
        first. Both are invisible to every assertion a demo makes afterwards,
        because a demo asks this map where to press and then presses there: the
        press lands on nothing, the widget never arms, and the failure surfaces
        as a value that did not change — sixty lines later, naming neither the
        press nor the geometry. Three demos were red for exactly that.

        Full coverage in one round trip, so this is not a sample: the wire
        enumerates EVERY tag, and the two maps are compared whole.

        A binary too old to answer the method is driven without the gate, the
        same tolerance the boot baseline gives.
        """
        try:
            resp = self.request("scene/tag_rects")
        except RpcError as exc:
            if exc.code in (-32601, -32602):
                return  # stale binary, or no painted frame to enumerate
            raise
        assert resp is not None
        wire: dict[str, tuple[int, int, int, int]] = {}
        for row in resp.result.get("tags", []):
            win = row.get("window")
            if win is not None:
                wire[row["tag"]] = (win["x"], win["y"], win["w"], win["h"])
        mine = abs_rects_of(self.snapshot(source="paint"))
        if mine == wire:
            return
        rows = []
        for tag in sorted(set(mine) | set(wire))[:200]:
            if mine.get(tag) != wire.get(tag):
                rows.append(f"{tag}: harness {mine.get(tag)} vs framework {wire.get(tag)}")
        raise AssertionError(
            f"{self.example}: {len(rows)} tag(s) where this harness and the "
            f"framework disagree about where a mark is painted — "
            f"{'; '.join(rows[:6])}. The framework is the authority: its answer "
            f"is the one `Scene::rect_for_tag_absolute` routes a press by, so a "
            f"demo pressing the harness's rectangle presses somewhere the app "
            f"never looks. `None` on the framework side means the mark is "
            f"painted but wholly clipped away, and the harness must then not "
            f"report it at all."
        )

    def _gate_voice_census(self) -> None:
        """★★★★★ R1692 — every surface is told what a reader is told, and the
        four defects that need no judgment are refused.

        `scene/voice` has classified every addressable region since R1691 and
        exactly one screen ever asked it. The other 196 surfaces' numbers were
        not "unmeasured" — they were *unrequested*, which is the failure mode
        this whole family of gates exists to end.

        What is refused is derived rather than listed: **every arm the census
        publishes except the two correct outcomes and the one this round
        deliberately defers**. Today that is the part needing **no per-screen
        judgment**:

        * `mumbled` — a node whose name says nothing, is the tag, or has no
          letter in it. Nobody has to decide whether that is wrong.
        * `hollow` — a box declared quiet *because its children speak*, over a
          subtree where nothing does.
        * `dangling` — a silence that hands a reader to a node which is not in
          the tree.
        * `ghost` — a name with nothing painted, referring to it, or composing
          it.

        What is only **reported** is `unvoiced`: a painted region nobody decided
        about. Refusing those needs a screen-by-screen answer to "which regions
        owe a reader anything", which is a round of work per screen and not a
        thing a boot gate can assume. It prints on every run so the backlog is a
        number somebody can see falling — measured the day this landed, 1376 of
        2260 regions across the surfaces that answer.

        Prints unconditionally, defects or none. The debt this closes was
        created by a surface that answered and was never asked.

        ## What it does NOT see, stated because nothing checks it

        **The opening screen only.** Every one of the 42 defects this found was
        already on screen at boot; a region that exists only after an act — a
        toast, a dialog, a row a chip added — is invisible here. That is not
        hypothetical: R1691 measured its own screen's transient message as
        inaudible, and only a DRIVEN act found it. Re-running the census at
        teardown would double the coverage for about ten lines, and is filed as
        `debt-the-voice-gate-judges-only-the-opening-screen` rather than done
        here, because landing it means re-sweeping every demo to find the ones
        that legitimately end in a state this would newly refuse.
        """
        if self.measuring:
            return  # the producer is not judged by the file it produces
        try:
            resp = self.request("scene/voice")
        except RpcError as exc:
            if exc.code in (-32601, -32602):
                return  # stale binary
            raise
        assert resp is not None
        out = resp.result
        counts = out.get("counts", {})
        # ★ Derived from the arms the binary publishes, minus the two correct
        # outcomes and the one deliberately deferred. A hand-written list of
        # defect arms would judge the day it was written: R1692 added two arms
        # to a partition that had five, and a gate naming its own four would
        # have gone on ignoring whichever it had not heard of. A new arm is
        # judged by default, which is the right way round — an arm nobody added
        # to a list is a defect nobody refuses.
        judged = tuple(
            arm for arm in counts if arm not in ("announced", "silent", "unvoiced")
        )
        found = sum(counts.get(arm, 0) for arm in judged)
        allowed = _VOICE_DEFECT_EXCEPTIONS.get(self.example, 0)
        if found > allowed:
            rows = "; ".join(
                f"{r['tag']!r} is {r['voice']}"
                + (f" ({r['fault']}, said {r.get('name')!r})" if r.get("fault") else "")
                + (f" (its reason names {r.get('detail')!r})" if r["voice"] == "dangling" else "")
                for r in out.get("nodes", [])
                if r["voice"] in judged
            )
            raise AssertionError(
                f"{self.example}: {found} region(s) whose accessibility answer "
                f"is wrong on its own terms, and the budget allows {allowed} — "
                f"{rows}. A `mumbled` row wants a name a person can hear: a "
                f"decoration glyph painted before the label is declared with "
                f"`TextNode::with_role(TextRole::Presentational)` so the "
                f"name-from-contents walk steps past it, and a region with no "
                f"text of its own is named with `aria_label` / "
                f"`AccessNode::with_name`. A `hollow` box either holds "
                f"something that speaks or is not a `layout` silence. A "
                f"`ghost` is announced for a tag nothing paints — paint it, "
                f"compose it from what does, or stop emitting it."
            )
        # ★★★★★ R1856 — an arm that is REPORTED and not judged must at least be
        # NAMED.
        #
        # `unvoiced` is deliberately not refused here: the count is a backlog
        # across ~200 surfaces, and a workspace-wide zero would be a ratchet
        # rather than a gate. But until this round the report was a bare number,
        # so a screen that grew an undecided region said `6` and nothing else —
        # and the two demos that DO assert zero for their own screen failed with
        # `expected 0, got 6`, which names neither the regions nor the widget
        # that painted them. Reproducing R1851's six cost a hand-written probe
        # against a booted binary to learn they were one card's feed.
        #
        # Bounded at eight so a surface with a large backlog reports its shape
        # without burying the rest of the run; the count above is the total
        # either way.
        undecided = [r["tag"] for r in out.get("nodes", []) if r["voice"] == "unvoiced"]
        shown = ", ".join(undecided[:8]) + (" …" if len(undecided) > 8 else "")
        print(
            f"[voice] {self.example}: {out.get('total', 0)} region(s) — "
            f"{counts.get('announced', 0)} announced, "
            f"{counts.get('silent', 0)} declared quiet, "
            f"{counts.get('unvoiced', 0)} undecided (reported, not judged)"
            + (f" — {shown}" if undecided else "")
        )

    def _gate_aria_structure(self) -> None:
        """★★★★★ R1693 — the announced tree is one a reader can **walk**.

        `scene/voice` asks whether each painted region has a voice and
        `NameFault` asks whether that voice says anything usable. Both are about
        one node at a time, and neither can see a node that is perfect on its own
        terms and structurally a lie: a pane announcing `role = table`, named,
        with a rectangle a reader can land on, that holds no row.

        Measured across the surfaces that answer the day this landed, **16
        carried a violation** — small enough to close in the round that found it,
        which is what lets this gate refuse at a budget of zero instead of
        ratcheting. The reference analysis tool's screen B was two of them.

        Both arms are refused and neither needs a per-screen judgment:

        * `empty` — a collection owning none of the roles its own role promises,
          **and not declaring itself empty**. An empty list is a real state and
          ARIA has the vocabulary for it (`aria-setsize` / `aria-rowcount` /
          `aria-colcount` of zero), so the legitimate case is a declaration
          rather than an exception.
        * `stray` — a member outside the collection its role requires. Every
          check about the node itself passes and an assistive technology still
          cannot place it.

        Prints unconditionally, faults or none, including `judged`: a structural
        census over a tree with no collections in it is green, and "well formed"
        and "nothing to check" must not read alike.

        ## What it does NOT see, stated because nothing checks it

        The opening screen, like its two siblings — see
        `debt-the-voice-gate-judges-only-the-opening-screen`. A collection is
        emptied by *editing* (a filter that matches nothing, a folded branch), so
        this arm has more to gain from a teardown pass than either of the others.
        """
        if self.measuring:
            return  # the producer is not judged by the file it produces
        try:
            resp = self.request("scene/conform")
        except RpcError as exc:
            if exc.code in (-32601, -32602):
                return  # stale binary
            raise
        assert resp is not None
        out = resp.result
        counts = out.get("counts", {})
        found = sum(counts.values())
        allowed = _CONFORM_FAULT_EXCEPTIONS.get(self.example, 0)
        if found > allowed:
            rows = "; ".join(
                f"{r['tag']!r} ({r['role']}) is {r['fault']}, wanted "
                + "/".join(r.get("required", []))
                + (f" and is inside {r['found']}" if r.get("found") else "")
                for r in out.get("nodes", [])
            )
            raise AssertionError(
                f"{self.example}: {found} announced node(s) do not hold their "
                f"end of WAI-ARIA's structural relation, and the budget allows "
                f"{allowed} — {rows}. An `empty` collection either owns a member "
                f"of a role it promises, or SAYS it is empty "
                f"(`AccessNode::with_size_of_set(0)` / `with_row_count(0)`) — an "
                f"empty list is a state, a forgotten one is a defect, and the "
                f"declaration is what separates them. A `stray` member is "
                f"attached with `with_child` on a node of one of the roles its "
                f"own role requires."
            )
        print(
            f"[conform] {self.example}: {out.get('judged', 0)} node(s) carry a "
            f"structural requirement — {counts.get('empty', 0)} empty, "
            f"{counts.get('stray', 0)} stray"
        )

    def __exit__(self, exc_type, exc, tb) -> None:
        leak = self.shutdown()
        # ★ R1899 — and the store this harness minted goes with it. Only that
        # one: a root the CALLER chose is the caller's to remove, which is what
        # keeps `isolated_storage_dir`'s several-processes-one-store walks
        # working. Removed before the leak check so a leaking demo still
        # cleans up after itself.
        if self._own_storage is not None:
            shutil.rmtree(self._own_storage, ignore_errors=True)
            self._own_storage = None
        # R1570.3 — a leak fails the demo that caused it, but never masks a
        # failure already in flight: an assertion error is the more useful
        # verdict, and the leak is a consequence of whatever went wrong.
        if leak is not None and exc_type is None:
            raise LeakedProcess(
                f"{self.example} was still running after teardown: {leak}. "
                f"A demo that leaves a process behind poisons every demo the "
                f"sweep runs after it — see R1570.3"
            )

    def shutdown(self) -> Optional[str]:
        """Reap the driven binary. Return `None`, or why it survived.

        R1570.3 — returns rather than raises, so `__exit__` can decide whether
        the leak is the most interesting thing that happened.
        """
        if self._proc is None:
            return None
        proc, self._proc = self._proc, None
        if proc.poll() is None:
            try:
                if proc.stdin is not None and not proc.stdin.closed:
                    proc.stdin.close()
            except (OSError, BrokenPipeError):
                pass
        return terminate_process_tree(proc)

    def _resolve_binary(self) -> Optional[Path]:
        # R1330 — build-then-run: unless the caller vouches the binary is already
        # fresh (`ensure_build=False` / `PINION_ASSUME_BUILT`), rebuild the example
        # first so a stale on-disk binary can never silently run against edited
        # source. `cargo build` is incremental (sub-second when unchanged); a build
        # failure raises `BuildError` here — the demo dies loud instead of
        # exercising the previous artifact.
        if self.ensure_build:
            try:
                return ensure_built(self.example, release=self.release)
            except BuildError as exc:
                raise RpcError(
                    -32099,
                    f"cargo build -p {self.example} failed before launch",
                    exc.output[-4000:],
                ) from exc
            except FileNotFoundError as exc:
                # R1333 — cargo succeeded but the expected binary is missing (a
                # lib-only package, or a `[[bin]]` name != package name). Surface it
                # through the same loud `-32099` path as a build failure rather than
                # letting a raw `FileNotFoundError` escape.
                raise RpcError(
                    -32099,
                    f"{self.example} built but no runnable binary was produced",
                    str(exc),
                ) from exc
        flavor = "release" if self.release else "debug"
        path = WORKSPACE_ROOT / "target" / flavor / self.example
        return path if path.exists() else None

    def _cargo_run_cmd(self) -> list[str]:
        cargo = shutil.which("cargo") or "cargo"
        cmd = [cargo, "run", "-p", self.example, "--quiet"]
        if self.release:
            cmd.append("--release")
        return cmd

    def _pump_stdout(self) -> None:
        assert self._proc is not None
        assert self._proc.stdout is not None
        for line in self._proc.stdout:
            line = line.rstrip("\n")
            if line:
                self._inbox.put(line)

    def _pump_stderr(self) -> None:
        assert self._proc is not None
        assert self._proc.stderr is not None
        for line in self._proc.stderr:
            line = line.rstrip("\n")
            if line:
                self._stderr_lines.append(line)

    def request(
        self,
        method: str,
        params: Any = None,
        *,
        notify: bool = False,
    ) -> Optional[Response]:
        if self._proc is None or self._proc.stdin is None:
            raise RpcError(-32099, "subprocess not running")
        if self._proc.poll() is not None:
            raise RpcError(
                -32099,
                f"subprocess exited (rc={self._proc.returncode})",
                "\n".join(self._stderr_lines[-20:]),
            )

        request_id: Any
        if notify:
            request_id = None
        else:
            request_id = self._next_id
            self._next_id += 1

        envelope: dict[str, Any] = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            envelope["params"] = params
        if request_id is not None:
            envelope["id"] = request_id

        try:
            self._proc.stdin.write(json.dumps(envelope) + "\n")
            self._proc.stdin.flush()
        except (OSError, BrokenPipeError) as exc:
            raise RpcError(-32099, f"stdin write failed: {exc}") from exc

        if notify:
            return None

        deadline = time.monotonic() + self.request_timeout
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise RpcError(
                    -32099,
                    f"timeout waiting for response to id={request_id} method={method}",
                    "\n".join(self._stderr_lines[-10:]),
                )
            try:
                line = self._inbox.get(timeout=remaining)
            except queue.Empty:
                continue
            try:
                msg = json.loads(line)
            except json.JSONDecodeError:
                continue
            if msg.get("id") is None and "method" in msg:
                # R1552 — a JSON-RPC 2.0 notification (spec §4.1). Kept, not
                # dropped: it is somebody's subscription stream. This is the
                # discriminator the whole wire form turns on — a second
                # RESPONSE carrying this request's own id would be
                # indistinguishable from the answer below.
                self._notifications.append(msg)
                continue
            if msg.get("id") != request_id:
                continue
            if "error" in msg:
                err = msg["error"]
                raise RpcError(
                    int(err.get("code", -32603)),
                    str(err.get("message", "")),
                    err.get("data"),
                )
            return Response(id=msg.get("id"), result=msg.get("result"))

    def notifications(self, method: Optional[str] = None) -> list[dict]:
        """R1552 — every server-initiated frame seen so far, optionally
        filtered by `method`.

        Notifications are collected as a side effect of `request`, which is
        the only thing that reads the inbox. A demo that wants to observe a
        stream therefore drives a request (any request) between checks — see
        `drain_notifications`.
        """
        if method is None:
            return list(self._notifications)
        return [n for n in self._notifications if n.get("method") == method]

    def drain_notifications(self, method: Optional[str] = None) -> list[dict]:
        """Read the inbox WITHOUT sending a request, then return
        `notifications(method)`.

        `request` only sees frames that arrive while it is waiting for its own
        answer, so a stream that lands between requests would otherwise sit in
        the queue unseen. Non-blocking: it takes what is already there.
        """
        while True:
            try:
                line = self._inbox.get_nowait()
            except queue.Empty:
                break
            try:
                msg = json.loads(line)
            except json.JSONDecodeError:
                continue
            if msg.get("id") is None and "method" in msg:
                self._notifications.append(msg)
        return self.notifications(method)

    def await_notifications(
        self,
        method: str,
        count: int,
        *,
        timeout: float = 5.0,
    ) -> list[dict]:
        """Block until at least `count` notifications of `method` have been
        seen, and return them.

        Bounded and deterministic — never a fixed sleep. Raises `RpcError` on
        timeout naming what was actually seen, so a failure reports the count
        rather than an opaque hang.
        """
        deadline = time.monotonic() + timeout
        while True:
            seen = self.drain_notifications(method)
            if len(seen) >= count:
                return seen
            if time.monotonic() >= deadline:
                raise RpcError(
                    -32099,
                    f"timed out awaiting {count} {method} notification(s); saw {len(seen)}",
                    "\n".join(self._stderr_lines[-10:]),
                )

    def query(self, path: str, *, with_origin: bool = False) -> Any:
        """`scene/query` typed wrapper (§5.12).

        `with_origin=True` (R1482) asks the answer to say which surface
        produced it, and the result becomes `{"value": ..., "origin":
        "state"|"paint_driver"|"paint_frame"}` instead of the bare value.
        The flag mirrors the wire param rather than being defaulted on:
        the bare shape is what ~287 call sites read, and the disclosure is
        opt-in precisely so that stays true.
        """
        params: dict[str, Any] = {"path": path}
        if with_origin:
            params["with_origin"] = True
        resp = self.request("scene/query", params)
        assert resp is not None
        return resp.result

    def invoke(self, path: str, args: Any, *, with_origin: bool = False) -> Any:
        """`scene/invoke` typed wrapper (§5.12 item 8).

        `with_origin=True` (R1487) is the same opt-in `query` has carried
        since R1482, now covering the action channel: the result becomes
        `{"value": ..., "origin": "state"|"paint_driver"}` so a caller can
        tell an action that ran on the live simulation from one that ran on
        the retained model.
        """
        params: dict[str, Any] = {"path": path, "args": args}
        if with_origin:
            params["with_origin"] = True
        resp = self.request("scene/invoke", params)
        assert resp is not None
        return resp.result

    def intervene(self, path: str, value: Any, *, with_origin: bool = False) -> Any:
        """`scene/intervene` typed wrapper (R56.1.f.3 §5.22).

        Mirrors `invoke` shape — `{"path": str, "value": Any}` —
        but routes through the §5.15 item 7 state-write channel
        instead of the §5.15 item 8 action channel. `value=None`
        sends a JSON `null`, which `TextFieldExternal::intervene`
        treats as "clear selection" on the `selection` slot.

        `with_origin=True` (R1487) reports which surface took the write, in
        the same envelope `query` and `invoke` use: `{"value": null,
        "origin": ...}`. Bare, the result stays the ratified `null`.
        """
        params: dict[str, Any] = {"path": path, "value": value}
        if with_origin:
            params["with_origin"] = True
        resp = self.request("scene/intervene", params)
        assert resp is not None
        return resp.result

    def snapshot(
        self,
        path: str = "",
        *,
        source: str = "state",
        viewport: Optional[tuple[int, int]] = None,
        window: Optional[str] = None,
    ) -> Any:
        """`scene/snapshot` typed wrapper.

        `source="state"` (default) dumps the state scene root (root
        `External`). `source="paint"` dumps the paint scene produced
        by `V::view` at `viewport` (default 720x480) — see R51.194
        §5.49 §5.45 for the wire shape.

        `window` (R883.1) scopes the snapshot to a named window spec
        (the R670.B `{window: "<id>"}` wire param). `None` keeps the
        primary-window default. Pre-R883.1 every multi-window demo
        hand-rolled the raw `scene/snapshot` envelope just to add this
        one key — the wrapper now mirrors the full wire surface.
        """
        params: dict[str, Any] = {"path": path, "from": source}
        if viewport is not None:
            params["viewport"] = {"w": viewport[0], "h": viewport[1]}
        if window is not None:
            params["window"] = window
        resp = self.request("scene/snapshot", params)
        assert resp is not None
        return resp.result

    def voice(self, *, window: Optional[str] = None) -> dict[str, Any]:
        """`scene/voice` typed wrapper (R1691 §5.40 §2 #7) — which painted
        regions a reader is told about, and why the silent ones are silent.

        Answers `{total, counts, nodes}`. `counts` partitions the addressable
        population into `announced` / `silent` / `unvoiced` / `ghost` /
        `dangling` / `mumbled` / `hollow`; the last five are five different
        defects, which is why they are five keys and not one number.

        R1692 — every row also carries `name` (what a reader hears) and `fault`
        (why that is not usable, on a `mumbled` row). Prefer
        [`voice_partition_sum`] and [`voice_defects`] over a hand-written list
        of arm names: both derive from what the running binary publishes, and a
        demo carrying its own list measures the day it was written.

        Reads the last painted scene and the same access-tree producer
        `scene/access` runs, so a demo asserting on both is asking one surface
        rather than comparing two.
        """
        params: dict[str, Any] = {}
        if window is not None:
            params["window"] = window
        resp = self.request("scene/voice", params or None)
        assert resp is not None, "scene/voice answered nothing"
        assert isinstance(resp.result, dict), f"scene/voice: {resp.result!r}"
        return resp.result

    def conform(self, *, window: Optional[str] = None) -> dict[str, Any]:
        """`scene/conform` typed wrapper (R1693 §5.40 §2 #7) — whether the
        announced tree is one a reader can **walk**.

        Answers `{judged, counts, nodes}`. `counts` has two keys, because
        WAI-ARIA's structural relation has two directions and the repairs
        differ: `empty` is a collection owning none of what its role promises,
        `stray` is a member outside the collection its role requires.

        `judged` is the denominator — how many announced nodes carry a
        structural requirement at all. Read it: a tree with no collections in it
        is soundly green and says nothing about a screen full of tables.

        Runs the same access-tree producer `scene/access` and `scene/voice` run,
        so a demo asserting on all three is asking one surface three questions
        rather than comparing three answers.
        """
        params: dict[str, Any] = {}
        if window is not None:
            params["window"] = window
        resp = self.request("scene/conform", params or None)
        assert resp is not None, "scene/conform answered nothing"
        assert isinstance(resp.result, dict), f"scene/conform: {resp.result!r}"
        return resp.result

    def locate_region(
        self,
        *,
        shape: str = "rect",
        fit: str = "intersects",
        source: str = "state",
        viewport: Optional[tuple[int, int]] = None,
        **geometry: Any,
    ) -> Any:
        """`scene/locate_region` typed wrapper (R1591 §5.32 §2 #7).

        `shape` picks the region — `"rect"` (`x`/`y`/`w`/`h`), `"circle"`
        (`cx`/`cy`/`r`) or `"lasso"` (`points=[[x, y], ...]`) — and `fit`
        picks what "covered" means: `"intersects"` or `"contains"`.

        `source="paint"` asks the PAINTED frame rather than the state
        tree, which is the only one carrying geometry for a view-fn
        binding (the same two-scene basis `snapshot` uses).
        """
        params: dict[str, Any] = {"shape": shape, "fit": fit, "from": source}
        params.update(geometry)
        if viewport is not None:
            params["viewport"] = {"w": viewport[0], "h": viewport[1]}
        resp = self.request("scene/locate_region", params)
        assert resp is not None
        return resp.result

    def marks(
        self,
        tag: str,
        index: Optional[int] = None,
        *,
        source: str = "paint",
        viewport: Optional[tuple[int, int]] = None,
    ) -> dict[str, Any]:
        """`scene/marks` typed wrapper (R1615 §5.12 §2 #7) — *why* the node
        tagged `tag` looks the way it does.

        Answers with `{tag, kind, channel, published}` plus, when the node
        published named runs, the `domain` its indices count in and the `runs`
        themselves in declaration order. Pass `index` to also get `at`: the
        whole stack covering that position, innermost last, and `top` — the one
        the painter obeyed.

        `published: false` is not one fact but two, and `channel` says which:
        `"carries"` is a node that could have named its runs and did not,
        anything else is a node whose KIND has nothing to attribute.

        Defaults to `source="paint"` unlike its spatial neighbours, because
        marks are a paint fact: a view-fn binding's state tree holds none of
        the nodes the view emits.
        """
        params: dict[str, Any] = {"tag": tag, "from": source}
        if index is not None:
            params["index"] = index
        if viewport is not None:
            params["viewport"] = {"w": viewport[0], "h": viewport[1]}
        resp = self.request("scene/marks", params)
        assert resp is not None, "scene/marks answered nothing"
        assert isinstance(resp.result, dict), f"scene/marks: {resp.error}"
        return resp.result

    def mark_names(
        self,
        tag: str,
        index: int,
        *,
        viewport: Optional[tuple[int, int]] = None,
    ) -> list[str]:
        """The names covering `index`, in declaration order — the common case
        of [`marks`], with the envelope unwrapped. Empty when the node
        published nothing or nothing covers that position; the two are
        distinguished by `marks(...)["published"]`."""
        answer = self.marks(tag, index, viewport=viewport)
        at = answer.get("at")
        return [] if at is None else at["names"]

    def cache_stats(self, *, window: Optional[str] = None) -> dict[str, Any]:
        """`scene/cache_stats` typed wrapper (R682.B §5.16 / R883.1).

        Returns the per-window `FragmentCacheStats` snapshot. The
        `paint_count` field is the canonical "a real frame landed"
        observable — only `AppShell::render_window` advances it, so
        gating on it (see [`wait_paint_beyond`]) replaces every
        frame-timing guess in continuous-paint demos.
        """
        params: dict[str, Any] = {}
        if window is not None:
            params["window"] = window
        resp = self.request("scene/cache_stats", params)
        assert resp is not None and isinstance(resp.result, dict)
        return resp.result

    def text_cache_stats(self) -> dict[str, Any]:
        """`scene/text_cache_stats` typed wrapper (R1521 §5.36 §5.7).

        Returns the §5.36 shape cache's `{shapes, entries, capacity,
        max_capacity, growths, font_scans, at_ceiling}`. Per-SHELL, not
        per-window — one `LayoutCache` serves every window — so unlike
        [`cache_stats`] this takes no `window` param.

        Distinct from [`cache_stats`], which reports the §5.16 paint
        FRAGMENT cache. The two disagree in the direction that matters: a
        working set past the shape cache's capacity re-runs the shaper on
        every string every frame while the fragment cache reports a
        perfectly healthy hit rate.
        """
        resp = self.request("scene/text_cache_stats", {})
        assert resp is not None and isinstance(resp.result, dict)
        return resp.result

    def frame_count(self, *, window: Optional[str] = None) -> int:
        """How many frames this window has painted, or `0` before its first.

        R1761 — the half of [`await_paint`] a caller takes BEFORE the action, so
        the wait afterwards is against a baseline rather than against a number
        that was already moving.
        """
        try:
            return int(self.frame_timings(window=window)["frame_count"])
        except (AssertionError, RpcError, KeyError, TypeError, ValueError):
            # Before the first frame the profiler refuses, which is `0` frames
            # painted rather than an error a caller should have to handle: the
            # question is "how many", and none is an answer.
            return 0

    def await_paint(
        self,
        beyond: int,
        *,
        window: Optional[str] = None,
        timeout: float = 8.0,
    ) -> int:
        """★★★★★ R1761 — block until this window has painted a frame AFTER an
        action, and answer the count it reached.

        # What this exists for, measured

        A verdict read from the PAINT (`pinion_core::painted`, and everything
        `conformance`'s `report_from_paint` is built on) is a fact about the
        last frame the window drew. An action driven over RPC lands in the
        deferred-input inbox and applies on the next shell frame, so a read
        taken straight after it can be answered from the frame BEFORE the action
        — and the answer is a true statement about a screen the reader has
        already left.

        `tick(dt)` does not close that: it advances the animation CLOCK, and a
        window's paint is not synchronous with the call. Measured 2026-08-21
        during a 34-demo sweep, two demos that navigate and then read a
        paint-derived verdict failed once each and passed on every isolated
        re-run — the classic shape of a read racing a render, which
        [`wait_until`]'s own header already names as the reason it exists.

        # Why the frame counter and not the verdict

        Waiting until the thing being asserted is true makes the assertion
        vacuous. This waits for a fact about the WINDOW — that it drew another
        frame — which is a different fact from what any caller goes on to
        assert, and it fails loudly rather than reading a stale frame quietly.

        One frame beyond the baseline, and the baseline is taken BEFORE the
        action: the shell drains its deferred input at the start of a frame and
        then builds the view, so the first frame counted after the action is the
        one that painted its effect. Waiting for more than that would hang a
        window whose pacing lets it go idle once the screen has settled, which
        is a failure mode with the same shape as the one being repaired.
        """
        target = beyond + 1

        def painted() -> Optional[int]:
            count = self.frame_count(window=window)
            return count if count >= target else None

        return wait_until(
            painted,
            timeout=timeout,
            desc=f"the window to paint frame {target} (baseline {beyond})",
        )

    def intervene_painted(
        self,
        path: str,
        value: Any,
        *,
        dt: float = 16.0,
        window: Optional[str] = None,
    ) -> Any:
        """★★★★★ R1761 — write a slot and do not come back until the window has
        PAINTED the result.

        The form to use before reading anything derived from the paint — a
        conformance verdict, a mark census, a rectangle. `intervene` + `tick`
        alone leaves the read racing the render: the clock moves, the model
        moves, and the frame that draws the new state is scheduled rather than
        taken. See [`await_paint`] for the measurement.

        The tick is kept and happens BEFORE the wait, because a demo that
        advances the clock is usually asking for the settled state of something
        that animates, and dropping it would change what a caller reads.

        # The class this was applied to, counted rather than described

        Eight demos read a conformance verdict. Five of them navigate and then
        read one, and all five use this; the other three (`r1728`, `r1730`,
        `r1731`) read at boot, where `RpcSubprocess`'s own `boot_grace` already
        covers the first frame. ★ Counted at R1761's closing audit, which is
        also where the count in that round's ledger entry was found wrong — it
        says *twelve*, which is how many demos mention the word.
        """
        before = self.frame_count(window=window)
        out = self.intervene(path, value)
        if dt:
            self.tick(dt)
        self.await_paint(before, window=window)
        return out

    def frame_timings(self, *, window: Optional[str] = None) -> dict[str, Any]:
        """`scene/frame_timings` typed wrapper (R907 §5.16 §5.7).

        Returns the per-window frame-timing profiler snapshot: the last
        frame's `{build,encode,render,total,other}_us`, the rolling
        `window` min/mean/max + per-phase means, the cumulative
        `frame_count`, the `window_len`, and `mean_fps`. Raises if the
        window has not painted yet (`-32602 FrameTimingsUnavailable`);
        gate on it via [`wait_frame_timings`] before a hard read.
        """
        params: dict[str, Any] = {}
        if window is not None:
            params["window"] = window
        resp = self.request("scene/frame_timings", params)
        assert resp is not None and isinstance(resp.result, dict)
        return resp.result

    def export_pdf(
        self,
        *,
        page: Optional[str] = None,
        orientation: Optional[str] = None,
        window: Optional[str] = None,
    ) -> dict[str, Any]:
        """`scene/export_pdf` typed wrapper (R908 §3 §5.53).

        Renders the addressed window's current paint scene to a vector
        PDF and returns `{page_count, page_width_pt, page_height_pt,
        object_count, byte_len, document}` (the `document` is the full
        ASCII PDF). `page` ("letter"|"a4") / `orientation`
        ("portrait"|"landscape") are optional; absent `page` sizes the
        page to the scene's own pixel bounds. Raises `-32602 NoPaintScene`
        until the window has painted; gate via [`wait_export_pdf`].
        """
        params: dict[str, Any] = {}
        if page is not None:
            params["page"] = page
        if orientation is not None:
            params["orientation"] = orientation
        if window is not None:
            params["window"] = window
        resp = self.request("scene/export_pdf", params)
        assert resp is not None and isinstance(resp.result, dict)
        return resp.result

    def intents(self) -> list[Any]:
        resp = self.request("scene/intents")
        assert resp is not None
        result = resp.result
        return list(result) if isinstance(result, list) else []

    def key(
        self,
        at: Optional[tuple[float, float]] = None,
        *,
        path: Optional[str] = None,
        name: str = "",
        state: Optional[str] = None,
    ) -> None:
        """`scene/key` typed wrapper (R51.197 / R51.202 §5.49 §5.45).

        Mutually exclusive cursor target — supply exactly one of:
          * `at = (x, y)` — explicit logical-pixel cursor coordinate.
          * `path = "<tag>"` — R51.202 path-based form: the dispatcher
            walks the paint scene for the first node carrying `tag`
            and uses its rect centre as the key-event cursor target.
            Eliminates the snapshot/lookup boilerplate.

        Inject a W3C `KeyboardEvent.key` string at the resolved
        cursor location. R666 §5.37 — the dispatcher
        auto-discriminates by `name.chars().count()`: single-codepoint
        strings ("a", " ", "漢") route through `handle_character_key`
        (V::keybinding typed-event channel first, then apply_key
        fallback); multi-codepoint W3C named strings ("Enter",
        "ArrowDown", "PageUp") route through `handle_named_key`
        (focused-widget shortcut then scroll fallback). Closes the
        pre-R666 [[scene-key-character-named-gap]] — single-character
        V::keybinding intercepts were previously invisible to RPC
        drivers because every scene/key request was treated as named.

        R882 §5.49 §5.39 — `state="down"` / `state="up"` mirror the
        winit KeyboardInput Pressed/Released edges (held-key absolute
        state; "Space" arms the left-drag pan chord). `state=None`
        keeps the legacy atomic press, which never touches the
        held-key cache. R882.1 — `state="up"` is positionless (a real
        key release carries no cursor and dispatches nothing), so
        `at`/`path` may both be omitted for it.
        """
        if not name:
            raise ValueError("key name must not be empty")
        if state == "up":
            if at is not None and path is not None:
                raise ValueError("supply at most one of `at` or `path`")
        elif (at is None) == (path is None):
            raise ValueError("exactly one of `at` or `path` must be supplied")
        params: dict[str, Any] = {"key": name}
        if state is not None:
            params["state"] = state
        if at is None and path is None:
            self.request("scene/key", params)
            return
        if at is not None:
            params["at"] = {"x": float(at[0]), "y": float(at[1])}
        else:
            assert path is not None
            params["path"] = path
        self.request("scene/key", params)

    def keys(
        self,
        strokes: "Sequence[Any]",
        *,
        at: Optional[tuple[float, float]] = None,
        path: Optional[str] = None,
    ) -> None:
        """R1757 §5.49 §5.39 — one `scene/key` request carrying SEVERAL
        keystrokes, so they reach the binding as one keystroke *delivery*.

        This is the only way to drive a **gesture** over the wire. A repeat
        window, a chord timeout and a double-tap are statements about "these
        arrived together" (R1658), and one drain is one delivery — so N
        separate `.key()` calls are N deliveries and can never express one.
        `.text()` below has the same limitation by construction: it loops.

        Each stroke is either a key name (an atomic press) or a
        `(name, state)` pair for an explicit `"down"` / `"up"` edge, which is
        what a chord needs — `Space` down, the arrows, `Space` up, all in one
        delivery.

        Confirm it landed as one with `.key_delivery_opened()`: that ordinal
        advances by exactly one across this call, whatever the burst carried.

        Target resolution matches `.key()`, except that the burst shares ONE
        target — a keyboard delivery has one focus. It may be omitted only
        when every stroke is a release, which dispatches nothing.
        """
        if not strokes:
            raise ValueError("a burst must carry at least one keystroke")
        entries: list[Any] = []
        all_releases = True
        for stroke in strokes:
            if isinstance(stroke, str):
                name, state = stroke, None
            else:
                name, state = stroke
            if not name:
                raise ValueError("key name must not be empty")
            entries.append(name if state is None else {"key": name, "state": state})
            all_releases = all_releases and state == "up"
        if all_releases:
            if at is not None and path is not None:
                raise ValueError("supply at most one of `at` or `path`")
        elif (at is None) == (path is None):
            raise ValueError("exactly one of `at` or `path` must be supplied")
        params: dict[str, Any] = {"keys": entries}
        if at is not None:
            params["at"] = {"x": float(at[0]), "y": float(at[1])}
        elif path is not None:
            params["path"] = path
        self.request("scene/key", params)

    def key_delivery_opened(self, *, window: Optional[str] = None) -> int:
        """R1757 §5.49 — the `key_delivery.opened` ordinal from
        `scene/input_state`: how many keystroke deliveries the runtime has
        opened, the last of which is the one now open.

        Read it before and after a `.keys()` burst; the difference is the
        number of deliveries that request opened, and for a burst it is one.
        A request that dispatches no keystroke does not move it, so
        bracketing a burst with these two reads does not perturb what they
        measure.
        """
        params: dict[str, Any] = {}
        if window is not None:
            params["window"] = window
        resp = self.request("scene/input_state", params)
        assert resp is not None
        result = resp.result
        assert isinstance(result, dict), f"input_state is an object: {result!r}"
        return int(result["key_delivery"]["opened"])

    def text(
        self,
        body: str,
        *,
        path: Optional[str] = None,
        at: Optional[tuple[float, float]] = None,
    ) -> None:
        """R666 §5.37 — convenience wrapper for typing a multi-char
        string into a focused TextField. Iterates one character at a
        time so each keystroke flows through `scene/key`'s
        character-key arc (R666 #3), matching the per-keystroke
        cadence of a real keyboard. `path` / `at` reach the field the
        same way `.key()` does; pre-resolving a single coordinate is
        usually fine for typing since the text field rect stays put
        across single-keystroke mutations.
        """
        if not body:
            return
        for ch in body:
            self.key(at=at, path=path, name=ch)

    def click(
        self,
        at: Optional[tuple[float, float]] = None,
        *,
        path: Optional[str] = None,
        button: Optional[str] = None,
    ) -> None:
        """`scene/click` typed wrapper (R51.196 / R51.201 §5.49).

        Mutually exclusive — supply exactly one:
          * `at = (x, y)` — click at the given logical-pixel coordinate.
          * `path = "<tag>"` — R51.201 path-based form: the dispatcher
            walks the paint scene for the first node carrying `tag`
            and clicks at its rect centre. Eliminates the
            `snapshot → find_by_tag → node_center` boilerplate when
            the caller only wants "click on widget X".

        `button` (R887 §5.49 §5.53) selects the mouse button: omitted /
        `"left"` is the press/release activation pair; `"right"` is the
        secondary-button press-edge one-shot (`apply_secondary_click`,
        the context-menu arc — no release half). A middle press-release
        is a gesture and lives on `scene/drag {button: "middle"}`.

        The shell drains the deferred-input inbox after the request
        returns, applying `cursor_moved`, `mouse_pressed`, then
        `mouse_released` (left) or `cursor_moved` +
        `secondary_click_for_window` (right) so the substrate fires the
        same arc winit's `WindowEvent::MouseInput` triggers from a real
        mouse. Follow up with `query(...)` or `snapshot(...)` to
        observe the post-click state transition.
        """
        if (at is None) == (path is None):
            raise ValueError("exactly one of `at` or `path` must be supplied")
        if at is not None:
            params: dict[str, Any] = {"at": {"x": float(at[0]), "y": float(at[1])}}
        else:
            assert path is not None
            params = {"path": path}
        if button is not None:
            params["button"] = button
        self.request("scene/click", params)

    def pointer_button(
        self,
        button: str,
        state: str,
        at: Optional[tuple[float, float]] = None,
        *,
        path: Optional[str] = None,
    ) -> None:
        """`scene/pointer_button` typed wrapper (R1416 §5.35 §5.15).

        Inject ONE raw mouse-button EDGE — the single-edge peer the press-pair
        `click()` / press-only right-click / gesture `drag()` never expressed.
        `button` is `"left"` / `"middle"` / `"right"` (both required — a raw
        edge is meaningless without them); `state` is `"down"` / `"up"`.

        A widget that owns the raw multi-button stream
        (`External::wants_raw_pointer_buttons`) receives the edge verbatim, with
        the held modifiers (set out-of-band via `modifiers()`) on BOTH edges and
        the button identified — while a non-raw widget under the cursor runs the
        standard per-button GUI arc (left = focus, middle = paste, right =
        context menu). The shell seeds the cursor with a `cursor_moved` before
        the edge (so a raw sink's hover-tracked position is fresh) then routes
        through the SAME `pointer_button_for_window` seam the native winit
        `MouseInput` path reaches, so an injected edge is indistinguishable from
        a physical one.

        Selector taxonomy mirrors `click()` — supply exactly one of
        `at = (x, y)` or `path = "<tag>"`.
        """
        if (at is None) == (path is None):
            raise ValueError("exactly one of `at` or `path` must be supplied")
        if at is not None:
            params: dict[str, Any] = {"at": {"x": float(at[0]), "y": float(at[1])}}
        else:
            assert path is not None
            params = {"path": path}
        params["button"] = button
        params["state"] = state
        self.request("scene/pointer_button", params)

    def pointer_pressure(self, value: float) -> None:
        """`scene/pointer_pressure` typed wrapper (R1423 §5.35 §5.15).

        Set the pointer PRESSURE (W3C `PointerEvent.pressure` / the toolkit
        `pressure()`), normalised `0.0..=1.0`. Positionless
        (out-of-band, like `modifiers()`): the value is delivered to the surface
        under the pointer at once and rides subsequent moves. The AI-first source
        for a pressure-reactive surface (an ink brush, a DCC viewport), so a
        tablet is not required to exercise force headless.
        """
        self.request("scene/pointer_pressure", {"value": float(value)})

    def pointer_tilt(self, tilt_x: float, tilt_y: float) -> None:
        """`scene/pointer_tilt` typed wrapper (R1429 §5.35 §5.15).

        Set the pointer TILT (W3C `PointerEvent.tiltX/tiltY` / the toolkit
        `xTilt/yTilt`), each axis in degrees `-90.0..=90.0`.
        Positionless (out-of-band, like `pointer_pressure()`): the value is
        delivered to the surface under the pointer at once and rides subsequent
        moves. The AI-first source for a tilt-reactive surface (a calligraphy
        nib, a DCC viewport); winit exposes no tilt axis, so the RPC is the sole
        driver, and a tablet is not required to exercise lean headless.
        """
        self.request(
            "scene/pointer_tilt",
            {"tilt_x": float(tilt_x), "tilt_y": float(tilt_y)},
        )

    def pointer_twist(self, twist: float) -> None:
        """`scene/pointer_twist` typed wrapper (R1430 §5.35 §5.15).

        Set the pointer TWIST (W3C `PointerEvent.twist` / the toolkit
        `rotation()`), the barrel rotation in degrees, wrapped to
        `0.0..=360.0` at the router. Positionless (out-of-band), delivered to the
        surface under the pointer at once; winit exposes no barrel axis, so the
        RPC is the sole driver.
        """
        self.request("scene/pointer_twist", {"twist": float(twist)})

    def pointer_tangential_pressure(self, tangential: float) -> None:
        """`scene/pointer_tangential_pressure` typed wrapper (R1430 §5.35 §5.15).

        Set the airbrush finger-wheel position (W3C
        `PointerEvent.tangentialPressure` / the toolkit
        `tangentialPressure()`), clamped to `-1.0..=1.0` at the
        router. Positionless, out-of-band.
        """
        self.request(
            "scene/pointer_tangential_pressure", {"tangential": float(tangential)}
        )

    def pointer_height(self, height: float) -> None:
        """`scene/pointer_height` typed wrapper (R1430 §5.35 §5.15).

        Set the pointer HEIGHT (the toolkit `z()`), the hover distance above
        the surface, floored at `0.0` at the router. Positionless, out-of-band;
        no W3C peer.
        """
        self.request("scene/pointer_height", {"height": float(height)})

    def pointer_type(self, kind: str) -> None:
        """`scene/pointer_type` typed wrapper (R1431 §5.35 §5.15).

        Set the pointer DEVICE kind (W3C `PointerEvent.pointerType` / the toolkit
        `pointerType()`): one of ``"mouse"`` / ``"pen"`` /
        ``"eraser"`` / ``"touch"``. Positionless, out-of-band; lets a headless
        client present as a pen or eraser with no device.
        """
        self.request("scene/pointer_type", {"type": kind})

    def hover(
        self,
        at: Optional[tuple[float, float]] = None,
        *,
        path: Optional[str] = None,
    ) -> None:
        """`scene/hover` typed wrapper (R695 §5.35 §5.49).

        Moves the pointer to `(x, y)` (or the centre of the node tagged
        `path`) with no press — the bare hover transition. The shell
        drains the deferred-input inbox after this returns, applying a
        single `cursor_moved` so the `InputRouter` re-resolves its hover
        target and fires the synthetic `PointerEnter` / `PointerLeave`
        arc (the Tooltip show/hide trigger). The pointer-position-only
        peer to `click()`; follow up with `query(...)` or
        `snapshot(...)` to observe the resulting hover-driven state.

        Selector taxonomy mirrors `click()` — supply exactly one of
        `at = (x, y)` or `path = "<tag>"`.
        """
        if (at is None) == (path is None):
            raise ValueError("exactly one of `at` or `path` must be supplied")
        if at is not None:
            params: dict[str, Any] = {"at": {"x": float(at[0]), "y": float(at[1])}}
        else:
            assert path is not None
            params = {"path": path}
        self.request("scene/hover", params)

    def pointer_leave(self) -> None:
        """`scene/pointer_leave` typed wrapper (R719 §5.35 §5.49).

        Moves the pointer *off* the window entirely (winit
        `CursorLeft`): the shell drops the cursor and rolls back any
        in-flight `Hover`, so whatever widget the pointer was over
        returns to its un-hovered resting state. The cursor-exit peer
        to `hover()` — positionless, so it takes no `at` / `path`.

        `__enter__` calls this once after boot to establish a
        deterministic "no pointer over the window" baseline: a real
        X / Wayland server maps the test window wherever the window
        manager places it, often under the developer's physical
        cursor, which delivers a genuine boot-time `CursorMoved` and
        leaves a widget reporting `Hover` before any RPC ran. A true
        headless CI (cursor parked) never sees that event; this wrapper
        makes a cursor-occupied desktop match that clean baseline so
        boot-state assertions are reproducible regardless of where the
        host mouse sits.
        """
        self.request("scene/pointer_leave")

    def modifiers(
        self,
        *,
        shift: bool = False,
        ctrl: bool = False,
        alt: bool = False,
        meta: bool = False,
    ) -> None:
        """`scene/modifiers` typed wrapper (R763 §5.39 §5.49).

        The winit `WindowEvent::ModifiersChanged` RPC peer: sets the
        shell's *absolute* modifier cache so a subsequent `click()` /
        `drag()` / `key()` press reads the held modifiers exactly as a
        real key-down would. Modifiers are tracked out-of-band (their
        own event, not a per-click field), so this is a standalone state
        setter that persists until the next call — issue
        `modifiers()` (all released) afterwards to mirror the key-up, the
        same way a real session releases Shift after a Shift-click.

        Canonical Shift-click-extend sequence::

            tf.modifiers(shift=True)
            tf.click(at=(x, y))      # press reads shift -> extend
            tf.modifiers()           # release Shift

        Closes the R742.2 RPC-modifier-channel gap for every input path.
        """
        self.request(
            "scene/modifiers",
            {"shift": shift, "ctrl": ctrl, "alt": alt, "meta": meta},
        )

    def tick(self, dt: float) -> None:
        """`scene/tick` typed wrapper (R724 §5.28).

        Advances the window's animation clock by `dt` seconds, so
        time-driven state — §5.28 springs, the R57.X theme-fade, caret
        blink, timed widget dismissal — can be driven *deterministically*
        rather than waiting on non-deterministic real-frame ticks
        between RPC calls. Call it, then read the settled state via
        `snapshot()` / `query()`. A large `dt` (e.g. 0.5) fast-forwards
        a short animation (~200 ms theme-fade) safely past completion;
        smaller deltas step it. `dt` must be finite and >= 0.
        """
        self.request("scene/tick", {"dt": dt})

    def tick_ms(self, ms: float) -> None:
        """Advance the window's animation clock by `ms` MILLISECONDS.

        ★★★★★ R1783 — this exists because `tick()` takes SECONDS and 172
        call sites across 28 demos were written `tick(16)`, meaning one
        frame. `tick(16)` advances sixteen seconds.

        For most of those sites the difference never showed: overshooting a
        200 ms fade lands on the same settled value, which is what `tick()`'s
        own docstring blesses. It shows the moment something has a FINITE
        LIFETIME that is supposed to still be running. Measured on
        `hello-analyzer-shell`, whose toast lives 2.6 s: after one press,
        `tick(2.5)` leaves the sentence standing and `tick(2.7)` empties it,
        so a single `tick(16)` destroyed the very thing eight demos were
        about — and they had been passing only because the wire went on
        reporting a sentence the screen had already stopped showing.

        Prefer this spelling whenever the test means "one frame" or "a few
        frames". Keep `tick()` for a deliberate fast-forward, where the
        number reads as seconds because it is.
        """
        self.tick(ms / 1000.0)

    def set_fps(self, fps: Optional[int], *, window: Optional[str] = None) -> None:
        """`scene/set_fps` typed wrapper (R829 §2 #4 §5.28).

        Sets the addressed window's target frame rate — the §2 #4
        game-loop pacing policy. `fps=0` *pauses* the per-window paint
        clock so the continuous immediate-mode loop stops auto-advancing;
        the window then only repaints on an explicit `tick()` step, so an
        AI client frame-steps the immediate-mode game loop
        deterministically (`set_fps(0)` then `tick(dt)` advances the
        drivers by exactly `dt`). `fps=N` (re)starts the continuous loop
        at N fps. `fps=None` (R888) clears the override, restoring the
        adaptive default policy. Read back via `pacing_state()`.
        `window` scopes the write (R889: an unknown id raises
        `unknown_window` instead of silently targeting the primary).
        """
        params: dict[str, Any] = {"fps": fps}
        if window is not None:
            params["window"] = window
        self.request("scene/set_fps", params)

    def pacing_state(self, *, window: Optional[str] = None) -> Any:
        """`scene/pacing_state` typed wrapper (R888 §5.49 §5.28): the
        READ peer of `set_fps`. Returns the addressed window's target:
        `{"fps": N}` (override; 0 = paused) or `{"fps": None}` (no
        override — the adaptive default policy applies). `window`
        scopes the read (R889 unknown ids raise `unknown_window`)."""
        params: dict[str, Any] = {}
        if window is not None:
            params["window"] = window
        resp = self.request("scene/pacing_state", params)
        assert resp is not None
        return resp.result

    def double_click(
        self,
        at: Optional[tuple[float, float]] = None,
        *,
        path: Optional[str] = None,
    ) -> None:
        """`scene/double_click` typed wrapper (R663 §5.49).

        Emits two complete press/release cycles at `(x, y)` without
        an intervening cursor move so the receiving `InputRouter`
        arc fires identically to a real-mouse double-click. Mirrors
        `click()` for selector taxonomy (`at` xor `path`).

        Use when the receiving widget distinguishes single from
        double activation (e.g. TasteJS TodoMVC double-click-to-edit
        row text). A widget that only cares about single click sees
        the double-click as two activations — usually idempotent on
        toggle / commit-class wires.
        """
        if (at is None) == (path is None):
            raise ValueError("exactly one of `at` or `path` must be supplied")
        if at is not None:
            params: dict[str, Any] = {"at": {"x": float(at[0]), "y": float(at[1])}}
        else:
            assert path is not None
            params = {"path": path}
        self.request("scene/double_click", params)

    def drag(
        self,
        *,
        from_at: Optional[tuple[float, float]] = None,
        from_path: Optional[str] = None,
        to_at: Optional[tuple[float, float]] = None,
        to_path: Optional[str] = None,
        steps: int = 8,
        button: str = "left",
        phase: str = "full",
    ) -> None:
        """`scene/drag` typed wrapper (R660 §5.49).

        Simulates a full real-mouse drag arc — press at `from`, march
        the cursor through `steps` interpolated frames to `to`, release.
        The shell substrate forwards every frame to the `InputRouter`
        under the R51.34 capture lock so the receiving widget's
        `pointer_move` arc fires exactly as it would for a real mouse
        (R55.D.3 ScrollBar drag math; future Slider drag reuses the
        same primitive).

        Endpoint selection mirrors [`click`] — supply exactly one of
        `from_at = (x, y)` or `from_path = "<tag>"` (and same for
        `to_at` / `to_path`); the path form walks the paint scene for
        the first node carrying `tag` and uses its rect centre.

        `steps` defaults to 8 — enough intermediate cursor frames for
        a receiving widget's state machine to observe mid-drag values
        without paying for hundreds of redundant samples. Pass `0` for
        a degenerate press / release at `from_at` (well-defined but
        usually a test bug).

        `button` (R881 §5.35 §5.49) selects the held mouse button:
        "left" (default — the capture-lock / DnD / text-select arc) or
        "middle" (drag-to-pan; an in-place press/release is the
        middle-click paste).

        `phase` (R1138 §5.49 §2 #2) runs only a slice of the press /
        march / release arc so an AI can HOLD a drag mid-gesture: "full"
        (default — the whole self-contained arc), "begin" (press + march,
        then HOLD — a follow-up `snapshot(source="paint")` then sees the
        held mid-drag), "move" (re-aim the held drag, no press / release),
        "end" (march + release, settling it).
        """
        if (from_at is None) == (from_path is None):
            raise ValueError("exactly one of `from_at` or `from_path` must be supplied")
        if (to_at is None) == (to_path is None):
            raise ValueError("exactly one of `to_at` or `to_path` must be supplied")
        if steps < 0:
            raise ValueError("steps must be non-negative")
        params: dict[str, Any] = {"steps": int(steps)}
        if button != "left":
            params["button"] = button
        if phase != "full":
            params["phase"] = phase
        if from_at is not None:
            params["from"] = {"x": float(from_at[0]), "y": float(from_at[1])}
        else:
            assert from_path is not None
            params["from_path"] = from_path
        if to_at is not None:
            params["to"] = {"x": float(to_at[0]), "y": float(to_at[1])}
        else:
            assert to_path is not None
            params["to_path"] = to_path
        self.request("scene/drag", params)

    def wheel(
        self,
        at: Optional[tuple[float, float]] = None,
        *,
        path: Optional[str] = None,
        lines: Optional[tuple[float, float]] = None,
        pixels: Optional[tuple[float, float]] = None,
    ) -> None:
        """`scene/wheel` typed wrapper (R51.195 / R51.202 §5.49 §5.45).

        Mutually exclusive cursor target — supply exactly one of
        `at = (x, y)` or `path = "<tag>"` (the latter resolves the
        target via paint-scene lookup; see R51.202). Delta is also
        mutually exclusive — supply exactly one of `lines` /
        `pixels`. The shell drains the deferred-input inbox after
        this returns, applies `cursor_moved` then `wheel`, and bumps
        the redraw flag if the router dispatched against an attached
        `ScrollState`. Follow up with `snapshot(source="paint")` to
        observe the post-wheel offset.
        """
        if (at is None) == (path is None):
            raise ValueError("exactly one of `at` or `path` must be supplied")
        if (lines is None) == (pixels is None):
            raise ValueError("exactly one of `lines` or `pixels` must be supplied")
        if lines is not None:
            delta = {"lines": {"dx": float(lines[0]), "dy": float(lines[1])}}
        else:
            assert pixels is not None
            delta = {"pixels": {"dx": float(pixels[0]), "dy": float(pixels[1])}}
        params: dict[str, Any] = {"delta": delta}
        if at is not None:
            params["at"] = {"x": float(at[0]), "y": float(at[1])}
        else:
            assert path is not None
            params["path"] = path
        self.request("scene/wheel", params)

    def pinch_gesture(
        self,
        magnification: float,
        phase: str,
        at: Optional[tuple[float, float]] = None,
        *,
        path: Optional[str] = None,
    ) -> None:
        """`scene/pinch_gesture` typed wrapper (R1432 §5.35 §5.15).

        Drive a native PINCH (magnify) gesture — the toolkit native gesture event
        `ZoomNativeGesture` peer — at the cursor target. Supply exactly one of
        `at = (x, y)` or `path = "<tag>"` (the widget under the cursor receives
        the offer). `magnification` is the INCREMENTAL scale delta (positive
        zooms in, negative out); `phase` brackets the arc, one of ``"begin"`` /
        ``"update"`` / ``"end"`` / ``"cancel"``. The shell drains the inbox
        after this returns, applying `cursor_moved` then the pinch offer. winit
        exposes no trackpad headless, so this RPC is the sole driver (§2 #2);
        follow up with `query(...)` / `snapshot(...)` to observe the zoom.
        """
        if (at is None) == (path is None):
            raise ValueError("exactly one of `at` or `path` must be supplied")
        params: dict[str, Any] = {
            "magnification": float(magnification),
            "phase": phase,
        }
        if at is not None:
            params["at"] = {"x": float(at[0]), "y": float(at[1])}
        else:
            assert path is not None
            params["path"] = path
        self.request("scene/pinch_gesture", params)

    def rotation_gesture(
        self,
        rotation: float,
        phase: str,
        at: Optional[tuple[float, float]] = None,
        *,
        path: Optional[str] = None,
    ) -> None:
        """`scene/rotation_gesture` typed wrapper (R1433 §5.35 §5.15).

        Drive a native ROTATION gesture — the toolkit native gesture event
        `RotateNativeGesture` peer, the `pinch_gesture` sibling — at the cursor
        target. Supply exactly one of `at = (x, y)` or `path = "<tag>"` (the
        widget under the cursor receives the offer). `rotation` is the
        INCREMENTAL delta in DEGREES (positive rotates counter-clockwise,
        winit's convention); `phase` brackets the arc, one of ``"begin"`` /
        ``"update"`` / ``"end"`` / ``"cancel"``. The shell drains the inbox after
        this returns, applying `cursor_moved` then the rotation offer. winit
        exposes no trackpad headless, so this RPC is the sole driver (§2 #2);
        follow up with `query(...)` / `snapshot(...)` to observe the rotation.
        """
        if (at is None) == (path is None):
            raise ValueError("exactly one of `at` or `path` must be supplied")
        params: dict[str, Any] = {
            "rotation": float(rotation),
            "phase": phase,
        }
        if at is not None:
            params["at"] = {"x": float(at[0]), "y": float(at[1])}
        else:
            assert path is not None
            params["path"] = path
        self.request("scene/rotation_gesture", params)

    def pan_gesture(
        self,
        delta_x: float,
        delta_y: float,
        phase: str,
        at: Optional[tuple[float, float]] = None,
        *,
        path: Optional[str] = None,
    ) -> None:
        """`scene/pan_gesture` typed wrapper (R1434 §5.35 §5.15).

        Drive a native N-finger PAN gesture — the toolkit native gesture event
        `PanNativeGesture` peer, the `pinch_gesture` sibling with a
        two-dimensional delta — at the cursor target. Supply exactly one of
        `at = (x, y)` or `path = "<tag>"` (the widget under the cursor receives
        the offer). `delta_x` / `delta_y` are the INCREMENTAL pan in LOGICAL
        pixels, carrying the platform's own sign (a pan is direct manipulation:
        the content follows the fingers, unlike the sign-flipped `wheel()`
        scroll command); `phase` brackets the arc, one of ``"begin"`` /
        ``"update"`` / ``"end"`` / ``"cancel"``. The shell drains the inbox
        after this returns, applying `cursor_moved` then the pan offer. winit
        exposes no trackpad headless, so this RPC is the sole driver (§2 #2);
        follow up with `query(...)` / `snapshot(...)` to observe the slide.
        """
        if (at is None) == (path is None):
            raise ValueError("exactly one of `at` or `path` must be supplied")
        params: dict[str, Any] = {
            "delta_x": float(delta_x),
            "delta_y": float(delta_y),
            "phase": phase,
        }
        if at is not None:
            params["at"] = {"x": float(at[0]), "y": float(at[1])}
        else:
            assert path is not None
            params["path"] = path
        self.request("scene/pan_gesture", params)

    def smart_zoom_gesture(
        self,
        at: Optional[tuple[float, float]] = None,
        *,
        path: Optional[str] = None,
    ) -> None:
        """`scene/smart_zoom_gesture` typed wrapper (R1435 §5.35 §5.15).

        Drive a native SMART-ZOOM — the two-finger double tap, the toolkit
        native gesture event `SmartZoomNativeGesture` / winit
        `DoubleTapGesture` — at the cursor target. Supply exactly one of
        `at = (x, y)` or `path = "<tag>"`.

        The family's PHASE-LESS member: unlike `pinch_gesture()` /
        `rotation_gesture()` / `pan_gesture()` there is no payload and no
        ``phase`` — the platform reports one completed toggle, so each call is
        one committed state change with no arc to bracket. The anchor is the
        entire payload (it selects the object to fit), which is why the target
        is the only argument. Not to be confused with `double_click()`: that is
        two mouse press/release cycles, this is a buttonless trackpad gesture.
        The shell drains the inbox after this returns, applying `cursor_moved`
        then the offer; follow up with `query(...)` / `snapshot(...)`.
        """
        if (at is None) == (path is None):
            raise ValueError("exactly one of `at` or `path` must be supplied")
        params: dict[str, Any] = {}
        if at is not None:
            params["at"] = {"x": float(at[0]), "y": float(at[1])}
        else:
            assert path is not None
            params["path"] = path
        self.request("scene/smart_zoom_gesture", params)

    def scroll(
        self,
        path: str,
        *,
        to: Optional[tuple[int, int]] = None,
        by: Optional[tuple[int, int]] = None,
    ) -> None:
        """`scene/scroll` typed wrapper (R55.F §5.45).

        Programmatic scroll mutation — bypasses the InputRouter
        wheel/key activation arc and directly drives the attached
        `ScrollState`. Mutually exclusive — supply exactly one of:
          * `to = (x, y)` — absolute offset (clamped to `[0, max]`).
          * `by = (dx, dy)` — relative delta (saturating-add then
            clamped).

        Use for "jump to row N" patterns where simulating ten
        PageDown injections would be noisy. Follow up with
        `snapshot(source="paint")` to observe the new offset.
        """
        if (to is None) == (by is None):
            raise ValueError("exactly one of `to` or `by` must be supplied")
        params: dict[str, Any] = {"path": path}
        if to is not None:
            params["to"] = {"x": int(to[0]), "y": int(to[1])}
        else:
            assert by is not None
            params["by"] = {"dx": int(by[0]), "dy": int(by[1])}
        self.request("scene/scroll", params)

    def stderr_tail(self, n: int = 20) -> list[str]:
        return list(self._stderr_lines[-n:])

    def wait_self_exit(self, *, timeout: float = 8.0) -> int:
        """Block until the app exits ON ITS OWN and return its exit code (R1362).

        For a binding that closes ITSELF (`WindowControlSink` — hello-tray's
        Quit, sprag's dead-daemon poll thread). Every other demo ends by the
        harness signalling the app in `shutdown()`; here the app's own exit IS
        the assertion, so the harness must not race it with a SIGTERM.

        Returns the real exit code rather than asserting `== 0` so a caller can
        say which code it expects. `shutdown()` is a no-op afterwards (it
        already gates on `poll() is None`), so the `with` block still exits
        cleanly.
        """
        if self._proc is None:
            raise RpcError(-32099, "subprocess not running")
        try:
            return self._proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            raise AssertionError(
                f"app did not exit within {timeout}s of its own accord; "
                f"stderr tail:\n" + "\n".join(self.stderr_tail(20))
            ) from None


def assert_input_axes(state: dict, *, needs: Iterable[str], label: str = "input_state") -> None:
    """R1627 — assert `scene/input_state` carries the axes a reader USES.

    Deliberately a superset check, not set equality. Three demos used to
    hand-write the whole axis list and demand equality; R1619 added
    `held_pointer_buttons` and R1620 added `auto_scroll`, so all three went red
    and stayed red for five rounds — because the local gate runs only the demos
    a round touched and the full sweep is CI's. An ADDITIVE wire change must not
    break a reader that never looked at the new field.

    "No axis silently disappeared" is a different question and belongs where the
    emitter is: `pinion_rpc::dispatch::INPUT_STATE_AXES`, asserted against the
    emitted object in both directions by a Rust test. A demo cannot hold that
    census honestly, because nothing makes a Python list and a Rust `json!`
    literal land in the same diff.
    """
    missing = [axis for axis in needs if axis not in state]
    if missing:
        raise AssertionError(
            f"{label}: missing {missing}; got {sorted(state.keys())}"
        )


def assert_eq(actual: Any, expected: Any, label: str = "value") -> None:
    if actual != expected:
        raise AssertionError(
            f"{label}: expected {expected!r}, got {actual!r}"
        )


#: R1720 — arguments chosen so a verb that CAN refuse will: a name nothing is
#: called, an index nothing sits at, and a value of the wrong kind entirely.
#: Tried in order, and the first refusal is the one judged.
HOSTILE_ARGS: tuple[Any, ...] = ("no.such.thing,no.such.thing", -987654, True)


def assert_every_refusal_is_heard(
    tf,
    *,
    ext: str = "/external",
    said: str = "said",
    exempt: Optional[dict[str, str]] = None,
    args: Iterable[Any] = HOSTILE_ARGS,
) -> dict[str, int]:
    """★★★★★ R1720 §5.15 §2 #2 — **every action this surface publishes, driven
    until it refuses, and the person told every time.**

    §2 #2 makes the headless path the primary one, so "an agent drives and a
    person watches" is the ordinary state of a pinion screen. Measured on the
    three analysis screens the day this was written, in that state **55 verbs
    refused and 2 reached the person** — and the two that did were the two
    sites where somebody had written the pair out by hand. The refusal went to
    the caller; the screen carried on showing a sentence about some earlier act;
    and nothing anywhere could tell the two situations apart.

    So this drives the surface's own published action list, not a sample of it:
    a verb added next round is covered the moment it is declared, which is the
    property a hand-written list of verbs cannot have.

    Two things are asserted per refusing verb, and they check each other:

      * the wire says the person was told (`error.data.announced.reach == "at"`,
        naming the live region), and
      * that live region really carries the refusal — the sentence read back at
        `said` has the clause the agent was given.

    The second is what makes the first unforgeable. A surface can answer
    `Announced::At` without doing anything; it cannot also make the region say
    the right sentence.

    `exempt` maps a verb to the REASON it is allowed to refuse silently, in a
    sentence. A bare list would let a red be silenced by adding a name (R1640),
    and an exemption whose verb stops refusing fails too — an exemption that
    outlives its defect is a claim nobody re-checked.

    Returns the census: how many verbs were declared, how many refused, and how
    many were exempt. A verb that accepted every hostile argument is not a
    failure — some take no argument they can refuse — but it is counted, because
    a surface where nothing refuses proves nothing.
    """
    exempt = dict(exempt or {})
    schema = tf.query(f"{ext}/$schema")
    if isinstance(schema, str):
        schema = json.loads(schema)
    verbs = [f["path"] for f in schema if f.get("channel") == "invoke"]
    if not verbs:
        raise AssertionError(
            f"{ext} declares no action at all, so this gate would pass on nothing"
        )

    refused: list[str] = []
    unrefused: list[str] = []
    for verb in verbs:
        outcome = None
        for arg in args:
            try:
                tf.invoke(f"{ext}/{verb}", arg, with_origin=True)
            except RpcError as why:
                outcome = why
                break
        if outcome is None:
            unrefused.append(verb)
            continue
        refused.append(verb)
        why_person_missed = exempt.get(verb)
        data = outcome.data if isinstance(outcome.data, dict) else {}
        announced = data.get("announced") or {}
        reach = announced.get("reach")
        if why_person_missed is not None:
            if reach == "at":
                raise AssertionError(
                    f"{verb} is exempt because {why_person_missed!r}, and it "
                    f"announced at {announced.get('at')!r} — an exemption that "
                    "outlived its defect is a claim nobody re-checked"
                )
            continue
        if reach != "at":
            raise AssertionError(
                f"{verb} refused with {data.get('reason')!r} and the person was "
                f"told nothing (announced={announced or None}). Either the "
                "surface answers `Announced::at` with the tag of its live "
                "region, or it says in a sentence why it cannot."
            )
        clause = data.get("reason")
        heard = tf.query(f"{ext}/{said}")
        if isinstance(heard, str):
            heard = json.loads(heard) if heard else None
        if not isinstance(heard, dict):
            raise AssertionError(
                f"{verb}: the surface claims it announced at "
                f"{announced.get('at')!r} and {ext}/{said} reads {heard!r}"
            )
        # ★★★★ R1720 — the two kinds of refusal are held to OPPOSITE rules,
        # and conflating them is what the first draft of this gate did.
        #
        # A refusal the SURFACE authored travels as the producer's own sentence
        # (`ACTION_REFUSED` / `VALUE_OUT_OF_RANGE`), so the agent's copy and the
        # person's copy must be the same string — one fact, one wording.
        #
        # A refusal the FRAMEWORK authored travels as a TAG an agent matches on
        # (`UnknownInvokePath`, `PathIsAReadSlot`), and a tag is not a thing to
        # put in front of somebody. There the two must DIFFER, and the person's
        # copy has to be a sentence.
        surface_authored = outcome.code in (ACTION_REFUSED, VALUE_OUT_OF_RANGE)
        if surface_authored:
            if heard.get("clause") != clause:
                raise AssertionError(
                    f"{verb}: the agent was told {clause!r} and the person is "
                    f"reading {heard.get('clause')!r} — one refusal, two wordings"
                )
        else:
            if heard.get("clause") == clause:
                raise AssertionError(
                    f"{verb}: the person is reading the wire's own tag "
                    f"({clause!r}), which is a word an agent matches on and not "
                    "a thing anybody reads"
                )
            if " " not in (heard.get("clause") or "").strip():
                raise AssertionError(
                    f"{verb}: the person is reading {heard.get('clause')!r}, "
                    "which is not a sentence"
                )
        if heard.get("tone") != "refused":
            raise AssertionError(
                f"{verb}: the person's copy is toned {heard.get('tone')!r}, so a "
                "reader who cannot see the screen is told about a thing that "
                "did not happen when they are next idle"
            )

    for verb, why in exempt.items():
        if verb not in refused:
            raise AssertionError(
                f"{verb} is declared silent-on-refusal because {why!r} and it "
                "never refused — a declaration over an arm nobody ran is a "
                "claim, not a check"
            )

    census = {
        "declared": len(verbs),
        "refused": len(refused),
        "exempt": len(exempt),
        "never_refused": len(unrefused),
    }
    print(
        f"[refusal-heard] {tf.example}: {census['refused']} of "
        f"{census['declared']} declared action(s) refused and were heard, "
        f"{census['exempt']} exempt, {census['never_refused']} refused nothing"
    )
    return census


#: R1564 §5.15 (PINION-PR82) — JSON-RPC code for an action the surface REFUSED
#: to fire, as distinct from `-32602 Invalid params` (the parameters were fine).
#: Mirrors `pinion_rpc::ACTION_REFUSED`; see its doc for why the split exists.
ACTION_REFUSED = -32005

#: R1565 §5.15 (PINION-PR82) — JSON-RPC code for a written value outside the
#: slot's accepted range. Mirrors `pinion_rpc::VALUE_OUT_OF_RANGE`. Split from
#: `-32602` for the payload rather than the category: the value really was a bad
#: parameter, but `error.data` now carries the surface's own sentence naming the
#: range, and `-32602` is published as carrying a closed vocabulary.
VALUE_OUT_OF_RANGE = -32006

#: R1667 §5.15 — JSON-RPC code for a READ whose family is declared and whose
#: argument addresses no member: the name is right and the index is not.
#: Mirrors `pinion_rpc::NO_SUCH_MEMBER`. Opposite instructions to
#: `-32602 UnknownIntrospectPath`, which means stop asking for that name at all.
NO_SUCH_MEMBER = -32007

#: R1667 §5.15 — JSON-RPC code for a declared read this instance cannot answer,
#: because it holds no state to read from. Mirrors
#: `pinion_rpc::READ_UNAVAILABLE`. Distinct from `NO_SUCH_MEMBER` because the
#: argument was never the problem: nothing the client varies about the call
#: helps until the surface is bound.
READ_UNAVAILABLE = -32008

#: R1565.2 — the codes whose `error.data` is the SURFACE's own sentence rather
#: than a word from this dispatcher's closed vocabulary. Mirrors the
#: `data_is_prose` column `rpc/errors` publishes, and is asserted equal to it by
#: `r1564_refusal_states_why.py` phase (H) — a mirror nothing compares is a
#: second contract free to drift from the first.
#:
#: A test may only MATCH a payload under a code that is not in here. That is the
#: rule `assert_rpc_error` enforces below: prose is shown, never branched on.
PROSE_DATA_CODES = frozenset(
    {ACTION_REFUSED, VALUE_OUT_OF_RANGE, NO_SUCH_MEMBER, READ_UNAVAILABLE}
)

#: The helper that asserts each prose-carrying code, for the error message a
#: caller reaching for the wrong one gets.
_PROSE_HELPER = {
    ACTION_REFUSED: "assert_action_refused(fn, saying=...)",
    VALUE_OUT_OF_RANGE: "assert_out_of_range(fn, saying=...)",
    NO_SUCH_MEMBER: "assert_no_such_member(fn, saying=...)",
    READ_UNAVAILABLE: "assert_read_unavailable(fn, saying=...)",
}


def _assert_refused_with_reason(fn, code: int, kind: str, saying: str) -> str:
    """The body every prose-carrying refusal assertion shares.

    Lifted in R1670 when the read channel's two arrived and made it a fourth and
    fifth copy — the three-site rule, and the copies had already started to
    differ in what they said when the call SUCCEEDED, which is the half a reader
    of a failure message needs most.
    """
    try:
        fn()
    except RpcError as exc:
        assert_eq(exc.code, code, f"{kind} saying {saying!r}: JSON-RPC code")
        reason = exc.data if isinstance(exc.data, str) else (exc.data or {}).get("reason")
        assert isinstance(reason, str), (
            f"a {kind} must state a reason; error.data was {exc.data!r}"
        )
        assert saying in reason, (
            f"the {kind} did not say {saying!r}; it said {reason!r}"
        )
        return reason
    raise AssertionError(
        f"expected a {kind} saying {saying!r}, but the call succeeded"
    )


def assert_no_such_member(fn, *, saying: str) -> str:
    """Assert `fn()` is refused because the argument addresses NO MEMBER of a
    declared family, with a stated reason containing `saying`.

    The read channel's peer of `assert_action_refused`, and the reason the four
    demos this round repaired could not be migrated when R1667 split the read
    refusal into four arms: the wire gained a vocabulary and the harness did
    not, so a demo that wanted to say "the name is right and the index is not"
    had no way to say it and went on asserting the collapsed answer.

    Distinct from `assert_rpc_error(..., data="UnknownIntrospectPath")` in what
    it tells the CALLER, which is the whole point of the split: that one means
    stop asking for this name, and this one means read the family's count path
    and ask again.
    """
    return _assert_refused_with_reason(fn, NO_SUCH_MEMBER, "no-such-member refusal", saying)


def assert_read_unavailable(fn, *, saying: str) -> str:
    """Assert `fn()` is refused because this instance holds no state to answer a
    DECLARED read, with a stated reason containing `saying`.

    Separate from `assert_no_such_member` for the same reason the codes are: the
    caller's next move differs. A missing member says try another index; this
    says the argument was never the problem.
    """
    return _assert_refused_with_reason(
        fn, READ_UNAVAILABLE, "read-unavailable refusal", saying
    )


def widest_flat_run(png, rect, colour, *, channel_tolerance: int = 2):
    """The widest run of `colour` across `rect`'s mid-height row, as
    `(start_x, length)` in absolute pixels — or `(rect_x, 0)` if there is none.

    ★ R1670 — the helper three demos needed and none had. Each sampled a
    CONSTANT inset into a rect whose size is derived, with a comment claiming
    the point was clear of the glyph, and `r760_fab` measured what that is worth:
    on the smallest FAB the flat run of the declared accent is 15px wide and the
    icon's ink begins at 16, so the constant 12 sat three pixels from a glyph's
    antialiased edge. It passed on a real GPU and on lavapipe here, and failed
    in CI twice, reading a colour 16 off the one it wanted.

    Scanning for the run instead is a MEASUREMENT of the same claim, and it is
    the stronger one: a fill the binding never painted has no run at all, where
    a single sample can be right by luck about a wrong picture. Callers assert
    the run's WIDTH (an interior exists) and then read its centre.

    `colour` is an `(r, g, b)` the caller took from the SCENE, so this is not
    circular: the pixels are being held to what the scene declared.
    """
    x, y, w, h = rect
    row = sample_png_points(png, [(x + dx, y + h // 2) for dx in range(w)])
    best_start, best_len, run_start = x, 0, None
    for dx, sample in enumerate([*row, None]):
        hit = sample is not None and all(
            abs(sample[n] - colour[n]) <= channel_tolerance for n in range(3)
        )
        if hit and run_start is None:
            run_start = dx
        elif not hit and run_start is not None:
            if dx - run_start > best_len:
                best_start, best_len = x + run_start, dx - run_start
            run_start = None
    return best_start, best_len


def assert_interior_is(png, rect, colour, *, label: str, floor: int = 8,
                       tolerance: int = 12):
    """Assert `rect` paints a run of `colour` at least `floor` wide across its
    middle, and that the run's centre really is that colour.

    The two halves are different claims and both are wanted: the width says the
    binding painted the fill at all (a wrongly-painted surface has no run), and
    the centre sample says the pixels match the value the scene declared. See
    `widest_flat_run` for why the run is measured rather than guessed at.
    """
    start, length = widest_flat_run(png, rect, colour)
    _x, y, w, h = rect
    assert length >= floor, (
        f"{label}: the widest run of the declared fill across a {w}x{h} surface "
        f"is {length}px and an interior is at least {floor}px — the surface is "
        f"not painting the fill its scene declares"
    )
    middle = sample_png_points(png, [(start + length // 2, y + h // 2)])[0]
    assert_pixel_eq(middle, (*colour, 255), label, tolerance=tolerance)
    return middle


def assert_out_of_range(fn, *, saying: str) -> str:
    """Assert `fn()` is refused as OUT OF RANGE, with a stated reason containing
    `saying`. Returns the full reason.

    The write channel's peer of `assert_action_refused`, and separate from it on
    purpose: the two are different codes because they are different facts about
    the caller's request (its ARGUMENT was outside a range, versus its argument
    was fine and the surface declined), and a helper that accepted either would
    let a test pass while the wire reported the wrong one.
    """
    return _assert_refused_with_reason(
        fn, VALUE_OUT_OF_RANGE, "out-of-range refusal", saying
    )


def assert_action_refused(fn, *, saying: str) -> str:
    """Assert `fn()` is refused by the SURFACE, with a stated reason containing
    `saying`. Returns the full reason.

    The wire peer of `pinion_core::test_fixtures::assert_refused_saying`, and the
    replacement for `assert_rpc_error(..., data="InvokeRejected")`.

    That older call asserted the wire carried the *variant name* — the whole of
    what a refusal could say before R1564, and, as PINION-PR82 measured
    downstream, not enough for a consumer to build a message from. The
    mechanical migration (drop the `data=`) would have left these demos checking
    only that something failed, which is weaker than what they replaced. So this
    asserts the fact R1564 added: the producer's own sentence, under the code
    that says a well-formed call was declined.
    """
    return _assert_refused_with_reason(fn, ACTION_REFUSED, "refusal", saying)


def assert_rpc_error(fn, *, data: Any, code: int = -32602) -> None:
    """Assert `fn()` raises a JSON-RPC error carrying `data`, under `code` — and
    did NOT succeed.

    The typed peer of `assert_eq` for the wire's failure channel. The dispatch
    layer maps a framework-diagnosed `InvokeError` / `InterveneError` variant to
    a `-32602 Invalid params` carrying the variant name in `error.data` (e.g.
    `"UnknownInvokePath"`, `"ReadOnly"`), so `data=` proves the failure travelled
    the real wire with the right typed reason — not a silent success and not a
    generic transport error. Callers pass a zero-arg lambda:
    `assert_rpc_error(lambda: g.invoke(P, A), data="UnknownInvokePath")`.

    R1564 — an action the SURFACE refused is no longer one of these: it arrives
    under `ACTION_REFUSED` carrying the producer's sentence. Use
    `assert_action_refused` for it.

    # Why `data` is REQUIRED (R1565.2)

    It used to default to `None`, which made "some error came back under -32602"
    a whole assertion. Twenty of the 114 call sites were written that way, and
    the shape cost this project a red `main` twice in two rounds: R1564 and
    R1565 each split a fact out of `-32602` into its own code, each ran a census
    of what read the old contract, and each census filtered on `data=` — so the
    data-less sites were invisible to it BOTH times. Four of them then failed in
    CI 50 minutes after the push, saying `expected -32602, got -32006`: an
    assertion that had never named which refusal it wanted could not say that
    the refusal it got was the right one under a new name.

    A demo that does not name the fact it expects is also passing for a reason
    nobody chose. Three of the twenty were proving something other than their
    own comment claimed — `r1441` asserts "it is a READ; a client cannot set a
    card's height" and what the wire actually answered was
    `UnknownIntervenePath`, which says the path does not exist, about a path
    `$schema` declares (R1566's subject).

    So the fact is named at every site. `-32602` carries a word from a closed
    vocabulary — `rpc/errors` publishes exactly that — so naming it is cheap and
    stable, and the codes that carry PROSE are refused here by name, pointing at
    the helper that asserts a sentence instead.
    """
    if code in PROSE_DATA_CODES:
        raise AssertionError(
            f"assert_rpc_error cannot check code {code}: rpc/errors publishes its "
            f"error.data as the surface's own prose, which a test must show and "
            f"never match. Use {_PROSE_HELPER[code]}."
        )
    try:
        fn()
    except RpcError as exc:
        assert_eq(exc.code, code, f"{data!r}: JSON-RPC code")
        assert_eq(exc.data, data, "error.data variant")
        return
    raise AssertionError(
        f"expected RpcError (code={code}, data={data!r}), but the call succeeded"
    )


def rpc_error_data(
    fn,
    *,
    code: int = -32602,
    expect: Any = str,
    label: str = "call",
) -> Any:
    """Run `fn()` expecting a JSON-RPC error; return its `error.data`.

    The capture peer of `assert_rpc_error`, for the demos that do not merely
    compare the payload but go on to READ it — pick a member out of it, feed
    it to a later assertion, compare two refusals against each other.
    Lifted R1485 after the shape reached four hand-rolled sites (three inside
    `r1386_snapshot_path_error.py` alone, plus `r1485_refusal_origin.py`),
    each re-deriving the same three mechanical steps: fail loudly if the call
    unexpectedly SUCCEEDS (a silent success hides exactly the regression
    these demos exist to catch), check the JSON-RPC code, and confirm the
    payload's shape before the caller indexes into it.

    `expect` defaults to `str` because a bare word is the ratified §5.12
    refusal payload; a caller that asked the wire to disclose more (R1485
    `with_origin`) opts into `expect=dict`, mirroring the request it made.
    A shape mismatch is reported here rather than as a `TypeError` from the
    caller's first subscript, so a build that accepted an opt-in and dropped
    it is named as that.
    """
    try:
        fn()
    except RpcError as exc:
        assert_eq(exc.code, code, f"{label}: JSON-RPC error code")
        assert isinstance(exc.data, expect), (
            f"{label}: expected error.data as {expect.__name__}, got {exc.data!r}"
        )
        return exc.data
    raise AssertionError(f"{label}: expected an RpcError, but the call succeeded")


def call(tf, method: str, params: Any = None) -> Any:
    """The `result` of a request — `tf.request` answers with the envelope.

    Lifted R1566 at the eighth byte-identical copy. Seven demos had written it
    out (`r1539`, `r1546`, `r1551`, `r1552`, `r1559`, `r1560`, `r1564`), each
    with the same body and the same docstring, because `request` returns the
    frame and almost every caller wants what is inside it. Mechanical wiring
    with no opinion in it, so the Rule-of-Three lift is unconditional
    ([[three-site-internal-duplication-substrate-lift]]).

    The `assert` is the part worth having in one place: a `None` envelope means
    the request produced no response at all, and a caller that indexed straight
    into `.result` would meet that as an `AttributeError` naming nothing.
    """
    resp = tf.request(method, params if params is not None else {})
    assert resp is not None, f"{method} answered nothing"
    return resp.result


def declared_read_paths(fields: Any) -> list[str]:
    """The paths of `fields` a READ walk should visit: declared on the read
    channel, addressable as spelled.

    R1643 — one definition of the population, because five walkers had their own
    and the fix reached one of them. R1637 made the declaration a precondition of
    dispatch, so a walk that queries every declared path now FAILS on the first
    declared action with `PathIsAnAction`; four demos over
    `hello-answer-origin` did exactly that and stayed red in CI from R1637,
    while R1641.6 repaired the fifth copy — a Rust unit test — and its own
    blast-radius note read the two sibling *unit* walks and not these.

    `assert_declared_channels_are_true` (R1566) already asserted both channels
    over a whole surface, and nothing routed the copies to it. That is the
    lesson worth keeping: the abstraction existed for 76 rounds and the
    duplication was not for want of one, it was for want of a POINTER to it. So
    this is deliberately the small piece those walks each need — the filter —
    rather than a second whole-surface assertion they would have to be rewritten
    around.

    A parametric template is excluded because it addresses nothing (`cell.<row>`
    is not a path), which is the same reason `scene/snapshot` omits it, and
    `SchemaField::EMPTY`'s blank path for the same reason.
    """
    if isinstance(fields, dict):
        fields = fields.get("fields", fields)
    return [
        f["path"]
        for f in fields
        if f.get("path") and "<" not in f["path"] and f.get("channel", "read") == "read"
    ]


#: What the wire says when an `intervene` names a path the surface does not
#: have. Two spellings because the refusal is worded by whoever noticed first:
#: the framework's own dispatch answers `UnknownIntervenePath` and an
#: `External` answers `InterveneError::UnknownPath`. Named here rather than
#: matched at three call sites, so a third spelling is one edit.
UNKNOWN_INTERVENE_PATH = ("UnknownIntervenePath", "UnknownPath")


def assert_declared_channels_are_true(tf, external: str = "/external") -> dict:
    """Assert every scalar path in `external`'s `$schema` answers on the channel
    it declares. Returns `{"read": n, "invoke": n}`, the counts it checked.

    R1566 §2 #7 — the gate that makes `SchemaChannel` load-bearing.

    R1504 added the channel and nothing ever branched on it, so `Read` was a
    silent default and a surface could declare a verb as a readable slot with
    nothing to notice. Measured at R1566 over nine bindings: **116 of 288**
    declared scalar fields — 40% — were actions declared as read slots.
    `hello-data-grid` published `add_row`, `paste` and `reset_all` as string
    fields an agent could read; `hello-untangle`'s own demo docstring calls
    `untangle` "a **verb**" beside a declaration that says otherwise.

    Nobody was lying. The declaration simply had no consumer, and a fact with no
    consumer is a fact nothing keeps true. R1566 gave it one — the refusal a
    client gets for addressing a path on the wrong channel is now derived from
    it — which is what turned 116 silent mistakes into a measurable defect and
    is why this gate ships with them.

    # It cannot mutate

    `query` is used on every path and `intervene` on every path. A `read` path
    must answer the query and must not TAKE an ill-typed write; an `invoke` path
    must refuse both with `PathIsAnAction`.

    That is the 2x2 — two channels, two directions — and until R1644.1 the walk
    checked three of its four cells. The fourth (writing to a declared action)
    is as cheap and as safe as the others: it is refused before anything is
    dispatched, so nothing fires.

    Probing the write direction of an *action* is deliberately still not done:
    `invoke` on a path that is an action fires the action, and a gate that can
    change the thing it is inspecting is a gate no demo can afford to run
    mid-scenario.

    R1644 — that sentence used to cover both write directions, and it is only
    true of one. Intervening on a path the surface declares as a READ is
    refused by definition and changes nothing, so the stated danger does not
    apply to it; one sentence bundling two directions deferred the safe one
    along with the unsafe one.

    ★ What the probe then found is not what it was written for. The guess was
    that a read declared in `$schema` and missing from the surface's own
    `intervene` match arm would answer "no such path". It cannot: `intervene.rs`
    derives `ReadOnly` for any declared **scalar** the impl declines, so the
    refusal has **two independent sources and either alone suffices**. Two
    counterfactuals say so — removing a path from the surface's arm is not
    observable (the framework answers), and breaking the framework's derivation
    is not observable either (the arm answers). So there is no assertion to make
    about that path being *known*, and none is made; the debt registered on the
    first guess was withdrawn and re-registered as what it is
    ([[debt-a-read-only-refusal-has-two-unguarded-sources]]).

    # The write probe cannot ask for `ReadOnly`, and finding out why was the
    # finding

    A declared read is not necessarily read-**only**: some are writable, and
    `SchemaChannel` has no way to say which ([[debt-a-schema-channel-cannot-say-a-slot-is-writable]],
    open since R1566). So the strongest thing this can assert is that the
    surface **knows the path** on the write channel — anything but
    `UnknownPath`. The first draft demanded `ReadOnly` and its first run
    reported `hello-text-field`'s `mode`, which is writable and answered a type
    refusal; that is the standing debt showing up rather than a defect, and the
    check is written to what the declaration can express.

    Nothing mutates. The value sent is deliberately of the **wrong declared
    type** — `Text` at a field the schema calls `int`, an `Int` at anything
    else — so a writable slot refuses on the type and a read-only one refuses
    on the channel, and neither takes the value. A probe that sent a plausible
    value would fire the write it was inspecting.

    A **negative control** runs first, for R1640's reason: a surface that
    refuses every name with one stated sentence would satisfy any refusal
    check, so an invented path is probed and a surface that does not call it
    unknown is reported rather than trusted.

    Parametric families (`cell.<row>.<col>`) are skipped — the placeholder is not
    an address, so there is nothing to ask for. `SchemaField::EMPTY`'s blank path
    is skipped for the same reason.
    """
    fields = tf.query(f"{external}/$schema")
    if isinstance(fields, dict):
        fields = fields.get("fields", fields)
    checked = {"read": 0, "invoke": 0}
    unreadable: list[str] = []
    readable_actions: list[str] = []
    took_the_probe: list[str] = []
    writable_actions: list[str] = []
    # The negative control: a name this surface cannot have published. If
    # writing to it is refused as `ReadOnly`, the surface is answering by habit
    # rather than from its declaration and the write probe below proves nothing.
    absent_probe = "r1644_no_such_path"
    try:
        tf.intervene(f"{external}/{absent_probe}", 0)
        knows_everything = True
    except RpcError as exc:
        knows_everything = exc.data not in UNKNOWN_INTERVENE_PATH
    assert not knows_everything, (
        f"{external}: writing to {absent_probe!r}, a path it cannot have "
        f"declared, was not refused as an unknown path — so this surface answers "
        f"the write channel by habit, and the check below would pass whatever "
        f"its declaration said"
    )
    for field in fields:
        path = field.get("path") or ""
        if not path or "<" in path:
            continue
        channel = field.get("channel", "read")
        if channel == "read":
            checked["read"] += 1
            try:
                tf.query(f"{external}/{path}")
            except RpcError as exc:
                unreadable.append(f"{path} ({exc.data!r})")
            # Deliberately the wrong declared type, so a writable slot refuses
            # on the type and a read-only one on the channel: nothing moves.
            # `type`, the key the WIRE uses. The first draft read `ty`, the
            # name on the Rust struct, so every probe fell through to the
            # integer and a writable int slot TOOK it — the gate mutated what
            # it was inspecting, which is the one thing it must not do. Read
            # the wire's own spelling; do not guess it from the type that
            # produces it.
            wrong = "" if field.get("type") == "int" else 0
            try:
                tf.intervene(f"{external}/{path}", wrong)
                took_the_probe.append(f"{path} ({wrong!r})")
            except RpcError:
                # Any refusal is fine. `ReadOnly` and a type refusal are both
                # correct answers and the declaration cannot say which to
                # expect — a declared read is not necessarily read-ONLY, and
                # `SchemaChannel` has no word for writable
                # ([[debt-a-schema-channel-cannot-say-a-slot-is-writable]]).
                # What matters is that the value was not taken.
                pass
        else:
            checked["invoke"] += 1
            try:
                tf.query(f"{external}/{path}")
                readable_actions.append(path)
            except RpcError as exc:
                if exc.data != "PathIsAnAction":
                    readable_actions.append(f"{path} (refused {exc.data!r})")
            # The fourth cell. Refused before dispatch, so the action does not
            # fire — measured, not assumed, before this was turned on.
            try:
                tf.intervene(f"{external}/{path}", 0)
                writable_actions.append(f"{path} (accepted a write)")
            except RpcError as exc:
                if exc.data != "PathIsAnAction":
                    writable_actions.append(f"{path} (refused {exc.data!r})")
    assert not unreadable, (
        f"{external}: {len(unreadable)} path(s) declared on the READ channel "
        f"that query does not answer — the surface publishes a name and then "
        f"says it does not exist: {unreadable}"
    )
    assert not readable_actions, (
        f"{external}: {len(readable_actions)} path(s) declared on the INVOKE "
        f"channel that do not refuse a read as PathIsAnAction: {readable_actions}"
    )
    assert not writable_actions, (
        f"{external}: {len(writable_actions)} path(s) declared on the INVOKE "
        f"channel that do not refuse a WRITE as PathIsAnAction — an action is "
        f"not a slot, and a client told otherwise will address it as one: "
        f"{writable_actions}"
    )
    assert not took_the_probe, (
        f"{external}: {len(took_the_probe)} path(s) ACCEPTED a deliberately "
        f"ill-typed write, so this gate changed the surface it was inspecting "
        f"— the probe's type is wrong, or the slot takes anything: "
        f"{took_the_probe}"
    )
    return checked


def assert_disclosed(result: Any, label: str = "call") -> dict:
    """Assert `result` is an origin-disclosing envelope; return it.

    The success-channel peer of `rpc_error_data`'s `expect=dict`. A wire that
    accepts `with_origin` and then ignores it answers with the bare value, so
    the caller's first `result["origin"]` dies as a `TypeError` /
    `AttributeError` about `int` — the defect goes unnamed at exactly the
    moment a counterfactual is asking whether the guard can name it (measured
    R1487: reverting the disclosure produced `'int' object has no attribute
    'keys'`).

    Lifted R1487 after the shape reached a third demo: `r1482_answer_origin`
    and `r1485_refusal_origin` each hand-rolled the same `sorted(x.keys()) ==
    ["origin", "value"]` check before reading the envelope.
    """
    assert isinstance(result, dict) and sorted(result) == ["origin", "value"], (
        f"{label}: asked the wire to disclose the origin and got the bare "
        f"answer instead: {result!r}"
    )
    return result


def wait_until(
    predicate,
    *,
    timeout: float = 8.0,
    interval: float = 0.04,
    desc: str = "condition",
) -> Any:
    """Poll `predicate()` until it returns a truthy value (returned) or
    `timeout` seconds elapse (raises `AssertionError`).

    Replaces a fixed `time.sleep()` between an action and the assertion
    that observes its effect. An action driven over RPC (a click / key /
    focus change) lands in the deferred-input inbox and applies on the
    next shell frame, and `scene/snapshot from=paint` reads the last
    *rendered* frame (R705) — so a fixed sleep races the render under
    load (the R694 sweep flake). Polling makes the demo deterministic on
    the *observed* post-action state instead of wall-clock, so it stays
    green whatever the machine load. See [[introspection-from-paint-not-screen]].
    """
    deadline = time.monotonic() + timeout
    last: Any = None
    while True:
        last = predicate()
        if last:
            return last
        if time.monotonic() >= deadline:
            raise AssertionError(
                f"wait_until timed out after {timeout}s: {desc} (last={last!r})"
            )
        time.sleep(interval)


def without_extent(verdict: dict) -> dict:
    """A conformance verdict with the size it was read at taken off it.

    ★★★★★ R1770 — lifted the round the extent was introduced, because three
    demos needed it in the same edit and they are the three that compare ONE
    build in TWO placements: a section standalone in its own window against the
    same section as a page of the assembled tool. Those two verdicts are equal
    in everything except where they were read, and until this round that was
    invisible — the comparison was a plain equality and it passed, because
    neither answer said what extent it came from.

    Dropping the qualifier here rather than in each demo keeps the claim those
    checks exist for (*one build, two placements*) and leaves the new fact to be
    asserted separately, which is the honest split: the sameness is about the
    BUILD and the difference is about the WINDOW, and a check that folded them
    together could no longer fail for the right reason.

    Removes `at` and `read_where_written` from the report and `at` from every
    surface row. `written_at` is KEPT — it is a fact about the specification
    rather than about this reading, and two placements judged against different
    canons would be exactly the drift these checks watch for.
    """
    stripped = {
        key: value
        for key, value in verdict.items()
        if key not in ("at", "read_where_written")
    }
    if isinstance(stripped.get("surfaces"), dict):
        stripped["surfaces"] = {
            name: {key: value for key, value in row.items() if key != "at"}
            for name, row in stripped["surfaces"].items()
        }
    return stripped


def resize_and_settle(
    tf: "RpcSubprocess",
    size: "tuple[int, int]",
    *,
    timeout: float = 8.0,
) -> Any:
    """Drive a real window resize and answer the paint snapshot once the new
    size has actually been RENDERED.

    ★★★★★ R1686 — lifted because four demos had written it and three had
    written it wrong. `scene/snapshot from=paint` reads the last rendered frame
    (R705, and that is the point — introspection from the paint rather than
    from a re-render nobody saw), so a resize followed by a fixed `tick` is a
    sleep racing the render: the read answers with the PREVIOUS window's
    rectangles, and every derived report — `scene/containment`,
    `scene/scroll_reach` — answers about that window too.

    Measured: R1685's demo read the opening body height at the tall size once
    under load and passed three times idle. A demo that is green when the
    machine is quiet is exactly what [[zero-flake-policy]] refuses, and one
    that has already been written four times is the harness's job.

    Waits on the ROOT's own width and height, because those are the two numbers
    a resize is about — an outcome, not an elapsed interval.

    ★★★★★ R1710 — it waits for the size the response says was **GRANTED**, not
    for the size that was asked. A window declares a floor; an ask below it is
    resolved up to that floor and the window will never take the asked size, so
    a wait for the ask is a wait for something that cannot happen. Measured: on
    a display with a window manager, `r1708_a_drag_is_answered_once.py` asked
    the dashboard (floor 1440x900) for 880 of height and timed out here after 8
    seconds, reporting "the window settles at (1637, 880)" — a sentence about
    the framework for a size the framework had been told to refuse. On the bare
    display CI runs on, nothing enforced the floor and the same call passed.

    It also asserts the response's granted size against the PAINTED rectangle,
    so the wire and the window cannot disagree — the divergence R1710 measured
    was invisible precisely because no caller ever compared them.

    Returns the settled `from=paint` snapshot, so a caller reads the frame it
    waited for rather than taking another one that could be a frame later.
    """
    resp = tf.request("scene/resize", {"width": size[0], "height": size[1]})
    assert resp is not None and resp.result is not None, (
        f"scene/resize to {size} was accepted"
    )
    outcome = resp.result
    granted = (outcome["width"], outcome["height"]) if isinstance(outcome, dict) else size
    if granted != size:
        # Loud rather than silent: a caller asked for a size its own window
        # declared it will not take, and the round's record should say so.
        print(
            f"[harness] scene/resize {size} -> granted {granted} "
            f"(width {outcome['width_bound']}, height {outcome['height_bound']})"
        )

    def settled() -> Any:
        shot = tf.snapshot(source="paint", viewport=granted)
        rect = shot.get("rect", {}) if isinstance(shot, dict) else {}
        return shot if (rect.get("w"), rect.get("h")) == granted else None

    return wait_until(
        settled,
        timeout=timeout,
        desc=f"the window settles at {granted} after a resize asking {size}",
    )


def assert_targets_survive_resize(
    tf: "RpcSubprocess",
    sizes: "Iterable[tuple[int, int]]",
    *,
    label: str = "",
) -> "dict[tuple[int, int], dict[str, Any]]":
    """★★★★★ R1700 — what is drawn is what is pressed, at EVERY window size.

    The half `_gate_pointer_targets` cannot reach on its own. That gate runs at
    the size the demo booted at, and a screen laid out at its design size
    agrees with itself by construction — the capture viewer did, on every gate,
    while a person maximised the window and found that nothing responded.
    Measured before the repair, driving the real shell at 2494x1011: of the 166
    painted rectangles that moved, **166** had stopped being pressable where
    they were drawn, and `scene/pointer_target` classifies every one of them
    `unreachable`.

    So this resizes, waits for the frame to actually land (`resize_and_settle`
    — a fixed tick after a resize is a bet on the render, R1686), and requires
    at every size that no painted rectangle disagrees with its own paint.

    ★ It also requires `deliverable > 0` at each size. Without that the check
    is satisfiable by a screen that answers "nothing is addressable" everywhere
    — the shape R1691 named, where a total is met by declaring everything
    silent — and the failure mode this exists to catch turns every addressable
    rectangle into a silent one.

    Returns the report per size, so a caller can assert on the numbers as well
    as on their absence of defects.
    """
    reports: "dict[tuple[int, int], dict[str, Any]]" = {}
    for size in sizes:
        resize_and_settle(tf, size)
        resp = tf.request("scene/pointer_target")
        assert resp is not None and resp.result is not None, (
            f"scene/pointer_target answers at {size}"
        )
        report = resp.result
        reports[size] = report
        answered = [s for s in report["surfaces"] if s["answers"]]
        assert answered, (
            f"{label or tf.example}: no surface resolves presses at {size}, so "
            f"this check would pass whatever the screen did"
        )
        broken = [
            f"{row['tag']} is {row['verdict']} — {row['by_name']!r} by name, "
            f"{row['at_centre']!r} at its centre ({row['x']},{row['y']})"
            for surface in answered
            for row in surface["rows"]
            if row["verdict"] == "unreachable"
        ]
        assert not broken, (
            f"{label or tf.example}: at {size[0]}x{size[1]}, "
            f"{len(broken)} painted rectangle(s) are not pressable where they "
            f"are drawn — {'; '.join(broken[:6])}"
        )
        delivered = sum(s["deliverable"] + s["handle"] for s in answered)
        assert delivered > 0, (
            f"{label or tf.example}: at {size[0]}x{size[1]} not one painted "
            f"rectangle is addressable, so 'no disagreement' means nothing here"
        )
        # ★★★★★ R1700 — the framework's own half, and it is here because a
        # counterfactual PASSED without it. Falsifying the size the framework
        # RECORDS for a surface left every check on all three analyser screens
        # green: none of them has an addressable rectangle whose position
        # depends on the window's height, so the vertical axis of the size
        # question had no consumer to disagree through. The invariant
        # `announce_external_sizes` states in its own comment — the size
        # announced and the size a pointer fraction is a fraction OF are one
        # derivation — was claimed and checked by nothing.
        #
        # Asserted here rather than in the boot gate on purpose: this helper
        # runs where the agreement has been measured, and the boot gate runs on
        # every demo in the tree, whose surfaces have not been.
        misannounced = [
            f"{s['surface']} was painted {tuple(s['painted_size'])} and is told "
            f"{s['announced'] and tuple(s['announced'])}"
            for s in report["surfaces"]
            if s["announced"] is not None and tuple(s["announced"]) != tuple(s["painted_size"])
        ]
        assert not misannounced, (
            f"{label or tf.example}: at {size[0]}x{size[1]} the framework tells a "
            f"surface a size it was not painted at — {'; '.join(misannounced)}. "
            f"Every pointer fraction that surface resolves is a fraction of the "
            f"rectangle it was painted in, so the two have to be one number."
        )
    return reports


def wait_query(
    tf: "RpcSubprocess",
    path: str,
    expected: Any,
    *,
    timeout: float = 8.0,
    interval: float = 0.04,
    desc: Optional[str] = None,
) -> None:
    """Poll `tf.query(path)` until it equals `expected` (R883 zero-flake).

    The typed convenience for the dominant demo shape — an action
    followed by `assert_eq(tf.query(P), V, label)`. Subsumes the
    assertion: returns once equal, raises `AssertionError` carrying the
    last observed value on timeout, so the failure message keeps the
    `expected X, got Y` diagnostics `assert_eq` gave. Use a raw
    [`wait_until`] for non-equality predicates (`> 0`, set membership).

    A dispatch over RPC commits its full effect chain (deferred-input
    drain, intent `handle_tail`, the R705 dirty-on-mutation paint
    re-store) before the response is written, so for plain state this
    returns on the first poll; the polling is the [[zero-flake-policy]]
    guard for the genuinely asynchronous paths (winit `scene/resize`
    round trips, window create/close, native-input injection,
    wall-clock animation) where a fixed sleep raced the event loop.
    """
    label = desc or f"query {path} == {expected!r}"
    deadline = time.monotonic() + timeout
    while True:
        last = tf.query(path)
        if last == expected:
            return
        if time.monotonic() >= deadline:
            raise AssertionError(
                f"{label}: expected {expected!r}, got {last!r} "
                f"after {timeout}s"
            )
        time.sleep(interval)


def wait_snap(
    tf: "RpcSubprocess",
    predicate,
    *,
    source: str = "paint",
    viewport: Optional[tuple[int, int]] = None,
    window: Optional[str] = None,
    timeout: float = 8.0,
    interval: float = 0.04,
    desc: str = "snapshot condition",
) -> Any:
    """Poll `tf.snapshot(...)` until `predicate(snap)` is truthy (R883).

    Returns the first snapshot satisfying the predicate so the caller's
    follow-up assertions read the *same* observed frame (no re-fetch
    race). The paint-read peer of [`wait_query`]: gate on the first
    condition the demo asserts about the post-action frame, then keep
    the remaining assertions plain against the returned snap.

    `window` (R883.1) addresses a named window spec — the one home for
    the window-scoped polling every multi-window demo previously
    hand-rolled. A window that may not EXIST yet (e.g. a dock tear-off
    minting it mid-demo) still needs a caller-side `RpcError` guard;
    an absent window is an error, not a not-yet state, by default.
    """
    def poll() -> Any:
        snap = tf.snapshot(source=source, viewport=viewport, window=window)
        return snap if predicate(snap) else None

    return wait_until(poll, timeout=timeout, interval=interval, desc=desc)


def wait_paint_beyond(
    tf: "RpcSubprocess",
    baseline: int,
    *,
    window: Optional[str] = None,
    timeout: float = 8.0,
    interval: float = 0.04,
    desc: Optional[str] = None,
) -> None:
    """Poll `scene/cache_stats` until `paint_count` strictly exceeds
    `baseline` (R883.1). Real paint cycles fire only on winit's
    `RedrawRequested` — asynchronous to RPC dispatch — so the observed
    frame counter, not wall-clock, is the gate for "a frame landed"
    (continuous-paint / immediate-mode demos)."""
    wait_until(
        lambda: int(tf.cache_stats(window=window)["paint_count"]) > baseline,
        timeout=timeout,
        interval=interval,
        desc=desc or f"paint_count advances past {baseline}",
    )


def wait_json_file(
    path: Path | str,
    predicate,
    *,
    timeout: float = 8.0,
    interval: float = 0.04,
    desc: str = "persisted JSON condition",
) -> Any:
    """Poll a JSON file until it is readable AND `predicate(blob)` is
    truthy; returns the matching blob (R883.1).

    The persistence substrate writes via atomic rename (R665), so a
    poll observes whole blobs only — a not-yet-written or garbage file
    simply polls again, while a predicate exception stays loud (a
    malformed-but-valid-JSON blob is a real failure, not a race).
    """
    target = Path(path)

    def poll() -> Any:
        try:
            blob = json.loads(target.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            return None
        return blob if predicate(blob) else None

    return wait_until(poll, timeout=timeout, interval=interval, desc=desc)


def wait_stderr(
    tf: "RpcSubprocess",
    needle: str,
    *,
    n: int = 120,
    timeout: float = 8.0,
    interval: float = 0.04,
    desc: Optional[str] = None,
) -> None:
    """Poll `tf.stderr_tail(n)` until a line contains `needle` (R883).

    The shell logs drained intents to stderr *during* the dispatch, but
    the harness reads stderr through a pump thread — so "the RPC
    response arrived" does not order the log line's arrival in
    `_stderr_lines`. A fixed sleep raced that pump; polling the
    observed tail is the [[zero-flake-policy]] form.
    """
    wait_until(
        lambda: any(needle in ln for ln in tf.stderr_tail(n)),
        timeout=timeout,
        interval=interval,
        desc=desc or f"stderr contains {needle!r}",
    )


def wire_bytes(value: Any) -> int:
    """The compact-JSON size of a wire value, in bytes.

    What a claim about a representation's cost is stated in: `[[0, 9999]]` is
    eleven bytes whatever the model behind it holds.

    R1563 obligation-3b lift: r1561, r1562 and r1563 each carried a
    byte-identical private copy (measured — one md5 across all three), which is
    unsurprising since all three are about the size of a selection's statement.
    No per-demo opinion in it — the separators are what "compact" means — so it
    is shared, and each demo keeps its own budget constant, which IS an opinion.
    """
    return len(json.dumps(value, separators=(",", ":")))


def text_of_tag(tf, tag: str, *, viewport: Optional[tuple] = None) -> str:
    """The first text the painted node at `tag` carries.

    `find_by_tag` + `texts_of(...)[0]`, with the assertion that makes the
    failure legible: a missing node otherwise surfaces as an `IndexError`
    inside a helper rather than as "this tag is not in the paint tree".

    R1563 obligation-3b lift. R1478 declined this one at two copies, and
    recorded why — each demo picks its own "which text counts" rule. Three
    demos now carry the *same* rule byte for byte (r1561, r1562, r1563: the
    first text of a status bar), which is the threshold that rule was waiting
    for. A demo whose rule differs still writes its own; this is the one that
    repeated.
    """
    snap = tf.snapshot(source="paint", viewport=viewport)
    node = find_by_tag(snap, tag)
    assert node is not None, f"{tag!r} must be in the paint tree"
    texts = texts_of(node)
    assert texts, f"{tag!r} is in the paint tree but carries no text"
    return texts[0]


def texts_of(node: Any) -> list[str]:
    """Every `Text.content` string under `node`, depth-first.

    The read half of `find_by_tag`: that answers *which* node, this answers
    *what it says*. Descends `Container.children` and `Scroll.content`, and
    treats a string `content` as the text itself rather than a child — the
    same wire-shape distinction `find_by_tag` makes.

    R1478 obligation-3b lift: r1393, r1469 and r1478 each carried a
    byte-identical private copy (measured — one md5 across all three). Purely
    mechanical tree walking with no per-demo opinion in it, so it is shared;
    the one-line `find_by_tag(...)` + `texts_of(...)[0]` convenience each demo
    wraps it in stays local, being only two copies and each picking its own
    "which text counts" rule.
    """
    out: list[str] = []
    if not isinstance(node, dict):
        return out
    content = node.get("content")
    if isinstance(content, str):
        out.append(content)
    children = node.get("children")
    if isinstance(children, list):
        for child in children:
            out.extend(texts_of(child))
    if isinstance(content, dict):  # Scroll.content is a node, not a string
        out.extend(texts_of(content))
    return out


def walk_nodes(tree: Any, path: str = "/") -> Iterator[tuple[str, dict]]:
    """Every node of a snapshot tree, depth-first, with its positional path.

    The traversal `find_by_tag` and `texts_of` each make for their own
    question, for a demo that needs the nodes themselves: descends
    `Container.children` and `Scroll.content`, and ignores a string `content`
    (which is a `Text`'s own text, not a child) — the wire-shape distinction
    those two already draw.

    The path is `/`-rooted with a child index per level (`/2/0/`), and a
    `Scroll`'s subtree appears under `content/`. It is for failure messages:
    "this node" is unlocatable in a 40-node tree without one.

    R1516 obligation-3b lift: five demos carried a private copy (measured —
    `hello_toggle_style`, `r1467`, `r1468`, `r1514`, and this round's). Purely
    mechanical tree walking with no per-demo opinion, so it is shared, like
    `texts_of` before it (R1478). What each demo does with the nodes stays
    local. The two that walk a `scene/layout` tree rather than a snapshot are
    left alone: that is a different wire shape, and folding them in would put
    one helper in charge of two formats.
    """
    if not isinstance(tree, dict):
        return
    yield path, tree
    children = tree.get("children")
    if isinstance(children, list):
        for i, child in enumerate(children):
            yield from walk_nodes(child, f"{path}{i}/")
    content = tree.get("content")
    if isinstance(content, dict):
        yield from walk_nodes(content, f"{path}content/")


def find_by_tag(snap: Any, tag: str) -> Optional[dict]:
    """Depth-first walk of a snapshot tree for the first node with this `tag`.

    Returns the node dict (with `type`, `rect`, `tag`, children/content
    fields as the wire format defines) or `None` when the tag is absent.

    Descends through `Container.children`, `Scroll.content`, and ignores
    `Text.content` (which is a string, not a child). R51.198 §5.49.
    """
    if not isinstance(snap, dict):
        return None
    if snap.get("tag") == tag:
        return snap
    children = snap.get("children")
    if isinstance(children, list):
        for child in children:
            found = find_by_tag(child, tag)
            if found is not None:
                return found
    content = snap.get("content")
    # `Text.content` is a string, not a child node — only descend when
    # `content` is itself a node dict (carried by `Scroll`).
    if isinstance(content, dict):
        return find_by_tag(content, tag)
    return None


def address_prefix(rows: Any, *, key: str = "key", tag: str = "tag") -> str:
    """The common prefix the addresses in a published roster carry.

    ★★★★★ R2051 — a walk is Python and cannot call the declaration a screen
    composes its painted addresses from, so a screen that publishes them hands
    the walk the answer. Some readings need the whole address and some need only
    the PREFIX — to classify a tag by family, or to build an address for a key
    the roster does not have because the point of the check is that it should.
    This recovers the prefix from a published address by taking that row's own
    key off the end, so the prefix a walk classifies by is the one the paint
    used.

    Lifted here at the third walk family to need it (R2049's role rows, R2050's
    form controls, R2051's rail seats) and after thirteen walks in one family
    alone had been spelling it — well past the rule of three this workspace
    lifts at.

    Raises rather than guessing when the roster is empty or its first row does
    not end with its own key: both mean the roster is not the shape this is
    reading, and a prefix worked out from one that is not would be a plausible
    string that addresses nothing.
    """
    rows = list(rows)
    assert rows, "an empty roster publishes no address to recover a prefix from"
    first = rows[0]
    address, own = first[tag], first[key]
    assert address.endswith(own), (
        f"{address!r} does not end with its own key {own!r}, so this roster's "
        "addresses are not its prefix followed by its key"
    )
    return address[: len(address) - len(own)]


def access_node_by_tag(result: Any, tag: str) -> Optional[dict]:
    """The `scene/access` node carrying this `tag`, or `None` when absent.

    The a11y peer of `find_by_tag`: an access tree is a FLAT `nodes` list (the
    parent/child relation is by tag reference, not nesting), so it is a scan
    rather than a descent — a distinction worth keeping visible, which is why
    this is its own name instead of an argument to `find_by_tag`.

    R1517 obligation-3b lift: five demos carried a byte-identical private copy
    (measured — r979, r980, r981, r982, r983). Mechanical, no per-demo opinion,
    so it is shared, following `texts_of` (R1478) and `walk_nodes` (R1516).
    """
    if not isinstance(result, dict):
        return None
    for node in result.get("nodes") or ():
        if isinstance(node, dict) and node.get("tag") == tag:
            return node
    return None


def voice_rows(result: Any) -> dict[str, dict]:
    """The `scene/voice` census keyed by tag (R1691).

    The census answers `{total, counts, nodes}` and almost every question a
    caller has is about one region, so the flat list is turned into a lookup
    once here rather than scanned per assertion. The counts stay on the
    envelope: they are DERIVED on every call, and a caller that re-derived them
    from these rows would be checking the arithmetic rather than the screen.
    """
    if not isinstance(result, dict):
        return {}
    return {
        row["tag"]: row
        for row in result.get("nodes") or ()
        if isinstance(row, dict) and isinstance(row.get("tag"), str)
    }


def voice_partition_sum(result: Any) -> int:
    """The `scene/voice` counts summed over every **painted** arm (R1692).

    Derived from the keys the running binary publishes rather than from a list
    written here, which is the difference between checking the partition and
    checking a copy of it: R1692 added two arms, and a demo carrying its own
    four-name list would have gone on summing four and calling the result total.

    `ghost` is the one arm excluded, and by a rule of the surface rather than a
    convenience: a ghost is announced for a tag nothing paints, so it is not part
    of the painted population `total` counts.
    """
    counts = result.get("counts", {}) if isinstance(result, dict) else {}
    return sum(n for arm, n in counts.items() if arm != "ghost")


def voice_defects(result: Any) -> list[dict]:
    """Every census row on a defect arm, in census order (R1692).

    Derived from the rows rather than from a list of arm names, so an arm added
    to the enum is one this finds without being told: a row is a defect exactly
    when it is not `announced` and not `silent`, and those two are the surface's
    documented correct outcomes.
    """
    return [
        row
        for row in voice_rows(result).values()
        if row.get("voice") not in ("announced", "silent")
    ]


def access_focus_flags(result: Any) -> set[str]:
    """Every `scene/access` tag whose `state.focused` is `true`.

    The wire form omits default-valued fields, so an unfocused node carries no
    `focused` key at all — "the set of tags claiming focus" is the honest read,
    and it is a SET so a caller can assert what is *not* in it. The AT tree's
    focus flag is single-sourced from the shell's focused tag (R1517), so this
    set is expected to hold at most the one tag `focus/get` reports.
    """
    if not isinstance(result, dict):
        return set()
    return {
        node["tag"]
        for node in result.get("nodes") or ()
        if isinstance(node, dict)
        and isinstance(node.get("tag"), str)
        and (node.get("state") or {}).get("focused") is True
    }


def count_indexed_tags(snap: Any, prefix: str, suffix: str = "") -> int:
    """Count consecutive `{prefix}{k}{suffix}` tags (k = 0, 1, 2, …) present in
    `snap`, stopping at the first gap. The mechanical counter the chart demos share
    for `pinion-chart`'s indexed nodes — x-tick labels (`{tag}.label.x.`) and legend
    rows (`{tag}.legend.` / `.label`). Lifted (R1410 rule-of-three) from the
    byte-identical `x_label_count` / `legend_label_count` the r1396 / r1409 / r1410
    chart demos each defined."""
    k = 0
    while find_by_tag(snap, f"{prefix}{k}{suffix}") is not None:
        k += 1
    return k


def chord_click(tf, tag: str, *, shift: bool = False, ctrl: bool = False) -> None:
    """A `scene/click` on `tag` with held modifiers, released after — the R763
    `scene/modifiers` press/release pair the shell reads at the activate edge.

    R1562 obligation-3b lift: three demos carried a byte-identical body,
    differing only in how they built the tag (measured — r781 `click_row`, r782
    `click_cell`, and this round's band press would have been the third).
    Mechanical, no per-demo opinion, so it is shared and each caller keeps its
    own tag construction — following `indexed_tags` (R1523) and
    `access_node_by_tag` (R1517).

    The press/release pair is what makes it worth sharing: a demo that forgets
    the release leaves the chord held into the NEXT click, which reads as a
    passing test for the wrong reason.
    """
    if shift or ctrl:
        tf.modifiers(shift=shift, ctrl=ctrl)
    tf.click(path=tag)
    if shift or ctrl:
        tf.modifiers()  # release


def indexed_tags(rects: dict, prefix: str) -> list[int]:
    """Sorted indices `k` of the `{prefix}{k}` tags present in a `abs_rects_of`
    mapping.

    The **windowed-axis** reader: which members of an indexed family actually
    reached the paint tree, when the answer is a *window* rather than a prefix
    starting at 0. Contrast `count_indexed_tags`, which counts consecutively from
    0 and stops at the first gap — right for a chart's legend rows, wrong for a
    virtualized grid, whose rendered rows (and, since R1523, columns) start
    wherever the scroll offset put them.

    Takes the rects mapping rather than the snapshot, unlike the rest of this
    family: a windowed-axis assertion usually already holds the rects (it is
    checking geometry too), and several call sites only ever have the mapping —
    `wait_until` predicates return one, not the snapshot it came from.

    R1523 obligation-3b lift: five demos carried a byte-identical private copy
    (measured — r777, r778, r782, r998, r1004) and this round's column-window
    assertions would have been the sixth and seventh. Mechanical, no per-demo
    opinion, so it is shared — following `access_node_by_tag` (R1517) and
    `walk_nodes` (R1516).
    """
    out: list[int] = []
    for tag in rects:
        if tag.startswith(prefix):
            suffix = tag[len(prefix):]
            if suffix.isdigit():
                out.append(int(suffix))
    return sorted(out)


def selection_rows(answer: Any) -> list[int]:
    """The rows an R1561 `"selection"` answer names, ascending.

    The slot carries the selection as the **runs** it is made of —
    `[[first, last], …]`, inclusive — because "rows 0 through 999 999" is one
    fact and spelling it as a million integers made `query("selection")` cost
    the model (measured on `hello-multi-select`: 58 890 bytes and 10.9 ms for a
    select-all, against 11 bytes and 0.3 ms after). This decodes runs back to
    rows for the assertions that are about *which* rows.

    It lives here, once, for the reason the Rust `read_selection` /
    `selection_to_value` pair lives in one module: seven demos read this slot,
    and seven private decodes would be seven chances to disagree with the
    encoder about what the wire says. An assertion about the *representation*
    (that a span is one run, that a hole makes two) reads the raw answer
    instead — that is the property, not an encoding detail.

    Raises on anything that is not a list of pairs, rather than skipping the
    parts it does not understand: a decoder that silently drops what it cannot
    read turns a wire-shape regression into a quieter, wrong assertion.
    """
    if not isinstance(answer, list):
        raise AssertionError(f"selection answer is not a list of runs: {answer!r}")
    rows: list[int] = []
    for run in answer:
        if not (isinstance(run, list) and len(run) == 2):
            raise AssertionError(f"selection run is not a [first, last] pair: {run!r}")
        first, last = run
        rows.extend(range(first, last + 1))
    return rows


def runs_of(rows: Iterable[int]) -> list[list[int]]:
    """The canonical R1561 run form of `rows` — the inverse of
    `selection_rows`, for a demo that knows the rows it expects and wants to
    assert the wire answer *exactly* (`wait_query` compares whole values).

    Written here beside the decoder so the two cannot drift, and canonicalising
    (sorted, deduplicated, abutting rows merged) so an expectation is a function
    of the rows rather than of the order they were listed in — the same property
    the Rust `IndexRuns` holds.
    """
    out: list[list[int]] = []
    for row in sorted(set(rows)):
        if out and out[-1][1] + 1 == row:
            out[-1][1] = row
        else:
            out.append([row, row])
    return out


def cursor_to_source(tf, group_tag: str, source: int) -> None:
    """Move a `GroupOrderExternal`'s roving visual-row cursor onto the data row
    with stable `source` index (R873).

    Walks the group proxy's flatten (`visible_len` + `source_at.<pos>`) for the
    visual position whose data-row source matches, then sets the cursor there
    via `intervene cursor`. The grouped-collection peer of a direct selection
    set — `source_at`/`visible_len`/`cursor` are the generic `GroupOrderExternal`
    wire, so this works for any grouped binding (property-grid, grouped-list,
    grouped-grid, …). Raises if the source is filtered/collapsed out of the
    flatten.
    """
    visible = tf.query(f"/{group_tag}/external/visible_len")
    for pos in range(visible):
        if tf.query(f"/{group_tag}/external/source_at.{pos}") == source:
            tf.intervene(f"/{group_tag}/external/cursor", pos)
            return
    raise AssertionError(f"source {source} not visible in {group_tag}'s flatten")


def rect_of(node: dict) -> dict:
    """Return the geometry rect of a snapshot node.

    `Scroll` reports its geometry under `viewport` (the clip window);
    every other primitive uses `rect`. R51.198 §5.49 / R51.199 §5.49.
    Raises `AssertionError` when the node carries neither field.
    """
    if node.get("type") == "Scroll":
        rect = node.get("viewport")
    else:
        rect = node.get("rect")
    if not isinstance(rect, dict):
        raise AssertionError(f"node has no geometry rect: {node!r}")
    return rect


def node_center(node: dict) -> tuple[float, float]:
    """Return the centre `(x, y)` of a node's geometry rect.

    Uses `rect_of` so the helper works uniformly for leaf primitives
    (`Box` / `Text` / `Path` / `Image`), `Container`, `External`, and
    `Scroll` (whose geometry lives under `viewport`). Raises
    `AssertionError` when the node has no rect (`Effect` / `Unknown`
    markers, future variants). R51.198 §5.49.
    """
    rect = rect_of(node)
    cx = float(rect["x"]) + float(rect["w"]) / 2.0
    cy = float(rect["y"]) + float(rect["h"]) / 2.0
    return (cx, cy)


# R705.1 §5.39 — the injected focus-ring overlay box tag (mirrors
# `pinion_overlay::focus_ring::FOCUS_RING_TAG`).
FOCUS_RING_TAG = "ai-overlay/focus-ring"


def frame_diff(a: Any, b: Any) -> "tuple[int, int]":
    """`(bytes that disagree, the largest disagreement)` for two RGBA8 captures.

    The second number is the one that matters. A rasteriser rounding sub-pixel
    coverage differently moves a channel by **1**; a glyph painted somewhere
    else swaps ink for background, which moves it by tens or hundreds. So the
    magnitude separates "the same picture drawn twice" from "a different
    picture" in a way a count never can — measured R1664, and see
    [`assert_same_picture`].
    """
    assert (a.width, a.height) == (b.width, b.height), (
        f"captures are different surfaces: {a.width}x{a.height} vs {b.width}x{b.height}"
    )
    count = 0
    worst = 0
    for x, y in zip(a.pixels, b.pixels):
        if x != y:
            count += 1
            delta = x - y if x > y else y - x
            if delta > worst:
                worst = delta
    return count, worst


def assert_same_picture(
    control: "Sequence[Any]",
    under_test: "tuple[Any, Any]",
    what: str,
) -> int:
    """★★★★ R1664 — assert two frames are the same PICTURE, against a noise
    floor this run measured rather than one anybody assumed. Returns the
    measured floor.

    # Why byte-identity was the wrong claim

    Three demos asserted `second.pixels == first.pixels` to prove a cache
    replayed a frame instead of rebuilding it. That is a claim about the paint
    pipeline, and it was being made through the **rasteriser**, which is not
    part of the claim and is not byte-deterministic everywhere.

    Measured (R1664): capturing one unchanged screen four times in a row, with
    no input between the captures —

    * on this host's GPU, every pair is byte-identical;
    * under lavapipe, the software Vulkan the CI sweep runs on, consecutive
      captures of *the same state* differ in **4 to 7 bytes of 768,000** —
      sub-pixel coverage on a handful of glyph edges.

    So all three demos went red on CI for something no cache did, and they had
    been red there for many runs while passing locally, which is exactly the
    green-local / red-CI shape [[zero-flake-policy]] forbids.

    # The predicate is the MAGNITUDE, not the count

    Characterised rather than guessed. Ten consecutive captures of one unchanged
    screen under lavapipe disagreed on 2 to 10 bytes, scattered over two vertical
    edges — **and every single differing byte differed by exactly 1**. That is
    one least-significant bit of coverage, which is what a tile-parallel software
    rasteriser rounds differently between runs.

    A count therefore cannot be the test: it drifts run to run (a floor sampled
    at 4 rejected a real replay that noised at 6), so it would either flake or
    have to be padded until it stopped meaning anything. The magnitude does not
    drift, and it is what actually separates the two cases — any *positional*
    error swaps ink for background at a glyph edge, which moves a channel by tens
    or hundreds, never by one.

    # Why this does not weaken anything

    `control` is two or more captures of ONE state with nothing in between,
    taken in the same process on the same rasteriser. On a deterministic one its
    tolerance is **0**, and this assertion is then exactly the byte-identity it
    replaces — which is what it reduces to on this project's GPU hosts.

    # ★★ R1676 — a measured 0 is not evidence of determinism

    The two directions are not symmetric, and reading them as if they were is
    what put `r1527` red on CI while it passed here. **Seeing a difference
    proves the rasteriser is non-deterministic. Seeing agreement proves
    nothing** — it is one sample of a stochastic process, and this one agrees
    by luck about 7% of the time (measured: 3 of 45 pairs under the software
    Vulkan the sweep ran on). A control pair that agrees reports a floor of 0,
    and the tested pair then fails for the noise the floor was supposed to
    absorb. Widening the sample only lowers that rate, and a lowered flake rate
    is not what [[zero-flake-policy]] asks for.

    So the CAUSE is removed instead: `RpcSubprocess` pins the software
    rasteriser to one tile thread (see `_enter_inner`), where all 45 pairs are
    byte-identical. This helper stays, because a host it does not control can
    still be non-deterministic and the demo should say so rather than flake —
    but on the hosts this project runs, the tolerance it measures is now 0
    because the rasteriser is deterministic, not because the sample was lucky.
    """
    assert len(control) >= 2, (
        f"{what}: the tolerance is measured, so it needs at least two captures "
        f"of the unchanged state (got {len(control)})"
    )
    tolerance = max(
        frame_diff(control[i], control[j])[1]
        for i in range(len(control))
        for j in range(i + 1, len(control))
    )
    count, worst = frame_diff(*under_test)
    total = len(control[0].pixels)
    assert worst <= tolerance, (
        f"{what}: the two frames disagree by up to {worst} on a channel "
        f"({count} of {total} bytes differ), and this rasteriser's own "
        f"disagreement with itself — measured in this run by capturing one "
        f"unchanged screen {len(control)} times — never exceeds {tolerance}. A "
        f"channel moving by more than that is ink where background was: a run "
        f"dropped, a position transcribed run-relative where layout-absolute was "
        f"meant, or a fragment replayed after it went stale."
    )
    return tolerance


def assert_router_press_moves(
    tf: "RpcSubprocess",
    tag: str,
    read: "Callable[[], Any]",
    what: str,
    *,
    viewport: tuple[int, int] = (1440, 900),
) -> Any:
    """★★★★★ R1664 — press the centre of `tag` the way a mouse does, and assert
    the app moved. Returns the value `read` gives afterwards.

    The rect is resolved **at press time**, from the paint scene, in
    window-absolute coordinates. Both halves of that are load-bearing and both
    were got wrong while this helper was being written:

    * *At press time*, because a screen re-lays-out under the presses a sequence
      of these makes. Taking the geometry once and pressing five times aimed the
      fifth press at where the fourth press had moved the target, and the
      failure looked exactly like the routing defect this helper exists to catch.
    * *Window-absolute*, because a node inside a `Scroll` carries a scroll-LOCAL
      rect — which is what `find_by_tag(...)["rect"]` hands back, and what put
      every press on two analyzer screens at coordinates nothing is painted at
      once R1662 made their panes scroll.

    # The bypass this exists to end

    A screen can pass every test in this tree and be completely dead to a person,
    because every wire verb a demo normally reaches for addresses a widget **by
    name**:

    * `scene/invoke {path}` / `scene/query {path}` call the widget's own
      introspection handler;
    * `scene/click {path}` resolves the path's rect and presses its centre, but
      it is the *path* that found the widget;
    * an example's own `Hit::at`, which its Rust sweep calls, is the app's
      private hit function and not the router at all.

    A real mouse has no name. It has a point, and the §5.35 router has to turn
    that point into a widget: it resolves the deepest tagged node under the
    cursor, splits the composite half off, and looks the result up as an
    `External` in the state scene. Two joins, both of them plain strings, and a
    failure of either returns without a word — `dispatch_send` discards the
    widget's answer (`let _ = intro.invoke("send", …)`), so a widget that does
    not know the verb is as silent as one that was never found.

    Measured three times: R1497 (a header cell's own label swallowed the press),
    R1649.1 (an entire shell, with a 118-assertion demo passing), and R1663
    (`hello-packet-view`: 11 integration tests, a 160-assertion demo and a boot
    gate all green, and a person opening the window found that pressing anything
    did nothing at all — BOTH joins were broken, and repairing only the first one
    still produced a screen where every press was refused and dropped).

    `scene/click {at: {x, y}}` is the one wire entry point that goes through the
    router, so it is the one a screen's own coverage has to include. The
    assertion is that the app's state MOVED — a press that resolves and is
    refused changes nothing, which is indistinguishable from no press at all
    unless something reads the state on both sides of it.

    """
    rects = abs_rects_of(tf.snapshot(source="paint", viewport=viewport))
    assert tag in rects, (
        f"{what}: nothing painted carries `{tag}`, so there is no point to press "
        f"— the screen paints {len(rects)} tagged node(s)"
    )
    x, y, w, h = rects[tag]
    at = (x + w // 2, y + h // 2)
    before = read()
    tf.request("scene/click", {"button": "left", "at": {"x": at[0], "y": at[1]}})
    tf.tick_ms(16)
    after = read()
    assert after != before, (
        f"{what}: a press at {at}, the centre of `{tag}` — driven through the "
        f"§5.35 router, the way a "
        f"mouse arrives — left the screen exactly as it was ({before!r}). Either "
        f"no painted tag at that point resolves to a registered `External` (ask "
        f"`scene/pointer_reach`.externals, which names both sides of that join), "
        f"or one does and the widget does not answer `send`, which the router "
        f"dispatches a press as and whose rejection it discards. Driving the same "
        f"target with `scene/invoke` proves neither: that path never asks the "
        f"router."
    )
    return after


def assert_every_destination_arrives(
    tf: "RpcSubprocess",
    *,
    roster_path: str,
    seat: "Callable[[str], str]",
    region: str,
    viewport: tuple[int, int] = (1440, 900),
) -> dict:
    """★★★★★ R1695 — pressing a destination **takes you there**, and a
    destination that refuses says why.

    The law of a navigation rail, written once. Every screen that grew one wrote
    its own half of it and both analyzer screens got a different half wrong:

    * One highlighted the seat and painted the same page. Measured through the
      router before the repair: pressing four of its seven seats moved the
      string the rail highlights itself from and left the window at **193
      tagged regions before and 193 after**.
    * The other answered every seat — including the one it already was, and
      including two seats declared unavailable with a stated reason — with a
      message saying the destination is not this screen.

    `assert_router_press_moves` could not catch either, and the reason is worth
    naming: it asserts that a press **moved the state**, which is exactly what a
    screen that highlights a seat and shows nothing new does. *Moved* is not
    *arrived*, and the difference is a rectangle.

    So this reads the roster the screen publishes (`roster_path`, the shape
    `pinion_core::widgets::destination::Destinations::wire` produces), presses
    each seat **through the §5.35 router**, and asserts against the painted
    scene inside `region`:

    * an **open** destination arrives — `at` becomes its key, and the tags
      inside the region are not the tags that were there before;
    * the open destinations' pages are **pairwise distinct**, without which
      "arriving" is satisfied by a region that paints one page whatever the
      journey says;
    * a **closed** destination does not arrive, and the wire channel refuses it
      naming the reason the seat is painted with — one reason, two channels.

    Returns the roster, so a caller can go on asserting about it.

    ## The floor

    Measured by building a probe against the reference toolkit at 6.11.1 and
    running it: its paged container is addressed by ordinal,
    `setCurrentIndex` returns `void`, an out-of-range ordinal is a silent
    no-op, and a **disabled page is arrived at anyway** — so there is no
    refusal there for a test to assert on, and the container's own accessible
    value is empty, so no client can even ask which page is showing.
    """

    def read_at() -> str:
        answer = tf.query(f"{roster_path}")
        return answer["at"] if isinstance(answer, dict) else json.loads(answer)["at"]

    def roster() -> dict:
        answer = tf.query(roster_path)
        return answer if isinstance(answer, dict) else json.loads(answer)

    def page_tags() -> frozenset:
        """Every tag in the region's SUBTREE.

        The subtree and not the rectangle. Chrome legitimately overlaps a page
        region — the toast on this screen floats over it at every destination —
        so a geometric test would report the same tag as belonging to both
        pages and the disjointness assertion below would be about the toast
        rather than about the pages. What makes a node part of a page is that
        the page built it, which is the tree relation.
        """
        snap = tf.snapshot(source="paint", viewport=viewport)
        node = find_by_tag(snap, region)
        assert node is not None, (
            f"the screen paints no {region!r}, so there is no page region to "
            f"compare across destinations"
        )
        return frozenset(
            found.get("tag")
            for _, found in walk_nodes(node)
            if found.get("tag") and found.get("tag") != region
        )

    start = roster()
    rows = start["destinations"]
    current = [row["key"] for row in rows if row["current"]]
    assert current == [start["at"]], (
        f"the roster marks {current} current and says it is at "
        f"{start['at']!r} — one journey, two answers"
    )

    pages: dict[str, frozenset] = {}
    for row in rows:
        key, tag = row["key"], seat(row["key"])
        before_at, before_page = read_at(), page_tags()
        x, y, w, h = abs_rects_of(tf.snapshot(source="paint", viewport=viewport))[tag]
        tf.request("scene/click", {"button": "left", "at": {"x": x + w // 2, "y": y + h // 2}})
        tf.tick_ms(16)
        if row["open"]:
            assert read_at() == key, (
                f"a press at the centre of {tag!r}, driven through the §5.35 "
                f"router the way a mouse arrives, left the journey at "
                f"{read_at()!r} rather than taking it to {key!r}"
            )
            after = page_tags()
            if before_at != key:
                assert after != before_page, (
                    f"arriving at {key!r} changed the journey and painted the "
                    f"same {len(after)} region(s) inside {region!r}. That is "
                    f"the shape this helper exists for: the rail highlighted a "
                    f"seat and the window did not move"
                )
            pages[key] = after
            continue
        # A closed seat is painted inert, so the pointer never reaches it.
        assert read_at() == before_at, (
            f"{key!r} is closed ({row['sentence']}) and a press navigated there"
        )
        assert page_tags() == before_page, f"{key!r} is closed and the page changed"
        # The wire says the same thing, in words, which is the channel a person
        # gets from the seat's own declaration.
        refusal = ""
        try:
            tf.intervene(f"{roster_path.rsplit('/', 1)[0]}/nav", key)
        except Exception as why:  # noqa: BLE001 - any refusal shape is fine
            refusal = str(why)
        assert row["detail"] and row["detail"] in refusal, (
            f"the wire refused {key!r} with {refusal!r}, which does not name "
            f"the reason {row['detail']!r} its seat is painted with"
        )

    assert len(pages) >= 2, (
        f"{len(pages)} open destination(s): a paged region with one page proves "
        f"nothing about paging"
    )
    keys = sorted(pages)
    for i, a in enumerate(keys):
        for b in keys[i + 1 :]:
            shared = pages[a] & pages[b]
            assert not shared, (
                f"{a!r} and {b!r} paint the same regions inside {region!r} "
                f"({sorted(shared)[:6]}), so the rail chooses between two names "
                f"for one page"
            )
    return start


def _clipped_into(
    x: int, y: int, w: int, h: int, clip: Optional[tuple[int, int, int, int]]
) -> Optional[tuple[int, int, int, int]]:
    """Mirror of `pinion_core::scene::translate_rect_into_clip`, arm for arm.

    Kept a separate function for the reason the Rust one is: the fold is the
    part that is easy to write *almost* right, and a copy of it inlined at each
    call site is a copy free to drift from the others.
    """
    left, top = x, y
    right, bottom = left + w, top + h
    if clip is not None:
        cx, cy, cw, ch = clip
        left, top = max(left, cx), max(top, cy)
        right, bottom = min(right, cx + cw), min(bottom, cy + ch)
    if right <= left or bottom <= top:
        return None
    out_x, out_y = max(left, 0), max(top, 0)
    return (out_x, out_y, right - out_x, bottom - out_y)


def unclipped_rects_of(snap: Any) -> dict[str, tuple[int, int, int, int]]:
    """Map every tagged node to **where it would be if nothing clipped it**.

    ★★ R1676 — the OTHER question, and it needed its own name because for a
    long time it did not have one.

    `abs_rects_of` answers "where can a pointer reach this", and a mark a
    viewport cuts away is absent from it. That is the right answer for the
    caller aiming a press, and the WRONG one for a caller asking what the view
    emitted: a virtualized grid's claim is that it builds exactly the cells it
    paints, and a cell built and then clipped is still a cell it built. Asking
    the visible map that question reports the virtualization as tighter than it
    is — measured, four demos changed their answer when the clip was folded in.

    The framework names both halves and always has:
    [`NodeVisit::offset`] is documented as "where it would be … even when the
    leaf is scrolled out of view", and `absolute_rect()` as "where it can be
    seen". This is the first, `abs_rects_of` is the second, and both come out of
    one walk so they cannot disagree about the offsets they share.

    Reach for this one when the question is about what the VIEW did — which
    rows it built, which columns it asked the model for, how far a pane's
    content extends past its own edge. Reach for `abs_rects_of` when the
    question is about what a PERSON can do.
    """
    return _walk_tag_rects(snap, clipped=False)


def abs_rects_of(snap: Any) -> dict[str, tuple[int, int, int, int]]:
    """Map every tagged node to the **part of it a pointer can reach**, `(x, y, w, h)`.

    Independently re-implements what `pinion_core::scene::Scene::absolute_rects_by_tag`
    does in Rust: a node inside a `Scroll` carries a scroll-LOCAL rect, the
    renderer paints it at `viewport_pos + (local - scroll_offset)`, and every
    enclosing viewport then CUTS it. Accumulating `(viewport.x - offset_x,
    viewport.y - offset_y)` per Scroll boundary yields the on-screen position;
    intersecting with the accumulated viewport stack yields the part that is on
    screen. This is the GROUNDING that makes a focus-ring assertion
    non-tautological: the ring rect (a top-level overlay, already
    window-absolute) is checked against a *separately computed* absolute
    position, so a ring drawn at a scroll-local rect is caught (R705.1,
    [[introspection-from-paint-not-screen]]).

    ★★ R1676 — THE CLIP IS HALF THE ANSWER, and this mirror used to fold only
    the offset. `NodeVisit::absolute_rect`'s doc argues at length that reported
    and visible have to be ONE fact, because a caller that forgets the second
    call "asserts against a rectangle nothing was drawn in" — and this was that
    caller. Every demo picks press points from this map, so the map handed out
    coordinates outside the viewport and the presses went nowhere. Measured on
    `hello-data-grid`: a cell reported at `x=-31 w=100` inside a viewport
    starting at `x=21`, its centre two pixels to the LEFT of anything, the press
    silently dropped, and the release landing on a DIFFERENT cell. Three demos
    were red for it.

    The floor makes the same split — measured, offscreen, on the mature
    retained-mode toolkit at 6.11: its per-item rect for a horizontally scrolled
    view answers `x=-232 w=99` against a `0..368` viewport, and its own
    point→item lookup then names that cell for a point no pointer can occupy.
    Its honest visible-region call is a *widget* member, and an item-view cell
    is not a widget. Here there is one fact and it is this one.

    A tag whose node is painted but wholly clipped away is ABSENT from the map,
    which is `absolute_rects_by_tag`'s rule and is why `in rects` is the
    question "can this be reached" rather than "does this exist". Ask
    `scene/tag_rects` when the difference matters: it carries those tags with a
    null window.

    First tag wins, pre-order — the rule the whole tag surface shares. Writing
    it as a plain assignment made this mirror answer a *later* duplicate while
    the Rust answered the first, which is the second way the two had come apart.
    """
    return _walk_tag_rects(snap, clipped=True)


def screen_spec(app: "RpcSubprocess", external: str = "/external") -> Any:
    """The specification a screen publishes about itself, as data.

    ★ Two spellings, and both are read: one screen publishes `spec` as JSON and
    another as a string holding JSON. Which a screen chose is not what any
    caller of this is about, and a demo that knows is a demo that breaks when
    the other screen is added.

    R1709 lifted this out of `r1708_a_drag_is_answered_once.py` on its second
    consumer, per the standing rule that what a demo re-derives belongs in the
    harness before a third one derives it differently.
    """
    spec = app.query(f"{external}/spec")
    if isinstance(spec, str):
        spec = json.loads(spec)
    return spec


def declared_panes(app: "RpcSubprocess", external: str = "/external") -> list[dict]:
    """The panes this screen DECLARES, or `[]` if it is not organised in panes."""
    spec = screen_spec(app, external)
    panes = spec.get("panes") if isinstance(spec, dict) else None
    return panes or []


def design_size(app: "RpcSubprocess") -> tuple[int, int]:
    """This screen's own opening size, ASKED FOR rather than written down."""
    rect = app.snapshot(source="paint")["rect"]
    return (rect["w"], rect["h"])


def names_in_spec(spec: Any) -> set[str]:
    """Every string anywhere in a published specification.

    Screens organise their specifications differently — panes and columns here,
    a rail roster and a catalogue there — so the general form reads all of it
    rather than knowing any of it. It is what lets a screen whose specification
    is not a list of panes be checked at all.
    """
    if isinstance(spec, str):
        return {spec}
    if isinstance(spec, dict):
        return set().union(*(names_in_spec(v) for v in spec.values())) if spec else set()
    if isinstance(spec, list):
        return set().union(*(names_in_spec(v) for v in spec)) if spec else set()
    return set()


def declared_and_painted(
    app: "RpcSubprocess", size: tuple[int, int], external: str = "/external"
) -> set[str]:
    """What this screen's specification NAMES and its paint actually draws."""
    return names_in_spec(screen_spec(app, external)) & set(
        abs_rects_of(app.snapshot(source="paint", viewport=size))
    )


def settle_saying(app: "RpcSubprocess", external: str = "/external") -> str:
    """★★★★★ R1867 — advance until the screen's transient sentence has EXPIRED,
    and answer what it had been saying.

    ## Why a helper and not a `tick`

    A screen that says something on arrival has two resting states, not one: the
    band holds a toast for as long as it lives and the permanent sentence the
    rest of the time. A census taken in one of them reports the other's region
    as missing — measured: `r1694` demanded `shell.status.gesture` while the
    boot toast was up and failed. Which is why the caller UNIONS the two
    readings rather than moving to the later one: `shell.toast` is painted only
    while a toast shows, so a census taken solely after the settle would lose
    that region for the same reason, the other way round.

    The duration is **asked for, never written down**. `Saying::to_wire` (R1790)
    publishes `left` for this reason in as many words — *"a test that guesses
    the duration is a test that pins a number this type owns"* — and R1787's CI
    red was a demo doing the guessing. ⚠ `left` is SECONDS and so is `tick`
    (R1783); the epsilon is one frame's worth, because the tick that reaches
    exactly zero is the one that clears it.

    Returns the sentence that was cleared, or `""` when nothing was being said,
    so a caller can assert it settled something rather than assume it did.
    """
    said = app.query(f"{external}/saying")
    if not isinstance(said, dict) or said.get("said") is None:
        return ""
    sentence = said["said"].get("sentence", "") if isinstance(said["said"], dict) else ""
    app.tick(float(said["left"]) + 0.05)
    after = app.query(f"{external}/saying")
    assert isinstance(after, dict) and after.get("said") is None, (
        f"the sentence outlived the {said['left']}s the screen said it had "
        f"left: {after!r}"
    )
    return sentence


def bring_into_view(
    app: "RpcSubprocess", tag: str, *, source: str = "paint"
) -> tuple[int, int, int, int]:
    """★★★★★ R1867 — scroll whatever holds `tag` until a reader can see it, and
    answer its rectangle in WINDOW space.

    ## What this is the repair of, measured

    `r1695_the_rail_takes_you_there` pressed the settings page's theme segments
    by reading their rectangle straight out of the paint snapshot. That worked
    while the page's last group happened to be above the fold, and stopped the
    moment R1864 gave the window a status band: the segment moved below the
    viewport, `abs_rects_of` had no key for it, and the demo died
    `KeyError('shell.settings.theme.0')` — a red that was **not** a defect in
    the screen. Measured here at R1867: `scene/scroll_reach` reports the theme
    segments `scrollable`, `lost: 0`, with the offset that shows them.

    So the demo was asserting a **fold position** while claiming to assert a
    **press**. A reader who cannot see a control scrolls to it; this is the
    harness doing what the reader does, and it takes the offset from the
    framework's own answer rather than from a number somebody picked — which is
    the property that stops the next round moving the fold from breaking it
    again.

    ## Why it is here rather than in one demo

    Every screen this project builds now has panes whose content outruns them,
    so "press a control that may be below the fold" is a harness capability, not
    one demo's paragraph. The floor's own test harness offers a scroll-to-item
    only for its model-driven item views; anything hand-painted is the author's
    problem there.

    Raises `AssertionError` when the tag is neither painted nor reachable, and
    says which of the two — a control nothing brings into view is a real defect
    and this must not paper over it.
    """
    rects = abs_rects_of(app.snapshot(source=source))
    if tag in rects:
        return rects[tag]
    resp = app.request("scene/scroll_reach")
    assert resp is not None and isinstance(resp.result, dict)
    rows = [row for row in resp.result.get("out_of_sight", []) if row.get("tag") == tag]
    if not rows:
        raise AssertionError(
            f"{tag} is not painted and `scene/scroll_reach` does not know it "
            f"either, so nothing on this screen can bring it into view"
        )
    row = rows[0]
    if row["reach"] != "scrollable":
        raise AssertionError(
            f"{tag} is {row['reach']} in viewport {row['viewport']['name']!r} "
            f"(short by {row.get('short_by')}), so no offset shows it whole"
        )
    # Outermost first, which is the order the report publishes and the order a
    # chain has to move in: an inner viewport's offset is meaningless until the
    # one above it has put the inner viewport on screen.
    for move in row["moves"]:
        app.scroll(move["viewport"], to=(move["to_x"], move["to_y"]))
        app.tick_ms(16)
    rects = abs_rects_of(app.snapshot(source=source))
    if tag not in rects:
        raise AssertionError(
            f"{tag} was reported reachable by {row['moves']} and is still not "
            f"painted after performing exactly that — the report and the scroll "
            f"disagree, which is a defect in one of them"
        )
    return rects[tag]


def declared_but_unreachable(
    app: "RpcSubprocess", declared: "Iterable[str]", size: tuple[int, int]
) -> "list[str]":
    """★★★★★ R1733 — of the names a screen's specification declares, the ones
    that are neither PAINTED at `size` nor **one gesture away**.

    ## What this is the repair of

    Two demos asserted that nothing the specification names stops being painted
    when a window is resized to its floor, and computed that as
    `declared - declared_and_painted(...)`. That rule was written when a window
    could not pan and its panes could not scroll. Both changed: R1714 gave
    `assert_declared_panes_on_screen` exactly this clause — *painted, or one
    gesture away, and the wire says which* — and left the sibling rule beside it
    on the old footing.

    Measured on the capture viewer at its own declared floor, ten rounds out of
    ten: **290** declared regions are painted at the design size and **127** at
    the floor, so **163** are out of sight — and `scene/scroll_reach` reports
    `lost: 0` and names every one of them `scrollable`, with the scroll that
    brings it back. Nothing is missing; the rule had no way to say so. It passed
    whenever the state it ran in happened to leave nothing out of sight, which
    is what made it a flake rather than a red: it went red once in a 29-demo
    sweep and passed ten times standing alone.

    ## It keeps its teeth

    A region that is neither drawn nor reachable still fails, and so does one
    the report cannot name — the two directions R1714 kept for panes. The
    caller should ALSO assert `scene/scroll_reach`'s `lost` is zero; this
    answers the per-name question, not the screen-wide one.
    """
    painted = set(abs_rects_of(app.snapshot(source="paint", viewport=size)))
    resp = app.request("scene/scroll_reach")
    assert resp is not None and isinstance(resp.result, dict), (
        "scene/scroll_reach answers, or a region out of sight cannot be told "
        "from a region that is gone"
    )
    reachable = {
        row["tag"]
        for row in resp.result.get("out_of_sight", [])
        if isinstance(row, dict) and row.get("tag") and row.get("reach") != "lost"
    }
    return sorted(set(declared) - painted - reachable)


def ink_in_boxes(
    app: "RpcSubprocess",
    *,
    scale: int = 4,
    slack: int = 3,
) -> "list[dict]":
    """★★★★★ R1794 — **where the GLYPHS sit inside the box a reader sees**, asked
    of the wire.

    ## Why this exists

    A reader opened the assembled tool and reported that five chips, three row
    seats and a switch caption were not centred. R1792 had just centred them and
    its gate was green. Both were true: **the gate measured RECTANGLES and the
    reader was looking at GLYPHS.** A run given a 32-wide box for a word whose
    glyphs advance 15 draws them `Start`-aligned at the left of it, so the
    rectangle can be perfectly centred while the ink is 8.5px off.

    Nothing here asked the difference — and the surface that could had existed
    since R1654. `scene/text_painted` publishes `ink_w` / `ink_h` (what the
    shaper produced), `painted` (what was drawn when it differs from what the
    scene holds) and `over_w` / `over_h`. The defect was not that the framework
    could not answer; it was that every gate reached for `scene/snapshot`, which
    carries boxes.

    ## What a row means

    For each run, the smallest TAGGED box of a caption's own scale whose centre
    holds the run's centre — the same "which box is this word in" judgment
    `pinion_widget_paint::caption::escapes` makes, because a word overlapping a
    whole pane is on a pane rather than in a box. `left` / `right` / `top` /
    `bottom` are the INK's gaps inside that box, so `abs(left - right) <= 1` is
    "centred" and nothing else is.

    `scale` bounds how much bigger than the ink a box may be and still count as
    *its* box; `slack` is how many pixels of asymmetry read as centred.
    """
    resp = app.request("scene/text_painted")
    assert resp is not None and isinstance(resp.result, dict), "scene/text_painted answers"
    runs = resp.result["runs"]
    boxes = abs_rects_of(app.snapshot(source="paint"))
    out: list[dict] = []
    for run in runs:
        ink_w, ink_h = run.get("ink_w", 0), run.get("ink_h", 0)
        if ink_w <= 0:
            continue
        # The ink starts where the run's box starts: the shaper lays it out
        # inside that rectangle, and every alignment in this tree is `Start`.
        ix, iy = run["x"], run["y"]
        cx, cy = ix + ink_w // 2, iy + ink_h // 2
        holders = [
            (tag, b)
            for tag, b in boxes.items()
            if b[0] <= cx < b[0] + b[2]
            and b[1] <= cy < b[1] + b[3]
            and b[2] <= ink_w * scale
            # ★★★★★ A caption's OWN box is the ink, so measuring the ink against
            # it answers 0/0 for everything — a vacuous pass, which is the exact
            # shape of the gate this replaces. The question is where the caption
            # sits in the box somebody ELSE drew.
            and not tag.endswith(".caption")
        ]
        if not holders:
            continue
        tag, box = min(holders, key=lambda kv: kv[1][2] * kv[1][3])
        left, top = ix - box[0], iy - box[1]
        out.append(
            {
                "box": tag,
                "content": run.get("content", ""),
                "painted": run.get("painted"),
                "ink": (ink_w, ink_h),
                "box_rect": box,
                "left": left,
                "right": box[2] - ink_w - left,
                "top": top,
                "bottom": box[3] - ink_h - top,
                "centred_x": abs((box[2] - ink_w - left) - left) <= slack,
                "centred_y": abs((box[3] - ink_h - top) - top) <= slack,
            }
        )
    return out


def behind_an_overflow(
    app: "RpcSubprocess", external: str = "/external"
) -> set[str]:
    """★★★★★ R1791 — the tags a screen's toolbar has **moved behind an overflow
    control**, asked of the screen rather than guessed.

    A control the row gave up is not a control the screen lost: it is one press
    away, and the thing holding it says so by name. Every rule of the form *what
    the specification declares is painted* has to know the difference, and the
    honest way to know it is to ask — a hard-coded list here would go stale the
    day a group is added, and a width-dependent list cannot be hard-coded at all.

    ## Why the screen is the one that knows

    Whether anything moved is a function of the room the toolbar has, which is a
    function of the window. Measured on the node lab: the right cluster needs 607
    and is given 410 at the screen's own design width, so what is behind the
    control at one size is on the row at another. No caller can compute that; the
    screen already has.

    Empty for a screen with no such control, so a caller subtracts it
    unconditionally.
    """
    try:
        state = app.query(f"{external}/toolbar_overflow")
    except Exception:  # noqa: BLE001 - a screen without the slot moved nothing
        return set()
    if not isinstance(state, dict):
        return set()
    return set(state.get("moved_seats") or [])


def press_painted_tag(
    app: "RpcSubprocess",
    tag: str,
    viewport: "tuple[int, int]",
    external: str = "/external",
) -> None:
    """★★★★★ R1791 — press a painted tag, **opening the control that holds it
    first** when the toolbar has moved it.

    The same rule the in-process paint gates learned, on the other channel: a
    control the row gave up is one press away rather than gone, so a caller
    aiming at `lab.toolbar.config` goes on meaning the configuration export
    whether or not the window is wide enough to keep it on the row. Two demos
    wrote this press by hand and both broke the moment a group moved; the rule
    belongs here so a third does not have to learn it.

    Re-reading the paint after opening is not optional: the seat's rectangle is
    the menu's, not the row's.
    """
    if tag in behind_an_overflow(app, external):
        control = abs_rects_of(app.snapshot(source="paint", viewport=viewport))[
            "lab.toolbar.more"
        ]
        app.click(at=(control[0] + control[2] // 2, control[1] + control[3] // 2))
        app.tick_ms(16)
    box = abs_rects_of(app.snapshot(source="paint", viewport=viewport))[tag]
    app.click(at=(box[0] + box[2] // 2, box[1] + box[3] // 2))


def widen_until_row_whole(
    app: "RpcSubprocess",
    height: int,
    external: str = "/external",
    *,
    start: int = 1440,
    stop: int = 2560,
    step: int = 64,
) -> "tuple[int, int]":
    """★★★★★ R1791 — resize until the screen's toolbar keeps **every** group on
    the row, and answer the size that happened at.

    ## Why a demo needs this, and why the number is not written down

    A claim of the form *these two seats sit side by side, in this order, before
    the run button* is a claim about the ROW. Since R1791 the row gives a group
    up when it is tight, so at a narrow window those seats are in a column under
    the overflow control and the claim is simply about something else. Widening
    first is what makes the sentence a fact again — and stating the width it was
    read at is R1770's rule, arrived at here from the other direction.

    The width is **derived, not declared**: the crossover moves whenever a seat's
    caption changes, and a constant here would be a fourth copy of a number the
    screen already computes. Measured on the node lab, 2026-08-23: two groups
    moved at 1388 and at 1440, and the search answers **1696** — which is the
    first width its 64px step reaches, not the crossover itself, so a caller may
    read the answer as *a width where the row is whole* and never as *the
    narrowest one*.

    Answers the granted size unchanged for a screen with no overflow slot — such
    a row is whole by construction — and raises when no width up to `stop`
    empties it, because silently reading a moved row is the failure this exists
    to prevent.
    """
    def granted_of(shot: Any) -> "tuple[int, int]":
        rect = shot.get("rect", {}) if isinstance(shot, dict) else {}
        return (rect.get("w", 0), rect.get("h", 0))

    try:
        app.query(f"{external}/toolbar_overflow")
    except Exception:  # noqa: BLE001 - no such control: the row is always whole
        return granted_of(app.snapshot(source="paint"))
    width = start
    while width <= stop:
        shot = resize_and_settle(app, (width, height))
        app.tick_ms(16)
        state = app.query(f"{external}/toolbar_overflow")
        if isinstance(state, dict) and not state.get("moved"):
            return granted_of(shot)
        width += step
    raise AssertionError(
        f"the toolbar still holds groups back at {stop}px wide -- widening is "
        "not what makes this row whole, so the demo's premise is wrong"
    )


def settled_baseline(
    app: "RpcSubprocess",
    size: tuple[int, int],
    external: str = "/external",
    *,
    step: float = 0.05,
    tries: int = 8,
) -> set[str]:
    """★★★★★ R1790 — [`declared_and_painted`], taken once it has **stopped
    moving**.

    ## The defect this is the repair of

    Three demos take a baseline of what a screen paints at its design size and
    compare it against a later read at another size, calling the difference
    *regions the reader can no longer reach*. That rule is false for any region
    with a **lifetime**.

    Measured on the analysis shell: it says `Overview loaded` at boot, so
    `shell.toast` is alive and painted when the baseline is taken (214 regions);
    2.6 seconds later it is gone (213). Whether it survives to the comparison is
    therefore a fact about **how much wall-clock passed**, and a toast is not
    scroll-reachable, so on a slow runner it lands in the unreachable list. That
    is exactly what happened: R1787's CI run failed on
    `r1709_a_resize_is_followed_by_pixels` with `got ['shell.toast']`, and the
    same demo passes here in 8 seconds. A check whose verdict depends on machine
    speed is a flake, and this project does not rerun those.

    ## It ASKS how long, and does not guess

    ★★★★★ The first draft of this took two reads a step apart and returned when
    they agreed. **Running it disproved it**: a 2.6-second toast looks perfectly
    stable across a one-second step, so the helper returned with the toast still
    in the baseline — 214 regions, unchanged, and `STABLE False` when settled
    twice. A predicate that detects *change* cannot establish *impermanence*.

    So it asks. R1790 put `saying` on the wire — the sentence AND how long it
    has left — because `Saying::left`'s own doc already argued for it: *"a test
    that guesses the duration is a test that pins a number this type owns"*, and
    the fact was reachable only from Rust. This advances by exactly what the
    screen says is left, until the screen says nothing is, and then reads. No
    lifetime constant here, no per-screen exemption table, and nothing that goes
    stale when a screen changes its mind about how long it speaks for.

    A screen with no `saying` slot holds no lifetime, so there is nothing to
    settle and the baseline is taken as it stands. `step` and `tries` bound the
    loop rather than drive it: a screen whose sentence never runs down is a
    finding and is raised, not waited out.
    """
    for _ in range(tries):
        try:
            saying = app.query(f"{external}/saying")
        except Exception:  # noqa: BLE001 - a screen without the slot has no lifetime
            return declared_and_painted(app, size, external)
        left = float(saying.get("left", 0.0)) if isinstance(saying, dict) else 0.0
        if left <= 0.0:
            return declared_and_painted(app, size, external)
        # Past the end of it, by the smallest margin that is certainly past.
        app.tick(left + step)
    raise AssertionError(
        f"the screen at {size} is still saying something after {tries} advances; "
        f"a sentence whose time never runs down is a finding, not a wait"
    )


def assert_declared_panes_on_screen(
    app: "RpcSubprocess", size: tuple[int, int], *, label: str
) -> list[str]:
    """Every pane the screen declares is painted, at its declared width, tiling
    the body — asserted at `size`.

    Returns one description per check made, so a caller can count what it
    proved; `[]` for a screen that is not organised in panes, which the caller
    prints rather than reading as a pass.

    The tiling clause is not decoration: three rectangles of the right widths
    that OVERLAP satisfy every width check and are a broken screen.
    """
    panes = declared_panes(app)
    if not panes:
        return []
    made: list[str] = []
    painted = abs_rects_of(app.snapshot(source="paint", viewport=size))
    missing = [p["tag"] for p in panes if p["tag"] not in painted]
    # ★★★★★ R1714 — painted, **or one gesture away**, and the wire says which.
    #
    # A window whose policy declares a pan is a viewport onto a layout bigger
    # than itself, so at a size below the layout a whole pane can be off screen
    # — and reachable. Before there was a pan this could not happen, and the
    # rule was written when it could not: measured, the node lab's inspector at
    # 748 wide is now entirely outside the window and one scroll from being
    # inside it.
    #
    # The check keeps its teeth. A pane that is neither drawn nor reachable
    # still fails, and so does one the report cannot name.
    if missing:
        reach = app.request("scene/scroll_reach")
        assert reach is not None and isinstance(reach.result, dict)
        reachable = {
            row["tag"]
            for row in reach.result["out_of_sight"]
            if row["reach"] == "scrollable" and row["tag"]
        }
        gone = [tag for tag in missing if tag not in reachable]
        assert_eq(
            gone,
            [],
            f"{label} {size}: every declared pane is painted or reachable "
            f"(off screen and reachable: {sorted(set(missing) & reachable)})",
        )
        made.append(f"{label}: {len(missing)} pane(s) off screen and reachable")
        panes = [p for p in panes if p["tag"] in painted]
    made.append(f"{label}: panes painted")
    # ★★★★★ R1860 — a pane is painted BETWEEN the width below which it cannot
    # draw what it holds and the width the design draws it at.
    #
    # This asserted equality with the declared width, and that was the right
    # question only while a pane could not flex: a screen narrower than its
    # design arrangement had no way to serve the shortfall except to lay out at
    # the design arrangement anyway and let the window cut. The capture viewer's
    # side panes now give width back down to a floor derived from what each one
    # has to hold, so equality is a claim about a constant rather than about the
    # screen — R1858's shape, one surface over.
    #
    # ⚠ A screen that publishes no `floor` is read at its declared width, so the
    # default is the STRICTER reading and not a fail-open: a pane that concedes
    # nothing says so by declaring no floor.
    wrong = [
        f"{p['tag']} declares {p['width']} (floor {p.get('floor', p['width'])}) "
        f"and is painted {painted[p['tag']][2]}"
        for p in panes
        if p["width"]
        and not p.get("floor", p["width"]) <= painted[p["tag"]][2] <= p["width"]
    ]
    assert_eq(
        wrong,
        [],
        f"{label} {size}: a pane is painted between the width it can draw in "
        f"and the width it is drawn at",
    )
    made.append(f"{label}: declared widths held")
    row = sorted((painted[p["tag"]] for p in panes), key=lambda r: r[0])
    gaps = [
        f"{b[0]} does not begin where the pane before it ended ({a[0] + a[2]})"
        for a, b in zip(row, row[1:])
        if a[0] + a[2] != b[0]
    ]
    assert_eq(gaps, [], f"{label} {size}: the panes tile the body")
    made.append(f"{label}: panes tile")
    return made


def _walk_tag_rects(snap: Any, *, clipped: bool) -> dict[str, tuple[int, int, int, int]]:
    """One walk behind both readers, so they cannot disagree about the offsets
    they share — only about the clip, which is the whole difference between the
    two questions."""
    out: dict[str, Optional[tuple[int, int, int, int]]] = {}

    def keep(tag: str, rect: Optional[tuple[int, int, int, int]]) -> None:
        # `setdefault`, not `[]=`: a first match that is clipped away has to
        # occupy its slot as `None`, or a later duplicate fills it and this
        # answers a different node from the one the framework resolves.
        out.setdefault(tag, rect)

    def place(
        x: int, y: int, w: int, h: int, clip: Optional[tuple[int, int, int, int]]
    ) -> Optional[tuple[int, int, int, int]]:
        # The unclipped reader returns the sum verbatim — negative coordinates
        # and all. Clamping them to the window would be a THIRD answer, neither
        # "where it can be reached" nor "where the view put it", and a caller
        # comparing a placement against a layout would silently get a rectangle
        # the layout never produced.
        if not clipped:
            return (x, y, w, h)
        return _clipped_into(x, y, w, h, clip)

    def walk(
        node: Any, xoff: int, yoff: int, clip: Optional[tuple[int, int, int, int]]
    ) -> None:
        if not isinstance(node, dict):
            return
        tag = node.get("tag")
        if node.get("type") == "Scroll":
            vp = node.get("viewport") or {}
            vx, vy = vp.get("x", 0), vp.get("y", 0)
            vw, vh = vp.get("w", 0), vp.get("h", 0)
            seat = _clipped_into(vx + xoff, vy + yoff, vw, vh, clip)
            if tag:
                keep(tag, place(vx + xoff, vy + yoff, vw, vh, clip))
            # An empty seat is carried down as an empty clip rather than as
            # "no clip": the children are still walked (a walk that omits nodes
            # is the failure this exists to end) and each reports unreachable,
            # which is the honest answer. Same arm as `Scene::walk_from`.
            walk(
                node.get("content"),
                xoff + vx - node.get("offset_x", 0),
                yoff + vy - node.get("offset_y", 0),
                seat or (0, 0, 0, 0),
            )
            return
        rect = node.get("rect")
        if tag and isinstance(rect, dict):
            keep(tag, place(rect["x"] + xoff, rect["y"] + yoff,
                            rect["w"], rect["h"], clip))
        # ★★ R1685 — a Scroll is no longer the only node that cuts its
        # children: a container that declares `overflow: hidden` publishes
        # `clips: true` and narrows the same way. This mirror had the clip
        # welded to the node KIND — the same shape the framework itself had,
        # and the reason a second clipping kind was expensive — so without
        # this arm it reports a cut mark at its full size and, for one wholly
        # cut away, reports a rectangle where nothing is drawn. Measured the
        # first time this ran: three tags, two partly cut and one gone.
        #
        # No offset arm beside it, deliberately: a container does not move its
        # children, it only stops drawing them past its edge. That asymmetry is
        # the framework's (`Scene::clip_window` versus the scroll's frame
        # shift), and mirroring only half of it is what keeps the two answers
        # the same shape.
        child_clip = clip
        if node.get("clips") and isinstance(rect, dict):
            child_clip = _clipped_into(
                rect["x"] + xoff, rect["y"] + yoff, rect["w"], rect["h"], clip
            ) or (0, 0, 0, 0)
        for child in (node.get("children") or []):
            walk(child, xoff, yoff, child_clip)

    walk(snap, 0, 0, None)
    return {tag: rect for tag, rect in out.items() if rect is not None}


def assert_focus_ring_concentric(snap: Any, offset: int = 2) -> Optional[str]:
    """Assert the focus ring is drawn **exactly** around a real node.

    The ring overlay box (`FOCUS_RING_TAG`) is a top-level sibling, so its
    rect is already window-absolute. This helper recomputes the
    window-absolute rect of every *other* tagged node (scroll-translated,
    via `abs_rects_of`) and asserts the ring is the saturating-inflate of
    exactly one of them — i.e. the ring concentrically frames a node that
    actually paints at that position on screen. A ring placed at a
    scroll-local rect (the pre-R705.1 bug) frames no real absolute node
    and raises here.

    Returns the framed node's tag (for the caller to assert *which*
    widget is framed), or `None` when no ring is present (nothing focused,
    or the focused node is scrolled fully out of view — both legitimate,
    the caller decides whether a ring was expected).
    """
    rects = abs_rects_of(snap)
    ring = rects.get(FOCUS_RING_TAG)
    if ring is None:
        return None
    # (R1324) The VIEWPORT the ring was clamped into — the snapshot root's own rect.
    # `build_focus_ring_box` clamps the ring's FAR edges into it (R1022), so a node
    # flush to the window's right / bottom edge gets a SHORTER ring, not one drawn
    # outside the framebuffer. Modelling only the near clamp (below) made this helper
    # demand a ring 2px past the window bottom for such a node — `hello-nav-rail`'s
    # full-height rail — so `r705_focus_ring_placement` failed against CORRECT shell
    # output. The helper, not the framework, was stale.
    root = snap.get("rect") if isinstance(snap, dict) else None
    view_w = int(root.get("w", 0)) if isinstance(root, dict) else 0
    view_h = int(root.get("h", 0)) if isinstance(root, dict) else 0

    def inflate_sat(r: tuple[int, int, int, int]) -> tuple[int, int, int, int]:
        # Mirrors `build_focus_ring_box`: (1) clamp the near origin and shrink the span
        # by the same clamped amount so the far edge stays at `target + offset` — the
        # ring is concentric, with the framebuffer edge clipping any lost near gap; the
        # LEFT origin floors at 0 and the TOP origin floors at TOP_EDGE_INSET (1px) to
        # dodge the vello y=0 top-tile flood (a stroke touching the framebuffer top row
        # rasterises ~16px thick). (2) R1022: clamp the FAR edges into the viewport, so
        # the stroke of an edge-flush node lands fully visible. Identical to the plain
        # `+2*offset` inflate for any node clear of every window edge.
        top_edge_inset = 1
        x = max(0, r[0] - offset)
        y = max(top_edge_inset, r[1] - offset)
        ideal_right = r[0] + r[2] + offset
        ideal_bottom = r[1] + r[3] + offset
        if view_w:
            ideal_right = min(ideal_right, view_w)
        if view_h:
            ideal_bottom = min(ideal_bottom, view_h)
        return (x, y, ideal_right - x, ideal_bottom - y)

    for tag, r in rects.items():
        if tag == FOCUS_RING_TAG:
            continue
        if inflate_sat(r) == ring:
            return tag
    raise AssertionError(
        f"focus ring {ring} is not concentric around any node's "
        f"window-absolute rect — misplaced. candidates="
        + ", ".join(f"{t}:{inflate_sat(r)}" for t, r in rects.items()
                    if t != FOCUS_RING_TAG)
    )


@dataclass(frozen=True)
class Png:
    """Decoded PNG framebuffer — row-major, top-left origin, RGBA8.

    `pixels` is a `bytes` of length `width * height * 4`. Pixel `(x, y)`
    lives at offset `(y * width + x) * 4`; the four bytes there are
    `R, G, B, A` — same shape `HeadlessScreenshot::render_to_rgba8`
    returns at the Rust side.
    """

    width: int
    height: int
    pixels: bytes


_PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"


def read_png_rgba8(path: Path | str) -> Png:
    """Decode an RGBA8 non-interlaced PNG via the stdlib (zlib + struct).

    Targets the exact subset the wgpu + vello + `png` substrate at
    `pinion-shell::headless_screenshot` emits:

      * 8-bit-per-channel RGBA (color type 6, bit depth 8)
      * no interlace (Adam7 not supported here)
      * any of the five PNG filters per row (None, Sub, Up, Average,
        Paeth)

    R640 §5.7 — the 9-point pixel sampler (`sample_png_points`) sits on
    top of this. Raises `AssertionError` on any format the substrate
    does not produce, so a future format drift surfaces here rather
    than at a downstream pixel comparison.
    """
    raw = Path(path).read_bytes()
    if not raw.startswith(_PNG_SIGNATURE):
        raise AssertionError(f"{path}: not a PNG (signature mismatch)")
    pos = len(_PNG_SIGNATURE)
    width = height = 0
    bit_depth = color_type = interlace = -1
    idat = bytearray()
    seen_iend = False
    while pos < len(raw):
        if pos + 8 > len(raw):
            raise AssertionError(f"{path}: truncated chunk header at {pos}")
        (length,) = struct.unpack(">I", raw[pos : pos + 4])
        chunk_type = raw[pos + 4 : pos + 8]
        data = raw[pos + 8 : pos + 8 + length]
        pos += 8 + length + 4  # skip data + CRC32
        if chunk_type == b"IHDR":
            if length != 13:
                raise AssertionError(f"{path}: IHDR length {length} != 13")
            (width, height, bit_depth, color_type, _comp, _filt, interlace) = (
                struct.unpack(">IIBBBBB", data)
            )
        elif chunk_type == b"IDAT":
            idat.extend(data)
        elif chunk_type == b"IEND":
            seen_iend = True
            break
    if not seen_iend:
        raise AssertionError(f"{path}: missing IEND chunk")
    if bit_depth != 8 or color_type != 6:
        raise AssertionError(
            f"{path}: only RGBA8 (color type 6, bit depth 8) supported, "
            f"got color_type={color_type} bit_depth={bit_depth}"
        )
    if interlace != 0:
        raise AssertionError(f"{path}: interlaced PNG not supported")
    decompressed = zlib.decompress(bytes(idat))
    bpp = 4  # RGBA8
    row_len = width * bpp
    stride = row_len + 1  # 1 filter byte per row
    if len(decompressed) != stride * height:
        raise AssertionError(
            f"{path}: decompressed size {len(decompressed)} != {stride * height}"
        )

    out = bytearray(row_len * height)
    prev_row = bytes(row_len)
    for y in range(height):
        row_start = y * stride
        filter_byte = decompressed[row_start]
        scanline = decompressed[row_start + 1 : row_start + 1 + row_len]
        cur = bytearray(row_len)
        if filter_byte == 0:  # None
            cur[:] = scanline
        elif filter_byte == 1:  # Sub
            for i in range(row_len):
                left = cur[i - bpp] if i >= bpp else 0
                cur[i] = (scanline[i] + left) & 0xFF
        elif filter_byte == 2:  # Up
            for i in range(row_len):
                cur[i] = (scanline[i] + prev_row[i]) & 0xFF
        elif filter_byte == 3:  # Average
            for i in range(row_len):
                left = cur[i - bpp] if i >= bpp else 0
                above = prev_row[i]
                cur[i] = (scanline[i] + ((left + above) >> 1)) & 0xFF
        elif filter_byte == 4:  # Paeth
            for i in range(row_len):
                left = cur[i - bpp] if i >= bpp else 0
                above = prev_row[i]
                upper_left = prev_row[i - bpp] if i >= bpp else 0
                p = left + above - upper_left
                pa = abs(p - left)
                pb = abs(p - above)
                pc = abs(p - upper_left)
                if pa <= pb and pa <= pc:
                    predictor = left
                elif pb <= pc:
                    predictor = above
                else:
                    predictor = upper_left
                cur[i] = (scanline[i] + predictor) & 0xFF
        else:
            raise AssertionError(
                f"{path}: unknown PNG filter byte {filter_byte} at row {y}"
            )
        out[y * row_len : (y + 1) * row_len] = cur
        prev_row = bytes(cur)
    return Png(width=width, height=height, pixels=bytes(out))


def png_pixel(png: Png, x: int, y: int) -> tuple[int, int, int, int]:
    """Return the `(R, G, B, A)` byte tuple at `(x, y)` in `png`."""
    if not 0 <= x < png.width or not 0 <= y < png.height:
        raise AssertionError(
            f"({x}, {y}) outside {png.width}x{png.height} viewport"
        )
    offset = (y * png.width + x) * 4
    return (
        png.pixels[offset],
        png.pixels[offset + 1],
        png.pixels[offset + 2],
        png.pixels[offset + 3],
    )


def sample_png_points(
    png: Png, points: list[tuple[int, int]]
) -> list[tuple[int, int, int, int]]:
    """Vectorised [`png_pixel`] for the 9-point sample arc.

    R640 §5.7 — pair with `PINION_SCREENSHOT` capture +
    `[[center-only-pixel-sample-anti-pattern]]` to verify both
    interior fill AND corner / edge roundness in one assertion batch.
    The companion `design_button_m3_r640.py` demo is the first client.
    """
    return [png_pixel(png, x, y) for (x, y) in points]


def assert_pixel_eq(
    actual: tuple[int, int, int, int],
    expected: tuple[int, int, int, int],
    label: str,
    tolerance: int = 0,
) -> None:
    """Per-channel RGBA byte equality with a tolerance band.

    `tolerance=0` enforces bit-exact match. Higher values tolerate the
    small anti-alias bleed wgpu + vello pipeline produces at fill /
    canvas boundaries (the area AA mode default per
    `headless_screenshot.rs`); a few-byte band covers the half-pixel
    coverage interpolation without admitting whole-channel drift.
    """
    diffs = [abs(int(a) - int(e)) for a, e in zip(actual, expected)]
    if max(diffs) > tolerance:
        raise AssertionError(
            f"{label}: expected {expected} ±{tolerance}, "
            f"got {actual} (max diff {max(diffs)})"
        )


class RealPointerUnavailable(RuntimeError):
    """The host cannot drive a real pointer, and the demo must say so out loud.

    Raised rather than silently degrading to the wire path. A demo whose whole
    claim is "a person's gesture does this" must fail visibly when it could not
    make a person's gesture, because the alternative is the exact shape R1726
    shipped: a green that belonged to a gesture nobody performs.
    """


class RealPointer:
    """★★★★★ R1727 §5.35 §2 #2 — **drive the machine's own pointer, and read
    the scene while the button is down.**

    Every gesture assertion in this tree was made through `scene/drag`, which
    hands the router a press, a march and a release from inside one RPC. R1726
    measured what that costs: on the analysis tool's dashboard the same press
    from the same pixel to the same pixel left the board in two different
    arrangements depending on which path drove it, and three defects were living
    behind the difference — a drop preview that covered the widget under it, a
    drop that missed after a scroll, and a missing cursor label. All three
    needed a real pointer, a scroll, or a pair of eyes, and the harness had none
    of them. The owner found all three by hand and asked why the harness could
    not.

    This is the answer. It moves the X server's pointer with `xdotool`, which is
    the same event source a hand is, and every read in between is the ordinary
    wire — so a demo can press, march, **stop while still holding**, ask the
    surface what it looks like, and only then let go.

    Requires a mapped window (`RpcSubprocess(visible_window=True)`) and an X
    display. On a host without either it raises [`RealPointerUnavailable`]
    rather than falling back, for the reason in that class's docstring.

    ## Calibration is a measurement, not an assumption

    Screen pixels and the surface's logical pixels differ by wherever the window
    manager put the window (and by nothing at all under a bare `Xvfb`). The
    offset is not guessed: the pointer is moved to a known screen point and the
    surface is asked where it thinks the cursor is (`scene/input_state`). If the
    surface reports no cursor, or reports one that does not move when the
    pointer does, the constructor refuses — a demo must not be able to "drive" a
    window that is not receiving anything.
    """

    #: Where the calibration probe lands, in screen pixels. Inside any window
    #: a demo of ordinary size will occupy, and away from its edges.
    PROBE = (400, 300)

    def __init__(
        self,
        tf: "RpcSubprocess",
        *,
        settle: float = 0.12,
        window: Optional[str] = None,
    ) -> None:
        if not getattr(tf, "visible_window", False):
            raise RealPointerUnavailable(
                "a real pointer needs a mapped window — launch with "
                "RpcSubprocess(..., visible_window=True)"
            )
        if not os.environ.get("DISPLAY"):
            raise RealPointerUnavailable("no DISPLAY: there is no pointer to move")
        if shutil.which("xdotool") is None:
            raise RealPointerUnavailable("xdotool is not installed")
        self.tf = tf
        self.settle = settle
        self.window = window
        self._held: set[str] = set()
        self.offset = self._calibrate()

    # -- the X server side -------------------------------------------------
    def _xdo(self, *args: str) -> None:
        subprocess.run(
            ["xdotool", *args],
            check=True,
            capture_output=True,
            text=True,
        )

    def _cursor(self) -> Optional[tuple[float, float]]:
        """Where the SURFACE thinks the cursor is, in its own logical pixels."""
        params: dict[str, Any] = {}
        if self.window is not None:
            params["window"] = self.window
        resp = self.tf.request("scene/input_state", params)
        assert resp is not None
        cursor = resp.result.get("cursor")
        if cursor is None:
            return None
        return (float(cursor["x"]), float(cursor["y"]))

    def _calibrate(self) -> tuple[float, float]:
        self._xdo("mousemove", str(self.PROBE[0]), str(self.PROBE[1]))
        time.sleep(max(self.settle, 0.25))
        first = self._cursor()
        if first is None:
            raise RealPointerUnavailable(
                "the surface reports no cursor after a real pointer move — it is "
                "not receiving pointer events, so nothing driven here would mean "
                "anything"
            )
        # A second, DIFFERENT probe: a surface that answers a constant would
        # pass the first check while receiving nothing.
        second_screen = (self.PROBE[0] + 40, self.PROBE[1] + 30)
        self._xdo("mousemove", str(second_screen[0]), str(second_screen[1]))
        time.sleep(max(self.settle, 0.25))
        second = self._cursor()
        if second is None or second == first:
            raise RealPointerUnavailable(
                f"the surface's cursor did not move with the pointer "
                f"({first} -> {second}): the window is not under it"
            )
        moved = (second[0] - first[0], second[1] - first[1])
        if (round(moved[0]), round(moved[1])) != (40, 30):
            raise RealPointerUnavailable(
                f"the surface saw the pointer travel {moved}, not (40, 30): its "
                "logical pixels are not the screen's, and this driver has no "
                "scale factor"
            )
        return (float(second_screen[0]) - second[0], float(second_screen[1]) - second[1])

    # -- the gesture -------------------------------------------------------
    def move(self, at: tuple[float, float], *, confirm: bool = False) -> None:
        """Put the real pointer at a point in the surface's logical pixels."""
        sx = int(round(at[0] + self.offset[0]))
        sy = int(round(at[1] + self.offset[1]))
        self._xdo("mousemove", str(sx), str(sy))
        time.sleep(self.settle)
        if confirm:
            got = self._cursor()
            # ★★★★★ R1795 — against the pixel the driver ACTUALLY asked for, and
            # within one, because **a fractional aim cannot be hit by an integral
            # pointer**. This compared `round(got)` with `round(at)` — two
            # separate roundings of the same fractional value, through Python's
            # banker's rounding, so an aim at `x.5` rounds DOWN here and the X
            # server had already put the pointer one pixel up.
            #
            # It surfaced when R1794 measured seat widths: a node card's width
            # became odd, its centre landed on `.5`, and a demo that had aimed at
            # whole pixels for two hundred rounds started aiming between them.
            # The aim is legitimately fractional — it is the centre of a
            # rectangle — so the check is what has to admit that a pointer is
            # not.
            want = (sx - self.offset[0], sy - self.offset[1])
            assert (
                got is not None
                and abs(got[0] - want[0]) <= 1
                and abs(got[1] - want[1]) <= 1
            ), (
                f"aimed the real pointer at {at} (pixel {want}), the surface "
                f"received {got}"
            )

    def press(self, button: str = "left") -> None:
        self._xdo("mousedown", _REAL_POINTER_BUTTONS[button])
        self._held.add(button)
        time.sleep(self.settle)

    def release(self, button: str = "left") -> None:
        self._xdo("mouseup", _REAL_POINTER_BUTTONS[button])
        self._held.discard(button)
        time.sleep(self.settle)

    def drag(
        self,
        *,
        from_at: tuple[float, float],
        to_at: tuple[float, float],
        steps: int = 8,
        button: str = "left",
        hold: Optional[Callable[["RealPointer"], None]] = None,
    ) -> None:
        """Press, march through `steps` points, optionally STOP, then release.

        `hold` runs with the button still down and the pointer at `to_at`,
        which is the whole point of this class: it is where a demo reads the
        scene mid-gesture. Anything it raises still releases the button.
        """
        self.move(from_at)
        self.press(button)
        try:
            for step in range(1, steps + 1):
                t = step / steps
                self.move(
                    (
                        from_at[0] + (to_at[0] - from_at[0]) * t,
                        from_at[1] + (to_at[1] - from_at[1]) * t,
                    )
                )
            if hold is not None:
                hold(self)
        finally:
            self.release(button)

    def __enter__(self) -> "RealPointer":
        return self

    def __exit__(self, *exc: Any) -> None:
        # ★ A held button outlives the process that pressed it. Leaving one down
        # on a shared display makes every later window behave as if a hand were
        # dragging across it, which is a failure mode a demo's own teardown must
        # not be able to cause.
        for button in list(self._held):
            try:
                self.release(button)
            except Exception:  # noqa: BLE001 — teardown must not mask the body
                pass
        # ★★★★★ R1737 — every surface this session's pointer reached must have
        # been told the pixel the pointer was actually over. This is the check
        # that found R1736's defect, made total: it used to require a screen that
        # publishes a cursor field (three of five did, in two spellings) and a
        # 600-point sweep. Now the framework compares its own two accounts at
        # every event, so ANY demo that drives a real pointer gets the round trip
        # checked for free, over exactly the pixels its own gestures used.
        #
        # Skipped while an exception is already propagating: the body's failure
        # is the interesting one and a teardown assertion would bury it.
        if exc and exc[0] is not None:
            return
        assert_no_pointer_drift(self.tf, label="real-pointer session")


#: xdotool's button numbers, named the way the rest of this harness names them.
_REAL_POINTER_BUTTONS = {"left": "1", "middle": "2", "right": "3"}


def pointer_arrivals(tf: "RpcSubprocess") -> Optional[dict]:
    """★★★★★ R1737 — where a pointer arrived in every surface, from the
    framework's own record.

    `None` on a binary that predates the method, the same tolerance
    `_gate_pointer_targets` gives — a stale binary is not evidence of a defect.
    """
    try:
        resp = tf.request("scene/pointer_arrival")
    except RpcError as exc:
        if exc.code == -32601:
            return None
        raise
    assert resp is not None
    return resp.result


def assert_no_pointer_drift(tf: "RpcSubprocess", *, label: str = "pointer") -> Optional[dict]:
    """★★★★★ R1737 — refuse a run in which a pointer was delivered to a surface
    at a different pixel from the one it was over.

    The framework holds two accounts of every arrival — the cursor the window
    system reported, and the fraction the surface multiplies back — and
    `Landing` is the comparison. `drifted` is the one verdict with no benign
    reading: `strayed` is a capture lock forwarding past the rectangle's edge on
    purpose, and `never` is a surface this run did not point at.

    ★ The framework COUNTS, so this covers every arrival the session caused
    rather than the last one. That distinction is the whole point: the first
    draft read only the most recent arrival, so a six-hundred-position sweep
    with one bad pixel in the middle would have passed — which is R1736's own
    finding (a gate whose coverage is an accident of which point it looked at).

    Returns the report, so a caller can also assert on WHAT it covered — which
    matters, because "nothing drifted" is trivially true of a run that pointed
    at nothing, and `delivered` is the number that says whether it did.
    """
    report = pointer_arrivals(tf)
    if report is None:
        return None
    if report.get("defects"):
        rows = []
        for row in report.get("surfaces", []):
            evidence = row.get("drifted_at")
            if evidence is None:
                continue
            rows.append(
                f"{row['surface']}: {row['drifted']} of {row['delivered']} "
                f"arrival(s) went wrong; the first had the pointer over "
                f"{tuple(evidence['inside'])} of {evidence['over']} and told the "
                f"surface {tuple(evidence['resolved'])} (drift "
                f"{tuple(evidence['drift'])})"
            )
        raise AssertionError(
            f"{label}: {report['drifts']} arrival(s) across {report['defects']} "
            f"surface(s) told a different pixel from the one the pointer was over "
            f"— {'; '.join(rows[:4])}. Every press on a self-hit-testing screen "
            f"is resolved against this pixel, so the whole screen is aimed wrong "
            f"by that much; see `pinion_core::external::pixel_of` for the "
            f"arithmetic and R1736 for the measurement of what it costs a person."
        )
    return report


def assert_gesture_reads_one_fact(
    example: str,
    *,
    from_at: tuple[float, float],
    to_at: tuple[float, float],
    read: Callable[["RpcSubprocess"], Any],
    steps: int = 8,
    label: str = "gesture",
    env: Optional[dict[str, str]] = None,
) -> Any:
    """★★★★★ R1727 — the same press-march-release, delivered two ways, must
    leave the same state.

    The gate the class debt `paint-and-gesture-read-two-facts` never had. A
    captured pointer arrives as a reading over a rectangle the last **paint**
    produced; if the consumer scales it by anything read from the **model**, the
    answer depends on whether a frame happened between two moves — and a real
    pointer usually gets one, so the wrong reading hides behind the right
    answer. Delivering the identical march with and without that frame is what
    makes the difference observable without a display:

    * **one call** — `scene/drag` marches inside a single drain, no frame
      between the moves;
    * **one call per move** — the shell paints between them, as a hand does.

    Measured before the repair, on the analysis tool's dashboard: one call put
    the dragged card on row 10 and the other on row 4. A consumer that reads
    [`PointerReading::px`] answers the same either way, and this refuses the
    round if it ever stops doing so.

    Returns the (single, agreed) reading, so a caller can go on to assert what
    it is as well as that it is one thing.
    """
    points = [
        (
            from_at[0] + (to_at[0] - from_at[0]) * i / steps,
            from_at[1] + (to_at[1] - from_at[1]) * i / steps,
        )
        for i in range(1, steps + 1)
    ]

    with RpcSubprocess(example, env=env) as tf:
        tf.drag(from_at=from_at, to_at=to_at, steps=steps, phase="begin")
        tf.drag(from_at=to_at, to_at=to_at, steps=0, phase="end")
        batched = read(tf)

    with RpcSubprocess(example, env=env) as tf:
        tf.drag(from_at=from_at, to_at=from_at, steps=0, phase="begin")
        for point in points:
            tf.drag(from_at=point, to_at=point, steps=0, phase="move")
        tf.drag(from_at=to_at, to_at=to_at, steps=0, phase="end")
        per_move = read(tf)

    assert batched == per_move, (
        f"{label}: the same gesture answered differently depending on whether a "
        f"frame happened between its moves — one call gave {batched!r}, one call "
        f"per move gave {per_move!r}. The reading is being scaled by something "
        f"the gesture itself moves; see pinion_core::PointerReading::px"
    )
    return batched


def run_demo(name: str, body) -> NoReturn:
    """Run one demo body and EXIT with its status. Never returns.

    R1527 — this used to `return` the status, and the sweep
    (`tools/sweep_headless.sh`) judges a demo by its exit code, so a
    caller that dropped the value made its demo incapable of failing:
    every assertion inside ran, printed `[demo] FAIL: ...`, and exited
    0. Measured 2026-08-01, exactly two of 474 demos dropped it —
    `r1520_scrolled_paint_cache` and `r1521_shape_cache_working_set`,
    the cost-counter demos of the two rounds immediately before this
    one, and the perf axis's own `demo-body` evidence.

    Raising `SystemExit` here instead of returning a number makes that
    unrepresentable rather than merely fixed: there is no longer a value
    to drop. Every existing form keeps working unchanged —
    `sys.exit(run_demo(...))` (451 demos), `raise SystemExit(...)` (8),
    `return run_demo(...)` inside a `main()` reached by `sys.exit(main())`
    (14) — because in each the wrapper only ever forwarded what this
    function now raises, and no demo calls it twice or runs anything
    after it (both verified by an AST census over `tools/demos/`).
    """
    print(f"[demo] {name}")
    started = time.monotonic()
    try:
        body()
    except AssertionError as exc:
        print(f"[demo] FAIL: {exc}", file=sys.stderr)
        sys.exit(1)
    except RpcError as exc:
        print(f"[demo] RPC ERROR: {exc}", file=sys.stderr)
        sys.exit(2)
    except Exception as exc:
        print(f"[demo] UNEXPECTED: {exc!r}", file=sys.stderr)
        sys.exit(3)
    elapsed = time.monotonic() - started
    print(f"[demo] PASS ({elapsed:.2f}s)")
    # ★★★★★ R1984 — a PASS is the evidence the push gate reads.
    #
    # Written here, at the one place that knows the body succeeded, for every
    # binary this demo launched. `driven_binaries.write_record` refuses a launch
    # with no artifact behind it (a `cargo run` fallback), because a record with
    # no binary identity is evidence of nothing.
    #
    # ⚠ It never fails the demo. This is bookkeeping about a run that already
    # passed, and a walk that reported FAIL because its own record could not be
    # written would be a harness turning a green claim red for a reason the
    # claim has nothing to do with.
    for package, binary in _DRIVEN:
        try:
            driven_binaries.write_record(package, binary, name)
        except OSError as why:  # pragma: no cover — a full or read-only target/
            print(f"[demo] (could not record the run of {package}: {why})")
    sys.exit(0)


def iter_demos() -> Iterator[str]:
    demos_dir = Path(__file__).resolve().parent / "demos"
    for path in sorted(demos_dir.glob("*.py")):
        if not path.name.startswith("_"):
            yield path.name
