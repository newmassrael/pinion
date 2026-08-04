//! RPC path resolution with single-window short-circuit (§5.18, R16 slice 3).
//!
//! Schema (§5.18 outputs):
//!   `/window[<id>]/<scene_path>`
//! where `<id>` matches an SCE-emit `AppState` variant name. Absent prefix
//! resolves to the first declared window via `App::initial_window` — the
//! zero-parser-branch short-circuit ratified for v1 single-window compat.
//!
//! Multi-window perfect-hash dispatch on the `WindowId` enum lands when
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathError {
    /// Input started with `/window[` but had no closing `]`.
    MalformedPrefix,
    /// Window id between `[` and `]` was empty.
    EmptyWindowId,
    /// Id parsed cleanly but did not match any SCE-declared window.
    /// Carries the offending `requested` id AND the `valid` window set, so
    /// the RPC message echoes *which* id was rejected (R1387) AND teaches
    /// *what would have worked* (R1388) — recovery from the message alone.
    /// The two syntax errors above have no such parameters. `valid` is
    /// gathered from [`App::window_names`] at resolve time (document order).
    UnknownWindow {
        /// The window id the caller sent that matched no declared window.
        requested: String,
        /// Every declared window id (the set `requested` could have been).
        valid: Vec<&'static str>,
    },
}

impl PathError {
    /// The machine-matchable reason tag carried in a failing RPC
    /// response's `error.data` — the concrete reason, not a blanket
    /// `"Path"`.
    ///
    /// This is the SSOT for the reason string: every RPC error surface
    /// that wraps a [`PathError`] (each method's `…Error::Path` arm —
    /// `scene/snapshot`, `scene/query`, `scene/invoke`, `scene/simulate`,
    /// the preview `set_signal` apply, …) forwards it, so an AI agent
    /// switches on the exact failure — a mistyped window id reads
    /// differently from a syntax slip without parsing prose. It replaces
    /// the family of `Path(_) => "Path"` mappers, each of which used to
    /// collapse all three reasons into one uninformative tag — the same
    /// concrete-reason-tag discipline the codebase's error `data` already
    /// follows elsewhere (see
    /// [`crate::preview::ApplyError::ApplyRejected`]).
    ///
    /// The tag is the variant name; [`UnknownWindow`](PathError::UnknownWindow)
    /// appends its offending id AND the valid set
    /// (`UnknownWindow: "nope" (valid: main)`) so the message names the
    /// reason, echoes what was rejected, and teaches what would work — all
    /// while staying prefix-matchable. The paramless variants borrow a
    /// `'static` string (no allocation); only the id-bearing one owns.
    #[must_use]
    pub fn wire_tag(&self) -> std::borrow::Cow<'static, str> {
        match self {
            PathError::MalformedPrefix => std::borrow::Cow::Borrowed("MalformedPrefix"),
            PathError::EmptyWindowId => std::borrow::Cow::Borrowed("EmptyWindowId"),
            PathError::UnknownWindow { requested, valid } => std::borrow::Cow::Owned(format!(
                "UnknownWindow: {requested:?} (valid: {})",
                valid.join(", ")
            )),
        }
    }
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
    Some((segments(prefix), suffix))
}

