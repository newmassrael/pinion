#!/usr/bin/env python3
"""R1611 — rewrite reference-project names out of comments, evidence intact.

`tools/reference_names.py` says how many there are. This says what to do with
one, and it exists because the recorded prescription did not survive contact
with the measurement: R1610 wrote "restate every sentence by hand, and put the
source in a memory note", and the census then found **7,999 occurrences across
501 files**. Hand-restating 7,999 sentences is not a plan, it is a wish.

## What is actually being removed

Almost every occurrence is one of two things:

* the **product**, used as an actor -- "the toolkit keys sections by index";
* a **class name**, used as a noun -- "header view persists an opaque blob".

and a class name in that toolkit is its own generic noun with a letter in
front. header view IS "the header view". So the substitution is *derived* from
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

sys.path.insert(0, str(Path(__file__).resolve().parent))
from reference_names import mentions as rn_mentions  # noqa: E402

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
# owns ("non-the toolkit") is left for a human instead of half-rewritten.
PRODUCT_RE = re.compile(
    # Any determiner, not just an article: "no DCC comparison" became "no the
    # DCC comparison" because `no` was not in this set, and a reader sees that
    # before they see anything else on the line.
    r"(?P<article>\b(?:an?|the|no|any|every|each|another|some)\s+)?"
    r"(?P<name>\b(?:" + "|".join(sorted(PRODUCT_PHRASE, key=len, reverse=True))
    + r")\b)(?!::)",
    re.IGNORECASE,
)

# Products whose lowercase spelling is ordinary English, so only the capitalised
# form is one. Kept apart from `PRODUCT_PHRASE` because that table is matched
# case-insensitively, and matching these that way would rewrite the verb.
CASED_PHRASE: dict[str, tuple[str, str]] = {
    "Compose": ("another declarative toolkit", "declarative toolkit"),
    "React": ("the web UI library", "web UI library"),
    "Electron": ("a browser-shell runtime", "browser-shell runtime"),
    "Excel": ("the spreadsheet", "spreadsheet"),
}

CASED_RE = re.compile(
    r"(?P<article>\b(?:an?|the|no|any|every|each|another|some)\s+)?"
    r"(?P<name>\b(?:" + "|".join(CASED_PHRASE) + r")\b)(?!::)"
)


def rewrite_cased(match: re.Match[str]) -> str:
    phrase, bare = CASED_PHRASE[match.group("name")]
    article = match.group("article")
    compound = match.string[match.end():match.end() + 1] == "-"
    if article:
        return article + bare
    return bare if compound else phrase


# Toolkit class names: header view -> `header view`, backticks and all,
# because a generic English noun in code font reads as a symbol that does not
# exist. A name followed by `::` is a symbol PATH and is left alone -- there is
# no derivation from `saveState()` to prose, only a rewrite, and
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

# R1614 -- the id families the first pass reported and left alone.
#
# The reason it left them is that they are not class names, so the "strip the
# prefix and de-camel it" derivation the toolkit's classes have does not apply
# unchanged. It applies with one substitution each, and every one of them is a
# derivation rather than an invention:
#
# * an **operator id** is `<vendor prefix>_<capability>`, so the capability is
#   what is left when the prefix goes -- and that is not a new spelling, it is
#   the SAME spelling `tools/reference_census.py`'s `public_id` already
#   derives, so the prose and the proof tables agree by construction.
# * a **C struct** is `b` + its own generic noun, exactly as the toolkit's
#   class is `Q` + one.
# * an **engine class** is `<letter><subsystem><Rest>` where the subsystem is
#   the vendor's word for a generic thing: graph is a graph, script node is a
#   node in the visual-script graph, the visual-script compiler is that language's compiler,
#   visual script is a visual script.
#
# What is lost, in every case, is the ability to look the symbol up. That is
# what the round's memory note is for, and it is the trade the standing
# directive names.

# `add_group` -> `add_group`; `link_insert` -> `link_insert`;
# `idname` -> `idname`; `types.Node` -> `types.Node`. The code span is
# kept, because what is left IS an identifier.
ID_PREFIX_RE = re.compile(
    r"(?P<prefix>\bNODE_OT_|\bED_node_|\bbl_(?=(?:idname|label|info)\b)|\bbpy\.)"
)

# node tree -> `node tree`. The struct's own noun, with the vendor's letter
# off the front -- the same shape as the toolkit's classes.
STRUCT_RE = re.compile(
    r"(?P<tick>`?)\bb(?P<rest>Node(?:Tree|Socket|Link)?[A-Za-z]*)\b(?P<tail>`?)"
)

# graph schema -> `graph schema`; variable set node -> `variable set
# node`; compiler context -> `compiler context`; visual script ->
# `visual script`. The leading letter is the engine's type-prefix convention
# (`U` object, `F` struct, `E` enum, `S` widget) and carries no meaning here.
ENGINE_RE = re.compile(
    r"(?P<tick>`?)\b[UFESA]?(?P<family>EdGraph|K2Node|Kismet|Blueprint)"
    r"(?P<rest>[A-Za-z0-9_]*)\b(?P<tail>`?)"
)

ENGINE_NOUN: dict[str, str] = {
    "EdGraph": "graph",
    "K2Node": "node",
    "Kismet": "",
    "Blueprint": "visual script",
}


def _span(match: re.Match[str], noun: str) -> str:
    """Put a derived noun back where a symbol was, code span and all.

    The backticks go with the symbol only when they fence it exactly: a half
    span (`` `UK2Node_Foo::Bar` ``) would otherwise lose one tick and leave the
    line with an odd number of them -- the defect clippy caught on the first
    pass of this tool.
    """
    if match.group("tick") and match.group("tail"):
        return noun
    return match.group("tick") + noun + match.group("tail")


def rewrite_struct(match: re.Match[str]) -> str:
    return _span(match, decamel(match.group("rest")))


def rewrite_engine(match: re.Match[str]) -> str:
    """`K2Node_VariableSet` -> `variable set node`, and the bare family word ->
    the generic thing it names."""
    family = match.group("family")
    rest = match.group("rest").lstrip("_")
    noun = ENGINE_NOUN[family]
    tail = decamel(rest) if rest else ""
    if family == "K2Node":
        # The node's own name reads better before the noun: `variable set node`.
        derived = f"{tail} node" if tail else "script node"
    elif family == "Kismet":
        derived = tail or "the visual-script compiler"
    elif family == "EdGraph":
        derived = f"graph {tail}" if tail else "graph"
    else:
        derived = f"{noun} {tail}".strip() if tail else noun
    return _span(match, derived)

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
    # `dashboard tool-class dashboard` is a compound adjective, so the standalone phrase's article lands in
    # the middle of it: "a dashboard tool-class".
    compound = match.string[match.end():match.end() + 1] == "-"
    if article:
        return article + bare
    return bare if compound else phrase


# Only the phrases this tool introduces are ever re-capitalised. Capitalising
# after any full stop would also capitalise the word after "e.g." and "i.e.".
INTRODUCED = sorted({phrase for phrase, _ in PRODUCT_PHRASE.values()},
                    key=len, reverse=True)
# The `(?<!\.[A-Za-z])` is what keeps "e.g. the toolkit does this" from becoming "e.g. The
# toolkit does this" -- an abbreviation's full stop does not end a sentence,
# and the selftest caught this on the first run. `(?<!//)` because the inner-doc
# marker `//!` ends in an exclamation mark, so every `//! the toolkit …` line read as a sentence
# boundary. Found by the selftest, not by reading.
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
    """Whether `line` is prose rather than code, judged on the line alone.

    Rust takes the `//` family and the continuation lines of a block comment; a
    trailing `// note` on a code line is deliberately NOT prose, because
    rewriting half a line risks the code half. Everything else needs the file
    around it -- see [`prose_mask`], which is what `migrate` actually uses.
    """
    stripped = line.lstrip()
    if suffix in (".py", ".sh", ".toml", ".tsv"):
        return stripped.startswith("#")
    return stripped.startswith(("//", "*", "/*"))


DOUBLE_QUOTE = '"' * 3
SINGLE_QUOTE = "'" * 3


IN_STRINGS = False


def prose_mask(lines: list[str], suffix: str) -> list[bool]:
    """Which lines of a file are prose this tool may rewrite.

    A line on its own is not enough for three of the four kinds here:

    * **Python** -- the demos carry their explanation in the MODULE docstring
      rather than in `#` comments, so most of that population is invisible to a
      line-local rule. Only the module docstring counts: a triple-quoted string
      further down may be a payload an assertion compares against, and
      rewriting one would change what a test asserts rather than what it says.
    * **Markdown** -- everything outside a fenced code block, because a fence
      holds commands and identifiers.
    * **TSV** -- the round ledger's third column is prose, on one very long
      line. Whole rows are prose here and nothing is re-flowed, a row being a
      line by definition.
    """
    if IN_STRINGS:
        # R1614 -- the opt-in that reaches an assertion's LABEL.
        #
        # The default mask refuses every string literal, and the reason it gives
        # is sound for the general case: a literal may be a payload an assertion
        # compares against, so rewriting one changes what a test asserts rather
        # than what it says. But the population left after the comment pass is
        # almost entirely labels -- the third argument of `assert_eq`, the
        # sentence in an `expect` -- and a label is printed on failure and
        # compared to nothing.
        #
        # What makes the difference safe is not a cleverer classifier. It is
        # that the JUDGE exists: a rewritten payload fails its own test, and a
        # rewritten demo string fails its own demo. So this is opt-in per file
        # and the caller runs those tests. It is never on by default, because a
        # file nobody runs would be rewritten with nothing watching.
        return [True] * len(lines)
    if suffix == ".md":
        mask: list[bool] = []
        fenced = False
        for line in lines:
            if line.lstrip().startswith("```"):
                fenced = not fenced
                mask.append(False)
                continue
            mask.append(not fenced)
        return mask
    if suffix == ".tsv":
        return [not line.lstrip().startswith("#") for line in lines]
    if suffix in (".py", ".sh", ".toml"):
        # A shebang starts with `#` and is an interpreter directive, not prose.
        mask = [
            line.lstrip().startswith("#") and not (index == 0 and line.startswith("#!"))
            for index, line in enumerate(lines)
        ]
        if suffix == ".py":
            for index in module_docstring(lines):
                mask[index] = True
        return mask
    return [is_comment(line, suffix) for line in lines]


def module_docstring(lines: list[str]) -> list[int]:
    """The line indices of the module docstring, or empty if there is none."""
    start = None
    quote = None
    for index, line in enumerate(lines):
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        opener = stripped.lstrip("rfbu")
        for candidate in (DOUBLE_QUOTE, SINGLE_QUOTE):
            if opener.startswith(candidate):
                start, quote = index, candidate
        break  # the first real line decides; anything else means no docstring
    if start is None or quote is None:
        return []
    opener = lines[start].strip().lstrip("rfbu")
    if len(opener) > 6 and opener.endswith(quote):
        return [start]  # a one-line docstring
    out = [start]
    for index in range(start + 1, len(lines)):
        out.append(index)
        if quote in lines[index]:
            break
    return out


def skip_reason(body: str, suffix: str) -> str | None:
    """Why `body` is not this tool's to rewrite, beyond not being prose."""
    if LINK_RE.search(body):
        return "doc link"
    return None


