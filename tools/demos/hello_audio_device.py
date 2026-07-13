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
  (E) POOL ADMISSION under a live callback — the one behaviour `AudioEngine`'s
      own docs call "timing-dependent (which plays win the budget depends on how
      commands batch into callbacks)", and therefore the one a single-threaded
      test cannot pin down: over-subscribe the pool and `rejected` rises; switch
      to steal-oldest over the wire and `stolen` rises; the bound always holds;
  (F) `stop_all` reaches the audio thread and every voice is reaped;
  (G) the compile-time-composed `$schema` is what the WIRE actually announces;
  (H) failures still surface loudly, and there is NO `render` step-verb — this
      binary is pumped by the device, not by RPC.

What it does NOT prove:
  - "no data race, ever" — no test can. The lock-free protocol itself is verified
    deterministically by the orchestrated cross-thread tests in
    `crates/pinion-audio/tests/realtime_channel.rs`; this adds integration
    confidence for the shipping configuration.
  - that the SAMPLES REACHING THE CARD are correct. `peak` is measured on the
    stereo mix BEFORE `write_frames` converts it to the device's format and
    channel layout, so no read here can see a conversion bug. That conversion is
    covered by unit tests in `crates/pinion-audio/src/device.rs` instead; a
    loopback capture (`snd-aloop`) that verifies the bytes the card received is a
    further step this demo does not take.

Run from the workspace root:
    sudo modprobe snd-dummy
    cargo build -p hello-audio-device --release
    python3 tools/demos/hello_audio_device.py

>= 40 assertions.
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

# The silent virtual card, named EXACTLY. `hello-audio-device` refuses an
# ambiguous request rather than guessing — and it must: on a typical Linux box
# "Dummy" matches several PCMs, and a bare "hw" matches the REAL sound card
# first, so a "first match wins" policy would route this test's tone out of the
# speakers. If the card is absent the binary aborts LOUDLY (listing what it did
# find) rather than falling back to an audible default, so this demo can never
# silently "pass" by making noise, nor by not running at all.
SILENT_CARD = "hw:CARD=Dummy,DEV=0"
# What the opened device's name must contain — the card, whatever PCM prefix.
SILENT_CARD_MARK = "Dummy"
# A SECOND PCM on the same silent card, so `set_device` can be proven over the
# wire without any risk of the test ever becoming audible.
SECOND_SILENT_PCM = "plughw:CARD=Dummy,DEV=0"

# Peak is the max |sample| of the last block the audio thread rendered. The tone
# is authored at 0.9, so "audible" clears this comfortably and "silent" is well
# under it — the gap is wide enough that no timing jitter can straddle it.
AUDIBLE = 0.1
SILENT = 0.01

