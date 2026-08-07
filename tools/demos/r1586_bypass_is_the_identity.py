#!/usr/bin/env python3
"""R1586 + R1587 §5.38 §5.52 §2 #7 — a node says how it takes part, and only
one of those facts is the graph's meaning.

Phase (H2) is R1587: a PORT declares whether a value passes through it, which is
the one thing the identity rule cannot say on its own. One demo because it is
one subject.

Taking a stage OUT of a pipeline is half of what an editor does. A node can be
**bypassed** — it stops computing and the values at its inputs pass through it
— or **dissolved**, which does the same to the structure and deletes it. Both
read ONE derivation (`Document::passthrough`), so the preview and the edit
cannot disagree; Blender unifies them too and says so in its own operator
description ("Remove nodes and reconnect nodes as if deletion was muted"). What
is chosen differently here is the rule underneath, and what is said when it
cannot be applied.

What this script checks, and why each check discriminates:

* **The rule is one sentence: a bypassed node is the identity as far as its
  signature allows.** Output `n` takes input `n` when their types agree, else
  the lowest-indexed input of the right type, else nothing.
* **PAST BLENDER (1): the route does not read the wiring.** Blender scores every
  input against every output through a static socket-type table
  (`get_internal_link_type_priority`) and breaks ties by whether the input
  happens to be WIRED (`find_internally_linked_input`, `node_tree_update.cc`,
  `8cf50599`). Under that rule, unplugging one port changes which value comes
  out of a DIFFERENT port of the same bypassed node. Here the answer is asserted
  to be identical before and after a port is unwired.
* **PAST BLENDER (2): an output no input can feed is NAMED.** Blender's
  derivation simply produces no internal link for it and `node_internal_relink`
  then removes the downstream link with `node_remove_link`, returning `void`. A
  value disappearing is the thing an author most needs told.
* **PAST BLENDER (3): the derivation is not stored.** Blender materialises it
  into `node->runtime->internal_links` and keeps a tree-update pass whose job is
  to notice when the stored answer has gone stale. Asserted here by editing the
  graph and reading the routing with nothing asked to refresh.
* **PAST BLENDER (4): a bypassed NODE and a muted LINK are different words,
  because they are opposite behaviours.** Blender spells both "mute"
  (`NODE_MUTED`, `NODE_LINK_MUTED`): one passes a value through, the other stops
  one. Both are exercised on the same wire and the two results are asserted
  different.
* **PAST BLENDER (5): what a node LOOKS like is a different type from what it
  MEANS.** Blender keeps `NODE_COLLAPSED`, `NODE_PREVIEW`, `NODE_SELECT` and
  `NODE_MUTED` in one `flag` integer, so nothing in its model says which bits
  its evaluator may read. Here every appearance toggle is driven over the wire
  and the evaluated value is asserted unchanged.
* **R1587: the extension point is a PORT declaration, not a per-node hook.**
  Censused at `8cf50599`, eleven Blender node types register
  `internally_linked_input` and their callbacks compute exactly three things —
  the identity by name (7), the identity by index (1), and "skip the leading
  control input" (3) — every one of which this crate's default already produces.
  What is left over is exclusion, which Blender spells `no_mute_links` and
  **sets on no node type in its tree**. So twelve C callbacks there reduce to
  one declaration here.
* **Where a bypass and a dissolve CANNOT agree, the difference is named.** A
  bypass passes an unwired port's declared default on; a dissolve has no link to
  redirect and removes the downstream one. The two are driven on the same node
  and shown to produce different values, with `severed` naming why.

Run from the workspace root:
    cargo build -p hello-node-groups --release
    python3 tools/demos/r1586_bypass_is_the_identity.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
)

#: A widget's primary External is addressed by the framework path, not the tag.
EXT = "/external"

#: `hello-node-groups`'s seed, mirrored rather than imported — a demo that read
#: the fixture out of the code under test could not catch it changing.
BASE, BLEND, LEVEL, MIX, FADE, OUT = 0, 1, 2, 3, 4, 5

#: Link ids, in the order the seed wires them.
L_BASE, L_BLEND, L_LEVEL, L_MIX, L_FADE = 0, 1, 2, 3, 4

#: mix(200,60,60 @ 75% with 40,90,220 @ 25%) — what the seeded Mix answers.
SEEDED = "160,67,100"


def q(tf: RpcSubprocess, path: str):
    return tf.query(f"{EXT}/{path}")


def inv(tf: RpcSubprocess, path: str, args):
    return tf.invoke(f"{EXT}/{path}", args)


def fields(reply: str) -> dict[str, str]:
    """`"routes:0<-0|dropped:|identity:true"` -> a dict."""
    out: dict[str, str] = {}
    for piece in str(reply).split("|"):
        key, _, value = piece.partition(":")
        out[key] = value
    return out


def through(tf: RpcSubprocess, node: int) -> dict[str, str]:
    return fields(inv(tf, "passthrough", str(node)))


def refused(tf: RpcSubprocess, path: str, args) -> None:
    """Run a verb that must be refused."""
    try:
        inv(tf, path, args)
    except AssertionError:
        pass
    except Exception:  # noqa: BLE001 - any refusal shape is fine here
        pass
    else:
        raise AssertionError(f"{path}({args!r}) was expected to be refused")


def body() -> None:
    with RpcSubprocess("hello-node-groups", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)

        # ── (A) the derivation, before anything is bypassed ─────────────────
        assert_eq(q(tf, "nodes"), 6, "A: the seeded material")
        assert_eq(q(tf, "bypassed"), "", "A: nothing is bypassed yet")
        assert_eq(q(tf, "muted_links"), "", "A: and no wire is muted")
        assert_eq(q(tf, "last_rewire"), "", "A: nothing has been rewired")

        fade = through(tf, FADE)
        assert_eq(fade["routes"], "0<-0", "A: Fade is Colour,Amount -> Colour")
        assert_eq(fade["identity"], "true", "A: the output takes the input at its own index")
        assert_eq(
            fade["unreached"],
            "1",
            "A: the Factor reaches no output, and is NAMED rather than left "
            "to be worked out from the routes",
        )
        mix = through(tf, MIX)
        assert_eq(mix["routes"], "0<-0", "A: Mix passes its Base through")
        assert_eq(mix["unreached"], "1,2", "A: Blend and Factor reach nothing")

        swatch = through(tf, BASE)
        assert_eq(swatch["routes"], "", "A: a source node has nothing to pass")
        assert_eq(
            swatch["dropped"],
            "0",
            "A: PAST BLENDER — the output that would carry NOTHING is named. "
            "Blender produces no internal link for it and node_internal_relink "
            "then deletes the downstream wire, returning void",
        )
        assert_eq(swatch["identity"], "false", "A: nothing passed is not the identity")
        assert_eq(through(tf, OUT)["unreached"], "0", "A: the sink takes and emits nothing")

        # ── (B) a bypassed node does not compute ────────────────────────────
        assert_eq(inv(tf, "connect", "2.0>4.1"), "linked", "B: give Fade a real factor")
        assert_eq(inv(tf, "node_value", str(FADE)), "147,77,102", "B: it fades")
        assert_eq(inv(tf, "bypass", str(FADE)), "4=true", "B: take it out of the meaning")
        assert_eq(q(tf, "bypassed"), "4", "B: and it says so")
        assert_eq(
            inv(tf, "node_value", str(FADE)),
            SEEDED,
            "B: the Colour input passes straight out, unfaded",
        )
        assert_eq(
            through(tf, FADE)["routes"],
            "0<-0",
            "B: PAST BLENDER — the routing is DERIVED, not stored. Blender keeps "
            "it in node->runtime->internal_links with a tree-update pass to "
            "notice when the stored copy has gone stale",
        )
        assert_eq(inv(tf, "bypass", str(FADE)), "4=false", "B: put it back")
        assert_eq(inv(tf, "node_value", str(FADE)), "147,77,102", "B: computing again")

        # ── (C) the route is a function of the signature alone ──────────────
        assert_eq(through(tf, MIX)["routes"], "0<-0", "C: Mix routes from its Base")
        rewired = fields(inv(tf, "detach", str(BASE)))
        assert_eq(rewired["bridged"], "", "C: a source node bridges nothing")
        assert_eq(rewired["severed"], "3.0", "C: so the wire it fed is severed, by name")
        assert_eq(rewired["lossless"], "false", "C: and the loss is stated")
        assert_eq(
            fields(q(tf, "last_rewire"))["severed"],
            "3.0",
            "C: and the report is readable after the fact, not only at the call",
        )
        assert_eq(
            through(tf, MIX)["routes"],
            "0<-0",
            "C: PAST BLENDER — unwiring the Base did NOT re-route the output. "
            "Blender's linked-tie-break would now pick the Blend, so unplugging "
            "one port changes which value leaves by another",
        )
        assert_eq(inv(tf, "bypass", str(MIX)), "3=true", "C: bypass it now")
        assert_eq(
            inv(tf, "node_value", str(MIX)),
            "0,0,0",
            "C: the Base port's own declared resting value passes through",
        )

        # ── (D) a muted LINK is the opposite of a bypassed NODE ─────────────
        assert_eq(inv(tf, "reset", ""), "reset", "D: back to the seed")
        assert_eq(inv(tf, "node_value", str(FADE)), SEEDED, "D: the fade is a no-op at 0%")
        assert_eq(inv(tf, "mute_link", str(L_MIX)), "muted", "D: stop the wire into Fade")
        assert_eq(q(tf, "muted_links"), "3", "D: and it says which")
        assert_eq(q(tf, "links"), 5, "D: the wire is still THERE — five, as seeded")
        assert_eq(
            inv(tf, "node_value", str(FADE)),
            "0,0,0",
            "D: PAST BLENDER — a muted LINK stops the value, where a muted NODE "
            "passes one through. Blender calls both 'mute' (NODE_LINK_MUTED, "
            "NODE_MUTED), so one word names two opposite behaviours there",
        )
        assert_eq(inv(tf, "mute_link", str(L_MIX)), "unmuted", "D: and it comes back")
        assert_eq(inv(tf, "node_value", str(FADE)), SEEDED, "D: carrying again")

        # ── (E) a dissolve is the same derivation, applied to the structure ──
        assert_eq(inv(tf, "reset", ""), "reset", "E: back to the seed")
        assert_eq(inv(tf, "bypass", str(MIX)), "3=true", "E: bypass the Mix")
        bypassed_value = inv(tf, "node_value", str(FADE))
        assert_eq(bypassed_value, "200,60,60", "E: the Base reaches the Fade")

        assert_eq(inv(tf, "reset", ""), "reset", "E: and again from the seed")
        dissolved = fields(inv(tf, "dissolve", str(MIX)))
        assert_eq(dissolved["bridged"], "0.0->4.0", "E: the Base is wired straight to the Fade")
        assert_eq(dissolved["severed"], "", "E: nothing was lost")
        assert_eq(dissolved["removed"], "4", "E: four wires touched the Mix")
        assert_eq(q(tf, "nodes"), 5, "E: the node is gone")
        assert_eq(q(tf, "links"), 2, "E: four wires became one bridge, plus the fade's")
        assert_eq(
            inv(tf, "node_value", str(FADE)),
            bypassed_value,
            "E: ONE derivation, so bypassing and dissolving agree on the value",
        )
        assert_eq(q(tf, "valid"), "ok", "E: and the document still satisfies its invariants")

        # ── (F) where they cannot agree, the difference is named ────────────
        # A bypass passes an unwired port's declared default on; a dissolve has
        # no link to redirect. Driven on the Level, whose output nothing can
        # feed, so the two answers actually differ.
        assert_eq(inv(tf, "reset", ""), "reset", "F: back to the seed")
        assert_eq(inv(tf, "bypass", str(LEVEL)), "2=true", "F: bypass the Level")
        assert_eq(
            inv(tf, "node_value", str(MIX)),
            "null",
            "F: its output carries nothing, and the wire delivers that nothing",
        )
        assert_eq(inv(tf, "reset", ""), "reset", "F: and again from the seed")
        lost = fields(inv(tf, "dissolve", str(LEVEL)))
        assert_eq(lost["bridged"], "", "F: nothing to bridge from")
        assert_eq(lost["severed"], "3.2", "F: the Factor wire is severed, by name")
        assert_eq(lost["lossless"], "false", "F: and the report says a value went")
        assert_eq(
            inv(tf, "node_value", str(MIX)),
            "120,75,140",
            "F: with the wire GONE the Factor port falls back to its own 50%, "
            "which a bypass cannot produce — the honest limit, and `severed` is "
            "where it is stated. Blender removes the same wire in silence",
        )

        # ── (G) a detach rewires and leaves the node ────────────────────────
        assert_eq(inv(tf, "reset", ""), "reset", "G: back to the seed")
        detached = fields(inv(tf, "detach", str(MIX)))
        assert_eq(detached["bridged"], "0.0->4.0", "G: the same bridge a dissolve makes")
        assert_eq(q(tf, "nodes"), 6, "G: but the node is still there")
        assert_eq(q(tf, "links"), 2, "G: wired to nothing")
        assert_eq(inv(tf, "node_value", str(FADE)), "200,60,60", "G: and the value flows past it")

        # ── (H) what a node LOOKS like cannot change what it means ──────────
        assert_eq(inv(tf, "reset", ""), "reset", "H: back to the seed")
        assert_eq(
            inv(tf, "visible_ports", str(FADE)),
            "in:0,1|out:0|hidden_in:|hidden_out:",
            "H: every port is drawn while nothing asks otherwise",
        )
        assert_eq(inv(tf, "collapse", str(FADE)), "4=true", "H: collapse it")
        assert_eq(
            inv(tf, "visible_ports", str(FADE)),
            "in:0|out:0|hidden_in:1|hidden_out:",
            "H: the unwired Factor is not drawn — and is NAMED, so an editor can "
            "offer to show it without recomputing the complement",
        )
        assert_eq(inv(tf, "hide_ports", str(FADE)), "4=true", "H: and the other toggle")
        looks = fields(inv(tf, "looks", str(FADE)))
        assert_eq(looks["collapsed"], "true", "H: both are held")
        assert_eq(looks["hide_unused_ports"], "true", "H: independently")

        # Wire the Factor before asserting the value is untouched. Unwired, this
        # Fade fades by 0% and so computes exactly what BYPASSING it would pass
        # through — an assertion made there could not tell the two apart, and a
        # counterfactual that made `collapsed` read as `bypassed` proved it by
        # going unnoticed.
        assert_eq(inv(tf, "connect", "2.0>4.1"), "linked", "H: give it a real factor")
        assert_eq(
            inv(tf, "visible_ports", str(FADE)),
            "in:0,1|out:0|hidden_in:|hidden_out:",
            "H: and a port that is used is drawn again, collapsed or not — the "
            "hiding is a question about the WIRING, which is why only the "
            "document can answer it",
        )
        computed = inv(tf, "node_value", str(FADE))
        assert_eq(computed, "147,77,102", "H: it is really fading now")
        assert_eq(
            looks["bypassed"],
            "false",
            "H: PAST BLENDER — the bypass is a DIFFERENT FIELD from the looks. "
            "Blender keeps NODE_COLLAPSED, NODE_PREVIEW, NODE_SELECT and "
            "NODE_MUTED in one flag integer, so which bits its evaluator may "
            "read is stated nowhere in its model",
        )
        assert_eq(q(tf, "bypassed"), "", "H: and the bypassed set did not move")
        for verb in ("collapse", "hide_ports", "collapse", "hide_ports"):
            inv(tf, verb, str(FADE))
            assert_eq(
                inv(tf, "node_value", str(FADE)),
                computed,
                f"H: {verb} changed nothing about what the node computes",
            )
        assert_eq(
            inv(tf, "bypass", str(FADE)),
            "4=true",
            "H: while the one fact that IS the meaning does change it",
        )
        assert_eq(
            inv(tf, "node_value", str(FADE)),
            SEEDED,
            "H: which is what makes the assertions above discriminating",
        )

        # ── (H2) a PORT says whether a value passes through it (R1587) ──────
        # The one shape the identity rule gets wrong on its own: a control input
        # that shares the data type it controls. Blender reaches the same answer
        # with a per-node C callback (`node_geo_switch`); here it is a
        # declaration on the port, which is the extension point ELEVEN of
        # Blender's callbacks turn out not to need, because our default is the
        # identity rather than a static socket-type priority table.
        assert_eq(inv(tf, "reset", ""), "reset", "H2: back to the seed")
        assert_eq(inv(tf, "add", "cap"), "6", "H2: a Cap node")
        assert_eq(
            inv(tf, "node_ports", "6"),
            "in:Ceiling:amount(off),Amount:amount|out:Amount:amount,Clipped:amount(off)",
            "H2: the DECLARATION is published beside the ports — Blender's own "
            "no_mute_links is private to the socket declaration and is set by "
            "no node type in its tree",
        )
        cap = through(tf, 6)
        assert_eq(
            cap["routes"],
            "0<-1",
            "H2: the Ceiling is off the path, so the VALUE passes — the bare "
            "identity would have passed the ceiling, since both are amounts",
        )
        assert_eq(
            cap["dropped"],
            "1",
            "H2: and Clipped carries nothing while the node is not computing, "
            "which is `node_geo_menu_switch`'s nullptr arm as a declaration",
        )
        assert_eq(cap["unreached"], "0", "H2: the Ceiling reaches no output")
        assert_eq(cap["identity"], "false", "H2: it is not the plain identity")

        # And it is the same derivation the structure uses, driven end to end.
        assert_eq(inv(tf, "connect", "2.0>6.1"), "linked", "H2: level feeds the cap")
        assert_eq(
            inv(tf, "connect", "6.0>3.2"),
            "linked, displaced 2",
            "H2: and the cap the mix, displacing the level's own wire — an "
            "input takes at most one link and the replacement is REPORTED",
        )
        assert_eq(inv(tf, "node_value", "6"), "25|0", "H2: 25 under a ceiling of 100")
        assert_eq(inv(tf, "bypass", "6"), "6=true", "H2: bypass it")
        assert_eq(
            inv(tf, "node_value", "6"),
            "25|null",
            "H2: the amount passes and Clipped is empty, exactly as declared",
        )
        assert_eq(inv(tf, "bypass", "6"), "6=false", "H2: restore")
        rewired = fields(inv(tf, "dissolve", "6"))
        assert_eq(
            rewired["bridged"],
            "2.0->3.2",
            "H2: dissolving it bridges the SAME pair the bypass routed — one "
            "derivation, so the declaration reaches the structure too",
        )
        assert_eq(q(tf, "valid"), "ok", "H2: and the document is still sound")

        # ── (I) refusals name what was not found ────────────────────────────
        refused(tf, "bypass", "99")
        refused(tf, "mute_link", "99")
        refused(tf, "dissolve", "99")
        refused(tf, "detach", "99")
        refused(tf, "passthrough", "99")
        refused(tf, "visible_ports", "99")
        assert_eq(q(tf, "nodes"), 6, "I: and no refusal changed the document")
        assert_eq(q(tf, "valid"), "ok", "I: which still satisfies its invariants")


if __name__ == "__main__":
    run_demo("r1586_bypass_is_the_identity", body)