# `saveState()` -> `saveState()`, `AlignVCenter` -> `AlignVCenter`. The class half of a symbol path is what names the
# vendor, and it is also redundant once the sentence around it says whose
# toolkit this is -- so the method or the enumerator stands alone and the claim
# is unchanged. There is no derivation from the path to prose, which is why the
# first pass left these alone; there IS one from the path to its own tail.
SYMBOL_PATH_RE = re.compile(r"\bQ(?:t|[A-Z][A-Za-z0-9]*)::(?=[A-Za-z_])")


def strip_symbol_path(line: str) -> str:
    for token in ALLOW_PATHS:
        if token in line:
            return line
    return SYMBOL_PATH_RE.sub("", line)


# Paths whose head is not a vendor class.
ALLOW_PATHS: tuple[str, ...] = ("QName::",)


def workspace_packages() -> set[str]:
    """Every package name this workspace builds.

    R1614 -- a package name is an IDENTIFIER wearing a string literal's
    clothes, and `--in-strings` rewrote one: `figma-button-m3` in a demo's
    sweep list became `design tool-button-m3`, with a space, and cargo refused
    it. The demo caught it, which is the judge working -- and a class this
    cheap to close should not need the judge twice.

    Read from the workspace manifest rather than from `cargo metadata`, which
    would make this tool need a toolchain to rewrite a comment.
    """
    names: set[str] = set()
    for manifest in ROOT.glob("*/*/Cargo.toml"):
        for line in manifest.read_text(encoding="utf-8").splitlines():
            if line.startswith("name"):
                _, _, value = line.partition("=")
                names.add(value.strip().strip('"'))
                break
    return {n for n in names if n}


