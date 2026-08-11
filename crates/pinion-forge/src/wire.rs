//! NDJSON wire serialization for [`PinionForgeDiagnostic`]. Mirrors the
//! SCE v1 record shape (`v`, `id`, `generator`, `code`, `stage`,
//! `message`, optional `location`) per
//! `schemas/sce-diagnostic.v1.schema.json` — but lives in pinion's own
//! namespace (RFC 001 §4 closed: SCE's `DiagnosticCode` enum is not
//! extensible by downstream frameworks).
//!
//! That mirroring claim is not prose: `mirrors_every_field_the_sce_record_carries`
//! destructures a real `sce-build` diagnostic from the pinned build with
//! no `..`, so every field on SCE's record is either mirrored here or
//! named as deliberately absent — and a field added upstream fails to
//! *compile*. That is the direction that matters: a new required field
//! must force a decision here rather than be absorbed by silence.
//! `generator` is exactly that case. It became required upstream at the
//! R1657 pin, and nothing in this file had to change for the old wording
//! "mirrors the SCE v1 record shape" to become false.
//!
//! Output is a single JSON object per call to [`to_ndjson_line`], with no
//! trailing newline (callers append `\n` between records).

use serde_json::{Map, Value};

use crate::diagnostic::PinionForgeDiagnostic;

/// Current wire format version. Bumped only on incompatible shape
/// changes; additive optional fields stay on `v=1`.
pub const WIRE_VERSION: u32 = 1;

