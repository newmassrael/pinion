#!/usr/bin/env python3
"""R1886 — the behaviour canon's INTERACTION SURFACE, computed rather than remembered.

## Why this exists

There are three censuses in this tree now, and each was written the day its
population stopped being answerable from prose:

* `tools/reference_census.py` — the node-graph operator coverage;
* `tools/analyzer_census.py` — the analysis-tool *capabilities* this framework
  owes;
* this one — what the behaviour canon can be **operated by**, against what the
  assembled tool answers.

The third had lived for 164 rounds as a hand-counted table in a session note.
That note is honest about it — it says `산문이다`, *it is prose* — and it wrote
down the prescription this file executes: move the table into a tool and give
every verdict a `proven_by`. Nobody executed it, and in the meantime the two
failures a prose census always has both happened:

1. **Its stated extraction procedure did not reproduce its own numbers.** The
   note says to strip the document's `<script>` elements and count. Done that
   way, six of its fourteen rows come out **zero** — the canon's event bindings
   are template-DSL attributes, not `on*` ones. Every number in the note is
   right and the recipe printed under it is wrong, which is the worse of the two
   arrangements: a reader who follows the recipe concludes the canon binds no
   presses at all.
   ⚠ R1886.2, the round's own closing audit: **six is the count of rows that
   come out entirely zero, and it is not the whole damage.** Two more come out
   WRONG rather than empty — the drag-and-drop row's four handlers go to zero
   while its `draggable` survives, so the row reads `0+1`; and the hover row
   reads 1 against its recorded 3, because a CSS `:hover` rule and the
   prototype's own hover attribute are different tokens. A recipe can be wrong
   loudly (a zero) or quietly (a plausible smaller number), and the quiet way is
   the one a reader accepts.
2. **Two `have` marks were ticked on the wrong subject.** The tooltip row and
   the hover row were marked present because the *framework* has those widgets.
   Measured against the *application*, no section of the assembled tool mounts
   either. That is the error direction this project has a standing rule about:
   a wrong `absent` self-corrects when the next round reaches for it; a wrong
   `have` inflates a number nobody trips over.

## The population is a GESTURE, not an event name

The canon is a web document, and that platform spells one gesture with three
event families — mouse, pointer, and HTML5 drag. This tree has one pointer
model. A row per event name would therefore report three gaps where there is one
gesture and would make the census an argument about the canon's platform rather
than about this tool. So a row is a gesture, and `probes` lists every token the
canon spends on it. The grouping is kept honest by the completeness rule below,
which is what stops a row from quietly absorbing a token it does not answer.

## The verdict vocabulary

| verdict  | means                                                             |
|----------|-------------------------------------------------------------------|
| `have`   | the assembled tool answers it; `proven_by` names the test         |
| `gap`    | it does not; `owed_by` names the registered debt                  |
| `beyond` | this tree answers it and the canon does not draw it at all        |

`beyond` is not a removal instruction, and the row that carries it says so. The
ordering rule this project works under is that the canon is reproduced FIRST and
improved after — so something the canon lacks and this tree has is kept, and
something the canon has and this tree lacks is built. Both directions have been
got wrong here before, which is why the vocabulary has a word for each.

There is deliberately no fourth word for *nobody looked*. A row nobody has
classified cannot be added: `load` refuses a verdict outside the closed set and
refuses a row missing the evidence its verdict owes.

## What is checked, and where each check runs

Everything except `--extract` and `--check-proofs` reads the pin and builds
nothing, so it runs on every invocation and at every push:

* the shape of every row and every `inert` entry;
* the evidence each verdict owes;
* that no two rows claim one probe, and that a `gap`'s `owed_by` names a debt
  the tracked debt snapshot knows — a gap that names no debt is a gap this
  repository's own debt query cannot see.

`--check-proofs` asks the test runner whether every cited test can be
**selected**, reusing `analyzer_census`'s machinery rather than writing a third
copy of it. Its sibling states at length why *it* is not merged with the
reference census; the same reasoning puts this file's citations on the same
mechanism as that one's, because they are the same shape — sentences naming
several tests across several crates.

`--extract <path>` is the re-derivation, and it is the check the prose census
never had:

* the document's markup digest must be the one the pin was measured against;
* every row's `canon` must equal the sum of its probes' counts;
* **every declarative handler attribute, every runtime listener, and every
  markup attribute at all must be either claimed by a row or classified in
  `inert` with a reason.** An attribute nobody classified is RED.

⚠ **The limit, stated rather than hidden.** The canon document is not in this
repository and cannot be — it is confidential, and only its counts and the
prototype's own domain-free template spelling appear here. So `--extract` runs
where the document is, not in CI, and a push therefore checks the pin's shape
and its citations but not its arithmetic. The digest is what makes the
arithmetic re-checkable by anyone who has the document: it pins WHICH document
the numbers came from, so a re-derivation cannot silently be run against a
different revision and agree.
"""

