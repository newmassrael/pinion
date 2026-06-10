#!/usr/bin/env python3
"""R790 §5.15 §5.16 §5.38 §5.39 — modal file-save dialog, driven over RPC.

The AI-first proof that the own-rendered "Save As" picker is a complete,
introspectable peer of an OS file-save dialog. An agent opens the dialog,
types a filename into a real `TextField` (a *modal member*), browses target
folders, and confirms — all through `scene/click` + `scene/key` +
`scene/invoke` + `scene/query` + `focus/get`, reading every outcome back as
data (§2 #2 RPC-primary / #7 scene-as-data). A native OS dialog is a window
pinion does not own, invisible to `scene/query`; this one is a pinion scene,
so the whole save flow is verifiable headless.

Like R693/R788 this is a **boot-closed overlay**: the boot frame renders only
the trigger, so verification is by scene-as-data — `scene/snapshot
source=paint` reflects the *open* dialog once an RPC click re-renders it. The
filename field's caret/selection pixels are guarded by R56/R657 (hello-
textfield) and the dialog chrome by R693/R788; R790 is their composition plus
the R789 write surface.

  (A) closed shape — trigger present; no scrim / panel; "Saved: (none)".
  (B) open via trigger click — scrim + panel + browser rows + filename field
      + Save + Cancel appear; browser opens at /proj; name pre-filled.
  (C) modal trap — focus auto-moves onto the filename field (first member);
      focus/get reports the trap [Name, Cancel, Save]; focus/next wraps;
      focus/set to the trigger behind the scrim is rejected.
  (D) filename field is a live text input — query its text; clear + type a
      new name through `scene/key` (focused), reading the content back.
  (E) Save gated on a non-blank name — blank → Save paints a distinct
      (disabled) fill; typed → Save washes the Accent fill.
  (F) browse by invoke + click — navigate descends; cwd tracked.
  (G) click an existing file → it populates the filename field (the Save As
      "overwrite this" convention, the selection-sync Effect).
  (H) scrim blocks — a backdrop click does not dismiss (WAI-ARIA modal).
  (I) Cancel — closes without writing; focus restored to the trigger.
  (J) confirm — re-open, type a fresh name, click Save: the file is written
      into the current folder, the path is recorded, the dialog closes.
  (K) save into a subdirectory via Enter — re-open, navigate, type, Enter.
  (L) Escape — re-open, Escape dismisses as a cancel.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_query,
    wait_snap,
)

EXAMPLE = "hello-file-save-dialog"
VIEWPORT = (600, 640)

TRIGGER = "save_file"
DIR = "fb_dir"
NAME = "savefile_name"
SAVE = "savefile_save"
CANCEL = "savefile_cancel"
SCRIM = "savefile_scrim"
PANEL = "savefile_panel"
DEFAULT_NAME = "untitled.txt"


def dpath(slot: str) -> str:
    return f"/{DIR}/external/{slot}"


def npath(slot: str) -> str:
    return f"/{NAME}/external/{slot}"


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


def _saved_line(snap) -> str:
    for t in _all_text(snap):
        if t.startswith("Saved:"):
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


def _name_text(tf) -> str:
    return tf.query(npath("text"))


def _row_index(tf, name: str) -> int:
    n = tf.query(dpath("count"))
    for i in range(n):
        if tf.query(dpath(f"name.{i}")) == name:
            return i
    raise AssertionError(f"{name} not in listing")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) closed shape ────────────────────────────────────────
        snap = _snap(tf)
        assert _present(snap, TRIGGER), "trigger present when closed"
        assert not _present(snap, SCRIM), "no scrim when closed"
        assert not _present(snap, PANEL), "no panel when closed"
        assert_eq(_saved_line(snap), "Saved: (none)", "nothing saved at boot")

        # ── (B) open via trigger click ──────────────────────────────
        tf.click(path=TRIGGER)
        snap = wait_snap(
            tf,
            lambda s: _present(s, SCRIM),
            viewport=VIEWPORT,
            desc="scrim appears on open",
        )
        assert _present(snap, PANEL), "panel appears on open"
        assert _present(snap, NAME), "filename field present in the body slot"
        assert _present(snap, SAVE), "Save action present"
        assert _present(snap, CANCEL), "Cancel action present"
        assert "Save file" in _all_text(snap), "dialog title renders"
        assert "Save as:" in _all_text(snap), "filename label renders"
        assert _present(snap, f"{DIR}#0"), "browser row 0 painted in the body slot"
        assert _present(snap, f"{DIR}#up"), "parent affordance painted in the body"
        assert_eq(tf.query(dpath("cwd")), "/proj", "browser opens at /proj")
        assert_eq(_name_text(tf), DEFAULT_NAME, "filename pre-filled with the default")

        # ── (C) modal trap (focus confined to field + buttons) ──────
        assert_eq(_focused(tf), NAME, "open auto-focuses the filename field (first member)")
        assert_eq(_tab_order(tf), [NAME, CANCEL, SAVE], "focus/get reports the trap enumeration")
        assert_eq(_focus_next(tf), CANCEL, "focus/next: Name -> Cancel")
        assert_eq(_focus_next(tf), SAVE, "focus/next: Cancel -> Save")
        assert_eq(_focus_next(tf), NAME, "focus/next wraps Save -> Name")
        rejected = False
        try:
            tf.request("focus/set", {"tag": TRIGGER})
        except RpcError:
            rejected = True
        assert rejected, "focus/set to the trigger behind the scrim is rejected"

        # ── (D) the filename field is a live text input ─────────────
        # Refocus the field, clear the default, and type a new name one
        # keystroke at a time through scene/key (routed to the focused
        # field's TextField substrate), reading the content back as data.
        tf.request("focus/set", {"tag": NAME})
        assert_eq(_focused(tf), NAME, "filename field focused for typing")
        tf.intervene(npath("text"), "")
        assert_eq(_name_text(tf), "", "field cleared via intervene")
        tf.text("report.md", path=NAME)
        wait_query(tf, npath("text"), "report.md", desc="typed characters land in the field")
        assert_eq(tf.query(npath("caret")), len("report.md"), "caret advanced past the typed text")

        # ── (E) Save gated on a non-blank name ──────────────────────
        tf.intervene(npath("text"), "")
        wait_query(tf, npath("text"), "", desc="field cleared for the blank-name gate")
        save_disabled_fill = _fill(_snap(tf), SAVE)
        # A Save click with a blank name is a no-op (dialog stays open) —
        # the dispatch commits before the response, so read directly.
        tf.click(path=SAVE)
        assert _present(_snap(tf), PANEL), "Save with a blank name does not close the dialog"
        assert_eq(_saved_line(_snap(tf)), "Saved: (none)", "blank Save wrote nothing")
        tf.request("focus/set", {"tag": NAME})
        tf.text("draft.md", path=NAME)
        wait_query(tf, npath("text"), "draft.md", desc="non-blank name typed for the gate")
        save_enabled_fill = _fill(_snap(tf), SAVE)
        assert save_enabled_fill != save_disabled_fill, (
            f"Save fill changes when enabled: disabled {save_disabled_fill} vs "
            f"enabled {save_enabled_fill} (the name gate)"
        )

        # ── (F) browse by invoke + click ────────────────────────────
        assert_eq(tf.invoke(dpath("navigate"), "src"), "/proj/src", "navigate descends")
        assert_eq(tf.query(dpath("cwd")), "/proj/src", "cwd tracked the navigate")
        assert_eq(tf.invoke(dpath("up"), None), "/proj", "up climbs to the parent")
        assert_eq(_name_text(tf), "draft.md", "navigation leaves the typed name intact")

        # ── (G) clicking an existing file populates the filename ────
        readme = _row_index(tf, "README.md")
        tf.click(path=f"{DIR}#{readme}")
        wait_query(
            tf, dpath("selected"), "/proj/README.md", desc="clicking a file row selects it"
        )
        wait_query(
            tf,
            npath("text"),
            "README.md",
            desc="selecting a file mirrors its basename into the field (Save As convention)",
        )

        # ── (H) scrim blocks (no light-dismiss) ─────────────────────
        tf.click(path=SCRIM)
        # Blocked no-op: the dispatch commits before the response, so the
        # still-open dialog is readable directly (no gate for a non-change).
        assert _present(_snap(tf), PANEL), "backdrop click does NOT dismiss the modal"

        # ── (I) Cancel closes without writing ───────────────────────
        before = tf.query(dpath("count"))
        tf.click(path=CANCEL)
        snap = wait_snap(
            tf,
            lambda s: not _present(s, PANEL),
            viewport=VIEWPORT,
            desc="Cancel closes the dialog",
        )
        assert not _present(snap, SCRIM), "scrim gone after Cancel"
        assert_eq(_saved_line(snap), "Saved: (none)", "Cancel did not save")
        assert_eq(_focused(tf), TRIGGER, "focus restored to the trigger on close")

        # ── (J) confirm: re-open, type a fresh name, click Save ─────
        tf.click(path=TRIGGER)
        wait_query(tf, npath("text"), DEFAULT_NAME, desc="re-open restores the default name")
        assert_eq(tf.query(dpath("cwd")), "/proj", "re-open resets the browser to the root")
        assert_eq(tf.query(dpath("count")), before, "no file was created by the cancelled flow")
        tf.request("focus/set", {"tag": NAME})
        tf.intervene(npath("text"), "")
        tf.text("fresh.txt", path=NAME)
        wait_query(tf, npath("text"), "fresh.txt", desc="fresh name typed before Save")
        tf.click(path=SAVE)
        snap = wait_snap(
            tf,
            lambda s: not _present(s, PANEL),
            viewport=VIEWPORT,
            desc="Save with a name closes the dialog",
        )
        assert_eq(_saved_line(snap), "Saved: /proj/fresh.txt", "Save recorded the written path")
        assert_eq(_focused(tf), TRIGGER, "focus restored after Save")
        # The write landed in the directory backing: re-open + verify the
        # saved file now appears in the /proj listing (count grew by one).
        tf.click(path=TRIGGER)
        wait_query(tf, dpath("count"), before + 1, desc="the saved file grew the listing by one")
        saved_idx = _row_index(tf, "fresh.txt")
        assert saved_idx >= 0, "the saved file appears in the directory listing"
        tf.key(path=PANEL, name="Escape")
        wait_snap(
            tf,
            lambda s: not _present(s, PANEL),
            viewport=VIEWPORT,
            desc="Escape closed before the subdirectory flow",
        )

        # ── (K) save into a subdirectory, committed with Enter ──────
        tf.click(path=TRIGGER)
        wait_snap(
            tf,
            lambda s: _present(s, PANEL),
            viewport=VIEWPORT,
            desc="re-opened for the subdirectory save",
        )
        assert_eq(tf.invoke(dpath("navigate"), "assets"), "/proj/assets", "descend into assets")
        tf.request("focus/set", {"tag": NAME})
        tf.intervene(npath("text"), "")
        tf.text("sprite.png", path=NAME)
        wait_query(tf, npath("text"), "sprite.png", desc="subdirectory name typed before Enter")
        # Enter in the focused name field is the dialog's default action.
        tf.key(path=NAME, name="Enter")
        snap = wait_snap(
            tf,
            lambda s: not _present(s, PANEL),
            viewport=VIEWPORT,
            desc="Enter in the name field saves + closes",
        )
        assert_eq(
            _saved_line(snap),
            "Saved: /proj/assets/sprite.png",
            "Save wrote into the browsed-into folder, not the root",
        )

        # ── (L) Escape dismisses as a cancel ────────────────────────
        tf.click(path=TRIGGER)
        wait_snap(tf, lambda s: _present(s, PANEL), viewport=VIEWPORT, desc="re-opened")
        tf.key(path=PANEL, name="Escape")
        snap = wait_snap(
            tf,
            lambda s: not _present(s, PANEL),
            viewport=VIEWPORT,
            desc="Escape dismisses the dialog",
        )
        # Escape is a cancel: the prior saved path is unchanged, not cleared.
        assert_eq(
            _saved_line(snap),
            "Saved: /proj/assets/sprite.png",
            "Escape did not save anew",
        )
        assert_eq(_focused(tf), TRIGGER, "focus restored after Escape")


if __name__ == "__main__":
    sys.exit(run_demo("R790 §5.15 §5.16 §5.38 §5.39 — modal file-save dialog", body))
