#!/usr/bin/env python3
"""R1706 §5.38 §5.40 §2 #2 §2 #7 — **a selection is a set, and one of it
leads**, driven through the real window of the analysis tool's node canvas.

# What this exists for

The reference tool's node laboratory has one gesture that acts on many cards at
once: grab a host frame by its tab and it *selects everything that host holds*
and then carries it. Both halves are one act — the behaviour prototype's
frame-drag handler calls its select-the-frame function on its first line and
then moves the members.

This screen had only the second half, and the reason it could not have the
first is that its selection was `Option<NodeId>`: a leader with no set, so
"select these six" had nowhere to land. Measured before this round, through the
wire, on the built binary: pressing either host's tab left `selected` exactly
where it was — six cards visibly moving under the cursor and the inspector still
showing whichever card had been picked minutes earlier.

The sibling node canvas in this tree holds the mirror image — a `BTreeSet` with
no leader — and pays for it in the same place: measured the same way, selecting
two of its nodes makes its `selected` slot answer **nothing at all**, so the one
question a reader asks most ("which one am I looking at?") becomes unanswerable
in exactly the case a set exists for.

The framework half is why neither can come back. `pinion_core::selection` holds
the members *and* the leader, with the leader an index into the members, so a
selection whose inspector follows something unselected is unrepresentable rather
than merely avoided.

Measured on the reference toolkit at 6.11.1 — built as a probe and run
offscreen, not read out of headers — the same fact is missing there twice over.
Its free-form canvas class, which is the shape a node canvas actually is,
publishes 11 properties and 21 methods of which exactly **two** name selection
(`selectionChanged`, `clearSelection`) and **zero** name a current, a primary, a
lead or an anchor; selecting three of its items leaves its focus item null, so
nothing answers which of the three. Its item-model selection class *does* carry
a current index — and speaks only model indices, so it cannot address an item on
a canvas at all. And across all three of its selection-bearing classes exactly
one member names a *reason*, which is about focus rather than selection: its
canvas signal carries **no arguments**, so a listener re-derives the delta from
a fresh read.

# What it asserts

* **A** — the canon gesture: pressing a host's tab selects every card that host
  holds and nothing else, and the first of them leads. Driven through the real
  pointer wire at rectangles read out of the painted scene, on both hosts.
* **B** — one gesture, two halves: the same press-and-drag both selects the
  group and carries it, every member moves by the same delta, and no card
  outside the host moves at all.
* **C** — the wire and the paint are one fact: `selected` names the leader,
  `selected_ids` names the members in arrival order, and the cards the canvas
  outlines are exactly those members.
* **D** — ★ PIXELS. Three states have to be visibly different or the set is
  indistinguishable from one card selected and five not, so the outlines are
  scanned out of a real screenshot taken *after* the gesture.
* **E** — the panel says how many and which, in words, and announces the same
  sentence it paints.
* **F** — the semantic tree: `aria-selected` on exactly the members,
  `aria-current` on exactly the leader, `aria-multiselectable` on the canvas.
* **G** — §2 #2: an agent can do what the person just did. `move_frame` selects
  and moves identically to the gesture, refuses a host that does not exist, and
  its declared vocabulary is the hosts the canvas actually draws.
* **H** — narrowing and the frame's own place in the set: pressing one card
  collapses the selection to it, and a host is never a member of its own group.
* **I** — the canon's volatility rule: a save carries the LEADER, and a restore
  collapses the selection to it rather than handing back a group nobody picked.
* **J** — ★ membership moving and the leader moving are two events, and the
  screen answers them differently: growing the set around the card the panel is
  showing leaves the open text field alone, and moving the leader shuts it.

Run from the workspace root:
    cargo build -p hello-node-lab --release
    python3 tools/demos/r1706_a_selection_is_a_set_with_a_leader.py
"""

from __future__ import annotations

import json
import sys
import tempfile
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    Png,
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    isolated_storage_dir,
    png_pixel,
    read_png_rgba8,
    run_demo,
)

EXT = "/external"

#: The opening graph, as the screen's own specification lays it out: which host
#: holds which cards, in the order the document holds them. Written here rather
#: than read back off the wire on purpose — a test that asks the screen what it
#: contains and then checks the screen against that answer checks nothing.
HOSTS: dict[str, list[str]] = {
    "host-b": ["T-01", "Q-01"],
    "host-a": ["P-01", "S-01", "R-01", "P-02", "T-02", "P-03"],
}