PACKAGE_NAMES = workspace_packages()


def guard_packages(line: str) -> tuple[str, list[str]]:
    """Hide every workspace package name in `line` behind a placeholder.

    Returns the masked line and what was hidden, so the caller can put them
    back untouched after the substitutions have run.
    """
    hidden: list[str] = []
    for name in sorted(PACKAGE_NAMES, key=len, reverse=True):
        if name in line:
            token = f"\x02{len(hidden)}\x02"
            line = line.replace(name, token)
            hidden.append(name)
    return line, hidden


def unguard_packages(line: str, hidden: list[str]) -> str:
    for index, name in enumerate(hidden):
        line = line.replace(f"\x02{index}\x02", name)
    return line


def rewrite_line(line: str) -> str:
    line, hidden = guard_packages(line)
    out = strip_symbol_path(line)
    out = ENGINE_RE.sub(rewrite_engine, out)
    out = STRUCT_RE.sub(rewrite_struct, out)
    out = ID_PREFIX_RE.sub("", out)
    out = CLASS_RE.sub(rewrite_class, out)
    out = PRODUCT_RE.sub(rewrite_product, out)
    out = CASED_RE.sub(rewrite_cased, out)
    return unguard_packages(fix_caps(out), hidden)


def has_name(body: str) -> bool:
    """Whether this line holds anything this tool knows how to rewrite."""
    # `SYMBOL_PATH_RE` has to be here too: the class and product patterns both
    # refuse a name followed by `::`, so a line holding ONLY `DecorationRole`
    # matched neither and was skipped.
    return bool(
        CLASS_RE.search(body)
        or PRODUCT_RE.search(body)
        or SYMBOL_PATH_RE.search(body)
        or CASED_RE.search(body)
        or ENGINE_RE.search(body)
        or STRUCT_RE.search(body)
        or ID_PREFIX_RE.search(body)
    )


