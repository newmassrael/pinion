//! RPC path resolution with single-window short-circuit (§5.18, R16 slice 3).
//!
//! Schema (§5.18 outputs):
//!   /window[<id>]/<scene_path>
//! where `<id>` matches an SCE-emit `AppState` variant name. Absent prefix
//! resolves to the first declared window via `App::initial_window` — the
//! zero-parser-branch short-circuit ratified for v1 single-window compat.
//!
//! Multi-window perfect-hash dispatch on the WindowId enum lands when
//! `<parallel>` root brings N states (later R16 slice); for now reverse
//! lookup goes through `App::window_from_name` (linear under single-window).

use pinion_core::app::{App, AppState};

/// Outcome of parsing an RPC path against the SCE-emitted window topology.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedPath<'a> {
    /// Window resolved by prefix lookup, or `App::initial_window` when absent.
    pub window: AppState,
    /// Remainder of the input after `/window[id]` (or the full input when no
    /// prefix was present — §5.18 zero-branch short-circuit).
    pub scene_path: &'a str,
    /// `true` when the input carried an explicit `/window[...]/` prefix.
    pub had_explicit_prefix: bool,
}

/// Reasons path resolution can fail. Absent prefix never errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathError {
    /// Input started with `/window[` but had no closing `]`.
    MalformedPrefix,
    /// Window id between `[` and `]` was empty.
    EmptyWindowId,
    /// Id parsed cleanly but did not match any SCE-declared window.
    UnknownWindow,
}

/// Split `scene_path` at the literal `/external/` separator (§5.34 R42).
///
/// Returns `Some((scene_segments, introspect_path))` when the
/// separator is present — `scene_segments` is the chain of tag/index
/// nodes to walk from the scene root to reach the addressed
/// [`pinion_core::scene::ExternalNode`], and `introspect_path` is
/// the per-External path the §5.15 introspect surface (`query` /
/// `intervene`) consumes.
///
/// Examples (window prefix already stripped by [`resolve`]):
///
///   * `"/external/count"` → `(vec![], "count")` — scene root **is**
///     the External (existing v0 behaviour).
///   * `"/info_panel/external/count"` → `(vec!["info_panel"], "count")`
///     — walk to the tagged `info_panel` node, then descend into its
///     embedded External.
///   * `"/0/1/external/foo"` → `(vec!["0", "1"], "foo")` — index-
///     based walk.
///
/// Returns `None` when `/external/` does not appear; callers surface
/// this as `QueryError::UnsupportedPath` / `RewindError::UnsupportedPath`.
///
/// The literal `external` is reserved as the path separator; scene
/// nodes tagged `"external"` are addressable only via numeric index
/// (collision avoidance — same convention as Python's `__init__` or
/// Rust's `crate::`).
#[must_use]
pub fn split_at_external(scene_path: &str) -> Option<(Vec<String>, &str)> {
    const SEP: &str = "/external/";
    let idx = scene_path.find(SEP)?;
    let prefix = &scene_path[..idx];
    let suffix = &scene_path[idx + SEP.len()..];
    let segments = prefix
        .split('/')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    Some((segments, suffix))
}

/// Resolve an RPC path against the app-level window topology.
///
/// Absent `/window[...]/` prefix short-circuits to the initial window with
/// the input passed through verbatim as `scene_path` — no string scan, no
/// allocation. Explicit prefix triggers reverse lookup via
/// [`App::window_from_name`].
pub fn resolve(input: &str) -> Result<ResolvedPath<'_>, PathError> {
    if let Some(rest) = input.strip_prefix("/window[") {
        let close = rest.find(']').ok_or(PathError::MalformedPrefix)?;
        let id = &rest[..close];
        if id.is_empty() {
            return Err(PathError::EmptyWindowId);
        }
        let window = App::window_from_name(id).ok_or(PathError::UnknownWindow)?;
        let scene_path = &rest[close + 1..];
        Ok(ResolvedPath {
            window,
            scene_path,
            had_explicit_prefix: true,
        })
    } else {
        Ok(ResolvedPath {
            window: App::initial_window(),
            scene_path: input,
            had_explicit_prefix: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_prefix_short_circuits_to_initial_window() {
        let r = resolve("/scene/root").unwrap();
        assert_eq!(r.window, AppState::Main);
        assert_eq!(r.scene_path, "/scene/root");
        assert!(!r.had_explicit_prefix);
    }

    #[test]
    fn empty_input_short_circuits_too() {
        let r = resolve("").unwrap();
        assert_eq!(r.window, AppState::Main);
        assert_eq!(r.scene_path, "");
        assert!(!r.had_explicit_prefix);
    }

    #[test]
    fn valid_prefix_resolves_window_and_strips_suffix() {
        let r = resolve("/window[main]/scene/root").unwrap();
        assert_eq!(r.window, AppState::Main);
        assert_eq!(r.scene_path, "/scene/root");
        assert!(r.had_explicit_prefix);
    }

    #[test]
    fn prefix_only_yields_empty_scene_path() {
        let r = resolve("/window[main]").unwrap();
        assert_eq!(r.window, AppState::Main);
        assert_eq!(r.scene_path, "");
        assert!(r.had_explicit_prefix);
    }

    #[test]
    fn malformed_prefix_missing_close_bracket() {
        assert_eq!(resolve("/window[main"), Err(PathError::MalformedPrefix));
    }

    #[test]
    fn empty_window_id_rejected() {
        assert_eq!(resolve("/window[]/scene"), Err(PathError::EmptyWindowId));
    }

    #[test]
    fn unknown_window_id_rejected() {
        assert_eq!(
            resolve("/window[ghost]/scene"),
            Err(PathError::UnknownWindow)
        );
    }

    // ---- §5.34 R42: split_at_external ----

    #[test]
    fn split_at_external_root_external() {
        let (segs, intro) = split_at_external("/external/count").expect("root external");
        assert!(segs.is_empty());
        assert_eq!(intro, "count");
    }

    #[test]
    fn split_at_external_nested_external() {
        let (segs, intro) =
            split_at_external("/info_panel/external/count").expect("nested external");
        assert_eq!(segs, vec!["info_panel".to_string()]);
        assert_eq!(intro, "count");
    }

    #[test]
    fn split_at_external_deeply_nested_external() {
        let (segs, intro) =
            split_at_external("/0/1/btn/external/value").expect("deep nested external");
        assert_eq!(
            segs,
            vec!["0".to_string(), "1".to_string(), "btn".to_string()]
        );
        assert_eq!(intro, "value");
    }

    #[test]
    fn split_at_external_missing_separator_returns_none() {
        assert!(split_at_external("/some/other/path").is_none());
        assert!(split_at_external("/external").is_none(), "needs trailing slash");
        assert!(split_at_external("").is_none());
    }

    #[test]
    fn split_at_external_multi_segment_introspect_path_preserved() {
        let (segs, intro) =
            split_at_external("/panel/external/nested/slot").expect("multi-seg introspect");
        assert_eq!(segs, vec!["panel".to_string()]);
        assert_eq!(intro, "nested/slot");
    }
}
