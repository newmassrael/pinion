#!/usr/bin/env python3
"""R1611 — rewrite reference-project names out of comments, evidence intact.

`tools/reference_names.py` says how many there are. This says what to do with
one, and it exists because the recorded prescription did not survive contact
with the measurement: R1610 wrote "restate every sentence by hand, and put the
source in a memory note", and the census then found **7,999 occurrences across
501 files**. Hand-restating 7,999 sentences is not a plan, it is a wish.

## What is actually being removed

Almost every occurrence is one of two things:

* the **product**, used as an actor -- "Qt keys sections by index";
* a **class name**, used as a noun -- "`QHeaderView` persists an opaque blob".

and a class name in that toolkit is its own generic noun with a letter in
front. `QHeaderView` IS "the header view". So the substitution is *derived* from
the token rather than invented per site: strip the prefix, split the camel case,
lowercase it. The sentence keeps saying exactly what it said -- which capability
the reference has, and how ours differs -- and stops naming the vendor.

That is why this is not the "mechanical replacement" the debt warned against.
The warning was about **losing the evidence**, and the evidence is the
capability claim, which survives whole. What is lost is the ability to look the
symbol up, and that is what the round's memory note is for.

## What it will not touch

Comment lines only. A name inside a string literal may be load-bearing for an
assertion, and a name inside an identifier is an API change; both are reported
for a human and left alone. Running this does not finish a file -- the census
tool says whether it did.

Usage:
    python3 tools/reference_names_migrate.py --dry-run crates/pinion-core
    python3 tools/reference_names_migrate.py --apply crates/pinion-core
    python3 tools/reference_names_migrate.py --selftest
"""

from __future__ import annotations

import argparse
import re
import subprocess
import textwrap
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# --- how each product is named once its own name is gone --------------------
#
# Two forms, because English needs both: the phrase that stands alone at the
# head of a clause ("the toolkit keys sections by index") and the bare noun that
# follows an article already in the sentence ("a toolkit view"). A single
# replacement gets one of the two wrong every time, which is what the first
# draft of this tool did and why it was thrown away.
PRODUCT_PHRASE: dict[str, tuple[str, str]] = {
    "qt": ("the toolkit", "toolkit"),
    "qtbase": ("the toolkit's widget module", "toolkit widget module"),
    "qtcharts": ("the toolkit's charting module", "toolkit charting module"),
    "qtwidgets": ("the toolkit's widget module", "toolkit widget module"),
    "qtquick": ("the toolkit's declarative module", "toolkit declarative module"),
    "qml": ("the toolkit's declarative language", "toolkit declarative language"),
    "blender": ("the DCC", "DCC"),
    "unreal": ("the engine", "engine"),
    "unrealengine": ("the engine", "engine"),
    "grafana": ("the dashboard tool", "dashboard tool"),
    "wireshark": ("the analyser", "analyser"),
    "figma": ("the design tool", "design tool"),
    "flutter": ("another retained-mode toolkit", "retained-mode toolkit"),
    "godot": ("another engine", "engine"),
    "vscode": ("the code editor", "code editor"),
    "jetbrains": ("the IDE vendor", "IDE vendor"),
    "photoshop": ("the raster editor", "raster editor"),
    "houdini": ("another procedural DCC", "procedural DCC"),
    "maya": ("another DCC", "DCC"),
    "chromium": ("an embedded browser engine", "embedded browser engine"),
    "qcustomplot": ("a third-party charting library", "third-party charting library"),
    "kicad": ("the EDA tool", "EDA tool"),
    "audacity": ("the audio editor", "audio editor"),
    "ableton": ("another audio workstation", "audio workstation"),
}

# `(?<![\w-])` rather than `\b` so a hyphenated compound the sentence already
# owns ("non-Qt") is left for a human instead of half-rewritten.
PRODUCT_RE = re.compile(
    r"(?P<article>\b(?:a|an|the|A|An|The)\s+)?"
    r"(?P<name>\b(?:" + "|".join(sorted(PRODUCT_PHRASE, key=len, reverse=True))
    + r")\b)(?!::)",
    re.IGNORECASE,
)

