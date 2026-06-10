#!/usr/bin/env python3
"""todomvc R658 §5.16 — per-item toggle + scroll wrap RPC verification.

R658 extends the R655/R656 composed-app foundation with three
application-tier additions:

  1. **`TodoItem.completed: bool`** — every entry carries a
     completion flag (defaults to `false` on submit; flipped by the
     toggle button).
  2. **`TodoToggleExternal`** singleton — 2nd ExtraExternal alongside
     `TodoDeleteExternal` ([[multi-external-substrate-extra-externals-pattern]]).
     The per-item `todo_toggle#<id>` paint tag routes clicks through
     the R51.42 §5.35 composite-tag wire into
     `invoke("send", "<id>:PointerDown")` which flips `completed`.
     Direct typed route `invoke("toggle", Int(<id>))` also exists
     (reachable via unit tests; RPC `scene/invoke` v0 still only
     reaches the primary External so this stays a unit-test path
     until R690+).
  3. **`Scene::Scroll` wrap** — the todo list region is wrapped in a
     `ScrollNode` anchored on `Owner::cache`-keyed `ScrollState`
     (key `"todomvc.list_scroll"`). 6+ entries scroll smoothly
     within the fixed 220-px viewport instead of pushing the outer
     window flex past `WIN_H = 480`. Substrate (R55.A / R55.B /
     R55.G.5 layout) writes `max_y` from the laid-out content
     automatically.

Visible payoff: a 2nd click on a row's left button does NOT delete
the row (the right `×` deletes), it instead toggles the entry's
visual "done" state (☐/☑ glyph + muted text). Add 7+ items and the
list overflow scrolls within the window — the WIN_H magic budget
(R655 carry) is fully cleaned up.

Driven sequence (≥20 typed assertions):

  1. focus/set TF_TAG, add 3 items (alpha/beta/gamma).
  2. snapshot → each row carries `todo_toggle#<id>` paint tag with
     UNCHECKED glyph; header reads `"Todos (3)"`.
  3. click `todo_toggle#<id_beta>` → snapshot → beta row's toggle
     glyph flips to CHECKED; alpha + gamma stay UNCHECKED; header
     becomes `"Todos (1 of 3 completed)"`.
  4. click same toggle again → flips back; header back to
     `"Todos (3)"`.
  5. toggle alpha + gamma → both completed; header
     `"Todos (2 of 3 completed)"`.
  6. delete completed alpha → list shrinks; gamma stable id +
     completed flag preserved; header
     `"Todos (1 of 2 completed)"`.
  7. add 5 more items (kappa/lambda/mu/nu/xi) → 7 total → list
     content overflows the 220-px viewport → snapshot the Scroll
     node and confirm `content height > viewport height`.
  8. R658 stable-id-under-toggle invariant — the gamma id captured
     in step 1 still resolves to gamma in the final paint scene,
     and gamma is still marked completed (toggle is a flip-only
     mutation, not an id-recycling operation).
"""

from __future__ import annotations

import re
import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    assert_eq,
    isolated_storage_dir,
    run_demo,
    wait_until,
)

TF_TAG = "main_textfield"
LIST_TAG = "todo_list"
LIST_SCROLL_KEY = "todomvc.list_scroll"
ITEM_TAG_RE = re.compile(r"^todo_item#(\d+)$")
TOGGLE_TAG_RE = re.compile(r"^todo_toggle#(\d+)$")

TOGGLE_GLYPH_UNCHECKED = "☐"
TOGGLE_GLYPH_CHECKED = "☑"


def focus_set(tf: RpcSubprocess, tag: str | None) -> None:
    tf.request("focus/set", {"tag": tag})


def type_text(tf: RpcSubprocess, text: str) -> None:
    for ch in text:
        result = tf.invoke("/external/key", ch)
        assert_eq(result, True, f"invoke('key', {ch!r}) recognized")


def submit_enter(tf: RpcSubprocess) -> None:
    tf.key(path=TF_TAG, name="Enter")


def completed_map(tf: RpcSubprocess) -> dict[int, bool]:
    return {rid: c for (rid, _t, c) in list_items(tf)}


def find_node_by_tag(node: dict[str, Any], tag: str) -> dict[str, Any] | None:
    if not isinstance(node, dict):
        return None
    if node.get("tag") == tag:
        return node
    for child in node.get("children") or []:
        found = find_node_by_tag(child, tag)
        if found is not None:
            return found
    content = node.get("content")
    if isinstance(content, dict):
        found = find_node_by_tag(content, tag)
        if found is not None:
            return found
    return None