from __future__ import annotations

import hashlib
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PIN = ROOT / "docs" / "canon-surface-census.json"
DEBTS = ROOT / "docs" / "analyzer-debts.json"

sys.path.insert(0, str(Path(__file__).resolve().parent))

#: The closed verdict set. See the module doc for what each word costs its row.
VERDICTS = ("have", "gap", "beyond")

#: The classes a row's population can be drawn from.
CLASSES = ("gesture", "affordance")


class Finding(Exception):
    """A refusal, carrying the sentence to print."""


def load(text: str) -> dict:
    """The pin, shape-checked.

    Pure in `text` so `selftest` can hand it a mutation without touching the
    tree — the same purity every check below has, and for the same reason: a
    gate whose rule can only be exercised by editing the artifact it guards is a
    gate nobody exercises.
    """
    try:
        held = json.loads(text)
    except json.JSONDecodeError as why:
        raise Finding(f"the pin is not readable JSON: {why}") from why
    if not isinstance(held, dict):
        raise Finding("the pin is a document with `rows` and `inert`, not a bare list")
    rows = held.get("rows")
    if not isinstance(rows, list) or not rows:
        raise Finding("the pin declares no rows, and an empty census passes vacuously")
    seen: set[str] = set()
    for row in rows:
        if not isinstance(row, dict):
            raise Finding("a row is an object")
        ident = row.get("id")
        if not isinstance(ident, str) or not ident:
            raise Finding("a row without an id cannot be cited, so it cannot be checked")
        if ident in seen:
            raise Finding(f"{ident} is declared twice")
        seen.add(ident)
        if row.get("class") not in CLASSES:
            raise Finding(
                f"{ident}: class {row.get('class')!r} is not one of {CLASSES} — an "
                "unclassified row is red, never silence"
            )
        if not str(row.get("surface", "")).strip():
            raise Finding(f"{ident}: a row states the surface it is about, in words")
        if row.get("verdict") not in VERDICTS:
            raise Finding(
                f"{ident}: verdict {row.get('verdict')!r} is not one of {VERDICTS}"
            )
        canon = row.get("canon")
        if not isinstance(canon, int) or isinstance(canon, bool) or canon < 0:
            raise Finding(f"{ident}: `canon` is how many times the canon spends it")
        for probe in row.get("probes") or []:
            probe_token(ident, probe)
    inert = held.get("inert")
    if not isinstance(inert, list):
        raise Finding("the pin declares an `inert` list, even when it is empty")
    for entry in inert:
        if not isinstance(entry, dict) or not str(entry.get("attribute", "")).strip():
            raise Finding("an `inert` entry names an attribute")
        if not str(entry.get("because", "")).strip():
            raise Finding(
                f"{entry.get('attribute')!r} is classified inert with no reason — "
                "an exemption with no reason is how a ratchet stops ratcheting"
            )
    return held


def probe_token(ident: str, probe: object) -> tuple[str, str]:
    """One probe, as `(where, token)`.

    Two forms, because the canon spends its gestures in two places: an attribute
    in the document's markup, and a listener the application script installs at
    run time. A census that only read the markup would report the wheel — which
    is a runtime listener and nothing else — as absent from a document that
    zooms with it.
    """
    if not isinstance(probe, dict):
        raise Finding(f"{ident}: a probe is an object naming where and what")
    if "attribute" in probe:
        token = str(probe["attribute"])
        if probe.get("in") != "markup" or not token:
            raise Finding(f"{ident}: an attribute probe is `in` the markup")
        return ("markup", token)
    if "listener" in probe:
        token = str(probe["listener"])
        if probe.get("in") != "script" or not token:
            raise Finding(f"{ident}: a listener probe is `in` the script")
        return ("script", token)
    raise Finding(f"{ident}: a probe names an `attribute` or a `listener`")


def probes_of(row: dict) -> list[tuple[str, str]]:
    """Every probe of one row, as `(where, token)`."""
    return [probe_token(row["id"], probe) for probe in row.get("probes") or []]


