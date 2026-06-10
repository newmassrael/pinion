#!/usr/bin/env python3
"""R666 §5.34 §5.37 — todomvc 12+ step AI-driven end-to-end workflow.

Drives R666 substrate land (scene/invoke v1 multi-External path
syntax + scene/key character vs named dispatch + R665 persistence
cycle) through a single contiguous AI workflow. §2 invariant #2
("RPC as AI primary path") production stress test — every step is
self-verifying through symbolic introspection, with cross-process
verification via the on-disk storage blob (`R665` substrate).

R666 v1 path convention:
- ExtraExternal singletons live in the state scene with their base
  tag (`todo_toggle`, `todo_delete`, `todo_item`, …) — the `#<id>`
  composite suffix is a *paint-side* artefact for click routing.
- v1 syntax targets the singleton by base tag and passes the
  per-row id as `args`: `/{extra_tag}/external/{action}` with
  `args = Int(<id>)`.
- Equivalent to the paint-side composite-tag click but skips the
  router, exactly what an AI driver wants.

12+ workflow steps:

  (1) Boot 1 — empty storage, defaults visible
  (2) Type three rows via scene/key character arc (R666 #3:
      single-codepoint chars route through `handle_character_key`)
  (3) Toggle row[0] via scene/invoke v1 path
      `/todo_toggle/external/toggle` with `args = row0_id`
      (R666 #1: ExtraExternal singleton addressing)
  (4) Filter cycle to Active via scene/key "ArrowRight" on the
      RadioGroupExternal
  (5) Begin edit on row[1] via scene/invoke v1 path
      `/todo_item/external/begin` with `args = row1_id`
  (6) Backspace + retype the edited row's text through the character
      arc (R666 #3 demonstrates V::keybinding cleanup → printable
      letters reach apply_key)
  (7) Enter commit
  (8) Verify post-edit row text via paint snapshot
  (9) Kill the process (process exit + storage Effect drains)
 (10) Relaunch with the same storage dir
 (11) Verify hydrated state: row[1] text changed, row[0] still
      completed, filter Active persisted, next_id advanced
 (12) Add one more row through R666 #3 character arc; verify
      next_id chained correctly (no id collision with persisted set)
 (13) Toggle off the persisted-completed row via scene/invoke v1
      `/todo_toggle/external/toggle` (cycle 2)
 (14) Delete a row via scene/invoke v1
      `/todo_delete/external/delete`
 (15) Kill + relaunch
 (16) Verify delete + toggle persisted across the second cycle

Total assertions: ≥ 50.
"""

from __future__ import annotations

import json
import os
import shutil
import sys
import tempfile
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, run_demo, wait_json_file, wait_until  # noqa: E402

# Tags — kept stable across todomvc rounds R655-R666.
TF_TAG = "main_textfield"
EDIT_TF_TAG = "todo_edit"
ITEM_TAG = "todo_item"
TOGGLE_TAG = "todo_toggle"
DELETE_TAG = "todo_delete"
EDIT_TAG = "todo_edit"
LIST_TAG = "todo_list"
FILTER_TAG = "todo_filter"
VIEWPORT_W = 480
VIEWPORT_H = 720
STATE_KEY = "todomvc.state"
SCHEMA_VERSION = 1


def storage_path(root: Path) -> Path:
    return root / STATE_KEY


def read_state_blob(root: Path) -> dict[str, Any]:
    raw = storage_path(root).read_text(encoding="utf-8")
    return json.loads(raw)


def make_subprocess(storage_dir: Path) -> RpcSubprocess:
    os.environ["PINION_STORAGE_DIR"] = str(storage_dir)
    return RpcSubprocess("todomvc")


def focus_set(tf: RpcSubprocess, tag: str | None) -> None:
    tf.request("focus/set", {"tag": tag})


def find_node_by_tag(node: dict[str, Any], tag: str) -> dict[str, Any] | None:
    if not isinstance(node, dict):
        return None
    if node.get("tag") == tag:
        return node
    for child in node.get("children") or []:
        hit = find_node_by_tag(child, tag)
        if hit is not None:
            return hit
    content = node.get("content")
    if isinstance(content, dict):
        hit = find_node_by_tag(content, tag)
        if hit is not None:
            return hit
    return None


