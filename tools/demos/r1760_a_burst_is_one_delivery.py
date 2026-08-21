#!/usr/bin/env python3
"""R1760 §5.49 §5.39 §2 #2 — a burst of keys is ONE delivery, proved
against a live windowed app.

R1757 gave `scene/key` a `keys: [...]` arm so an agent can drive a
keystroke GESTURE and not just a keystroke — one drain is one delivery
(R1658), so every key of one request reaches the binding carrying the
same arrival, which is what a repeat window, a chord timeout and a
double-tap are statements about. It published the open delivery as
`scene/input_state`'s `key_delivery.opened` so the agent can CONFIRM
that, by reading the axis either side of its request and seeing it
advance by exactly one.

## Why this demo exists: R1757 shipped that reading BROKEN, and every
## in-process gate said it was fine

R1757's verification was unit tests plus `ShellCore` fixtures, and the
defect lived where none of them look — in the winit event loop.
`AppShell::new_events` opens a delivery on EVERY iteration, so in a real
windowed app the counter free-ran on the clock: the difference an agent
read across its own burst was "my burst, plus however many idle
iterations happened while I waited". The two demos that assert
`scene/input_state` is side-effect-free (`r885_input_state`,
`r1419_window_focus` — it is a `HandlerKind::Read`) went red in CI and
were right to.

That is R1757's own headline lesson at the other end of the frame: it
stopped the RPC drain opening a delivery for a keyless request and left
the event loop doing exactly the same thing. R1760 allocates the NUMBER
lazily — a handover that delivers no keystroke burns none — so `opened`
counts deliveries that delivered, the only kind an agent can observe.

So this demo is the gate that was missing. It is deliberately end-to-end
over the wire against a live GUI binding, because that is the only place
the defect was visible.

## What it proves

  (A) the read is side-effect-free ON THIS AXIS: two reads separated by
      a real wait are identical. This is the regression itself — before
      R1760 the wait alone moved it.
  (B) a burst of N named keys advances `opened` by EXACTLY ONE.
  (C) the same keys sent as N requests advance it by N — the honest
      difference, and the assertion that makes (B) mean something. A
      fixture where both answered the same would gate nothing.
  (D) a request that dispatches no keystroke does not move it at all,
      which is what keeps (B) and (C) readable: the bracketing reads are
      themselves requests.
  (E) the keys really arrived — the burst is not a counter trick, so the
      app's own state changes under it.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    assert_input_axes,
    run_demo,
    text_of_tag,
)

EXAMPLE = "hello-textfield"

#: The binding's own tag for its field (`hello-textfield`'s `TF_TAG`), so the
#: keystrokes are aimed by NAME rather than at a coordinate this demo guessed.
TEXT_FIELD = "main_textfield"

#: R1627 — the axes this demo reads. The whole-set census lives beside the
#: emitter (`pinion_rpc::dispatch::INPUT_STATE_AXES`).
USES = ("key_delivery",)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        r0 = tf.request("scene/input_state", {})
        assert r0 is not None
        assert_input_axes(r0.result, needs=USES, label="boot input_state")

        # ── (A) the axis does not move on its own ───────────────────
        # THE REGRESSION. A real wait, because the defect was driven by
        # event-loop iterations rather than by requests: before R1760 the
        # window between these two reads was enough on its own.
        before_idle = tf.key_delivery_opened()
        time.sleep(0.5)
        assert_eq(tf.key_delivery_opened(), before_idle,
                  "waiting half a second does not open a delivery")

        # ── (D) a keyless request does not move it either ───────────
        # Asserted BEFORE the burst, because the bracketing reads in (B)
        # are themselves requests: if a request could move the axis, (B)
        # would be measuring its own instrument.
        before_click = tf.key_delivery_opened()
        tf.click(at=(40.0, 40.0))
        assert_eq(tf.key_delivery_opened(), before_click,
                  "a click dispatches no keystroke, so it opens no delivery")

        # ── (B) a burst of three is ONE delivery ────────────────────
        before_burst = tf.key_delivery_opened()
        tf.keys(["ArrowLeft", "ArrowLeft", "ArrowLeft"], at=(40.0, 40.0))
        after_burst = tf.key_delivery_opened()
        assert_eq(after_burst - before_burst, 1,
                  "three keys in ONE request opened exactly one delivery")

        # ── (C) ...and three requests are three ─────────────────────
        # The discriminating half: without it, an implementation that
        # never advanced the counter would pass (B).
        before_singles = tf.key_delivery_opened()
        for _ in range(3):
            tf.key(at=(40.0, 40.0), name="ArrowLeft")
        after_singles = tf.key_delivery_opened()
        assert_eq(after_singles - before_singles, 3,
                  "the same three keys sent separately are three deliveries")

        # ── (E) the keys actually arrived ───────────────────────────
        # A counter that advanced while nothing was delivered would satisfy
        # everything above. Type into the field and read the text back through
        # the PAINT, so the burst is anchored to an observable effect rather
        # than to its own bookkeeping.
        tf.click(path=TEXT_FIELD)
        before_typed = tf.key_delivery_opened()
        tf.keys(["a", "b", "c"], path=TEXT_FIELD)
        assert_eq(tf.key_delivery_opened() - before_typed, 1,
                  "a character burst is one delivery too")
        assert "abc" in text_of_tag(tf, TEXT_FIELD), \
            "the burst's characters reached the field"

        # Assertion count: A 1 + B 1 + C 1 + D 1 + E 2 = 6 assert_eq/assert
        # calls, plus the boot axis census and its non-None response assert.


if __name__ == "__main__":
    sys.exit(run_demo("R1760 §5.49 — a burst of keys is one delivery", body))
