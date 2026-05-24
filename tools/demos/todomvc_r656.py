#!/usr/bin/env python3
"""todomvc R656 §5.16 — stable id + delete RPC self-verification demo.

R656 extends the R655 composed-app scaffolding with three application-
tier additions:

  1. **TodoItem** struct (`{id: u64, text: String}`) — `Signal<Vec<
     TodoItem>>` replaces the R655 `Signal<Vec<String>>` so every
     entry carries a stable monotonic id.
  2. **TodoDeleteExternal** singleton — registered via
     `WidgetCore::create_extra_externals` under tag `"todo_delete"`.
     The per-item delete button paints a Container tagged
     `"todo_delete#<id>"` (no per-item External — the singleton owns
     the dispatch). The R51.42 §5.35 composite-tag wire splits the
     paint tag at `#`, walks the state scene for the External with
     primary tag `"todo_delete"`, and forwards the sub-index `<id>`
     to `invoke("send", "<id>:PointerDown")`. The External parses
     the wire and calls `Signal::set_with(retain not matching id)`.
  3. **ARIA `list`/`listitem` roles** (R656 §5.40 AriaRole additions)
     — `WidgetA11y::access_node` emits one `AccessNode` per todo
     plus per-delete `Button` nodes; the list root references items
     by tag through `AccessNode::with_child` so AT cursor traversal
     order matches paint order.

The contract this script pins is the **stable-id invariant**: after
adding three items and deleting the middle one, the surviving items
keep their ORIGINAL `todo_item#<id>` tags (no resequencing to a
fresh 0-based array index). An AI client that captured `id=7` for a
specific row before the delete can still address that same row by
`scene/click {path: "todo_item#7"}` after a sibling delete — the
mapping survives.

Driven sequence (each step ends in a typed assertion; ≥20 asserts
total):

  1. focus/set TF_TAG, type 'alpha', submit Enter → list grows to 1.
  2. type 'beta', submit Enter → list grows to 2.
  3. type 'gamma', submit Enter → list grows to 3.
  4. snapshot → walk LIST_TAG children → extract per-item
     `(id, text)` triples from the `todo_item#<id>` row tags.
     Assert monotonic id allocation + texts match insertion order.
  5. scene/click {path: "todo_delete#<id_beta>"} → middle item
     removed via composite-tag wire (mirror of the visible mouse
     click on the × button).
  6. snapshot → walk → assert SURVIVING items keep their original
     ids (`id_alpha` + `id_gamma`), and `id_beta` is gone.
  7. scene/click {path: "todo_delete#<id_alpha>"} → delete first
     remaining → only gamma survives, still with original id.
  8. scene/click on stale id (`id_beta` already deleted) → RpcError
     (the composite tag is not in the paint scene, so the dispatcher
     correctly rejects rather than silently no-op'ing).
  9. add 'delta' + 'epsilon' → assert their ids continue
     monotonically past `id_gamma`, never reusing `id_alpha` /
     `id_beta`.

The R655 demo verified composition + Enter intercept; R656 verifies
the orthogonal stable-identity axis that every future CRUD axis
(R658 toggle, R660 edit, R661 persistence) layers on top of.
"""

from __future__ import annotations

import re
import sys
import time
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcError, RpcSubprocess, assert_eq, run_demo

TF_TAG = "main_textfield"
LIST_TAG = "todo_list"
ITEM_TAG_RE = re.compile(r"^todo_item#(\d+)$")


def focus_set(tf: RpcSubprocess, tag: str | None) -> None:
    tf.request("focus/set", {"tag": tag})


def type_text(tf: RpcSubprocess, text: str) -> None:
    for ch in text:
        result = tf.invoke("/external/key", ch)
        assert_eq(result, True, f"invoke('key', {ch!r}) recognized")
    time.sleep(0.05)


def submit_enter(tf: RpcSubprocess) -> None:
    tf.key(path=TF_TAG, name="Enter")
    time.sleep(0.1)


def find_node_by_tag(node: dict[str, Any], tag: str) -> dict[str, Any] | None:
    """Depth-first walk for the first node carrying `tag`."""
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


