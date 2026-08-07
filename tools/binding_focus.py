"""R1588 — which bindings present an operable control, derived in ONE place.

Two demos ask different questions of the same set of bindings. R1518's asks
whether the AT focus target and the tree's focus flag are one answer; R1570.1's
asks whether a declared interactive role is a keyboard focus stop. Different
questions, and the **population must be the same** — otherwise a binding is
covered by one gate and invisible to the other, and which one it is depends on
whoever last edited a list.

That is not hypothetical. R1518's population was a hand-written list of fourteen
composite names, and a curated population cannot find an absence: a binding with
zero focus stops is simply never walked, which is how
`debt-interactive-role-without-focus-stop` stayed invisible in seventeen
bindings for some fifty rounds. R1570.1 did not fix that list — it added a
SECOND sweep with a derived population, so the tree ended up with one curated
sweep and one derived one. This module is the merge: the derivation lives here,
both sweeps import it, and there is no second place for it to drift to.

# What this can and cannot see

The derivation is a **source scan**, so it is only as wide as the thing it
derives from — the lesson R1570.5 paid for when scoping the scan to the
`#[widget(...)]` attribute made its verdict read as total while covering a
quarter of the subject. Two sources are read here for that reason: the attribute
(precise — it names a `tag` that can be asked a pointed question) and every
`AriaRole::X` the binding constructs (wider, and the larger half).

It still cannot see a binding whose focus stop comes from a
`pinion-widget-paint` helper while the binding itself constructs no interactive
role. Censusing THAT would mean asking every binding rather than reading it, and
that was measured at R1588 and does not fit: 218 bindings at ~0.6s each is
already most of the sweep's 180s per-demo budget, and a binding that does not
answer RPC at all — measured at four in the first twenty — costs a full request
timeout rather than a boot. So the limit is stated here rather than hidden, and
the honest population is "bindings that SAY they present an operable control".
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from pathlib import Path

WORKSPACE = Path(__file__).resolve().parent.parent

#: ARIA roles that denote an OPERABLE control. A role outside this set is a
#: structural or live-region role and carries no focus obligation, so listing
#: the interactive ones (rather than excluding the structural ones) keeps a role
#: added later from being silently swept in.
INTERACTIVE_ROLES = {"Button", "CheckBox", "Switch", "RadioButton", "Listbox"}

#: TUI siblings. They are terminal bindings driven through a different entry
#: point, and the RPC harness's stdin handshake gets a broken pipe rather than a
#: refusal. Excluded by NAME and published, so the exclusion is a decision on
#: the page instead of a silent gap; their GUI siblings (`hello-button`,
#: `hello-commands`, `hello-toggle`) are in the population and share the
#: `WidgetCore` body under test.
NO_RPC_STDIN = frozenset({"hello-button-tui", "hello-commands-tui", "hello-toggle-tui"})


@dataclass(frozen=True)
class Declared:
    """A binding whose `#[widget(...)]` names both an interactive role and the
    tag that carries it, so it can be asked whether THAT tag is focusable."""

    name: str
    tag: str
    role: str


@dataclass
class Population:
    """Every binding that says it presents an operable control, plus the edges.

    The exclusions are carried rather than dropped: a population that reports
    only what it kept cannot be audited, which is the whole defect this module
    exists to remove.
    """

    #: Bindings that declare `role` + `tag` in their widget attribute.
    declared: list[Declared] = field(default_factory=list)
    #: Bindings that construct an interactive `AriaRole` in a hand-written
    #: `WidgetA11y` impl. Only "does this window have a stop at all" can be
    #: asked of these — weaker, and still enough for the defect the gates exist
    #: for.
    hand_written: list[str] = field(default_factory=list)
    #: `(name, role)` for a widget attribute whose role is not operable.
    excluded_by_role: list[tuple[str, str]] = field(default_factory=list)
    #: Bindings held out by name, with the reason.
    excluded_by_name: list[tuple[str, str]] = field(default_factory=list)

    @property
    def walkable(self) -> list[str]:
        """Every binding either gate should visit, ascending and without
        repeats. One name per process, because booting a binding twice is what
        pushed R1570.1 past the sweep's budget."""
        return sorted({d.name for d in self.declared} | set(self.hand_written))

    def summary(self) -> str:
        return (
            f"{len(self.walkable)} binding(s): {len(self.declared)} declared, "
            f"{len(self.hand_written)} hand-written; excluded "
            f"{len(self.excluded_by_role)} by role, "
            f"{len(self.excluded_by_name)} by name"
        )


def _sources() -> list[tuple[str, str]]:
    return [
        (p.parts[-3], p.read_text())
        for p in sorted((WORKSPACE / "examples").glob("*/src/main.rs"))
    ]


def interactive_bindings() -> Population:
    """Derive the population. See the module docs for what it cannot see."""
    out = Population(
        excluded_by_name=[(n, "no RPC stdin — TUI entry point") for n in sorted(NO_RPC_STDIN)]
    )
    for name, src in _sources():
        attr = re.search(r"#\[widget\((?P<a>.*?)\n\)\]", src, re.S)
        if attr is not None:
            body = attr.group("a")
            # Anchored to line starts: an unanchored match reads `tag = "..."`
            # out of the module's own doc comments, which is how R1570.1's first
            # draft picked the wrong tag for two bindings.
            role = re.search(r"^\s*role\s*=\s*(\w+)", body, re.M)
            tag = re.search(r'^\s*tag\s*=\s*"([^"]+)"', body, re.M)
            if role is not None and tag is not None:
                if role.group(1) in INTERACTIVE_ROLES and name not in NO_RPC_STDIN:
                    out.declared.append(Declared(name, tag.group(1), role.group(1)))
                elif role.group(1) not in INTERACTIVE_ROLES:
                    out.excluded_by_role.append((name, role.group(1)))
                continue
        # No attribute, or one that declares no role: fall back to the roles the
        # binding CONSTRUCTS. `AriaRole::X` in the source is the only statement
        # a hand-written `WidgetA11y` impl makes that this scan can read.
        roles = set(re.findall(r"AriaRole::(\w+)", src))
        if roles & INTERACTIVE_ROLES and name not in NO_RPC_STDIN:
            out.hand_written.append(name)
    return out


def build_command() -> str:
    """The `cargo build` line that makes every walked binding runnable.

    R1588 — derived for the same reason the population is. Both focus demos
    carried a hand-written `cargo build --release -p …` line in their docstring,
    which is a curated list wearing a build command's clothes: it drifts the
    moment the population grows, and then a demo run measures whichever
    binaries happen to be lying in `target/release`.

    That is not hypothetical either. Measured at R1588 while widening the
    population: every example binary in this tree predated the R1583 change to
    `pinion-rpc` that the widened sweep was about to assert on, except the
    handful rebuilt since — so a `PINION_ASSUME_BUILT=1` run was quietly
    comparing binaries from two different commits, and the first assertion to
    read a field R1583 added failed on a binding whose binary simply predated
    it.
    """
    pop = interactive_bindings()
    return "cargo build --release " + " ".join(f"-p {n}" for n in pop.walkable)


if __name__ == "__main__":
    population = interactive_bindings()
    print(population.summary())
    print(build_command())