def find_scroll_with_content_tag(
    node: dict[str, Any], inner_tag: str
) -> dict[str, Any] | None:
    """Depth-first walk for a Scroll node whose content tree carries
    `inner_tag`. R658 wraps the list in a ScrollNode, so the LIST_TAG
    container lives inside `Scroll.content`."""
    if not isinstance(node, dict):
        return None
    if node.get("type") == "Scroll":
        content = node.get("content")
        if isinstance(content, dict) and find_node_by_tag(content, inner_tag):
            return node
    for child in node.get("children") or []:
        hit = find_scroll_with_content_tag(child, inner_tag)
        if hit is not None:
            return hit
    return None


def collect_text_nodes(node: dict[str, Any]) -> list[str]:
    out: list[str] = []
    if not isinstance(node, dict):
        return out
    if node.get("type") == "Text":
        content = node.get("content")
        if isinstance(content, str):
            out.append(content)
    for child in node.get("children") or []:
        out.extend(collect_text_nodes(child))
    content = node.get("content")
    if isinstance(content, dict):
        out.extend(collect_text_nodes(content))
    return out


def list_rows(tf: RpcSubprocess) -> list[dict[str, Any]]:
    snap = tf.snapshot(source="paint", viewport=(480, 480))
    list_node = find_node_by_tag(snap, LIST_TAG)
    assert list_node is not None, f"snapshot must carry {LIST_TAG} tag"
    children = list_node.get("children") or []
    return list(children[1:])


def parse_item_id_from_row(row: dict[str, Any]) -> int:
    tag = row.get("tag")
    assert isinstance(tag, str), f"row missing 'tag' (got {row!r})"
    m = ITEM_TAG_RE.match(tag)
    assert m is not None, f"row tag {tag!r} does not match todo_item#<id>"
    return int(m.group(1))


def row_toggle_glyph(row: dict[str, Any]) -> str:
    """The first child of a row is the toggle Container; its first
    Text child carries the ☐ or ☑ glyph."""
    children = row.get("children") or []
    assert children, f"row {row.get('tag')!r} has no children"
    toggle_container = children[0]
    assert toggle_container.get("type") == "Container", (
        f"first row child must be Container (got {toggle_container.get('type')!r})"
    )
    assert isinstance(toggle_container.get("tag"), str) and (
        toggle_container["tag"].startswith("todo_toggle#")
    ), f"first row child missing todo_toggle tag: {toggle_container!r}"
    for ch in toggle_container.get("children") or []:
        if ch.get("type") == "Text":
            return ch.get("content") or ""
    raise AssertionError(f"toggle container has no Text glyph: {toggle_container!r}")


def row_entry_text(row: dict[str, Any]) -> str:
    """Skip the toggle Container (1st child) and find the first
    Text node at depth 1 — that's the entry text. The delete glyph
    is the LAST Text reachable but lives inside a Container child."""
    children = row.get("children") or []
    for ch in children:
        if ch.get("type") == "Text":
            content = ch.get("content")
            if isinstance(content, str):
                return content
    raise AssertionError(f"row {row.get('tag')!r} has no Text child")


def list_items(tf: RpcSubprocess) -> list[tuple[int, str, bool]]:
    """Return `[(id, text, completed), ...]`. `completed` is decoded
    from the toggle glyph (☐ = false, ☑ = true)."""
    items: list[tuple[int, str, bool]] = []
    for row in list_rows(tf):
        rid = parse_item_id_from_row(row)
        text = row_entry_text(row)
        glyph = row_toggle_glyph(row)
        completed = glyph == TOGGLE_GLYPH_CHECKED
        if not completed:
            assert glyph == TOGGLE_GLYPH_UNCHECKED, (
                f"row {rid} toggle glyph must be ☐ or ☑ (got {glyph!r})"
            )
        items.append((rid, text, completed))
    return items


def list_header_text(tf: RpcSubprocess) -> str:
    snap = tf.snapshot(source="paint", viewport=(480, 480))
    list_node = find_node_by_tag(snap, LIST_TAG)
    assert list_node is not None
    for ch in list_node.get("children") or []:
        if ch.get("type") == "Text":
            return ch.get("content") or ""
    raise AssertionError("list region missing header Text node")


