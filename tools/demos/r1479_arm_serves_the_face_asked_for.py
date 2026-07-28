#!/usr/bin/env python3
"""R1479 §5.37 §2#7 — a second shaper may only draw the face it was asked for.

pinion can run two text engines at once: parley, which selects from the platform
font database, and the opt-in self-hosted §5.37 arm (`PINION_TEXT_ENGINE=1`),
which holds exactly ONE parsed face. Until this round the arm claimed every leaf
whose *shape* it could reproduce and never looked at the leaf's family, so with
the arm on:

  * text naming a family was measured and painted in whatever face the arm
    happened to hold, and
  * an application default (`ShellConfig::with_default_font_family`, R1472) was
    overridden for every unstyled leaf — the exact text it is declared to serve.

Both are silent: the report still names the declared family while a different
face draws the glyphs. Wrong face is also a wrong *advance*, so the measured box
is wrong too — the R1472 regression this reintroduced showed up as Korean prose
that stopped folding.

`hello-app-font` declares a face and pins it by name on every row, so it is the
forcing consumer for the named half. This demo asserts over the wire:

  1. PREMISE — the arm is off by default and the binding reports the face it
     declared. The baseline really is one shaper.
  2. THE REPORT — with the arm on, `scene/snapshot` carries which face the arm
     holds (§2 #7). Qt has no answer here at all: nothing in QFontDatabase says
     which of two live shapers drew a run.
  3. THE CLAIM — with the arm holding a DIFFERENT face from the one every row
     names, every row measures identically arm-on and arm-off. The arm declined;
     parley, the shaper that can select that face, kept the text.
  4. THE ARM IS NOT MERELY OFF — a falsey opt-in reports `disabled` while a
     truthy one reports the face, so the row tracks the running arm rather than
     being a constant; and with no application face declared the rows go unpinned
     while the arm keeps reporting the same face, so the two facts are
     independent.

The fixture the application declares is CHOSEN from the arm's reported face: the
premise the claim needs is "the two shapers hold different faces", and which face
the arm picks is a property of the host's font directories. Deriving it from the
measurement instead of assuming it keeps the demo honest on any runner.

ZERO-FLAKE: no clocks, no pixels. Every assertion is a content or geometry datum
read off the published scene.

Run from the workspace root:
    python3 tools/demos/r1479_arm_serves_the_face_asked_for.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_snap,
)

EXAMPLE = "hello-app-font"

# The other half's forcing consumer: every row Korean, no row naming a family,
# and NanumGothic declared as the process default (R1472).
VN_EXAMPLE = "hello-vn-tide"
VN_EXT = "/external"
VN_LINE_TAG = "vn.line"
VN_FONT_TAG = "vn.font_state"
VN_W, VN_H = 800, 460
# Enough logical time for the typewriter to finish the line — stepped, not slept.
VN_REVEAL_MS = 60_000

# The rows the binding tags. `arm` is this round's addition.
SYSTEM_TAG = "afd_system"
FAMILIES_TAG = "afd_families"
ARM_TAG = "afd_arm"
SAMPLE_TAG = "afd_sample"
ROWS = (SYSTEM_TAG, FAMILIES_TAG, ARM_TAG, SAMPLE_TAG)

# The two faces the repo already ships as shaping fixtures, with the family each
# one's `name` table declares. The binding is pointed at whichever of them the
# self-hosted arm did NOT pick up off this host.
FIXTURES = {
    "NanumGothic": "crates/pinion-text-font/tests/fonts/NanumGothic-Regular.ttf",
    "Noto Sans": "crates/pinion-text-font/tests/fonts/NotoSans-Regular.ttf",
}

ARM_ON = {"PINION_TEXT_ENGINE": "1"}


def read_rows(env: dict[str, str]) -> dict[str, dict]:
    """Boot the binding under `env` and return `{tag: {w, h, content}}`."""
    with RpcSubprocess(EXAMPLE, env=env) as app:
        snap = wait_snap(
            app,
            lambda s: find_by_tag(s, SAMPLE_TAG) is not None,
            desc="the sample row is present in the painted scene",
        )
        out = {}
        for tag in ROWS:
            node = find_by_tag(snap, tag)
            assert node is not None, f"the row {tag} is missing from the scene"
            rect = node.get("rect") or {}
            w, h = rect.get("w"), rect.get("h")
            assert isinstance(w, int) and isinstance(h, int), (
                f"{tag} publishes an integer rect, got {rect!r}"
            )
            content = node.get("content")
            assert isinstance(content, str), f"{tag} publishes text, got {content!r}"
            out[tag] = {"w": w, "h": h, "content": content}
        return out


def layout_line_count(layout, tag: str) -> int:
    """`line_count` of the tagged node in a `scene/layout` projection.

    Second copy of the walker `r1473_app_default_font.py` holds — folding is a
    measurement, so it is a `scene/layout` datum and not a `scene/snapshot` one
    (R1344). A third consumer is what lifts it into `rpc_verify`.
    """
    found: list[int] = []

    def walk(node) -> None:
        if isinstance(node, dict):
            if node.get("tag") == tag and "line_count" in node:
                found.append(node["line_count"])
            for value in node.values():
                walk(value)
        elif isinstance(node, list):
            for item in node:
                walk(item)

    walk(layout)
    assert found, f"scene/layout publishes a line_count for {tag}"
    return found[0]


def read_narration(env: dict[str, str]) -> dict:
    """Boot `hello-vn-tide` under `env`, reveal its line, and read the row.

    Returns the dialogue row's `w` / `h` / `content` / `line_count` plus the
    binding's font-state row. No clocks: the typewriter is stepped by the
    binding's own `tick {ms}` verb and every wait is a content predicate.
    """
    with RpcSubprocess(VN_EXAMPLE, env=env) as app:
        wait_snap(
            app,
            lambda s: find_by_tag(s, VN_LINE_TAG) is not None,
            desc="the dialogue row is present in the painted scene",
        )
        app.invoke(f"{VN_EXT}/tick", VN_REVEAL_MS)
        snap = wait_snap(
            app,
            lambda s: "▌" not in (find_by_tag(s, VN_LINE_TAG) or {}).get("content", "▌"),
            desc="the typewriter finished, so the row holds the whole line",
        )
        node = find_by_tag(snap, VN_LINE_TAG)
        assert node is not None, "the dialogue row survives the reveal"
        rect = node.get("rect") or {}
        state = find_by_tag(snap, VN_FONT_TAG)
        assert state is not None, "the binding publishes its font state"
        resp = app.request(
            "scene/layout", {"viewport": {"width": VN_W, "height": VN_H}}
        )
        assert resp is not None and resp.result is not None, "scene/layout answers"
        return {
            "w": rect.get("w"),
            "h": rect.get("h"),
            "content": node.get("content", ""),
            "lines": layout_line_count(resp.result, VN_LINE_TAG),
            "font_state": state.get("content", ""),
        }


def field(rows: dict[str, dict], tag: str, prefix: str) -> str:
    """The value part of a `label: value` row, asserted to carry its label."""
    content = rows[tag]["content"]
    assert content.startswith(prefix), f"{tag} publishes {prefix!r}…, got {content!r}"
    return content[len(prefix) :].strip()


def body() -> None:
    for family, path in FIXTURES.items():
        assert Path(path).is_file(), (
            f"premise: the {family} fixture exists at {path} — the demo declares "
            "one of these as the application's face"
        )

    # ---- 1. premise: off by default, and the declaration is reported --------
    default_font = FIXTURES["NanumGothic"]
    base_env = {"PINION_APP_FONT": default_font}
    off_probe = read_rows(base_env)
    assert_eq(
        field(off_probe, ARM_TAG, "self-hosted arm:"),
        "disabled",
        "the self-hosted arm is opt-in, so an ordinary run has one shaper",
    )
    assert_eq(
        field(off_probe, SYSTEM_TAG, "system fonts:"),
        "available",
        "premise: this host HAS a font database — the claim is about which face "
        "is chosen, not about a host with none",
    )
    assert_eq(
        field(off_probe, FAMILIES_TAG, "application families:"),
        "NanumGothic",
        "the application's declared family, read off the scene",
    )

    # ---- 2. the report: which face does the second shaper hold? ------------
    on_probe = read_rows(base_env | ARM_ON)
    arm_field = field(on_probe, ARM_TAG, "self-hosted arm:")
    assert arm_field.startswith("serving "), (
        "with the opt-in set and a usable system font the row names the face the "
        f"arm holds, got {arm_field!r}"
    )
    arm_face = arm_field.removeprefix("serving ").strip()
    assert arm_face, "the arm's family is a non-empty name"
    print(f"[demo] the self-hosted arm holds {arm_face!r}")

    # The claim needs the two shapers to hold DIFFERENT faces. Which face the
    # arm selects is a property of this host's font directories, so pick the
    # application's declaration from the measurement rather than assuming it.
    declared = next(
        (fam for fam in FIXTURES if fam.casefold() != arm_face.casefold()),
        None,
    )
    assert declared is not None, (
        f"premise: a fixture family differing from the arm's {arm_face!r} — with "
        "both fixtures matching it there is no foreign face to ask for"
    )
    env = {"PINION_APP_FONT": FIXTURES[declared]}
    print(f"[demo] the application declares {declared!r} — a face the arm does not hold")

    # ---- 3. the claim: the arm declines what it cannot serve ---------------
    off = read_rows(env)
    on = read_rows(env | ARM_ON)
    assert_eq(
        field(off, FAMILIES_TAG, "application families:"),
        declared,
        "premise: the binding really declared the foreign face",
    )
    assert_eq(
        field(on, FAMILIES_TAG, "application families:"),
        declared,
        "premise: and the same one with the arm enabled — one variable moved",
    )
    assert_eq(
        field(off, ARM_TAG, "self-hosted arm:"),
        "disabled",
        "premise: the baseline run has no second shaper",
    )
    assert_eq(
        field(on, ARM_TAG, "self-hosted arm:"),
        f"serving {arm_face}",
        "premise: the comparison run DOES — an arm is live and holds another face",
    )
    assert arm_face.casefold() != declared.casefold(), (
        f"premise: the arm's {arm_face!r} is not the declared {declared!r}, so "
        "every row below asks for a face the arm cannot draw"
    )

    # The `arm` row is left out of this loop entirely: it reports the variable
    # under test, so its TEXT differs between the two runs ("disabled" versus
    # the face) and a width comparison would be measuring the string length. The
    # other three rows all name the declared family and carry the claim.
    for tag in (SYSTEM_TAG, FAMILIES_TAG, SAMPLE_TAG):
        assert_eq(on[tag]["content"], off[tag]["content"], f"{tag} says the same")
        assert_eq(
            on[tag]["w"],
            off[tag]["w"],
            f"{tag} width — a leaf naming {declared} is measured by the shaper "
            "that can select it, arm on or off",
        )
        assert_eq(on[tag]["h"], off[tag]["h"], f"{tag} height, for the same reason")
    print(
        f"[demo] every row measured identically with the arm on: "
        f"sample {on[SAMPLE_TAG]['w']}x{on[SAMPLE_TAG]['h']}"
    )

    # ---- 4. the row tracks the arm, and is independent of the declaration --
    falsey = read_rows(env | {"PINION_TEXT_ENGINE": "0"})
    assert_eq(
        field(falsey, ARM_TAG, "self-hosted arm:"),
        "disabled",
        "a falsey opt-in builds no arm, so the row is reporting the running "
        "shaper rather than repeating the environment",
    )
    bare = read_rows({"PINION_APP_FONT": "there-is-no-face-at-this-path.ttf"} | ARM_ON)
    assert_eq(
        field(bare, FAMILIES_TAG, "application families:"),
        "(none)",
        "with the asset absent the application declares nothing",
    )
    assert_eq(
        field(bare, ARM_TAG, "self-hosted arm:"),
        f"serving {arm_face}",
        "and the arm still holds its own face — the two facts are independent, "
        "which is why the report carries both",
    )
    # Observation, deliberately NOT asserted: with nothing declared the rows name
    # no family, so the arm serves them and their geometry is its own. Whether
    # that geometry DIFFERS from parley's depends on whether this host's default
    # sans is the same file the arm selected, which is not a property of pinion.
    # The deterministic proof that the arm still measures what it may is the
    # fixture-only unit test `r1479_leaf_naming_a_foreign_family_defers_to_parley_measure`.
    bare_off = read_rows({"PINION_APP_FONT": "there-is-no-face-at-this-path.ttf"})
    print(
        f"[demo] unpinned rows, arm on vs off: "
        f"sample {bare[SAMPLE_TAG]['w']}x{bare[SAMPLE_TAG]['h']} vs "
        f"{bare_off[SAMPLE_TAG]['w']}x{bare_off[SAMPLE_TAG]['h']}"
    )

    # ---- 5. the other half: an application DEFAULT is a request too --------
    # Every row above SPELLS its family, so they exercise only the named half.
    # `hello-vn-tide` names none and declares its face as the process default
    # (R1472) — the case the arm overrode most quietly, because there is no
    # family on the node for a reader to compare against. It showed up as
    # Korean prose that stopped folding: the arm measured it in a Latin face.
    vn_off = read_narration({})
    vn_on = read_narration(ARM_ON)
    assert "default=NanumGothic" in vn_off["font_state"], (
        f"premise: the binding declares a default face, got {vn_off['font_state']!r}"
    )
    assert_eq(
        vn_on["font_state"],
        vn_off["font_state"],
        "premise: and reports the same one with the arm on — the declaration is "
        "not what changed",
    )
    assert any("가" <= ch <= "힣" for ch in vn_off["content"]), (
        f"premise: the revealed row really is Hangul: {vn_off['content'][:24]!r}"
    )
    assert vn_off["lines"] > 1, (
        "premise: through the declared face the narration FOLDS — an unfolded "
        f"baseline would make the comparison below vacuous (line_count={vn_off['lines']})"
    )
    assert_eq(vn_on["content"], vn_off["content"], "premise: the same script")
    assert_eq(
        vn_on["lines"],
        vn_off["lines"],
        "text that names NO family resolves to the application's default, which "
        "the arm does not hold — so it folds exactly as it does with the arm off",
    )
    assert_eq(vn_on["h"], vn_off["h"], "and its box is the same height to match")
    assert_eq(vn_on["w"], vn_off["w"], "premise: the column geometry is identical")
    print(
        f"[demo] unset text keeps the declared default: line_count="
        f"{vn_on['lines']} h={vn_on['h']} with the arm on and off"
    )


if __name__ == "__main__":
    sys.exit(run_demo("r1479 the arm serves the face asked for", body))