def centre(rect: tuple[int, int, int, int]) -> tuple[int, int]:
    x, y, w, h = rect
    return (x + w // 2, y + h // 2)


def selection_of(tf: RpcSubprocess) -> tuple[str, list[str]]:
    """What the wire says is selected: the leader, and the members in order."""
    leader = tf.query(f"{EXT}/selected")
    members = tf.query(f"{EXT}/selected_ids")
    return leader, [m for m in members.split(",") if m]


def card_positions(tf: RpcSubprocess) -> dict[str, tuple[int, int]]:
    """Where each card sits **in the graph**, from the screen's `layout` read.

    Canvas coordinates and not window ones, and the difference decides an
    assertion below: the window rectangle is the canvas position through the
    zoom (84% on the opening screen) and then rounded, so six cards carried by
    one identical delta land on window deltas that differ by a pixel. Measured —
    the first draft of this compared window positions and reported five cards
    moved by `(49, 36)` and one by `(49, 37)`. That is the rounding, not the
    move, and asserting on it would have been asserting on the zoom.
    """
    return {
        name: (int(at[0]), int(at[1]))
        for name, at in json.loads(tf.query(f"{EXT}/layout")).items()
    }


def card_edges(tf: RpcSubprocess) -> dict[str, tuple[int, int, int, int]]:
    """Each card's declared border colour, out of the painted scene."""
    snap = tf.snapshot(source="paint")
    out: dict[str, tuple[int, int, int, int]] = {}
    for name in [n for names in HOSTS.values() for n in names]:
        node = find_by_tag(snap, f"lab.node.{name}")
        assert node is not None, f"the canvas draws no card for {name}"
        border = (node.get("style") or {}).get("border")
        assert border is not None, f"{name}'s card has no border at all"
        rgba = border["color"]
        out[name] = (rgba["r"], rgba["g"], rgba["b"], rgba["a"])
    return out


def access_nodes(tf: RpcSubprocess) -> dict[str, dict]:
    answer = tf.request("scene/access", {}).result
    return {n["tag"]: n for n in answer.get("nodes", []) if "tag" in n}


def press_host_tab(tf: RpcSubprocess, host: str) -> None:
    shot = abs_rects_of(tf.snapshot(source="paint"))
    tf.click(centre(shot[f"lab.frame.{host}.name"]))
    tf.tick(16)


# ── A — the canon gesture ────────────────────────────────────────────────────


def a_pressing_a_host_selects_everything_it_holds(tf: RpcSubprocess) -> None:
    for host, members in HOSTS.items():
        press_host_tab(tf, host)
        leader, picked = selection_of(tf)
        assert_eq(picked, members, f"pressing {host} picks exactly the cards it holds")
        assert_eq(leader, members[0], f"and the first of them leads on {host}")
        outside = [
            name
            for other, names in HOSTS.items()
            if other != host
            for name in names
        ]
        for name in outside:
            assert name not in picked, f"{name} is not in {host} and must not be picked"


# ── B — one gesture, two halves ──────────────────────────────────────────────


def b_the_same_gesture_selects_and_carries(tf: RpcSubprocess) -> None:
    # Start from somewhere else entirely, so "it was already selected" cannot
    # make this pass.
    shot = abs_rects_of(tf.snapshot(source="paint"))
    tf.click(centre(shot["lab.node.T-01"]))
    tf.tick(16)
    assert_eq(selection_of(tf)[1], ["T-01"], "the run starts on one card of the OTHER host")

    before = card_positions(tf)
    tab = abs_rects_of(tf.snapshot(source="paint"))["lab.frame.host-a.name"]
    start = centre(tab)
    tf.drag(from_at=start, to_at=(start[0] + 48, start[1] + 36))
    tf.tick(16)

    leader, picked = selection_of(tf)
    assert_eq(picked, HOSTS["host-a"], "the drag SELECTED the host's cards on the way in")
    assert_eq(leader, HOSTS["host-a"][0], "and the first of them leads")

    after = card_positions(tf)
    deltas = {
        name: (after[name][0] - before[name][0], after[name][1] - before[name][1])
        for name in before
    }
    moved = {name: d for name, d in deltas.items() if d != (0, 0)}
    assert_eq(
        sorted(moved),
        sorted(HOSTS["host-a"]),
        "exactly the host's own cards moved",
    )
    one_delta = set(moved.values())
    assert_eq(len(one_delta), 1, f"and every one of them by the SAME delta: {moved}")
    for name in HOSTS["host-b"]:
        assert_eq(deltas[name], (0, 0), f"{name} belongs to the other host and stayed put")


# ── C — the wire and the paint are one fact ──────────────────────────────────


def c_the_answer_and_the_drawing_agree(tf: RpcSubprocess) -> None:
    press_host_tab(tf, "host-a")
    leader, picked = selection_of(tf)
    edges = card_edges(tf)

    lead_edge = edges[leader]
    member_edges = {edges[name] for name in picked if name != leader}
    other_edges = {edges[name] for name in edges if name not in picked}

    assert_eq(len(member_edges), 1, "every non-leading member is outlined the same way")
    assert_eq(len(other_edges), 1, "and so is every card that is not selected")
    member_edge = member_edges.pop()
    other_edge = other_edges.pop()
    assert lead_edge != member_edge, (
        f"the leader has to be told from the rest of the set: both {lead_edge}"
    )
    assert member_edge != other_edge, (
        f"and a member from a card nobody picked: both {member_edge}"
    )


# ── D — pixels ───────────────────────────────────────────────────────────────


def probe_of(rect: tuple[int, int, int, int]) -> tuple[int, int]:
    """A point ON a card's left border, a few pixels below its top corner.

    Not the corner itself — the radius rounds it away — and not the middle of
    the edge, which on a short card can meet a row of text bleeding out to the
    boundary. The left edge is the one nothing is ever drawn over.
    """
    x, y, _w, h = rect
    return (x, y + max(h // 2, 8))


def sample_edge(img: Png, at: tuple[int, int]) -> tuple[int, int, int, int]:
    """The most saturated pixel in a 3-wide band across the border line.

    A 1px border rasterised through the anti-aliased pipeline spreads its ink
    over two or three columns, so a single-column probe is a coin flip on which
    side of the coverage it lands. Taking the strongest of the band is what
    makes the reading stable without widening what is being read.
    """
    x, y = at
    band = [png_pixel(img, x + dx, y) for dx in (-1, 0, 1)]
    return max(band, key=lambda p: p[0] + p[1] + p[2])


def d_the_three_states_are_visibly_different(tf: RpcSubprocess, out_dir: Path) -> None:
    press_host_tab(tf, "host-a")
    leader, picked = selection_of(tf)
    rects = abs_rects_of(tf.snapshot(source="paint"))

    png = out_dir / "r1706-group-selected.png"
    tf.request("scene/screenshot", {"path": "", "out_path": str(png)})
    assert png.exists(), "the screenshot was not written"
    img = read_png_rgba8(png)

    lead_px = sample_edge(img, probe_of(rects[f"lab.node.{leader}"]))
    member = next(name for name in picked if name != leader)
    member_px = sample_edge(img, probe_of(rects[f"lab.node.{member}"]))
    stranger = HOSTS["host-b"][0]
    stranger_px = sample_edge(img, probe_of(rects[f"lab.node.{stranger}"]))

    def apart(a: tuple[int, ...], b: tuple[int, ...]) -> int:
        return max(abs(int(p) - int(q)) for p, q in zip(a, b))

    assert apart(lead_px, member_px) >= 12, (
        f"★ the leader {leader} {lead_px} and the member {member} {member_px} "
        "are the same ink on the glass — a group selection looks like one card"
    )
    assert apart(member_px, stranger_px) >= 12, (
        f"★ the member {member} {member_px} and the unpicked {stranger} "
        f"{stranger_px} are the same ink on the glass — the set is invisible"
    )
    print(
        f"[pixels] leader {lead_px}  member {member_px}  unpicked {stranger_px}"
    )


# ── E — the panel says how many, in words ────────────────────────────────────


def e_the_panel_says_how_many_and_which(tf: RpcSubprocess) -> None:
    press_host_tab(tf, "host-a")
    leader, picked = selection_of(tf)
    nodes = access_nodes(tf)
    chip = nodes.get("lab.inspector.selcount")
    assert chip is not None, "the inspector does not announce the selection at all"
    said = chip["name"]
    assert str(len(picked)) in said, f"the count is not in {said!r}"
    assert leader in said, f"the card being shown is not named in {said!r}"

    # And what it ANNOUNCES is what it PAINTS — one derivation, not two.
    snap = tf.snapshot(source="paint")
    painted = find_by_tag(snap, "lab.inspector.selcount.text")
    assert painted is not None, "the chip has no painted caption"
    assert_eq(painted["content"], said, "the chip says one thing, once")

    # With one card picked it does not claim a group.
    shot = abs_rects_of(snap)
    tf.click(centre(shot["lab.node.T-01"]))
    tf.tick(16)
    single = access_nodes(tf)["lab.inspector.selcount"]["name"]
    assert single.startswith("1 selected"), f"one card picked, and it says {single!r}"


# ── F — the semantic tree ────────────────────────────────────────────────────


def f_the_tree_carries_membership_and_the_lead(tf: RpcSubprocess) -> None:
    press_host_tab(tf, "host-a")
    leader, picked = selection_of(tf)
    nodes = access_nodes(tf)

    canvas = nodes["lab.canvas"]
    assert canvas.get("multiselectable") is True, (
        "a canvas whose frame gesture picks six at once has to say it is "
        "multi-selectable, or a per-card 'not selected' is noise"
    )

    said_selected = sorted(
        tag.removeprefix("lab.node.")
        for tag, node in nodes.items()
        if tag.startswith("lab.node.")
        and tag.count(".") == 2
        and node.get("selected") is True
    )
    assert_eq(said_selected, sorted(picked), "aria-selected names exactly the members")

    said_current = [
        tag.removeprefix("lab.node.")
        for tag, node in nodes.items()
        if tag.startswith("lab.node.")
        and tag.count(".") == 2
        and node.get("current") is not None
    ]
    assert_eq(said_current, [leader], "aria-current names exactly the leader")


# ── G — the agent can do what the person did ─────────────────────────────────


def g_the_agent_path_is_the_same_act(tf: RpcSubprocess) -> None:
    # The gesture first, from a known start, and record what it did.
    tf.invoke(f"{EXT}/select", "T-01")
    start_at = card_positions(tf)
    shot = abs_rects_of(tf.snapshot(source="paint"))
    start = centre(shot["lab.frame.host-a.name"])
    tf.drag(from_at=start, to_at=(start[0] + 32, start[1] + 24))
    tf.tick(16)
    by_gesture = selection_of(tf)
    after_gesture = card_positions(tf)
    moved = {
        name: (
            after_gesture[name][0] - start_at[name][0],
            after_gesture[name][1] - start_at[name][1],
        )
        for name in HOSTS["host-a"]
    }
    deltas = set(moved.values())
    assert_eq(len(deltas), 1, f"the gesture carried the host by one delta: {moved}")
    (dx, dy) = deltas.pop()
    assert (dx, dy) != (0, 0), "the gesture has to have moved something"

    # ★ Now the verb, asked to put it back exactly. The two channels are
    # compared by COMPOSING them to identity rather than by comparing their
    # numbers, because their numbers are in different units and always will
    # be: a drag is window pixels through the zoom, a verb is canvas units.
    # Measured — at the opening zoom of 84% a 32-pixel drag is a 38-unit move,
    # and a draft that asserted `32 == 38` would have been asserting on the
    # zoom rather than on the two channels agreeing.
    tf.invoke(f"{EXT}/select", "T-01")
    said = tf.invoke(f"{EXT}/move_frame", f"host-a,{-dx},{-dy}")
    by_verb = selection_of(tf)
    back = card_positions(tf)
    for name in HOSTS["host-a"]:
        assert_eq(back[name], start_at[name], f"{name} is back exactly where it started")
    assert_eq(by_verb, by_gesture, "and the verb leaves the same selection the gesture does")
    assert "host-a" in said, f"and says what it did: {said!r}"

    # A host that does not exist is refused, by name, rather than doing nothing.
    try:
        tf.invoke(f"{EXT}/move_frame", "nowhere,1,1")
    except Exception as exc:  # noqa: BLE001
        assert "nowhere" in str(exc), f"the refusal has to name what it refused: {exc}"
    else:
        raise AssertionError("a host that does not exist was accepted")

    # ★ The declared vocabulary is the hosts the canvas DRAWS. A schema that
    # offers a word the screen has no frame for is a promise to an agent that
    # cannot be kept — and one that omits a host the canvas draws leaves a
    # gesture with no wire spelling, which is this round's whole subject.
    assert_eq(
        sorted(declared_hosts(tf)),
        sorted(HOSTS),
        "the schema offers exactly the hosts the canvas draws",
    )


def declared_hosts(tf: RpcSubprocess) -> list[str]:
    """The `host` vocabulary `move_frame` publishes on its `$schema` path."""
    schema = tf.query(f"{EXT}/$schema")
    if isinstance(schema, str):
        schema = json.loads(schema)
    for field in schema:
        if field.get("path") != "move_frame":
            continue
        for arg in field.get("args", []):
            if arg.get("name") == "host":
                return list(arg.get("domain", {}).get("values") or [])
    raise AssertionError(f"move_frame declares no host vocabulary: {schema}")


# ── H — narrowing, and what a host is not ────────────────────────────────────


def h_narrowing_and_the_hosts_own_place(tf: RpcSubprocess) -> None:
    press_host_tab(tf, "host-a")
    assert len(selection_of(tf)[1]) == len(HOSTS["host-a"]), "the group is picked"

    shot = abs_rects_of(tf.snapshot(source="paint"))
    tf.click(centre(shot["lab.node.S-01"]))
    tf.tick(16)
    assert_eq(selection_of(tf), ("S-01", ["S-01"]), "pressing one card narrows to it")

    # ★ A host is never a member of its own group. Its rectangle is DERIVED
    # from the cards it holds, so a host in the set would be a member whose
    # position is a function of the other members — and the group drag would
    # move it twice.
    press_host_tab(tf, "host-a")
    _leader, picked = selection_of(tf)
    for host in HOSTS:
        assert host not in picked, f"the host {host} put itself in its own group"


# ── I — what a save carries ──────────────────────────────────────────────────


def i_a_save_carries_the_leader(tf: RpcSubprocess) -> None:
    press_host_tab(tf, "host-a")
    leader, picked = selection_of(tf)
    assert len(picked) > 1, "the fixture needs a group on screen"

    tf.invoke(f"{EXT}/save_graph", "")
    # Move somewhere else so a restore has something to undo.
    tf.invoke(f"{EXT}/select", "T-01")
    assert_eq(selection_of(tf), ("T-01", ["T-01"]), "the selection moved before the load")

    tf.invoke(f"{EXT}/open_graph", "")
    restored_leader, restored = selection_of(tf)
    assert_eq(restored_leader, leader, "the save carried the card the panel was showing")
    assert_eq(
        restored,
        [leader],
        "and a restore collapses to it — a group is something a person is "
        "holding now, not something a file hands back",
    )


# ── J — growing a selection is not the same event as changing its leader ─────


def j_growing_the_set_does_not_disturb_the_open_field(tf: RpcSubprocess) -> None:
    """★ The two halves of a change are two facts, and this is where it shows.

    The inspector's one text field is opened OVER the inspected card's row, so a
    selection that moves the card the panel is showing has to shut it. Growing
    the selection while the SAME card keeps leading does not: the panel still
    shows what the box is standing on, and taking a half-typed value away there
    would be the screen doing something a person has no way to explain.

    The fixture is exact rather than incidental: pressing the host that holds
    the already-selected leading card grows the set from one to six **and
    leaves the leader where it is**, because the leader of a group is its first
    member and that card is it.
    """
    def editing(tf: RpcSubprocess) -> dict:
        return json.loads(tf.query(f"{EXT}/editing"))

    tf.invoke(f"{EXT}/select", "P-01")
    tf.invoke(f"{EXT}/edit", "name")
    tf.invoke(f"{EXT}/type", "half-typed")
    open_now = editing(tf)
    assert_eq(open_now["target"], "name", "the field is open on the name")
    assert_eq(open_now["text"], "half-typed", "with something half-typed in it")

    press_host_tab(tf, "host-a")
    leader, picked = selection_of(tf)
    assert_eq(leader, "P-01", "the fixture needs the leader to STAY on P-01")
    assert len(picked) > 1, "and the set to have grown around it"
    still = editing(tf)
    assert_eq(
        (still["target"], still["text"]),
        ("name", "half-typed"),
        "★ the set grew and the leader did not move, so the open field stays open",
    )

    # And the other direction still shuts it: moving the leader does.
    shot = abs_rects_of(tf.snapshot(source="paint"))
    tf.click(centre(shot["lab.node.S-01"]))
    tf.tick(16)
    assert_eq(
        editing(tf)["target"],
        None,
        "picking a different card moves the leader, so the field shuts",
    )


def body() -> None:
    # ★ Isolated storage: assertion I saves a graph, and a demo that wrote into
    # the developer's real store would leave this screen opening on a group
    # somebody's next run did not pick.
    with tempfile.TemporaryDirectory(prefix="r1706-") as tmp, isolated_storage_dir("r1706-store-"):
        out_dir = Path(tmp)
        with RpcSubprocess("hello-node-lab") as tf:
            a_pressing_a_host_selects_everything_it_holds(tf)
            b_the_same_gesture_selects_and_carries(tf)
            c_the_answer_and_the_drawing_agree(tf)
            d_the_three_states_are_visibly_different(tf, out_dir)
            e_the_panel_says_how_many_and_which(tf)
            f_the_tree_carries_membership_and_the_lead(tf)
            g_the_agent_path_is_the_same_act(tf)
            h_narrowing_and_the_hosts_own_place(tf)
            i_a_save_carries_the_leader(tf)
            j_growing_the_set_does_not_disturb_the_open_field(tf)


run_demo("r1706 a selection is a set with a leader", body)
