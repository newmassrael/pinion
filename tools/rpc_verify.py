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
        cursor location. The shell drains the deferred-input inbox
        after this returns, applying `cursor_moved` then
        `handle_named_key`, so the substrate first offers the key
        to `V::apply_key` (focused widget shortcut) and falls
        through to the §5.45 R55.C.3 scroll arc for unhandled
        arrow / page / Home / End over a `Scene::Scroll`.
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
