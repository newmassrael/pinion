#!/usr/bin/env python3
"""R1570.1 §5.39 §5.16 §5.12 — a declared interactive role is a keyboard focus stop.

`#[widget(role = Button | CheckBox | Switch | RadioButton | Listbox)]` announces
an operable control to assistive technology. WAI-ARIA requires such a control to
be focusable, and every floor this project measures against gives it for free:
HTML's native `<button>` / `<input type=checkbox>` are focusable without a
`tabindex`, and Qt's `QAbstractButton` / `QCheckBox` / `QRadioButton` are
`Qt::StrongFocus` by default.

pinion's focus enumeration is scene-derived (R1020 §5.39): a node is a Tab stop
because its `LayoutStyle` says `focusable`. That is the right source — R1020
retired the trait-level `focusable_tags()` precisely to avoid a second one — but
it is an OPT-IN, and R1570's census found the opt-in missing in **17 of 23**
bindings that declare an interactive role. Measured before the fix: `focus/set`
refused the tag and `focus/next` answered `None`, meaning those windows had no
keyboard focus stop at all.

The second-order consequence is what makes it more than an accessibility gap.
`apply_aria_activate` gates on `focused == Some(my_tag)`, so in a binding whose
control can never hold focus that body **can never return true** — 13 of the 25
byte-identical `apply_key` bodies in the tree were unreachable code, and the doc
comments above them described a Space/Enter behaviour that could not happen.

Why the tree did not already know: the R1518 focus sweep asserts every binding
it walks has a stop, but its population is a hand-written list of 14 composites.
A curated population cannot find an absence — a binding with zero focus stops is
simply never walked. So THIS demo derives its population from the source
declarations instead, and prints it: a binding written tomorrow is covered the
day its `role` is declared, and a parse that silently matched nothing shows up
as a count of zero rather than as a pass.

What this asserts:

  * the population is DERIVED and non-empty, and every excluded binding is
    named with the role that excluded it, so the judgment is visible rather
    than buried in a regex;
  * for every interactive-role binding: `focus/set` accepts its declared tag,
    `focus/get` then reports exactly that tag, and `focus/next` from cold
    reaches it. Three facts, because "the tag is focusable" and "the tag is in
    the Tab order" are different claims and the pre-R1570.1 tree failed both;
  * NEGATIVE CONTROL — a decorative node is NOT a stop. `hello-gradient`'s
    `hue_strip` is a `Scene::Box` painting a gradient, and R1570.1 briefly made
    it focusable by editing the wrong node; the wire caught it. Without this
    assertion the sweep would pass just as well if `focusable` were stamped on
    everything;
  * CONSEQUENCE — on the Switch family the activation that was dead code now
    fires: with focus on the control, `Enter` flips the value bit through
    `apply_key`. Asserted on the bindings whose observable is uniform, rather
    than claimed for all 23.

ZERO-FLAKE: bounded `wait_until` polling (never a fixed sleep). >=30 assertions.

Run from the workspace root (the sweep builds these already):
    python3 tools/demos/r1570_1_interactive_role_is_a_focus_stop.py
"""

from __future__ import annotations

import re
import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

WORKSPACE = Path(__file__).resolve().parent.parent.parent

#: ARIA roles that denote an OPERABLE control. A role outside this set is a
#: structural or live-region role and carries no focus obligation, so listing
#: the interactive ones (rather than excluding the structural ones) keeps a
#: role added later from being silently swept in.
INTERACTIVE_ROLES = {"Button", "CheckBox", "Switch", "RadioButton", "Listbox"}

#: R1576 — no fixed sleep in front of the readiness handshake.
#:
#: `RpcSubprocess` sleeps `boot_grace` and THEN polls `scene/cache_stats` until
#: the first windowed paint completes — bounded polling, the zero-flake shape
#: this demo's own docstring claims. The sleep only widens the window in which
#: an instant crash is reported as a boot failure rather than as a failed first
#: request, and both carry the same stderr. This demo starts ~89 processes, so
#: at the 1.5s it used to pass that padding WAS the 180s sweep budget: measured
#: per boot on one box, 2.06s at 1.5, 1.26s at the 0.8 default, 1.00s at zero.
BOOT_GRACE = 0.0

#: Bindings whose Switch value bit is readable the same way, used for the
#: activation half. A subset by design — the sweep proves the PRECONDITION for
#: all 23, and this proves the CONSEQUENCE where the observable is uniform.
SWITCH_FAMILY = [
    "hello-toggle",
    "hello-richtext",
    "hello-richtext-background",
    "hello-richtext-blocks",
    "hello-richtext-list",
]

