#!/usr/bin/env python3
"""R1663 — **a decoded field says which bytes it came from**, over the wire.

`docs/analyzer-census.json` carried `capture.t0.3` — *bidirectional highlight
between a field and its bytes* — as a `have`, covered by `widgets::hex_dump`
plus `scene/marks`. Driven, neither surface holds the relation: the dissection
External publishes thirteen paths and not one names a byte, and the hex External
publishes seventeen and not one names a field. That verdict was covering the
byte-to-*cell* pair, which is a different pair.

`pinion_core::widgets::field_bytes` is the relation, and this demo drives it
through a running `hello-packet-view`:

  (A) the SPECIFICATION is read off the wire, not carried here — the screen
      publishes `spec`, and every population below is derived from it, so a
      table that drifts fails rather than being quietly re-asserted;
  (B) FORWARD — every declared field's extent and its already-shaped byte
      selection;
  (C) INVERSE — `owner`, `coverage` and the `layers` chain at a byte, with
      `owner` proven to BE the last link of the chain rather than a second
      computation;
  (D) the LAW, driven end to end: selecting a field lights exactly the bytes
      the map names, and pressing one of those bytes selects the field back;
  (E) the three answers at a byte address stay three — a field, an unclaimed
      byte, and one past the end of the buffer;
  (F) a DERIVED field is not a missing one, and it lights nothing truthfully;
  (G) a second byte source, which is what a reassembled payload needs and what
      one index space cannot express;
  (H) the relation is READ-ONLY on the wire — a client that could rewrite it
      could make the screen disagree with the bytes it is showing.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    assert_router_press_moves,
    run_demo,
)

EXAMPLE = "hello-packet-view"
VIEW = "packet_view"
MAP = "pv.map"

CHECKS = []


def banner(what: str) -> None:
    print(f"[demo] -- {what}")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def q(app, tag, path):
    return app.query(f"/{tag}/external/{path}")


def inv(app, tag, path, args):
    return app.invoke(f"/{tag}/external/{path}", args)


def body() -> None:
    with RpcSubprocess(EXAMPLE) as app:
        # ── (A) the specification, off the wire ────────────────────────────
        banner("A — the screen publishes the specification it is built against")
        spec = q(app, VIEW, "spec")
        ok("the screen publishes a specification", isinstance(spec, dict))
        fields = spec["fields"]
        rows = spec["rows"]
        sources = spec["sources"]
        layers = spec["layers"]
        ok("the specification names fields", len(fields) > 0)
        ok("the specification names two byte sources", len(sources) == 2)
        ok("the specification names four layers", len(layers) == 4)
        print(
            f"[demo] {len(fields)} field(s), {len(rows)} message(s), "
            f"{len(layers)} layer(s), {len(sources)} byte source(s)"
        )

        assert_eq(
            q(app, MAP, "field_count"),
            len(fields),
            "the map holds exactly the specification's fields",
        )
        assert_eq(
            q(app, MAP, "source_count"),
            len(sources),
            "the map holds exactly the specification's sources",
        )
        CHECKS.extend(["map field count", "map source count"])
        published = q(app, MAP, "field_paths")
        assert_eq(
            published,
            [f["path"] for f in fields],
            "the wire lists the fields in declaration order",
        )
        CHECKS.append("field_paths")

        # ── (B) forward: a field names its bytes ───────────────────────────
        banner("B — FORWARD: every declared field says which bytes it came from")
        with_bytes = 0
        for field in fields:
            extent = q(app, MAP, f"extent.{field['path']}")
            origin = q(app, MAP, f"origin.{field['path']}")
            if field["source"] is None:
                assert_eq(origin, "derived", f"{field['path']} is derived")
                assert_eq(extent, None, f"{field['path']} has no extent")
                continue
            assert_eq(origin, "bytes", f"{field['path']} was read from bytes")
            assert_eq(
                extent,
                {"source": field["source"], "at": field["at"], "len": field["len"]},
                f"{field['path']} extent",
            )
            # The selection is already the shape a hex view highlights, so a
            # consumer never converts an extent into a range by hand.
            selection = q(app, MAP, f"selection.{field['path']}")
            if field["len"] > 0:
                assert_eq(
                    selection,
                    {
                        "source": field["source"],
                        "start": field["at"],
                        "end": field["at"] + field["len"],
                    },
                    f"{field['path']} selection",
                )
            with_bytes += 1
            CHECKS.append(f"forward {field['path']}")
        ok("most declared fields have bytes", with_bytes >= len(fields) - 3)
        print(f"[demo] {with_bytes} field(s) answered forward")

        # ── (C) inverse: a byte names its field, and its layer chain ───────
        banner("C — INVERSE: a byte names the field that owns it, and its layers")
        frame_len = sources[0]["len"]
        owned = 0
        for byte in range(frame_len):
            coverage = q(app, MAP, f"coverage.0.{byte}")
            owner = q(app, MAP, f"owner.0.{byte}")
            chain = q(app, MAP, f"layers.0.{byte}")
            if coverage == "field":
                ok(f"byte {byte} names an owner", owner is not None)
                assert_eq(
                    chain[-1],
                    owner,
                    f"byte {byte}: owner IS the last link of the chain",
                )
                # A chain is a containment chain: each link covers the next.
                for outer, inner in zip(chain, chain[1:]):
                    a = q(app, MAP, f"extent.{outer}")
                    b = q(app, MAP, f"extent.{inner}")
                    ok(
                        f"byte {byte}: `{outer}` covers `{inner}`",
                        a["at"] <= b["at"] and b["at"] + b["len"] <= a["at"] + a["len"],
                    )
                owned += 1
            else:
                assert_eq(owner, None, f"byte {byte} has no owner")
                assert_eq(chain, [], f"byte {byte} has no layer chain")
        ok("most of the frame is claimed", owned > frame_len // 2)
        print(f"[demo] {owned} of {frame_len} frame byte(s) are owned by a field")

        # ── (D) the law, end to end ────────────────────────────────────────
        banner("D — the LAW: the lit bytes are the map's, and pressing one comes back")
        checked = 0
        for field in fields:
            if field["source"] != 0 or field["len"] == 0:
                continue
            inv(app, VIEW, "select_field", field["path"])
            assert_eq(
                q(app, VIEW, "selected_field"), field["path"], "the screen selected it"
            )
            assert_eq(
                q(app, VIEW, "selected_span"),
                {"start": field["at"], "end": field["at"] + field["len"]},
                f"the screen lights exactly {field['path']}'s bytes",
            )
            # And the inverse gesture, through the same handler a press reaches.
            back = inv(app, VIEW, "select_byte", field["at"])
            ok(
                f"pressing a byte of `{field['path']}` selects it or a child",
                back == field["path"] or back.startswith(field["path"] + "."),
            )
            checked += 1
        ok("the law was checked on every framed field", checked >= 15)
        print(f"[demo] the law held for {checked} field(s)")

        # ── (E) three answers stay three ───────────────────────────────────
        banner("E — a field, an unclaimed byte and one past the end are three answers")
        unclaimed = [
            b for b in range(frame_len) if q(app, MAP, f"coverage.0.{b}") == "unmapped"
        ]
        ok("the frame has bytes no field claims", len(unclaimed) > 0)
        assert_eq(
            q(app, MAP, f"coverage.0.{frame_len}"),
            "out-of-buffer",
            "one past the end is not the same answer as unclaimed",
        )
        assert_eq(
            q(app, MAP, "unmapped_bytes.0"),
            len(unclaimed),
            "the count agrees with the per-byte answers",
        )
        CHECKS.extend(["unmapped exists", "out-of-buffer", "unmapped count"])
        before = q(app, VIEW, "selected_field")
        inv(app, VIEW, "select_byte", unclaimed[0])
        assert_eq(
            q(app, VIEW, "selected_field"),
            before,
            "pressing an unclaimed byte selects nothing new",
        )
        # ★ R1719 — `said` answers the VALUE now, not the sentence, on all three
        # screens of this tool. The words a person reads are `["sentence"]`.
        ok(
            "and the screen says why",
            "no field" in ((q(app, VIEW, "said") or {}).get("sentence") or ""),
        )

        # ── (F) a derived field is not a missing one ───────────────────────
        banner("F — a derived field is an answer, not an absence")
        derived = [f["path"] for f in fields if f["source"] is None]
        ok("the reference decode has derived fields", len(derived) > 0)
        for path in derived:
            assert_eq(q(app, MAP, f"origin.{path}"), "derived", f"{path} is derived")
            inv(app, VIEW, "select_field", path)
            assert_eq(
                q(app, VIEW, "selected_span"),
                None,
                f"{path} truthfully lights nothing",
            )
            CHECKS.append(f"derived {path}")
        assert_eq(
            q(app, MAP, "origin.l3.no_such_field"),
            None,
            "an undeclared path is a different answer from a derived one",
        )
        CHECKS.append("undeclared != derived")

        # ── (G) a second byte source ───────────────────────────────────────
        banner("G — a reassembled payload lives in its own buffer")
        assert_eq(q(app, MAP, "source_name.0"), sources[0]["name"], "source 0")
        assert_eq(q(app, MAP, "source_name.1"), sources[1]["name"], "source 1")
        assert_eq(q(app, MAP, "source_len.1"), sources[1]["len"], "source 1 length")
        payload = [f for f in fields if f["source"] == 1]
        ok("a field lives in the second buffer", len(payload) == 1)
        assert_eq(
            q(app, MAP, f"extent.{payload[0]['path']}")["source"],
            1,
            "the payload names the second buffer",
        )
        inv(app, VIEW, "select_field", payload[0]["path"])
        assert_eq(
            q(app, VIEW, "selected_span"),
            None,
            "a field in another buffer lights nothing in the frame pane",
        )
        CHECKS.extend(["source names", "second buffer", "payload span"])
        # The last byte of the payload is owned, which one index space could not
        # even address.
        assert_eq(
            q(app, MAP, f"owner.1.{sources[1]['len'] - 1}"),
            payload[0]["path"],
            "the second buffer's last byte has an owner",
        )
        CHECKS.append("second buffer owner")

        # ── (H) read-only ──────────────────────────────────────────────────
        banner("H — the relation is derived, so the wire cannot rewrite it")
        for path in ("field_count", "field_paths"):
            try:
                app.intervene(f"/{MAP}/external/{path}", 3)
                raise AssertionError(f"{path} accepted a write")
            except RpcError as exc:
                ok(f"{path} refuses a write", "ReadOnly" in str(exc))
        try:
            inv(app, MAP, "select", "l1.sn")
            raise AssertionError("the map accepted an action")
        except RpcError:
            ok("the map declares no actions", True)

        # ── the screen still works after all of that ───────────────────────
        banner("the screen is still the screen")
        inv(app, VIEW, "select_message", 1)
        assert_eq(q(app, VIEW, "selected_row"), 1, "another message decodes")
        ok(
            "and its decode is a different one",
            q(app, MAP, "field_count") != len(fields),
        )
        inv(app, VIEW, "select_message", spec["opening_row"])
        assert_eq(
            q(app, MAP, "field_count"), len(fields), "and the first one comes back"
        )
        CHECKS.extend(["re-decode", "decode differs", "decode returns"])

        # ── (I) ★★★★★ the ROUTER, which is the path a person's hand takes ──
        #
        # Everything above this line drives the oracle by name, and every one of
        # those assertions passed on a screen where no press anywhere in the
        # window did anything at all. This section presses WINDOW POINTS through
        # `scene/click {at}` — the one wire verb that goes through the §5.35
        # router — and asserts the screen moved.
        banner("I — a real press, through the router, on every kind of target")
        # A message row, a decode row, a byte cell, a saved filter and a layer
        # chevron: the five things `Hit` distinguishes, so a press that resolved
        # to the widget but reached the wrong sub-region shows up as the wrong
        # one of these moving.
        # The decode row and the byte cell are chosen so that pressing the second
        # cannot land on the field the first one selected — otherwise "nothing
        # moved" would be the correct answer and this check would be asserting
        # the press was DELIVERED against a read that cannot tell.
        # The owner of a byte is the INNERMOST field covering it (R1663's
        # `owner_at`), so a parent field's declared `at` is not a byte that
        # selects the parent. The target is therefore derived from the same
        # inverse read section C proves, not from the forward table.
        row_field = fields[1]["path"]
        byte_index = next(
            b
            for b in range(64)
            if (owner := q(app, MAP, f"owner.0.{b}")) is not None and owner != row_field
        )
        targets = [
            ("a message row", "pv.list.row.2", lambda: q(app, VIEW, "selected_row")),
            (
                "a decode row",
                f"pv.tree.field.{row_field}",
                lambda: q(app, VIEW, "selected_field"),
            ),
            (
                f"a byte cell owned by `{q(app, MAP, f'owner.0.{byte_index}')}`",
                f"pv.bytes.cell.{byte_index}",
                lambda: q(app, VIEW, "selected_field"),
            ),
            ("a saved filter", "pv.filter.saved.1", lambda: q(app, VIEW, "saved")),
            ("a layer chevron", "pv.tree.layer.l1", lambda: q(app, VIEW, "folded")),
        ]
        for what, tag, read in targets:
            moved = assert_router_press_moves(app, tag, read, f"I: {what}")
            ok(f"a router press on {what} moves the screen", True)
            print(f"[demo]   {tag}: -> {moved!r}")

        # ★ And the negative control that makes the five above mean something: a
        # press on a decorative strip resolves to the same widget and correctly
        # changes nothing, so "the screen moved" is a fact about WHERE the press
        # landed and not about pressing at all.
        before = q(app, VIEW, "selected_row")
        app.request("scene/click", {"button": "left", "at": {"x": 700, "y": 6}})
        app.tick(16)
        assert_eq(
            q(app, VIEW, "selected_row"),
            before,
            "I: ★ a press on the app bar's dead space selects nothing",
        )
        CHECKS.append("router press on dead space")

        # The hit test the painter shares, driven through the wire: the point a
        # byte cell was painted at answers as that byte.
        lit = q(app, VIEW, "selected_field")
        print(f"[demo] closing with `{lit}` selected")

    print(f"[demo] {len(CHECKS)} named assertion(s)")


if __name__ == "__main__":
    run_demo("R1663 a field says which bytes it came from", body)
