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
from typing import Any, Iterator, Optional


WORKSPACE_ROOT = Path(__file__).resolve().parent.parent


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
        visible_window: bool = False,
    ) -> None:
        self.example = example
        self.release = release
        self.boot_grace = boot_grace
        self.request_timeout = request_timeout
        # R835 §5.16 — by default the shell window is created UNMAPPED
        # (`PINION_HIDDEN_WINDOW`) so a local verification run renders the
        # full real pipeline (winit + GPU + Vello + present) WITHOUT a
        # window flashing on the developer's display / stealing focus.
        # Demos that screen-capture the real window (ffmpeg x11grab) pass
        # `visible_window=True` so the window is mapped for the grab.
        self.visible_window = visible_window

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
        try:
            self.pointer_leave()
        except RpcError as exc:
            if exc.code != -32601:
                raise
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

    def query(self, path: str) -> Any:
        resp = self.request("scene/query", {"path": path})
        assert resp is not None
        return resp.result

    def invoke(self, path: str, args: Any) -> Any:
        resp = self.request("scene/invoke", {"path": path, "args": args})
        assert resp is not None
        return resp.result

    def intervene(self, path: str, value: Any) -> None:
        """`scene/intervene` typed wrapper (R56.1.f.3 §5.22).

        Mirrors `invoke` shape — `{"path": str, "value": Any}` —
        but routes through the §5.15 item 7 state-write channel
        instead of the §5.15 item 8 action channel. `value=None`
        sends a JSON `null`, which `TextFieldExternal::intervene`
        treats as "clear selection" on the `selection` slot.
        """
        resp = self.request("scene/intervene", {"path": path, "value": value})
        assert resp is not None

    def snapshot(
        self,
        path: str = "",
        *,
        source: str = "state",
        viewport: Optional[tuple[int, int]] = None,
    ) -> Any:
        """`scene/snapshot` typed wrapper.

        `source="state"` (default) dumps the state scene root (root
        `External`). `source="paint"` dumps the paint scene produced
        by `V::view` at `viewport` (default 720x480) — see R51.194
        §5.49 §5.45 for the wire shape.
        """
        params: dict[str, Any] = {"path": path, "from": source}
        if viewport is not None:
            params["viewport"] = {"w": viewport[0], "h": viewport[1]}
        resp = self.request("scene/snapshot", params)
        assert resp is not None
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
        """
        if not name:
            raise ValueError("key name must not be empty")
        if (at is None) == (path is None):
            raise ValueError("exactly one of `at` or `path` must be supplied")
        params: dict[str, Any] = {"key": name}
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
    ) -> None:
        """`scene/click` typed wrapper (R51.196 / R51.201 §5.49).

        Mutually exclusive — supply exactly one:
          * `at = (x, y)` — click at the given logical-pixel coordinate.
          * `path = "<tag>"` — R51.201 path-based form: the dispatcher
            walks the paint scene for the first node carrying `tag`
            and clicks at its rect centre. Eliminates the
            `snapshot → find_by_tag → node_center` boilerplate when
            the caller only wants "click on widget X".

        The shell drains the deferred-input inbox after the request
        returns, applying `cursor_moved`, `mouse_pressed`, then
        `mouse_released` so the `InputRouter` fires the same
        activation arc winit's `WindowEvent::MouseInput` triggers
        from a real mouse click. Follow up with `query(...)` or
        `snapshot(...)` to observe the post-click state transition.
        """
        if (at is None) == (path is None):
            raise ValueError("exactly one of `at` or `path` must be supplied")
        if at is not None:
            params: dict[str, Any] = {"at": {"x": float(at[0]), "y": float(at[1])}}
        else:
            assert path is not None
            params = {"path": path}
        self.request("scene/click", params)

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

    def set_fps(self, fps: int) -> None:
        """`scene/set_fps` typed wrapper (R829 §2 #4 §5.28).

        Sets the addressed window's target frame rate — the §2 #4
        game-loop pacing policy. `fps=0` *pauses* the per-window paint
        clock so the continuous immediate-mode loop stops auto-advancing;
        the window then only repaints on an explicit `tick()` step, so an
        AI client frame-steps the immediate-mode game loop
        deterministically (`set_fps(0)` then `tick(dt)` advances the
        drivers by exactly `dt`). `fps=N` (re)starts the continuous loop
        at N fps. `fps` must be a non-negative integer.
        """
        self.request("scene/set_fps", {"fps": fps})

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


def assert_eq(actual: Any, expected: Any, label: str = "value") -> None:
    if actual != expected:
        raise AssertionError(
            f"{label}: expected {expected!r}, got {actual!r}"
        )


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

    def inflate_sat(r: tuple[int, int, int, int]) -> tuple[int, int, int, int]:
        # Mirrors `build_focus_ring_box` (R806): clamp the near origin and
        # shrink the span by the same clamped amount so the far edge stays at
        # `target + offset` — the ring is concentric, with the framebuffer
        # edge clipping any lost near gap. The LEFT origin floors at 0; the
        # TOP origin floors at TOP_EDGE_INSET (1px) to dodge the vello y=0
        # top-tile flood (a stroke touching the framebuffer top row
        # rasterises ~16px thick). Identical to the plain `+2*offset` inflate
        # for any node clear of the top/left edge.
        top_edge_inset = 1
        x = max(0, r[0] - offset)
        y = max(top_edge_inset, r[1] - offset)
        ideal_right = r[0] + r[2] + offset
        ideal_bottom = r[1] + r[3] + offset
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
