#!/usr/bin/env python3
"""R2006 §5.2 §5.11 — **a saved graph says when it was written**, and the load
path brings it up to date one step at a time.

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
This drives the capability the reference-census row
`schema::BackwardCompatibilityNodeConversion` names, on **screen A** — the node
lab, mounted whole into `hello-analyzer-shell`, which is the one screen here
that really writes and reads an archive.

# ★★★★★ The four measurements that decided what was built

Read from the engine's own source:

1. **The hook carries no version.** The virtual takes a graph and a bool,
   nothing more, so every implementor fetches a version for itself out of the
   serialisation linker. Of the **two** that implement it, only ONE does — the
   other runs its conversions unconditionally on every load, forever, for every
   document however new.
2. **The one that does branches instead of composing.** It is written
   `if (v < 21) { four conversions } else if (v < 24) { one }`, and the
   declaration of step 24 carries a comment saying documents brought up to date
   by step 21 may end up with a wrong default for one parameter. So step 24
   exists to repair what step 21 produces — and a document at version 10 takes
   step 21 and is then excluded by the `else` from the repair it just earned.
3. **It answers `void`**, writing failures to a warning log, so *nothing was
   needed* and *four things happened* reach a caller the same.
4. **Its safe-changes parameter is `true` at its only call site**, so the other
   mode is unreachable.

# What this walk holds

  (A) the journey reaches the node lab, and the history register answers.
  (B) ★★★★★ TWO versions are published, not one — the archive FORMAT's and this
      screen's own vocabulary's, which are two histories with two owners.
  (C) ★★★★★ a saved file carries both, so a reader knows what to migrate FROM.
  (D) ★★★★★ asking what a migration WOULD do does not do it — the register
      answers on a scratch copy, so reading the canvas twice gives one answer.
  (E) ★ this screen's vocabulary has never moved, and the register SAYS so
      rather than staying silent; the load path is wired for the day it does.
  (F) ★★★★★ a real round trip through save and open still lands the same cards,
      and the sentence a person reads does not claim a rewrite that did not
      happen.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r2006_a_saved_graph_says_when_it_was_written.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, run_demo  # noqa: E402

SHELL = "hello-analyzer-shell"
EXT = "/external"
SEAT = "lab"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def js(value):
    return json.loads(value) if isinstance(value, str) else value


def surface_of(app: RpcSubprocess, seat: str) -> str:
    published = js(app.query(f"{EXT}/destinations"))
    row = next(row for row in published["destinations"] if row["key"] == seat)
    return row["screen"]["address"]


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        app.intervene(f"{EXT}/nav", SEAT)
        app.tick_ms(16)
        ok(
            "the journey reaches the node lab, so what follows is about the "
            "ASSEMBLED tool",
            app.query(f"{EXT}/nav") == SEAT,
        )
        surface = surface_of(app, SEAT)

        banner("A — the history register answers")
        history = js(app.query(f"{surface}/history"))
        ok(
            f"A: it has both versions and the would-be migration — "
            f"{sorted(history)}",
            {"revision", "taxonomy", "would"} <= set(history),
        )

        banner("B — ★★★★★ TWO versions, not one")
        ok(
            f"B: ★★★★★ the archive FORMAT's revision and this screen's own "
            f"vocabulary version are published apart — revision="
            f"{history['revision']}, taxonomy={history['taxonomy']}",
            isinstance(history["revision"], int)
            and isinstance(history["taxonomy"], int),
        )
        ok(
            "B: ★ the format's revision is a real version, so the pair is not "
            "two names for one number",
            history["revision"] >= 1,
        )

        banner("C — ★★★★★ a saved file carries both")
        text = app.query(f"{surface}/archive")
        saved = json.loads(text)
        ok(
            f"C: ★★★★★ the file stamps the FORMAT's revision — "
            f"{saved.get('revision')!r}",
            saved.get("revision") == history["revision"],
        )
        ok(
            f"C: ★★★★★ and the TAXONOMY's version beside it, which is what a "
            f"reader migrates FROM — {saved.get('taxonomy', 0)!r}",
            saved.get("taxonomy", 0) == history["taxonomy"],
        )

        banner("D — ★★★★★ asking what a migration WOULD do does not do it")
        cards_before = {
            row["card"] for row in js(app.query(f"{surface}/stand_ins"))["cards"]
        }
        again = js(app.query(f"{surface}/history"))
        cards_after = {
            row["card"] for row in js(app.query(f"{surface}/stand_ins"))["cards"]
        }
        ok(
            "D: ★★★★★ the register answers the same thing twice, so it is a "
            "question and not an act",
            again == history,
        )
        ok(
            f"D: ★★★★★ and the canvas is untouched by having been asked — "
            f"{len(cards_before)} cards",
            cards_before == cards_after and len(cards_before) > 1,
        )

        banner("E — ★ this vocabulary has never moved, and it SAYS so")
        would = history["would"]
        ok(
            f"E: ★ the register states it as a fact rather than staying silent "
            f"— from={would['from']}, to={would['to']}",
            would["from"] == 0 and would["to"] == history["taxonomy"],
        )
        ok(
            f"E: ★ so a migration from the oldest possible file would run no "
            f"step and rewrite no card — steps={would['steps']}, "
            f"cards={would['cards']}",
            would["steps"] == [] and would["cards"] == 0,
        )

        banner("F — ★★★★★ a real round trip, and the sentence does not overclaim")
        app.invoke(f"{surface}/save_graph", "")
        app.tick_ms(16)
        opened = app.invoke(f"{surface}/open_graph", "")
        app.tick_ms(16)
        ok(f"F: the graph came back — {opened!r}", "opened" in str(opened))
        ok(
            f"F: ★★★★★ and the sentence does NOT claim a rewrite, because none "
            f"happened — a load path that said so anyway would be worse than "
            f"the reference's silence: {opened!r}",
            "brought up from" not in str(opened),
        )
        back = {row["card"] for row in js(app.query(f"{surface}/stand_ins"))["cards"]}
        ok(
            f"F: ★ the same cards are on the canvas — {len(back)} vs "
            f"{len(cards_before)}",
            back == cards_before,
        )
        ok(
            "F: ★ and the history register still answers the same, so a round "
            "trip did not move either version",
            js(app.query(f"{surface}/history")) == history,
        )

        print(f"\n{len(CHECKS)} check(s) held.")


if __name__ == "__main__":
    run_demo("r2006 a saved graph says when it was written", body)
