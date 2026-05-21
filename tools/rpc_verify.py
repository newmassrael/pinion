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

Python 3.9+ stdlib only — no third-party deps. Run from the workspace
root so `cargo run -p <example>` resolves.
"""

from __future__ import annotations

import json
import queue
import shutil
import signal
import subprocess
import sys
import threading
import time
from contextlib import AbstractContextManager
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterator, Optional


WORKSPACE_ROOT = Path(__file__).resolve().parent.parent


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
    ) -> None:
        self.example = example
        self.release = release
        self.boot_grace = boot_grace
        self.request_timeout = request_timeout

        self._proc: Optional[subprocess.Popen] = None
        self._inbox: "queue.Queue[str]" = queue.Queue()
        self._stderr_lines: list[str] = []
        self._stderr_thread: Optional[threading.Thread] = None
        self._stdout_thread: Optional[threading.Thread] = None
        self._next_id = 1

    def __enter__(self) -> "RpcSubprocess":
        binary = self._resolve_binary()
        cmd = [str(binary)] if binary else self._cargo_run_cmd()
        self._proc = subprocess.Popen(
            cmd,
            cwd=WORKSPACE_ROOT,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            bufsize=1,
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

    def snapshot(self, path: str = "") -> Any:
        resp = self.request("scene/snapshot", {"path": path})
        assert resp is not None
        return resp.result

    def intents(self) -> list[Any]:
        resp = self.request("scene/intents")
        assert resp is not None
        result = resp.result
        return list(result) if isinstance(result, list) else []

    def stderr_tail(self, n: int = 20) -> list[str]:
        return list(self._stderr_lines[-n:])


def assert_eq(actual: Any, expected: Any, label: str = "value") -> None:
    if actual != expected:
        raise AssertionError(
            f"{label}: expected {expected!r}, got {actual!r}"
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