def list_rows(tf: RpcSubprocess) -> list[dict[str, Any]]:
    """Snapshot the paint scene and return the children of LIST_TAG
    *excluding* the placeholder/header (always the first child)."""
    snap = tf.snapshot(source="paint", viewport=(480, 480))
    list_node = find_node_by_tag(snap, LIST_TAG)
    assert list_node is not None, f"snapshot must carry {LIST_TAG} tag"
    children = list_node.get("children") or []
    return list(children[1:])


def parse_item_id_from_row(row: dict[str, Any]) -> int:
    """Each item row is a Container with tag `todo_item#<id>`. Return
    the integer id parsed from the suffix; raises on malformed tag."""
    tag = row.get("tag")
    assert isinstance(tag, str), f"row missing 'tag' (got {row!r})"
    m = ITEM_TAG_RE.match(tag)
    assert m is not None, f"row tag {tag!r} does not match todo_item#<id>"
    return int(m.group(1))


def parse_item_text_from_row(row: dict[str, Any]) -> str:
    """The first Text child of the row is the entry text (the second
    Text child is the × delete glyph)."""
    for child in row.get("children") or []:
        if child.get("type") == "Text":
            content = child.get("content")
            if isinstance(content, str):
                return content
        # Nested Container → recurse one level for safety (the row
        # holds `[entry_text, delete_button]` — both at depth 1, but
        # the entry_text is the FIRST Text we encounter).
    for child in row.get("children") or []:
        sub = child.get("children") or []
        for s in sub:
            if s.get("type") == "Text":
                content = s.get("content")
                if isinstance(content, str):
                    return content
    raise AssertionError(f"row has no Text child: {row!r}")


def list_items(tf: RpcSubprocess) -> list[tuple[int, str]]:
    """Return `[(id, text), ...]` for every visible todo row, in
    paint (= submission) order."""
    return [(parse_item_id_from_row(r), parse_item_text_from_row(r)) for r in list_rows(tf)]


def scene_has_tag(tf: RpcSubprocess, tag: str) -> bool:
    """Cheap predicate — snapshot once + walk for `tag`."""
    snap = tf.snapshot(source="paint", viewport=(480, 480))
    return find_node_by_tag(snap, tag) is not None


