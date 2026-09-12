#!/usr/bin/env python3
"""R1716 §5.21 §5.12 §2 #7 — **a settings row says where its value came from**,
on the analysis tool's node-graph screen.

# What this exists for

The inspector could show one kind of row: a value somebody typed. The behaviour
canon shows three more — the session mode a node's ROLE implies, the host a card
runs on, and the addresses its drawn LINKS dial — and it draws them without a
control, without a delete affordance, and with the source written on the badge.
This tree had no way to say any of that, so those rows were absent, and one of
them was worse than absent: `connect.endpoints` was a value typed beside the
code while the canvas drew the real connections somewhere else.

Measured on this screen before the change, through this wire:

  * `R-01` showed `connect.endpoints = tcp/10.0.0.21:7449` — one address, at a
    host nothing in the graph listens on — while the canvas drew **three** links
    out of that card, to endpoints `:7449`, `:7451` and one unlabelled. The
    exported configuration shipped the typed value: a node dialling somewhere it
    was not drawn to reach and missing one it was.
  * The exported plan put **all eight nodes** on one host called `unplaced`,
    while the screen's own `frames` read said six were on `host-a` and two on
    `host-b`. Two derivations of one fact, and the one nobody read was wrong.

# The floor this is built to beat, measured rather than read

A probe was built against the mature toolkit 6.11.1 and run offscreen. It can
derive a value — one recomputed 20 -> 42 when its source moved 10 -> 21 — so the
capability is parity. What it cannot do, all measured:

  * asking whether a value is derived answers a **bool**; nothing names a source;
  * writing into a derived value **drops the derivation silently** (one ordinary
    value-changed notification, and afterwards the dropped derivation is
    unreachable);
  * a cell marked not-editable is a **view's convention**: `setData` on one
    returned true and changed the value;
  * a locked cell answers 3 of 256 standard roles and none of them is a reason;
    driving its editor logs a line and returns `void`;
  * six capabilities this round ships are compile errors there — naming a
    value's source, a refusal that carries a reason, asking a cell why it has no
    editor, taking a derivation over and being told what was dropped, a per-row
    removability predicate, and a rollup of the rows that are not part of the
    saved document.

# What it asserts

* **A** — the wire publishes the axis. Every row carries `source` and `aside`,
  and the set of derived rows is exactly the one the specification declares.
* **B** — ★★★★★ the headline: a value nobody wrote refuses the write, the
  refusal NAMES the source, and the value does not move. This is the assertion
  the floor fails.
* **C** — ★★★★★ take-over. The act is named, it says what it displaced, the row
  becomes writable in the same breath, and the screen's seat changes from
  "take over" to "remove".
* **D** — a derived row is not a text box: pressing its control opens nothing
  and says why, and a reader is told `read_only` plus the source.
* **E** — ★★★★★ derived from the WIRE: drawing a link moves the connect row and
  the exported configuration with it; deleting one takes it back out.
* **F** — the document. A derived row is configuration and ships; the placement
  row is not and does not — and the plan's hosts are the frames the canvas
  draws, not one bucket.
* **G** — the pointer. The seat's rectangle answers `author:<key>` through the
  router, and pressing it takes the row over.

>= 30 assertions.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    access_node_by_tag,
    assert_action_refused,
    assert_eq,
    form_part_prefixes,
    form_part_tag,
    form_row,
    run_demo,
    screen_spec,
)


def dialled_key(tf) -> str:
    """The row this walk is about: the one worked out from the WIRES.

    ⚠ `_key`, because this file already binds `dialled` to the LIST of
    addresses that row holds. Two meanings under one name is a defect this
    repository counts by the dozen, and Python's scoping turns this particular
    one into a function-wide `UnboundLocalError` rather than a shadowed read.

    ★ R2156 — asked for rather than spelled. Which configuration path carries
    the dialled addresses is the screen's own answer (it names that row
    internally and publishes the role), and a walk spelling the path was making
    a second claim about it that nothing checked.
    """
    return form_row(tf, "dialled")["key"]

EXAMPLE = "hello-node-lab"
EXT = "/external"
VIEWPORT = (1440, 900)

#: ★ R2079 — the card this screen opens with selected. Named because section E
#: now has to MOVE the selection (the router has no wire-derived row since
#: R2074) and every section after it reads the selected card, so E has to put
#: it back to what the screen chose rather than to a card of its own choosing.
OPENS_SELECTED = "R-01"

CHECKS: list[str] = []


def ok(what: str, condition: bool) -> None:
    assert condition, f"FAILED: {what}"
    CHECKS.append(what)


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def rows(tf) -> list[dict]:
    return json.loads(tf.query(f"{EXT}/form"))


def row(tf, key: str) -> dict:
    found = [r for r in rows(tf) if r["key"] == key]
    assert found, f"no row {key!r} on the selected card"
    return found[0]


def rects(tf) -> dict:
    return abs_rects_of(tf.snapshot(source="paint", viewport=VIEWPORT))


def document(tf) -> dict:
    return json.loads(tf.query(f"{EXT}/document"))


# ── A: the axis is on the wire ──────────────────────────────────────────────


def a_every_row_says_where_its_value_came_from(tf) -> None:
    banner("A — the wire publishes where each value came from")
    declared = {f["key"]: f for f in screen_spec(tf)["fields"]}
    live = rows(tf)
    assert_eq(
        [r["key"] for r in live],
        list(declared),
        "A: the inspector shows the specification's rows, in its order",
    )
    for r in live:
        want = declared[r["key"]]
        assert_eq(r["source"], want["source"], f"A: {r['key']} names its source")
        assert_eq(r["aside"], want["aside"], f"A: {r['key']} says where it goes")
        assert_eq(r["value"], want["value"], f"A: {r['key']} holds what it should")
    # ⚠⚠ ★★★★★ R2079 — **derived from the specification, not written out.**
    #
    # These read `== {"mode", "host", "connect.endpoints"}` and
    # `== {"role", "frame", "wire"}`, and R2074 made both false: correcting
    # `spec::LINKS` to name the dialler the behaviour canon names leaves the
    # router dialling nothing, so the row worked out FROM the wires is not
    # painted at all — which that round recorded as taking the screen TOWARD
    # the reference.
    #
    # ⇒ ★ **the walk was already deriving the row LIST from the screen** (the
    # comparison against `declared`, twelve lines up) and then hand-wrote the
    # subset with a source. One half could not rot and the other could. Both
    # come off the specification now, so a row that stops being derived moves
    # this check with it — which is R2049's rule for addresses, applied to a
    # population.
    worked_out = {r["key"] for r in live if r["source"]}
    want_worked = {k for k, f in declared.items() if f["source"]}
    ok(
        f"A: the rows the specification says are worked out are the rows that "
        f"are ({sorted(worked_out)})",
        worked_out == want_worked,
    )
    ok(
        f"A: ★ and they are worked out from MORE THAN ONE kind of thing, so "
        f"'derived' is not one mechanism wearing three names — "
        f"{sorted({f['source'] for f in declared.values() if f['source']})}",
        len({r["source"] for r in live if r["source"]}) > 1,
    )
    ok(
        "A: exactly one row is not configuration at all, and it says what it is "
        "instead (placement)",
        [r["key"] for r in live if r["aside"]] == ["host"],
    )


# ── B: the refusal is the value's own ───────────────────────────────────────


def b_a_derived_row_refuses_the_write(tf) -> None:
    banner("B — a value nobody wrote refuses the write, and says where it comes from")
    before = row(tf, "mode")["value"]
    said = assert_action_refused(
        lambda: tf.invoke(f"{EXT}/set_field", "mode=client"),
        saying="role",
    )
    ok(f"B: the refusal names the source it comes from ({said!r})", "role" in said)
    ok("B: and it names the row", "mode" in said)
    assert_eq(row(tf, "mode")["value"], before, "B: ★ the value did not move")
    assert_action_refused(
        lambda: tf.invoke(f"{EXT}/remove_field", "mode"),
        saying="role",
    )
    ok(
        "B: nor may it be taken away — the derivation is still true, so the row "
        "would be back one render later",
        row(tf, "mode")["key"] == "mode",
    )
    # The placement row is on the other axis and refuses for the same reason.
    assert_action_refused(
        lambda: tf.invoke(f"{EXT}/set_field", "host=10.0.0.9"),
        saying="frame",
    )
    ok("B: the placement row refuses too, naming the frame", True)


# ── C: taking a row over ────────────────────────────────────────────────────


def c_taking_a_row_over_is_announced(tf) -> None:
    banner("C — take-over: named, announced, and reversible")
    was = row(tf, "mode")
    tf.invoke(f"{EXT}/author_field", "mode")
    said = tf.query(f"{EXT}/toast")
    ok(f"C: the screen says what was displaced ({said!r})", "role" in said and "mode" in said)
    now = row(tf, "mode")
    assert_eq(now["source"], None, "C: the row is nobody's derivation any more")
    assert_eq(
        now["value"],
        was["value"],
        "C: ★ and it starts from what it WAS — taking a value over never starts "
        "from empty",
    )
    tf.invoke(f"{EXT}/set_field", "mode=client")
    assert_eq(row(tf, "mode")["value"], "client", "C: now it takes a value")
    assert_eq(
        row(tf, "mode")["edited"],
        True,
        "C: and it reads as edited, which a derived row never can",
    )
    seats = rects(tf)
    # ★ R2054 — the screen says where each part of a row is addressed. These
    # three readings are about a mark being ABSENT, which is the case a spelled
    # address is worst at: a wrong letter makes the absence true for a reason
    # that has nothing to do with the screen.
    part = form_part_prefixes(tf, ext=EXT)
    ok(
        "C: the seat on the row changed act — it offers to remove now, not to "
        "take over",
        f"{part['remove']}mode" in seats and f"{part['author']}mode" not in seats,
    )
    ok(
        "C: and the source badge is gone with it",
        f"{part['source']}mode" not in seats,
    )
    assert_action_refused(
        lambda: tf.invoke(f"{EXT}/author_field", "mode"),
        saying="already yours",
    )
    ok("C: a second take-over has nothing to take", True)
    # Put it back the way a person would: the row is theirs, so it can go.
    tf.invoke(f"{EXT}/remove_field", "mode")
    back = row(tf, "mode")
    assert_eq(
        back["source"],
        "role",
        "C: ★★ and removing it hands the row back to the role — the derivation "
        "was never gone, only overridden",
    )
    assert_eq(back["value"], "router", "C: holding what the role implies again")


# ── D: a derived row is not a text box ──────────────────────────────────────


def d_a_derived_row_is_a_read_out(tf) -> None:
    banner("D — pressing a derived row opens nothing and says why")
    # ★ R2050 — the address the screen publishes for that row's control.
    mode_control = next(
        row["control"] for row in json.loads(tf.query(f"{EXT}/form")) if row["key"] == "mode"
    )
    where = rects(tf)[mode_control]
    centre = (where[0] + where[2] // 2, where[1] + where[3] // 2)
    tf.click(centre)
    said = tf.query(f"{EXT}/toast")
    ok(
        f"D: the press is answered with a reason, not with a box ({said!r})",
        "role" in said and "mode" in said,
    )
    assert_eq(
        json.loads(tf.query(f"{EXT}/editing"))["target"],
        None,
        "D: ★ and no field opened — a box that cannot commit is worse than none",
    )
    access = tf.request("scene/access").result
    node = access_node_by_tag(access, mode_control)
    assert node is not None, "the row's control is in the tree"
    assert_eq(
        node["state"].get("read_only"),
        True,
        "D: ★★ read-only and not disabled — the value is still worth hearing",
    )
    assert_eq(
        node["state"].get("disabled", False),
        False,
        "D: it is fully reachable",
    )
    assert_eq(
        node["role"],
        "textbox",
        "D: ★ it announces what it IS — a read-out, not the radio group its "
        "shape would paint if anybody could operate it",
    )
    described = json.dumps(access, ensure_ascii=False)
    ok(
        "D: and a reader who cannot see the badge is told the same sentence",
        "worked out from the role" in described,
    )
    ok(
        "D: the placement row says it is not configuration",
        "placement, not configuration" in described,
    )


# ── E: derived from the wires the canvas draws ──────────────────────────────


def e_drawing_a_link_moves_the_row(tf) -> None:
    banner("E — the connect row is worked out from the links this canvas draws")
    # ⚠⚠ ★★★★★ R2079 — **the subject is a card that DIALS, chosen by asking
    # rather than named.**
    #
    # This section ran on whichever card the walk had selected, which was the
    # router, and R2074 made that impossible: correcting `spec::LINKS` to name
    # the dialler the behaviour canon names leaves the router dialling NOTHING,
    # so the row worked out from its wires is not painted — the row this whole
    # section is about. That round recorded the disappearance as taking the
    # screen toward the reference, and it is right; what it could not see is
    # that the sweep's walks transcribe the fixture.
    #
    # ⇒ ★ asked of the model: the subject is a card with an outbound wire, so
    # the section keeps its meaning on any graph where one exists, and says so
    # if none does.
    dialling = sorted(
        {link["from"] for link in json.loads(tf.query(f"{EXT}/links"))}
    )
    assert dialling, "E: no card on this canvas dials anything, so there is no derived row to be about"
    subject = dialling[0]
    tf.invoke(f"{EXT}/select", subject)
    opening = row(tf, dialled_key(tf))
    assert_eq(opening["source"], "wire", "E: it says so")
    dialled = [part.strip() for part in opening["value"].split(",")]
    # ★ R2079 — ONE PER DRAWN LINK, counted off the model rather than the two
    # addresses this used to name. The claim was never which addresses they are;
    # it is that the row is the wires, resolved.
    out_of_subject = [
        link
        for link in json.loads(tf.query(f"{EXT}/links"))
        if link["from"] == subject
    ]
    ok(
        f"E: {subject} holds one address per drawn link, resolved to the host "
        f"the target runs on ({dialled} for {len(out_of_subject)} link(s))",
        len(dialled) == len(out_of_subject) and all(dialled),
    )
    assert_eq(
        document(tf)["connect"]["endpoints"],
        dialled,
        "E: ★ and the exported configuration says the same thing — before this "
        "round it shipped an address nothing in the graph listens on",
    )
    # Draw one more link out of the selected card, through the wire. It needs
    # somewhere to land, so the card it dials is given a second address first —
    # which is itself the point: a row on one card is worked out from a value on
    # another, and neither of them is where this row's text lives.
    # ★ R2079 — the card to grow a wire to is DERIVED, and it took three
    # refusals from the model to get the predicate right. It must
    #
    #   * listen — a card that is the TARGET of some drawn link demonstrably
    #     does, which is the only listening test available from out here.
    #     Filtering without it picked `T-01` and earned
    #     `Action refused — T-01 does not listen, so nothing can dial it`;
    #   * not already be dialled by the subject, or the growth step grows
    #     nothing;
    #   * not be the subject.
    #
    # ⇒ ★ each clause is a refusal the model handed back. A hand-picked pair is
    # what earned `that link would close a cycle` in this round's first attempt
    # at the sibling walk, and this is the same lesson twice: the graph's rules
    # are the model's to state, and a fixture guessed against them fails for a
    # reason that has nothing to do with what the walk asks.
    # ⚠ `/nodes` answers a comma-separated STRING, not JSON — the first draft of
    # this line wrapped it in `json.loads` and died `Expecting value: line 1
    # column 1`. The slots on this surface are not uniform, and R1724's own
    # `lab_spec` carries the same warning for `spec`.
    all_links = json.loads(tf.query(f"{EXT}/links"))
    listens = {link["to"] for link in all_links}
    grow_to = next(
        name
        for name in sorted(listens)
        if name != subject and name not in {link["to"] for link in out_of_subject}
    )
    tf.invoke(f"{EXT}/select", grow_to)
    tf.invoke(f"{EXT}/set_field", "listen.endpoints=tcp/0.0.0.0:7449, tcp/0.0.0.0:7450")
    tf.invoke(f"{EXT}/select", subject)
    tf.invoke(f"{EXT}/connect", f"{subject},{grow_to}")
    grown = [part.strip() for part in row(tf, dialled_key(tf))["value"].split(",")]
    ok(
        f"E: ★★★★★ drawing a link MOVES the row ({grown})",
        len(grown) == len(dialled) + 1 and set(dialled) <= set(grown),
    )
    assert_eq(
        document(tf)["connect"]["endpoints"],
        grown,
        "E: and the configuration follows in the same act",
    )
    drawn = [
        link
        for link in json.loads(tf.query(f"{EXT}/links"))
        if link["from"] == subject
        and link["to"] == grow_to
        and link["endpoint"].endswith(":7450")
    ]
    assert drawn, "the link this section drew is in the model"
    tf.invoke(f"{EXT}/delete_link", str(drawn[0]["id"]))
    assert_eq(
        [part.strip() for part in row(tf, dialled_key(tf))["value"].split(",")],
        dialled,
        "E: ★ and undrawing it takes the address back out",
    )
    # ⚠ ★ R2079 — **the selection goes back.** This section now moves it (the
    # router has no wire-derived row to be about since R2074), and the sections
    # after it read the SELECTED card: leaving it moved made F report
    # `expected 'router', got 'peer'` — a true statement about the wrong card.
    # A section that changes shared state puts it back, or the walk's order
    # becomes part of its meaning.
    tf.invoke(f"{EXT}/select", OPENS_SELECTED)


# ── F: what ships and what does not ─────────────────────────────────────────


def f_the_document_carries_what_belongs_in_it(tf) -> None:
    banner("F — a derived row is configuration; a placement row is not")
    doc = document(tf)
    assert_eq(
        doc["mode"],
        "router",
        "F: ★ a value the screen worked out is still configuration and ships",
    )
    ok(
        "F: ★★ and the row that is about WHERE the node runs does not — it would "
        "be a key the target does not have",
        "host" not in doc,
    )
    tf.invoke(f"{EXT}/export", "")
    produced = json.loads(tf.query(f"{EXT}/produced"))
    hosts = produced["config"]["hosts"]
    ok(
        f"F: ★★★★★ the plan's hosts are the frames the canvas draws ({sorted(hosts)}) "
        f"— measured before this round, all eight nodes were on one bucket "
        f"called 'unplaced'",
        set(hosts) == {"host-a", "host-b"},
    )
    ok(
        "F: and every card is on one of them",
        sum(len(v) for v in hosts.values()) == 8,
    )
    placed = {entry["node"]: entry["host"] for entry in produced["config"]["order"]}
    assert_eq(placed["R-01"], "host-a", "F: the selected card runs where it is drawn")
    assert_eq(placed["Q-01"], "host-b", "F: and a card in the other frame too")
    # ★★★★★ And the plan's per-node configuration is the SAME configuration the
    # read publishes. This is the assertion that caught the round's own defect:
    # the plan was built from the stored rows, so it shipped neither the mode a
    # role implies nor the addresses the canvas draws, while the `document` read
    # beside it carried both. Two answers to one question, and the exported one
    # was the wrong one.
    assert_eq(
        produced["config"]["nodes"]["R-01"],
        doc,
        "F: ★★★★★ what the plan ships for a card IS what the screen says its "
        "configuration is",
    )
    # ★ R1850 — `admin` where this said `control`. R1842 moved the permissions
    # off a path the target does not have (`control.permissions`, carrying an
    # array) and onto the two boolean leaves it declares under `admin`, so the
    # document's top-level roster changed with it. The claim is unchanged: both
    # worked-out rows are in, and the placement row is not.
    # ⚠⚠ ★★★★★ R2079 — **the roster is DERIVED, and the claim is stated as the
    # claim.** This read `["admin", "connect", "id", "listen", "mode",
    # "transport"]`, and R2074 removed `connect` from it: the router dials
    # nothing now, so the row worked out from its wires is not painted and does
    # not ship. Writing the roster out made this check a transcription of the
    # fixture; what it is FOR is the sentence beside it — every row that is
    # configuration ships, and the placement row does not.
    #
    # ⇒ ★ asked of the specification, so the two halves of the claim are checked
    # against what the screen declares rather than against a list somebody
    # maintained. `aside` is the column that says a row is placement rather than
    # configuration, which is what makes the exclusion checkable at all.
    shipped = sorted(produced["config"]["nodes"][OPENS_SELECTED])
    live_rows = {r["key"]: r for r in rows(tf)}
    ships = sorted(
        {key.split(".", 1)[0] for key, r in live_rows.items() if not r["aside"]}
    )
    placements = {
        key.split(".", 1)[0] for key, r in live_rows.items() if r["aside"]
    }
    assert_eq(
        shipped,
        ships,
        "F: every row the screen calls configuration ships, and the roster is "
        "the specification's rather than a list written here",
    )
    ok(
        f"F: ★ and no placement row is in it — {sorted(placements)} vs {shipped}",
        not (placements & set(shipped)),
    )


# ── G: the pointer reaches the act ──────────────────────────────────────────


def g_the_seat_answers_a_real_press(tf) -> None:
    banner("G — the seat at the row's edge, through the router")
    seat = rects(tf)[form_part_tag(tf, "author", "host", ext=EXT)]
    centre = (seat[0] + seat[2] // 2, seat[1] + seat[3] // 2)
    resolved = tf.invoke(f"{EXT}/point", f"{centre[0]},{centre[1]}")
    assert_eq(
        resolved,
        "author:host",
        "G: ★ the screen resolves the press on that rectangle to the take-over "
        "of that row, not to the removal of it",
    )
    tf.click(centre)
    took = row(tf, "host")
    assert_eq(took["source"], None, "G: ★★ and the press took the row over")
    assert_eq(
        took["aside"],
        "placement",
        "G: ★★★ taking a row over does not move it into the document — the two "
        "axes are separate, and this is the crossing that proves it",
    )
    ok(
        "G: so it is still out of the configuration after a person owns it",
        "host" not in document(tf),
    )
    tf.invoke(f"{EXT}/set_field", "host=host-c")
    assert_eq(row(tf, "host")["value"], "host-c", "G: and now it takes a value")
    tf.invoke(f"{EXT}/export", "")
    placed = {
        entry["node"]: entry["host"]
        for entry in json.loads(tf.query(f"{EXT}/produced"))["config"]["order"]
    }
    assert_eq(
        placed["R-01"],
        "host-c",
        "G: ★★★★★ and the PLAN runs it there — a value a person owns that the "
        "plan ignored would be the two-answers-to-one-question this round is "
        "about, one round later",
    )
    assert_eq(
        placed["P-01"],
        "host-a",
        "G: while every card nobody has told still runs where it is drawn",
    )


# ── H: taking the wire's row over does not take the wires away ──────────────


def h_taking_the_wires_row_over_leaves_the_wires_reaching_it(tf) -> None:
    banner("H — a card may dial outside the drawing, and the drawing still reaches it")
    opening = json.loads(tf.query(f"{EXT}/gate"))
    # ★ R2079 — a card that DIALS, for section E's measured reason: the router
    # dials nothing since R2074, so it has no wire-derived row for this section
    # to take over. Derived rather than named, so the section keeps its meaning
    # on any graph where such a card exists.
    dials = sorted({link["from"] for link in json.loads(tf.query(f"{EXT}/links"))})
    assert dials, "H: no card dials anything, so there is no shared row to take over"
    tf.invoke(f"{EXT}/select", dials[0])
    drawn = [part.strip() for part in row(tf, dialled_key(tf))["value"].split(",")]
    tf.invoke(f"{EXT}/author_field", dialled_key(tf))
    taken = row(tf, dialled_key(tf))
    assert_eq(
        taken["written"],
        ", ".join(drawn),
        "H: the row is theirs, seeded with what the canvas was saying",
    )
    # ★★★★★ R1717 rewrote this section. R1716 answered "a card may be told to
    # dial something this canvas does not draw" by letting the written value
    # take the whole row, and paid for the lost drawn links with a gate warning.
    # A row can hold both contributions now, so the payment is gone: writing an
    # address of your own leaves every drawn one reaching the row.
    tf.invoke(f"{EXT}/set_field", "connect.endpoints=tcp/10.0.0.21:7449")
    shared = row(tf, dialled_key(tf))
    assert_eq(
        shared["written"],
        "tcp/10.0.0.21:7449",
        "H: their half is exactly what they typed",
    )
    ok(
        f"H: ★★★★★ and the canvas still reaches the row ({shared['value']!r}) — "
        f"R1716 dropped {drawn} here and warned about it instead",
        all(address in shared["value"] for address in drawn),
    )
    assert_eq(shared["source"], "wire", "H: which is what the row says it shares with")
    findings = json.dumps(json.loads(tf.query(f"{EXT}/gate")), ensure_ascii=False)
    ok(
        "H: ★★ the drawn addresses raise nothing — they are in the row by "
        "construction, so the R1716 warning could never fire again",
        not any(address in findings for address in drawn),
    )
    ok(
        "H: ★★★ and what the gate DOES say is the fact underneath it: this card "
        "dials an address nothing in the graph listens on",
        "10.0.0.21" in findings and "nothing here listens on" in findings,
    )
    verdict = json.loads(tf.query(f"{EXT}/verdict"))
    ok(
        f"H: ★★ it WARNS and does not block — dialling outside the drawing is a "
        f"legitimate thing to want ({verdict})",
        verdict["may_launch"],
    )
    tf.invoke(f"{EXT}/remove_field", dialled_key(tf))
    assert_eq(
        row(tf, dialled_key(tf))["source"],
        "wire",
        "H: ★ and giving their half back leaves the row worked out from the wires",
    )
    assert_eq(
        json.loads(tf.query(f"{EXT}/gate")),
        opening,
        "H: leaving the gate exactly as the screen opened it",
    )
    # ★ R2079 — and the SELECTION back too, for section E's reason: this
    # section moves it now, and a section that changes shared state puts it
    # back or the walk's order becomes part of its meaning.
    tf.invoke(f"{EXT}/select", OPENS_SELECTED)


def main() -> int:
    with RpcSubprocess(EXAMPLE) as tf:
        a_every_row_says_where_its_value_came_from(tf)
        b_a_derived_row_refuses_the_write(tf)
        c_taking_a_row_over_is_announced(tf)
        d_a_derived_row_is_a_read_out(tf)
        e_drawing_a_link_moves_the_row(tf)
        f_the_document_carries_what_belongs_in_it(tf)
        h_taking_the_wires_row_over_leaves_the_wires_reaching_it(tf)
        g_the_seat_answers_a_real_press(tf)
    print(f"\n{len(CHECKS)} named check(s) beyond the equalities:")
    for line in CHECKS:
        print(f"  - {line}")
    return 0


if __name__ == "__main__":
    run_demo("r1716_a_row_says_where_its_value_came_from", main)