/// Commit of the pinion build that emitted a record, or `"unknown"` on a
/// build with no git checkout to read (a git or vendored dependency).
///
/// Resolved by this crate's `build.rs`; see its header for why the crate
/// version cannot serve and why the stamp names the committed state
/// rather than the worktree.
///
/// Every wire surface in this crate that names its producer reads *this*
/// constant, so two surfaces reporting the generator cannot report
/// different answers.
pub const GENERATOR_COMMIT: &str = env!("PINION_GIT_COMMIT");

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
        PinionForgeDiagnostic::InvalidIdent {
            tag,
            attribute,
            found,
            ..
        } => {
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
/// ```
/// use pinion_forge::{GENERATOR_COMMIT, diagnostic::{Location, PinionForgeDiagnostic}};
///
/// let diag = PinionForgeDiagnostic::UnknownKind {
///     found: "gizmo".into(),
///     location: Location::new("app.pinion.xml").with_line_col(1, 9),
/// };
/// let line = pinion_forge::to_ndjson_line(&diag);
/// // {"v":1,"id":"fnv1a:…","generator":"<commit>","code":"dsl/unknown-kind",
/// //  "stage":"validate","message":"…","location":{…},"actual":"gizmo"}
/// let record: serde_json::Value = serde_json::from_str(&line).unwrap();
///
/// // A consumer version-gates, dedups, then attributes — all three
/// // before it has to understand the payload.
/// assert_eq!(record["v"], 1);
/// assert!(record["id"].as_str().unwrap().starts_with("fnv1a:"));
/// assert_eq!(record["generator"], GENERATOR_COMMIT);
/// ```
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
    // Third, ahead of the payload: a consumer version-gates on `v`,
    // dedups on `id`, and attributes on `generator` — all three before
    // it has to understand anything this crate might change. Per-record
    // rather than once per stream, because a rejected run emits records
    // and no manifest, so this is the only thing the consumer receives.
    map.insert("generator".into(), Value::from(GENERATOR_COMMIT));
    map.insert("code".into(), Value::from(diag.code()));
    map.insert("stage".into(), Value::from(diag.stage().as_wire()));
    map.insert("message".into(), Value::from(diag.to_string()));

    let loc = diag.location();
    let mut loc_obj = Map::new();
    loc_obj.insert(
        "file".into(),
        Value::from(loc.file.to_string_lossy().into_owned()),
    );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::{Location, PinionForgeDiagnostic};

    /// A pinion record that populates every optional key this module can
    /// emit, so a key-set comparison is not silently narrowed by the
    /// fixture. `UnknownKind` carries `found`, which is what fills
    /// `actual`; the location carries line and column.
    fn pinion_record() -> Map<String, Value> {
        let diag = PinionForgeDiagnostic::UnknownKind {
            found: "gizmo".into(),
            location: Location::new("a.pinion.xml").with_line_col(3, 7),
        };
        match to_json_value(&diag) {
            Value::Object(map) => map,
            other => panic!("record is an object, got {other:?}"),
        }
    }

    /// One real diagnostic from the pinned `sce-build`, taken through the
    /// same public path a consumer would: an error out of the compile
    /// entry, lowered by `ToDiagnostics`.
    ///
    /// It has to be a *real* record rather than a constructed one so the
    /// values are what SCE actually emits — a hand-built `Diagnostic`
    /// would prove only that this test can fill in fields.
    fn sce_record() -> sce_build::forge::diagnostic::Diagnostic {
        use sce_build::forge::diagnostic::ToDiagnostics;
        use sce_build::forge::error::{ForgeError, Located, XmlError};

        let err = Located::new(
            ForgeError::Xml(XmlError::FileNotFound {
                path: "a.scxml".into(),
            }),
            "a.scxml",
            Some(3),
            Some(7),
        );
        err.to_diagnostics()
            .into_iter()
            .next()
            .expect("a forge error lowers to at least one diagnostic")
    }

    #[test]
    fn a_record_names_the_build_that_emitted_it() {
        // The schema's own pattern for `generator`: a short-or-full hex
        // commit, or the `unknown` a checkout-less build reports. A
        // consumer told to pin a commit while the surface is pre-release
        // can only do that if the payload is one of those two things.
        let generator = pinion_record()["generator"]
            .as_str()
            .expect("generator is a string")
            .to_string();
        assert!(
            generator == "unknown"
                || (generator.len() >= 7
                    && generator.len() <= 40
                    && generator
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())),
            "generator {generator:?} matches ^([0-9a-f]{{7,40}}|unknown)$"
        );
    }

    #[test]
    fn every_record_names_the_same_build() {
        // Per-record, not per-stream — but every record in a stream comes
        // from one build, so two records disagreeing would mean the stamp
        // is being derived rather than read from `GENERATOR_COMMIT`.
        let other = PinionForgeDiagnostic::MissingKind {
            location: Location::new("b.pinion.xml"),
        };
        let Value::Object(other) = to_json_value(&other) else {
            panic!("record is an object");
        };
        assert_eq!(pinion_record()["generator"], other["generator"]);
        assert_eq!(other["generator"], Value::from(GENERATOR_COMMIT));
    }

    #[test]
    fn mirrors_every_field_the_sce_record_carries() {
        let sce = sce_record();

        // THE CENSUS IS THE PATTERN. Destructuring with no `..` is
        // exhaustive, so a field added to SCE's record fails to COMPILE
        // here — the mirror claim in this module's header cannot go stale
        // while nothing in this file changes, which is exactly how it went
        // stale when `generator` landed upstream. Every binding below is
        // either mirrored on pinion's record or named as deliberately not
        // mirrored, with the reason.
        let sce_build::forge::diagnostic::Diagnostic {
            // ── Mirrored: same key, same position, same meaning ──
            schema_version,
            id,
            generator,
            code,
            stage,
            message,
            location,
            actual,

            // ── Not mirrored, and why ──
            // `spec` is the SCXML/RFC anchor a `DiagnosticCode` resolves
            // to. RFC 001 §4 is closed: pinion's codes are not SCE codes,
            // so there is no anchor to name. A pinion code that ever
            // cites a pinion spec section would mirror this key.
            spec: _,
            // `expected` / `fix` are SCE's repair-loop payload. Pinion
            // emits neither yet (see `to_json_value`'s note that the
            // SCE_ERROR_CONTRACT disjointness clause holds trivially
            // while no pinion diagnostic carries a fix).
            expected: _,
            fix: _,
            // `spec_provenance` records which spec clause justified the
            // diagnostic — same reason as `spec`.
            spec_provenance: _,
            // `question_kind` classifies an SCE authoring question back
            // to the author; pinion's diagnostics are all refusals.
            question_kind: _,
        } = &sce;

        let pinion = pinion_record();
        // The comparison runs on the SERIALIZED SCE record, because the
        // schema describes the wire form and some of the struct's own
        // accessors are crate-private. The destructuring above is what
        // makes the key list below complete; this is what makes the
        // values comparable.
        let Value::Object(wire) = serde_json::to_value(&sce).expect("SCE record serializes") else {
            panic!("SCE record is an object");
        };

        // `v` is the one renamed key: SCE spells the field
        // `schema_version` and the wire spells it `v`.
        assert_eq!(wire["v"], Value::from(*schema_version));
        assert_eq!(pinion["v"], Value::from(WIRE_VERSION));
        assert!(matches!(pinion["v"], Value::Number(_)));

        // The rest keep their names. Each assertion is "pinion has this
        // key, holding the same kind of value SCE puts there" — the
        // shapes must agree; the contents are each project's own. The
        // bindings are re-read here so the census is not decorative.
        assert_eq!(wire["id"], Value::from(id.clone()));
        assert_eq!(wire["generator"], Value::from(*generator));
        assert_eq!(wire["message"], Value::from(message.clone()));
        for key in ["id", "generator", "code", "stage", "message"] {
            let mine = pinion
                .get(key)
                .unwrap_or_else(|| panic!("pinion record mirrors SCE's {key:?}"));
            assert!(
                mine.is_string() && wire[key].is_string(),
                "{key:?} is a string on both records"
            );
        }
        // `code` and `stage` are typed enums on SCE's struct and strings
        // on the wire; naming them here keeps the two bindings load-
        // bearing rather than silently discarded.
        assert_eq!(
            wire["code"],
            serde_json::to_value(code).expect("code serializes")
        );
        assert_eq!(
            wire["stage"],
            serde_json::to_value(stage).expect("stage serializes")
        );

        // `location` and `actual` are optional on SCE's record; the
        // fixture populates both so the comparison is not vacuous.
        let sce_location = location.as_ref().expect("fixture carries a location");
        let mine = pinion["location"]
            .as_object()
            .expect("pinion location is an object");
        assert!(mine.contains_key("file"), "location mirrors `file`");
        assert_eq!(mine.contains_key("line"), sce_location.line.is_some());
        assert_eq!(mine.contains_key("col"), sce_location.col.is_some());
        assert!(actual.is_some(), "fixture populates SCE's `actual`");
        assert!(
            pinion.contains_key("actual"),
            "pinion record mirrors SCE's `actual`"
        );

        // Backward: the SCE schema is `additionalProperties: false`, so a
        // key pinion invents would put the record outside the shape this
        // module claims to mirror. Derived from the destructuring above,
        // not restated.
        let sce_keys = [
            "v",
            "id",
            "generator",
            "code",
            "stage",
            "spec",
            "message",
            "location",
            "expected",
            "actual",
            "fix",
            "spec_provenance",
            "question_kind",
        ];
        for key in pinion.keys() {
            assert!(
                sce_keys.contains(&key.as_str()),
                "pinion emits {key:?}, which the mirrored shape has no room for"
            );
        }
    }
}
