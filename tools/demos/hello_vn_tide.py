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
  (B) a timed choice runs a countdown (`remaining_ms`), and letting it expire
      auto-resolves to the authored default option (`outcome` / `timed_out`);
  (C) a choice answered in time records the player's option
      (`choose <index>` -> `outcome` / `outcome_label`), not the default;
  (D) every failure surfaces loudly over the wire (InvokeRejected /
      InvokeTypeMismatch / UnknownInvokePath / ReadOnly / UnknownIntervenePath),
      never a silent success.

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

EXT = "/external"


def body() -> None:
    with RpcSubprocess("hello-vn-tide", request_timeout=12.0) as g:
        def q(name: str) -> Any:
            return g.query(f"{EXT}/{name}")

        def inv(verb: str, args: Any) -> Any:
            return g.invoke(f"{EXT}/{verb}", args)

        def tick(ms: int) -> Any:
            return inv("tick", ms)

        # ── (A) boot: step 0 is a narration line, nothing revealed yet.
        assert_eq(q("mode"), "line", "boots on a line")
        assert_eq(q("step"), 0, "at the first step")
        assert_eq(q("step_count"), 6, "the tide set-piece has 6 steps")
        assert_eq(q("speaker"), "", "step 0 is narration (no speaker)")
        assert_eq(q("revealed_chars"), 0, "nothing revealed at t=0")
        line_len = q("line_len")
        assert isinstance(line_len, int) and line_len > 8, f"a real line: {line_len}"
        assert_eq(q("fully_revealed"), False, "not yet fully revealed")

        # ── (A) the typewriter reveals a growing prefix (40 cps -> 200 ms = 8).
        tick(200)
        assert_eq(q("revealed_chars"), 8, "200 ms revealed 8 chars at 40 cps")
        assert q("revealed_chars") < line_len, "still mid-reveal"
        # advance() snaps the partly-revealed line to full ...
        inv("advance", None)
        assert_eq(q("fully_revealed"), True, "advance snapped the line to full")
        assert_eq(q("revealed_chars"), line_len, "the whole line is shown")
        # ... and a second advance steps to the next step (the 무녀 line).
        inv("advance", None)
        assert_eq(q("step"), 1, "advanced onto step 1")
        assert_eq(q("speaker"), "무녀", "step 1 has a speaker")

        # Walk the 무녀 line through to the first timed choice.
        inv("advance", None)  # snap
        inv("advance", None)  # step -> choice A
        assert_eq(q("mode"), "choice", "reached the first timed choice")
        assert_eq(q("step"), 2, "on step 2")

        # ── (B) the countdown: query it, then let it expire to the default.
        assert_eq(q("prompt"), "다시, 네 이름이 불린다 — 어쩔 텐가?", "choice A prompt")
        assert_eq(q("option_count"), 2, "two options")
        assert_eq(q("timeout_ms"), 4000, "4 s to answer")
        assert_eq(q("remaining_ms"), 4000, "full countdown on entry")
        assert_eq(q("expired"), False, "not expired yet")
        options = q("options")
        assert_eq([o["label"] for o in options], ["돌아본다", "버틴다"], "options over the wire")
        tick(2000)
        assert_eq(q("remaining_ms"), 2000, "half the countdown drained")
        assert_eq(q("expired"), False, "still live")
        # The tick that crosses the timeout auto-resolves to default_option (버틴다).
        tick(3000)
        assert_eq(q("mode"), "line", "the choice resolved and stepped past")
        assert_eq(q("step"), 3, "onto the consequence line")
        assert_eq(q("outcome"), "endure", "timed out to the default option")
        assert_eq(q("outcome_label"), "버틴다", "the default option's label")
        assert_eq(q("timed_out"), True, "resolved by the countdown, not the player")

        # Walk the consequence narration through to the second timed choice.
        inv("advance", None)  # snap
        inv("advance", None)  # step -> choice B
        assert_eq(q("mode"), "choice", "reached the second timed choice")
        assert_eq(q("step"), 4, "on step 4")
        assert_eq(q("timeout_ms"), 3000, "3 s on choice B")

        # ── (C) answer in time: choose the player's option, not the default.
        tick(1000)
        assert_eq(q("remaining_ms"), 2000, "one second gone, two left")
        inv("choose", 1)  # 잡는다 / take_hand (default would be 뿌리친다 / shake_off)
        assert_eq(q("outcome"), "take_hand", "the player's chosen outcome")
        assert_eq(q("outcome_label"), "잡는다", "the chosen option's label")
        assert_eq(q("timed_out"), False, "the player chose in time")
        assert_eq(q("step"), 5, "stepped past choice B")

        # ── walk the final narration to the End.
        inv("advance", None)  # snap
        inv("advance", None)  # step -> End
        assert_eq(q("mode"), "end", "the set-piece finished")
        assert_eq(q("step"), 6, "play-head past the last step")
        # The last resolution is still readable at End.
        assert_eq(q("outcome"), "take_hand", "the ending outcome persists")

        # ── (A') the whole script is queryable as data (AI reads ahead / back).
        script = q("script")
        assert_eq(len(script["steps"]), 6, "the authored script is introspectable")
        assert_eq(script["steps"][0]["kind"], "line", "step 0 is a line")
        assert_eq(script["steps"][2]["kind"], "timed_choice", "step 2 is a choice")

        # ── (D) every failure surfaces loudly over the wire, never a silent success.
        assert_rpc_error(lambda: inv("choose", 0), data="InvokeRejected")  # at End, not a choice
        assert_rpc_error(lambda: tick("soon"), data="InvokeTypeMismatch")
        assert_rpc_error(lambda: inv("bogus", None), data="UnknownInvokePath")
        assert_rpc_error(lambda: g.intervene(f"{EXT}/mode", "x"), data="ReadOnly")
        assert_rpc_error(lambda: g.intervene(f"{EXT}/nonexistent", 3), data="UnknownIntervenePath")

        # ── (C') scrub the play-head back onto a choice via intervene, then an
        # out-of-range choose is Rejected (well-typed args, refused action).
        g.intervene(f"{EXT}/step", 4)
        assert_eq(q("mode"), "choice", "scrubbed back onto choice B")
        assert_eq(q("remaining_ms"), 3000, "scrub reset the countdown")
        assert_rpc_error(lambda: inv("choose", 9), data="InvokeRejected")


if __name__ == "__main__":
    sys.exit(run_demo("hello_vn_tide", body))