# Toolkit class names: `QHeaderView` -> `header view`, backticks and all,
# because a generic English noun in code font reads as a symbol that does not
# exist. A name followed by `::` is a symbol PATH and is left alone -- there is
# no derivation from `QHeaderView::saveState()` to prose, only a rewrite, and
# that is a human's sentence to write.
CLASS_RE = re.compile(
    r"(?P<tick>`?)(?<![:\w])Q(?P<rest>[A-Z][A-Za-z0-9]*[a-z][A-Za-z0-9]*)\b"
    r"(?!::)(?P<tail>`?)"
)

# A handful of class names whose de-camel-cased form would read wrong or lose
# the point. Each is the noun a reader needs, not a paraphrase of the sentence.
CLASS_OVERRIDE: dict[str, str] = {
    "QWidget": "widget",
    "QObject": "object",
    "QVariant": "dynamic value",
    "QByteArray": "byte array",
    "QString": "string",
    "QAbstractItemModel": "abstract item model",
    "QAbstractItemView": "abstract item view",
    "QMdiSubWindow": "MDI child window",
    "QMdiArea": "MDI area",
    "QGraphicsView": "canvas view",
    "QGraphicsScene": "canvas scene",
    "QMetaObject": "meta-object",
    "QMetaProperty": "meta-property",
    "QMetaMethod": "meta-method",
    "QPainterPath": "painter path",
    "QOpenGLWidget": "GL widget",
    "QKeySequenceEdit": "key-sequence editor",
    "QFontComboBox": "font picker",
    "QMessageBox": "message box",
    "QInputDialog": "input dialog",
}

# Symbol families that are an id rather than a class -- these are the census's
# business (a round of their own), so they are reported, never rewritten.
UNTOUCHED_RE = re.compile(
    r"\bNODE_OT_[a-z_]+\b|\bbpy\.[A-Za-z_.]+|\b(?:bNode|bNodeTree|bNodeSocket|"
    r"bNodeLink)[A-Za-z]*\b|\bED_node_[a-z_]+\b|"
    r"\b(?:UEdGraph|UK2Node|FEdGraph|SGraphNode|UBlueprint|FBlueprint|"
    r"EEdGraphPin)[A-Za-z]*\b"
)

# A URL, a markdown link label, or a link-reference definition. None of the
# three survives word substitution, and a paragraph holding one must not be
# re-flowed either: link definitions are line-oriented, so joining two of
# them makes both disappear.
LINK_RE = re.compile(
    r"://"                      # a URL spells the vendor in its host name
    r"|\[[^\]]*`?Q(?:t\b|[A-Z])"   # a link LABEL naming one
    r"|\]\([^)]*Q(?:t\b|[A-Z])"    # a link TARGET naming one
)


def decamel(rest: str) -> str:
    """`HeaderView` -> `header view`; `AbstractItemModel` -> `abstract item model`."""
    words = re.findall(r"[A-Z]+(?![a-z])|[A-Z][a-z0-9]*|[a-z0-9]+", rest)
    return " ".join(w if w.isupper() else w.lower() for w in words)


def rewrite_class(match: re.Match[str]) -> str:
    """A class name becomes the generic noun it already was.

    The backticks go with it -- a plain English noun in code font reads as a
    symbol, and the reader would go looking for a `header view` that does not
    exist -- but ONLY when the token is the whole code span. `QList<qreal>` has
    an opening tick that belongs to the span, not to the class, and eating it
    leaves the line with an odd number of backticks. Clippy caught exactly that
    on the first run.
    """
    token = "Q" + match.group("rest")
    noun = CLASS_OVERRIDE.get(token) or decamel(match.group("rest"))
    if match.group("tick") and match.group("tail"):
        return noun
    return match.group("tick") + noun + match.group("tail")


def rewrite_product(match: re.Match[str]) -> str:
    """The standalone phrase, or the bare noun when the sentence already has
    the article -- "a toolkit view", not "a the toolkit view"."""
    phrase, bare = PRODUCT_PHRASE[match.group("name").lower()]
    article = match.group("article")
    return article + bare if article else phrase


# Only the phrases this tool introduces are ever re-capitalised. Capitalising
# after any full stop would also capitalise the word after "e.g." and "i.e.".
INTRODUCED = sorted({phrase for phrase, _ in PRODUCT_PHRASE.values()},
                    key=len, reverse=True)
