//! Report acquisition: parse a report from a string / file, and the
//! `env-or-bundled` resolution seam.
//!
//! This is the answer to "how does pinion *obtain* the report?" — the
//! acquisition mechanism a consumer must pin down (the report is produced
//! elsewhere, e.g. by `mnemosyne-cli report-playable-world --json` in the
//! authoring repo). pinion stays decoupled: it consumes a JSON *document*
//! (a file the producer wrote, or a bundled sample), never invokes the
//! producer's toolchain itself. [`resolve_report`] is the one seam an
//! application wires — an env-var-pointed live report, falling back to a
//! bundled sample so the binary always runs.

use std::fmt;
use std::path::Path;

use serde::de::DeserializeOwned;

use crate::model::PlayableWorld;
use crate::place::model::PlaceGraph;

/// Failure loading or parsing a report.
#[derive(Debug)]
pub enum ReportError {
    /// The report file could not be read.
    Io(std::io::Error),
    /// The report bytes were not valid report JSON.
    Parse(serde_json::Error),
}

impl fmt::Display for ReportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "reading narrative report: {e}"),
            Self::Parse(e) => write!(f, "parsing narrative report: {e}"),
        }
    }
}

impl std::error::Error for ReportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Parse(e) => Some(e),
        }
    }
}

/// Parse a [`PlayableWorld`] from a JSON string.
///
/// Deserialize is tolerant (unknown fields ignored, missing fields
/// defaulted), so this fails only on JSON that is structurally malformed —
/// not on a schema that has drifted forward.
///
/// # Errors
///
/// Returns [`ReportError::Parse`] when `json` is not well-formed JSON.
pub fn parse_report(json: &str) -> Result<PlayableWorld, ReportError> {
    parse_json(json)
}

/// Load a [`PlayableWorld`] from a report file on disk.
///
/// # Errors
///
/// Returns [`ReportError::Io`] if the file cannot be read, or
/// [`ReportError::Parse`] if its contents are not well-formed JSON.
pub fn load_report(path: impl AsRef<Path>) -> Result<PlayableWorld, ReportError> {
    let bytes = std::fs::read_to_string(path).map_err(ReportError::Io)?;
    parse_json(&bytes)
}

/// The acquisition seam: an env-var-pointed live report, else a bundled
/// fallback.
///
/// When the environment variable `env_var` is set, its value is treated as
/// a path to a live report file and loaded — a failure there is surfaced
/// (a caller that explicitly pointed at a report wants to know it was
/// unreadable, not to be silently handed the sample). When `env_var` is
/// unset, `fallback_json` (a bundled sample) is parsed instead so the
/// binary always has a valid world to project.
///
/// # Errors
///
/// Propagates the read error when `env_var` is set but its target cannot be
/// read or parsed, or the parse error if the bundled `fallback_json` is
/// itself malformed.
pub fn resolve_report(env_var: &str, fallback_json: &str) -> Result<PlayableWorld, ReportError> {
    resolve_json(env_var, fallback_json)
}

/// Parse a [`PlaceGraph`] (spatial report) from a JSON string. Tolerant,
/// like [`parse_report`].
///
/// # Errors
///
/// Returns [`ReportError::Parse`] when `json` is not well-formed JSON.
pub fn parse_place_graph(json: &str) -> Result<PlaceGraph, ReportError> {
    parse_json(json)
}

/// The spatial acquisition seam, mirroring [`resolve_report`]: an
/// env-var-pointed live place-graph file, else a bundled fallback.
///
/// # Errors
///
/// Propagates the read error when `env_var` is set but its target cannot be
/// read or parsed, or the parse error if the bundled `fallback_json` is
/// malformed.
pub fn resolve_place_graph(env_var: &str, fallback_json: &str) -> Result<PlaceGraph, ReportError> {
    resolve_json(env_var, fallback_json)
}

/// Tolerant JSON parse — the single body shared by [`parse_report`] /
/// [`parse_place_graph`].
fn parse_json<T: DeserializeOwned>(json: &str) -> Result<T, ReportError> {
    serde_json::from_str(json).map_err(ReportError::Parse)
}

/// The env-or-bundled acquisition body shared by [`resolve_report`] /
/// [`resolve_place_graph`]: load the `env_var`-pointed file if set (errors
/// surfaced), else parse the bundled `fallback_json`.
fn resolve_json<T: DeserializeOwned>(env_var: &str, fallback_json: &str) -> Result<T, ReportError> {
    match std::env::var_os(env_var) {
        Some(path) => {
            let bytes = std::fs::read_to_string(path).map_err(ReportError::Io)?;
            parse_json(&bytes)
        }
        None => parse_json(fallback_json),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "telling": "reader",
        "fork_tree": { "branches": [], "unplaced_fork_points": [], "branch_count": 0 },
        "worlds": [ { "branch_id": "main", "scenes": [
            { "idx": 0, "title": "t", "intent": "i", "disclosures": [] }
        ] } ]
    }"#;

    #[test]
    fn parse_reads_the_documented_shape() {
        let world = parse_report(SAMPLE).expect("sample parses");
        assert_eq!(world.telling, "reader");
        assert_eq!(world.world_count(), 1);
        assert_eq!(world.scene(0, 0).map(|s| s.title.as_str()), Some("t"));
    }

    #[test]
    fn parse_tolerates_unknown_and_missing_fields() {
        // Unknown top-level + per-scene fields are ignored; absent
        // `intent` / `disclosures` / `fork_tree` default rather than error.
        let drifted = r#"{
            "telling": "reader",
            "future_field": 42,
            "worlds": [ { "branch_id": "main", "scenes": [
                { "idx": 0, "title": "only-title", "unknown_scene_field": true }
            ] } ]
        }"#;
        let world = parse_report(drifted).expect("tolerant parse");
        let scene = world.scene(0, 0).expect("scene present");
        assert_eq!(scene.title, "only-title");
        assert_eq!(scene.intent, "");
        assert!(scene.disclosures.is_empty());
        assert_eq!(world.fork_tree.branch_count, 0);
    }

    #[test]
    fn parse_rejects_malformed_json() {
        assert!(matches!(
            parse_report("{ not json"),
            Err(ReportError::Parse(_))
        ));
    }

    #[test]
    fn resolve_falls_back_to_bundled_when_env_unset() {
        // Use a variable name unlikely to be set in the test environment.
        let world = resolve_report("PINION_NARRATIVE_REPORT_TEST_UNSET_XYZ", SAMPLE)
            .expect("fallback parses");
        assert_eq!(world.telling, "reader");
    }

    #[test]
    fn parse_place_graph_reads_and_tolerates_drift() {
        let json = r#"{
            "future_field": 1,
            "places": [
                { "id": "village", "label": "마을" },
                { "id": "shrine", "label": "굿당", "contained_by": "village" }
            ],
            "adjacencies": [ { "from": "village", "to": "mudflat", "direction": "east" } ]
        }"#;
        let graph = parse_place_graph(json).expect("place graph parses");
        assert_eq!(graph.place_count(), 2);
        assert_eq!(graph.places[1].contained_by.as_deref(), Some("village"));
        assert_eq!(graph.adjacencies.len(), 1);
    }
}
