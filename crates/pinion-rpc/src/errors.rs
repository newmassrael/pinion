//! `rpc/errors` — the error-code discovery surface (R1564.1 §5.7 §5.12 §2 #7).
//!
//! R1089 made the method NAMES askable and R1539 made the response SHAPES
//! askable. The third thing a client must know to talk to this wire is what a
//! failure means, and that was the one an agent could only learn by reading
//! pinion's source.
//!
//! It became load-bearing in R1564. Until then every `error.data` was a word
//! this crate authored, so a consumer could match one to classify a failure and
//! the codes hardly mattered. R1564 put the PRODUCER's sentence in `data` for a
//! refused action — arbitrary application prose, the thing a consumer must not
//! branch on — which moved the whole classifying job onto `error.code`. Shipping
//! that without publishing the codes would have replaced one undiscoverable
//! contract with another.
//!
//! **The rule this publishes** is the one a client needs and cannot infer:
//! `-32602` carries a word this crate owns, and every application-defined code
//! (`-32000..=-32099`) carries a sentence the surface wrote. So `data_is_prose`
//! is not decoration — it tells a client whether the payload it is holding may
//! be matched or only shown.
//!
//! R1566 published the words themselves (`data_vocabulary`), because a round
//! that ADDS to a closed vocabulary and does not publish it leaves the client
//! exactly where R1564 found it. Enumerating them corrected the rule as well as
//! completing it: `-32602`'s payload is not always one of the words. Two focus
//! refusals put the word in `error.message` and the caller's own tag in
//! `error.data`, and a window-prefix failure appends the offending id to its
//! word. So the honest statement is "a word this crate owns, or an echo of what
//! the caller supplied — never free application prose", and that is what the
//! entry now says.
//!
//! Standard JSON-RPC 2.0 codes are listed too, and deliberately: an agent
//! meeting `-32601` should not have to know which half of the protocol it came
//! from to find out what it means.

use serde::{Deserialize, Serialize};

use crate::dispatch::{ACTION_REFUSED, VALUE_OUT_OF_RANGE};

/// One published error code.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorEntry {
    /// The JSON-RPC `error.code`.
    pub code: i32,
    /// The `error.message` this dispatcher pairs with the code — the category
    /// label, stable per code.
    pub message: String,
    /// What the failure means, for a human and for an agent's prompt.
    pub meaning: String,
    /// Whether `error.data` is **the surface's own sentence** (`true` — show it,
    /// never match it) or a word from this crate's closed vocabulary (`false` —
    /// safe to match). The single fact that decides how a client may treat the
    /// payload.
    pub data_is_prose: bool,
    /// `true` for the four codes JSON-RPC 2.0 itself defines, `false` for the
    /// ones pinion allocates in the implementation-defined server-error range.
    /// A client that already handles the standard set can skip them.
    pub standard: bool,
    /// R1566 — every word `error.data` may hold under this code, exhaustively.
    /// Empty when `data_is_prose`, and empty for a code that carries no word.
    ///
    /// R1565 published `data_is_prose: false` for `-32602` on the strength of
    /// the payload being "a word from this dispatcher's closed vocabulary" —
    /// and left the vocabulary itself unpublished, so a client could learn that
    /// matching was *safe* and still had to read pinion's source to learn what
    /// to match. R1566 was the round that made that bite: it added three words
    /// (`PathIsAnAction`, `PathIsAReadSlot`, and `ReadOnly` reaching cases it
    /// never used to), and shipping them undiscoverable would have repeated
    /// exactly the mistake this module exists to end.
    ///
    /// Enumerating it also corrected the claim. See the `-32602` entry's
    /// `meaning`: not every payload under that code is one of these words, and
    /// the ones that are not are **echoes of the caller's own input** rather
    /// than application prose — a distinction `data_is_prose` alone cannot
    /// carry and a client very much needs.
    pub data_vocabulary: Vec<String>,
}