# The `(?<!\.[A-Za-z])` is what keeps "e.g. Qt does this" from becoming
# "e.g. The toolkit does this" -- an abbreviation's full stop does not end a
# sentence, and the selftest caught this on the first run.
# `(?<!//)` because the inner-doc marker `//!` ends in an exclamation mark,
# so every `//! Qt …` line read as a sentence boundary. Found by the
# selftest, not by reading.
SENTENCE_HEAD = r"((?<!\.[A-Za-z])(?<!//)[.!?]\s+)"


def fix_caps(line: str) -> str:
    """Capitalise an introduced phrase that begins a sentence WITHIN this line.

    A phrase at the very start of a comment line is left alone here, because a
    line break is not a sentence break -- "…keyed the way" / "Qt keys them"
    is one sentence, and the first draft capitalised the second half of it.
    [`fix_caps_across_lines`] is the pass that knows which line starts are also
    sentence starts.
    """
    for phrase in INTRODUCED:
        pattern = re.compile(SENTENCE_HEAD + re.escape(phrase))
        line = pattern.sub(
            lambda m, ph=phrase: m.group(1) + ph[0].upper() + ph[1:], line
        )
    return line


def fix_caps_across_lines(lines: list[str], index: int, suffix: str) -> None:
    """Capitalise a phrase opening `lines[index]` iff a sentence opens there.

    The previous meaningful character decides it, and it may be on an earlier
    line of the same comment run -- so this walks back through the paragraph
    rather than guessing from the marker.
    """
    prefix = comment_prefix(lines[index], suffix)
    if prefix is None:
        return
    body = lines[index][len(prefix):]
    phrase = next((p for p in INTRODUCED if body.startswith(p)), None)
    if phrase is None:
        return
    previous = ""
    scan = index - 1
    while scan >= 0 and comment_prefix(lines[scan], suffix) == prefix:
        earlier = lines[scan][len(prefix):].rstrip()
        if earlier:
            previous = earlier[-1]
            break
        scan -= 1
    if previous and previous not in ".!?:":
        return
    lines[index] = prefix + phrase[0].upper() + phrase[1:] + body[len(phrase):]


def is_comment(line: str, suffix: str) -> bool:
    """Whether `line` is prose rather than code.

    Python, shell and TOML take the whole `#` line; Rust takes the `//` family
    and the continuation lines of a block comment. A trailing `// note` on a
    code line is deliberately NOT a comment here -- rewriting half a line risks
    the code half, and the census keeps reporting it until a human looks.
    """
    stripped = line.lstrip()
    if suffix in (".py", ".sh", ".toml", ".tsv"):
        return stripped.startswith("#")
    return stripped.startswith(("//", "*", "/*"))


def skip_reason(body: str, suffix: str) -> str | None:
    """Why `body` is not this tool's to rewrite, or None."""
    if not is_comment(body, suffix):
        return "not a comment"
    if LINK_RE.search(body):
        return "doc link"
    return None


# `QHeaderView::saveState()` -> `saveState()`, `Qt::AlignVCenter` ->
# `AlignVCenter`. The class half of a symbol path is what names the vendor, and
# it is also redundant once the sentence around it says whose toolkit this is --
# so the method or the enumerator stands alone and the claim is unchanged. There
# is no derivation from the path to prose, which is why the first pass left
# these alone; there IS one from the path to its own tail.
SYMBOL_PATH_RE = re.compile(r"\bQ(?:t|[A-Z][A-Za-z0-9]*)::(?=[A-Za-z_])")


def strip_symbol_path(line: str) -> str:
    for token in ALLOW_PATHS:
        if token in line:
            return line
    return SYMBOL_PATH_RE.sub("", line)


# Paths whose head is not a vendor class.
ALLOW_PATHS: tuple[str, ...] = ("QName::",)


def rewrite_line(line: str) -> str:
    out = strip_symbol_path(line)
    out = CLASS_RE.sub(rewrite_class, out)
    out = PRODUCT_RE.sub(rewrite_product, out)
    return fix_caps(out)


