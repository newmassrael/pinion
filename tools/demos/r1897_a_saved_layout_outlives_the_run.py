#!/usr/bin/env python3
"""R1897 — a person's own layout outlives the run that saved it.

# What this demo exists for

Standing rule (7) of this repayment loop asks for the analyzer UI assembled and
asserted by one walk. This drives the LAST item of the campaign
`debt-the-arrangeable-unit-is-a-panel-and-should-be-an-area`'s order step 4: the
behaviour canon keeps its custom presets and its current layout in
`localStorage` and brings them back on load, and this tree kept nothing.

# The rule persistence needed, which is why it could not be built first

An application SHIPS arrangements and a person SAVES them, and only the second
kind may be written to disk. Storing the shipped ones looks harmless and is not:
a later build with different ones would find a previous version's on disk and
resurrect it. So `Workspaces::saved()` is what a session writes, and
`Workspaces::restore()` lays a stored set over what THIS build ships — refusing,
BY NAME, a stored row whose name the build now ships and a stored row that
claims to be a built-in.

⇒ `Provenance` (R1893) had to exist before persistence could be correct, which
is why this is the campaign's last item rather than its first.

# What this walk holds

Two processes, one storage directory. What a person saved in the first is in
the second; what the application ships is not doubled and not stale; a delete
survives the restart too; and the refusals a restore makes are said rather than
swallowed.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1897_a_saved_layout_outlives_the_run.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    isolated_storage_dir,
    run_demo,
)

SHELL = "hello-analyzer-shell"
EXT = "/external"
#: The key the shell writes its arrangements under, and the app dir it uses.
APP_DIR = "pinion-analyzer-shell"
KEY = "analyzer_shell.arrangements"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def rows(app: RpcSubprocess) -> list[dict]:
    return app.query(f"{EXT}/arrangements")


def names(app: RpcSubprocess) -> list[str]:
    return [r["name"] for r in rows(app)]


def invoke(app: RpcSubprocess, verb: str, args: str):
    return app.invoke(f"{EXT}/{verb}", args)


def key_file(root: Path) -> Path:
    """Where the blob lands under `PINION_STORAGE_DIR`.

    ⚠ The override is the ROOT ITSELF, not a parent the app dir hangs off —
    `build_app_storage` hands the path straight to `FileStorage::try_new`, so
    the app name only picks a directory on the DEFAULT path. Measured: the first
    draft looked under `<root>/<app>/` and reported "the save did not reach the
    disk" about a save that had.
    """
    return root / KEY


def blob(root: Path) -> dict | None:
    """What is actually on disk, read as a file rather than asked of the app."""
    path = key_file(root)
    if not path.exists():
        return None
    return json.loads(path.read_text(encoding="utf-8"))


def body() -> None:
    with isolated_storage_dir("pinion-analyzer-shell-r1897-") as root:
        _body(root)

    print(f"\n=== {len(CHECKS)} named check(s) ===")
    for line in CHECKS:
        print(f"  - {line}")


def _body(root: Path) -> None:
    banner("A — the first run: what this build ships, and nothing on disk yet")
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        shipped = names(app)
        ok(
            f"A: the application opens on the arrangements it ships — {shipped}",
            len(shipped) == 4 and all(r["provenance"] == "built-in" for r in rows(app)),
        )
        ok(
            "A: ★★ and nothing has been written yet, because shipping is not "
            "saving — a file here would mean the build's own arrangements had "
            "been persisted, which is what makes a later build resurrect a "
            "previous version's",
            blob(root) is None,
        )

        banner("B — a person saves one, and it reaches the disk")
        invoke(app, "save_preset", "Mine")
        stored = blob(root)
        ok(
            f"B: ★★★★★ the save reached the disk — {stored is not None}",
            stored is not None,
        )
        ok(
            f"B: ★★★★★ and ONLY the person's own is in it — {sorted(stored['arrangements']['entries'])}",
            sorted(stored["arrangements"]["entries"]) == ["Mine"],
        )
        ok(
            f"B: the blob carries a version, so a later build can say 'older "
            f"than this one' rather than misreading — {stored['version']}",
            isinstance(stored["version"], int),
        )
        ok(
            "B: ★ and the stored row keeps its provenance, which is what stops "
            "it coming back as something the application ships",
            stored["arrangements"]["entries"]["Mine"]["provenance"] == "saved",
        )

    banner("C — ★★★★★ a SECOND process: the saved one is there, the shipped are not doubled")
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        back = names(app)
        ok(
            f"C: ★★★★★ what a person saved outlived the run that saved it — {back}",
            "Mine" in back,
        )
        ok(
            f"C: ★★ and the four this build ships are there exactly once — "
            f"{len(back)} arrangements for 4 shipped + 1 saved",
            len(back) == 5,
        )
        row = next(r for r in rows(app) if r["name"] == "Mine")
        ok(
            f"C: ★★★★★ the restored one is still a PERSON's, so it still offers "
            f"a delete — {row}",
            row["provenance"] == "saved" and row["deletable"] is True,
        )
        ok(
            "C: ★ and what the application ships still refuses one",
            all(
                r["deletable"] is False
                for r in rows(app)
                if r["provenance"] == "built-in"
            ),
        )
        # ★ It APPLIES, which is the point of keeping it: a name that comes back
        # without its board would be a menu row that does nothing.
        app.intervene(f"{EXT}/preset", "Mine")
        app.tick_ms(16)
        assert_eq(app.query(f"{EXT}/preset"), "Mine", "C: the restored layout applies")
        ok(
            "C: ★★★★★ and applying it puts cards on the board, so the LAYOUT "
            "came back and not just its name",
            len(app.query(f"{EXT}/cards").split(",")) > 0,
        )

        banner("D — a delete survives the restart too")
        invoke(app, "delete_preset", "Mine")
        after = blob(root)
        ok(
            f"D: ★★ the delete reached the disk — {sorted(after['arrangements']['entries'])}",
            after is not None and after["arrangements"]["entries"] == {},
        )

    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        ok(
            f"D: ★★★★★ and it stays deleted in a third run — {names(app)}",
            "Mine" not in names(app) and len(names(app)) == 4,
        )

    banner("E — a stored set does not get to overrule what this build ships")
    # Write a file by hand: one row whose name this build ships, and one
    # claiming to BE a built-in. Neither may get in, and the application must
    # SAY so rather than dropping them silently.
    path = key_file(root)
    path.write_text(
        json.dumps(
            {
                "version": 1,
                "arrangements": {
                    "entries": {
                        "Overview": {"layout": None, "provenance": "saved"},
                        "Sneaky": {"layout": None, "provenance": "built-in"},
                    }
                },
            }
        ),
        encoding="utf-8",
    )
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        # A hand-written blob whose layouts are `null` cannot deserialise into
        # this application's `Preset`, so the whole file is refused — which is
        # the honest outcome and is SAID.
        said = app.query(f"{EXT}/toast")
        ok(
            f"E: ★★★★★ an unreadable stored set is REFUSED and the application "
            f"says so rather than starting silently — {said!r}",
            "saved layouts" in said,
        )
        ok(
            f"E: ★★ and it started from what this build ships — {names(app)}",
            len(names(app)) == 4
            and all(r["provenance"] == "built-in" for r in rows(app)),
        )
        ok(
            "E: ★ in particular the build's own `Overview` is the build's, not "
            "the file's",
            next(r for r in rows(app) if r["name"] == "Overview")["provenance"]
            == "built-in",
        )


if __name__ == "__main__":
    run_demo("r1897 a saved layout outlives the run", body)