/// R1566 — every word `-32602`'s `error.data` may hold, in sorted order.
///
/// Kept sorted because it is published as a set and a client may binary-search
/// it; kept here rather than beside the `match` arms that emit it because the
/// arms are spread over three dispatchers and a vocabulary is a property of the
/// code. `r1566_the_published_vocabulary_is_exactly_what_is_emitted` scans the
/// source in **both** directions, so a word added to a dispatcher and not here
/// fails, and so does a word here that nothing emits — a dead entry in a closed
/// vocabulary is a promise to a client that will never be kept.
/// R1610 — the vocabulary grew from 17 words to 35 without a single new
/// refusal being written, because the gate below could see two of the four
/// shapes this dispatcher emits a word in.
///
/// R1566 built the scan around `WireFault::params("…")` and
/// `Cow::Borrowed("…")`, which is how `path.rs` and `query.rs` spell it. The
/// dispatcher's own refusals are spelled two other ways —
/// `RpcError::invalid_params("Word")` directly, and the
/// `fn …_error_to_rpc(err) { let variant = match err { … }; …invalid_params(variant) }`
/// block that thirteen error types share — and both were invisible. Eighteen
/// words reached the wire that a client could only learn by reading pinion's
/// source, which is the exact defect this module exists to end, sitting inside
/// the gate built to end it.
///
/// It is the R1605 shape again: a text census that looks for the emissions it
/// knows how to find, rather than making every emission account for itself.
/// The scan below now walks the `variant` blocks structurally (from
/// `let variant = match` to the end of its function, counted only when that
/// function reaches `invalid_params(variant)` — `apply_error_to_rpc` builds an
/// OBJECT payload out of the same shape and is correctly not vocabulary), and
/// reads a bare CamelCase literal passed straight to `invalid_params`.
///
/// What remains outside is PROSE, and deliberately: 81 of the dispatcher's
/// literals are sentences like `"params.value missing"` — echoes of the
/// caller's own input, which the `-32602` entry's `meaning` already tells a
/// client not to match on.
const VOCABULARY_32602: &[&str] = &[
    // `PathError::wire_tag` — the window-prefix failures. `UnknownWindow`
    // appends the offending id after a colon, so it is the one member a client
    // matches by PREFIX; the entry's `meaning` says so.
    "CapacityFull",
    "ClosureUnavailable",
    // R1637 — the surface published the name and then did not answer it. Split
    // out of `UnknownInvokePath`, which now means only "no such declaration".
    "DeclaredButUnhandled",
    "EmptySteps",
    "EmptyWindowId",
    "InitialQueryFailed",
    "InputInjectionUnavailable",
    "Intervene",
    "InterveneTypeMismatch",
    "IntrospectionOptedOut",
    "InvalidSize",
    "InvalidViewport",
    "InvokeTypeMismatch",
    "MalformedDisplayAsk",
    "MalformedPrefix",
    "MissingCursor",
    "NoAxisDeclared",
    "NoExternalAtPath",
    "NoLastPaintLayout",
    "OutOfBounds",
    "PaintProducerUnavailable",
    "PathIsAReadSlot",
    "PathIsAnAction",
    "ReadOnly",
    "RenderBackendUnavailable",
    "RetainedNodeNotWritable",
    "RollbackFailed",
    "SnapshotFailed",
    // R1629 — the `scene/derivations` filter vocabulary. Appends the word the
    // caller asked for AND the whole accepted set after the colon, so a client
    // that mistyped `invented` learns the four kinds from the refusal instead
    // of from this source file.
    "UnknownDerivationKind",
    "UnknownIntervenePath",
    "UnknownIntrospectPath",
    "UnknownInvokePath",
    "UnknownLevel",
    "UnknownPath",
    // R1615 — `UnknownProposalKind` was on the wire and unpublished. The scan
    // that should have caught it recognised the `"Word: {detail}"` shape by
    // two hard-coded `contains` checks naming the two call sites that existed
    // when it was written, so a third one was invisible. Generalising the rule
    // surfaced it in the same round that added `UnknownTag` — the fifth
    // instance of a census finding only what it was told to look for.
    "UnknownProposalKind",
    "UnknownTag",
    "UnknownWindow",
    "UnmappedSurfaceError",
    "UnsupportedPath",
    "ZeroAttempts",
];

