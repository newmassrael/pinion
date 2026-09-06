#!/usr/bin/env python3
"""wire-only-deps — a consumer that speaks only the JSON-RPC wire links no font stack.

## Why this exists

`pinion-text` depends on parley, and parley's default `system` feature forwards
to `fontique/system`, which on Linux pulls `yeslogic-fontconfig-sys`. That crate
needs the **fontconfig development headers at build time**, so anything that
reaches `pinion-text` cannot be built on a machine without them. A consumer that
only speaks pinion's JSON-RPC wire reaches it through `pinion-rpc`, and its CI
failed with

    The system library `fontconfig` required by crate
    `yeslogic-fontconfig-sys` was not found.

on Linux only — macOS takes Core Text and never links it, which is what made the
failure look like a CI-runner problem rather than a dependency one.

R1659 fixed it by declaring parley `default-features = false` at the workspace
root and re-enabling the source through a `system-fonts` feature that is ON by
default, so nothing in this tree changed and an external consumer can decline it.

## Why a gate and not a comment

**The reported fix would not have worked, and the reason is exactly what this
checks.** The report proposed feature-gating the three text dependencies
`pinion-rpc` declares. Measured, `pinion-rpc` reaches `pinion-text` by TWO
mandatory paths — its own declaration, and again through `pinion-runtime` — so
gating the visible one leaves the font stack linked and the build still broken.
The property is not "this crate declared it carefully"; it is "**every** path
does", and a path is added by an ordinary-looking `Cargo.toml` line in a crate
nobody was thinking about.

That is a closure property over a graph, which is the shape no reviewer holds in
their head and no comment can state. So it is computed.

## What it checks

Walking `pinion-rpc`'s path-dependency closure across the workspace:

  1. every edge INTO `pinion-text` is declared `default-features = false`;
  2. every crate on such a path forwards a `system-fonts` feature, so turning
     the source back on is one word at the top and not a rediscovery;
  3. `pinion-text`'s `system-fonts` forwards to `parley/system`, and the
     workspace's parley dep is `default-features = false` — the two halves that
     make the whole thing mean anything.

Check 2 is what makes 1 safe to satisfy: a crate could pass 1 by taking
`pinion-text` without defaults and never offering a way back, which would
silently take system fonts away from every consumer of THAT crate.

## What it does not check

That the resolved dependency graph is actually font-free. That needs a real
`cargo tree` against an external consumer manifest — outside this repository, on
a machine with a populated registry — and a gate that needs the network is a
gate that fails open. R1659 ran that measurement by hand, both directions, and
recorded it in the round; this keeps the manifest conditions that made it true.

Usage:

    python3 tools/wire_only_deps.py --check      # the gate
    python3 tools/wire_only_deps.py --selftest   # the gate's own tests
"""

from __future__ import annotations

import argparse
import sys
import tomllib
from pathlib import Path

#: The crate whose consumers are the point: a consumer speaking only the wire
#: reaches the text stack through here.
WIRE_CRATE = "pinion-rpc"

#: The crate that carries the shaping stack, and with it the fontconfig
#: build requirement.
TEXT_CRATE = "pinion-text"

#: The feature every crate on a path to the text stack must forward, so a
#: consumer turns the source back on in one word.
SWITCH = "system-fonts"

DEP_TABLES = ("dependencies", "dev-dependencies", "build-dependencies")


def crate_manifests(repo: Path) -> dict[str, tuple[Path, dict]]:
    """Every workspace crate, by package name."""
    out: dict[str, tuple[Path, dict]] = {}
    for manifest in sorted((repo / "crates").glob("*/Cargo.toml")):
        data = tomllib.loads(manifest.read_text(encoding="utf-8"))
        name = data.get("package", {}).get("name")
        if name:
            out[name] = (manifest, data)
    return out


def normal_deps(data: dict) -> dict[str, dict]:
    """`[dependencies]` only — a dev-dep does not reach a consumer's build."""
    deps = data.get("dependencies", {})
    return {k: (v if isinstance(v, dict) else {}) for k, v in deps.items()}


def paths_to_text(
    crates: dict[str, tuple[Path, dict]], start: str
) -> list[list[str]]:
    """Every simple path from `start` to the text crate, through workspace crates.

    Depth-first over normal dependencies. Cycles cannot occur in a cargo graph,
    so no visited set is needed beyond the current path.
    """
    found: list[list[str]] = []

    def walk(node: str, path: list[str]) -> None:
        if node not in crates:
            return
        for dep in normal_deps(crates[node][1]):
            if dep in path:
                continue
            if dep == TEXT_CRATE:
                found.append([*path, node, dep])
            elif dep in crates:
                walk(dep, [*path, node])

    walk(start, [])
    return found