# R1614 -- a link whose TARGET is the vendor's documentation site.
#
# The first pass refused any line holding a URL, and it was right to: a host
# name is not prose and substituting inside one produced `doc.the toolkit.io`.
# But refusing them left the citation in place, which is the thing the
# directive is about. A link is not reworded -- it is REMOVED, and what it
# was citing goes to the round's memory note, which is where the standing
# prescription says the source belongs.
LINK_DEF_RE = re.compile(r"^(?P<lead>.*?)\[(?P<label>[^\]]+)\]:\s*(?P<url>\S+)\s*$")
INLINE_LINK_RE = re.compile(r"\[(?P<label>[^\]]+)\]\((?P<url>[^)]*)\)")


def _vendor_url(url: str) -> bool:
    """Whether a URL's HOST names a reference project.

    Only the host: a path segment can hold an ordinary word that happens to
    collide, and a link to our own repository is not a citation of anyone.
    """
    host = re.sub(r"^[a-z+]+://", "", url).split("/", 1)[0]
    return bool(rn_mentions(host))


def _blank_comment(line: str) -> bool:
    """A comment marker with nothing after it."""
    return line.strip() in ("//!", "///", "//", "#")


def unlink_vendor_docs(lines: list[str], mask: list[bool]) -> list[str]:
    """Drop link definitions pointing at a vendor's docs, and unlink their uses.

    Two passes, because a definition and its label are on different lines: the
    definitions go first and record which labels are now dangling, then every
    use of a dangling label loses its brackets. A dangling rustdoc link is a
    hard error under `-D warnings`, so leaving one behind would be caught --
    but caught at the commit gate rather than here, and this tool's job is to
    hand the gate something that compiles.
    """
    dangling: set[str] = set()
    out: list[str] = []
    trimmed: list[str] = []
    for index, line in enumerate(lines):
        body = line.rstrip("\n")
        match = LINK_DEF_RE.match(body)
        if mask[index] and match and _vendor_url(match.group("url")):
            dangling.add(match.group("label"))
            continue  # the whole line goes
        out.append(line)
    # A link definition sits at the end of its doc block behind a blank comment
    # line that exists only to separate it, so taking the definition can end
    # the block on an empty `//!`. Only a TRAILING one is dropped: the blank
    # before a definition that SURVIVES is load-bearing, because a link
    # reference glued to the paragraph above it stops being a definition at all
    # -- which is how this rule was found, as two broken rustdoc links.
    for index, line in enumerate(out):
        if (
            _blank_comment(line)
            and trimmed
            and _blank_comment(trimmed[-1]) is False
            and comment_prefix(trimmed[-1], "") is not None
            and (index + 1 >= len(out) or comment_prefix(out[index + 1], "") is None)
        ):
            continue
        trimmed.append(line)
    out = trimmed
    if not dangling:
        result = out
    else:
        result = []
        for line in out:
            body = line.rstrip("\n")
            for label in dangling:
                body = body.replace(f"[{label}]", label)
            result.append(body + ("\n" if line.endswith("\n") else ""))
    # An INLINE link to a vendor's docs keeps its label and loses its target.
    final: list[str] = []
    for index, line in enumerate(result):
        if index < len(mask) and not mask[index]:
            final.append(line)
            continue
        body = line.rstrip("\n")
        body = INLINE_LINK_RE.sub(
            lambda m: m.group("label") if _vendor_url(m.group("url")) else m.group(0),
            body,
        )
        final.append(body + ("\n" if line.endswith("\n") else ""))
    return final


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
    ("/// no Blender comparison surfaces it", "/// no DCC comparison surfaces it"),
    ("/// a Grafana-class dashboard", "/// a dashboard tool-class dashboard"),
    ("/// Unreal-class editor, self-hosted", "/// engine-class editor, self-hosted"),
    ("/// every Qt view does", "/// every toolkit view does"),
    ("/// things that Qt cannot answer", "/// things that the toolkit cannot answer"),
    ("/// no Qt peer exists", "/// no toolkit peer exists"),
    ("/// a Compose-class toolkit", "/// a declarative toolkit-class toolkit"),
    ("/// we compose a scene", "/// we compose a scene"),
    ("/// React-class rendering", "/// web UI library-class rendering"),
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
    # R1614 -- a link to a vendor's docs is REMOVED rather than reworded. The
    # definition line goes whole; the label it defined loses its brackets so no
    # rustdoc link dangles; and the name inside the label is then an ordinary
    # symbol the class pass can read.
    (
        ["//! spells it [`QEvent::X`] here.\n",
         "//! [`QEvent::X`]: https://doc.qt.io/qt-6/qevent.html\n"],
        ["//! spells it `X` here.\n"],
    ),
    (
        ["//! see [the Qt docs](https://doc.qt.io/) for it\n"],
        ["//! see the toolkit docs for it\n"],
    ),
    (
        ["//! see [our own notes](https://github.com/x/y) for it\n"],
        ["//! see [our own notes](https://github.com/x/y) for it\n"],
    ),
    # The id families R1614 taught it. Each is a derivation: the capability
    # left when a vendor prefix goes, or the generic noun a type-prefix hides.
    (["// what the DCC's NODE_OT_delete_reconnect does\n"],
     ["// what the DCC's delete_reconnect does\n"]),
    (["// `bNodeTree` holds the links\n"], ["// node tree holds the links\n"]),
    (["// `UEdGraphSchema` decides\n"], ["// graph schema decides\n"]),
    (["// `UK2Node_VariableSet` writes it\n"],
     ["// variable set node writes it\n"]),
    (["// FKismetCompilerContext walks it\n"],
     ["// compiler context walks it\n"]),
    (["// a Blueprint graph\n"], ["// a visual script graph\n"]),
    # R1614 -- a workspace package name is an identifier, not prose, wherever
    # it appears. `--in-strings` rewrote one into a name with a SPACE in it and
    # cargo refused to build; the demo caught it and this closes the class.

    (
        ["//! needs more. A\n", "//! Wireshark viewer does.\n"],
        ["//! needs more. An\n", "//! analyser viewer does.\n"],
    ),
]