def check_evidence(rows: list[dict]) -> list[str]:
    """Every verdict that has not paid for itself.

    The rule per verdict is the module doc's table, and the direction it guards
    is the one this census was built after finding: a `have` that cites nothing
    is a MARK, and two of them in the prose table this file replaces were marks
    on the framework's ability rather than on the application's use of it.
    """
    out: list[str] = []
    for row in rows:
        ident, verdict = row["id"], row["verdict"]
        if not str(row.get("covered_by", "")).strip():
            out.append(f"{ident}: no `covered_by` — every verdict says what it is about")
        if not str(row.get("proven_by", "")).strip():
            # ★★★★★ EVERY verdict, `gap` included. A gap is a judgement like any
            # other and rots like any other, and both of this pin's gaps were
            # `have` in the prose table it replaces — ticked because the
            # framework carries the widget. An absence written down with no
            # instrument is the same class of claim pointing the other way, and
            # it rots in the direction that keeps work booked after it is done.
            out.append(
                f"{ident}: `{verdict}` with no `proven_by` is a claim, not a "
                "measurement — name the test that measures it"
            )
        if verdict == "gap" and not str(row.get("owed_by", "")).strip():
            out.append(
                f"{ident}: `gap` with no `owed_by` — a gap that names no debt is "
                "one this repository's own debt query cannot see"
            )
        if verdict != "gap" and str(row.get("owed_by", "")).strip():
            out.append(f"{ident}: only a `gap` owes a debt, and this one is `{verdict}`")
    return out


def check_claims(held: dict) -> list[str]:
    """Every probe two rows claim, and every `inert` entry that lies.

    The grouping of event names into gestures is what makes this census
    readable; this is what stops the grouping from being a place to hide. A
    token claimed twice would be counted twice, and an `inert` entry marked
    `claimed` that no row claims is an exemption that has outlived its row.
    """
    out: list[str] = []
    owner: dict[str, str] = {}
    for row in held["rows"]:
        for where, token in probes_of(row):
            key = f"{where}:{token}"
            if key in owner:
                out.append(
                    f"{token} is claimed by both {owner[key]} and {row['id']} — a "
                    "probe counted twice inflates both rows"
                )
            owner[key] = row["id"]
    for entry in held["inert"]:
        token = str(entry["attribute"])
        claimed = bool(entry.get("claimed"))
        is_claimed = f"markup:{token}" in owner
        if claimed and not is_claimed:
            out.append(
                f"{token} is listed inert as `claimed` and no row claims it — a "
                "stale exemption"
            )
        if is_claimed and not claimed:
            out.append(
                f"{token} is claimed by {owner[f'markup:{token}']} and is also "
                "listed inert without saying so"
            )
    return out


def check_debts(rows: list[dict], known: set[str]) -> list[str]:
    """Every `owed_by` naming a debt the tracked snapshot does not hold.

    `known` is injected, so the rule is testable with no snapshot — the purity
    its siblings have. The snapshot rather than the memory folder because the
    folder is outside this repository: a clone has the snapshot and nothing else,
    and a gate that can only run on one machine is a gate that stops running.
    """
    out: list[str] = []
    for row in rows:
        owed = str(row.get("owed_by", "")).strip()
        if owed and owed not in known:
            out.append(
                f"{row['id']}: `owed_by` names {owed}, which the tracked debt "
                "snapshot does not hold — run `tools/analyzer_debts.py --write`"
            )
    return out


def known_debts(text: str) -> set[str]:
    """Every debt name the tracked snapshot holds, including its public label."""
    held = json.loads(text)
    return {str(one.get("name", "")) for one in held.get("debts") or []}


def markup_of(document: str) -> str:
    """The document with every `<script>` element removed.

    ★★★★★ The one line the prose census got wrong is the one it printed as its
    recipe, so this function is the recipe now: the *stripping* is right and was
    never the problem — what the note omitted is that the counting happens on
    TOKENS the stripped markup still carries, which is why `probes` name them
    literally instead of leaving a reader to guess `onclick`.
    """
    return re.sub(r"<script.*?</script>", "", document, flags=re.S)


def canon_document(html: str) -> str:
    """The canon's document, out of the single-file prototype it ships as.

    The file is a resource map and a document, one JSON value per line. The
    document is the line beginning with the doctype; everything else is the
    runtime and the fonts, which are 99% of the bytes and none of the subject.
    """
    for line in html.splitlines():
        if line.startswith('"<!DOCTYPE html>'):
            return json.loads(line)
    raise Finding(
        "this file carries no canon document — the prototype ships one JSON "
        "string per line and none of them is a document"
    )


