#!/usr/bin/env python3
"""R1687 §5.20 §5.21 — the graph says how it is brought up, and the form says
what it could not express.

Drives `hello-node-lab` over JSON-RPC. The screen's operation table has carried
`export the configuration` and `produce the launch script` as **absent on both
channels** since R1677 — no verb, no gesture. They are the last two of the
reference's own "what leaves the screen" group, and the reference writes them as
one group for a reason this round had to take seriously: they are ONE derivation
rendered twice, so building either alone means building the derivation and using
half of it.

The derivation is the launch ORDER, each node's configuration document, and the
rows of its form that the document could not carry. Two substrate pieces carry
it, and each was forced by trying to write this:

  * `Document::launch_order` — the order falls out of the link direction, by
    Kahn's walk. The first draft bucketed by "is reached / reaches out" and
    justified it with "a topological sort would have to refuse, because two
    peers that dial each other are a legal mesh". BOTH halves were false: the
    model refuses an authored cycle outright, and with the excuse gone the
    buckets were simply wrong — in `a -> b -> c -> d` the middle two share a
    bucket and a NAME decides their order while the links demand the reverse.
    The first fixture was three deep and passed.

  * `ConfigForm::compose` — the read half of this form has always been total
    (`adopt` names every leaf it could not place, and says so in its own
    documentation), and the WRITE half was all-or-nothing. One unparseable row
    and a caller got no document at all — no use to anybody whose job is to ship
    the configuration and report what did not fit. The asymmetry was invisible
    while the only caller was a launch gate, because a gate only wants the
    yes-or-no.

  (A) boot — the table declares both, on both channels, and the two seats are
      painted side by side.
  (B) they are pressable where they are painted, at the CORNERS.
  (C) nothing is produced until somebody asks: two keys, both null.
  (D) press config — the plan arrives, and its order is the LINKS' order.
  (E) press script — every node gets a file and exactly one start line, and
      each host gets its own branch.
  (F) the two artifacts are the same plan.
  (G) a card switched off leaves both at once.
  (H) a value that cannot be expressed is carried as news — into the document,
      into the script, and into the sentence a person reads.
  (I) the seats are named buttons to a screen reader.
  (J) the wire and the seats are the same act.

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
    assert_eq,
    run_demo,
)

EXAMPLE = "hello-node-lab"
EXT = "/external"
VIEWPORT = (1440, 900)

# The two seats, and the operations they answer.
SEATS = {
    "lab.toolbar.config": "export the configuration",
    "lab.toolbar.script": "produce the launch script",
}

# The row whose ceiling the specification's own `validate` argument names. One
# past it is a value the form holds and no document can carry.
OVER = ("transport.link.tx.batch_size", "70000")


def q(tf, path):
    return tf.query(f"{EXT}/{path}")


def rects(tf):
    return abs_rects_of(tf.snapshot(source="paint", viewport=VIEWPORT))


def produced(tf):
    # ★ `produced`, not `export`: the slot holds BOTH artifacts, and `export` is
    # an action — one address on two channels is refused by the server as
    # `PathIsAReadSlot`, which is how the first draft of this demo found out.
    return json.loads(q(tf, "produced"))


def press(tf, tag):
    box = rects(tf)[tag]
    tf.click(at=(box[0] + box[2] // 2, box[1] + box[3] // 2))


def resolves(tf, at) -> str:
    """What a press at that pixel would be, asked of the screen rather than
    computed here."""
    return tf.invoke(f"{EXT}/point", f"{at[0]},{at[1]}")


def links(tf):
    return json.loads(q(tf, "links"))


def access(tf):
    resp = tf.request("scene/access", {})
    assert resp is not None and resp.result is not None, "scene/access must answer"
    return resp.result["nodes"]


def order_of(plan) -> list[str]:
    return [row["node"] for row in plan["order"]]


def inside(a, b) -> bool:
    return (
        a[0] >= b[0]
        and a[1] >= b[1]
        and a[0] + a[2] <= b[0] + b[2]
        and a[1] + a[3] <= b[1] + b[3]
    )


def overlaps(a, b) -> bool:
    return (
        a[0] < b[0] + b[2]
        and b[0] < a[0] + a[2]
        and a[1] < b[1] + b[3]
        and b[1] < a[1] + a[3]
    )


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) the declaration, and the two seats ──────────────────
        spec = json.loads(q(tf, "spec"))
        rows = {row["name"]: row for row in spec["operations"]}
        for tag, name in SEATS.items():
            op = rows[name]
            assert_eq(op["gesture"], True, f"★ '{name}' has a way in for a person")
            assert op["verb"], f"★ and a verb an agent can call: {op}"
            assert_eq(
                op["absent"],
                False,
                f"so '{name}' is answered on both channels — it was absent on "
                "both since R1677",
            )

        painted = rects(tf)
        for tag in SEATS:
            assert tag in painted, f"{tag} is painted: {sorted(painted)[:12]}…"
        config_box, script_box = painted["lab.toolbar.config"], painted[
            "lab.toolbar.script"
        ]
        assert config_box[1] == script_box[1], (
            "★ side by side, which is where the reference puts them — they are "
            f"one derivation rendered twice: {config_box} against {script_box}"
        )
        assert config_box[0] + config_box[2] <= script_box[0], (
            f"and they do not overlap: {config_box} against {script_box}"
        )

        # ── (B) pressable where painted, at the CORNERS ─────────────
        # A check aimed at a centre cannot see an error smaller than half a
        # control, which is how a 1px transform defect survived two rounds
        # (R1684). The corners are where it shows.
        for tag in SEATS:
            box = painted[tag]
            want = "config" if tag.endswith("config") else "script"
            for dx, dy in ((0, 0), (box[2] - 1, 0), (0, box[3] - 1), (box[2] - 1, box[3] - 1)):
                at = (box[0] + dx, box[1] + dy)
                assert_eq(
                    resolves(tf, at),
                    want,
                    f"★★ the corner {at} of {tag} (painted {box}) must be that "
                    "seat — the paint and the hit test are two readings of one "
                    "layout",
                )

        # ── (C) nothing is produced until somebody asks ─────────────
        empty = produced(tf)
        assert_eq(
            sorted(empty),
            ["config", "script"],
            "★ both halves are present as KEYS — a null is an answer and a "
            "missing key is a question",
        )
        assert empty["config"] is None, f"nothing exported yet: {empty}"
        assert empty["script"] is None, f"and no script yet: {empty}"

        # ── (D) press config — the plan, in the LINKS' order ────────
        press(tf, "lab.toolbar.config")
        plan = produced(tf)["config"]
        assert plan is not None, "the export landed"
        assert produced(tf)["script"] is None, (
            "★ and it did not produce the other one — two operations, two "
            "artifacts, or neither has a witness of its own"
        )

        ordered = order_of(plan)
        assert ordered, f"the opening graph has cards: {plan}"
        assert_eq(
            sorted(ordered),
            sorted(plan["nodes"]),
            "every ordered node has a configuration and no other does",
        )

        # ★★ The order is the LINK direction, checked against the links the
        # screen publishes rather than against a list written here: a node that
        # is dialled has to be standing before the one that dials it.
        place = {name: at for at, name in enumerate(ordered)}
        checked = 0
        for link in links(tf):
            src, dst = link["from"], link["to"]
            if src not in place or dst not in place:
                continue  # an end that is switched off is in neither
            assert place[dst] < place[src], (
                f"★★ {dst} is dialled by {src}, so it has to be up first — "
                f"the order is {ordered}"
            )
            checked += 1
        assert checked >= 3, f"the opening graph exercised {checked} links"

        # Every entry says WHY it sits where it does, in a closed vocabulary.
        for row in plan["order"]:
            assert row["standing"] in ("first", "between", "last", "alone"), row
            assert row["host"], f"and which machine it starts on: {row}"
            assert row["program"], f"and what it runs: {row}"

        # Each host holds exactly the nodes the order assigns to it.
        for host, held in plan["hosts"].items():
            assert_eq(
                sorted(held),
                sorted(row["node"] for row in plan["order"] if row["host"] == host),
                f"{host}'s roster is the order's own answer",
            )

        # ── (E) press script — one file and one start per node ──────
        press(tf, "lab.toolbar.script")
        script = produced(tf)["script"]
        assert script is not None, "the script landed"
        for name in ordered:
            assert f'cat > "$OUT/{name}.json"' in script, (
                f"{name} gets a configuration file:\n{script}"
            )
        starts = {
            name: script.count(f'-c "$OUT/{name}.json" &') for name in ordered
        }
        for name, count in starts.items():
            assert_eq(count, 1, f"★ {name} is started exactly once:\n{script}")
        for host in plan["hosts"]:
            assert f'if [ "$HOST" = "{host}" ]; then' in script, (
                f"★★ {host} gets its own branch — one file cannot start "
                f"processes on two machines:\n{script}"
            )
        assert script.startswith("#!/usr/bin/env bash"), script[:80]

        # ── (F) the two artifacts are the same plan ─────────────────
        heredocs = [
            line.split('cat > "$OUT/')[1].split(".json")[0]
            for line in script.splitlines()
            if line.startswith('cat > "$OUT/')
        ]
        assert_eq(
            heredocs,
            ordered,
            "★★★ the script writes the files in the ORDER the document "
            "publishes — one derivation, or the two would drift",
        )

        # ── (G) a card switched off leaves both at once ─────────────
        victim = ordered[-1]
        tf.invoke(f"{EXT}/disable", victim)
        press(tf, "lab.toolbar.config")
        press(tf, "lab.toolbar.script")
        after = produced(tf)
        assert victim not in order_of(after["config"]), (
            f"★★ a card that produces nothing is not started: "
            f"{order_of(after['config'])}"
        )
        assert victim not in after["config"]["nodes"], "and has no configuration"
        assert f'"$OUT/{victim}.json"' not in after["script"], (
            f"★★ nor does the script write one for it:\n{after['script']}"
        )
        assert_eq(
            len(order_of(after["config"])),
            len(ordered) - 1,
            "and nothing else moved",
        )
        tf.invoke(f"{EXT}/disable", victim)  # back on — it is a toggle

        # ── (H) what could not be expressed is carried as NEWS ──────
        key, over = OVER
        tf.invoke(f"{EXT}/select", spec["selected_node"])
        tf.invoke(f"{EXT}/set_field", f"{key}={over}")
        said = tf.invoke(f"{EXT}/export", "")
        plan = produced(tf)["config"]
        # R1788 — `uncarried`, not `unexpressed`. The derivation moved into
        # `pinion-node-graph`, where `Unexpressed`'s reason vocabulary belongs
        # to the config FORM and a pure-data crate cannot hold it; the plan
        # carries the rendered sentence instead, under its own word. One word
        # per fact, which is why the wire key moved with it.
        uncarried = plan["uncarried"]
        assert_eq(len(uncarried), 1, f"one row, named: {uncarried}")
        assert_eq(uncarried[0]["key"], key)
        assert_eq(
            uncarried[0]["shown"],
            over,
            "★ the value AS THE ROW SHOWS IT — there is no parsed one, which "
            "is generally the reason it is here",
        )
        assert uncarried[0]["why"], f"and why: {uncarried[0]}"

        # ★★★ The rest of that node's configuration still ships. This is the
        # whole reason `compose` exists: before it, one bad value cost the file.
        entry = plan["nodes"][spec["selected_node"]]
        assert entry, f"the node still has a configuration: {entry}"
        assert "batch_size" not in json.dumps(entry), (
            f"★ and the refused row is not silently in it: {entry}"
        )
        assert "1 not expressed" in said, f"★★ the toast says so: {said}"
        assert_eq(q(tf, "toast"), said, "which is what a person reads")

        tf.invoke(f"{EXT}/script", "")
        assert "not in any file above" in produced(tf)["script"], (
            "★★ and the script keeps it as a comment — a report that lived "
            "only in a toast would be gone by the time it mattered"
        )

        # ── (I) a screen reader is told what the seats do ───────────
        # ★★★★ The WHOLE toolbar, not the two this round added. Asking whether
        # the new seats were named is what found that none of them were — the
        # strip holding the launch control had no access node at all, so a
        # reader could be told the gate's verdict and not that there was a
        # button for it. A gate placed where the last defect was finds only the
        # last defect (R1684.2).
        nodes = {n["tag"]: n for n in access(tf) if n.get("tag")}
        painted = rects(tf)
        toolbar = painted["lab.toolbar"]
        in_toolbar = {
            tag: box
            for tag, box in painted.items()
            if tag != "lab.toolbar"
            and inside(box, toolbar)
            and resolves(tf, (box[0] + box[2] // 2, box[1] + box[3] // 2))
            not in ("nothing", "canvas")
        }
        # ★ A tag INSIDE another one is part of that control, not a seat of its
        # own — `lab.toolbar.run.label` is the run button's own text and resolves
        # to the button because it is painted in it. `parts` means "inside the
        # control" here for the same reason R1686 found it does in the form.
        pressable = sorted(
            tag
            for tag, box in in_toolbar.items()
            if not any(
                other != tag and inside(box, in_toolbar[other])
                for other in in_toolbar
            )
        )
        assert len(pressable) >= 6, f"the toolbar's seats: {pressable}"
        for tag in pressable:
            seat = nodes.get(tag)
            assert seat is not None, (
                f"★★ {tag} is pressable and has no access node — every one of "
                f"{pressable} must announce itself"
            )
            assert_eq(seat["role"], "button", f"{tag} announces as a button")
            assert seat.get("name"), (
                f"★ and is NAMED by what it does: {seat} — a bare glyph "
                "announces as its own character"
            )
        for tag, name in (
            ("lab.toolbar.config", "export the configuration"),
            ("lab.toolbar.script", "produce the launch script"),
        ):
            assert_eq(nodes[tag]["name"], name, f"{tag} says what it does")

        # ── (I.2) no CONTROL's own name is cut ──────────────────────
        # ★★★★★ Found by looking at the screen — the view-reset seat painted
        # `ho…` for `home`, a four-letter button name cut to two letters and a
        # dot. Six other runs on this screen are shortened and every one of them
        # is a long sentence elided into a bounded strip, which is what that
        # behaviour is for; a control's own NAME is the case where it is a
        # defect, and nothing was asking. The eye found it once; this is what
        # finds it next time.
        runs = tf.request("scene/text_painted", {}).result["runs"]
        seats = {
            tag: painted[tag] for tag in pressable
        }  # the toolbar's own controls, from the roster above
        for run in runs:
            box = (run["x"], run["y"], run["w"], run["h"])
            for tag, seat in seats.items():
                if not overlaps(box, seat):
                    continue
                # ★★★★★ A caption may not leave the seat it names. Narrowing a
                # seat to prove the cut-label check below fired did NOT fire it:
                # the caption was authored with its own width, so it hung past
                # the button instead of eliding inside it, and the check went
                # quiet on the worse of the two defects. The captions are
                # derived from the seat now, and this is what holds that.
                assert inside(box, seat), (
                    f"★★ {tag}'s caption {run['content']!r} is painted {box} "
                    f"and its seat is {seat} — a caption outside its control "
                    "cannot be cut, so nothing below would ever notice it"
                )
                assert run.get("painted") is None, (
                    f"★★ {tag}'s label is cut: {run['content']!r} painted as "
                    f"{run['painted']!r} in {run['w']}px. A control whose own "
                    "name cannot be read is not a control — widen the seat."
                )
        shortened = [r for r in runs if r.get("painted") is not None]
        assert len(shortened) >= 1, (
            "this screen does elide long runs, so a zero here would mean the "
            "check above is vacuous"
        )

        # ── (J) the wire and the seats are the same act ─────────────
        tf.invoke(f"{EXT}/reset", "fields")
        by_wire = tf.invoke(f"{EXT}/export", "")
        wired = produced(tf)["config"]
        press(tf, "lab.toolbar.config")
        assert_eq(
            order_of(produced(tf)["config"]),
            order_of(wired),
            "★ the verb and the seat produce the same plan, through the same "
            "function",
        )
        assert_eq(q(tf, "toast"), by_wire, "and say the same sentence")


if __name__ == "__main__":
    sys.exit(run_demo("R1687 §5.21 — what leaves the screen", body))