def collect_item_ids(tf: RpcSubprocess) -> list[int]:
    snap = tf.snapshot(source="paint", viewport=(VIEWPORT_W, VIEWPORT_H))
    list_node = find_node_by_tag(snap, LIST_TAG)
    assert list_node is not None, "todo_list must be present"
    ids: list[int] = []
    for child in list_node.get("children") or []:
        tag = child.get("tag")
        if isinstance(tag, str) and tag.startswith(f"{ITEM_TAG}#"):
            ids.append(int(tag.split("#", 1)[1]))
    return ids


def row_completed(tf: RpcSubprocess, item_id: int) -> bool:
    snap = tf.snapshot(source="paint", viewport=(VIEWPORT_W, VIEWPORT_H))
    row = find_node_by_tag(snap, f"{ITEM_TAG}#{item_id}")
    if row is None:
        return False
    children = row.get("children") or []
    if len(children) < 2:
        return False
    middle = children[1]
    if middle.get("type") != "Text":
        return False
    deco = (middle.get("style") or {}).get("decoration") or {}
    return bool(deco.get("strikethrough"))


def row_text(tf: RpcSubprocess, item_id: int) -> str | None:
    snap = tf.snapshot(source="paint", viewport=(VIEWPORT_W, VIEWPORT_H))
    row = find_node_by_tag(snap, f"{ITEM_TAG}#{item_id}")
    if row is None:
        return None
    children = row.get("children") or []
    if len(children) < 2:
        return None
    middle = children[1]
    return middle.get("content") if middle.get("type") == "Text" else None


def submit_enter(tf: RpcSubprocess, target: str) -> None:
    tf.key(path=target, name="Enter")


def add_todo(tf: RpcSubprocess, body: str) -> None:
    """Type a body into the focused main TextField then commit with
    Enter. Uses the R666 #3 character arc — every printable char
    routes through `handle_character_key` ↪ `V::keybinding` (no
    intercept for TextField) ↪ `apply_key` printable insert."""
    focus_set(tf, TF_TAG)
    tf.text(body, path=TF_TAG)
    submit_enter(tf, target=TF_TAG)


def wait_blob(storage_dir: Path, predicate, desc: str) -> dict[str, Any]:
    """Poll the persisted blob via the harness JSON-file gate (R883.1
    — atomic-rename polling decision lives in rpc_verify, one home)."""
    return wait_json_file(storage_path(storage_dir), predicate, desc=desc)



