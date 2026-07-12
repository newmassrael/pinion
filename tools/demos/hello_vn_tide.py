#!/usr/bin/env python3
"""hello-vn-tide — the-tide VN Stage 0 set-piece over the JSON-RPC wire.

Drives `hello-vn-tide` — the reusable VN runner from `pinion-narrative`
(`vn::VnExternal`: typewriter dialogue + a timed choice with a countdown),
hosted as the primary widget on the `pinion_shell` GUI shell — entirely over
JSON-RPC 2.0.

This is the §2 #2 wire proof for the **VN render axis**
(`claudedocs/the-tide-vn-renpy-parity-requirements.md` §7). The runner is a
retained structured-scene surface (the dialogue is queryable text, not opaque
paint — §2 #1 / #7), driven by a deterministic **`tick {ms}` step-verb** that
advances *logical* time read from the argument, so every assertion observes
settled state with no background thread and no sleeps ([[zero-flake-policy]]) —
the same choice `hello-audio-rt`'s `render` verb made.

What this proves, all over the wire:

  (A) the typewriter reveals a growing prefix as logical time advances
      (`tick` -> `revealed_chars`), and `advance` snaps a line to full then
      steps to the next;
  (B) a choice BRANCHES (R1296): choosing 돌아본다 sends the play-head to its
      goto target (step 3, the peril branch), not the next step — and the
      peril line's own goto jumps past the endure branch to the End;
  (C) TIMEOUT branches too — letting the countdown expire takes the default
      option's branch (돌아본다 -> step 3), with `timed_out` true;
  (D) a DIFFERENT choice reaches a DIFFERENT ending — 버틴다 branches to the
      endure step (5) and its own ending, proving choices actually matter;
  (E) every failure surfaces loudly over the wire (InvokeRejected /
      InvokeTypeMismatch / UnknownInvokePath / ReadOnly / UnknownIntervenePath),
      never a silent success;
  (F) save/load (R1297): the play-head round-trips over the wire (`save` blob ->
      mutate -> `load` restores it), AND a `save_slot` written to disk by one
      process is reloaded by a SEPARATE fresh process (`load_slot`) that lands on
      the exact mid-countdown checkpoint — a save that survives a full restart;
  (G) the sprite/background director (R1298 data + R1301 render): `set_background`
      / `show` / `move` / `set_sprite_source` / `hide` place the stage as queryable
      data projected to positioned `Scene::Image` nodes; the whole stage is part of
      the save state; and a screenshot readback proves it RENDERS real positioned
      pixels (background fills the stage, the 무녀 sprite paints over it at centre)
      — the fix for the pre-R1301 hollow render (invisible, unpositioned nodes).

The §5.20 intent emission on each transition (`vn.advanced` / `vn.timeout` /
`vn.chosen`) is proven by the crate unit tests in `vn::external`, not here:
those intents accumulate in the retained state-scene External and are not
drained by the paint-scene `scene/intents` poll ([[state-scene-vs-paint-scene-introspect]]),
so asserting them over this wire would be a false / flaky proof — the same
reason `hello-audio-rt` leaves `audio.play` intent coverage to its crate tests.

Run from the workspace root:
    cargo build -p hello-vn-tide --release
    python3 tools/demos/hello_vn_tide.py

>= 40 assertions.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    assert_pixel_eq,
    assert_rpc_error,
    find_by_tag,
    isolated_storage_dir,
    png_pixel,
    read_png_rgba8,
    run_demo,
)

EXT = "/external"
WORKSPACE = Path(__file__).resolve().parent.parent.parent


def _prove_pixels() -> None:
    """Prove the stage RENDERS positioned pixels — the R1301 fix for the
    pre-R1301 hollow render (invisible `h:0`, unpositioned `x:0` Image nodes).
    Screenshots the initial frame (`PINION_SCREENSHOT`, no RPC) and asserts the
    tide-flat background fills the stage and the 무녀 sprite paints its own
    colour at centre, layered OVER the background."""
    binary = WORKSPACE / "target" / "release" / "hello-vn-tide"
    out = Path(tempfile.mkdtemp(prefix="vn-px-")) / "vn.png"
    env = os.environ.copy()
    env["PINION_SCREENSHOT"] = str(out)
    cmd = (
        [str(binary)]
        if binary.exists()
        else ["cargo", "run", "-p", "hello-vn-tide", "--release", "--quiet"]
    )
    subprocess.run(cmd, cwd=WORKSPACE, env=env, capture_output=True, text=True, timeout=180.0)
    assert out.exists(), "the binary rendered a screenshot"
    img = read_png_rgba8(out)
    blue = (30, 60, 110, 255)   # tideflat.png background
    ochre = (200, 140, 60, 255)  # mudang.png sprite
    # Background fills the stage away from the centre sprite.
    assert_pixel_eq(png_pixel(img, 40, 120), blue, "the background renders", tolerance=4)
    # The 무녀 sprite paints its colour at centre, OVER the background — proving
    # a positioned, sized, layered render (not the collapsed hollow node).
    assert_pixel_eq(png_pixel(img, 400, 120), ochre, "the sprite renders positioned", tolerance=4)


def _prove_and_checkpoint(g: RpcSubprocess) -> dict[str, Any]:
    """Process 1: prove typewriter / pure save-load / branching / failures,
    then persist a distinctive mid-countdown checkpoint to an on-disk slot for
    the restart to reload. Returns the expected checkpoint state."""
    def q(name: str) -> Any:
        return g.query(f"{EXT}/{name}")

    def inv(verb: str, args: Any) -> Any:
        return g.invoke(f"{EXT}/{verb}", args)

    def tick(ms: int) -> Any:
        return inv("tick", ms)

    def walk_line() -> None:
        inv("advance", None)  # snap the typewriter to full
        inv("advance", None)  # step off the line

    # ── (A) boot: step 0 is a narration line, the opening background is up.
    assert_eq(q("mode"), "line", "boots on a line")
    assert_eq(q("step"), 0, "at the first step")
    assert_eq(q("step_count"), 7, "the branching set-piece has 7 steps")
    assert_eq(q("speaker"), "", "step 0 is narration (no speaker)")
    assert_eq(q("revealed_chars"), 0, "nothing revealed at t=0")
    assert (q("background") or "").endswith("tideflat.png"), "the opening background is a real asset"
    assert_eq(q("sprite_count"), 1, "the opening 무녀 sprite is on stage")

    line_len = q("line_len")
    assert isinstance(line_len, int) and line_len > 8, f"a real line: {line_len}"

    # ── (G) the stage director: place / move / re-express / hide a sprite, all
    # as queryable data, and prove the PAINT scene carries it as an Image node
    # with a REAL rect (positioned + sized, not the collapsed w:800 h:0 the
    # pre-R1301 hollow render produced) — §2 #1 / #7. (Actual bitmap pixels are
    # proven separately by `_prove_pixels` via screenshot readback.)
    snap = g.snapshot(source="paint")
    bg = find_by_tag(snap, "vn.background")
    assert bg is not None and bg["rect"]["w"] == 800 and bg["rect"]["h"] == 240, (
        f"background image node fills the stage: {bg!r}"
    )
    mudang = find_by_tag(snap, "vn.sprite.mudang")
    assert mudang is not None, "the opening sprite is an Image node"
    assert mudang["rect"]["h"] == 200 and mudang["rect"]["w"] == 160, (
        f"the sprite has a real size (not h:0): {mudang['rect']!r}"
    )
    mid_x = mudang["rect"]["x"]

    # Direct a SECOND sprite over the wire (the opening 무녀 stays put).
    inv("show", {"id": "guest", "source": "guest_calm", "at": "right", "layer": 2})
    assert_eq(q("sprite_count"), 2, "the directed sprite joined the stage")
    snap = g.snapshot(source="paint")
    guest = find_by_tag(snap, "vn.sprite.guest")
    assert guest is not None and guest["rect"]["x"] > mid_x, (
        f"the 'right' sprite paints further right than centre: {guest['rect']!r} vs {mid_x}"
    )
    # move + expression swap by id, then hide — all queryable.
    inv("move", {"id": "guest", "at": "left"})
    inv("set_sprite_source", {"id": "guest", "source": "guest_grave"})
    guest_data = next(s for s in q("sprites") if s["id"] == "guest")
    assert_eq(guest_data["at"], "left", "the sprite moved over the wire")
    assert_eq(guest_data["source"], "guest_grave", "the expression swapped")
    inv("hide", "guest")
    assert_eq(q("sprite_count"), 1, "hide removed the directed sprite; 무녀 remains")

    # ── (A) the typewriter reveals a growing prefix (40 cps -> 200 ms = 8).
    tick(200)
    assert_eq(q("revealed_chars"), 8, "200 ms revealed 8 chars at 40 cps")
    assert q("revealed_chars") < line_len, "still mid-reveal"
    inv("advance", None)  # snap the partly-revealed line to full
    assert_eq(q("fully_revealed"), True, "advance snapped the line to full")
    inv("advance", None)  # step onto the 무녀 line
    assert_eq(q("step"), 1, "advanced onto step 1")
    assert_eq(q("speaker"), "무녀", "step 1 has a speaker")

    # ── (SL) pure save/load round-trip over the wire: snapshot, move on, restore.
    # The blob now carries BOTH the play-head and the stage (VnSave).
    blob = inv("save", None)
    assert blob["runtime"]["step"] == 1, f"save carries the play-head: {blob!r}"
    assert len(blob["stage"]["sprites"]) == 1, "save carries the stage too"
    walk_line()  # move off the saved state
    assert_eq(q("mode"), "choice", "moved off the saved line")
    inv("load", blob)
    assert_eq(q("step"), 1, "load restored the saved step")
    assert_eq(q("speaker"), "무녀", "and the saved line")
    walk_line()  # now walk to the choice for real

    # ── the one decisive timed choice, and its two branch targets.
    assert_eq(q("mode"), "choice", "reached the timed choice")
    assert_eq(q("step"), 2, "on step 2")
    assert_eq(q("timeout_ms"), 4000, "4 s to answer")
    assert_eq(q("remaining_ms"), 4000, "full countdown on entry")
    options = q("options")
    assert_eq([o["label"] for o in options], ["돌아본다", "버틴다"], "options over the wire")
    # The branch targets are authored data, queryable over the wire.
    assert_eq(options[0]["goto"], 3, "돌아본다 branches to the peril step")
    assert_eq(options[1]["goto"], 5, "버틴다 branches to the endure step")

    # ── (B) PATH A — choose 돌아본다: branch to the peril step 3, not step+1.
    inv("choose", 0)
    assert_eq(q("step"), 3, "the choice BRANCHED to its goto target (3), not 2+1")
    assert_eq(q("outcome"), "turn", "the chosen outcome")
    assert_eq(q("timed_out"), False, "the player chose in time")
    walk_line()  # step 3 peril narration -> step 4 peril ending line
    assert_eq(q("step"), 4, "on the peril ending line")
    assert "나쁜 끝" in q("line"), "the peril ending text"
    walk_line()  # the peril line's with_goto jumps past the endure branch to End
    assert_eq(q("mode"), "end", "peril branch reached the End")
    assert_eq(q("step"), 7, "line goto skipped the endure branch (5,6)")

    # ── (C) PATH timeout — scrub back onto the choice, let it expire.
    g.intervene(f"{EXT}/step", 2)
    assert_eq(q("remaining_ms"), 4000, "the scrub reset the countdown")
    tick(4500)  # cross the 4 s timeout
    assert_eq(q("step"), 3, "timeout took the DEFAULT option's branch (돌아본다 -> 3)")
    assert_eq(q("timed_out"), True, "resolved by the countdown, not the player")

    # ── (D) PATH B — a different choice reaches a DIFFERENT branch/ending.
    g.intervene(f"{EXT}/step", 2)
    inv("choose", 1)  # 버틴다 -> goto 5
    assert_eq(q("step"), 5, "버틴다 branched to the endure step, not the peril one")
    assert_eq(q("outcome"), "endure", "the other outcome")
    walk_line()  # step 5 endure narration -> step 6 endure ending line
    assert_eq(q("step"), 6, "on the endure ending line")
    assert "버틴 끝" in q("line"), "the endure ending text (a different ending)"
    walk_line()  # off the last step into the End
    assert_eq(q("mode"), "end", "endure branch reached the End")

    # ── the whole script (branch targets included) is queryable as data.
    script = q("script")
    assert_eq(len(script["steps"]), 7, "the authored script is introspectable")
    assert_eq(script["steps"][4]["goto"], 7, "the peril line carries its End jump")

    # ── (E) every failure surfaces loudly over the wire, never a silent success.
    assert_rpc_error(lambda: inv("choose", 0), data="InvokeRejected")  # at End, not a choice
    assert_rpc_error(lambda: tick("soon"), data="InvokeTypeMismatch")
    assert_rpc_error(lambda: inv("bogus", None), data="UnknownInvokePath")
    assert_rpc_error(lambda: inv("load", 3), data="InvokeTypeMismatch")
    assert_rpc_error(lambda: g.intervene(f"{EXT}/mode", "x"), data="ReadOnly")
    assert_rpc_error(lambda: g.intervene(f"{EXT}/nonexistent", 3), data="UnknownIntervenePath")
    g.intervene(f"{EXT}/step", 2)
    assert_rpc_error(lambda: inv("choose", 9), data="InvokeRejected")

    # ── (F) persist a distinctive checkpoint — mid-countdown dialogue AND a
    # non-default stage (night background + the 무녀 sprite still shown) — to an
    # on-disk slot, so the restart proves BOTH survived.
    inv("set_background", "tideflat_night")  # differs from the boot background
    g.intervene(f"{EXT}/step", 2)  # onto the choice
    tick(1500)  # remaining 2500 ms — a state no fresh boot could be in
    checkpoint = {
        "step": q("step"),
        "mode": q("mode"),
        "remaining_ms": q("remaining_ms"),
        "background": q("background"),
        "sprite_count": q("sprite_count"),
    }
    assert_eq(checkpoint["remaining_ms"], 2500, "checkpoint is mid-countdown")
    assert_eq(checkpoint["sprite_count"], 1, "the 무녀 sprite is on the checkpoint stage")
    inv("save_slot", "checkpoint")  # write it to the storage file
    return checkpoint


def _prove_reload(g: RpcSubprocess, checkpoint: dict[str, Any]) -> None:
    """Process 2: a FRESH binary boots at the start, then reloads the on-disk
    slot the previous process wrote and lands exactly on the checkpoint —
    proving the save (dialogue AND stage) survived a full process restart, not
    just an in-memory round-trip."""
    def q(name: str) -> Any:
        return g.query(f"{EXT}/{name}")

    # A brand-new process has its own play-head at the start and an empty stage
    # apart from the binding's opening background.
    assert_eq(q("step"), 0, "a fresh process boots at the start")
    assert_eq(q("mode"), "line", "fresh boot is on the first line")
    assert (q("background") or "").endswith("tideflat.png"), "fresh boot has the DEFAULT background"
    assert_eq(q("sprite_count"), 1, "fresh boot has only the opening 무녀 sprite")
    # Loading a slot that was never written is Rejected (well-typed, nothing there).
    assert_rpc_error(lambda: g.invoke(f"{EXT}/load_slot", "nope"), data="InvokeRejected")
    # The checkpoint the previous process saved reloads from disk — dialogue AND stage.
    g.invoke(f"{EXT}/load_slot", "checkpoint")
    assert_eq(q("step"), checkpoint["step"], "the saved step survived the restart")
    assert_eq(q("mode"), checkpoint["mode"], "the saved mode restored (on the choice)")
    assert_eq(
        q("remaining_ms"),
        checkpoint["remaining_ms"],
        "the exact countdown position survived the restart",
    )
    assert_eq(q("background"), "tideflat_night", "the saved STAGE background survived the restart")
    assert_eq(q("sprite_count"), 1, "the saved sprite came back off disk")


def body() -> None:
    # First: prove the stage renders real positioned pixels (screenshot readback).
    _prove_pixels()
    # Hermetic on-disk save slots: a fresh tempdir via PINION_STORAGE_DIR,
    # inherited by both subprocesses and cleaned up on exit.
    with isolated_storage_dir("vn_tide_save"):
        with RpcSubprocess("hello-vn-tide", request_timeout=12.0) as g:
            checkpoint = _prove_and_checkpoint(g)
        # A SEPARATE process — the save must come off disk, not shared memory.
        with RpcSubprocess("hello-vn-tide", request_timeout=12.0) as g:
            _prove_reload(g, checkpoint)


if __name__ == "__main__":
    sys.exit(run_demo("hello_vn_tide", body))
