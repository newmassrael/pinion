#!/usr/bin/env python3
"""hello-audio-device — a REAL cpal callback thread + the RPC control thread,
concurrently, over the JSON-RPC wire (§5.54, §2 #2).

R1293's `hello_audio_rt.py` proved the real-time audio *control surface* over the
wire, but it pumped the mixer itself with a synchronous `render` step-verb: one
thread, no device, fully deterministic. That left one path unproven — the one
that actually ships:

    a free-running audio callback thread, clocked by the sound card, calling
    `AudioRenderer::render` while the RPC thread concurrently reads the lock-free
    `AudioSnapshot` and pushes commands over the rtrb ring.

This demo is that proof. `hello-audio-device` opens a REAL output device via cpal
(so a real callback thread runs underneath) and hosts the SHIPPING
`AudioControllerExternal` verbatim — there is no `render` verb here, and the demo
asserts its absence: the device clock is the pump.

Silent by construction: it opens ALSA's `snd-dummy` card — a timer-paced device
with real callbacks and no audible output (the audio analogue of lavapipe). The
demo asserts over the wire that it really is on that card, so a misconfiguration
cannot quietly route a test tone to the developer's speakers.

    sudo modprobe snd-dummy          # once; creates the silent "Dummy" card

ZERO-FLAKE without a step-verb: a free-running callback cannot be stepped, so
nothing here counts frames exactly. Every assertion polls with `wait_until` /
`wait_query` until the observed snapshot SETTLES (a voice becomes live; peak
rises above the floor; a stop silences it) — outcome-based and
wall-clock-independent, which is the [[zero-flake-policy]] definition. The
assertions are coarser than the step-verb's, not flakier.

What this proves, all over the wire:

  (A) the device is the SILENT one, and the callback thread is real and
      FREE-RUNNING — `frames_rendered` advances with no RPC driving it;
  (B) a command from the RPC thread reaches the LIVE callback — `play` makes a
      voice audible in the snapshot the audio thread publishes;
  (C) control mutates the RUNNING stream — `set_master_gain` silences and
      restores a voice that is already playing (the concurrency proof proper);
  (D) the lock-free per-voice snapshot reads correctly UNDER a live callback;
  (E) `stop_all` reaches the audio thread and the voice is reaped;
  (F) failures still surface loudly over the wire, and there is NO `render`
      step-verb — this binary is pumped by the device, not by RPC.

What it does NOT prove: "no data race, ever" (no test can). The lock-free
protocol itself is verified deterministically by the orchestrated cross-thread
tests in `crates/pinion-audio/tests/realtime_channel.rs`; this adds integration
confidence for the shipping configuration.

Run from the workspace root:
    sudo modprobe snd-dummy
    cargo build -p hello-audio-device --release
    python3 tools/demos/hello_audio_device.py

>= 30 assertions.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    assert_rpc_error,
    run_demo,
    wait_query,
    wait_until,
)

EXT = "/external"

# The silent virtual card. `hello-audio-device` resolves this substring against
# the host's device list, so it finds ALSA's full `hw:CARD=Dummy,DEV=0` name.
# If the card is absent the binary aborts LOUDLY (listing what it did find)
# rather than falling back to an audible default — so this demo can never
# silently "pass" by making noise on the speakers, nor by not running at all.
SILENT_CARD = "Dummy"

# Peak is the max |sample| of the last block the audio thread rendered. The tone
# is authored at 0.9, so "audible" clears this comfortably and "silent" is well
# under it — the gap is wide enough that no timing jitter can straddle it.
AUDIBLE = 0.1
SILENT = 0.01


def body() -> None:
    # Inherited by the child process (RpcSubprocess passes os.environ through).
    os.environ["PINION_AUDIO_DEVICE"] = SILENT_CARD

    with RpcSubprocess("hello-audio-device", request_timeout=12.0) as g:

        def q(name: str) -> Any:
            return g.query(f"{EXT}/{name}")

        def inv(verb: str, args: Any) -> Any:
            return g.invoke(f"{EXT}/{verb}", args)

        # ── (A) the SILENT card, and a real free-running callback thread. ──────
        device = q("device")
        assert SILENT_CARD.lower() in device.lower(), (
            f"must run on the silent {SILENT_CARD!r} card, not {device!r} — "
            "a real output device would make this demo audible"
        )
        assert q("sample_rate") > 0, "the device reports a real sample rate"
        assert q("channels") >= 1, "the device reports a real channel count"

        # Nothing has been asked of the audio thread, yet it is already running:
        # the DEVICE clocks it. This is the load-bearing difference from
        # hello-audio-rt, where frames_rendered stays 0 until `invoke render`.
        first = wait_until(
            lambda: q("frames_rendered") > 0,
            desc="the cpal callback fires with no RPC driving it",
        )
        assert first is True
        f1 = q("frames_rendered")
        # …and it keeps going, concurrently with our RPC reads.
        wait_until(
            lambda: q("frames_rendered") > f1,
            desc="frames_rendered keeps advancing (free-running clock)",
        )
        assert q("frames_rendered") > f1, "the callback thread is free-running"
        assert_eq(q("voice_count"), 0, "boots with no voice")
        assert_eq(q("rejected"), 0, "nothing rejected yet")
        assert_eq(q("stolen"), 0, "nothing stolen yet")

        # ── (B) an RPC command reaches the LIVE callback. ─────────────────────
        voice = inv("play", {"name": "tone", "looping": True})
        assert isinstance(voice, int) and voice > 0, f"minted a voice id: {voice!r}"
        # The audio thread applies the queued play on its next callback and
        # publishes it — we poll for that, we do not step it.
        wait_query(g, f"{EXT}/voice_count", 1, desc="the audio thread admitted the play")
        wait_until(
            lambda: q("peak") > AUDIBLE,
            desc="the running callback renders the voice's samples",
        )
        assert q("peak") > AUDIBLE, "the live stream carries the tone"

        # ── (C) control mutates the RUNNING stream (the concurrency proof). ───
        # The voice is already playing on the audio thread; this command crosses
        # the lock-free ring into a callback that is mid-flight.
        inv("set_master_gain", 0.0)
        wait_until(
            lambda: q("peak") < SILENT,
            desc="master gain 0 silences the ALREADY-PLAYING voice",
        )
        assert_eq(q("voice_count"), 1, "silenced, but still live (gain != stop)")

        inv("set_master_gain", 1.0)
        wait_until(
            lambda: q("peak") > AUDIBLE,
            desc="restoring the gain brings the live voice back",
        )

        # Per-voice gain crosses the same ring and reaches the same live voice.
        inv("set_voice_gain", {"id": voice, "gain": 0.0})
        wait_until(
            lambda: q("peak") < SILENT,
            desc="per-voice gain 0 silences the live voice",
        )
        inv("set_voice_gain", {"id": voice, "gain": 1.0})
        wait_until(lambda: q("peak") > AUDIBLE, desc="per-voice gain restored")

        # ── (D) the lock-free per-voice snapshot, read UNDER a live callback. ──
        voices = q("voices")
        assert_eq(len(voices), 1, "exactly one live voice")
        live = voices[0]
        assert_eq(live["id"], voice, "the id the play minted")
        assert_eq(live["label"], "tone", "joined to the control-thread label map")
        assert_eq(live["looping"], True, "authored as looping")
        # A looping voice keeps playing, so its cursor advances on the audio
        # thread — another read of live, concurrently-mutated state.
        pos = live["position_secs"]
        assert pos >= 0.0, f"position_secs is real: {pos!r}"
        wait_until(
            lambda: q("voices")[0]["position_secs"] > pos,
            desc="the live voice's cursor advances on the audio thread",
        )

        # ── (E) stop reaches the audio thread; the voice is reaped. ───────────
        inv("stop_all", None)
        wait_query(g, f"{EXT}/voice_count", 0, desc="stop_all reached the callback")
        wait_until(
            lambda: q("peak") < SILENT,
            desc="the stopped voice leaves silence behind",
        )
        assert_eq(q("voices"), [], "no live voice remains")
        # The device never stopped clocking through any of it.
        assert q("frames_rendered") > f1, "the callback ran throughout"

        # ── (F) loud failures, and NO step-verb (the device is the pump). ─────
        # hello-audio-rt has `invoke render N`; this binary must not — asserting
        # its absence is what proves the pump is the device, not the demo.
        assert_rpc_error(lambda: inv("render", 1), data="UnknownInvokePath")
        assert_rpc_error(lambda: inv("bogus", None), data="UnknownInvokePath")
        assert_rpc_error(lambda: inv("play", "nope"), data="InvokeRejected")
        assert_rpc_error(lambda: inv("set_master_gain", "loud"), data="InvokeTypeMismatch")
        # The device facts are declared but read-only: you change the output by
        # opening a different one, not by writing to the surface.
        assert_rpc_error(lambda: g.intervene(f"{EXT}/device", "speakers"), data="ReadOnly")
        assert_rpc_error(lambda: g.intervene(f"{EXT}/sample_rate", 48000), data="ReadOnly")
        assert_rpc_error(lambda: g.intervene(f"{EXT}/channels", 2), data="ReadOnly")
        # …and the inner RT surface stays read-via-query, write-via-invoke.
        assert_rpc_error(lambda: g.intervene(f"{EXT}/voice_count", 3), data="ReadOnly")
        assert_rpc_error(lambda: g.intervene(f"{EXT}/peak", 1.0), data="ReadOnly")
        assert_rpc_error(lambda: g.intervene(f"{EXT}/nonexistent", 3), data="UnknownIntervenePath")


if __name__ == "__main__":
    sys.exit(run_demo("hello_audio_device", body))