CASES: list[tuple[str, str]] = [
    ("//! Qt's `QHeaderView` keys them.",
     "//! the toolkit's header view keys them."),
    ("/// a QTableView column", "/// a table view column"),
    ("// Blender attaches the node", "// the DCC attaches the node"),
    ("//! where Qt cannot follow", "//! where the toolkit cannot follow"),
    ("/// a Qt view keeps its width", "/// a toolkit view keeps its width"),
    ("/// QMdiSubWindow has keyboard move",
     "/// MDI child window has keyboard move"),
    ("# Grafana pushes panels", "# the dashboard tool pushes panels"),
    ("/// const QUARTET stays", "/// const QUARTET stays"),
    ("/// a `QList<qreal>` of lengths", "/// a `list<qreal>` of lengths"),
    ("/// the `QHeaderView` widget", "/// the header view widget"),
    ("/// `QHeaderView::saveState()` is opaque", "/// `saveState()` is opaque"),
    ("/// a `Qt::DecorationRole` mark", "/// a `DecorationRole` mark"),
    ("/// `QHeaderViewPrivate::write()` carries it",
     "/// `write()` carries it"),
    ("//! e.g. Qt does this", "//! e.g. the toolkit does this"),
]


# The pipeline as it actually runs: substitution, then the capitalisation pass
# that can see the previous line. A phrase opening a line is capitalised only
# when a sentence opens there, and the third case is the one the first draft got
# wrong -- a line break in the middle of a sentence.
FILE_CASES: list[tuple[list[str], list[str]]] = [
    (["// Blender attaches the node\n"], ["// The DCC attaches the node\n"]),
    (["# Grafana pushes panels\n"], ["# The dashboard tool pushes panels\n"]),
    (
        ["//! held together, keyed the way\n", "//! Qt keys them.\n"],
        ["//! held together, keyed the way\n", "//! the toolkit keys them.\n"],
    ),
    (
        ["//! a sentence ends here.\n", "//! Qt keys them.\n"],
        ["//! a sentence ends here.\n", "//! The toolkit keys them.\n"],
    ),
    (
        ["//! Qt's `QHeaderView` keys them.\n"],
        ["//! The toolkit's header view keys them.\n"],
    ),
    (
        ["/// mid-line. Qt keys them.\n"],
        ["/// mid-line. The toolkit keys them.\n"],
    ),
    (
        ["//! [`QEvent::X`]: https://doc.qt.io/qt-6/qevent.html\n"],
        ["//! [`QEvent::X`]: https://doc.qt.io/qt-6/qevent.html\n"],
    ),
    (
        ["//! see [the Qt docs](https://doc.qt.io/) for it\n"],
        ["//! see [the Qt docs](https://doc.qt.io/) for it\n"],
    ),
    (
        ["//! needs more. A\n", "//! Wireshark viewer does.\n"],
        ["//! needs more. An\n", "//! analyser viewer does.\n"],
    ),
]


def run_pipeline(lines: list[str], suffix: str) -> list[str]:
    """Substitute then capitalise, the way `migrate` does, minus the re-flow."""
    out = list(lines)
    dirty = []
    for index, line in enumerate(out):
        body = line.rstrip("\n")
        if skip_reason(body, suffix):
            continue
        new_body = rewrite_line(body)
        if new_body != body:
            out[index] = new_body + "\n"
            dirty.append(index)
    for index in dirty:
        fix_articles_across_lines(out, index, suffix)
        fix_caps_across_lines(out, index, suffix)
    return out


def selftest() -> int:
    failures = 0
    for given, want_lines in FILE_CASES:
        suffix = ".py" if given[0].lstrip().startswith("#") else ".rs"
        got_lines = run_pipeline(given, suffix)
        if got_lines != want_lines:
            failures += 1
            print(f"  FAIL pipeline\n    got  {got_lines}\n    want {want_lines}")
    for src_line, want in CASES:
        got = rewrite_line(src_line)
        if got != want:
            failures += 1
            print(f"  FAIL\n    in   {src_line}\n    got  {got}\n    want {want}")
    if decamel("HeaderView") != "header view":
        failures += 1
        print("  FAIL decamel")
    if failures:
        print(f"migrate selftest: {failures} failure(s)")
        return 1
    print(f"migrate selftest: {len(CASES) + len(FILE_CASES) + 1} cases OK")
    return 0


WIDTH = 79

# A doc paragraph is rewrappable only when every line of it is plain prose.
# A bullet, a table row, a heading or a fence carries structure that joining
# lines would destroy, so those runs keep whatever width the substitution left
# them and the file's own review catches it.
# `[*+-] ` with the space is a bullet; `**bold**` is not, and the first
# draft rejected every paragraph that opened with emphasis.
STRUCTURE = re.compile(r"^(?:[*+\-] |\d+\. |[>|#]|```|\s)")


