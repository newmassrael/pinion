//! `rpc/methods` — the self-describing wire surface (R1089 §5.7 §5.12 §2 #7;
//! R1090 adds the per-method OCC class).
//!
//! Every other dispatch method is reachable only by an AI that already
//! knows its literal string — there was no way to ASK the wire what it
//! offers. `rpc/methods` is that meta-method: it returns the catalog of
//! methods the dispatcher routes, so an agent DISCOVERS the surface
//! (`scene/window_move`, `scene/windows`, …) instead of needing each
//! literal baked in. The §2 #7 scene-as-data principle applied to the
//! protocol itself.
//!
//! **Per-method `occ` (R1090).** Each entry carries its [`MethodOcc`] — the
//! method's `SceneRevision` optimistic-concurrency class, mirroring the
//! `HandlerKind` the routing match tags it with. **This is NOT a
//! side-effect-freedom flag.** `occ: "read"` means only "the dispatcher
//! does NOT bump the [`crate::SceneRevision`] OCC token after this handler"
//! — it still includes methods that genuinely change state: deferred input
//! (`scene/key`, `scene/wheel` — the mutation lands a frame later), async
//! effects (`scene/resize`, `scene/window_move` — the move lands on
//! reconcile), out-of-OCC state (`focus/*`), and self-bumping handlers
//! (`scene/apply_preview`). `occ: "mutate"` means the dispatcher bumps the
//! OCC token synchronously on `Ok`. So `occ` answers "do I need to re-read
//! the revision after calling this" (optimistic-concurrency), NOT "is this
//! call free of effects". The honest field name (`occ`, not `kind`) keeps
//! that distinction on the wire, not just in this doc.
//!
//! **Catalog SSOT + no drift.** [`RPC_METHODS`] is the public surface; the
//! `catalog_matches_dispatch_match_arms` test parses the `dispatch_parsed`
//! routing match (the actual router) out of the source — scoped to the
//! `match request.method.as_str()` region — and asserts set-equality on
//! BOTH name AND `occ` (each arm's `HandlerKind`), so a method added to the
//! match, or an `occ` that drifts from its arm, fails CI. The catalog is
//! verified-complete against the router without macro magic or runtime
//! reflection.
//!
//! **MVP scope.** Names + `occ`. Per-method param schema is the natural
//! next slice, added when a consumer needs it
//! ([[abstraction-needs-second-consumer]]).

use serde::{Deserialize, Serialize};

/// A method's `SceneRevision` optimistic-concurrency class — see the module
/// docs. **Not** a side-effect flag: `Read` includes deferred / async /
/// out-of-OCC / self-bumping methods that DO change state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MethodOcc {
    /// The dispatcher does NOT bump the OCC token after this handler
    /// (pure reads, but also deferred-input / async-effect / `focus/*` /
    /// self-bumping handlers).
    Read,
    /// The dispatcher bumps the [`crate::SceneRevision`] OCC token
    /// synchronously after the handler returns `Ok` (immediate
    /// revision-tracked scene mutation).
    Mutate,
}

