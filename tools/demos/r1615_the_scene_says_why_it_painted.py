#!/usr/bin/env python3
"""R1615 §5.36 §5.12 §2 #7 — a painted run says what it *is*, and the scene
answers.

Every framework that draws styled content keeps a list of ranges: "these bytes
take this format". The list decides the picture and then the picture is all
that is left, because the run has no identity apart from its ink. Ask "why is
this word blue" and the honest answer is "because something set it blue".

A syntax highlighter is the sharpest case, because the classification is
computed and then thrown away. `highlight_code` decides *keyword*, *string*,
*comment*, *number* — and used to keep only a colour. Worse, the colour is not
even a stable name for the class: the light and dark schemes paint the same
class two different inks, so a reader matching on ink gets a theme-dependent
answer to a theme-independent question.

R1615 gives a run a **name** and the scene a way to be asked. This demo drives
the code editor over RPC and checks, without a pixel:

  * **Every coloured token names its class**, at the exact byte range, and the
    name matches the palette colour the same run carries — the ink and the
    reason for it are one declaration, so they cannot drift apart.
  * **A byte answers with its stack.** `scene/marks {tag, index}` takes a
    position and returns every run covering it, innermost last, plus `top` —
    the one the painter obeyed.
  * **The index space is stated.** A text node's runs are over UTF-8 bytes, and
    the answer says so rather than leaving a client to assume; a multi-byte
    character in the buffer is where an assumed index space would be wrong.
  * **The three ways of having no answer are three answers.** A node that could
    name its runs and did not, a node whose *kind* has nothing to attribute,
    and a tag that names nothing are distinct — and the middle one says which
    kind of nothing on the wire.
  * **Editing re-derives it.** The names follow the content, because they come
    from the same tokenise the colours do.

Run from the workspace root:
    cargo build -p hello-syntax-highlight --release
    python3 tools/demos/r1615_the_scene_says_why_it_painted.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    assert_eq,
    assert_rpc_error,
    run_demo,
    wait_until,
)

VIEWPORT = (480, 200)
EDITOR_TAG = "code_editor"
#: The field's own text node (`pinion_widget_paint::text_field::field_text_tag`).
TEXT_TAG = f"{EDITOR_TAG}-text"
STATUS_TAG = "syntax_status"

SEED = 'let n = 42 + "x" // sum'

#: `pinion_core::syntax::token` — the published class names.
KEYWORD, STRING, COMMENT, NUMBER = "keyword", "string", "comment", "number"

#: `SyntaxPalette::classic`, by the exact bytes. The demo asserts the NAME and
#: the INK agree, which is the property that makes the name trustworthy.
INK = {
    KEYWORD: (0x00, 0x00, 0xC0),
    STRING: (0xA3, 0x15, 0x15),
    COMMENT: (0x00, 0x80, 0x00),
    NUMBER: (0x09, 0x86, 0x58),
}


def style_runs(ed: RpcSubprocess) -> list[dict]:
    """The field's runs as the editor's own oracle reports them — ink, range,
    and (since R1615) the class name."""
    return ed.query("/external/style_runs")


def marks(ed: RpcSubprocess, index: int | None = None) -> dict:
    return ed.marks(TEXT_TAG, index, viewport=VIEWPORT)


def names_at(ed: RpcSubprocess, index: int) -> list[str]:
    return ed.mark_names(TEXT_TAG, index, viewport=VIEWPORT)


def set_text(ed: RpcSubprocess, text: str, expect_runs: int) -> None:
    ed.intervene("/external/text", text)
    wait_until(
        lambda: len(style_runs(ed)) == expect_runs,
        desc=f"{text!r} -> {expect_runs} runs",
    )
    # ...and the published marks catch up with the same edit, from the same
    # tokenise. Waiting on the paint separately is what would let the two drift.
    wait_until(
        lambda: len(marks(ed).get("runs") or []) == expect_runs,
        desc=f"{text!r} -> {expect_runs} published marks",
    )


def body() -> None:
    with RpcSubprocess("hello-syntax-highlight", request_timeout=12.0) as ed:
        checks(ed)


def checks(ed: RpcSubprocess) -> None:
    wait_until(lambda: len(style_runs(ed)) == 4, desc="the seed line tokenises")

    # --- every coloured token names its class -----------------------------
    runs = style_runs(ed)
    got = [(r["name"], SEED[r["start"] : r["end"]]) for r in runs]
    assert_eq(
        got,
        [
            (KEYWORD, "let"),
            (NUMBER, "42"),
            (STRING, '"x"'),
            (COMMENT, "// sum"),
        ],
        "each run says what it is, at its own byte range",
    )
    for run in runs:
        ink = run["style"]["fg_color"]
        assert_eq(
            (ink["r"], ink["g"], ink["b"]),
            INK[run["name"]],
            f"the {run['name']} run's ink is the {run['name']} colour",
        )

    # --- the scene answers, and says what its indices count ----------------
    answer = marks(ed)
    assert_eq(answer["kind"], "Text", "the field's text node carries the runs")
    assert_eq(answer["channel"], "carries", "a Text node can be attributed")
    assert_eq(answer["published"], True, "and this one did")
    assert_eq(
        answer["domain"],
        "utf8_byte",
        "the index space is stated rather than assumed",
    )
    assert_eq(len(answer["runs"]), 4, "one published run per coloured token")
    assert_eq(answer["runs"][0]["name"], KEYWORD, "in declaration order")
    assert_eq(answer["runs"][3]["name"], COMMENT, "...comment last")
    assert_eq(
        [(r["start"], r["end"]) for r in answer["runs"]],
        [(r["start"], r["end"]) for r in runs],
        "the published runs are the runs the field paints -- one list",
    )

    # --- a byte answers with its stack -------------------------------------
    assert_eq(names_at(ed, 0), [KEYWORD], "`l` of `let`")
    assert_eq(names_at(ed, 2), [KEYWORD], "`t` of `let`")
    assert_eq(names_at(ed, 3), [], "the space after it is uncoloured")
    assert_eq(names_at(ed, 4), [], "`n` is an identifier -- no run, no name")
    assert_eq(names_at(ed, 8), [NUMBER], "`4` of `42`")
    assert_eq(names_at(ed, 13), [STRING], "inside the string literal")
    assert_eq(names_at(ed, 17), [COMMENT], "inside the comment")
    assert_eq(names_at(ed, 999), [], "past the end of the buffer")
    at = marks(ed, 17)["at"]
    assert_eq(at["index"], 17, "the answer echoes the position it is about")
    assert_eq(at["top"], COMMENT, "the run the painter obeyed")
    assert_eq(at["names"][-1], at["top"], "top is the last of the stack")

    # A position nobody covers is an ANSWER, not a missing key: the request
    # named a position, so the stack is present and empty.
    uncovered = marks(ed, 4)
    assert_eq(uncovered["at"]["names"], [], "no run covers an identifier")
    assert_eq(uncovered["at"]["top"], None, "and nothing is on top")
    assert_eq(uncovered["published"], True, "the node still published its runs")
    # ...whereas not asking about a position omits the stack entirely.
    assert_eq("at" in marks(ed), False, "no position asked, no stack answered")

    # --- the class is the identity, the colour is not ----------------------
    # Two runs of the SAME class have the same name and the same ink; the ink
    # is what a theme swap would change and the name is what it would not.
    set_text(ed, 'let a = 1 let b = 2 // both', 5)
    named = [r["name"] for r in style_runs(ed)]
    assert_eq(named.count(KEYWORD), 2, "two keywords")
    assert_eq(named.count(NUMBER), 2, "two numbers")
    assert_eq(named.count(COMMENT), 1, "one comment")
    keyword_runs = [r for r in style_runs(ed) if r["name"] == KEYWORD]
    assert_eq(
        keyword_runs[0]["style"]["fg_color"],
        keyword_runs[1]["style"]["fg_color"],
        "one class, one ink",
    )
    assert_eq(
        names_at(ed, 0) + names_at(ed, 10),
        [KEYWORD, KEYWORD],
        "both `let`s answer with the same name",
    )

    # --- the index space matters, and the wire says which ------------------
    # `é` is two UTF-8 bytes, so a client counting CHARACTERS would land inside
    # it and read the wrong run. The domain the answer states is what makes
    # that a caller error rather than a silent one.
    set_text(ed, '// é\nlet x = 7', 3)
    assert_eq(marks(ed)["domain"], "utf8_byte", "still UTF-8 bytes")
    assert_eq(names_at(ed, 3), [COMMENT], "the first byte of `é` is comment")
    assert_eq(names_at(ed, 4), [COMMENT], "so is its second byte")
    assert_eq(names_at(ed, 6), [KEYWORD], "`let` begins at byte 6, not byte 5")
    assert_eq(names_at(ed, 5), [], "byte 5 is the newline -- uncoloured")

    # --- editing re-derives the names --------------------------------------
    set_text(ed, "identifiers only", 0)
    assert_eq(style_runs(ed), [], "nothing is coloured")
    bare = marks(ed)
    assert_eq(
        bare["published"],
        False,
        "a node with no NAMED run is silent, not published-empty",
    )
    assert_eq(bare["channel"], "carries", "it could have named runs -- it has none")
    assert_eq("runs" in bare, False, "and there is no run list to read")
    assert_eq("domain" in bare, False, "nor an index space for runs that do not exist")

    set_text(ed, "let x = 9", 2)
    assert_eq(
        [r["name"] for r in style_runs(ed)],
        [KEYWORD, NUMBER],
        "the names come back with the content",
    )
    assert_eq(marks(ed)["published"], True, "and the node publishes again")

    # --- the three ways of having no answer --------------------------------
    # A node whose KIND has nothing to attribute answers, and says which kind
    # of nothing. The status bar's container is structural; it paints nothing
    # of its own, and attribution belongs to its children.
    status = ed.marks(STATUS_TAG, viewport=VIEWPORT)
    assert_eq(status["kind"], "Container", "the status bar is a container")
    assert_eq(status["published"], False, "it published no runs")
    assert_eq(
        status["channel"],
        "structural",
        "and the wire says WHY -- it paints nothing of its own, so attribution "
        "belongs to its children. A client learns that without reading source",
    )
    assert_eq(status["tag"], STATUS_TAG, "the answer echoes what was asked")

    # A tag that names nothing is not an answer at all -- and the refusal is a
    # matchable word, not prose.
    assert_rpc_error(
        lambda: ed.request("scene/marks", {"tag": "no_such_node"}),
        data="UnknownTag: no_such_node",
    )
    assert_rpc_error(
        lambda: ed.request("scene/marks", {"index": 0}),
        data="params.tag missing or not a string",
    )
    assert_rpc_error(
        lambda: ed.request("scene/marks", {"tag": TEXT_TAG, "from": "sideways"}),
        data='params.from "sideways" is not "paint" or "state"',
    )


if __name__ == "__main__":
    sys.exit(run_demo("r1615_the_scene_says_why_it_painted", body))