def comment_prefix(line: str, suffix: str) -> str | None:
    """The `    /// ` an entire paragraph shares, or None for a non-comment."""
    if suffix in (".py", ".sh", ".toml", ".tsv"):
        match = re.match(r"(\s*#+\s)", line)
    else:
        match = re.match(r"(\s*//[/!]?\s)", line)
    return match.group(1) if match else None


def rewrap(lines: list[str], first: int, last: int, suffix: str) -> list[str] | None:
    """Re-flow `lines[first..=last]` to [`WIDTH`], or None if it must not be."""
    prefix = comment_prefix(lines[first], suffix)
    if prefix is None:
        return None
    bodies = []
    for line in lines[first : last + 1]:
        if comment_prefix(line, suffix) != prefix:
            return None
        body = line[len(prefix):].rstrip("\n")
        if STRUCTURE.match(body) or not body.strip():
            return None
        if LINK_RE.search(body):
            return None
        bodies.append(body)
    joined = " ".join(b.strip() for b in bodies)
    # A `code span` is one word to the wrapper. Splitting `set_filter "a&b"`
    # across two lines leaves an unterminated span, and rustdoc then reads the
    # next bullet as a continuation of it -- which is how this was found.
    spans: list[str] = []

    def hide(match: re.Match[str]) -> str:
        spans.append(match.group(0))
        return f"\x00{len(spans) - 1}\x00"

    joined = re.sub(r"`[^`]*`", hide, joined)
    wrapped = textwrap.wrap(
        joined,
        width=WIDTH - len(prefix),
        break_long_words=False,
        break_on_hyphens=False,
    )
    wrapped = [
        re.sub(r"\x00(\d+)\x00", lambda m: spans[int(m.group(1))], line)
        for line in wrapped
    ]
    # Never grow the paragraph past what it was plus the room the longer noun
    # needs; a rewrap that doubles the line count is a signal the run was not
    # prose after all.
    if len(wrapped) > len(bodies) + 3:
        return None
    return [prefix + w + "\n" for w in wrapped]


# The article the sentence already had, followed by the one the introduced
# phrase brought with it. Scoped to the phrases this tool introduces on purpose:
# a general "a/an" repair would also turn "a UI" into "an UI", because the rule
# is about the sound and not the letter.
ARTICLE_FIXES: list[tuple[re.Pattern[str], str]] = [
    (re.compile(r"\b([Aa]n?|[Tt]he)\s+" + re.escape(phrase) + r"\b"), bare)
    for phrase, bare in sorted(
        PRODUCT_PHRASE.values(), key=lambda pair: len(pair[0]), reverse=True
    )
]


def fix_articles(text: str) -> str:
    """Collapse the double article a cross-line substitution leaves behind.

    "A Wireshark / dlt-class viewer" wraps with the article closing one line and
    the name opening the next, so the line-local rule that turns "a Qt view"
    into "a toolkit view" cannot see the pair and leaves "A the analyser". This
    runs after the re-flow, when the two are on one line again.
    """
    for pattern, bare in ARTICLE_FIXES:
        text = pattern.sub(
            lambda m, noun=bare: _article(m.group(1), noun) + " " + noun, text
        )
    return text


def _article(had: str, noun: str) -> str:
    """`a` or `an` to match `noun`, keeping the case the sentence used."""
    if had.lower() == "the":
        return had
    article = "an" if noun[0].lower() in "aeiou" else "a"
    return article.capitalize() if had[0].isupper() else article


ARTICLE_WORDS = ("a", "an", "the", "A", "An", "The")


def fix_articles_across_lines(lines: list[str], index: int, suffix: str) -> None:
    """Repair an article left on the PREVIOUS line by the phrase on this one.

    The line-local rule cannot see "…needs more. A" / "Wireshark viewer", so
    without this the pair becomes "A the analyser". Re-flowing usually joins the
    two and [`fix_articles`] catches it, but a paragraph that must not be
    re-flowed -- a bullet list, a table -- never joins, so the pair is repaired
    here as well.
    """
    prefix = comment_prefix(lines[index], suffix)
    if prefix is None or index == 0:
        return
    if comment_prefix(lines[index - 1], suffix) != prefix:
        return
    body = lines[index][len(prefix):]
    match = next(
        ((phrase, bare) for phrase, bare in PRODUCT_PHRASE.values()
         if body.startswith(phrase)),
        None,
    )
    if match is None:
        return
    phrase, bare = match
    earlier = lines[index - 1][len(prefix):].rstrip()
    words = earlier.split()
    if not words or words[-1] not in ARTICLE_WORDS:
        return
    words[-1] = _article(words[-1], bare)
    lines[index - 1] = prefix + " ".join(words) + "\n"
    lines[index] = prefix + bare + body[len(phrase):]


