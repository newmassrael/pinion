#!/usr/bin/env python3
"""R1448 §5.36 §2#7 — a GUI window on a host with no fonts, drawing the
application's own face.

R1447 removed the font-less-host abort for the TUI only, by never building a
font context there, and carried the rest honestly: *"the fontique `NoMatch`
unwrap is upstream and unfixed — a font-less GUI host still dies."* This closes
that carry.

Qt reference (`QFontDatabase`), two behaviours pinion lacked:

  1. Qt does not abort when the platform font database is unusable. It reports
     the condition and keeps running.
  2. `QFontDatabase::addApplicationFont(FromData)` lets an application ship a
     face and select it by name, with no system database involved.

And one place Qt is weaker: it answers "are there fonts?" with a `qWarning` on
stderr, which no agent, test, or headless capture can read. Here the answer is
a `FontSourceReport` the binding paints into its scene, so it arrives over
`scene/snapshot` like any other node.

What this asserts, in both font environments:

  1. PREMISE — the host has fonts; the demo's synthesised config has none. Both
     measured, so a broken fixture cannot read as a pass.
  2. THE CARRY — `hello-app-font` boots, answers RPC and lays out a scene under
     the zero-font config. Before R1448 this exact run aborted in fontique.
  3. THE REPORT — the published status row reads `available` with system fonts
     and `unavailable` without them. The same binary, the same code path: the
     row tracks the host, which is what makes it a report rather than a
     constant.
  4. QT PARITY — the declared family is listed in the families row in both
     environments, because it came from the application and not the platform.
  5. IT ACTUALLY DREW — the sample row's measured width is positive with zero
     system fonts, and close to its width on the healthy host. A face that were
     merely *reported* would leave that row collapsed. This is the assertion
     that makes 3 and 4 more than string comparison.
  6. DISCRIMINATOR — with the declaration suppressed (`PINION_APP_FONT` pointed
     at a missing path) on the font-less host, the families row says `(none)` and
     the sample row collapses. So the width in 5 came from the registration, not
     from something the platform quietly supplied.

ZERO-FLAKE: every wait is a `wait_snap` predicate over published data; no
wall-clock sleeps. Widths come from the layout pass via `scene/snapshot`.

Run from the workspace root:
    python3 tools/demos/r1448_app_font.py
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
    fc_list_count,
    run_demo,
    wait_snap,
    write_fontconfig,
)

EXAMPLE = "hello-app-font"

SYSTEM_TAG = "afd_system"
FAMILIES_TAG = "afd_families"
SAMPLE_TAG = "afd_sample"

# The face the example declares by default (a fixture already in the repo).
DECLARED_FONT = "crates/pinion-text-font/tests/fonts/NanumGothic-Regular.ttf"


def font_count(fontconfig: Path | None) -> int:
    """Fonts `fc-list` reports under `fontconfig` (None = the system's)."""
    return fc_list_count(fontconfig)



def write_font_less_config(root: Path) -> Path:
    """A fontconfig over an empty font tree — a slim container's font
    situation, without needing one."""
    return write_fontconfig(root)


def text_of(snap, tag: str) -> str:
    """The published content of the tagged text node."""
    node = find_by_tag(snap, tag)
    assert node is not None, f"tagged node {tag} is missing from the snapshot"
    content = node.get("content")
    assert isinstance(content, str), f"{tag} publishes text content, got {content!r}"
    return content


def width_of(snap, tag: str) -> int:
    """The layout-resolved width of the tagged node — the honest evidence that
    something was really shaped, as opposed to reported."""
    node = find_by_tag(snap, tag)
    assert node is not None, f"tagged node {tag} is missing from the snapshot"
    rect = node.get("rect") or {}
    w = rect.get("w")
    assert isinstance(w, int), f"{tag} publishes an integer width, got {w!r}"
    return w


def read_rows(fontconfig: Path | None, *, font_path: str | None = None):
    """Boot the example and return `(system row, families row, sample width)`."""
    env = {}
    if fontconfig is not None:
        env["FONTCONFIG_FILE"] = str(fontconfig)
    if font_path is not None:
        env["PINION_APP_FONT"] = font_path
    with RpcSubprocess(EXAMPLE, env=env or None) as app:
        # Presence of all three rows in the POST-LAYOUT (paint) snapshot is the
        # readiness signal. Deliberately not "the status row has width": on a
        # font-less host with no declared face a row legitimately measures zero,
        # so gating on width would hang exactly in the environment this demo
        # exists to test.
        snap = wait_snap(
            app,
            lambda s: all(
                find_by_tag(s, t) is not None
                for t in (SYSTEM_TAG, FAMILIES_TAG, SAMPLE_TAG)
            ),
            desc="the three status rows are present in the painted scene",
        )
        return (
            text_of(snap, SYSTEM_TAG),
            text_of(snap, FAMILIES_TAG),
            width_of(snap, SAMPLE_TAG),
        )


def body() -> None:
    declared = Path(DECLARED_FONT)
    assert declared.is_file(), (
        f"premise: the declared face exists at {DECLARED_FONT} — without it the "
        "example has nothing to register and every assertion below is vacuous"
    )

    with tempfile.TemporaryDirectory(prefix="r1448-nofonts-") as tmp:
        no_fonts = write_font_less_config(Path(tmp))

        # ---- 1. premise: two genuinely different font environments ----
        system_fonts = font_count(None)
        assert system_fonts > 0, (
            f"premise: this host has fonts (fc-list = {system_fonts}); with none, "
            "the two runs below would be the same environment"
        )
        assert_eq(font_count(no_fonts), 0, "fonts visible under the demo's config")
        print(f"[demo] system fontconfig: {system_fonts} fonts; fixture: 0")

        # ---- 2 + 3 + 4 + 5. the healthy host, then the font-less one ----
        lit_system, lit_families, lit_sample = read_rows(None)
        assert_eq(lit_system, "system fonts: available", "status row with fonts")
        print(f"[demo] system fonts: {lit_system!r} / {lit_families!r}")

        # The carry: this run reached a window at all.
        dark_system, dark_families, dark_sample = read_rows(no_fonts)
        assert_eq(
            dark_system,
            "system fonts: unavailable",
            "status row on the font-less host — the same binary reporting the "
            "host it actually found, which is why this is a report and not a "
            "constant",
        )
        print(f"[demo] zero fonts:   {dark_system!r} / {dark_families!r}")

        # Qt parity: the declared family is present in BOTH, because the
        # application supplied it rather than the platform.
        assert "(none)" not in lit_families, (
            f"the declared face registered on the healthy host: {lit_families!r}"
        )
        assert "(none)" not in dark_families, (
            "the declared face registered on a host with NO font database — the "
            f"addApplicationFontFromData claim: {dark_families!r}"
        )
        assert_eq(
            dark_families,
            lit_families,
            "the application's families do not depend on the platform's",
        )

        # It actually drew: the sample row has real width with zero system
        # fonts, and lands close to its healthy-host width (same face, same
        # size — the platform contributed nothing to it either way).
        assert dark_sample > 0, (
            f"the sample row shaped to a positive width with no system fonts: "
            f"{dark_sample}"
        )
        assert abs(dark_sample - lit_sample) <= max(4, lit_sample // 20), (
            "the sample is shaped by the declared face in both environments, so "
            f"its width barely moves: system={lit_sample} zero={dark_sample}"
        )
        print(f"[demo] sample width: system={lit_sample} zero={dark_sample}")

        # ---- 6. discriminator: suppress the declaration ----
        # Same font-less host, but the application's asset is absent. If the
        # widths above had come from anywhere other than the registration, this
        # row would still be wide.
        missing = str(Path(tmp) / "no-such-face.ttf")
        bare_system, bare_families, bare_sample = read_rows(
            no_fonts, font_path=missing
        )
        assert_eq(bare_system, "system fonts: unavailable", "still a font-less host")
        assert_eq(
            bare_families,
            "application families: (none)",
            "with the asset absent the binding reports it instead of pretending",
        )
        assert bare_sample < dark_sample, (
            "premise: without the declared face the sample collapses, so the "
            f"width above was the registration's: bare={bare_sample} "
            f"registered={dark_sample}"
        )
        print(
            f"[demo] discriminator: no declaration -> {bare_families!r}, "
            f"sample width {bare_sample} (was {dark_sample})"
        )


if __name__ == "__main__":
    sys.exit(run_demo("r1448 application font on a font-less host", body))
