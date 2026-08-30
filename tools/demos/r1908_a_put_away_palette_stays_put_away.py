#!/usr/bin/env python3
"""R1908 §5.32 §5.15 §2 #7 — **the palette a person put away is still away when
they come back, and a session this build cannot honour is replaced OUT LOUD.**

# What this demo exists for

Standing rule (7) of this repayment loop asks for the analyzer UI assembled and
asserted by one walk. This one closes the arrangeable-unit campaign's last
order item, and the entry re-measurement turned that item around.

# ★★★★★ What the entry re-measurement found

The carried prescription was "make a node lab pane open FOLDED, as the reference
editor's tool region does". Extracting the behaviour canon and reading its
initial state, the whole vocabulary of open-and-shut in it is three flags:

    paletteOpen: true      <- the dashboard shell's drawer, OPEN
    presetOpen:  false     <- a menu
    cfgOpen:     null      <- a popover

⇒ the canon has **no panel that opens folded at all**, which is what R1902
measured when it built "hidden by default", found seventeen gates red, and
reverted. Building it again would repeat that.

What IS missing is one step on. The canon persists `{name, widgets}` and this
tree persisted the same, so a fold was re-seeded from the specification at every
boot: R1903 built a gesture that closing the application undid. And that is
where `EdgePlacement::folded_at` — built by R1902 and, measured at entry, with
ZERO consumers in the tree — actually belongs: a folded panel is not something a
build declares, it is something a person did and came back to.

# Superior to the floor

The floor toolkit round-trips a docked arrangement as an OPAQUE BYTE STRING.
Nothing can read which edge a panel is on out of it, diff two, or write one by
hand — and nothing JUDGES one on the way back in: a restore either takes or does
not. Here the session is named JSON, and it goes through the same predicate an
opening does, with one deliberate difference this round found: a panel that
declared it cannot move refuses a STORED edge, because nothing on the screen
could honour it. A refusal names what was asked and what was allowed, and a boot
still produces a screen.

# What this walk holds

  (A) the first run opens on the specification, with nothing written yet.
  (B) putting the palette away REACHES THE DISK, folded, with its width kept.
  (C) a SECOND PROCESS opens with the palette still away — the property, and the
      thing one process cannot show.
  (D) the tool says the placement was RESTORED, which `at` alone cannot: folded
      is the same bit whether the build opens that way or a person folded it.
  (E) bringing it back is written too, so a person who restores the panel does
      not find it away again tomorrow.
  (F) a session this build cannot honour is replaced by the specification AND
      EXPLAINED — a silent fallback is a reader seeing the wrong panel and
      having no way to learn why.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1908_a_put_away_palette_stays_put_away.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    isolated_storage_dir,
    run_demo,
)

SHELL = "hello-analyzer-shell"
EXT = "/external"
#: The key the shell writes its session under.
KEY = "analyzer_shell.arrangements"
#: The palette's own tag, which is its key inside that blob.
PALETTE = "shell.palette"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def blob(root: Path) -> dict | None:
    """What is actually on disk, read as a file rather than asked of the app.

    ⚠ The override IS the root — `build_app_storage` hands it straight to the
    file store, so the app name only picks a directory on the default path.
    R1897 measured that the hard way.
    """
    path = root / KEY
    return json.loads(path.read_text(encoding="utf-8")) if path.exists() else None


def placement(app: RpcSubprocess) -> dict:
    """What the running tool says about the palette's placement."""
    spec = app.query(f"{EXT}/spec")
    spec = json.loads(spec) if isinstance(spec, str) else spec
    return spec["palette_placement"]


def settle(app: RpcSubprocess) -> None:
    for _ in range(6):
        app.tick_ms(16)


def body() -> None:
    with isolated_storage_dir("pinion-analyzer-shell-r1908-") as root:
        _first_run(root)
        _second_run(root)
        _refused_session(root)
    print(f"\n{len(CHECKS)} check(s) held.")