def paragraph_bounds(lines: list[str], index: int, suffix: str) -> tuple[int, int]:
    """The contiguous run of same-prefix comment lines `index` belongs to."""
    prefix = comment_prefix(lines[index], suffix)
    first = last = index
    while first > 0 and comment_prefix(lines[first - 1], suffix) == prefix:
        first -= 1
    while last + 1 < len(lines) and comment_prefix(lines[last + 1], suffix) == prefix:
        last += 1
    return first, last


def migrate(paths: list[Path], apply: bool) -> tuple[int, int, list[str]]:
    """Rewrite comment lines and re-flow what got longer.

    Returns (files touched, lines changed, occurrences left for a human).
    """
    touched = 0
    changed = 0
    skipped: list[str] = []
    for path in paths:
        try:
            text = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        suffix = path.suffix
        lines = text.splitlines(keepends=True)
        dirty: list[int] = []
        for index, line in enumerate(lines):
            body = line.rstrip("\n")
            if UNTOUCHED_RE.search(body):
                skipped.append(f"{path.relative_to(ROOT)}:{index + 1}: reference id")
            # `SYMBOL_PATH_RE` has to be in the trigger too: both of the
            # other two refuse a name followed by `::`, so a line holding
            # ONLY `Qt::DecorationRole` matched neither and was skipped.
            if not (CLASS_RE.search(body) or PRODUCT_RE.search(body)
                    or SYMBOL_PATH_RE.search(body)):
                continue
            # A URL spells the vendor inside a host name, and a rustdoc link
            # label has to keep matching its definition line. Both are whole
            # constructs that come OUT rather than get reworded, and the first
            # run turned `doc.qt.io` into `doc.the toolkit.io`.
            reason = skip_reason(body, suffix)
            if reason:
                skipped.append(f"{path.relative_to(ROOT)}:{index + 1}: {reason}")
                continue
            new = rewrite_line(body)
            if new != body:
                lines[index] = new + ("\n" if line.endswith("\n") else "")
                dirty.append(index)
                changed += 1

        for index in dirty:
            fix_articles_across_lines(lines, index, suffix)
            fix_caps_across_lines(lines, index, suffix)

        # Re-flow bottom-up so an earlier paragraph's indices survive a later
        # splice. A paragraph is touched at most once even when several of its
        # lines changed.
        done: set[tuple[int, int]] = set()
        for index in sorted(dirty, reverse=True):
            bounds = paragraph_bounds(lines, index, suffix)
            if bounds in done:
                continue
            done.add(bounds)
            first, last = bounds
            if all(len(line.rstrip("\n")) <= WIDTH for line in lines[first : last + 1]):
                continue
            flowed = rewrap(lines, first, last, suffix)
            if flowed is not None:
                lines[first : last + 1] = flowed

        for index, line in enumerate(lines):
            prefix = comment_prefix(line, suffix)
            if prefix is None:
                continue
            body = line[len(prefix):]
            fixed = fix_articles(body)
            if fixed != body:
                lines[index] = prefix + fixed

        if dirty:
            touched += 1
            if apply:
                path.write_text("".join(lines), encoding="utf-8")
    return touched, changed, skipped


def files_under(targets: list[str]) -> list[Path]:
    out = subprocess.run(
        ["git", "ls-files", "-z", *targets],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    return [ROOT / p for p in out.split("\0") if p]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("targets", nargs="*", default=["crates"])
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--selftest", action="store_true")
    args = parser.parse_args()

    if args.selftest:
        return selftest()

    paths = files_under(args.targets or ["crates"])
    touched, changed, skipped = migrate(paths, apply=args.apply)
    verb = "rewrote" if args.apply else "would rewrite"
    print(f"{verb} {changed} comment line(s) in {touched} file(s)")
    if skipped:
        print(f"\n{len(skipped)} occurrence(s) left for a human:")
        for note in skipped[:40]:
            print(f"  {note}")
        if len(skipped) > 40:
            print(f"  ... and {len(skipped) - 40} more")
    return 0


if __name__ == "__main__":
    sys.exit(main())
