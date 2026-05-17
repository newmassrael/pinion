//! NDJSON wire serialization for [`PinionForgeDiagnostic`]. Mirrors the
//! SCE v1 record shape (`v`, `id`, `code`, `stage`, `message`,
//! optional `location`) per `schemas/sce-diagnostic.v1.schema.json` — but
//! lives in pinion's own namespace (RFC 001 §4 closed: SCE's
//! `DiagnosticCode` enum is not extensible by downstream frameworks).
//!
//! Output is a single JSON object per call to [`to_ndjson_line`], with no
//! trailing newline (callers append `\n` between records).

use serde_json::{Map, Value};

use crate::diagnostic::PinionForgeDiagnostic;

/// Current wire format version. Bumped only on incompatible shape
/// changes; additive optional fields stay on `v=1`.
pub const WIRE_VERSION: u32 = 1;

/// FNV-1a 64-bit hash. Inlined to keep the dependency surface zero — the
/// algorithm is fixed (RFC 8484 reference constants) and any future
/// migration to a different hash family changes the `id` prefix, not the
/// in-tree hash routine.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(PRIME);
    }
    h
}

/// Content-addressed diagnostic id. Hash inputs are the closed identity
/// fragments: `(code, stage, file, key_fragments)`. Message text is
/// deliberately excluded so re-wording does not shift the id.
fn diagnostic_id(diag: &PinionForgeDiagnostic) -> String {
    let key_fragments = key_fragments(diag);
    let file_str = diag.location().file.to_string_lossy();
    let mut hash_input = String::with_capacity(
        diag.code().len() + diag.stage().as_wire().len() + file_str.len() + key_fragments.len() + 3,
    );
    hash_input.push_str(diag.code());
    hash_input.push('\u{1f}');
    hash_input.push_str(diag.stage().as_wire());
    hash_input.push('\u{1f}');
    hash_input.push_str(&file_str);
    hash_input.push('\u{1f}');
    hash_input.push_str(&key_fragments);
    format!("fnv1a:{:016x}", fnv1a_64(hash_input.as_bytes()))
}

/// Variant-specific identity fragments — the values that *make this
/// diagnostic this diagnostic* beyond (code, stage, file). E.g. the
/// offending tag name for `UnsupportedElement`. Order matters and is
/// part of the wire contract; appending new fragments in a backwards-
/// compatible way is fine, reordering is a wire break.
fn key_fragments(diag: &PinionForgeDiagnostic) -> String {
    match diag {
        PinionForgeDiagnostic::InvalidRoot { found, .. }
        | PinionForgeDiagnostic::WrongXmlns { found, .. }
        | PinionForgeDiagnostic::UnknownKind { found, .. }
        | PinionForgeDiagnostic::InvalidName { found, .. }
        | PinionForgeDiagnostic::UnknownBackend { found, .. }
        | PinionForgeDiagnostic::UnknownAa { found, .. } => found.clone(),
        PinionForgeDiagnostic::UnsupportedElement { tag, .. }
        | PinionForgeDiagnostic::EmptyBody { tag, .. }
        | PinionForgeDiagnostic::RendererChildNotAllowed { tag, .. } => tag.clone(),
        PinionForgeDiagnostic::MissingAttribute { tag, attribute, .. } => {
            format!("{tag}\u{1f}{attribute}")
        }
        PinionForgeDiagnostic::InvalidIdent { tag, attribute, found, .. } => {
            format!("{tag}\u{1f}{attribute}\u{1f}{found}")
        }
        PinionForgeDiagnostic::XmlParseError { .. }
        | PinionForgeDiagnostic::MissingXmlns { .. }
        | PinionForgeDiagnostic::MissingKind { .. }
        | PinionForgeDiagnostic::MissingName { .. }
        | PinionForgeDiagnostic::MissingBackend { .. } => String::new(),
    }
}

/// Serialize a single diagnostic to a JSON line (no trailing newline).
/// Callers that emit a stream append `\n` between records.
///
/// # Panics
/// In theory `serde_json::to_string` is fallible (the `Serialize` impl
/// could fail), but the input here is always a `serde_json::Value`
/// constructed in this module from finite owned strings/integers — that
/// path has no fallible branches. The `.expect` documents the invariant
/// and would only fire if the standard library's `Value` `Serialize`
/// impl regressed.
#[must_use]
pub fn to_ndjson_line(diag: &PinionForgeDiagnostic) -> String {
    let value = to_json_value(diag);
    serde_json::to_string(&value).expect("Value::serialize cannot fail for owned input")
}

/// Lower a diagnostic to a `serde_json::Value` matching the wire schema.
/// Exposed primarily for testability (asserts can match on the structured
/// value rather than re-parsing a line of NDJSON).
#[must_use]
pub fn to_json_value(diag: &PinionForgeDiagnostic) -> Value {
    let mut map = Map::new();
    map.insert("v".into(), Value::from(WIRE_VERSION));
    map.insert("id".into(), Value::from(diagnostic_id(diag)));
    map.insert("code".into(), Value::from(diag.code()));
    map.insert("stage".into(), Value::from(diag.stage().as_wire()));
    map.insert("message".into(), Value::from(diag.to_string()));

    let loc = diag.location();
    let mut loc_obj = Map::new();
    loc_obj.insert("file".into(), Value::from(loc.file.to_string_lossy().into_owned()));
    if let Some(line) = loc.line {
        loc_obj.insert("line".into(), Value::from(line));
    }
    if let Some(col) = loc.column {
        loc_obj.insert("col".into(), Value::from(col));
    }
    map.insert("location".into(), Value::Object(loc_obj));

    // Variant-specific `actual` field, surfaced for agent dispatch when
    // present. Disjoint from `fix` per SCE_ERROR_CONTRACT.md §3.2 — at
    // R38.1 no diagnostic carries a fix proposal, so the contract holds
    // trivially.
    if let Some(actual) = actual_of(diag) {
        map.insert("actual".into(), Value::from(actual));
    }

    Value::Object(map)
}

fn actual_of(diag: &PinionForgeDiagnostic) -> Option<String> {
    match diag {
        PinionForgeDiagnostic::InvalidRoot { found, .. }
        | PinionForgeDiagnostic::WrongXmlns { found, .. }
        | PinionForgeDiagnostic::UnknownKind { found, .. }
        | PinionForgeDiagnostic::InvalidName { found, .. }
        | PinionForgeDiagnostic::InvalidIdent { found, .. }
        | PinionForgeDiagnostic::UnknownBackend { found, .. }
        | PinionForgeDiagnostic::UnknownAa { found, .. } => Some(found.clone()),
        PinionForgeDiagnostic::UnsupportedElement { tag, .. }
        | PinionForgeDiagnostic::RendererChildNotAllowed { tag, .. } => Some(tag.clone()),
        _ => None,
    }
}
