//! `rpc/methods` — the self-describing wire surface (R1089 §5.7 §5.12 §2 #7).
//!
//! Every other dispatch method is reachable only by an AI that already
//! knows its literal string — there was no way to ASK the wire what it
//! offers. `rpc/methods` is that meta-method: it returns the catalog of
//! method names the dispatcher routes, so an agent DISCOVERS the surface
//! (`scene/window_move`, `scene/windows`, …) instead of needing each
//! literal baked in. The §2 #7 scene-as-data principle applied to the
//! protocol itself.
//!
//! **Catalog SSOT + no drift.** [`RPC_METHODS`] is the public surface; the
//! `catalog_matches_dispatch_match_arms` test parses the `dispatch_parsed`
//! routing match (the actual router) out of the source and asserts
//! set-equality, so a method added to the match without a catalog entry
//! (or vice-versa) fails CI. The catalog is verified-complete against the
//! router without macro magic or runtime reflection.
//!
//! **MVP scope (names only).** This first cut returns names; per-method
//! kind (read/mutate) and param schema are the natural next slice, added
//! when a consumer needs them ([[abstraction-needs-second-consumer]]).

use serde::{Deserialize, Serialize};

/// Every method name the `dispatch_parsed` routing match handles — the
/// SSOT public wire surface, sorted. Kept exactly in sync with the router
/// by the `catalog_matches_dispatch_match_arms` test (parses the match
/// arms out of the dispatch source and asserts set-equality), so adding a
/// method to the match without a catalog entry fails the test.
pub const RPC_METHODS: &[&str] = &[
    "focus/get",
    "focus/next",
    "focus/prev",
    "focus/set",
    "font/cmap_subtables",
    "font/dispose",
    "font/family_name",
    "font/full_name",
    "font/glyph_id_for",
    "font/glyph_outline",
    "font/list",
    "font/metrics",
    "font/parse",
    "font/postscript_name",
    "font/subfamily_name",
    "rpc/methods",
    "scene/access",
    "scene/animate_cancel",
    "scene/animate_settle",
    "scene/animation_state",
    "scene/apply_preview",
    "scene/bbox",
    "scene/cache_stats",
    "scene/cancel_preview",
    "scene/caret_state",
    "scene/click",
    "scene/commands",
    "scene/double_click",
    "scene/drag",
    "scene/drop_file",
    "scene/dry_run",
    "scene/export_pdf",
    "scene/frame_timings",
    "scene/hover",
    "scene/hover_file",
    "scene/hover_file_cancel",
    "scene/input_state",
    "scene/intents",
    "scene/intervene",
    "scene/invoke",
    "scene/key",
    "scene/layout",
    "scene/list_previews",
    "scene/locate",
    "scene/locate_region",
    "scene/modifiers",
    "scene/pacing_state",
    "scene/pointer_leave",
    "scene/propose_change",
    "scene/query",
    "scene/render_fidelity",
    "scene/resize",
    "scene/rewind",
    "scene/screenshot",
    "scene/scroll",
    "scene/scroll_state",
    "scene/set_caret",
    "scene/set_fps",
    "scene/set_scroll_offset",
    "scene/set_selection",
    "scene/set_text",
    "scene/set_theme_mode",
    "scene/set_theme_palettes",
    "scene/simulate",
    "scene/snapshot",
    "scene/text_state",
    "scene/theme_tokens",
    "scene/tick",
    "scene/wheel",
    "scene/window_move",
    "scene/windows",
    "text/normalize",
];

/// Response payload for `rpc/methods`: the discoverable method catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RpcMethods {
    /// Every dispatch method name, sorted (the [`RPC_METHODS`] catalog).
    /// Owned `String`s rather than `&'static str` so the response is
    /// `Deserialize` (round-trippable), matching the `ResizeOutcome` /
    /// `WindowMoveOutcome` convention; the catalog read is rare so the
    /// 72 short allocations are immaterial.
    pub methods: Vec<String>,
    /// `methods.len()` — a convenience count so a client need not re-count.
    pub count: usize,
}

/// Build the `rpc/methods` response from the [`RPC_METHODS`] catalog.
#[must_use]
pub fn rpc_methods() -> RpcMethods {
    RpcMethods {
        methods: RPC_METHODS.iter().map(|s| (*s).to_owned()).collect(),
        count: RPC_METHODS.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse the `"<ns>/<method>" =>` arms out of the `dispatch_parsed`
    /// routing match and return them sorted+deduped. The arms are the only
    /// lines that begin (after trimming) with a quoted `ns/method` literal
    /// immediately followed by `=>` (verified: no other `"x/y" =>` pattern
    /// exists in the file), so this is a reliable extraction without a
    /// regex dependency.
    fn dispatch_match_arms() -> Vec<String> {
        let src = include_str!("dispatch.rs");
        let mut arms: Vec<String> = Vec::new();
        for line in src.lines() {
            let line = line.trim_start();
            let Some(rest) = line.strip_prefix('"') else {
                continue;
            };
            let Some(close) = rest.find('"') else {
                continue;
            };
            let name = &rest[..close];
            let after = rest[close + 1..].trim_start();
            if after.starts_with("=>")
                && name.contains('/')
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_' || c == '/')
            {
                arms.push(name.to_owned());
            }
        }
        arms.sort();
        arms.dedup();
        arms
    }

    #[test]
    fn catalog_matches_dispatch_match_arms() {
        let from_source = dispatch_match_arms();
        let catalog: Vec<String> = RPC_METHODS.iter().map(|s| (*s).to_owned()).collect();
        assert_eq!(
            from_source, catalog,
            "rpc/methods catalog drifted from the dispatch routing match \
             (add the new method to RPC_METHODS, or remove the stale entry)"
        );
    }

    #[test]
    fn catalog_is_sorted_and_unique() {
        let mut sorted = RPC_METHODS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.as_slice(),
            RPC_METHODS,
            "RPC_METHODS must be sorted + duplicate-free"
        );
    }

    #[test]
    fn catalog_lists_itself() {
        // `rpc/methods` is a routed method, so the discovery surface must
        // include itself — an agent can confirm the meta-method exists.
        assert!(RPC_METHODS.contains(&"rpc/methods"));
    }

    #[test]
    fn response_count_matches_methods_len() {
        let resp = rpc_methods();
        assert_eq!(resp.count, resp.methods.len());
        assert_eq!(resp.count, RPC_METHODS.len());
    }
}