/// The published catalog. Ordered by code, descending — the standard codes
/// first, then pinion's own, which is the order a reader meets them in.
fn entries() -> Vec<ErrorEntry> {
    let e =
        |code: i32, message: &str, meaning: &str, data_is_prose: bool, standard: bool| ErrorEntry {
            code,
            message: message.to_owned(),
            meaning: meaning.to_owned(),
            data_is_prose,
            standard,
            data_vocabulary: Vec::new(),
        };
    let with_vocabulary = |entry: ErrorEntry, words: &[&str]| ErrorEntry {
        data_vocabulary: words.iter().map(|w| (*w).to_owned()).collect(),
        ..entry
    };
    vec![
        e(
            -32600,
            "Invalid Request",
            "the frame is not a well-formed JSON-RPC 2.0 request object",
            false,
            true,
        ),
        e(
            -32601,
            "Method not found",
            "no method by that name; ask rpc/methods for the catalog",
            false,
            true,
        ),
        with_vocabulary(
            e(
                -32602,
                "Invalid params",
                "the parameters are missing, mis-shaped, or name something that \
                 does not exist. error.data is EITHER one of the words in \
                 data_vocabulary — a closed set this dispatcher owns, safe to \
                 match — or an echo of what the caller itself supplied (the tag \
                 a focus request named, the window id that did not resolve). It \
                 is never free application prose, which is what data_is_prose \
                 records. Two shapes widen it without changing the word: an \
                 opt-in with_origin request answers an OBJECT whose `reason` \
                 holds the word beside the surface that refused (R1485), and \
                 two failures answer the word with the offending name appended \
                 after a colon — a window prefix that did not resolve, and a \
                 malformed scene/displays parameter, which names the parameter \
                 path (R1576). Both stay matchable by prefix. A refusal that \
                 names a CLOSED value set — UnknownLevel is the one — does not \
                 carry the set: rpc/schema does, as the field's `values`, so a \
                 client reads what is accepted BEFORE it asks rather than \
                 guessing and being refused again (R1616)",
                false,
                true,
            ),
            VOCABULARY_32602,
        ),
        e(
            -32603,
            "Internal error",
            "the dispatcher failed for a reason that is not the caller's; \
             error.data is a programmer-facing detail",
            false,
            true,
        ),
        e(
            -32004,
            "focus manager unavailable",
            "the embedding shell wired no focus manager into this dispatch, so \
             the focus/* surface is unreachable in this process",
            false,
            false,
        ),
        e(
            ACTION_REFUSED,
            "Action refused",
            "the parameters were FINE — the path resolved and the argument type \
             matched — and the surface then declined on a fact about its own \
             state. error.data is that surface's own sentence: show it to the \
             operator, and branch on this code rather than on its text. Retrying \
             may succeed once the stated condition changes",
            true,
            false,
        ),
        e(
            VALUE_OUT_OF_RANGE,
            "Value out of range",
            "the value was the right TYPE for the slot and outside the range the \
             slot accepts. error.data is the surface's own sentence, which names \
             that range: show it, and branch on this code rather than on its text",
            true,
            false,
        ),
    ]
}

/// Response payload for `rpc/errors`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RpcErrors {
    /// Every error code this dispatcher can produce.
    pub errors: Vec<ErrorEntry>,
    /// `errors.len()`, so a client need not re-count.
    pub count: usize,
    /// The application-defined range JSON-RPC 2.0 reserves for a server, as
    /// `[low, high]` inclusive. Published so a client can tell one of pinion's
    /// codes from a standard one **arithmetically** — and so a code added after
    /// this client shipped is still classifiable rather than merely unknown.
    pub application_range: [i32; 2],
    /// The rule that decides whether `error.data` may be matched, on the wire
    /// rather than only in this crate's rustdoc.
    pub data_doc: String,
}

/// The rule [`RpcErrors::data_doc`] carries.
pub const DATA_DOC: &str = "error.data is matchable ONLY when data_is_prose is false. A false entry \
     carries a word from this dispatcher's closed vocabulary; a true entry carries \
     a sentence the application surface wrote, which is free text that may change \
     between builds and between surfaces. Branch on error.code; show error.data.";