/// (R1558) Split a scene-path tail into the segment chain
/// [`Scene::lookup_path_ref`](pinion_core::Scene::lookup_path_ref) walks.
///
/// `""`, `"/"` and `"//"` all yield an empty chain — the scene root, which is
/// the addressing rule every reader here follows.
///
/// Lifted to this module when [`crate::draw_profile`] became the third site to
/// need it: [`split_at_external`] above splits its prefix this way and
/// `locate`'s reverse lookup did too, each with its own copy of the same three
/// combinators. Segment syntax is this module's subject, so a rule applied at
/// two of three sites is how two readers come to disagree about what `"//a"`
/// addresses.
#[must_use]
pub fn segments(scene_path: &str) -> Vec<String> {
    scene_path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Resolve an RPC path against the app-level window topology.
///
/// Absent `/window[...]/` prefix short-circuits to the initial window with
/// the input passed through verbatim as `scene_path` — no string scan, no
/// allocation. Explicit prefix triggers reverse lookup via
/// [`App::window_from_name`].
///
/// # The registry this consults is not the one that decides (R1558)
///
/// [`App::window_names`] is the SCE `<parallel>` topology; the windows a host
/// actually has open are the shell's `WindowSpec` slots, which is what
/// `{window: "<id>"}` is judged against and what `scene/snapshot`,
/// `scene/layout` and `scene/cache_stats` scope by. A binding can open a
/// second `WindowSpec` without a second `AppState` — `hello-multi-window`
/// does — and then this function rejects `/window[inspector]/…` as an unknown
/// window while the shell serves that very window happily.
///
/// Measured at R1558: `scene/draw_profile` publishes rows addressed
/// `/window[inspector]/…` and, before that round, refused to read them back.
/// It now takes [`split_window_prefix`] and judges the name against the live
/// slots itself.
///
/// **The other five callers still have the divergence** — `locate::bbox`,
/// `screenshot`, `snapshot`, `resolve::resolve_external_path` and
/// `layout_query` — and closing it for them is not mechanical: every one of
/// them parses the window and then **discards** it (`let _ = resolved.window`),
/// operating on the dispatch's scene instead. So the fix is not "consult the
/// other set", it is deciding what a path's window *means* to a method that
/// ignores it — either it selects the dispatch window (the unfinished R16
/// multi-window slice) or it must agree with `{window: …}` and say so. That is
/// a semantic decision across five shipped methods, and a round of its own.
///
/// # Errors
///
/// See [`PathError`]. Specifically:
/// * [`PathError::MalformedPrefix`] — input started with `/window[` but had
///   no closing `]`.
/// * [`PathError::EmptyWindowId`] — window id between `[` and `]` was empty.
/// * [`PathError::UnknownWindow`] — id parsed cleanly but did not match any
///   SCE-declared window.
pub fn resolve(input: &str) -> Result<ResolvedPath<'_>, PathError> {
    let (id, scene_path) = split_window_prefix(input)?;
    let Some(id) = id else {
        return Ok(ResolvedPath {
            window: App::initial_window(),
            scene_path,
            had_explicit_prefix: false,
        });
    };
    let window = App::window_from_name(id).ok_or_else(|| PathError::UnknownWindow {
        requested: id.to_string(),
        valid: App::window_names(),
    })?;
    Ok(ResolvedPath {
        window,
        scene_path,
        had_explicit_prefix: true,
    })
}

