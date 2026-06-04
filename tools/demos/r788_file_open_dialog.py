#!/usr/bin/env python3
"""R788 §5.15 §5.16 §5.39 — modal file-open dialog, driven entirely over RPC.

The AI-first proof that the own-rendered file picker is a complete,
introspectable peer of an OS file-open dialog. An agent opens the dialog,
browses + selects a file, and confirms — all through `scene/click` +
`scene/invoke` + `scene/query` + `focus/get`, reading every outcome back as
data (§2 #2 RPC-primary / #7 scene-as-data). A native OS dialog is a window
pinion does not own, so it is invisible to `scene/query` and could not be
driven or verified this way at all ([[native-menu-macos-windows-only-verify]]).

Like R693's modal dialog, this is a **boot-closed overlay**: the boot frame
renders only the trigger, so (as r693_dialog.py documents) verification is by
scene-as-data — `scene/snapshot source=paint` reflects the *open* dialog once
an RPC click has re-rendered it, no rasterized boot screenshot. The dialog
chrome's pixel fidelity is already guarded by R693/R711 (elevation) and the
file-row inks by R787 (hello-file-browser); R788 is their composition.

  (A) closed shape — trigger present; no scrim / panel; "Chosen: (none)".
  (B) open via trigger click — scrim + panel + browser rows + OK + Cancel
      appear; the browser opens at /proj with nothing selected.
  (C) modal trap — focus auto-moves onto Cancel; focus/get reports the
      trap enumeration [Cancel, OK]; focus/next cycles + wraps; focus/set
      to the trigger behind the scrim is rejected.
  (D) browse by INVOKE — `navigate "src"` descends; `up` climbs (the
      DirectoryExternal, fully AI-first query + invoke).
  (E) browse by CLICK — clicking a directory row (`fb_dir#<i>`) descends
      through the same `send` wire a real pointer drives; `fb_dir#up`
      climbs.
  (F) OK gated on a selection — with nothing picked, OK paints a distinct
      (disabled) fill and `selected` is Null; the reducer no-ops an OK.
  (G) select a file — clicking a file row records its full path in
      `selected`; OK now paints its enabled (Accent) fill, matching the
      picked row's Accent ink.
  (H) scrim blocks — a backdrop click does not dismiss (WAI-ARIA modal).
  (I) Cancel — closes without choosing; focus restored to the trigger.
  (J) confirm — re-open, select via invoke, click OK: the chosen path is
      recorded ("Chosen: /proj/...") and the dialog closes.
  (K) Escape — re-open, Escape dismisses as a cancel.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
)

EXAMPLE = "hello-file-open-dialog"
VIEWPORT = (600, 600)
PAUSE = 0.12

TRIGGER = "open_file"
DIR = "fb_dir"
OK = "fileopen_ok"
CANCEL = "fileopen_cancel"
SCRIM = "fileopen_scrim"
PANEL = "fileopen_panel"


def dpath(slot: str) -> str:
    return f"/{DIR}/external/{slot}"


def _all_text(node) -> list[str]:
    out: list[str] = []

    def walk(n):
        if isinstance(n, dict):
            if n.get("type") == "Text":
                c = n.get("content")
                if isinstance(c, str):
                    out.append(c)
            for ch in n.get("children") or []:
                walk(ch)

    walk(node)
    return out


def _present(snap, tag) -> bool:
    return find_by_tag(snap, tag) is not None


def _chosen_line(snap) -> str:
    for t in _all_text(snap):
        if t.startswith("Chosen:"):
            return t
    return ""


def _fill(snap, tag):
    node = find_by_tag(snap, tag)
    assert node is not None, f"{tag} present"
    fill = (node.get("style") or {}).get("fill") or {}
    return (fill.get("r"), fill.get("g"), fill.get("b"), fill.get("a"))


def _focused(tf):
    return tf.request("focus/get").result.get("focused")


def _tab_order(tf):
    return tf.request("focus/get").result.get("tab_order") or []


def _focus_next(tf):
    return tf.request("focus/next").result.get("focused")


def _snap(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) closed shape ────────────────────────────────────────
        snap = _snap(tf)
        assert _present(snap, TRIGGER), "trigger present when closed"
        assert not _present(snap, SCRIM), "no scrim when closed"
        assert not _present(snap, PANEL), "no panel when closed"
        assert_eq(_chosen_line(snap), "Chosen: (none)", "nothing chosen at boot")

        # ── (B) open via trigger click ──────────────────────────────
        tf.click(path=TRIGGER)
        time.sleep(PAUSE)
        snap = _snap(tf)
        assert _present(snap, SCRIM), "scrim appears on open"
        assert _present(snap, PANEL), "panel appears on open"
        assert _present(snap, OK), "OK action present"
        assert _present(snap, CANCEL), "Cancel action present"
        assert "Open file" in _all_text(snap), "dialog title renders"
        assert _present(snap, f"{DIR}#0"), "browser row 0 painted in the body slot"
        assert _present(snap, f"{DIR}#up"), "parent affordance painted in the body"
        assert_eq(tf.query(dpath("cwd")), "/proj", "browser opens at /proj")
        assert_eq(tf.query(dpath("selected")), None, "nothing selected on open")

        # ── (C) modal trap (focus confined to the action buttons) ───
        assert_eq(_focused(tf), CANCEL, "open auto-focuses Cancel (safe default)")
        assert_eq(_tab_order(tf), [CANCEL, OK], "focus/get reports the trap enumeration")
        assert_eq(_focus_next(tf), OK, "focus/next: Cancel -> OK")
        assert_eq(_focus_next(tf), CANCEL, "focus/next wraps OK -> Cancel")
        rejected = False
        try:
            tf.request("focus/set", {"tag": TRIGGER})
        except RpcError:
            rejected = True
        assert rejected, "focus/set to the trigger behind the scrim is rejected"

        # ── (D) browse by INVOKE (the AI-first path) ────────────────
        assert_eq(tf.invoke(dpath("navigate"), "src"), "/proj/src", "navigate descends")
        assert_eq(tf.query(dpath("cwd")), "/proj/src", "cwd tracked the navigate")
        assert_eq(tf.invoke(dpath("up"), None), "/proj", "up climbs to the parent")

        # ── (E) browse by CLICK (the pointer send wire) ─────────────
        def row_index(name: str) -> int:
            n = tf.query(dpath("count"))
            for i in range(n):
                if tf.query(dpath(f"name.{i}")) == name:
                    return i
            raise AssertionError(f"{name} not in listing")

        tf.click(path=f"{DIR}#{row_index('assets')}")
        time.sleep(PAUSE)
        assert_eq(tf.query(dpath("cwd")), "/proj/assets", "clicking a directory row descends")
        tf.click(path=f"{DIR}#up")
        time.sleep(PAUSE)
        assert_eq(tf.query(dpath("cwd")), "/proj", "clicking the parent affordance climbs")

        # ── (F) OK gated on a selection (disabled until a pick) ─────
        assert_eq(tf.query(dpath("selected")), None, "no selection yet")
        ok_disabled_fill = _fill(_snap(tf), OK)
        # An OK click with nothing picked is a no-op (dialog stays open).
        tf.click(path=OK)
        time.sleep(PAUSE)
        assert _present(_snap(tf), PANEL), "OK with no selection does not close the dialog"
        assert_eq(_chosen_line(_snap(tf)), "Chosen: (none)", "OK no-op chose nothing")

        # ── (G) select a file → OK enables (Accent) ─────────────────
        cargo_row = row_index("Cargo.toml")
        tf.click(path=f"{DIR}#{cargo_row}")
        time.sleep(PAUSE)
        assert_eq(
            tf.query(dpath("selected")), "/proj/Cargo.toml", "clicking a file row selects it"
        )
        snap = _snap(tf)
        ok_enabled_fill = _fill(snap, OK)
        assert ok_enabled_fill != ok_disabled_fill, (
            f"OK fill changes when enabled: disabled {ok_disabled_fill} vs "
            f"enabled {ok_enabled_fill} (the selection gate)"
        )
        # The enabled OK and the selected row both wash the Accent tier.
        assert_eq(
            ok_enabled_fill,
            _fill(snap, f"{DIR}#{cargo_row}"),
            "enabled OK shares the Accent ink of the picked row",
        )

        # ── (H) scrim blocks (no light-dismiss) ─────────────────────
        tf.click(path=SCRIM)
        time.sleep(PAUSE)
        assert _present(_snap(tf), PANEL), "backdrop click does NOT dismiss the modal"

        # ── (I) Cancel closes without choosing ──────────────────────
        tf.click(path=CANCEL)
        time.sleep(PAUSE)
        snap = _snap(tf)
        assert not _present(snap, PANEL), "Cancel closes the dialog"
        assert not _present(snap, SCRIM), "scrim gone after Cancel"
        assert_eq(_chosen_line(snap), "Chosen: (none)", "Cancel did not choose")
        assert_eq(_focused(tf), TRIGGER, "focus restored to the trigger on close")

        # ── (J) confirm: re-open, select via invoke, click OK ───────
        tf.click(path=TRIGGER)
        time.sleep(PAUSE)
        assert_eq(tf.query(dpath("cwd")), "/proj", "re-open resets the browser to the root")
        assert_eq(tf.query(dpath("selected")), None, "re-open cleared the prior selection")
        assert_eq(
            tf.invoke(dpath("select"), "README.md"),
            "/proj/README.md",
            "select returns the picked path",
        )
        tf.click(path=OK)
        time.sleep(PAUSE)
        snap = _snap(tf)
        assert not _present(snap, PANEL), "OK with a selection closes the dialog"
        assert_eq(_chosen_line(snap), "Chosen: /proj/README.md", "OK recorded the chosen path")
        assert_eq(_focused(tf), TRIGGER, "focus restored after confirm")

        # ── (K) Escape dismisses as a cancel ────────────────────────
        tf.click(path=TRIGGER)
        time.sleep(PAUSE)
        assert _present(_snap(tf), PANEL), "re-opened"
        tf.key(path=PANEL, name="Escape")
        time.sleep(PAUSE)
        snap = _snap(tf)
        assert not _present(snap, PANEL), "Escape dismisses the dialog"
        # Escape is a cancel: the prior chosen path is unchanged, not cleared.
        assert_eq(_chosen_line(snap), "Chosen: /proj/README.md", "Escape did not choose anew")
        assert_eq(_focused(tf), TRIGGER, "focus restored after Escape")


if __name__ == "__main__":
    sys.exit(run_demo("R788 §5.15 §5.16 §5.39 — modal file-open dialog", body))