def _first_run(root: Path) -> None:
    banner("A — the first run opens on the specification, nothing written yet")
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        at = placement(app)
        ok(
            f"A: the palette opens SHOWING, as the canon's own state does — {at['at']}",
            at["at"]["folded"] is False and at["opens"]["folded"] is False,
        )
        ok(
            f"A: ** and it does not claim to be a restored arrangement — "
            f"restored={at.get('restored')}",
            at.get("restored") is False,
        )
        ok(
            "A: * nothing is on disk, because opening is not arranging",
            blob(root) is None,
        )

        banner("B — putting it away reaches the disk")
        said = app.invoke(f"{EXT}/palette", "fold")
        settle(app)
        ok(f"B: the host admits the fold — {said!r}", said is not None)
        stored = blob(root)
        ok(
            f"B: ***** the fold REACHED THE DISK — {stored is not None}. Before "
            "this round the placement was re-seeded from the specification at "
            "every boot, so closing the application undid the gesture",
            stored is not None,
        )
        kept = (stored or {}).get("chrome", {}).get(PALETTE)
        ok(
            f"B: ** stored under the panel's own tag, folded — {kept}",
            kept is not None and kept["folded"] is True,
        )
        ok(
            f"B: ***** with the WIDTH KEPT, so opening it gives back a size "
            f"worth having — {kept['extent']} vs the opening "
            f"{at['opens']['extent']}. A fold that forgot its extent would open "
            "to nothing, which is the difference between folding and hiding",
            kept["extent"] == at["opens"]["extent"],
        )
        ok(
            "B: * and the session still carries its version, so a later build "
            f"can say 'older than this' rather than misreading — {stored['version']}",
            isinstance(stored.get("version"), int),
        )


def _second_run(root: Path) -> None:
    banner("C — a SECOND PROCESS opens with the palette still away")
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        at = placement(app)
        ok(
            f"C: ***** the palette is still away — {at['at']}. This is the whole "
            "property, and no single process can show it",
            at["at"]["folded"] is True,
        )
        ok(
            f"C: ***** and the tool SAYS it was restored — restored="
            f"{at.get('restored')}. `at` alone cannot: folded is the same bit "
            "whether this build opens that way, a person just folded it, or a "
            "stored session was believed",
            at.get("restored") is True,
        )
        ok(
            f"C: ** while `opens` still reports what the BUILD declares, so the "
            f"two facts stay apart — {at['opens']}",
            at["opens"]["folded"] is False,
        )
        drawn = app.snapshot(source="paint")
        ok(
            "C: * and the screen actually painted the folded form — the strip "
            "is what a reader has to grab to bring it back",
            "shell.palette.strip" in json.dumps(drawn),
        )

        banner("E — bringing it back is written too")
        app.invoke(f"{EXT}/palette", "unfold")
        settle(app)
        kept = (blob(root) or {}).get("chrome", {}).get(PALETTE)
        ok(
            f"E: ***** unfolding is stored as well — {kept}. Storing only the "
            "fold would leave a person who restores the panel finding it away "
            "again tomorrow, which is the same defect in the other direction",
            kept is not None and kept["folded"] is False,
        )


def _refused_session(root: Path) -> None:
    banner("F — a session this build cannot honour is replaced, and explained")
    # An edge the palette is not on. Measured at R1908: this panel is
    # `allowed: []` and `Resize::Fixed`, so no width and no fold is refusable —
    # a stored EDGE is the one thing its policy can refuse, and it refuses it
    # because nothing on the screen could honour it.
    #
    # ⚠ The edge is EDITED IN THE FILE THAT WAS WRITTEN rather than composed
    # here. The first draft wrote `"edge": "left"` by hand; the stored spelling
    # is the type's variant name, so the whole session failed to PARSE and the
    # tool said "saved layouts could not be read" — a different sentence for a
    # different event, and the walk caught the difference. A fixture invented
    # beside a serialiser agrees with whatever the author imagined it does.
    stored = blob(root) or {}
    kept = stored.get("chrome", {}).get(PALETTE)
    ok(
        f"F: the previous run wrote a placement to edit — {kept}",
        kept is not None and "edge" in kept,
    )
    edges = ["Top", "Bottom", "Left", "Right"]
    ok(
        f"F: ** and its edge is spelled as one of this type's own variants — "
        f"{kept['edge']!r}. If this ever fails the session shape moved and the "
        "assertions below would be testing a parse failure instead",
        kept["edge"] in edges,
    )
    kept["edge"] = next(e for e in edges if e != kept["edge"])
    (root / KEY).write_text(json.dumps(stored), encoding="utf-8")

    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        at = placement(app)
        ok(
            f"F: ***** the palette opens where the SPECIFICATION says, not where "
            f"an unhonourable session asked — {at['at']}. A boot has to produce "
            "a screen, so this cannot be a failure",
            at["at"] == at["opens"],
        )
        ok(
            f"F: ** and it does NOT claim to be restored — restored="
            f"{at.get('restored')}",
            at.get("restored") is False,
        )
        said = app.query(f"{EXT}/toast")
        ok(
            f"F: ***** the refusal REACHES THE PERSON — {said!r}. A silent "
            "fallback is a reader seeing the wrong panel with no way to learn "
            "why, which is the defect R1902 closed, one step on",
            isinstance(said, str) and "could not open where you left it" in said,
        )


if __name__ == "__main__":
    run_demo("r1908 a put-away palette stays put away", body)