/// Build the `rpc/errors` response.
#[must_use]
pub fn rpc_errors() -> RpcErrors {
    let errors = entries();
    RpcErrors {
        count: errors.len(),
        errors,
        application_range: [-32099, -32000],
        data_doc: DATA_DOC.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r1564_1_every_code_this_crate_emits_is_published() {
        // The census that makes this catalog worth asking. A code emitted by
        // `RpcError::new` and absent here would be exactly the undiscoverable
        // contract this module exists to end — so the SOURCE is scanned, not a
        // hand-kept list, the same cross-check `rpc/methods` makes against the
        // routing match.
        let published: Vec<i32> = rpc_errors().errors.iter().map(|e| e.code).collect();
        let mut emitted: Vec<i32> = Vec::new();
        for src in [
            include_str!("dispatch.rs"),
            include_str!("focus.rs"),
            include_str!("errors.rs"),
        ] {
            for line in src.lines() {
                // Skip prose: a code MENTIONED in a doc comment is not a code
                // emitted, and this module's own doc mentions several.
                let code_only = line.trim_start();
                if code_only.starts_with("//") || code_only.starts_with("///") {
                    continue;
                }
                let mut rest = line;
                while let Some(i) = rest.find("RpcError::new(") {
                    rest = &rest[i + "RpcError::new(".len()..];
                    let arg: String = rest
                        .chars()
                        .take_while(|c| *c == '-' || c.is_ascii_digit())
                        .collect();
                    if let Ok(n) = arg.parse::<i32>() {
                        emitted.push(n);
                    }
                }
            }
        }
        // `ACTION_REFUSED` reaches `RpcError::new` through the constant, so the
        // scan sees no literal for it; assert it explicitly rather than
        // loosening the scan, which would let a real omission through.
        for (code, round) in [(ACTION_REFUSED, "R1564"), (VALUE_OUT_OF_RANGE, "R1565")] {
            assert!(
                published.contains(&code),
                "the code {round} allocated is published",
            );
        }
        for code in emitted {
            assert!(
                published.contains(&code),
                "code {code} is emitted by this crate and absent from rpc/errors",
            );
        }
    }

    #[test]
    fn r1564_1_prose_and_matchable_payloads_are_told_apart() {
        // The distinction the whole catalog exists to carry. If every entry
        // agreed, `data_is_prose` would be a constant and a client reading it
        // would learn nothing.
        let errors = rpc_errors().errors;
        assert!(errors.iter().any(|e| e.data_is_prose));
        assert!(errors.iter().any(|e| !e.data_is_prose));
        // And the rule it states holds of the entries: exactly the
        // application-defined codes carry prose today.
        let [low, high] = rpc_errors().application_range;
        for e in &errors {
            let application = (low..=high).contains(&e.code);
            assert_eq!(
                e.standard,
                !application,
                "code {} sits {} the application range but is marked standard={}",
                e.code,
                if application { "inside" } else { "outside" },
                e.standard,
            );
        }
    }

    /// R1610 — is this a CamelCase vocabulary word rather than prose?
    ///
    /// The dispatcher's `-32602` payloads are a MIX, which is what the
    /// `-32602` entry's `meaning` already tells a client: a word from a closed
    /// vocabulary, or an echo of the caller's own input
    /// (`"params.value missing"`). One rule tells them apart — a word has no
    /// space, no colon and no dot, and starts upper-case — and it has to be
    /// applied, because "collect every literal" would publish 81 sentences as
    /// vocabulary and "collect the ones I recognise" is how R1566's scan came
    /// to miss eighteen.
    fn is_vocabulary_word(s: &str) -> bool {
        s.starts_with(|c: char| c.is_ascii_uppercase())
            && s.chars().all(|c| c.is_ascii_alphanumeric())
    }

    /// R1610 — the word in `RpcError::invalid_params("Word")`, when the literal
    /// is a vocabulary word rather than prose.
    fn bare_invalid_params_word(line: &str) -> Option<&str> {
        let after = line.split_once("invalid_params(\"")?.1;
        let word = after.split_once('"')?.0;
        is_vocabulary_word(word).then_some(word)
    }

    /// R1615 — every vocabulary word a string literal on this line opens with,
    /// in the `"Word: {detail}"` shape the -32602 entry tells clients to match
    /// by prefix.
    ///
    /// Derived from the shape, not from a list of the three call sites that
    /// currently use it. The two hard-coded `line.contains("\"UnknownWindow:
    /// {requested:?}")` checks this replaces would have called R1615's
    /// `UnknownTag` a dead entry — the scan would have been blind to a word
    /// genuinely on the wire, which is precisely what this gate exists to
    /// catch one layer down.
    ///
    /// The `: ` after the word is what keeps it from swallowing prose: a
    /// sentence like `"params.tag missing"` does not start with an upper-case
    /// bare word, and a word followed by a colon-space in a payload literal is
    /// the documented shape.
    fn colon_prefixed_words(line: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let mut rest = line;
        while let Some(i) = rest.find('"') {
            rest = &rest[i + 1..];
            let Some(end) = rest.find('"') else { break };
            let literal = &rest[..end];
            // A vocabulary word, and long enough to be one: `is_vocabulary_word`
            // alone admits a single capital, and a debug format like `"P: {x}"`
            // is not a refusal.
            if let Some((word, _)) = literal.split_once(": ")
                && is_vocabulary_word(word)
                && word.len() >= 4
                && word.chars().any(char::is_lowercase)
            {
                out.push(word);
            }
            rest = &rest[end + 1..];
        }
        out
    }

    /// R1610 — every word emitted by a `let variant = match err { … }` block
    /// whose function reaches `RpcError::invalid_params(variant)`.
    ///
    /// Structural rather than line-local on purpose. `=> "Word",` is how a
    /// dozen VALUE enums spell their wire names too (`Butt`, `Center`, `Tile`,
    /// `Ellipsis`), so a line pattern collects sixteen of those as refusals —
    /// measured. Scoping to the block, and requiring the block's function to
    /// actually hand `variant` to `invalid_params`, is also what correctly
    /// EXCLUDES `apply_error_to_rpc`: it has the same `let variant = match`
    /// shape and builds an object payload, so its words are not vocabulary.
    fn variant_block_words(src: &str) -> Vec<&str> {
        let lines: Vec<&str> = src.lines().collect();
        let mut words = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            if !lines[i].trim_start().starts_with("let variant = match") {
                i += 1;
                continue;
            }
            let mut block = Vec::new();
            let mut reaches_invalid_params = false;
            let mut j = i + 1;
            // A bare `}` in column zero ends the enclosing function — the one
            // boundary that does not need a brace counter here, since every
            // one of these lives at the top level of the module.
            while j < lines.len() && lines[j] != "}" {
                let line = lines[j];
                if !line.trim_start().starts_with("//") {
                    if let Some(word) = line
                        .split_once("=> \"")
                        .and_then(|(_, rest)| rest.split_once('"'))
                        .map(|(word, _)| word)
                        .filter(|w| is_vocabulary_word(w))
                    {
                        block.push(word);
                    }
                    if line.contains("RpcError::invalid_params(variant)") {
                        reaches_invalid_params = true;
                    }
                }
                j += 1;
            }
            if reaches_invalid_params {
                words.extend(block);
            }
            i = j;
        }
        words
    }

    /// R1566 — the vocabulary is complete in BOTH directions.
    ///
    /// One direction stops a word reaching the wire undiscoverable, which is
    /// the whole reason the field exists. The other stops a word being
    /// published that nothing emits: a closed vocabulary with a dead member is
    /// a client branch that can never be taken, and it is the failure mode a
    /// hand-kept list drifts into first — an entry outlives the arm that made
    /// it, and nothing notices because the *interesting* direction still
    /// passes.
    #[test]
    fn r1566_the_published_vocabulary_is_exactly_what_is_emitted() {
        let catalogue = rpc_errors();
        let published: &[String] = &catalogue
            .errors
            .iter()
            .find(|e| e.code == -32602)
            .expect("-32602 is published")
            .data_vocabulary;
        let mut emitted: Vec<&str> = Vec::new();
        for src in [
            include_str!("dispatch.rs"),
            include_str!("query.rs"),
            include_str!("invoke.rs"),
            include_str!("intervene.rs"),
            include_str!("path.rs"),
            // R1576 — `scene/displays`' own refusal. The scan's population is
            // a list, so a module emitting a new word is invisible to this
            // gate until it is added here; the round that adds the word adds
            // the file, and the reverse direction below then keeps both true.
            include_str!("displays.rs"),
        ] {
            for line in src.lines() {
                let trimmed = line.trim_start();
                // A word MENTIONED in a doc comment is not a word emitted, and
                // these modules quote most of the vocabulary in prose.
                if trimmed.starts_with("//") {
                    continue;
                }
                for opener in ["WireFault::params(\"", "Cow::Borrowed(\""] {
                    let mut rest = line;
                    while let Some(i) = rest.find(opener) {
                        rest = &rest[i + opener.len()..];
                        if let Some(end) = rest.find('"') {
                            emitted.push(&rest[..end]);
                        }
                    }
                }
                // Shape two: a word built by `format!` with the offending
                // name appended after a colon, which the -32602 entry tells
                // clients to match by prefix. Recognised by SHAPE rather than
                // by a list of the call sites: R1615 added a third
                // (`UnknownTag`) and the two hard-coded `contains` checks that
                // used to stand here would have reported it dead — a census
                // that only finds the shapes it was told about is the defect
                // this file exists to prevent one layer down.
                emitted.extend(colon_prefixed_words(line));
                // R1610 — shape three: a bare word handed straight to
                // `invalid_params`. Invisible to the two openers above, which
                // is how `InputInjectionUnavailable` reached the wire from 24
                // call sites without ever being published.
                if let Some(word) = bare_invalid_params_word(line) {
                    emitted.push(word);
                }
            }
            // R1610 — shape four: the `let variant = match err { … }` block
            // thirteen error types share. Walked structurally rather than by a
            // line pattern, because `=> "Word",` is ALSO how a dozen value
            // enums spell their wire names (`Butt`, `Center`, `Tile`), and a
            // line-local rule collects sixteen of those as refusals.
            emitted.extend(variant_block_words(src));
        }
        emitted.sort_unstable();
        emitted.dedup();
        for word in &emitted {
            assert!(
                published.iter().any(|p| p == word),
                "{word:?} reaches the wire under -32602 and is absent from the \
                 published vocabulary, so a client can only learn it by reading \
                 pinion's source",
            );
        }
        for word in published {
            assert!(
                emitted.contains(&word.as_str()),
                "{word:?} is published as a -32602 payload and nothing emits it \
                 — a dead entry is a client branch that can never be taken",
            );
        }
    }

    /// The vocabulary is a SET: sorted, so a client may binary-search it, and
    /// free of duplicates, because two entries for one word is two meanings.
    #[test]
    fn r1566_the_vocabulary_is_a_sorted_set() {
        for entry in rpc_errors().errors {
            let mut sorted = entry.data_vocabulary.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(
                sorted, entry.data_vocabulary,
                "code {} publishes an unsorted or duplicated vocabulary",
                entry.code,
            );
            assert!(
                !entry.data_is_prose || entry.data_vocabulary.is_empty(),
                "code {} says its payload is prose AND publishes words to match",
                entry.code,
            );
        }
    }

    #[test]
    fn r1616_a_word_that_names_a_closed_set_says_where_the_set_is() {
        // A refusal word is only half an answer. `UnknownLevel` tells a client
        // its spelling was wrong; the accepted spellings live in `rpc/schema`
        // as the field's `values`, and this entry is the one place a client
        // meeting the word is already reading. Without the pointer the word
        // leads to pinion's source, which is the loop R1566 exists to end.
        let entry = rpc_errors()
            .errors
            .into_iter()
            .find(|e| e.code == -32602)
            .expect("-32602 is published");
        assert!(
            entry.data_vocabulary.iter().any(|w| w == "UnknownLevel"),
            "the word is in the closed vocabulary",
        );
        assert!(
            entry.meaning.contains("rpc/schema"),
            "and the entry points at where the accepted values are published",
        );
        assert!(
            entry.meaning.contains("values"),
            "by the name of the slot that carries them, not vaguely",
        );
    }

    #[test]
    fn r1564_1_no_code_is_published_twice() {
        let mut codes: Vec<i32> = rpc_errors().errors.iter().map(|e| e.code).collect();
        let before = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(
            codes.len(),
            before,
            "a duplicate code is two meanings for one"
        );
    }
}
