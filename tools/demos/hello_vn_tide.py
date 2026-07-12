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

        def walk_line() -> None:
            """Advance through the current line (snap, then step)."""
            inv("advance", None)  # snap the typewriter to full
            inv("advance", None)  # step off the line

        # ── (A) boot: step 0 is a narration line, nothing revealed yet.
        assert_eq(q("mode"), "line", "boots on a line")
        assert_eq(q("step"), 0, "at the first step")
        assert_eq(q("step_count"), 7, "the branching set-piece has 7 steps")
        assert_eq(q("speaker"), "", "step 0 is narration (no speaker)")
        assert_eq(q("revealed_chars"), 0, "nothing revealed at t=0")
        line_len = q("line_len")
        assert isinstance(line_len, int) and line_len > 8, f"a real line: {line_len}"
        assert_eq(q("fully_revealed"), False, "not yet fully revealed")

        # ── (A) the typewriter reveals a growing prefix (40 cps -> 200 ms = 8).
        tick(200)
        assert_eq(q("revealed_chars"), 8, "200 ms revealed 8 chars at 40 cps")
        assert q("revealed_chars") < line_len, "still mid-reveal"
        inv("advance", None)  # snap the partly-revealed line to full
        assert_eq(q("fully_revealed"), True, "advance snapped the line to full")
        assert_eq(q("revealed_chars"), line_len, "the whole line is shown")
        inv("advance", None)  # step onto the 무녀 line
        assert_eq(q("step"), 1, "advanced onto step 1")
        assert_eq(q("speaker"), "무녀", "step 1 has a speaker")
        walk_line()  # through the 무녀 line to the choice

        # ── the one decisive timed choice, and its two branch targets.
        assert_eq(q("mode"), "choice", "reached the timed choice")
        assert_eq(q("step"), 2, "on step 2")
        assert_eq(q("option_count"), 2, "two options")
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
        # The peril line's with_goto jumps past the endure branch (5,6) to End.
        walk_line()
        assert_eq(q("mode"), "end", "peril branch reached the End")
        assert_eq(q("step"), 7, "line goto skipped the endure branch (5,6)")

        # ── (C) PATH timeout — scrub back onto the choice, let it expire.
        g.intervene(f"{EXT}/step", 2)
        assert_eq(q("mode"), "choice", "scrubbed back onto the choice")
        assert_eq(q("remaining_ms"), 4000, "the scrub reset the countdown")
        tick(4500)  # cross the 4 s timeout
        assert_eq(q("step"), 3, "timeout took the DEFAULT option's branch (돌아본다 -> 3)")
        assert_eq(q("timed_out"), True, "resolved by the countdown, not the player")
        assert_eq(q("outcome"), "turn", "the default option's outcome")

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
        assert_eq(script["steps"][2]["kind"], "timed_choice", "step 2 is the choice")
        assert_eq(script["steps"][4]["goto"], 7, "the peril line carries its End jump")

        # ── (E) every failure surfaces loudly over the wire, never a silent success.
        assert_rpc_error(lambda: inv("choose", 0), data="InvokeRejected")  # at End, not a choice
        assert_rpc_error(lambda: tick("soon"), data="InvokeTypeMismatch")
        assert_rpc_error(lambda: inv("bogus", None), data="UnknownInvokePath")
        assert_rpc_error(lambda: g.intervene(f"{EXT}/mode", "x"), data="ReadOnly")
        assert_rpc_error(lambda: g.intervene(f"{EXT}/nonexistent", 3), data="UnknownIntervenePath")
        # A well-typed but out-of-range choose is Rejected (scrub onto a choice first).
        g.intervene(f"{EXT}/step", 2)
        assert_rpc_error(lambda: inv("choose", 9), data="InvokeRejected")


if __name__ == "__main__":
    sys.exit(run_demo("hello_vn_tide", body))
