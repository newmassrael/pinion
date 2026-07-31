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
from typing import Any, Iterator, NoReturn, Optional

from build_gate import BuildError, ensure_built


WORKSPACE_ROOT = Path(__file__).resolve().parent.parent

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

        self._proc: Optional[subprocess.Popen] = None
        self._inbox: "queue.Queue[str]" = queue.Queue()
        self._stderr_lines: list[str] = []
        self._stderr_thread: Optional[threading.Thread] = None
        self._stdout_thread: Optional[threading.Thread] = None
        self._next_id = 1

    def __enter__(self) -> "RpcSubprocess":
        binary = self._resolve_binary()
        cmd = [str(binary)] if binary else self._cargo_run_cmd()
        # R835 §5.16 — windowless-by-default env. Hidden unless the demo
        # asked for a visible window (x11grab screen capture) or the caller
        # set PINION_HIDDEN_WINDOW explicitly.
        env = dict(os.environ)
        if self.visible_window:
            env.pop("PINION_HIDDEN_WINDOW", None)
        elif "PINION_HIDDEN_WINDOW" not in env:
            env["PINION_HIDDEN_WINDOW"] = "1"
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
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.shutdown()

    def shutdown(self) -> None:
        if self._proc is None:
            return
        if self._proc.poll() is None:
            try:
                if self._proc.stdin is not None and not self._proc.stdin.closed:
                    self._proc.stdin.close()
            except (OSError, BrokenPipeError):
                pass
            try:
                self._proc.send_signal(signal.SIGTERM)
                self._proc.wait(timeout=2.0)
            except subprocess.TimeoutExpired:
                self._proc.kill()
                self._proc.wait(timeout=1.0)
        self._proc = None

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

        Set the pointer PRESSURE (W3C `PointerEvent.pressure` / Qt
        `QTabletEvent::pressure()`), normalised `0.0..=1.0`. Positionless
        (out-of-band, like `modifiers()`): the value is delivered to the surface
        under the pointer at once and rides subsequent moves. The AI-first source
        for a pressure-reactive surface (an ink brush, a DCC viewport), so a
        tablet is not required to exercise force headless.
        """
        self.request("scene/pointer_pressure", {"value": float(value)})

    def pointer_tilt(self, tilt_x: float, tilt_y: float) -> None:
        """`scene/pointer_tilt` typed wrapper (R1429 §5.35 §5.15).

        Set the pointer TILT (W3C `PointerEvent.tiltX/tiltY` / Qt
        `QTabletEvent::xTilt/yTilt`), each axis in degrees `-90.0..=90.0`.
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

        Set the pointer TWIST (W3C `PointerEvent.twist` / Qt
        `QTabletEvent::rotation()`), the barrel rotation in degrees, wrapped to
        `0.0..=360.0` at the router. Positionless (out-of-band), delivered to the
        surface under the pointer at once; winit exposes no barrel axis, so the
        RPC is the sole driver.
        """
        self.request("scene/pointer_twist", {"twist": float(twist)})

    def pointer_tangential_pressure(self, tangential: float) -> None:
        """`scene/pointer_tangential_pressure` typed wrapper (R1430 §5.35 §5.15).

        Set the airbrush finger-wheel position (W3C
        `PointerEvent.tangentialPressure` / Qt
        `QTabletEvent::tangentialPressure()`), clamped to `-1.0..=1.0` at the
        router. Positionless, out-of-band.
        """
        self.request(
            "scene/pointer_tangential_pressure", {"tangential": float(tangential)}
        )

    def pointer_height(self, height: float) -> None:
        """`scene/pointer_height` typed wrapper (R1430 §5.35 §5.15).

        Set the pointer HEIGHT (Qt `QTabletEvent::z()`), the hover distance above
        the surface, floored at `0.0` at the router. Positionless, out-of-band;
        no W3C peer.
        """
        self.request("scene/pointer_height", {"height": float(height)})

    def pointer_type(self, kind: str) -> None:
        """`scene/pointer_type` typed wrapper (R1431 §5.35 §5.15).

        Set the pointer DEVICE kind (W3C `PointerEvent.pointerType` / Qt
        `QTabletEvent::pointerType()`): one of ``"mouse"`` / ``"pen"`` /
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

        Drive a native PINCH (magnify) gesture — the Qt `QNativeGestureEvent`
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

        Drive a native ROTATION gesture — the Qt `QNativeGestureEvent`
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

        Drive a native N-finger PAN gesture — the Qt `QNativeGestureEvent`
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

        Drive a native SMART-ZOOM — the two-finger double tap, Qt
        `QNativeGestureEvent` `SmartZoomNativeGesture` / winit
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