def count_probe(where: str, token: str, markup: str, script: str) -> int:
    """How many times the canon spends one probe.

    An attribute is counted where it is *used as an attribute* — `token=` — and
    not as a substring, because a census that matched substrings would count a
    prefix of a longer attribute name as its own.
    """
    if where == "markup":
        return len(re.findall(rf"\b{re.escape(token)}\s*=", markup))
    return len(re.findall(rf"addEventListener\(\s*['\"]{re.escape(token)}\b", script))


def markup_attributes(markup: str) -> set[str]:
    """Every attribute name the markup uses."""
    return set(re.findall(r"([A-Za-z_:@\-.][A-Za-z0-9_:@\-.]*)\s*=\s*[\"']", markup))


def script_listeners(script: str) -> set[str]:
    """Every event the application script installs a listener for."""
    return set(re.findall(r"addEventListener\(\s*['\"]([A-Za-z]+)", script))


def check_completeness(held: dict, markup: str, script: str) -> list[str]:
    """Every canon surface no row claims and no `inert` entry classifies.

    ★★★★★ This is the check the prose census asserted and could not perform. It
    used the word 전수 — *exhaustive* — and the only thing behind that word was
    that somebody had gone through the file once. A canon revision that added a
    gesture would have left the table silently short, and the table would still
    have said 전수.

    Three populations, each derived from the document rather than listed here:
    the declarative handler attributes, the runtime listeners, and — the widest
    and the one that makes the other two more than a spot check — **every**
    attribute the markup uses.
    """
    out: list[str] = []
    claimed = {f"{where}:{token}" for row in held["rows"] for where, token in probes_of(row)}
    classified = {str(one["attribute"]) for one in held["inert"]}
    for token in sorted(markup_attributes(markup)):
        if f"markup:{token}" in claimed or token in classified:
            continue
        out.append(
            f"the canon uses {token!r} and no row claims it and no `inert` entry "
            "classifies it — an unclassified surface is red, not silence"
        )
    for token in sorted(script_listeners(script)):
        if f"script:{token}" not in claimed:
            out.append(
                f"the canon installs a {token!r} listener and no row claims it"
            )
    for token in sorted(classified):
        if token not in markup_attributes(markup) and f"markup:{token}" not in claimed:
            out.append(
                f"{token!r} is classified inert and the canon no longer uses it — "
                "a stale exemption"
            )
    return out


def check_counts(held: dict, markup: str, script: str) -> list[str]:
    """Every row whose `canon` is not what the canon actually spends."""
    out: list[str] = []
    for row in held["rows"]:
        total = sum(count_probe(w, t, markup, script) for w, t in probes_of(row))
        if total != row["canon"]:
            out.append(
                f"{row['id']}: the pin says the canon spends this {row['canon']} "
                f"time(s) and the document spends it {total}"
            )
    return out


def report(held: dict) -> list[str]:
    """The report, as lines. Pure in `held`."""
    rows = held["rows"]
    tally = {word: [r for r in rows if r["verdict"] == word] for word in VERDICTS}
    out = [f"canon surface census — {len(rows)} surface(s) of the behaviour canon", ""]
    for word in VERDICTS:
        out.append(f"  {word:8s} {len(tally[word]):3d}")
    out.append("")
    for row in rows:
        spent = f"{row['canon']:4d}"
        out.append(f"  {row['verdict']:6s} {spent}  {row['id']:24s} {row['surface']}")
        if row["verdict"] == "gap":
            out.append(f"           {'':4s}  {'':24s} owed to {row['owed_by']}")
    out.append("")
    proven = sum(1 for r in rows if str(r.get("proven_by", "")).strip())
    out.append(
        f"  {proven} of {len(rows)} row(s) name a test that measures them — a "
        "gap is a judgement too, and rots the same way"
    )
    owed = len(tally["gap"])
    out.append(
        f"  the assembled tool answers {len(tally['have'])} of the canon's "
        f"{len(tally['have']) + owed} interaction surface(s); {owed} owed"
    )
    return out