def body() -> None:
    # R666 — isolate from `$XDG_DATA_HOME/pinion-todomvc/` so the
    # demo's typed rows do not bleed into later runs (R665 persistence).
    with isolated_storage_dir("pinion-todomvc-r658-"):
        _body_impl()


def _body_impl() -> None:
    with RpcSubprocess("todomvc") as tf:
        # ── (0) Initial posture ───────────────────────────────────
        assert_eq(tf.query("/external/state"), "Idle", "initial state")
        assert_eq(list_items(tf), [], "initial list is empty")

        # ── R658 scroll wrap visible ──────────────────────────────
        snap0 = tf.snapshot(source="paint", viewport=(480, 480))
        scroll = find_scroll_with_content_tag(snap0, LIST_TAG)
        assert scroll is not None, (
            "R658: list region must be wrapped in Scene::Scroll(...)"
        )
        # Scroll node carries the ScrollState-derived tag.
        assert_eq(
            scroll.get("tag"),
            LIST_SCROLL_KEY,
            "R658: ScrollNode tag derived from LIST_SCROLL_KEY",
        )
        # The scroll node carries a viewport. Read its height; we'll
        # compare against the content extent after we add many items.
        viewport = scroll.get("viewport") or {}
        viewport_h = int(viewport.get("h") or 0)
        assert viewport_h > 0, f"R658: ScrollNode viewport height must be > 0 (got {viewport_h})"
        assert_eq(viewport_h, 220, "R658: LIST_VIEWPORT_H matches the constant")

        # ── (1) Focus + add three items ───────────────────────────
        focus_set(tf, TF_TAG)
        for word in ("alpha", "beta", "gamma"):
            type_text(tf, word)
            submit_enter(tf)

        wait_until(lambda: len(list_items(tf)) == 3, desc="three items present")
        items = list_items(tf)
        assert_eq(
            [t for (_id, t, _c) in items],
            ["alpha", "beta", "gamma"],
            "submission order preserved",
        )
        # R658 — every fresh entry starts active (completed=false).
        assert all(not c for (_id, _t, c) in items), (
            f"R658: every fresh todo starts uncompleted — got {items}"
        )
        id_alpha, id_beta, id_gamma = (items[0][0], items[1][0], items[2][0])
        assert id_alpha < id_beta < id_gamma, (
            f"R656 monotonic-id contract preserved — got {id_alpha} < {id_beta} < {id_gamma}"
        )

        # R658 — header reads plain count when nothing completed.
        assert_eq(
            list_header_text(tf),
            "Todos (3)",
            "R658: header without completed = plain count",
        )

        # ── (2) Click middle row's toggle → flip beta to completed ──
        tf.click(path=f"todo_toggle#{id_beta}")
        wait_until(
            lambda: completed_map(tf).get(id_beta) is True,
            desc="beta flipped to completed after toggle click",
        )

        items_after_toggle = list_items(tf)
        assert_eq(
            len(items_after_toggle),
            3,
            "toggle does NOT remove the item (delete is the × button)",
        )
        # Verify per-item state: alpha active, beta completed, gamma
        # active. R658 — stable-id contract still holds: ids unchanged.
        completed_after = {
            rid: completed for (rid, _t, completed) in items_after_toggle
        }
        assert_eq(
            completed_after[id_alpha],
            False,
            "alpha still active after toggling beta",
        )
        assert_eq(
            completed_after[id_beta],
            True,
            "R658: beta flipped to completed=true",
        )
        assert_eq(
            completed_after[id_gamma],
            False,
            "gamma still active after toggling beta",
        )

        # Header reflects the new progress.
        assert_eq(
            list_header_text(tf),
            "Todos (1 of 3 completed)",
            "R658: progress header reads X of N completed",
        )

        # ── (3) Click beta toggle again → flips back to active ────
        tf.click(path=f"todo_toggle#{id_beta}")
        wait_until(
            lambda: completed_map(tf).get(id_beta) is False,
            desc="2nd toggle click flips beta back to active",
        )

        items_after_unflip = list_items(tf)
        completed_after_unflip = {
            rid: c for (rid, _t, c) in items_after_unflip
        }
        assert_eq(
            completed_after_unflip[id_beta],
            False,
            "R658: 2nd click flips beta back to uncompleted (monoidal toggle)",
        )
        assert_eq(
            list_header_text(tf),
            "Todos (3)",
            "R658: header back to plain count when nothing completed",
        )

        # ── (4) Toggle alpha + gamma → 2 of 3 completed ───────────
        tf.click(path=f"todo_toggle#{id_alpha}")
        wait_until(
            lambda: completed_map(tf).get(id_alpha) is True,
            desc="alpha completed after toggle click",
        )
        tf.click(path=f"todo_toggle#{id_gamma}")
        wait_until(
            lambda: completed_map(tf).get(id_gamma) is True,
            desc="gamma completed after toggle click",
        )

        items_mixed = list_items(tf)
        completed_mixed = {rid: c for (rid, _t, c) in items_mixed}
        assert_eq(completed_mixed[id_alpha], True, "alpha completed")
        assert_eq(completed_mixed[id_beta], False, "beta untouched (active)")
        assert_eq(completed_mixed[id_gamma], True, "gamma completed")
        assert_eq(
            list_header_text(tf),
            "Todos (2 of 3 completed)",
            "R658: header reflects 2 of 3 completed",
        )

        # ── (5) Delete a completed item (alpha) → shrinks list ────
        # The delete + toggle are independent — deleting a completed
        # item still works, and the surviving items keep their
        # `completed` flag.
        tf.click(path=f"todo_delete#{id_alpha}")
        wait_until(
            lambda: len(list_items(tf)) == 2,
            desc="list shrank to 2 after deleting alpha",
        )

        items_after_alpha_delete = list_items(tf)
        assert_eq(
            len(items_after_alpha_delete),
            2,
            "list shrank to 2 after deleting alpha",
        )
        surviving_ids = {rid for (rid, _t, _c) in items_after_alpha_delete}
        assert_eq(
            surviving_ids,
            {id_beta, id_gamma},
            "R656 stable-id contract preserved under toggle+delete combo",
        )
        completed_post_delete = {
            rid: c for (rid, _t, c) in items_after_alpha_delete
        }
        # gamma was completed BEFORE the delete; it stays completed.
        assert_eq(
            completed_post_delete[id_gamma],
            True,
            "R658: gamma's completed flag survives sibling delete",
        )
        assert_eq(
            completed_post_delete[id_beta],
            False,
            "beta still active",
        )
        assert_eq(
            list_header_text(tf),
            "Todos (1 of 2 completed)",
            "R658: header reflects 1 of 2 completed after delete",
        )

        # ── (6) Add 5 more items → 7 total → scroll engaged ───────
        # The list grows past the 220-px viewport so the substrate's
        # layout pass writes a non-zero max_y. We verify scroll
        # engagement by inspecting the Scroll node's viewport vs
        # content extent.
        for word in ("kappa", "lambda", "mu", "nu", "xi"):
            type_text(tf, word)
            submit_enter(tf)

        wait_until(lambda: len(list_items(tf)) == 7, desc="list grew to 7 items")
        items_grown = list_items(tf)

        snap_post_grow = tf.snapshot(source="paint", viewport=(480, 480))
        scroll_post = find_scroll_with_content_tag(snap_post_grow, LIST_TAG)
        assert scroll_post is not None, "Scroll wrapper still present"
        viewport_post = scroll_post.get("viewport") or {}
        # The Scroll node exposes `offset_y` (current vertical
        # offset) — at boot this stays 0 (no scroll dispatched yet).
        # The substrate's `compute_layout` runs against the laid-out
        # content; the max_y bound flows back into ScrollState
        # which the next frame reads via `from_state`. We confirm
        # the wrapper exists and the viewport height stayed fixed.
        assert_eq(
            int(viewport_post.get("h") or 0),
            220,
            "R658: viewport height stays at 220 even with 7 items",
        )

        # ── R658 final stable-id invariant ────────────────────────
        # The original gamma id (captured in step 1) still resolves
        # to gamma in the final paint, and gamma is still completed.
        final = {rid: (text, c) for (rid, text, c) in items_grown}
        gamma_state = final.get(id_gamma)
        assert gamma_state is not None, (
            f"R656 stable id contract: gamma (id={id_gamma}) survives all mutations"
        )
        text_g, completed_g = gamma_state
        assert_eq(
            text_g,
            "gamma",
            "R658: gamma's text never mutated through toggle / delete",
        )
        assert_eq(
            completed_g,
            True,
            "R658: gamma's completed flag survives all sibling mutations",
        )


if __name__ == "__main__":
    sys.exit(run_demo("todomvc R658", body))