def body() -> None:
    with RpcSubprocess("todomvc") as tf:
        # ── (0) Initial posture ────────────────────────────────────
        assert_eq(tf.query("/external/state"), "Idle", "initial state")
        assert_eq(tf.query("/external/text"), "", "initial text")
        assert_eq(list_items(tf), [], "initial list is empty")

        # ── (1) Focus + add three items ───────────────────────────
        focus_set(tf, TF_TAG)
        time.sleep(0.05)
        assert_eq(tf.query("/external/state"), "Focused", "post-focus state")

        for word in ("alpha", "beta", "gamma"):
            type_text(tf, word)
            assert_eq(
                tf.query("/external/text"),
                word,
                f"typed {word!r}",
            )
            submit_enter(tf)
            assert_eq(
                tf.query("/external/text"),
                "",
                f"Enter clears field after {word!r}",
            )

        items = list_items(tf)
        assert_eq(len(items), 3, "three items present")
        assert_eq(
            [t for (_id, t) in items],
            ["alpha", "beta", "gamma"],
            "submission order preserved in paint scene",
        )

        # Capture per-item ids — the R656 stable-id contract depends
        # on these surviving subsequent deletes verbatim.
        id_alpha, id_beta, id_gamma = (items[0][0], items[1][0], items[2][0])

        # R656 — ids must be strictly monotonic + unique. The Enter
        # handler calls `allocate_todo_id` which advances the
        # `Owner::cache`-backed `Cell<u64>` counter.
        assert id_alpha < id_beta < id_gamma, (
            f"ids must be strictly increasing — got {id_alpha} < {id_beta} < {id_gamma}"
        )
        assert_eq(
            len({id_alpha, id_beta, id_gamma}),
            3,
            "every allocated id is unique",
        )

        # ── (2) Delete the middle item via composite-tag click ────
        # The paint scene carries a Container tagged
        # `todo_delete#<id_beta>` at the row's right edge; the
        # `scene/click {path}` walker finds the rect, clicks at its
        # centre, and the R51.42 composite-tag wire routes the
        # PointerDown to TodoDeleteExternal.invoke("send",
        # "<id_beta>:PointerDown").
        tf.click(path=f"todo_delete#{id_beta}")
        time.sleep(0.1)

        items_after_middle_delete = list_items(tf)
        assert_eq(
            len(items_after_middle_delete),
            2,
            "list shrank after middle delete",
        )

        # R656 STABLE-ID CONTRACT — surviving items keep their
        # ORIGINAL ids (NOT resequenced to a fresh array index).
        surviving_ids = [i for (i, _t) in items_after_middle_delete]
        assert_eq(
            surviving_ids,
            [id_alpha, id_gamma],
            "alpha + gamma survive WITH ORIGINAL IDS — no resequencing",
        )
        assert_eq(
            [t for (_id, t) in items_after_middle_delete],
            ["alpha", "gamma"],
            "surviving texts in original order",
        )

        # The deleted item's tag is GONE from the paint scene.
        assert not scene_has_tag(tf, f"todo_item#{id_beta}"), (
            f"todo_item#{id_beta} (beta) must not be present after delete"
        )
        assert scene_has_tag(tf, f"todo_item#{id_alpha}"), (
            f"todo_item#{id_alpha} (alpha) must still be present"
        )
        assert scene_has_tag(tf, f"todo_item#{id_gamma}"), (
            f"todo_item#{id_gamma} (gamma) must still be present"
        )

        # ── (3) Delete the first remaining item ───────────────────
        tf.click(path=f"todo_delete#{id_alpha}")
        time.sleep(0.1)

        items_after_alpha_delete = list_items(tf)
        assert_eq(
            len(items_after_alpha_delete),
            1,
            "only one item left after alpha delete",
        )
        assert_eq(
            items_after_alpha_delete[0],
            (id_gamma, "gamma"),
            "gamma keeps its original id after sibling deletes",
        )

        # ── (4) Click on a stale id → RpcError ────────────────────
        # `id_beta` is already deleted; the `scene/click` walker
        # cannot find the tag in the paint scene and returns
        # invalid_params (rather than silently no-op'ing). This is
        # the AI-side feedback signal — a stale tag is a bug to
        # surface, not silently swallow.
        try:
            tf.click(path=f"todo_delete#{id_beta}")
        except RpcError as e:
            assert "not found" in str(e).lower() or "params" in str(e).lower(), (
                f"stale-tag click must surface a path-not-found error — got {e!r}"
            )
        else:
            raise AssertionError(
                f"click on stale tag todo_delete#{id_beta} should have raised"
            )

        # ── (5) Add more items — verify monotonic id allocation ────
        for word in ("delta", "epsilon"):
            type_text(tf, word)
            submit_enter(tf)

        items_after_grow = list_items(tf)
        assert_eq(len(items_after_grow), 3, "list grew back to 3 items")
        # Gamma is still index 0, then delta, then epsilon.
        assert_eq(
            [t for (_id, t) in items_after_grow],
            ["gamma", "delta", "epsilon"],
            "post-grow text order",
        )

        # Critical: the new items' ids are GREATER THAN id_gamma —
        # the counter never reuses retired ids (id_alpha / id_beta).
        id_delta = items_after_grow[1][0]
        id_epsilon = items_after_grow[2][0]
        assert id_delta > id_gamma, (
            f"delta id ({id_delta}) must be > gamma id ({id_gamma})"
        )
        assert id_epsilon > id_delta, (
            f"epsilon id ({id_epsilon}) must be > delta id ({id_delta})"
        )
        # Specifically, the retired ids stay retired — neither new
        # item recycled `id_alpha` or `id_beta`.
        new_ids = {id_delta, id_epsilon}
        assert id_alpha not in new_ids, (
            f"retired id {id_alpha} (alpha) must NOT be reused by new items"
        )
        assert id_beta not in new_ids, (
            f"retired id {id_beta} (beta) must NOT be reused by new items"
        )


if __name__ == "__main__":
    sys.exit(run_demo("todomvc R656", body))