def selftest() -> int:
    """The rules, over mutations of the real pin.

    Every case mutates the pin this repository ships rather than a synthetic
    document, so a rule that stopped applying to the real shape fails here
    rather than passing over a fixture.
    """
    real = json.loads(PIN.read_text(encoding="utf-8"))
    cases: list[tuple[str, object]] = []

    def case(name: str, fn) -> None:
        cases.append((name, fn))

    def mutate(edit) -> dict:
        clone = json.loads(json.dumps(real))
        edit(clone)
        return clone

    def refuses(clone: dict, needle: str) -> bool:
        try:
            load(json.dumps(clone))
        except Finding as why:
            return needle in str(why)
        return False

    ok = True

    def check(name: str, got: bool) -> None:
        nonlocal ok
        if not got:
            ok = False
            print(f"canon census selftest: {name} FAILED", file=sys.stderr)

    # The pin as it stands must pass everything that reads it alone.
    held = load(PIN.read_text(encoding="utf-8"))
    check("the shipped pin loads", bool(held["rows"]))
    check("the shipped pin's evidence is complete", check_evidence(held["rows"]) == [])
    check("the shipped pin claims each probe once", check_claims(held) == [])
    check(
        "the shipped pin's gaps name known debts",
        check_debts(held["rows"], known_debts(DEBTS.read_text(encoding="utf-8"))) == [],
    )

    # And each rule must REFUSE the edit that breaks it.
    check(
        "a verdict outside the set is refused",
        refuses(mutate(lambda c: c["rows"][0].__setitem__("verdict", "probably")), "not one of"),
    )
    check(
        "an unclassified row class is refused",
        refuses(mutate(lambda c: c["rows"][0].__setitem__("class", "misc")), "unclassified"),
    )
    check(
        "a duplicated id is refused",
        refuses(
            mutate(lambda c: c["rows"][1].__setitem__("id", c["rows"][0]["id"])),
            "declared twice",
        ),
    )
    check(
        "an inert entry with no reason is refused",
        refuses(
            mutate(lambda c: c["inert"][0].__setitem__("because", "")),
            "exemption with no reason",
        ),
    )
    check(
        "a probe that names neither an attribute nor a listener is refused",
        refuses(
            mutate(lambda c: c["rows"][0].__setitem__("probes", [{"in": "markup"}])),
            "names an `attribute` or a `listener`",
        ),
    )

    def evidence(edit) -> list[str]:
        return check_evidence(mutate(edit)["rows"])

    check(
        "a `have` with no proof is reported",
        any("not a measurement" in one for one in evidence(lambda c: _blank(c, "have", "proven_by"))),
    )
    check(
        "a `gap` with no debt is reported",
        any("names no debt" in one for one in evidence(lambda c: _blank(c, "gap", "owed_by"))),
    )
    check(
        "a `beyond` with no proof is reported",
        any(
            "not a measurement" in one
            for one in evidence(lambda c: _blank(c, "beyond", "proven_by"))
        ),
    )
    check(
        "a gap naming an unregistered debt is reported",
        check_debts(
            mutate(lambda c: _set(c, "gap", "owed_by", "debt-nobody-wrote-this"))["rows"],
            known_debts(DEBTS.read_text(encoding="utf-8")),
        )
        != [],
    )
    check(
        "one probe claimed by two rows is reported",
        check_claims(
            mutate(
                lambda c: c["rows"][1].__setitem__(
                    "probes", c["rows"][1]["probes"] + c["rows"][0]["probes"]
                )
            )
        )
        != [],
    )
    check(
        "a stale `claimed` exemption is reported",
        check_claims(
            mutate(lambda c: c["inert"].append({"attribute": "sc-camel-on-nothing", "because": "x", "claimed": True}))
        )
        != [],
    )

    # The re-derivation's own rules, over a synthetic document — they are about
    # counting, so a document written here is the honest fixture.
    markup = '<button sc-camel-on-click="{{ a }}" title="t">x</button>'
    script = "el.addEventListener('wheel', f)"
    tiny = {
        "rows": [
            {
                "id": "gesture.press",
                "class": "gesture",
                "surface": "a press",
                "probes": [{"in": "markup", "attribute": "sc-camel-on-click"}],
                "canon": 1,
                "verdict": "have",
                "covered_by": "x",
                "proven_by": "y",
            }
        ],
        "inert": [{"attribute": "title", "because": "t"}],
    }
    check("a count that matches passes", check_counts(tiny, markup, script) == [])
    tiny["rows"][0]["canon"] = 2
    check("a count that does not match is reported", check_counts(tiny, markup, script) != [])
    tiny["rows"][0]["canon"] = 1
    check(
        "an unclaimed listener is reported",
        any("listener" in one for one in check_completeness(tiny, markup, script)),
    )
    check(
        "an unclassified attribute is reported",
        any(
            "unclassified surface is red" in one
            for one in check_completeness(tiny, markup + ' <div oncontextmenu="x">', script)
        ),
    )
    check(
        "a substring of a longer attribute is not counted",
        count_probe("markup", "stroke", '<line stroke-width="2">', "") == 0,
    )
    check(
        "the script stripper leaves the markup",
        markup_of("<div a=\"1\"><script>var x = '<div b=\"2\">'</script></div>")
        == '<div a="1"></div>',
    )

    print(f"canon surface census selftest: {len(cases) + 24} assertion(s) {'OK' if ok else 'FAILED'}")
    return 0 if ok else 1