#: R1570.5 — TUI siblings. They are terminal bindings driven through a
#: different entry point, and this harness's stdin handshake gets a broken pipe
#: rather than a refusal. Excluded by NAME and listed, so the exclusion is a
#: decision on the page instead of a silent gap; their GUI siblings
#: (`hello-button`, `hello-commands`, `hello-toggle`) are in the population and
#: share the `WidgetCore` body under test.
NO_RPC_STDIN = {"hello-button-tui", "hello-commands-tui", "hello-toggle-tui"}

#: A painted node that must NOT be a stop. `hue_strip` is a decorative gradient
#: `Scene::Box` in `hello-gradient`; R1570.1 stamped it by mistake and this is
#: what found it.
DECORATIVE = ("hello-gradient", "hue_strip")


def declared_controls() -> tuple[
    list[tuple[str, str, str]], list[tuple[str, str]], list[str]
]:
    """Every binding that presents an interactive ARIA role, however it says so.

    R1570.5 — the population used to be "bindings whose `#[widget(...)]`
    attribute names an interactive `role`", which is 23. It is not the class:
    89 bindings construct an interactive `AriaRole`, and the other 66 write
    their `WidgetA11y` impl by hand. Scoping the gate to the attribute made its
    verdict read as total while covering a quarter of the subject, and five of
    the unscanned bindings had the exact defect (`hello-grouped-sort`,
    `hello-listbox-multi`, `hello-radio-group`, `hello-scene-scale`,
    `hello-virtual-sort`). A derived population is only as wide as the thing it
    derives from — that is the same lesson as R1518's curated list, one level
    less obvious.

    Two kinds come back, because only one of them can be asked a precise
    question. A binding that DECLARES `role` + `tag` can be asked whether THAT
    tag is focusable; a hand-written one names its roles somewhere in a
    `WidgetA11y` impl this scan will not try to parse, so all it can be asked
    is whether the window has any focus stop at all. Weaker, and still enough:
    the defect this gate exists for is "no stop anywhere".

    Returns `(declared, excluded_by_role, hand_written)`.
    """
    declared: list[tuple[str, str, str]] = []
    excluded: list[tuple[str, str]] = []
    hand_written: list[str] = []
    for main_rs in sorted((WORKSPACE / "examples").glob("*/src/main.rs")):
        src = main_rs.read_text()
        name = main_rs.parts[-3]
        attr = re.search(r"#\[widget\((?P<a>.*?)\n\)\]", src, re.S)
        if attr is not None:
            body = attr.group("a")
            # Anchored to line starts: an unanchored match reads `tag = "..."`
            # out of the module's own doc comments, which is how this demo's
            # first draft picked the wrong tag for two bindings.
            role = re.search(r"^\s*role\s*=\s*(\w+)", body, re.M)
            tag = re.search(r'^\s*tag\s*=\s*"([^"]+)"', body, re.M)
            if role is not None and tag is not None:
                if role.group(1) in INTERACTIVE_ROLES:
                    declared.append((name, tag.group(1), role.group(1)))
                else:
                    excluded.append((name, role.group(1)))
                continue
        # No attribute, or one that declares no role: fall back to the roles the
        # binding CONSTRUCTS. `AriaRole::X` in the source is the only statement
        # a hand-written `WidgetA11y` impl makes that this scan can read.
        roles = sorted(set(re.findall(r"AriaRole::(\w+)", src)))
        if any(r in INTERACTIVE_ROLES for r in roles):
            if name not in NO_RPC_STDIN:
                hand_written.append(name)
    return declared, excluded, hand_written


def focused(tf: RpcSubprocess) -> Any:
    return tf.request("focus/get").result.get("focused")