/// (R1558 §5.18) Split `/window[<id>]/<tail>` into the window **name** and the
/// tail — syntax only, with no registry consulted.
///
/// [`resolve`] above is this plus a lookup in the SCE-declared window
/// topology. The split exists because those are two different questions and
/// this codebase has two different answers to the second one:
///
/// * [`App::window_names`] — the windows the SCE `<parallel>` root declares.
/// * The shell's live `WindowSpec` slots, which is what `{window: "<id>"}`
///   is judged against
///   ([`unknown_window_verdict`](crate::unknown_window_verdict)), and what
///   `scene/snapshot`, `scene/layout` and `scene/cache_stats` scope by.
///
/// They are not the same set. A binding can open a second `WindowSpec` without
/// bringing a second `AppState` — `hello-multi-window` does exactly that — and
/// then `scene/draw_profile` publishes rows addressed `/window[inspector]/…`
/// that [`resolve`] rejects as an unknown window. The address vocabulary is
/// bound to the registry that does not decide.
///
/// So a caller that knows which registry governs it takes the syntax here and
/// judges the name itself. `scene/draw_profile` is the first: the window it
/// profiles is a live slot, so a live slot is what its prefix must name.
///
/// # Errors
///
/// The two SYNTAX errors, and only those — whether a name exists is not a
/// syntax question:
/// * [`PathError::MalformedPrefix`] — `/window[` with no closing `]`.
/// * [`PathError::EmptyWindowId`] — nothing between `[` and `]`.
pub fn split_window_prefix(input: &str) -> Result<(Option<&str>, &str), PathError> {
    let Some(rest) = input.strip_prefix("/window[") else {
        return Ok((None, input));
    };
    let close = rest.find(']').ok_or(PathError::MalformedPrefix)?;
    let id = &rest[..close];
    if id.is_empty() {
        return Err(PathError::EmptyWindowId);
    }
    Ok((Some(id), &rest[close + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r1558_split_window_prefix_answers_syntax_and_nothing_else() {
        // The name is handed back unjudged: this function does not know which
        // registry governs the caller, and there are two — the SCE topology
        // `resolve` consults, and the shell's live `WindowSpec` slots that
        // `{window: "<id>"}` is judged against.
        assert_eq!(
            split_window_prefix("/window[inspector]/a/b").unwrap(),
            (Some("inspector"), "/a/b"),
            "a name no `AppState` declares still parses",
        );
        assert_eq!(split_window_prefix("/a/b").unwrap(), (None, "/a/b"));
        assert_eq!(split_window_prefix("").unwrap(), (None, ""));
        assert_eq!(
            split_window_prefix("/window[main]").unwrap(),
            (Some("main"), "")
        );
        // The two syntax failures, and only those.
        assert_eq!(
            split_window_prefix("/window[main/a"),
            Err(PathError::MalformedPrefix)
        );
        assert_eq!(
            split_window_prefix("/window[]/a"),
            Err(PathError::EmptyWindowId)
        );
        // `resolve` is this plus the topology lookup — same syntax verdicts.
        assert_eq!(resolve("/window[main/a"), Err(PathError::MalformedPrefix));
        assert!(matches!(
            resolve("/window[inspector]/a"),
            Err(PathError::UnknownWindow { .. })
        ));
    }

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
        // R1387/R1388 — the rejected id AND the valid set are carried on the
        // error so the wire can echo which window was not found and which
        // ones would have worked (`main` is the sole declared window).
        assert_eq!(
            resolve("/window[ghost]/scene"),
            Err(PathError::UnknownWindow {
                requested: "ghost".to_string(),
                valid: vec!["main"],
            })
        );
    }

    #[test]
    fn wire_tag_names_the_concrete_reason() {
        // The SSOT that replaces the `Path(_) => "Path"` collapse: each
        // reason surfaces its own machine-matchable tag, never a blanket
        // "Path". A distinct tag per variant is the whole point.
        assert_eq!(PathError::MalformedPrefix.wire_tag(), "MalformedPrefix");
        assert_eq!(PathError::EmptyWindowId.wire_tag(), "EmptyWindowId");
        // R1387 echoes the offending id; R1388 also teaches the valid set,
        // all while staying prefix-matchable ("UnknownWindow: …").
        let unknown = PathError::UnknownWindow {
            requested: "nope".to_string(),
            valid: vec!["main"],
        };
        assert_eq!(unknown.wire_tag(), "UnknownWindow: \"nope\" (valid: main)");
        assert!(
            unknown.wire_tag().starts_with("UnknownWindow"),
            "the reason tag stays a machine-matchable prefix"
        );
        // A multi-window app lists every valid id in order.
        let multi = PathError::UnknownWindow {
            requested: "x".to_string(),
            valid: vec!["main", "settings"],
        };
        assert_eq!(
            multi.wire_tag(),
            "UnknownWindow: \"x\" (valid: main, settings)"
        );
        let tags = [
            PathError::MalformedPrefix.wire_tag(),
            PathError::EmptyWindowId.wire_tag(),
            unknown.wire_tag(),
        ];
        let distinct: std::collections::HashSet<_> = tags.iter().collect();
        assert_eq!(
            distinct.len(),
            tags.len(),
            "every reason has a distinct tag"
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
        assert!(
            split_at_external("/external").is_none(),
            "needs trailing slash"
        );
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
