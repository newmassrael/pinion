#!/usr/bin/env python3
"""hello-audio-rt (engine extra) — the retained single-thread audio engine over RPC.

The companion of `hello_audio_rt.py`. That demo drives the real-time *device*
path (the primary External, invoke-write). This one drives the retained
single-thread [`AudioEngineExternal`] hosted as an **extra** External at
`/engine/external/...` in the same `hello-audio-rt` binary.

This is the single-thread half of [[debt-audio-rpc-wire-proof]] (Finding A,
plan item 3). That surface is `hello-audio`'s, but `hello-audio` runs on
`pinion_tui` and paints to stdout — the same channel the JSON-RPC server replies
on — so it could never be cleanly driven over stdin/stdout RPC. Hosting the
engine in the `pinion_shell` GUI harness (which paints to a GPU window, leaving
stdout a clean RPC channel) is the "GUI variant" the debt anticipated.

Why prove it separately from the RT path: the retained engine has no lock-free
control split, so its §2 #7 read twins are **directly writable through
`intervene`** — `master_gain` / `listener` / `attenuation` /
`voice.<id>.{gain,pan,position}` — a genuinely different §2 #2 write contract
from the RT primary's invoke-only writes. A retained `play` updates `voice_count`
synchronously, and the same `render` step-verb the RT path uses reaps finished /
stopped voices (`AudioEngine::render`), so the full play -> stop -> reap lifecycle
is exercised over the wire. Both are proven here.

Run from the workspace root:
    cargo build -p hello-audio-rt --release
    python3 tools/demos/hello_audio_engine.py

>= 25 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    assert_rpc_error,
    run_demo,
)

# The retained single-thread engine is an extra External addressed by tag.
ENG = "/engine/external"


def _find_voice(voices: list[dict], label: str) -> dict:
    for v in voices:
        if v.get("label") == label:
            return v
    raise AssertionError(f"no voice labelled {label!r} in {voices!r}")


def body() -> None:
    with RpcSubprocess("hello-audio-rt", request_timeout=12.0) as g:
        def q(name: str) -> Any:
            return g.query(f"{ENG}/{name}")

        def inv(verb: str, args: Any) -> Any:
            return g.invoke(f"{ENG}/{verb}", args)

        def iv(path: str, value: Any) -> None:
            g.intervene(f"{ENG}/{path}", value)

        def render(blocks: int) -> Any:
            return inv("render", blocks)

        # ── boot baseline of the retained engine surface.
        assert_eq(q("voice_count"), 0, "engine boots silent")
        assert_eq(q("sample_rate"), 48_000, "engine sample rate")
        assert_eq(q("master_gain"), 1.0, "default master gain")
        clips = q("clips")
        assert "bell" in clips and "waves" in clips, f"registered clips: {clips!r}"
        assert_eq(q("voices"), [], "no live voices")

        # ── a retained `play` applies synchronously — no render step.
        bid = inv("play", "bell")
        assert isinstance(bid, int) and bid >= 1, f"play mints an id: {bid!r}"
        assert_eq(q("voice_count"), 1, "retained play is synchronous (no render)")
        bell = _find_voice(q("voices"), "bell")
        assert abs(bell["gain"] - 1.0) < 1e-3, "authored gain 1.0"
        assert abs(bell["effective_gain"] - 1.0) < 1e-3, "flat voice: effective == authored"
        assert "position" not in bell, "a flat voice omits its 3D position"

        # A JSON play carries options over the wire.
        inv("play", {"name": "waves", "gain": 0.3, "looping": True})
        waves = _find_voice(q("voices"), "waves")
        assert_eq(waves["looping"], True, "looping option honoured")
        assert abs(waves["gain"] - 0.3) < 1e-3, "authored gain 0.3 over the wire"

        # ── the distinct surface: WRITES through `intervene` (not invoke).
        iv("master_gain", 0.25)
        assert_eq(q("master_gain"), 0.25, "master_gain intervene round-trips over the wire")

        # Per-voice gain is the write twin of the `voices[].gain` read.
        iv(f"voice.{bid}.gain", 0.5)
        bell = _find_voice(q("voices"), "bell")
        assert abs(bell["gain"] - 0.5) < 1e-3, "per-voice gain intervene reached the voice"

        # Per-voice position spatialises: distance attenuation resolves.
        iv(f"voice.{bid}.position", [2.0, 0.0, 0.0])
        bell = _find_voice(q("voices"), "bell")
        assert abs(bell["distance"] - 2.0) < 1e-3, f"placed at distance 2: {bell['distance']}"
        # effective = authored 0.5 x 1/(1+1) distance atten = 0.25.
        assert abs(bell["effective_gain"] - 0.25) < 1e-3, (
            f"gain x distance attenuation: {bell['effective_gain']}"
        )
        # `null` un-spatialises back to a flat voice.
        iv(f"voice.{bid}.position", None)
        bell = _find_voice(q("voices"), "bell")
        assert "distance" not in bell, "un-spatialised: flat again"

        # Listener intervene (partial update keeps unspecified axes).
        listener = q("listener")
        assert_eq(listener["position"], [0.0, 0.0, 0.0], "listener at origin")
        assert_eq(listener["forward"], [0.0, 0.0, -1.0], "listener faces -Z")
        iv("listener", {"position": [5.0, 0.0, 0.0]})
        listener = q("listener")
        assert_eq(listener["position"], [5.0, 0.0, 0.0], "listener moved via intervene")
        assert_eq(listener["forward"], [0.0, 0.0, -1.0], "partial update kept forward")

        # Attenuation intervene (partial update).
        ref_before = q("attenuation")["reference_distance"]
        iv("attenuation", {"rolloff": 0.0})
        atten = q("attenuation")
        assert abs(atten["rolloff"]) < 1e-9, "rolloff zeroed via intervene"
        assert_eq(atten["reference_distance"], ref_before, "partial update kept reference")

        # ── stop marks a voice finished; the `render` step-verb reaps it — the
        # full play -> stop -> reap lifecycle, exactly as a capture / device pull
        # loop drives the retained engine. (`render` steps AudioEngine::render,
        # which produces PCM and drops finished voices.)
        assert_eq(q("voice_count"), 2, "two live voices before the stop")
        assert_eq(inv("stop", bid), True, "stop by id matched the voice")
        bell = _find_voice(q("voices"), "bell")
        assert_eq(bell["finished"], True, "stopped voice marked finished, not yet reaped")
        assert_eq(q("voice_count"), 2, "still counted until a render reaps it")
        assert_eq(render(1), 1, "render step-verb returns the block count")
        assert_eq(q("voice_count"), 1, "render reaped the finished voice over the wire")
        inv("stop_all", None)
        render(1)
        assert_eq(q("voice_count"), 0, "stop_all + render silenced the engine")

        # ── the `send` wire: a bare clip name plays it; `stop_all` is reserved.
        sid = inv("send", "bell")
        assert isinstance(sid, int), f"send plays by clip name: {sid!r}"
        assert_eq(q("voice_count"), 1, "send bell played (synchronous)")
        inv("send", "stop_all")
        render(1)
        assert_eq(q("voice_count"), 0, "send stop_all + render silenced")

        # ── every failure surfaces loudly over the wire.
        assert_rpc_error(lambda: inv("play", "nope"), data="InvokeRejected")
        assert_rpc_error(lambda: iv("master_gain", "loud"), data="InterveneTypeMismatch")
        assert_rpc_error(lambda: iv("voice_count", 3), data="ReadOnly")
        assert_rpc_error(lambda: iv("nonexistent", 3), data="UnknownIntervenePath")
        assert_rpc_error(lambda: iv("voice.999.gain", 1.0), data="UnknownIntervenePath")


if __name__ == "__main__":
    sys.exit(run_demo("hello_audio_engine", body))
