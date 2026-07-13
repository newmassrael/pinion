#!/usr/bin/env python3
"""R1318 §5.16 §5.51 §2 #7 PR-52 — a dock panel's DISPLAY TITLE is not its IDENTITY.

SCOPE — read this before the assertions. Pre-R1318 the dock walker painted the
`panel_id` as the panel's header title and as its tab label: identity and display
name were the same string. A consumer whose panel CONTENT names itself — a terminal
pane renamed by its child on every prompt (`OSC 0`/`OSC 2`), an editor tab following
the open document — could not follow that name. The only escape was to push the
title into the `panel_id`, which is the panel's ADDRESS (topology leaf key, paint
tag, `DockPanelExternal` registration, drop target, RPC path): a pane would change
its own address whenever its child renamed itself (breaking drag / dock / focus /
RPC), and two panes showing `vim` would collide as a `DuplicatePanelId`.

R1318 splits the two. `DockPanelChrome::with_title(|panel_id| …)` supplies the
DISPLAY name the walker paints — in the header, and in the tab-well label (a well's
strip IS its panels' header row). The walker keeps OWNING identity: it re-stamps the
`panel_id` tag after the binding's chrome runs, so nothing painted can move an
address.

The editor stands in for the terminal: its `panel_id` → display-title map is app
state, exposed on the wire as a `SignalExternal`, so an AI retitles a panel with ONE
`scene/intervene` — the wire stand-in for the child's `OSC 0` rename.

This drives it over the real wire and observes §2 #7 scene-as-data:

  (A) Boot — no titles declared → every header paints its own `panel_id`
      (the back-compat witness: an existing binding is unchanged).
  (B) Retitle `outliner` → its header paints `vim README`, while its paint tag,
      composite hit regions, External, and topology id all stay `outliner`.
  (C) Identity still WORKS under a display name — an RPC dock reorganize addressed
      by `outliner` moves the renamed panel (a label is not an address).
  (D) Tab labels follow the display title (the strip is the panels' header row).
  (E) TWO panels, ONE display title — both stay independently addressable (the
      `DuplicatePanelId` collision the id-as-title workaround would have caused).
  (F) A torn-off panel keeps its display name in its own floating window (one
      naming SSOT across docked + floating chrome).
  (G) R1319 — the OS WINDOW TITLE of that floating window follows a LIVE retitle
      (pre-R1319 `WindowSpec::title` was read once, at create).

The live FEEL is HW-gated; this pins what is observable as scene-as-data.

WHAT (G) DOES AND DOES NOT ASSERT. The declared title (`scene/windows`) is sampled
UPSTREAM of the reconcile pass under test, so on its own it would prove only that the
binding's Effect ran — not that the shell drove the OS window. The apply itself is
therefore asserted through the shell's own production `tracing` line ("window title
updated", `PINION_LOG=pinion::shell=debug`), which fires inside the pass, after
`Window::set_title`. The X-server layer below that is winit's contract, not asserted
here: winit's X11 `Window::title()` is a stub returning an empty string, so there is
no title read-back to converge on (which is also why, unlike position (R1088), the
title axis has no declared→actual write-back).
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_stderr,
    wait_until,
)

EXAMPLE = "hello-dock-panels-editor"
MAIN = "main"
MW, MH = 1200, 800
FW, FH = 420, 320

REORG = "/dock_reorganize/external"
TITLES = "/panel_titles/external/value"

OUTLINER = "outliner"
VIEWPORT = "viewport"
PROPERTIES = "properties"
CONSOLE = "console"
ALL_PANELS = {OUTLINER, VIEWPORT, PROPERTIES, CONSOLE}

VIM = "vim README"
HTOP = "htop"


# ─── scene-as-data helpers ───────────────────────────────────────────


def _topology(tf: RpcSubprocess) -> Any:
    return tf.query(f"{REORG}/topology")


def _panel_ids(node: Any, out: set | None = None) -> set:
    """Every panel id in the topology — the IDENTITY set (never the titles)."""
    out = set() if out is None else out
    if isinstance(node, dict):
        if node.get("type") == "Leaf" and node.get("panel_id"):
            out.add(node["panel_id"])
        for p in node.get("panels", []):
            out.add(p)
        for v in node.values():
            _panel_ids(v, out)
    elif isinstance(node, list):
        for x in node:
            _panel_ids(x, out)
    return out


def _texts(node: Any) -> list[str]:
    """Every `Scene::Text` content painted under `node`, in paint order."""
    out: list[str] = []

    def walk(n: Any) -> None:
        if not isinstance(n, dict):
            return
        if n.get("type") == "Text" and isinstance(n.get("content"), str):
            out.append(n["content"])
        for child in n.get("children") or []:
            walk(child)
        content = n.get("content")
        if isinstance(content, dict):
            walk(content)

    walk(node)
    return out


def _header_text(tf: RpcSubprocess, panel: str, *, window: str = MAIN) -> Optional[str]:
    """The string painted in `panel`'s header strip — the panel's DISPLAY name.

    Addressed through the panel's IDENTITY (the `{panel}#header` composite tag the
    walker emits from the topology's `panel_id`), which is exactly the point: the
    title changes, the address does not.
    """
    size = (MW, MH) if window == MAIN else (FW, FH)
    snap = tf.snapshot(source="paint", viewport=size, window=window)
    header = find_by_tag(snap, f"{panel}#header")
    if header is None:
        return None
    texts = _texts(header)
    return texts[0] if texts else None


def _set_titles(tf: RpcSubprocess, titles: dict[str, str]) -> None:
    """Retitle panels over the wire — the stand-in for a terminal child's `OSC 0`.

    One `scene/intervene` onto the app's `panel_id` → display-title map (a
    `SignalExternal` over the same `Signal` the view reads), so the repaint is
    reactive.
    """
    tf.intervene(TITLES, titles)


def _access_nodes(tf: RpcSubprocess) -> list[dict]:
    """The `scene/access` AT tree — the structured NAME channel (not the paint tree)."""
    resp = tf.request("scene/access", {})
    assert resp is not None and resp.result is not None, "scene/access must answer"
    nodes = resp.result.get("nodes")
    assert isinstance(nodes, list), f"scene/access.nodes must be a list; got {resp.result!r}"
    return nodes


def _windows(tf: RpcSubprocess) -> list[dict]:
    resp = tf.request("scene/windows", {})
    assert resp is not None and resp.result is not None, "scene/windows must answer"
    return resp.result["windows"]


def _floating_ids(tf: RpcSubprocess) -> set:
    return {w["id"] for w in _windows(tf)}


def _declared_title(tf: RpcSubprocess, window_id: str) -> Optional[str]:
    """The window's DECLARED OS title (`scene/windows`) — the binding's spec.

    Note this is sampled upstream of the shell's `set_title` pass; section (G)
    asserts the pass itself through the shell's tracing line.
    """
    for w in _windows(tf):
        if w["id"] == window_id:
            return w.get("title")
    return None


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    # (R1319) Raise the shell's log level so its `tracing` title-apply line reaches
    # stderr (default filter is `warn`) — section (G)'s observable for a pass whose
    # OS effect winit gives no read-back for.
    with RpcSubprocess(
        EXAMPLE, boot_grace=1.5, env={"PINION_LOG": "pinion::shell=debug"}
    ) as tf:
        # ── (A) boot: no titles declared → identity IS the display name ──
        assert_eq(_panel_ids(_topology(tf)), ALL_PANELS, "A.1 four panels at boot")
        assert_eq(tf.query(TITLES), {}, "A.2 no display titles declared")
        for panel in sorted(ALL_PANELS):
            assert_eq(
                _header_text(tf, panel),
                panel,
                f"A.3 {panel}'s header paints its own id (pre-R1318 behaviour intact)",
            )

        # ── (B) retitle: the header follows, the ADDRESS does not ────────
        _set_titles(tf, {OUTLINER: VIM})
        wait_until(
            lambda: _header_text(tf, OUTLINER) == VIM,
            desc="B.1 the retitled panel's header repaints",
        )
        assert_eq(_header_text(tf, OUTLINER), VIM, "B.2 ★header paints the DISPLAY title")
        assert_eq(
            _header_text(tf, VIEWPORT),
            VIEWPORT,
            "B.3 an unnamed panel still falls back to its id",
        )
        # ★Identity untouched: the panel is STILL tagged / addressed by `outliner`.
        snap = tf.snapshot(source="paint", viewport=(MW, MH), window=MAIN)
        assert find_by_tag(snap, OUTLINER) is not None, "B.4 ★panel root still tagged by its id"
        assert find_by_tag(snap, f"{OUTLINER}#content") is not None, (
            "B.5 ★composite hit regions still key on the id"
        )
        assert find_by_tag(snap, VIM) is None, "B.6 ★the display title NEVER becomes a tag"
        assert_eq(
            _panel_ids(_topology(tf)),
            ALL_PANELS,
            "B.7 ★the topology (identity SSOT) is untouched by a rename",
        )
        assert tf.query(f"/{OUTLINER}/external/dragging") is not None, (
            "B.8 ★the panel's External is still registered under its id"
        )

        # ── (C) the address still WORKS under a display name ─────────────
        # An RPC reorganize addressed by `outliner` moves the panel titled
        # `vim README` — the gesture path a title-in-the-id workaround would break.
        tf.invoke(f"{REORG}/reorganize", {
            "source": OUTLINER, "target": CONSOLE, "zone": "Right",
        })
        wait_until(
            lambda: tf.query(f"{REORG}/last_outcome") is not None,
            desc="C.1 the reorganize lands",
        )
        assert_eq(
            _panel_ids(_topology(tf)),
            ALL_PANELS,
            "C.2 ★every panel survives a dock addressed by id-under-a-display-name",
        )
        assert_eq(
            _header_text(tf, OUTLINER),
            VIM,
            "C.3 ★the panel keeps its display name across the move",
        )

        # ── (D) tab labels follow the display title ──────────────────────
        # A well's strip IS its panels' header row, so a tab LABEL is the header
        # title relocated — same provider, same string.
        _set_titles(tf, {OUTLINER: VIM, VIEWPORT: HTOP})
        tf.invoke(f"{REORG}/reorganize", {
            "source": VIEWPORT, "target": PROPERTIES, "zone": "Center",
        })
        wait_until(
            lambda: HTOP in _texts(tf.snapshot(source="paint", viewport=(MW, MH), window=MAIN)),
            desc="D.1 the tab strip repaints with the display title",
        )
        painted = _texts(tf.snapshot(source="paint", viewport=(MW, MH), window=MAIN))
        assert HTOP in painted, f"D.2 ★the tab label paints the display title, got {painted}"
        assert VIEWPORT not in painted, (
            f"D.3 ★no raw panel id leaks into the strip for a titled panel, got {painted}"
        )
        assert_eq(
            _panel_ids(_topology(tf)),
            ALL_PANELS,
            "D.4 ★the tabified panel is still addressed by its id",
        )
        # ★R1320 — the AT tree (`scene/access`) is the OTHER structured name channel,
        # and it must agree with the pixels. R1318 named the `tab` from the painted
        # label (→ the display title) but the `tabpanel` it controls EXPLICITLY from the
        # panel id, so one panel was announced under two names to a screen reader / an
        # AI reading `scene/access`. The TAG stays the id (it is what `activate_tab`
        # addresses); only the NAME follows the display title.
        # The AT tree is rebuilt on the next paint (like the r1096 tablist probe), so
        # poll rather than race it.
        wait_until(
            lambda: any(n.get("role") == "tabpanel" for n in _access_nodes(tf)),
            desc="D.5 scene/access grows the well's tabpanel node",
        )
        active = next(n for n in _access_nodes(tf) if n.get("role") == "tabpanel")
        assert_eq(active.get("name"), HTOP, "D.6 ★the AT announces the panel's DISPLAY title")
        assert_eq(active.get("tag"), VIEWPORT, "D.7 ★…while the node is ADDRESSED by the id")

        # ── (E) two panels, ONE display title ────────────────────────────
        # A label is not an address: two panes both running `vim` show `vim` twice
        # and stay independently addressable (the `DuplicatePanelId` collision the
        # id-as-title workaround would have caused).
        _set_titles(tf, {OUTLINER: VIM, CONSOLE: VIM})
        wait_until(
            lambda: _header_text(tf, CONSOLE) == VIM,
            desc="E.1 the second panel takes the same display title",
        )
        assert_eq(_header_text(tf, OUTLINER), VIM, "E.2 ★both headers paint the same title")
        assert_eq(_header_text(tf, CONSOLE), VIM, "E.3 ★…the duplicate is legal")
        assert_eq(
            _panel_ids(_topology(tf)),
            ALL_PANELS,
            "E.4 ★…and both panels remain distinct identities",
        )
        # ★Each is still separately addressable: the two same-titled panels report
        # DIFFERENT identities through their own Externals (the wire address is the
        # panel id, never the label).
        assert_eq(
            tf.query(f"/{OUTLINER}/external/panel_id"),
            OUTLINER,
            "E.5 ★one panel titled `vim` is addressed as `outliner`…",
        )
        assert_eq(
            tf.query(f"/{CONSOLE}/external/panel_id"),
            CONSOLE,
            "E.6 ★…the other, identically titled, as `console`",
        )

        # ── (F) a torn-off panel keeps its name in its own window ────────
        tf.invoke(f"/{CONSOLE}/external/tear_off", None)
        floating = f"torn-{CONSOLE}"
        wait_until(lambda: floating in _floating_ids(tf), desc="F.1 the floating window opens")
        assert_eq(
            _header_text(tf, CONSOLE, window=floating),
            VIM,
            "F.2 ★the floating panel keeps its DISPLAY name (one naming SSOT, docked + floating)",
        )
        assert (
            find_by_tag(
                tf.snapshot(source="paint", viewport=(FW, FH), window=floating),
                CONSOLE,
            )
            is not None
        ), "F.3 ★…and is still addressed by its panel id in its own window"
        # ★R1320 — the TORN SLOT the panel left behind in the dock is the LAST painted
        # panel string, and R1318 missed it: it took one `panel_id` for both its label
        # and its tag, so a panel titled `vim README` left a slot reading
        # `(console torn off)` — two names for one panel, on screen at the same time.
        # The tag is load-bearing (`resolve_drop` recovers the panel id from it to
        # resolve a redock), so label and address had to be split, not swapped.
        slot = find_by_tag(
            tf.snapshot(source="paint", viewport=(MW, MH), window=MAIN),
            f"{CONSOLE}_placeholder",
        )
        assert slot is not None, "F.4 the torn slot is still TAGGED by the panel id"
        assert_eq(
            _texts(slot),
            [f"({VIM} torn off)"],
            "F.5 ★…and LABELLED by the display title (one name per panel, everywhere)",
        )

        # ── (G) R1319: the OS window title follows a LIVE retitle ────────
        # The floating window opened while `console` was already titled `vim README`
        # (the tear-off in (F) read the same display-title SSOT).
        assert_eq(
            _declared_title(tf, floating),
            f"{EXAMPLE} — {VIM} (floating)",
            "G.1 the torn-off window OPENS under the panel's display name",
        )
        # ★Now RENAME the panel while its window is already open — the case that was
        # structurally impossible pre-R1319 (`title` was read once, at create).
        _set_titles(tf, {OUTLINER: VIM, CONSOLE: HTOP})
        wait_until(
            lambda: _declared_title(tf, floating) == f"{EXAMPLE} — {HTOP} (floating)",
            desc="G.2 the binding's Effect re-declares the window title",
        )
        # ★…and the SHELL drove the real window: this trace fires inside
        # `reconcile_windows`'s title pass, AFTER `Window::set_title`. Without the
        # R1319 pass the declared title above would change and the OS window would
        # silently keep its boot title — exactly the pre-R1319 gap.
        wait_stderr(
            tf,
            "window title updated",
            desc="G.3 ★the shell APPLIED the new title to the live OS window",
        )
        applied = [ln for ln in tf.stderr_tail(120) if "window title updated" in ln]
        assert any(floating in ln and HTOP in ln for ln in applied), (
            f"G.4 ★the apply names the right window + title, got {applied}"
        )

        print("[demo] r1318_dock_display_title: all sections PASS (display title ⊥ identity)")


if __name__ == "__main__":
    sys.exit(run_demo("r1318_dock_display_title", body))
