#!/usr/bin/env python3
"""R1972 — the north star's two end conditions, as commands that ANSWER.

The standing goal states condition (A) with a pair of reproduction commands:

    grep -n 'title:' examples/hello-analyzer-shell/src/spec.rs
    grep -n 'const TABS' examples/hello-analyzer-shell/src/main.rs

Measured at R1972, **neither answers what it claims to**, and the second is the
sharper case because it once did. `const TABS` was real in that file: added by
`126bd5aa` (R1649, *the shell matches the reference tool*) and **removed by
`47b50070` (R1946, *a list says which reference it is true of*)** — the round
that replaced a hand-kept list with a declaration. The condition kept greping
for what a refactor had taken away, so it now prints nothing, and nothing is
indistinguishable from a satisfied condition.

    git log -S 'const TABS' --oneline --all -- examples/hello-analyzer-shell/src/main.rs

answers those two commits; `git grep -n 'const TABS'` answers one unrelated
example (`hello-tab-reorder`), which is why a bare repository-wide grep does
not settle it either. The first command answers **26**, because `title:` is a
field of four different specs in that file and only eight of those rows are
destinations.

A condition nobody can evaluate is a condition nobody checks, which is this
repository's standing rule about prose. So the two greps are replaced by this,
and what it asks is derived rather than remembered:

* the DECLARATION is `spec::RAIL`, parsed here from the source rather than
  counted by hand, and it is the population both gates sweep;
* condition (A) is judged by two named gates, and this asserts they EXIST
  (a gate deleted or renamed is the failure mode a count cannot see) and,
  with `--run`, runs them;
* condition (B) is judged over the debt files the goal names, reporting for
  each whether it carries a repayment section and whether it is `closed_by:
  person` — because those five are repaid by the loop and closed by a person,
  so "still open" is the CORRECT state and a checker that reads `status: open`
  as failure would answer no forever.

Exit status is the verdict: 0 when both conditions hold as far as a command
can tell, 1 otherwise. `--json` prints the same as a machine-readable object.

Run from the workspace root:

    python3 tools/north_star.py
    python3 tools/north_star.py --run     # also runs the gates (cargo)
    python3 tools/north_star.py --json
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

SHELL = Path("examples/hello-analyzer-shell/src")
MEMORY = Path.home() / ".claude/projects/-home-coin-pinion/memory"

#: The gates that judge condition (A). Named here because a gate that is
#: deleted or renamed is exactly the drift this file exists to catch, and a
#: count of destinations cannot see it.
GATES_A: tuple[tuple[str, str], ...] = (
    ("painted.rs", "r1695_each_destination_paints_the_regions_the_specification_gives_it"),
    ("tests.rs", "r1946_every_declared_seat_is_open_with_a_page_or_closed_with_a_reason"),
)

#: Condition (B)'s population: the defects a person reported on 2026-09-01 by
#: looking at the running window. Derived from the front matter rather than
#: listed, so a sixth report joins by being written.
OPENED_BY_THE_REPORT = "opened: R1945"
CENTRING = "debt-text-is-not-centred-in-the-box-that-holds-it"


def declared_seats() -> list[str]:
    """The destinations `spec::RAIL` declares, parsed from the declaration."""
    src = (SHELL / "spec.rs").read_text(encoding="utf-8")
    body = re.search(r"pub const RAIL: &\[RailSpec\] = &\[(.*?)\n\];", src, re.S)
    if body is None:
        raise SystemExit("spec.rs declares no `RAIL`: the population is gone")
    return re.findall(r'key:\s*"([^"]+)"', body.group(1))


def condition_a(run_gates: bool) -> dict:
    seats = declared_seats()
    missing = []
    for file_name, gate in GATES_A:
        if gate not in (SHELL / file_name).read_text(encoding="utf-8"):
            missing.append(f"{file_name}::{gate}")
    out = {
        "declared": len(seats),
        "seats": seats,
        "gates": [g for _, g in GATES_A],
        "gates_missing": missing,
        "ran": False,
        "ok": not missing and len(seats) > 0,
    }
    if run_gates and out["ok"]:
        # One invocation, both gates: `cargo test` takes a substring filter and
        # `r19` reaches neither by accident — the two names share no prefix, so
        # they are run by name, one after the other.
        for _, gate in GATES_A:
            done = subprocess.run(
                ["cargo", "test", "-p", "hello-analyzer-shell", gate],
                capture_output=True,
                text=True,
                check=False,
            )
            if done.returncode != 0:
                out["ok"] = False
                out["failed_gate"] = gate
                out["said"] = done.stdout[-800:]
                break
        out["ran"] = True
    return out


def condition_b() -> dict:
    if not MEMORY.is_dir():
        return {"ok": False, "why": f"no memory directory at {MEMORY}"}
    files = sorted(
        p for p in MEMORY.glob("debt-*.md")
        if OPENED_BY_THE_REPORT in p.read_text(encoding="utf-8")
    )
    centring = MEMORY / f"{CENTRING}.md"
    if centring.exists() and centring not in files:
        files.append(centring)
    rows = []
    for p in files:
        text = p.read_text(encoding="utf-8")
        rows.append(
            {
                "debt": p.stem,
                # A repayment section, by the shape this repository writes them.
                "repaid": bool(re.search(r"^## .*(상환|✅)", text, re.M)),
                # `closed_by: person` is why `status: open` is CORRECT here.
                "person_closes": "closed_by: person" in text
                or "사람이 닫는다" in text
                or "closed_by` 이므로" in text,
            }
        )
    return {
        "population": len(rows),
        "rows": rows,
        "unrepaid": [r["debt"] for r in rows if not r["repaid"]],
        "ok": bool(rows) and all(r["repaid"] for r in rows),
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--run", action="store_true", help="also run condition (A)'s gates")
    ap.add_argument("--json", action="store_true", help="machine-readable")
    args = ap.parse_args()

    a = condition_a(args.run)
    b = condition_b()
    verdict = "YES" if a["ok"] and b["ok"] else "NO"

    if args.json:
        print(json.dumps({"verdict": verdict, "A": a, "B": b}, indent=2, ensure_ascii=False))
        return 0 if verdict == "YES" else 1

    print(f"north star: {verdict}")
    ran = " (gates run)" if a["ran"] else " (gates named, not run — pass --run)"
    print(
        f"  (A) {a['declared']} declared destination(s){ran}: "
        f"{'ok' if a['ok'] else 'NO'}"
    )
    for gate in a["gates"]:
        print(f"        judged by {gate}")
    for gone in a["gates_missing"]:
        print(f"        MISSING {gone}")
    if "failed_gate" in a:
        print(f"        FAILED  {a['failed_gate']}")
    print(f"  (B) {b.get('population', 0)} reported defect(s): {'ok' if b['ok'] else 'NO'}")
    for row in b.get("rows", []):
        mark = "repaid" if row["repaid"] else "NOT REPAID"
        who = "person closes" if row["person_closes"] else "loop may close"
        print(f"        {mark:10} {who:15} {row['debt']}")
    if b.get("unrepaid"):
        print(f"        owed: {', '.join(b['unrepaid'])}")
    print(
        "  note: (B)'s files stay `status: open` on purpose — the loop repays "
        "them and a person closes them after looking at the window."
    )
    return 0 if verdict == "YES" else 1


if __name__ == "__main__":
    sys.exit(main())
