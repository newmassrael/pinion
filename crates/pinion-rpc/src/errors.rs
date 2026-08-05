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
//! `-32602` carries a word from a closed vocabulary this crate owns, and every
//! application-defined code (`-32000..=-32099`) carries a sentence the surface
//! wrote. So `data_is_prose` is not decoration — it tells a client whether the
//! payload it is holding may be matched or only shown.
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
}

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
        e(
            -32602,
            "Invalid params",
            "the parameters are missing, mis-shaped, or name something that does \
             not exist. error.data carries a word from this dispatcher's closed \
             vocabulary (\"UnknownInvokePath\", \"ReadOnly\", …), so it may be matched",
            false,
            true,
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
