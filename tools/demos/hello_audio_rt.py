#!/usr/bin/env python3
"""hello-audio-rt — the real-time device audio path (§5.54) over the JSON-RPC wire.

Drives `hello-audio-rt` — the shipping RT audio path (`AudioController` +
`AudioRenderer`, the model a real cpal device runs on, R1282) hosted as the
primary widget on the `pinion_shell` GUI shell — entirely over JSON-RPC 2.0.

This closes the one uncleared cost-dodge the R1290–R1292 audit registered
([[debt-audio-rpc-wire-proof]] / Finding A): every prior audio round proved the
RT surface only by **in-process crate tests calling the trait methods**, which
bypass the whole wire (the `/external/` path split, JSON marshalling, error-code
mapping) — exactly what §2 #2 exists to guarantee. Here the shipping path is
driven as real `scene/invoke` + `scene/query` + `scene/intervene` requests, so
§2 #2 (RPC = AI primary path) is proven for audio, not merely asserted.

The RT model is asynchronous — a queued command takes effect on the audio
thread's next render, which publishes the snapshot the control thread reads. On
real hardware the cpal callback pumps that render continuously; here the
deterministic **`render` step-verb** (`invoke render N`) pumps exactly N blocks
synchronously on the dispatch thread, so every assertion observes settled state
with no background thread and no sleeps ([[zero-flake-policy]]).

What this proves, all over the wire:

  (A) a `play` is genuinely queued — `voice_count` stays 0 until a `render`
      applies it and publishes the snapshot;
  (B) per-voice reads (`voices`: id / label / authored + effective gain+pan /
      position / distance) join the lock-free slots to the control-thread labels;
  (C) a 3D emitter MOVES over RPC (`set_voice_position`), and the listener /
      attenuation drive the mix (`set_listener` / `set_attenuation`);
  (D) master + per-voice gain/pan reach the mix (`peak` tracks them);
  (E) voice-pool starvation is introspectable — `rejected` (reject-newest) and
      `stolen` (steal-oldest, chosen over RPC via `set_voice_policy`);
  (F) every failure is surfaced *loudly* over the wire (Rejected / TypeMismatch /
      UnknownPath / ReadOnly), never a silent success.

Run from the workspace root:
    cargo build -p hello-audio-rt --release
    python3 tools/demos/hello_audio_rt.py

>= 30 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Callable

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    run_demo,
)

EXT = "/external"
# The binding's MAX_VOICES — small on purpose so the pool can starve.
MAX_VOICES = 4
# Interleaved-stereo samples per render block -> frames = SAMPLES / 2.
FRAMES_PER_BLOCK = 256


def _find_voice(voices: list[dict], label: str) -> dict:
    for v in voices:
        if v.get("label") == label:
            return v
    raise AssertionError(f"no live voice labelled {label!r} in {voices!r}")


def _expect_invoke_error(fn: Callable[[], Any], data: str) -> None:
    """Assert `fn()` raises a JSON-RPC error whose `error.data` is `data`.

    The dispatch layer maps every `InvokeError` / `InterveneError` variant to a
    `-32602 Invalid params` carrying the variant name in `error.data`, so this
    proves the failure travelled the *wire* with the right typed reason — not a
    silent success and not a generic transport error.
    """
    try:
        fn()
    except RpcError as exc:
        assert_eq(exc.code, -32602, f"{data}: JSON-RPC code")
        assert_eq(exc.data, data, "error.data variant")
        return
    raise AssertionError(f"expected RpcError with data={data!r}, but the call succeeded")


def body() -> None:
    with RpcSubprocess("hello-audio-rt", request_timeout=12.0) as g:
        def q(name: str) -> Any:
            return g.query(f"{EXT}/{name}")

        def inv(verb: str, args: Any) -> Any:
            return g.invoke(f"{EXT}/{verb}", args)

        def render(blocks: int) -> Any:
            return inv("render", blocks)

        # ── (A) boot baseline: nothing rendered, so the snapshot is all zero.
        assert_eq(q("voice_count"), 0, "boots silent")
        assert_eq(q("frames_rendered"), 0, "no block rendered yet")
        assert_eq(q("peak"), 0.0, "no signal yet")
        assert_eq(q("rejected"), 0, "no rejection yet")
        assert_eq(q("stolen"), 0, "no steal yet")
        assert_eq(q("voice_policy"), "reject_newest", "default full-pool policy")
        assert_eq(q("voices"), [], "no live voices")

        # ── (A) a play is queued, not applied, until a render pumps the audio thread.
        vid = inv("play", "bell")
        assert isinstance(vid, int) and vid >= 1, f"play mints a control-thread id: {vid!r}"
        assert_eq(q("voice_count"), 0, "queued play not yet applied (async)")
        assert_eq(render(1), 1, "render step-verb returns the block count")
        assert_eq(q("voice_count"), 1, "one render applies the queued play")
        assert_eq(q("frames_rendered"), FRAMES_PER_BLOCK, "one block advanced the clock")
        peak = q("peak")
        assert 0.5 < peak < 0.75, f"a centred full tone is audible: {peak}"

        # ── (B) per-voice detail reads (numeric slots joined to labels).
        voices = q("voices")
        assert_eq(len(voices), 1, "one live voice listed")
        bell = _find_voice(voices, "bell")
        assert_eq(bell["id"], vid, "the listed voice is the one we played")
        assert abs(bell["gain"] - 1.0) < 1e-3, f"authored gain 1.0: {bell['gain']}"
        assert abs(bell["effective_gain"] - 1.0) < 1e-3, "flat voice: effective == authored"
        assert "position" not in bell, "a flat voice omits its 3D position"

        # ── (C) a spatial play, then MOVE the emitter over the wire.
        wid = inv("play", {"name": "waves", "position": [4.0, 0.0, 0.0]})
        render(1)
        assert_eq(q("voice_count"), 2, "the spatial voice joined the mix")
        waves = _find_voice(q("voices"), "waves")
        assert_eq(waves["position"], [4.0, 0.0, 0.0], "emitter placed 4 units right")
        assert abs(waves["distance"] - 4.0) < 1e-3, f"distance 4: {waves['distance']}"
        assert abs(waves["effective_gain"] - 0.25) < 1e-2, (
            f"1/(1+3) distance attenuation: {waves['effective_gain']}"
        )
        # Move it onto the listener over RPC -> distance 1.
        inv("set_voice_position", {"id": wid, "position": [1.0, 0.0, 0.0]})
        render(1)
        waves = _find_voice(q("voices"), "waves")
        assert abs(waves["distance"] - 1.0) < 1e-3, (
            f"the emitter moved on the audio thread over the wire: {waves['distance']}"
        )
        # `null` un-spatialises back to a flat voice.
        inv("set_voice_position", {"id": wid, "position": None})
        render(1)
        waves = _find_voice(q("voices"), "waves")
        assert "distance" not in waves, "un-spatialised: flat again"

        # ── (D) master + per-voice gain/pan reach the mix (isolate one voice first).
        inv("stop_all", None)
        render(1)
        assert_eq(q("voice_count"), 0, "stop_all silenced the mix")
        assert_eq(q("voices"), [], "no live voices after stop_all")
        bid = inv("play", "bell")
        render(1)
        assert_eq(q("voice_count"), 1, "single voice for the gain test")
        inv("set_master_gain", 0.5)
        inv("set_voice_gain", {"id": bid, "gain": 0.5})
        render(1)
        peak = q("peak")
        # 0.9 sample x 0.7071 centre x 0.5 voice x 0.5 master ~= 0.159.
        assert abs(peak - 0.159) < 0.03, f"master + voice gain both reached the mix: {peak}"
        inv("set_voice_pan", {"id": bid, "pan": 1.0})
        render(1)
        bell = _find_voice(q("voices"), "bell")
        assert abs(bell["effective_pan"] - 1.0) < 1e-3, "pan hard-right reached the mix"
        # Restore unity master so later phases read clean.
        inv("set_master_gain", 1.0)
        render(1)

        # ── (C) listener + attenuation query/drive over the wire.
        inv("stop_all", None)
        render(1)
        listener = q("listener")
        assert_eq(listener["position"], [0.0, 0.0, 0.0], "listener at origin")
        assert_eq(listener["forward"], [0.0, 0.0, -1.0], "listener faces -Z")
        inv("set_listener", {"position": [3.0, 0.0, 0.0]})
        listener = q("listener")
        assert_eq(listener["position"], [3.0, 0.0, 0.0], "listener moved over RPC")
        assert_eq(listener["forward"], [0.0, 0.0, -1.0], "partial update kept forward")
        atten = q("attenuation")
        ref_before = atten["reference_distance"]
        inv("set_attenuation", {"rolloff": 0.0})
        atten = q("attenuation")
        assert abs(atten["rolloff"]) < 1e-9, "rolloff zeroed over RPC"
        assert_eq(atten["reference_distance"], ref_before, "partial update kept reference")
        # Restore the listener/attenuation for the clean phases below.
        inv("set_listener", {"position": [0.0, 0.0, 0.0]})
        inv("set_attenuation", {"rolloff": 1.0})
        render(1)

        # ── (E) one-shot finish: a 2-frame blip is reaped before the next publish.
        fr0 = q("frames_rendered")
        inv("play", "blip")
        render(1)
        assert_eq(q("voice_count"), 0, "a finished one-shot is not listed")
        assert_eq(q("frames_rendered"), fr0 + FRAMES_PER_BLOCK, "the block still advanced the clock")

        # ── (E) voice-pool starvation: rejection under the default reject-newest.
        inv("stop_all", None)
        render(1)
        assert_eq(q("voice_policy"), "reject_newest", "still the default policy")
        rej0 = q("rejected")
        stol0 = q("stolen")
        for _ in range(MAX_VOICES + 1):
            inv("play", "bell")
        render(1)
        assert_eq(q("voice_count"), MAX_VOICES, "the pool is bounded")
        assert_eq(q("rejected"), rej0 + 1, "the dropped play is introspectable over RPC")
        assert_eq(q("stolen"), stol0, "reject-newest steals nothing")

        # ── (E) switch to steal-oldest over RPC and prove the audio thread obeys.
        inv("stop_all", None)
        render(1)
        inv("set_voice_policy", "steal_oldest")
        assert_eq(q("voice_policy"), "steal_oldest", "policy switched over RPC")
        rej1 = q("rejected")
        stol1 = q("stolen")
        for _ in range(MAX_VOICES + 1):
            inv("play", "bell")
        render(1)
        assert_eq(q("voice_count"), MAX_VOICES, "the pool is still bounded")
        assert_eq(q("stolen"), stol1 + 1, "the overflow stole the oldest (RPC policy reached the thread)")
        assert_eq(q("rejected"), rej1, "steal-oldest rejects nothing")

        # ── (F) every failure surfaces loudly over the wire, never a silent success.
        _expect_invoke_error(lambda: inv("play", "nope"), "InvokeRejected")
        _expect_invoke_error(lambda: inv("bogus", None), "UnknownInvokePath")
        _expect_invoke_error(lambda: inv("set_master_gain", "loud"), "InvokeTypeMismatch")
        _expect_invoke_error(lambda: inv("set_voice_policy", "nonsense"), "InvokeRejected")
        _expect_invoke_error(lambda: render("two"), "InvokeTypeMismatch")
        # The RT surface is read-via-query, write-via-invoke: an intervene of a
        # declared read-only field is ReadOnly; an undeclared path is unknown.
        _expect_invoke_error(lambda: g.intervene(f"{EXT}/voice_count", 3), "ReadOnly")
        _expect_invoke_error(lambda: g.intervene(f"{EXT}/voices", []), "ReadOnly")
        _expect_invoke_error(
            lambda: g.intervene(f"{EXT}/listener", {"position": [1.0, 0.0, 0.0]}),
            "ReadOnly",
        )
        _expect_invoke_error(lambda: g.intervene(f"{EXT}/nonexistent", 3), "UnknownIntervenePath")


if __name__ == "__main__":
    sys.exit(run_demo("hello_audio_rt", body))