# The binding's voice-pool bound (MAX_VOICES in its main.rs). Asking for more
# live voices than this must be REFUSED, not silently over-allocated.
MAX_VOICES = 8


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
        assert SILENT_CARD_MARK.lower() in device.lower(), (
            f"must run on the silent {SILENT_CARD_MARK!r} card, not {device!r} — "
            "a real output device would make this demo audible"
        )
        assert q("sample_rate") > 0, "the device reports a real sample rate"
        assert q("channels") >= 1, "the device reports a real channel count"
        # Which conversion branch is live (f32 / i16 / u16). Otherwise invisible:
        # the mix is f32 whatever the card wants, so nothing else reveals the path
        # the samples took — and CI would not record which one it exercised.
        assert q("sample_format") in ("f32", "i16", "u16"), q("sample_format")
        # The output is healthy. A dead device just stops calling back, so without
        # this reading "idle" and "the card is gone" look identical on the wire.
        assert_eq(q("stream_errors"), 0, "the stream has not faulted")
        # Where sound COULD go, not just where it is going — the list a settings
        # panel is built from. The opened device must be one of them.
        devices = q("devices")
        assert isinstance(devices, list) and devices, f"an output list: {devices!r}"
        assert device in devices, f"the open device {device!r} is not in {devices!r}"

        # Nothing has been asked of the audio thread, yet it is already running:
        # the DEVICE clocks it. This is the load-bearing difference from
        # hello-audio-rt, where frames_rendered stays 0 until `invoke render`.
        wait_until(
            lambda: q("frames_rendered") > 0,
            desc="the cpal callback fires with no RPC driving it",
        )
        f1 = q("frames_rendered")
        # …and it keeps going, concurrently with our RPC reads. (`wait_until`
        # raises on timeout, so reaching the next line IS the assertion.)
        wait_until(
            lambda: q("frames_rendered") > f1,
            desc="frames_rendered keeps advancing (free-running clock)",
        )
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
        wait_until(
            lambda: q("voices")[0]["position_secs"] > pos,
            desc="the live voice's cursor advances on the audio thread",
        )

        # ── (E) POOL ADMISSION under a live callback. ─────────────────────────
        # `AudioEngine::apply`'s own docs say admission at the pool bound is
        # "timing-dependent (which plays win the budget depends on how commands
        # batch into callbacks)" — so it is precisely the behaviour that a
        # single-threaded test CANNOT pin down, and the reason this demo exists.
        # One voice is live; ask for MAX_VOICES more so the pool must refuse.
        for i in range(MAX_VOICES):
            inv("play", {"name": "bell", "looping": True})
        # Outcome-based, so batching cannot make it flaky: the pool is bounded, so
        # SOME play must be refused, and the live count can never exceed the bound.
        wait_until(
            lambda: q("rejected") >= 1,
            desc="the audio thread refuses plays past the voice budget",
        )
        assert q("voice_count") <= MAX_VOICES, (
            f"the pool bound is not a suggestion: {q('voice_count')} > {MAX_VOICES}"
        )
        assert_eq(q("stolen"), 0, "reject-newest is the default: nothing stolen")

        # Switch the policy over the wire and the SAME saturated pool now steals.
        inv("set_voice_policy", "steal_oldest")
        wait_query(g, f"{EXT}/voice_policy", "steal_oldest", desc="policy took")
        inv("play", {"name": "bell", "looping": True})
        wait_until(
            lambda: q("stolen") >= 1,
            desc="steal-oldest evicts a live voice to admit the newcomer",
        )
        assert q("voice_count") <= MAX_VOICES, "stealing keeps the pool bounded"

        # ── (F) stop reaches the audio thread; every voice is reaped. ─────────
        inv("stop_all", None)
        wait_query(g, f"{EXT}/voice_count", 0, desc="stop_all reached the callback")
        wait_until(
            lambda: q("peak") < SILENT,
            desc="the stopped voices leave silence behind",
        )
        # Poll, don't snap: `voice_count` is Relaxed while the per-voice slot ids
        # are Release/Acquire, so a zero count does not formally order against a
        # still-stale slot. Polling for the settled list costs nothing and closes
        # the gap that a bare assert would leave open.
        wait_until(lambda: q("voices") == [], desc="no live voice remains")
        # The device never stopped clocking through any of it.
        assert q("frames_rendered") > f1, "the callback ran throughout"

        # ── (G) THE PER-FRAME CONTROL-SIDE AUDIO TICK. ───────────────────────
        # `set_camera` moves the WORLD only — it never touches the audio engine.
        # The listener follows solely because `AudioFollowClock`, a retained
        # Tickable registered with the shell's animation driver, carries the pose
        # across on the next paint. So `listener.position` catching up to `camera`
        # is a proof that the per-frame tick ran and pushed over the lock-free
        # ring — which is what makes "per-frame audio needs Phase C" false.
        assert_eq(q("camera"), [0.0, 0.0, 0.0], "camera starts at the origin")
        listener = q("listener")
        assert_eq(listener["position"], [0.0, 0.0, 0.0], "listener starts there too")

        inv("set_camera", [3.0, 0.0, -4.0])
        assert_eq(q("camera"), [3.0, 0.0, -4.0], "the world moved immediately")
        # The audio engine has NOT been told yet — only a frame can do that.
        wait_until(
            lambda: q("listener")["position"] == [3.0, 0.0, -4.0],
            desc="the per-frame clock carries the camera pose to the listener",
        )
        # Orientation is preserved: the clock moves where the ears ARE, not
        # where they look.
        assert_eq(q("listener")["forward"], listener["forward"], "facing kept")
        assert_eq(q("listener")["up"], listener["up"], "up kept")

        # …and the pose reached the AUDIO THREAD, not merely the control-thread
        # mirror. `query listener` reads the mirror, which advances on ENQUEUE — so
        # on its own it proves only "the command was queued". `voices[].distance`
        # is published BY the callback from its own engine, so polling it proves
        # the audio thread actually consumed the pose and re-spatialised the mix.
        emitter = inv("play", {"name": "bell", "looping": True, "position": [3.0, 0.0, 6.0]})
        assert isinstance(emitter, int)
        wait_until(lambda: len(q("voices")) >= 1, desc="the emitter is live")

        def emitter_distance():
            for v in q("voices"):
                if v["id"] == emitter:
                    return v.get("distance")
            return None

        # Listener at [3,0,-4], emitter at [3,0,6] -> 10 units apart.
        wait_until(
            lambda: emitter_distance() is not None and abs(emitter_distance() - 10.0) < 0.01,
            desc="the audio thread spatialises the emitter against the carried pose",
        )
        # Walk the camera onto the emitter; the AUDIO THREAD must see distance -> 0.
        inv("set_camera", [3.0, 0.0, 6.0])
        wait_until(
            lambda: emitter_distance() is not None and emitter_distance() < 0.01,
            desc="a frame carried the new pose all the way into the MIX",
        )

        # ★ And the EMITTER moves too — the operation a game performs hundreds of
        # times a frame (one listener, many sounds). `set_emitter` writes the WORLD
        # only; the frame carries it, exactly like the camera. A direct
        # `set_voice_position` is REFUSED here, because on this binding the world
        # owns all spatial state — two unarbitrated writers is the bug that shipped
        # once already.
        assert_rpc_error(
            lambda: inv("set_voice_position", {"id": emitter, "position": [0.0, 0.0, 0.0]}),
            data="InvokeRejected",
        )
        inv("set_emitter", {"id": emitter, "position": [3.0, 0.0, 14.0]})
        wait_until(
            lambda: emitter_distance() is not None and abs(emitter_distance() - 8.0) < 0.01,
            desc="a frame carried the EMITTER pose into the mix",
        )
        # `null` un-spatialises it back to its authored pan.
        inv("set_emitter", {"id": emitter, "position": None})
        wait_until(
            lambda: emitter_distance() is None,
            desc="a frame carried the un-spatialise across",
        )
        inv("stop", emitter)
        wait_query(g, f"{EXT}/voice_count", 0, desc="emitter stopped")

        # ★ Re-asserting the SAME pose must still be serviced. `Signal::set`
        # equality-skips, so before the write-sequence stamp this armed no repaint
        # while latching the dirty flag: the push was stranded and the audio thread
        # never heard it. Drive the listener away first so a successful re-assert is
        # observable — and note `set_listener` itself is REFUSED here, because on
        # this binding the world owns the listener (two unarbitrated writers was the
        # other half of that bug).
        assert_rpc_error(
            lambda: inv("set_listener", {"position": [9.0, 9.0, 9.0]}),
            data="InvokeRejected",
        )
        inv("set_camera", [3.0, 0.0, 6.0])  # the pose it already holds
        wait_until(
            lambda: q("listener")["position"] == [3.0, 0.0, 6.0],
            desc="a re-asserted, unchanged pose is still carried (not equality-skipped)",
        )

        # It settles (is_at_rest) and re-arms on the next move — so the frame loop
        # is not pinned awake, and a second move still lands.
        inv("set_camera", [-1.0, 2.0, 0.0])
        wait_until(
            lambda: q("listener")["position"] == [-1.0, 2.0, 0.0],
            desc="a second camera move lands after the clock went back to rest",
        )
        # The per-frame sync is observable — declared, and non-zero because it ran.
        assert q("frame_ticks") >= 1, "the audio sync has run"

        # ── (H) SWITCH THE DEVICE over RPC. ──────────────────────────────────
        # `devices` used to tell an agent where sound COULD go while giving it no
        # way to go there — selection was possible only via an environment
        # variable, so a human with a shell could do what the AI's §2 #2 *primary*
        # path could not. Both PCMs below are the SILENT dummy card, so this can
        # never become audible.
        assert SECOND_SILENT_PCM in q("devices"), q("devices")
        assert_eq(q("device"), SILENT_CARD, "still on the first PCM")
        inv("set_device", SECOND_SILENT_PCM)
        wait_query(g, f"{EXT}/device", SECOND_SILENT_PCM, desc="the output switched")
        # A REAL device, freshly opened: its callback thread is running.
        wait_until(
            lambda: q("frames_rendered") > 0,
            desc="the new device's callback thread is live",
        )
        assert q("sample_rate") > 0 and q("channels") >= 1
        assert_eq(q("stream_errors"), 0, "the new stream is healthy")
        # The world was re-pushed onto the new engine — the listener is NOT lost.
        wait_until(
            lambda: q("listener")["position"] == [-1.0, 2.0, 0.0],
            desc="the camera pose is carried onto the new device",
        )
        # And the clip library survived the switch (it is Arc'd and resampled —
        # the thing the old docstring claimed had to be "rebuilt").
        inv("play", {"name": "tone", "looping": True})
        wait_query(g, f"{EXT}/voice_count", 1, desc="clips still play after a switch")
        wait_until(lambda: q("peak") > AUDIBLE, desc="the new device renders them")
        inv("stop_all", None)
        wait_query(g, f"{EXT}/voice_count", 0, desc="stopped")

        # A bad name is REFUSED and the current device keeps playing — a typo must
        # never leave the app silent.
        assert_rpc_error(lambda: inv("set_device", "no::such::device"), data="InvokeRejected")
        assert_eq(q("device"), SECOND_SILENT_PCM, "still on the working device")
        assert_eq(q("stream_errors"), 0, "and it is still healthy")

        # ── (I) the composed schema, ON THE WIRE — not just in a unit test. ────
        # This binding's schema is the RT surface's fields + its device fields,
        # composed at compile time. Proving that in Rust only would be an
        # in-process check of exactly the kind §2 #2 exists to replace — so read
        # it back over the wire, the way an AI agent discovers the contract.
        paths = [f["path"] for f in g.query(f"{EXT}/$schema")]
        for field in ("voice_count", "peak", "frames_rendered", "voices", "voice_policy"):
            assert field in paths, f"the RT surface's {field!r} must be declared: {paths}"
        for field in ("device", "devices", "sample_rate", "channels", "sample_format",
                      "stream_errors", "camera", "frame_ticks"):
            assert field in paths, f"this binding's {field!r} must be declared: {paths}"
        # Composed, so no name may be announced twice (a duplicate would mean the
        # device half silently shadows an RT read).
        assert len(paths) == len(set(paths)), f"$schema announces a field twice: {paths}"

        # ── (J) loud failures, and NO step-verb (the device is the pump). ─────
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
        assert_rpc_error(lambda: g.intervene(f"{EXT}/devices", []), data="ReadOnly")
        assert_rpc_error(lambda: g.intervene(f"{EXT}/stream_errors", 0), data="ReadOnly")
        # Declared => ReadOnly. This one used to answer `query` while `intervene`
        # called it UnknownPath — three answers for one path.
        assert_rpc_error(lambda: g.intervene(f"{EXT}/frame_ticks", 0), data="ReadOnly")
        assert_rpc_error(lambda: g.intervene(f"{EXT}/camera", []), data="ReadOnly")
        # …and the inner RT surface stays read-via-query, write-via-invoke.
        assert_rpc_error(lambda: g.intervene(f"{EXT}/voice_count", 3), data="ReadOnly")
        assert_rpc_error(lambda: g.intervene(f"{EXT}/peak", 1.0), data="ReadOnly")
        assert_rpc_error(lambda: g.intervene(f"{EXT}/nonexistent", 3), data="UnknownIntervenePath")


if __name__ == "__main__":
    sys.exit(run_demo("hello_audio_device", body))