def run_pipeline(lines: list[str], suffix: str) -> list[str]:
    """Unlink, substitute, then capitalise, the way `migrate` does, minus the
    re-flow.

    R1614 -- the unlink pass is here because it was NOT, and the two cases
    asserting that a vendor doc link is left alone went on passing after the
    pass that removes them landed. A selftest that runs a different pipeline
    than the tool is a selftest of nothing.
    """
    out = list(lines)
    out = unlink_vendor_docs(out, prose_mask(out, suffix))
    mask = prose_mask(out, suffix)
    dirty = []
    for index, line in enumerate(out):
        body = line.rstrip("\n")
        if not mask[index] or skip_reason(body, suffix):
            continue
        new_body = rewrite_line(body)
        if new_body != body:
            out[index] = new_body + "\n"
            dirty.append(index)
    for index in dirty:
        fix_articles_across_lines(out, index, suffix)
        fix_caps_across_lines(out, index, suffix)
    return out



# `prose_mask` decides what a whole FILE offers this tool, and each of its four
# kinds has a way to be wrong that a line-local rule cannot see.
MASK_CASES: list[tuple[str, list[str], list[bool]]] = [
    (
        ".py",
        ['#!/usr/bin/env python3\n', 'Q3one line docstring.Q3\n',
         'PAYLOAD = Q3\n', 'Qt appears here\n', 'Q3\n'],
        [False, True, False, False, False],
    ),
    (
        ".py",
        ['Q3\n', 'the prose is here\n', 'Q3\n', 'x = 1\n',
         'DATA = Q3\n', 'not prose\n', 'Q3\n'],
        [True, True, True, False, False, False, False],
    ),
    (
        ".py",
        ['import sys\n', 'DATA = Q3\n', 'not a docstring\n', 'Q3\n'],
        [False, False, False, False],
    ),
    (
        ".md",
        ['prose\n', '```sh\n', 'qt-config --version\n', '```\n', 'more\n'],
        [True, False, False, False, True],
    ),
    (
        ".tsv",
        ['# a comment header\n', '1610\tosnative\tprose about it\n'],
        [False, True],
    ),
]


