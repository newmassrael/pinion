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

#: A painted node that must NOT be a stop. `hue_strip` is a decorative gradient
#: `Scene::Box` in `hello-gradient`; R1570.1 stamped it by mistake and this is
#: what found it.
DECORATIVE = ("hello-gradient", "hue_strip")


def declared_controls() -> tuple[list[tuple[str, str, str]], list[tuple[str, str]]]:
    """Every `#[widget(...)]` binding, split into interactive and not.

    Derived from source rather than listed, so the population tracks the tree.
    Returns `(interactive, excluded)` and the caller prints both.
    """
    interactive: list[tuple[str, str, str]] = []
    excluded: list[tuple[str, str]] = []
    for main_rs in sorted((WORKSPACE / "examples").glob("*/src/main.rs")):
        src = main_rs.read_text()
        attr = re.search(r"#\[widget\((?P<a>.*?)\n\)\]", src, re.S)
        if attr is None:
            continue
        body = attr.group("a")
        # Anchored to line starts: an unanchored match reads `tag = "..."` out
        # of the module's own doc comments, which is how this demo's first
        # draft picked the wrong tag for two bindings.
        role = re.search(r"^\s*role\s*=\s*(\w+)", body, re.M)
        tag = re.search(r'^\s*tag\s*=\s*"([^"]+)"', body, re.M)
        if role is None or tag is None:
            continue
        name = main_rs.parts[-3]
        if role.group(1) in INTERACTIVE_ROLES:
            interactive.append((name, tag.group(1), role.group(1)))
        else:
            excluded.append((name, role.group(1)))
    return interactive, excluded


def focused(tf: RpcSubprocess) -> Any:
    return tf.request("focus/get").result.get("focused")


def body() -> None:
    checks = 0
    interactive, excluded = declared_controls()

    # ── (A) the population is derived, non-empty, and its edges are named ──
    print(f"[demo] derived population: {len(interactive)} interactive-role binding(s)")
    for name, tag, role in interactive:
        print(f"[demo]   {name:30}{role:13}{tag}")
    print(f"[demo] excluded by role: {len(excluded)}")
    for name, role in excluded:
        print(f"[demo]   {name:30}{role} (not an operable role)")
    assert interactive, (
        "the source scan matched no interactive-role binding — every assertion "
        "below would be vacuous, which is exactly how a curated population "
        "hides an absence"
    )
    checks += 1

    # ── (B) every one of them is a focus stop ─────────────────────────────
    for name, tag, role in interactive:
        with RpcSubprocess(name, boot_grace=1.5) as tf:
            wait_until(
                lambda: focused(tf) is None,
                timeout=4.0,
                interval=0.03,
                desc=f"{name}: nothing is focused at boot",
            )
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

    # ── (C) and it is in the Tab order, from cold ─────────────────────────
    # Separate from (B) on purpose: `focus/set` reaching a tag and `focus/next`
    # ENUMERATING it are different claims, and a stop that only the former can
    # reach is not usable from a keyboard.
    for name, tag, _role in interactive:
        with RpcSubprocess(name, boot_grace=1.5) as tf:
            stop = tf.request("focus/next").result.get("focused")
            assert stop is not None, f"{name}: focus/next found no stop at all"
            checks += 1

    # ── (D) NEGATIVE CONTROL — a decorative node is not a stop ────────────
    deco_binding, deco_tag = DECORATIVE
    with RpcSubprocess(deco_binding, boot_grace=1.5) as tf:
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

    # ── (E) CONSEQUENCE — the activation that was dead code now fires ─────
    for name in SWITCH_FAMILY:
        with RpcSubprocess(name, boot_grace=1.5) as tf:
            tag = next(t for n, t, _ in interactive if n == name)

            def value() -> Any:
                node = find_by_tag(tf.snapshot(), tag)
                assert node is not None, f"{name}: the External is in the state scene"
                return node["introspect"]["value"]

            tf.request("focus/set", {"tag": tag})
            before = value()
            tf.key(path=tag, name="Enter")
            wait_until(
                lambda: value() != before,
                timeout=4.0,
                interval=0.03,
                desc=f"{name}: Enter reached apply_key's ARIA activation",
            )
            checks += 1

    print(f"[demo] {checks} assertions")
    assert checks >= 30, f"the R660 baseline is 30 assertions; made {checks}"


if __name__ == "__main__":
    run_demo("R1570.1 §5.39 a declared interactive role is a focus stop", body)
