#!/usr/bin/env python3
"""R1473 §5.36 §2#7 — an application's declared face is what unset text uses.

Qt gives an application two calls: `QFontDatabase::addApplicationFont` makes a
shipped face selectable, and `QApplication::setFont` makes it the one every
unstyled widget gets. R1448 built the first half. Without the second, a face the
application ships is reachable only from a `TextStyle` that spells its name, and
`hello-vn-tide` — whose every row is Korean — named none. So its dialogue was
rendered by whatever the host happened to install, and R1471 had to delete
Hangul from a layout assertion because whether it wrapped was a property of the
developer's machine.

This demo runs the binding against a fontconfig with **no Hangul face at all** —
the CI runner's condition, not an empty container's — and asserts over the wire:

  1. PREMISE — the restricted config really has zero Hangul coverage while the
     host has plenty, and the declared face exists. Both measured, so a broken
     fixture cannot read as a pass.
  2. THE REPORT — `scene/snapshot` carries the declared family, so an agent
     reading the scene can tell what an unset `font_family` resolves to. Qt
     answers this with nothing at all.
  3. THE CLAIM — the Korean dialogue row FOLDS onto several measured lines under
     that config, and its width is the stage width rather than the collapse of an
     unshapeable run.
  4. THE DISCRIMINATOR — the same binding with its declaration suppressed
     (`PINION_VN_FONT` pointed at a path that does not exist) reports no family
     and its dialogue row does not fold. That is the pre-R1473 behaviour, so
     every assertion above is a pass against an environment that would catch a
     regression.

ZERO-FLAKE: no clocks. The typewriter is stepped by the `tick {ms}` step-verb
`hello-vn-tide` already exposes, and every wait is a content predicate over the
published scene.

Run from the workspace root:
    python3 tools/demos/r1473_app_default_font.py
"""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    fc_list_count,
    host_font_count,
    wait_snap,
    write_fontconfig,
)

EXAMPLE = "hello-vn-tide"

# The External the binding hosts; verbs are addressed under its tag.
EXT = "/external"

# The dialogue row the binding tags — the prose whose folding is the claim.
LINE_TAG = "vn.line"

# The row the binding publishes its font state on.
FONT_ROW_TAG = "vn.font_state"

# The face `hello-vn-tide` declares, and the family it registers as.
DECLARED_FONT = "crates/pinion-text-font/tests/fonts/NanumGothic-Regular.ttf"
DECLARED_FAMILY = "NanumGothic"

# A face with no Hangul coverage, used to build a host that has fonts and still
# cannot draw Korean. `pinion-text`'s own fixtures already ship it.
LATIN_ONLY_FONT = "crates/pinion-text-font/tests/fonts/NotoSans-Regular.ttf"

# The dialogue band reflows to the stage width, not the window width.
STAGE_W = 800

# The binding's shipping window, which `scene/layout` is asked about.
WIN_W, WIN_H = 800, 460

# Enough logical time for the typewriter to finish any line in the script.
REVEAL_MS = 60_000


HANGUL_CHARSET = ":charset=ac00"


def write_hangul_less_config(root: Path) -> Path:
    """A fontconfig over a directory holding one Latin-only face.

    Deliberately NOT an empty font tree. With no font at all the application's
    registered face is the only one left in the collection and could win by
    default, which would make the claim pass for a reason that has nothing to do
    with the default family. A host that HAS fonts and lacks this script is both
    the harder case and the real one.
    """
    return write_fontconfig(root, faces=(LATIN_ONLY_FONT,))


def line_node(snap):
    """The tagged dialogue row, asserted present."""
    node = find_by_tag(snap, LINE_TAG)
    assert node is not None, f"the dialogue row {LINE_TAG} is missing from the scene"
    return node


def rect_of(node) -> tuple[int, int]:
    rect = node.get("rect") or {}
    w, h = rect.get("w"), rect.get("h")
    assert isinstance(w, int) and isinstance(h, int), (
        f"the dialogue row publishes an integer rect, got {rect!r}"
    )
    return w, h


def font_state(snap) -> dict[str, str]:
    """The binding's font-state row, parsed into its three fields.

    Read off the scene rather than from an RPC method, because that is where
    R1448 put it: the report is a fact a pure `view` reads and PUBLISHES, so it
    travels over `scene/snapshot` like any other node and there is no second
    surface to keep in sync.
    """
    node = find_by_tag(snap, FONT_ROW_TAG)
    assert node is not None, f"the font-state row {FONT_ROW_TAG} is missing"
    content = node.get("content")
    assert isinstance(content, str), f"the row publishes text, got {content!r}"
    fields = {}
    for part in content.removeprefix("font: ").split(" · "):
        key, _, value = part.partition("=")
        fields[key.strip()] = value.strip()
    for key in ("default", "app", "system"):
        assert key in fields, f"the row publishes {key}=: {content!r}"
    return fields