def selftest_packages() -> int:
    """The package guard, against a package whose NAME holds a vendor token.

    R1614 -- no package in this workspace is named that way any more, which is
    exactly why this test injects one: a fixture built from the current
    manifest cannot tell the guard from its absence, and the round's first
    draft of this case did not. The defect it models is real: `--in-strings`
    rewrote `figma-button-m3` in a demo's sweep list into a name with a SPACE
    and cargo refused to build.
    """
    global PACKAGE_NAMES  # noqa: PLW0603 -- the table under test
    saved = PACKAGE_NAMES
    failures = 0
    try:
        PACKAGE_NAMES = saved | {"figma-button-m3"}
        cases = [
            ('    "figma-button-m3",', '    "figma-button-m3",'),
            ('# figma-button-m3 is built with Qt',
             '# figma-button-m3 is built with the toolkit'),
        ]
        for given, want in cases:
            got = rewrite_line(given)
            if got != want:
                failures += 1
                print("  FAIL package guard")
                print(f"    got  {got!r}")
                print(f"    want {want!r}")
    finally:
        PACKAGE_NAMES = saved
    return failures


def selftest_masks() -> int:
    failures = 0
    for suffix, given, want in MASK_CASES:
        lines = [line.replace("Q3", DOUBLE_QUOTE) for line in given]
        got = prose_mask(lines, suffix)
        if got != want:
            failures += 1
            print(f"  FAIL mask {suffix}\n    got  {got}\n    want {want}")
    return failures