def body() -> None:
    storage_dir = Path(tempfile.mkdtemp(prefix="pinion-todomvc-r666-"))
    try:
        # ──────────────────────────────────────────────────────────
        # Cycle 1 — substrate-driven workflow
        # ──────────────────────────────────────────────────────────
        with make_subprocess(storage_dir) as tf:
            # ── (1) Boot 1: empty storage, defaults visible ────────
            blob = wait_blob(
                storage_dir, lambda b: True,
                "R665 save Effect must materialize defaults at boot",
            )
            assert_eq(
                blob["schema_version"], SCHEMA_VERSION,
                "(1.a) boot blob schema=1",
            )
            assert_eq(blob["todos"], [], "(1.b) boot blob: no todos")
            assert_eq(blob["filter"], "All", "(1.c) default filter All")
            assert_eq(blob["next_id"], 1, "(1.d) next_id starts at 1")
            assert_eq(collect_item_ids(tf), [], "(1.e) empty list painted")

            # ── (2) Add three rows via R666 #3 character arc ───────
            for body_text in ("milk", "eggs", "bread"):
                add_todo(tf, body_text)
            wait_until(
                lambda: len(collect_item_ids(tf)) == 3,
                desc="(2.a) three rows materialised",
            )
            ids = collect_item_ids(tf)
            blob = wait_blob(
                storage_dir, lambda b: len(b.get("todos", [])) == 3,
                "(2.b) three rows persisted to disk",
            )
            assert_eq(
                [t["text"] for t in blob["todos"]],
                ["milk", "eggs", "bread"],
                "(2.c) row texts persisted in insertion order",
            )
            for t in blob["todos"]:
                assert_eq(
                    t["completed"], False,
                    f"(2.d) row {t['id']!r} starts un-completed",
                )
            row0_id, row1_id, row2_id = ids[0], ids[1], ids[2]

            # ── (3) Toggle row[0] via scene/invoke v1 path ─────────
            # /todo_toggle/external/toggle — ExtraExternal singleton
            # addressing per R666 #1. The TodoToggleExternal is a
            # singleton in the state scene tagged `todo_toggle`; the
            # `#<id>` composite is a paint-side click-routing artefact.
            # The direct typed invoke replaces the paint-side click
            # arc, exactly the workflow an AI client would prefer.
            result = tf.invoke(
                f"/{TOGGLE_TAG}/external/toggle", row0_id,
            )
            assert_eq(
                result, True,
                "(3.a) v1 invoke returns Bool(true) post-toggle "
                "(row now completed)",
            )
            wait_until(
                lambda: row_completed(tf, row0_id),
                desc="(3.b) row[0] paint reflects completed=true after v1 invoke",
            )
            assert not row_completed(tf, row1_id), (
                "(3.c) row[1] paint still un-completed (no cross-row leak)"
            )
            blob = read_state_blob(storage_dir)
            persisted_completed = [
                int(t["id"]) for t in blob["todos"] if t["completed"]
            ]
            assert_eq(
                persisted_completed, [row0_id],
                "(3.d) exactly row[0] persisted as completed",
            )

            # ── (4) Filter cycle to Active via scene/key arrow ─────
            # ArrowRight is a W3C named key (>1 char) so R666 #3
            # routes through `handle_named_key`. The RadioGroupExternal
            # state machine cycles 0 → 1 → 2 → 0 on right-arrow with
            # the focused filter button.
            focus_set(tf, FILTER_TAG)
            tf.key(path=FILTER_TAG, name="ArrowRight")
            blob = wait_blob(
                storage_dir, lambda b: b.get("filter") == "Active",
                "(4.a) ArrowRight cycled filter All → Active",
            )
            ids_in_active = collect_item_ids(tf)
            assert row0_id not in ids_in_active, (
                "(4.b) Active filter hides the completed row from paint"
            )
            assert row1_id in ids_in_active and row2_id in ids_in_active, (
                "(4.c) Active filter keeps un-completed rows visible"
            )

            # ── (5) Begin edit on row[1] via scene/invoke v1 ───────
            # /todo_item/external/begin — TodoEditExternal is the
            # singleton tagged `todo_item` (ITEM_TAG); its `begin`
            # action mirrors the double-click activation arc without
            # the paint-side detection. The `#<id>` composite is the
            # paint-side row tag, distinct from the state-scene tag.
            begin_result = tf.invoke(
                f"/{ITEM_TAG}/external/begin", row1_id,
            )
            assert_eq(
                begin_result, True,
                "(5.a) v1 invoke begin returns Bool(true) (row in edit mode)",
            )
            # ── (6) Backspace + retype via R666 #3 character arc ───
            # The edit TextField is pre-filled with "eggs"; clear it
            # by sending 4 backspaces, then type "tofu".
            for _ in range(4):
                tf.key(path=EDIT_TF_TAG, name="Backspace")
            tf.text("tofu", path=EDIT_TF_TAG)

            # ── (7) Enter commit ───────────────────────────────────
            tf.key(path=EDIT_TF_TAG, name="Enter")

            # ── (8) Verify post-edit row text ──────────────────────
            blob = wait_blob(
                storage_dir,
                lambda b: any(
                    int(t["id"]) == row1_id and t["text"] == "tofu"
                    for t in b.get("todos", [])
                ),
                "(8.a/8.b) edited row text persisted after commit",
            )
            edited_row = next(
                (t for t in blob["todos"] if int(t["id"]) == row1_id), None,
            )
            assert edited_row is not None, (
                "(8.a) edited row still present in persisted blob"
            )
            assert_eq(
                edited_row["text"], "tofu",
                "(8.b) row[1] text replaced via character arc + commit",
            )
            assert_eq(
                edited_row["completed"], False,
                "(8.c) edit preserved completion=false (orthogonal axes)",
            )
            # Other rows untouched.
            untouched = {
                int(t["id"]): t["text"] for t in blob["todos"]
                if int(t["id"]) != row1_id
            }
            assert_eq(
                untouched.get(row0_id), "milk",
                "(8.d) row[0] text 'milk' untouched by row[1] edit",
            )
            assert_eq(
                untouched.get(row2_id), "bread",
                "(8.e) row[2] text 'bread' untouched by row[1] edit",
            )
            saved_next_id = blob["next_id"]
            saved_max_id = max(int(t["id"]) for t in blob["todos"])
            assert saved_next_id > saved_max_id, (
                f"(8.f) next_id ({saved_next_id}) advanced past max id "
                f"({saved_max_id})"
            )

        # ── (9) Process exited via context manager ─────────────────
        # The R665 save Effect flushed during shutdown (atomic write).
        # Verify no .tmp leftovers — atomic rename contract.
        siblings = sorted(p.name for p in storage_dir.iterdir())
        assert STATE_KEY in siblings, (
            "(9.a) persisted blob survives subprocess exit"
        )
        assert all(not n.endswith(".tmp") for n in siblings), (
            f"(9.b) no .tmp leftovers after kill: {siblings}"
        )

        # ──────────────────────────────────────────────────────────
        # Cycle 2 — relaunch + verify persistence + further mutations
        # ──────────────────────────────────────────────────────────
        with make_subprocess(storage_dir) as tf:
            # ── (10) Relaunch — disk is the source of truth ────────
            blob = read_state_blob(storage_dir)
            persisted_ids = sorted(int(t["id"]) for t in blob["todos"])
            assert_eq(
                persisted_ids, sorted([row0_id, row1_id, row2_id]),
                "(10.a) all three rows survive relaunch",
            )

            # ── (11) Hydrated state matches the cycle-1 mutations ──
            persisted_completed = [
                int(t["id"]) for t in blob["todos"] if t["completed"]
            ]
            assert_eq(
                persisted_completed, [row0_id],
                "(11.a) row[0] completion preserved across relaunch",
            )
            assert_eq(
                blob["filter"], "Active",
                "(11.b) filter Active preserved across relaunch",
            )
            row1_persisted = next(
                t for t in blob["todos"] if int(t["id"]) == row1_id
            )
            assert_eq(
                row1_persisted["text"], "tofu",
                "(11.c) row[1] edited text 'tofu' preserved across relaunch",
            )
            assert_eq(
                blob["next_id"], saved_next_id,
                "(11.d) next_id preserved exactly (no off-by-one drift)",
            )
            # Editing state must NOT survive (transient by design).
            assert "editing_id" not in blob, (
                "(11.e) editing_id not persisted (transient UI state)"
            )

            # Cross-verify the painted scene against the disk state.
            # Filter Active hides row[0] from paint but storage still
            # carries it.
            painted_ids = collect_item_ids(tf)
            assert row0_id not in painted_ids, (
                "(11.f) Active filter hides persisted-completed row in paint"
            )
            assert row1_id in painted_ids and row2_id in painted_ids, (
                "(11.g) un-completed rows visible after relaunch"
            )

            # ── (12) Add one more row via R666 #3 character arc ────
            add_todo(tf, "honey")
            blob = wait_blob(
                storage_dir, lambda b: len(b.get("todos", [])) == 4,
                "(12) honey row persisted",
            )
            new_ids = [int(t["id"]) for t in blob["todos"]]
            fresh = [i for i in new_ids if i not in persisted_ids]
            assert_eq(
                len(fresh), 1,
                "(12.a) exactly one fresh id allocated for 'honey'",
            )
            assert_eq(
                fresh[0], saved_next_id,
                "(12.b) fresh id == persisted next_id (no collision, "
                "no max + 1 short-circuit)",
            )
            honey_id = fresh[0]
            honey_row = next(t for t in blob["todos"] if int(t["id"]) == honey_id)
            assert_eq(
                honey_row["text"], "honey",
                "(12.c) 'honey' text persisted under fresh id",
            )

            # ── (13) Toggle off the persisted-completed row via v1 ──
            # /todo_toggle/external/toggle — row0 is hidden in paint
            # (filter Active) but the state-scene singleton accepts
            # the id directly through the typed invoke channel,
            # bypassing the paint-side click router entirely.
            toggle_result = tf.invoke(
                f"/{TOGGLE_TAG}/external/toggle", row0_id,
            )
            assert_eq(
                toggle_result, False,
                "(13.a) v1 invoke toggle returns Bool(false) post-toggle "
                "(row now un-completed)",
            )
            blob = wait_blob(
                storage_dir,
                lambda b: [int(t["id"]) for t in b.get("todos", []) if t["completed"]] == [],
                "(13.b) no completed rows after toggling off the only one",
            )
            # Paint now shows row[0] (back to Active filter set).
            painted_after = collect_item_ids(tf)
            assert row0_id in painted_after, (
                "(13.c) row[0] visible in paint after toggle-off (now active)"
            )

            # ── (14) Delete row via scene/invoke v1 ────────────────
            # /todo_delete/external/delete on row[2]. Verify the
            # invoke returns Bool(true) (row was present) and the
            # subsequent paint + storage drop the row.
            delete_result = tf.invoke(
                f"/{DELETE_TAG}/external/delete", row2_id,
            )
            assert_eq(
                delete_result, True,
                "(14.a) v1 invoke delete returns Bool(true) "
                "(row was present)",
            )
            wait_until(
                lambda: row2_id not in collect_item_ids(tf),
                desc="(14.b) deleted row absent from painted scene",
            )
            blob = read_state_blob(storage_dir)
            disk_ids_after_delete = [int(t["id"]) for t in blob["todos"]]
            assert row2_id not in disk_ids_after_delete, (
                "(14.c) deleted row absent from persisted blob"
            )
            # Repeat-delete is idempotent — returns Bool(false).
            repeat_delete = tf.invoke(
                f"/{DELETE_TAG}/external/delete", row2_id,
            )
            assert_eq(
                repeat_delete, False,
                "(14.d) repeat delete on a removed id returns Bool(false)",
            )

            # Capture survivor set for cycle 3 verification.
            saved_survivors = sorted(disk_ids_after_delete)

        # ──────────────────────────────────────────────────────────
        # Cycle 3 — second relaunch verifies delete + toggle persist
        # ──────────────────────────────────────────────────────────
        with make_subprocess(storage_dir) as tf:
            # ── (15) Second relaunch ──────────────────────────────
            blob = read_state_blob(storage_dir)
            cycle3_ids = sorted(int(t["id"]) for t in blob["todos"])
            assert_eq(
                cycle3_ids, saved_survivors,
                "(15.a) delete persisted across cycle 3 — survivors only",
            )
            assert row2_id not in cycle3_ids, (
                "(15.b) deleted row[2] stays deleted after relaunch"
            )

            # ── (16) Toggle off + delete persisted across cycles ───
            cycle3_completed = [
                int(t["id"]) for t in blob["todos"] if t["completed"]
            ]
            assert_eq(
                cycle3_completed, [],
                "(16.a) toggle-off persisted (no rows completed)",
            )
            # row[1] still carries the edited text "tofu".
            row1_in_cycle3 = next(
                (t for t in blob["todos"] if int(t["id"]) == row1_id), None,
            )
            assert row1_in_cycle3 is not None, (
                "(16.b) row[1] survives all three cycles"
            )
            assert_eq(
                row1_in_cycle3["text"], "tofu",
                "(16.c) row[1] edited text persists across both relaunches",
            )
            # 'honey' row also persisted.
            honey_in_cycle3 = next(
                (t for t in blob["todos"] if int(t["id"]) == honey_id), None,
            )
            assert honey_in_cycle3 is not None, (
                "(16.d) row added in cycle 2 ('honey') survives cycle 3"
            )
            assert_eq(
                honey_in_cycle3["text"], "honey",
                "(16.e) 'honey' text preserved through cycle-3 relaunch",
            )
            # Filter still Active.
            assert_eq(
                blob["filter"], "Active",
                "(16.f) filter Active persists through the full cycle chain",
            )
            # next_id advanced past every id allocated so far.
            current_max = max(cycle3_ids) if cycle3_ids else 0
            assert blob["next_id"] > current_max, (
                f"(16.g) next_id ({blob['next_id']}) > current max "
                f"({current_max}) — monotonic invariant"
            )

            # ── Final cross-process verification through RPC ──────
            # Even after three lifecycle boundaries, the running
            # process exposes the same survivors through scene/snapshot.
            painted_in_cycle3 = collect_item_ids(tf)
            assert set(painted_in_cycle3).issubset(set(cycle3_ids)), (
                f"(16.h) painted ids subset of persisted set "
                f"(painted={painted_in_cycle3}, persisted={cycle3_ids})"
            )

    finally:
        shutil.rmtree(storage_dir, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(run_demo(
        "R666 §5.34 §5.37 — 12+ step E2E (invoke v1 + char-key + persistence)",
        body,
    ))