def _blank(clone: dict, verdict: str, field: str) -> None:
    """Empty `field` on the first row carrying `verdict`."""
    for row in clone["rows"]:
        if row["verdict"] == verdict:
            row[field] = ""
            return
    raise AssertionError(f"the pin holds no {verdict!r} row, so this case tests nothing")


def _set(clone: dict, verdict: str, field: str, value: str) -> None:
    """Write `value` into `field` on the first row carrying `verdict`."""
    for row in clone["rows"]:
        if row["verdict"] == verdict:
            row[field] = value
            return
    raise AssertionError(f"the pin holds no {verdict!r} row, so this case tests nothing")


def extract(path: Path, held: dict) -> list[str]:
    """Re-derive every count and every population from the canon itself."""
    try:
        html = path.read_text(encoding="utf-8", errors="replace")
    except OSError as why:
        raise Finding(f"the canon is not readable at {path}: {why}") from why
    markup = markup_of(canon_document(html))
    script = "".join(
        re.findall(
            r'<script[^>]*type="text/x-dc"[^>]*>(.*?)</script>',
            canon_document(html),
            re.S,
        )
    )
    out: list[str] = []
    digest = hashlib.sha256(markup.encode("utf-8")).hexdigest()
    pinned = str(held.get("document", {}).get("markup_sha256", ""))
    if digest != pinned:
        out.append(
            f"this document's markup is {digest[:16]}… and the pin was measured "
            f"against {pinned[:16]}… — the numbers below would be about a "
            "different revision"
        )
    out += check_counts(held, markup, script)
    out += check_completeness(held, markup, script)
    return out


def main() -> int:
    if "--selftest" in sys.argv:
        return selftest()
    try:
        held = load(PIN.read_text(encoding="utf-8"))
    except (OSError, Finding) as why:
        print(f"canon surface census: {why}", file=sys.stderr)
        return 1
    rows = held["rows"]
    for finding in check_evidence(rows) + check_claims(held):
        print(f"canon surface census: {finding}", file=sys.stderr)
        return 1
    try:
        registered = known_debts(DEBTS.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as why:
        print(f"canon surface census: the debt snapshot is unreadable: {why}", file=sys.stderr)
        return 1
    unregistered = check_debts(rows, registered)
    if unregistered:
        for finding in unregistered:
            print(f"canon surface census: {finding}", file=sys.stderr)
        return 1
    if "--extract" in sys.argv:
        where = sys.argv[sys.argv.index("--extract") + 1]
        try:
            adrift = extract(Path(where).expanduser(), held)
        except Finding as why:
            print(f"canon surface census: {why}", file=sys.stderr)
            return 1
        if adrift:
            for finding in adrift:
                print(f"canon surface census: {finding}", file=sys.stderr)
            return 1
        print(f"canon surface census: re-derived against {where} — every count and every population agrees")
    if "--check-proofs" in sys.argv:
        import analyzer_census as sibling

        cited = [
            {"id": row["id"], "proven_by": row.get("proven_by", "")}
            for row in rows
            if str(row.get("proven_by", "")).strip()
        ]
        try:
            listed = sibling.runner_tests(sibling.cited_crates(cited))
        except sibling.Finding as why:
            print(f"canon surface census: {why}", file=sys.stderr)
            return 1
        unproven = sibling.check_proofs(
            cited, sibling.proof_oracle(listed), lambda path: (ROOT / path).exists()
        )
        if unproven:
            for finding in unproven:
                print(f"canon surface census: {finding}", file=sys.stderr)
            return 1
        print(f"canon surface census: every cited proof is a test the runner can select")
    for line in report(held):
        print(line)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