/// Every method the `dispatch_parsed` routing match handles, paired with
/// its [`MethodOcc`] — the SSOT public wire surface, sorted by name. Kept
/// exactly in sync with the router by `catalog_matches_dispatch_match_arms`
/// (parses the match arms + their `HandlerKind` out of the dispatch source
/// and asserts set-equality), so adding/retagging a method without updating
/// this catalog fails the test.
pub const RPC_METHODS: &[(&str, MethodOcc)] = &[
    ("focus/get", MethodOcc::Read),
    ("focus/next", MethodOcc::Read),
    ("focus/prev", MethodOcc::Read),
    ("focus/set", MethodOcc::Read),
    ("font/cmap_subtables", MethodOcc::Read),
    ("font/dispose", MethodOcc::Read),
    ("font/family_name", MethodOcc::Read),
    ("font/full_name", MethodOcc::Read),
    ("font/glyph_id_for", MethodOcc::Read),
    ("font/glyph_outline", MethodOcc::Read),
    ("font/list", MethodOcc::Read),
    ("font/metrics", MethodOcc::Read),
    ("font/parse", MethodOcc::Read),
    ("font/postscript_name", MethodOcc::Read),
    ("font/subfamily_name", MethodOcc::Read),
    ("rpc/methods", MethodOcc::Read),
    ("scene/access", MethodOcc::Read),
    ("scene/animate_cancel", MethodOcc::Mutate),
    ("scene/animate_settle", MethodOcc::Mutate),
    ("scene/animation_state", MethodOcc::Read),
    ("scene/apply_preview", MethodOcc::Read),
    ("scene/bbox", MethodOcc::Read),
    ("scene/cache_stats", MethodOcc::Read),
    ("scene/cancel_preview", MethodOcc::Read),
    ("scene/caret_state", MethodOcc::Read),
    ("scene/click", MethodOcc::Mutate),
    ("scene/commands", MethodOcc::Read),
    ("scene/double_click", MethodOcc::Mutate),
    ("scene/drag", MethodOcc::Mutate),
    ("scene/drop_file", MethodOcc::Mutate),
    ("scene/dry_run", MethodOcc::Read),
    ("scene/export_pdf", MethodOcc::Read),
    ("scene/frame_timings", MethodOcc::Read),
    ("scene/hover", MethodOcc::Mutate),
    ("scene/hover_file", MethodOcc::Mutate),
    ("scene/hover_file_cancel", MethodOcc::Mutate),
    ("scene/input_state", MethodOcc::Read),
    ("scene/intents", MethodOcc::Read),
    ("scene/intervene", MethodOcc::Read),
    ("scene/invoke", MethodOcc::Mutate),
    ("scene/key", MethodOcc::Read),
    ("scene/layout", MethodOcc::Read),
    ("scene/list_previews", MethodOcc::Read),
    ("scene/locate", MethodOcc::Read),
    ("scene/locate_region", MethodOcc::Read),
    ("scene/modifiers", MethodOcc::Mutate),
    ("scene/pacing_state", MethodOcc::Read),
    ("scene/pointer_leave", MethodOcc::Mutate),
    ("scene/propose_change", MethodOcc::Read),
    ("scene/query", MethodOcc::Read),
    ("scene/render_fidelity", MethodOcc::Read),
    ("scene/resize", MethodOcc::Read),
    ("scene/rewind", MethodOcc::Mutate),
    ("scene/screenshot", MethodOcc::Read),
    ("scene/scroll", MethodOcc::Read),
    ("scene/scroll_state", MethodOcc::Read),
    ("scene/set_caret", MethodOcc::Mutate),
    ("scene/set_fps", MethodOcc::Mutate),
    ("scene/set_scroll_offset", MethodOcc::Mutate),
    ("scene/set_selection", MethodOcc::Mutate),
    ("scene/set_text", MethodOcc::Mutate),
    ("scene/set_theme_mode", MethodOcc::Mutate),
    ("scene/set_theme_palettes", MethodOcc::Mutate),
    ("scene/simulate", MethodOcc::Read),
    ("scene/snapshot", MethodOcc::Read),
    ("scene/text_state", MethodOcc::Read),
    ("scene/theme_tokens", MethodOcc::Read),
    ("scene/tick", MethodOcc::Mutate),
    ("scene/wheel", MethodOcc::Read),
    ("scene/window_move", MethodOcc::Read),
    ("scene/windows", MethodOcc::Read),
    ("text/normalize", MethodOcc::Read),
];

/// One catalog entry on the wire: a method name + its [`MethodOcc`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MethodEntry {
    /// The method name (`"<ns>/<method>"`).
    pub name: String,
    /// The `SceneRevision` OCC class — see [`MethodOcc`]. NOT an effect flag.
    pub occ: MethodOcc,
}

/// Response payload for `rpc/methods`: the discoverable method catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RpcMethods {
    /// Every dispatch method + its `occ`, sorted by name (the
    /// [`RPC_METHODS`] catalog).
    pub methods: Vec<MethodEntry>,
    /// `methods.len()` — a convenience count so a client need not re-count.
    pub count: usize,
}