def read_dialogue(fontconfig: Path, *, font_path: str | None = None):
    """Boot the binding under `fontconfig`, reveal the line, and return
    `(font-state fields, row width, row height, row line_count, content)`."""
    env = {"FONTCONFIG_FILE": str(fontconfig)}
    if font_path is not None:
        env["PINION_VN_FONT"] = font_path
    with RpcSubprocess(EXAMPLE, env=env) as app:
        wait_snap(
            app,
            lambda s: find_by_tag(s, LINE_TAG) is not None,
            desc="the dialogue row is present in the painted scene",
        )
        # Reveal the whole line: a VnState starts at revealed_chars = 0, so
        # without stepping the typewriter the row holds a caret and nothing to
        # fold. Logical time from the argument — no wall clock.
        app.invoke(f"{EXT}/tick", REVEAL_MS)
        snap = wait_snap(
            app,
            lambda s: "▌" not in (find_by_tag(s, LINE_TAG) or {}).get("content", "▌"),
            desc="the typewriter finished, so the row holds the whole line",
        )
        node = line_node(snap)
        w, h = rect_of(node)
        # `line_count` is a `scene/layout` datum, not a `scene/snapshot` one
        # (R1344): the snapshot carries the scene as authored, the layout query
        # carries what the measure pass resolved. Folding is a measurement, so
        # the claim is asked of the layout projection.
        resp = app.request(
            "scene/layout", {"viewport": {"width": WIN_W, "height": WIN_H}}
        )
        assert resp is not None and resp.result is not None, "scene/layout answers"
        lines = layout_line_count(resp.result, LINE_TAG)
        return (font_state(snap), w, h, lines, node.get("content", ""))


def layout_line_count(layout, tag: str) -> int:
    """`line_count` of the tagged node in a `scene/layout` projection."""
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


def body() -> None:
    declared = Path(DECLARED_FONT)
    assert declared.is_file(), (
        f"premise: the declared face exists at {DECLARED_FONT} — without it the "
        "binding has nothing to declare and every assertion below is vacuous"
    )
    assert Path(LATIN_ONLY_FONT).is_file(), (
        f"premise: the Latin-only face exists at {LATIN_ONLY_FONT} — it is what "
        "makes the restricted config a host WITH fonts"
    )

    with tempfile.TemporaryDirectory(prefix="r1473-nohangul-") as tmp:
        no_hangul = write_hangul_less_config(Path(tmp))

        # ---- 1. premise: an environment that has fonts and no Korean ----
        # Both measurements are of the DEMO'S CONFIG, deliberately. An earlier
        # draft also required the HOST to have Hangul faces, reasoning that
        # otherwise the restricted config restricts nothing — and CI failed on
        # it, because a GitHub runner has 53 fonts and none of them Korean. The
        # claim never needed that: what has to be true is that this config has
        # fonts AND cannot draw Hangul, which is a property of the config. The
        # host count is printed, not asserted.
        config_fonts = fc_list_count(no_hangul)
        config_hangul = fc_list_count(no_hangul, HANGUL_CHARSET)
        assert config_fonts > 0, (
            f"premise: the demo's config HAS faces ({config_fonts}) — with none "
            "this would be a font-less container, where the application's "
            "registered face is the only candidate left and would win for a "
            "reason that has nothing to do with the default"
        )
        assert_eq(config_hangul, 0, "Hangul faces under the demo's config")
        print(
            f"[demo] the demo's config: {config_fonts} faces, 0 covering U+AC00 "
            f"(host: {host_font_count(HANGUL_CHARSET)} covering it)"
        )

        # ---- 2. the report: what does an unset family resolve to? ----
        report, width, height, lines, content = read_dialogue(no_hangul)
        assert_eq(
            report["default"],
            DECLARED_FAMILY,
            "the family an unset TextStyle resolves to, read off the scene",
        )
        assert_eq(
            report["app"],
            DECLARED_FAMILY,
            "and the same family is reported as application-supplied",
        )
        assert_eq(
            report["system"],
            "available",
            "premise restated by the binding: the platform database is fine — "
            "what it lacks is this script, not fonts",
        )
        print(
            f"[demo] unset text resolves to {report['default']!r} "
            f"(system: {report['system']})"
        )

        # ---- 3. the claim: the Korean line folds, on a host with no Korean ----
        assert any("가" <= ch <= "힣" for ch in content), (
            f"premise: the revealed row really is Hangul: {content!r}"
        )
        assert_eq(width, STAGE_W - 32, "the row reflows to the stage column width")
        assert isinstance(lines, int) and lines > 1, (
            "the Korean narration FOLDS onto several measured lines through "
            f"the application's own face: line_count={lines!r}"
        )
        assert height > 32, (
            f"and its box is taller than one line to match: h={height}"
        )
        print(f"[demo] the Korean row folds: line_count={lines}, h={height}")

        # ---- 4. the discriminator: suppress the declaration ----
        # The binding survives a missing asset by design (R1448 made a font-less
        # host a legal state), so pointing it at a path that does not exist is
        # exactly the pre-R1473 binding: no declaration, no default.
        absent = str(Path(tmp) / "there-is-no-face-here.ttf")
        bare_report, bare_w, bare_h, bare_lines, bare_content = read_dialogue(
            no_hangul, font_path=absent
        )
        assert_eq(
            bare_report["default"],
            "(none)",
            "with nothing declared, unset text is back to the platform stack",
        )
        assert_eq(
            bare_report["app"],
            "(none)",
            "and no family was supplied by the application",
        )
        assert_eq(
            bare_content,
            content,
            "premise: the same script, so only the face differs between the two "
            "runs",
        )
        assert_eq(
            bare_lines,
            1,
            "line_count of the SAME text with nothing declared — it is "
            "unshapeable tofu on this host, so it cannot wrap",
        )
        assert bare_h < height, (
            f"so its box stays one line tall: bare={bare_h} declared={height}"
        )
        assert_eq(
            bare_w,
            width,
            "premise: the column geometry is identical between the runs, so "
            "the height difference above is the face and not the layout",
        )
        print(
            f"[demo] discriminator: no declaration -> line_count={bare_lines}, "
            f"h={bare_h} (declared: {lines}, {height})"
        )


if __name__ == "__main__":
    sys.exit(run_demo("r1473 an application default font", body))