def assert_eq(actual: Any, expected: Any, label: str = "value") -> None:
    if actual != expected:
        raise AssertionError(
            f"{label}: expected {expected!r}, got {actual!r}"
        )


def assert_rpc_error(fn, *, code: int = -32602, data: Any = None) -> None:
    """Assert `fn()` raises a JSON-RPC error with the given `code` (and, when
    supplied, `error.data`) — and did NOT succeed.

    The typed peer of `assert_eq` for the wire's failure channel. The dispatch
    layer maps every `InvokeError` / `InterveneError` variant to a `-32602
    Invalid params` carrying the variant name in `error.data` (e.g.
    `"InvokeRejected"`, `"ReadOnly"`), so `data=` proves the failure travelled
    the real wire with the right typed reason — not a silent success and not a
    generic transport error. Callers pass a zero-arg lambda:
    `assert_rpc_error(lambda: g.invoke(P, A), data="InvokeRejected")`.
    """
    try:
        fn()
    except RpcError as exc:
        assert_eq(exc.code, code, f"{data or 'error'}: JSON-RPC code")
        if data is not None:
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


def abs_rects_of(snap: Any) -> dict[str, tuple[int, int, int, int]]:
    """Map every tagged node to its **window-absolute** rect `(x, y, w, h)`.

    Independently re-implements the scroll-offset translation that
    `pinion_core::scene::Scene::rect_for_tag_absolute` does in Rust: a
    node inside a `Scroll` carries a scroll-LOCAL rect, and the renderer
    paints it at `viewport_pos + (local - scroll_offset)`. Accumulating
    `(viewport.x - offset_x, viewport.y - offset_y)` per Scroll boundary
    yields the on-screen position. This is the GROUNDING that makes a
    focus-ring assertion non-tautological: the ring rect (a top-level
    overlay, already window-absolute) is checked against a *separately
    computed* absolute position, so a ring drawn at a scroll-local rect
    is caught (R705.1, [[introspection-from-paint-not-screen]]).
    """
    out: dict[str, tuple[int, int, int, int]] = {}

    def walk(node: Any, xoff: int, yoff: int) -> None:
        if not isinstance(node, dict):
            return
        tag = node.get("tag")
        if node.get("type") == "Scroll":
            vp = node.get("viewport") or {}
            if tag:
                out[tag] = (vp.get("x", 0) + xoff, vp.get("y", 0) + yoff,
                            vp.get("w", 0), vp.get("h", 0))
            nx = xoff + vp.get("x", 0) - node.get("offset_x", 0)
            ny = yoff + vp.get("y", 0) - node.get("offset_y", 0)
            walk(node.get("content"), nx, ny)
            return
        rect = node.get("rect")
        if tag and isinstance(rect, dict):
            out[tag] = (rect["x"] + xoff, rect["y"] + yoff, rect["w"], rect["h"])
        for child in (node.get("children") or []):
            walk(child, xoff, yoff)

    walk(snap, 0, 0)
    return out


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
    The companion `figma_button_m3_r640.py` demo is the first client.
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


def run_demo(name: str, body) -> int:
    print(f"[demo] {name}")
    started = time.monotonic()
    try:
        body()
    except AssertionError as exc:
        print(f"[demo] FAIL: {exc}", file=sys.stderr)
        return 1
    except RpcError as exc:
        print(f"[demo] RPC ERROR: {exc}", file=sys.stderr)
        return 2
    except Exception as exc:
        print(f"[demo] UNEXPECTED: {exc!r}", file=sys.stderr)
        return 3
    elapsed = time.monotonic() - started
    print(f"[demo] PASS ({elapsed:.2f}s)")
    return 0


def iter_demos() -> Iterator[str]:
    demos_dir = Path(__file__).resolve().parent / "demos"
    for path in sorted(demos_dir.glob("*.py")):
        if not path.name.startswith("_"):
            yield path.name