/// Build the `rpc/methods` response from the [`RPC_METHODS`] catalog.
#[must_use]
pub fn rpc_methods() -> RpcMethods {
    RpcMethods {
        methods: RPC_METHODS
            .iter()
            .map(|(name, occ)| MethodEntry {
                name: (*name).to_owned(),
                occ: *occ,
            })
            .collect(),
        count: RPC_METHODS.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `occ` a source line tags (its `HandlerKind`), if any.
    fn line_occ(line: &str) -> Option<MethodOcc> {
        if line.contains("HandlerKind::Mutate") {
            Some(MethodOcc::Mutate)
        } else if line.contains("HandlerKind::Read") {
            Some(MethodOcc::Read)
        } else {
            None
        }
    }

    /// Parse the `"<ns>/<method>" =>` arms (+ each arm's `HandlerKind`) out
    /// of the `dispatch_parsed` routing match and return them sorted by
    /// name. Scoped to the `match request.method.as_str()` region (up to
    /// the `_ =>` default arm) so `HandlerKind` tokens in handler bodies /
    /// the enum doc / the test module never leak in. A single-line arm
    /// carries its kind inline; a block arm's kind is the next
    /// `HandlerKind::…` before the following arm.
    fn dispatch_match_arms() -> Vec<(String, MethodOcc)> {
        let src = include_str!("dispatch.rs");
        let mut in_match = false;
        let mut pending: Option<String> = None;
        let mut arms: Vec<(String, MethodOcc)> = Vec::new();
        for line in src.lines() {
            let t = line.trim_start();
            if t.contains("match request.method.as_str()") {
                in_match = true;
                continue;
            }
            if !in_match {
                continue;
            }
            if t.starts_with("_ =>") {
                break; // default arm — end of the named-method region
            }
            // Arm start: a line beginning with `"ns/method" =>`.
            if let Some(rest) = t.strip_prefix('"') {
                if let Some(close) = rest.find('"') {
                    let name = &rest[..close];
                    let after = rest[close + 1..].trim_start();
                    if after.starts_with("=>")
                        && name.contains('/')
                        && name
                            .chars()
                            .all(|c| c.is_ascii_lowercase() || c == '_' || c == '/')
                    {
                        match line_occ(t) {
                            Some(occ) => arms.push((name.to_owned(), occ)), // single-line arm
                            None => pending = Some(name.to_owned()),        // block arm
                        }
                        continue;
                    }
                }
            }
            // A block arm's `HandlerKind::…` tag closes the pending method.
            if let Some(occ) = line_occ(t) {
                if let Some(name) = pending.take() {
                    arms.push((name, occ));
                }
            }
        }
        arms.sort_by(|a, b| a.0.cmp(&b.0));
        arms
    }

    #[test]
    fn catalog_matches_dispatch_match_arms() {
        let from_source = dispatch_match_arms();
        let catalog: Vec<(String, MethodOcc)> = RPC_METHODS
            .iter()
            .map(|(n, occ)| ((*n).to_owned(), *occ))
            .collect();
        assert_eq!(
            from_source, catalog,
            "rpc/methods catalog drifted from the dispatch routing match \
             (name set OR an occ/HandlerKind disagrees — update RPC_METHODS)"
        );
    }

    #[test]
    fn catalog_is_sorted_and_unique_by_name() {
        let names: Vec<&str> = RPC_METHODS.iter().map(|(n, _)| *n).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted, names,
            "RPC_METHODS must be sorted by name + duplicate-free"
        );
    }

    #[test]
    fn catalog_lists_itself_as_read() {
        // `rpc/methods` is a routed read method, so the discovery surface
        // must include itself.
        assert!(
            RPC_METHODS.contains(&("rpc/methods", MethodOcc::Read)),
            "rpc/methods must list itself (occ read)"
        );
    }

    #[test]
    fn occ_is_honest_not_an_effect_flag() {
        // The documented trap: these methods DO change state but are
        // `occ: Read` (deferred / async / out-of-OCC), so a consumer must
        // not read `Read` as "side-effect-free".
        for effecting_read in [
            "scene/window_move",   // async — move lands on reconcile
            "scene/key",           // deferred input
            "scene/wheel",         // deferred input
            "scene/resize",        // async resize
            "focus/set",           // focus is out of OCC scope
            "scene/apply_preview", // self-bumping
        ] {
            assert_eq!(
                RPC_METHODS
                    .iter()
                    .find(|(n, _)| *n == effecting_read)
                    .map(|(_, occ)| *occ),
                Some(MethodOcc::Read),
                "{effecting_read} is occ Read (does not bump OCC) despite changing state"
            );
        }
        // A synchronous revision-bumping mutate, for contrast.
        assert_eq!(
            RPC_METHODS
                .iter()
                .find(|(n, _)| *n == "scene/set_text")
                .map(|(_, occ)| *occ),
            Some(MethodOcc::Mutate),
        );
    }

    #[test]
    fn response_count_matches_methods_len() {
        let resp = rpc_methods();
        assert_eq!(resp.count, resp.methods.len());
        assert_eq!(resp.count, RPC_METHODS.len());
        assert_eq!(resp.methods[0].name, RPC_METHODS[0].0);
    }
}