def selftest() -> int:
    failures = selftest_masks() + selftest_packages()
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
    print(
        f"migrate selftest: "
        f"{len(CASES) + len(FILE_CASES) + len(MASK_CASES) + 3} cases OK"
    )
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
    # R1614 -- the placeholder is PADDED to the span's own width. It was three
    # characters standing in for eighteen, so the wrapper measured a line that
    # was not the line, and every paragraph holding a code span came out over
    # the margin by the difference. Nothing failed: no lint measures a comment,
    # so the only witness is a reader, and the first pass of this tool had none.
    spans: list[str] = []

    def hide(match: re.Match[str]) -> str:
        span = match.group(0)
        spans.append(span)
        token = f"\x00{len(spans) - 1}\x00"
        return token + "\x01" * max(0, len(span) - len(token))

    joined = re.sub(r"`[^`]*`", hide, joined)
    wrapped = textwrap.wrap(
        joined,
        width=WIDTH - len(prefix),
        break_long_words=False,
        break_on_hyphens=False,
    )
    wrapped = [
        re.sub(r"\x00(\d+)\x00\x01*", lambda m: spans[int(m.group(1))], line)
        for line in wrapped
    ]
    # Never grow the paragraph past what it was plus the room the longer noun
    # needs; a rewrap that doubles the line count is a signal the run was not
    # prose after all.
    if len(wrapped) > len(bodies) + 3:
        return None
    return [prefix + w + "\n" for w in wrapped]


# `that` and `this` are absent on purpose: they are relative pronouns as often as
# determiners, and "three things that the toolkit cannot answer" needs "that
# THE toolkit cannot answer". Dropping the article there breaks the sentence.
ARTICLE_WORDS = (
    "a", "an", "the", "no", "any", "every", "each", "another", "some",
) + (
    "A", "An", "The", "No", "Any", "Every", "Each", "Another", "Some",
)


# The article the sentence already had, followed by the one the introduced
# phrase brought with it. Scoped to the phrases this tool introduces on purpose:
# a general "a/an" repair would also turn "a UI" into "an UI", because the rule
# is about the sound and not the letter.
ARTICLE_FIXES: list[tuple[re.Pattern[str], str]] = [
    (re.compile(r"\b(" + "|".join(ARTICLE_WORDS) + r")\s+"
                + re.escape(phrase) + r"\b"), bare)
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
        mask = prose_mask(lines, suffix)
        unlinked = unlink_vendor_docs(lines, mask)
        if unlinked != lines:
            lines = unlinked
            mask = prose_mask(lines, suffix)
            changed += 1
            dropped_links = True
        else:
            dropped_links = False
        dirty: list[int] = []
        for index, line in enumerate(lines):
            body = line.rstrip("\n")
            if not has_name(body):
                continue
            # A URL spells the vendor inside a host name, and a rustdoc link
            # label has to keep matching its definition line. Both are whole
            # constructs that come OUT rather than get reworded, and the first
            # run turned `doc.the toolkit.io` into `doc.the toolkit.io`.
            reason = None if mask[index] else "not prose"
            reason = reason or skip_reason(body, suffix)
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

        # A file whose names are already gone can still be carrying the double
        # article an earlier pass left behind, so the repair counts as a change
        # in its own right. Gating the write on `dirty` alone computed the fix
        # and threw it away.
        # The mask is recomputed here because the passes above SPLICE: a
        # re-flowed paragraph and a removed link definition both change the
        # line count, and indexing a stale mask threw `IndexError` the first
        # time a file lost a line.
        mask = prose_mask(lines, suffix)
        repaired = False
        for index, line in enumerate(lines):
            if not mask[index]:
                continue
            # A docstring line has no comment marker, so keying the repair off
            # one skipped exactly the population that needed it most.
            prefix = comment_prefix(line, suffix) or ""
            body = line[len(prefix):]
            fixed = fix_articles(body)
            if fixed != body:
                lines[index] = prefix + fixed
                repaired = True
                changed += 1

        if dirty or repaired or dropped_links:
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
    parser.add_argument(
        "--in-strings",
        action="store_true",
        help="also rewrite inside string literals; the caller MUST run the "
             "tests and demos of every file passed",
    )
    args = parser.parse_args()

    if args.selftest:
        return selftest()

    if args.in_strings:
        globals()["IN_STRINGS"] = True

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