def check(repo: Path) -> list[str]:
    """Return the list of violations; empty means the property holds."""
    problems: list[str] = []
    crates = crate_manifests(repo)

    if TEXT_CRATE not in crates or WIRE_CRATE not in crates:
        return [f"cannot find {WIRE_CRATE} / {TEXT_CRATE} under {repo}/crates"]

    # (3) the two halves that give the switch its meaning.
    root = tomllib.loads((repo / "Cargo.toml").read_text(encoding="utf-8"))
    parley = root.get("workspace", {}).get("dependencies", {}).get("parley")
    if not isinstance(parley, dict) or parley.get("default-features") is not False:
        problems.append(
            "workspace parley dep must be `default-features = false` — with its "
            "default `system` on, every switch below is decoration"
        )
    text_feats = crates[TEXT_CRATE][1].get("features", {})
    if "parley/system" not in text_feats.get(SWITCH, []):
        problems.append(
            f"{TEXT_CRATE} `{SWITCH}` must forward to `parley/system`, or nothing "
            "can turn the font source back on"
        )
    if SWITCH not in text_feats.get("default", []):
        problems.append(
            f"{TEXT_CRATE} default must include `{SWITCH}` — off by default would "
            "take system fonts from every existing consumer silently"
        )

    # (1) + (2) over every path the wire crate has to the text stack.
    paths = paths_to_text(crates, WIRE_CRATE)
    if not paths:
        problems.append(
            f"no path from {WIRE_CRATE} to {TEXT_CRATE} — this gate is checking "
            "nothing; if the dependency really went away, retire the gate"
        )
    for path in paths:
        for holder, dep in zip(path, path[1:], strict=False):
            spec = normal_deps(crates[holder][1]).get(dep, {})
            feats = crates[holder][1].get("features", {})
            declined = spec.get("default-features") is False

            if dep == TEXT_CRATE and not declined:
                problems.append(
                    f"{holder} takes {dep} with default features — path "
                    f"{' -> '.join(path)} keeps the font stack linked for a "
                    "wire-only consumer"
                )
            # (2) declining a dependency's defaults is only half a decision.
            # The other half is a way to ask for them back, and it has to name
            # THIS edge: `pinion-rpc` declining `pinion-runtime`'s defaults and
            # forwarding only to `pinion-text` would leave the runtime's own
            # path to the text stack off in a DEFAULT build — system fonts lost
            # from a build nobody asked to change.
            if declined and f"{dep}/{SWITCH}" not in feats.get(SWITCH, []):
                problems.append(
                    f"{holder} declines {dep}'s default features but its "
                    f"`{SWITCH}` does not forward `{dep}/{SWITCH}` — that edge "
                    "can never be turned back on"
                )

        # (2, cont.) every crate between the consumer and the text stack offers
        # the switch, and offers it by default.
        for holder in path[:-1]:
            feats = crates[holder][1].get("features", {})
            if SWITCH not in feats:
                problems.append(
                    f"{holder} is on a path to {TEXT_CRATE} and declares no "
                    f"`{SWITCH}` feature — its consumers would have no way to ask "
                    "for system fonts back"
                )
            elif SWITCH not in feats.get("default", []):
                problems.append(
                    f"{holder} `{SWITCH}` is not in its default features — an "
                    "existing consumer would lose system fonts without asking"
                )
    return problems


def selftest() -> int:
    """The gate's own tests. `pre-push` runs these before trusting it."""
    failures = 0

    def case(name: str, ok: bool) -> None:
        nonlocal failures
        if not ok:
            failures += 1
            print(f"selftest FAIL: {name}", file=sys.stderr)

    fake = {
        "a": (Path("a"), {"dependencies": {"b": {}}, "features": {}}),
        "b": (Path("b"), {"dependencies": {TEXT_CRATE: {}}, "features": {}}),
        TEXT_CRATE: (Path("t"), {"dependencies": {}}),
    }
    case("a transitive path is found", paths_to_text(fake, "a") == [["a", "b", TEXT_CRATE]])

    two = {
        "a": (Path("a"), {"dependencies": {"b": {}, TEXT_CRATE: {}}}),
        "b": (Path("b"), {"dependencies": {TEXT_CRATE: {}}}),
        TEXT_CRATE: (Path("t"), {"dependencies": {}}),
    }
    case(
        "BOTH paths are found, which is the whole point",
        len(paths_to_text(two, "a")) == 2,
    )

    none = {"a": (Path("a"), {"dependencies": {"z": {}}}), TEXT_CRATE: (Path("t"), {})}
    case("no path is no path", paths_to_text(none, "a") == [])

    dev_only = {
        "a": (Path("a"), {"dev-dependencies": {TEXT_CRATE: {}}}),
        TEXT_CRATE: (Path("t"), {}),
    }
    case(
        "a dev-dependency is not a consumer's build",
        paths_to_text(dev_only, "a") == [],
    )

    # ★★★★★ R2028 — THE ORACLE, over the real workspace.
    #
    # Every case above hands `paths_to_text` a fixture manifest map, which is
    # what makes the reachability rule testable without cargo. Nothing watched
    # the function that builds the real one — and a `crate_manifests` that
    # found no crate would make this gate answer *no consumer reaches the text
    # crate* for every crate in the workspace, with every case above green.
    # `tools/oracle_census.py` counts that shape.
    real = crate_manifests(Path(__file__).resolve().parent.parent)
    case("the manifest oracle finds this workspace's crates", len(real) > 20)
    case(
        "and the crate this gate is about is one of them",
        TEXT_CRATE in real,
    )
    case(
        "each answer is a manifest that exists and the table it parsed",
        all(path.is_file() and "package" in data for path, data in real.values()),
    )
    case(
        "and the key is the package's own name, which is what a dependency cites",
        all(name == data.get("package", {}).get("name") for name, (_, data) in real.items()),
    )

    print(f"wire_only_deps selftest: {'PASS' if failures == 0 else 'FAIL'} "
          f"({failures} failure(s))")
    return 1 if failures else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--check", action="store_true", help="run the gate")
    ap.add_argument("--selftest", action="store_true", help="run the gate's own tests")
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    repo = Path(__file__).resolve().parent.parent
    problems = check(repo)
    if problems:
        print("wire-only deps: FAILED", file=sys.stderr)
        for p in problems:
            print(f"  - {p}", file=sys.stderr)
        return 1
    paths = paths_to_text(crate_manifests(repo), WIRE_CRATE)
    print(
        f"wire-only deps: OK — {len(paths)} path(s) from {WIRE_CRATE} to "
        f"{TEXT_CRATE}, every one declining the font source by default"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