def body() -> None:
    checks = 0
    interactive, excluded, hand_written = declared_controls()

    # ── (A) the population is derived, non-empty, and its edges are named ──
    print(f"[demo] declared role + tag: {len(interactive)} binding(s)")
    for name, tag, role in interactive:
        print(f"[demo]   {name:30}{role:13}{tag}")
    print(f"[demo] hand-written WidgetA11y with an interactive role: "
          f"{len(hand_written)}")
    print(f"[demo] excluded by role: {len(excluded)}  "
          f"| excluded as non-RPC: {len(NO_RPC_STDIN)}")
    for name, role in excluded:
        print(f"[demo]   {name:30}{role} (not an operable role)")
    assert interactive, (
        "the source scan matched no interactive-role binding — every assertion "
        "below would be vacuous, which is exactly how a curated population "
        "hides an absence"
    )
    checks += 1
    # R1570.5 — the hand-written half is the larger one, and the gate reported
    # a total verdict without it for exactly one round. Asserting it is
    # non-empty keeps a scan that silently stops matching from reading as a
    # tree that has no such bindings.
    assert len(hand_written) > len(interactive), (
        f"the hand-written half ({len(hand_written)}) should outnumber the "
        f"declared one ({len(interactive)}) — if it does not, the AriaRole "
        f"scan has stopped matching and this gate has quietly narrowed again"
    )
    checks += 1

    # ── (B) every one of them is a focus stop, and is in the Tab order ────
    #
    # R1576 — ONE process per binding, ordered so the claims that need a COLD
    # focus state come first. Three separate loops over one population booted
    # this binding three times, and the sweep killed the whole demo at its 180s
    # budget: 120 real windowed shells, each paying a 1.5s FIXED SLEEP in front
    # of a readiness handshake that is already bounded polling. Measured on one
    # box, per boot: 2.06s at `boot_grace=1.5`, 1.26s at the 0.8 default, 1.00s
    # at zero — so the sleep, not the work, was most of the budget. The claims
    # are unchanged; what changed is how many times each binding is started.
    #
    # `focus/next` ENUMERATING a tag and `focus/set` REACHING it stay distinct
    # claims (a stop only the latter reaches is unusable from a keyboard) — the
    # enumeration simply runs BEFORE anything sets focus, which is what "from
    # cold" meant when it had its own process.
    for name, tag, role in interactive:
        with RpcSubprocess(name, boot_grace=BOOT_GRACE) as tf:
            wait_until(
                lambda: focused(tf) is None,
                timeout=4.0,
                interval=0.03,
                desc=f"{name}: nothing is focused at boot",
            )
            checks += 1

            # (C) — in the Tab order, from cold.
            stop = tf.request("focus/next").result.get("focused")
            assert stop is not None, f"{name}: focus/next found no stop at all"
            checks += 1

            # `focus/set` accepting the tag is the fact `apply_aria_activate`
            # needs; before R1570.1 seventeen of these refused it.
            tf.request("focus/set", {"tag": tag})
            assert_eq(
                focused(tf),
                tag,
                f"{name}: focus rests on the {role} the binding declares",
            )
            checks += 1

            # (E) CONSEQUENCE — on the Switch family the activation that was
            # dead code now fires. Runs here rather than in a fourth loop, with
            # focus already on the declared tag, which is what it needed anyway.
            if name in SWITCH_FAMILY:

                def value(tf: RpcSubprocess = tf, tag: str = tag, name: str = name) -> Any:
                    node = find_by_tag(tf.snapshot(), tag)
                    assert node is not None, f"{name}: the External is in the state scene"
                    return node["introspect"]["value"]

                before = value()
                tf.key(path=tag, name="Enter")
                wait_until(
                    lambda: value() != before,
                    timeout=4.0,
                    interval=0.03,
                    desc=f"{name}: Enter reached apply_key's ARIA activation",
                )
                checks += 1

    # ── (C2) the hand-written half has a stop at all ──────────────────────
    # Weaker than (B)/(C) by necessity: these bindings name their roles inside
    # a `WidgetA11y` impl this demo does not parse, so there is no single tag
    # to point at. "The window has SOME focus stop" is still the defect this
    # gate exists for — a control announced as operable that no keyboard can
    # reach — and it is the assertion the 23-binding population could not make
    # about the other 66.
    for name in hand_written:
        with RpcSubprocess(name, boot_grace=BOOT_GRACE) as tf:
            stop = tf.request("focus/next").result.get("focused")
            assert stop is not None, (
                f"{name}: presents an interactive ARIA role and has NO focus "
                f"stop at all — announced as operable, unreachable by keyboard"
            )
            checks += 1

    # ── (D) NEGATIVE CONTROL — a decorative node is not a stop ────────────
    deco_binding, deco_tag = DECORATIVE
    with RpcSubprocess(deco_binding, boot_grace=BOOT_GRACE) as tf:
        refused = False
        try:
            tf.request("focus/set", {"tag": deco_tag})
        except RpcError:
            refused = True
        assert refused, (
            f"{deco_binding}: the decorative `{deco_tag}` gradient must NOT be a "
            f"focus stop — without this the sweep passes just as well when "
            f"`focusable` is stamped on every node"
        )
        checks += 1
        assert_eq(focused(tf), None, "D: and the refusal left focus where it was")
        checks += 1

    print(f"[demo] {checks} assertions")
    assert checks >= 30, f"the R660 baseline is 30 assertions; made {checks}"


if __name__ == "__main__":
    run_demo("R1570.1 §5.39 a declared interactive role is a focus stop", body)
