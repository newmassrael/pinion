#!/usr/bin/env python3
"""R1728 §5.38 §5.39 §5.40 §2 #2 §2 #7 — **the application's navigation is the
navigation that was specified, and where it is not, it says so.**

# What this demo exists for

The analysis tool this project assembles is modelled on a reference that is
**one application with eight sections**, drawn in one order, identical on every
screen. Measured at this round's open, by pulling the seat list off each of the
reference's three delivered screens: eight seats, same order, and the only
difference between the screens is which one is marked current.

This application drew **seven**, and the differences had never been compared by
anything:

* three of its seven keys were seats the reference does not have — one of the
  reference's sections split into two under invented names, and a third named
  after nothing;
* two of the reference's sections were **absent from the rail entirely**,
  because no vocabulary existed for *the reference has this working and we have
  not written it* — `reserved` would have claimed a later release and
  `elsewhere` would have sent a reader to open a window nobody built;
* the seat carrying the one section this application had finished was drawn
  with the mark the reference gives its **log** section;
* and two adjacent seats fell through to a fallback and were drawn
  **identically**, on a screen whose whole subject is telling things apart.

Every one of those survived every gate this screen had, for the same reason: a
roster made the rail a value, and nothing said whether the value was the right
one. `docs/analyzer-rail-spec.json` is that statement, reviewed as a claim about
the reference rather than as code, and `RosterSpec::diff` is what fails when the
application stops matching it — **in both directions**, so a seat the
application invents is as much a failure as one it lacks.

What this drives:

* **A** — the specification is a file, and the running application publishes how
  much of it it reproduces. Read over the wire, compared with the same file.
* **B** — the rail on the screen: eight seats, in the specified order, each
  pressed with the **machine's own pointer**, arriving or refusing as specified.
* **C** — the three kinds of refusal are three different sentences, with the
  recourse each derives.
* **D** — and a reader hears the difference: the accessibility tree carries the
  kind and the reason, not a disabled bit.

# Floor, measured by building a probe against 6.11.1 and running it

Nothing to compare against on this axis, and that is the finding rather than a
dodge. A paged container there addresses pages by **ordinal**, so reordering the
rail and changing where a press goes are the same edit and no diff can name a
seat; it carries one bool per page with no vocabulary for *why* a page is inert;
a disabled page is **arrived at anyway**; and its accessible value for "which
page is current" is empty. The statement this demo checks — *these are the
sections, in this order, and this one is inert for this reason* — cannot be
written down there at all, let alone compared.

Run from the workspace root (a real pointer needs a display):
    cargo build --release -p hello-analyzer-shell -p hello-node-lab
    DISPLAY=:97 python3 tools/demos/r1728_the_rail_is_the_one_the_reference_draws.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from analyzer_spec import rail_spec  # noqa: E402
from rpc_verify import (  # noqa: E402
    RealPointer,
    RealPointerUnavailable,
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
)

SHELL = "hello-analyzer-shell"
EXT = "/external"

CHECKS: list[str] = []

#: How many real-pointer sessions actually opened. Zero means this host could
#: not drive one, and the coverage line at the end says so rather than letting a
#: shorter run read as a pass — R1727.2, after four sections of that round's
#: demo went quietly missing on a host with no display.
REAL_POINTER_RUNS = 0


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def q(app: RpcSubprocess, path: str):
    return app.query(f"{EXT}/{path}")


#: ★ R1730 — the loader moved to `tools/analyzer_spec.py`. Six demos had grown a
#: copy of it, and the round that paid `keys` off broke five of them at once.
#: The reviewed artifact is read from the repository rather than from the
#: application: both sides of every comparison below must come from different
#: places, or the comparison is the application agreeing with itself.
specification = rail_spec


def pointer(app: RpcSubprocess):
    """A real pointer, or `None` with a loud line. Never a silent fallback to
    the wire: this demo's claim in section B is that a *person's* press walks
    the rail, and a run that could not make one must say so."""
    global REAL_POINTER_RUNS
    try:
        driver = RealPointer(app)
    except RealPointerUnavailable as exc:
        print(f"[real-pointer] UNAVAILABLE — section B is not driven: {exc}")
        return None
    REAL_POINTER_RUNS += 1
    return driver


def body() -> None:  # noqa: PLR0915 - one narrative, read top to bottom
    spec = specification()
    canon = spec["canon"]
    owed = spec["owed"]
    keys = [seat["key"] for seat in canon]
    ok("the specification declares the reference's eight seats", len(canon) == 8)
    ok(
        "and states them in the reference's order",
        [seat["ordinal"] for seat in canon] == list(range(1, 9)),
    )

    with RpcSubprocess(SHELL, boot_grace=1.5, visible_window=True) as app:
        # ── (A) the application reports its own conformance ───────────────
        banner("A — the application says how much of its specification it is")
        rail = q(app, "rail").split(",")
        assert_eq(
            rail,
            keys,
            "A: the rail the application publishes IS the specified rail, in "
            "the specified order. This read seven keys until this round, three "
            "of which the reference does not have",
        )

        conformance = q(app, "conformance")
        assert_eq(conformance["specified"], len(canon), "A: it counts what it is measured against")
        assert_eq(
            [d["says"] for d in conformance["divergences"]],
            [entry["sentence"] for entry in owed],
            "A: ★★ every way the application differs from the reference is a "
            "way somebody wrote down and justified -- and EXACTLY those, so a "
            "divergence quietly paid off fails here too",
        )
        assert_eq(
            conformance["reproduced"],
            len(canon) - len(owed),
            "A: and the reproduction is a number rather than an impression",
        )
        ok(
            "A: every accepted difference names the round that accepted it",
            all(e["since"].startswith("R") for e in conformance["owed"]),
        )
        ok(
            "A: and states why, at length",
            all(len(e["why"]) > 40 for e in conformance["owed"]),
        )
        # ★ The direction a one-sided check cannot see. Nothing on the rail may
        # be a seat the specification does not declare.
        ok(
            "A: the application invents no seat of its own",
            not [k for k in rail if k not in keys],
        )

        # ── (B) the rail on the screen, walked with a real pointer ────────
        banner("B — eight seats, in order, pressed by the machine's own pointer")
        rects = abs_rects_of(app.snapshot(source="paint"))
        seats = []
        for seat in canon:
            tag = f"shell.rail.{seat['key']}"
            ok(f"B: the {seat['key']} seat is painted", tag in rects)
            seats.append((seat, rects[tag]))
        tops = [rect[1] for _, rect in seats]
        ok(
            "B: ★ and they run top to bottom in the specified order -- a roster "
            "is a list and a rail is a column, and the two agreeing is a "
            "separate fact nothing had ever checked",
            tops == sorted(tops) and len(set(tops)) == len(tops),
        )

        driver = pointer(app)
        if driver is not None:
            with driver as hand:
                pressed = 0
                for seat, rect in seats:
                    key = seat["key"]
                    centre = (rect[0] + rect[2] / 2, rect[1] + rect[3] / 2)
                    app.intervene(f"{EXT}/nav", "dashboard")
                    app.tick(8)
                    hand.move(centre)
                    hand.press()
                    hand.release()
                    app.tick(16)
                    at = q(app, "nav")
                    if seat["standing"] == "open" and not any(
                        e["key"] == key for e in owed
                    ):
                        assert_eq(at, key, f"B: a real press on {key} arrives there")
                    else:
                        assert_eq(
                            at,
                            "dashboard",
                            f"B: a real press on {key} refuses and STAYS -- the "
                            "row the floor fails, where a disabled page is "
                            "arrived at anyway",
                        )
                    pressed += 1
                ok(f"B: all {pressed} specified seats took a real press", pressed == len(canon))

        # ── (C) three reasons, three sentences ────────────────────────────
        banner("C — a seat that will not open says which kind of shut it is")
        disabled = {
            row["tag"]: row
            for row in app.request("scene/disabled", {}).result["disabled"]
            if row["tag"].startswith("shell.rail.")
        }
        by_reason: dict[str, list[str]] = {}
        for tag, row in disabled.items():
            by_reason.setdefault(row["reason"], []).append(tag.rsplit(".", 1)[1])
        # ★ R1730 — DERIVED from the specification rather than written out. The
        # first round to pay a divergence off broke both of the literals that
        # used to be here, which is the same class as a stale number in prose.
        # ★★★★★ R1731 — and the KINDS are derived too, for the same reason one
        # round later. This asserted `["reserved", "unbuilt"]`, and the round
        # that built the last owed section left the rail with ONE kind: an
        # `unbuilt` seat is one the specification opens and this build has not,
        # and there are none. A demo that pins the vocabulary a screen happens
        # to be using pins the state of the build, which is the thing under
        # test.
        reserved_keys = sorted(s["key"] for s in canon if s.get("kind") == "reserved")
        owed_keys = sorted(entry["key"] for entry in owed)
        wanted = {"reserved": reserved_keys}
        if owed_keys:
            wanted["unbuilt"] = owed_keys
        assert_eq(
            sorted(by_reason),
            sorted(wanted),
            "C: ★★★ every kind of shut this rail SPELLS is one the specification "
            "accounts for. `reserved` is booked for a later release the "
            "reference itself defers; `unbuilt` is in the reference's own FIRST "
            "release and not written here, and R1731 left none of those. ★ A "
            "third kind, `elsewhere`, was here until R1729 mounted the capture "
            "viewer -- no section of this tool is built-and-unreachable any more",
        )
        for reason, keys in wanted.items():
            assert_eq(sorted(by_reason[reason]), keys, f"C: the {reason} seats")
        # ★★ The recourse is DERIVED, and these two kinds legitimately share
        # one: the reader's action is the same (wait) and what they are waiting
        # for is not, which is why the kinds stay apart while the recourse
        # merges.
        for tag, row in disabled.items():
            assert_eq(
                row["recourse"],
                "await_release",
                f"C: {tag} derives its recourse",
            )
        # ★★★★★ R1731 — the claim is now over the kinds the rail ACTUALLY
        # spells, one seat of each. It read one reserved seat and one owed seat,
        # and the round that built the last owed section left the second index
        # out of range. What the check is about is that two seats sharing a
        # recourse do not share a SENTENCE — so its population is one seat per
        # kind, whatever kinds there are, and it says how many that was.
        one_each = [keys[0] for keys in wanted.values()]
        sentences = {disabled[f"shell.rail.{k}"]["detail"] for k in one_each}
        ok(
            f"C: ★ the {len(one_each)} kind(s) of shut this rail spells give "
            "as many distinct sentences, so a reader is not told to wait for "
            "the wrong thing",
            len(sentences) == len(one_each),
        )

        # ── (D) and a reader hears it ─────────────────────────────────────
        banner("D — the difference reaches somebody who cannot see the rail")
        tree = {n["tag"]: n for n in app.request("scene/access").result["nodes"]}
        for seat in canon:
            tag = f"shell.rail.{seat['key']}"
            ok(f"D: the {seat['key']} seat is in the tree", tag in tree)
        for key in owed_keys:
            node = tree[f"shell.rail.{key}"]
            reason = node.get("unavailable")
            ok(
                f"D: the {key} seat is announced unavailable rather than merely "
                "drawn faint",
                isinstance(reason, dict),
            )
            # ★ The row the floor cannot answer. Measured on 6.11.1: a locked
            # rail seat there is focusable, selectable, and carries no
            # unavailable state at all -- there is one bool on the widget and
            # nowhere to put a phrase.
            assert_eq(reason["kind"], "unbuilt", f"D: {key} announces WHICH kind of shut")
            assert_eq(reason["recourse"], "await_release", f"D: and what to do about it")
            ok(
                f"D: and {key} names what specifies it, so a listener is told "
                "the section exists in the plan",
                len(reason["detail"]) > 8,
            )
        # ★★ And every kind the rail spells reaches a listener as its own
        # reason, which is the whole point of spending an arm rather than
        # reusing one: the seats are inert, they ask the reader to wait, and
        # what they are waiting for is not the same.
        # ★ R1730 — one seat of each kind, derived. ★ R1731 — and the KINDS are
        # derived too, because the round that built the last owed section left
        # this reading one seat that no longer exists.
        heard = {tree[f"shell.rail.{k}"]["unavailable"]["kind"] for k in one_each}
        assert_eq(
            sorted(heard),
            sorted(wanted),
            "D: ★★★ a reader hears distinct reasons where a bool has one -- and "
            "hears them for seats whose RECOURSE is identical, which is exactly "
            "the case a bool, and a recourse alone, both flatten",
        )
        # ★ R1729 — the seat that used to be the third reason is now a page, so
        # a reader arrives at it instead of being told where it lives.
        ok(
            "D: the capture seat is announced as somewhere you go, not as a "
            "reason you cannot",
            tree["shell.rail.packets"].get("unavailable") is None,
        )

    # ★★★★★ The demo's own coverage, said out loud. Section B is the only one
    # that needs a real pointer; on a host without one it is skipped, and
    # without this line the only evidence would be a smaller number nobody was
    # comparing against anything.
    print(f"\n{len(CHECKS)} named check(s):")
    for line in CHECKS:
        print(f"  - {line}")
    driven = [c for c in CHECKS if c.startswith("B:")]
    if REAL_POINTER_RUNS == 0:
        print(
            f"[coverage] NO REAL POINTER on this host: {len(CHECKS)} checks ran "
            "and every one came from the wire. Section B's press walk "
            "contributed nothing."
        )
    else:
        assert len(driven) >= 10, (
            f"the real pointer ran {REAL_POINTER_RUNS} time(s) but only "
            f"{len(driven)} check(s) came from it — the press walk stopped "
            "contributing without saying so"
        )
        print(
            f"[coverage] {REAL_POINTER_RUNS} real-pointer session(s) contributed "
            f"{len(driven)} of {len(CHECKS)} named checks."
        )


if __name__ == "__main__":
    run_demo("r1728 the rail is the one the reference draws", body)
