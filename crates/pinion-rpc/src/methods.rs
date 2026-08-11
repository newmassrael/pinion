//! `rpc/methods` — the method-DISCOVERY wire surface (R1089 §5.7 §5.12 §2 #7;
//! R1090 per-method OCC class; R1091 audit-clearance).
//!
//! Every other dispatch method is reachable only by an AI that already
//! knows its literal string — there was no way to ASK the wire what it
//! offers. `rpc/methods` is that meta-method: it returns the catalog of
//! methods the dispatcher routes, so an agent DISCOVERS the surface
//! (`scene/window_move`, `scene/windows`, …) instead of needing each
//! literal baked in. The §2 #7 scene-as-data principle applied to the
//! protocol itself.
//!
//! **Discovery, not yet full self-description.** This lists method NAMES +
//! their `occ`, but NOT param schemas — so an AI learns a method EXISTS
//! without learning how to CALL it (the param shape is still out-of-band).
//! Per-method param schema is the next slice; until then this is a method
//! directory, not a round-trippable protocol description.
//!
//! **Per-method `occ` (R1090).** Each entry carries its [`MethodOcc`] — the
//! method's `SceneRevision` optimistic-concurrency class, mirroring the
//! `HandlerKind` the routing match tags it with. **This is NOT a
//! side-effect-freedom flag.** `occ: "read"` means only "the dispatcher
//! does NOT bump the [`pinion_core::SceneRevision`] OCC token after this handler"
//! — it still includes methods that genuinely change state: deferred input
//! (`scene/key`, `scene/wheel` — the mutation lands a frame later), async
//! effects (`scene/resize`, `scene/window_move` — the move lands on
//! reconcile), out-of-OCC state (`focus/*`), and self-bumping handlers
//! (`scene/apply_preview`). `occ: "mutate"` means the dispatcher bumps the
//! OCC token synchronously on `Ok`. So `occ` answers "do I need to re-read
//! the revision after calling this" (optimistic-concurrency), NOT "is this
//! call free of effects". To keep that caveat ON THE WIRE — not just in
//! this rustdoc the AI never sees — the `rpc/methods` response carries an
//! [`OCC_DOC`] legend string; the field name `occ` (not `kind`) reinforces
//! it. (R1091 fix: the prior doc claimed the distinction was "on the wire"
//! when it lived only in source.)
//!
//! **Catalog vs. router — a source cross-check (not a type guarantee).**
//! [`RPC_METHODS`] is the public surface; the
//! `catalog_matches_dispatch_match_arms` test parses the `dispatch_parsed`
//! routing match out of the source — scoped to the
//! `match request.method.as_str()` region, comment-stripped, with a
//! camelCase-inclusive name charset — and asserts set-equality on BOTH name
//! AND `occ` (each arm's `HandlerKind`), so a method added/retagged without
//! a catalog edit fails CI. This is a SOURCE-TEXT check: robust for the
//! current single-match router, but a future refactor that splits the match
//! or renames the scrutinee would need the parser re-audited (a macro
//! generating arm + entry together would be the structural guarantee, but
//! macro-generated arms are a deliberate readability non-goal). R1091 fixed
//! a real instance — a lowercase-only name filter silently dropped the
//! camelCase `scene/waitFor` from BOTH the parser and the catalog while the
//! cross-check stayed green over an incomplete surface.
//!
//! **Per-method `window` (R1585).** Each entry also carries how a window is
//! named TO THAT METHOD — [`MethodWindow`], with [`WINDOW_DOC`] as its legend.
//! There are two spellings and they mean different things (`params.window` is
//! the dispatch scope, `/window[<id>]/` is a path prefix), the difference was
//! published nowhere, and the cost is on the record: R1581 tried
//! `window[main]/scene/access`, met a bare `-32601`, and registered a debt
//! against a capability that had been there all along. `dispatch_parsed`'s
//! unknown-method arm now reads this catalog and CORRECTS that spelling.
//!
//! The column is **proven, not parsed**. `r1585_the_window_column_is_what_the_methods_actually_do`
//! probes every catalogued method with a malformed prefix and compares the
//! published class with the behaviour. A source census cannot answer it:
//! measured while building this, a call graph keyed by bare function name
//! merges four different `fn parse` — `displays.rs`, `draw_profile.rs` twice,
//! `font.rs` — and credits `font/parse` and `scene/displays` with reading a
//! window prefix neither of them sees.
//!
//! **MVP scope.** Names + `occ` + `window`. Per-method param schema is the
//! natural next slice, added when a consumer needs it
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
    /// The dispatcher bumps the [`pinion_core::SceneRevision`] OCC token
    /// synchronously after the handler returns `Ok` (immediate
    /// revision-tracked scene mutation).
    Mutate,
}

/// How a window is named **to one method** (R1585).
///
/// There are two spellings and they mean different things, which is the whole
/// reason this is published: `params.window` names the dispatch **scope** —
/// whose state answers — and `/window[<id>]/…` is a prefix on a scene **path**,
/// naming a place in the tree. A method NAME never carries either, and the one
/// time that was assumed it produced a bare `-32601` and a debt registered
/// against a capability that was there all along.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MethodWindow {
    /// The dispatch scope only. Every method takes this — see [`WINDOW_DOC`].
    Scope,
    /// The scope **and** a `/window[<id>]/` prefix on this method's path
    /// argument. The two are independent: the scope says whose state is being
    /// read, the prefix says which window's subtree the path runs through.
    Path,
}

/// The `window` legend, carried on the wire beside the catalog for the same
/// reason [`OCC_DOC`] is: a field name cannot hold a distinction, and the
/// rustdoc explaining it is not something an agent reads.
pub const WINDOW_DOC: &str = "window says how a window is named TO THIS METHOD. \
     'scope' — only params.window, the dispatch scope, which every method \
     accepts: it is type-checked and gated against the live window set BEFORE \
     routing, so an unknown window is refused whatever the method. 'path' — \
     that, AND a /window[<id>]/ prefix on this method's path argument, which \
     addresses a subtree rather than a scope. A METHOD NAME never carries \
     either spelling: 'window[main]/scene/access' is not a method.";

/// Every method the `dispatch_parsed` routing match handles, paired with
/// its [`MethodOcc`] and its [`MethodWindow`] — the SSOT public wire surface,
/// sorted by name. Kept
/// exactly in sync with the router by `catalog_matches_dispatch_match_arms`
/// (parses the match arms + their `HandlerKind` out of the dispatch source
/// and asserts set-equality), so adding/retagging a method without updating
/// this catalog fails the test.
pub const RPC_METHODS: &[(&str, MethodOcc, MethodWindow)] = &[
    ("app/quit", MethodOcc::Mutate, MethodWindow::Scope),
    ("focus/get", MethodOcc::Read, MethodWindow::Scope),
    ("focus/next", MethodOcc::Read, MethodWindow::Scope),
    ("focus/prev", MethodOcc::Read, MethodWindow::Scope),
    ("focus/set", MethodOcc::Read, MethodWindow::Scope),
    ("font/cmap_subtables", MethodOcc::Read, MethodWindow::Scope),
    ("font/dispose", MethodOcc::Read, MethodWindow::Scope),
    ("font/family_name", MethodOcc::Read, MethodWindow::Scope),
    ("font/full_name", MethodOcc::Read, MethodWindow::Scope),
    ("font/glyph_id_for", MethodOcc::Read, MethodWindow::Scope),
    ("font/glyph_outline", MethodOcc::Read, MethodWindow::Scope),
    ("font/list", MethodOcc::Read, MethodWindow::Scope),
    ("font/metrics", MethodOcc::Read, MethodWindow::Scope),
    ("font/parse", MethodOcc::Read, MethodWindow::Scope),
    ("font/postscript_name", MethodOcc::Read, MethodWindow::Scope),
    ("font/subfamily_name", MethodOcc::Read, MethodWindow::Scope),
    ("rpc/errors", MethodOcc::Read, MethodWindow::Scope),
    ("rpc/methods", MethodOcc::Read, MethodWindow::Scope),
    ("rpc/schema", MethodOcc::Read, MethodWindow::Scope),
    ("scene/accelerators", MethodOcc::Read, MethodWindow::Scope),
    ("scene/access", MethodOcc::Read, MethodWindow::Scope),
    (
        "scene/animate_cancel",
        MethodOcc::Mutate,
        MethodWindow::Scope,
    ),
    (
        "scene/animate_settle",
        MethodOcc::Mutate,
        MethodWindow::Scope,
    ),
    (
        "scene/animation_state",
        MethodOcc::Read,
        MethodWindow::Scope,
    ),
    ("scene/apply_preview", MethodOcc::Read, MethodWindow::Scope),
    ("scene/auto_repeat", MethodOcc::Read, MethodWindow::Scope),
    ("scene/bbox", MethodOcc::Read, MethodWindow::Path),
    ("scene/cache_stats", MethodOcc::Read, MethodWindow::Scope),
    ("scene/cancel_preview", MethodOcc::Read, MethodWindow::Scope),
    ("scene/caret_state", MethodOcc::Read, MethodWindow::Scope),
    ("scene/cell_editors", MethodOcc::Read, MethodWindow::Scope),
    ("scene/click", MethodOcc::Mutate, MethodWindow::Scope),
    ("scene/commands", MethodOcc::Read, MethodWindow::Scope),
    (
        "scene/cross_window_drop",
        MethodOcc::Read,
        MethodWindow::Scope,
    ),
    ("scene/derivations", MethodOcc::Read, MethodWindow::Scope),
    ("scene/disabled", MethodOcc::Read, MethodWindow::Scope),
    ("scene/displays", MethodOcc::Read, MethodWindow::Scope),
    ("scene/double_click", MethodOcc::Mutate, MethodWindow::Scope),
    ("scene/drag", MethodOcc::Mutate, MethodWindow::Scope),
    ("scene/draw_profile", MethodOcc::Read, MethodWindow::Path),
    ("scene/drop_file", MethodOcc::Mutate, MethodWindow::Scope),
    ("scene/dry_run", MethodOcc::Read, MethodWindow::Path),
    ("scene/export_pdf", MethodOcc::Read, MethodWindow::Scope),
    ("scene/frame_timings", MethodOcc::Read, MethodWindow::Scope),
    ("scene/grid_editors", MethodOcc::Read, MethodWindow::Scope),
    ("scene/hover", MethodOcc::Mutate, MethodWindow::Scope),
    ("scene/hover_file", MethodOcc::Mutate, MethodWindow::Scope),
    (
        "scene/hover_file_cancel",
        MethodOcc::Mutate,
        MethodWindow::Scope,
    ),
    ("scene/input_state", MethodOcc::Read, MethodWindow::Scope),
    ("scene/intents", MethodOcc::Read, MethodWindow::Scope),
    ("scene/intervene", MethodOcc::Read, MethodWindow::Path),
    ("scene/invoke", MethodOcc::Mutate, MethodWindow::Path),
    ("scene/key", MethodOcc::Read, MethodWindow::Scope),
    ("scene/layout", MethodOcc::Read, MethodWindow::Path),
    ("scene/list_previews", MethodOcc::Read, MethodWindow::Scope),
    ("scene/locate", MethodOcc::Read, MethodWindow::Scope),
    ("scene/locate_region", MethodOcc::Read, MethodWindow::Scope),
    ("scene/marks", MethodOcc::Read, MethodWindow::Scope),
    ("scene/memory", MethodOcc::Read, MethodWindow::Scope),
    ("scene/mnemonics", MethodOcc::Read, MethodWindow::Scope),
    ("scene/modifiers", MethodOcc::Mutate, MethodWindow::Scope),
    ("scene/pacing_state", MethodOcc::Read, MethodWindow::Scope),
    ("scene/pan_gesture", MethodOcc::Read, MethodWindow::Scope),
    ("scene/pinch_gesture", MethodOcc::Read, MethodWindow::Scope),
    (
        "scene/pointer_button",
        MethodOcc::Mutate,
        MethodWindow::Scope,
    ),
    (
        "scene/pointer_height",
        MethodOcc::Mutate,
        MethodWindow::Scope,
    ),
    (
        "scene/pointer_leave",
        MethodOcc::Mutate,
        MethodWindow::Scope,
    ),
    (
        "scene/pointer_pressure",
        MethodOcc::Mutate,
        MethodWindow::Scope,
    ),
    ("scene/pointer_reach", MethodOcc::Read, MethodWindow::Scope),
    (
        "scene/pointer_tangential_pressure",
        MethodOcc::Mutate,
        MethodWindow::Scope,
    ),
    ("scene/pointer_tilt", MethodOcc::Mutate, MethodWindow::Scope),
    (
        "scene/pointer_twist",
        MethodOcc::Mutate,
        MethodWindow::Scope,
    ),
    ("scene/pointer_type", MethodOcc::Mutate, MethodWindow::Scope),
    ("scene/propose_change", MethodOcc::Read, MethodWindow::Scope),
    ("scene/query", MethodOcc::Read, MethodWindow::Path),
    (
        "scene/render_fidelity",
        MethodOcc::Read,
        MethodWindow::Scope,
    ),
    ("scene/resize", MethodOcc::Read, MethodWindow::Scope),
    ("scene/revision", MethodOcc::Read, MethodWindow::Scope),
    ("scene/rewind", MethodOcc::Mutate, MethodWindow::Path),
    (
        "scene/rotation_gesture",
        MethodOcc::Read,
        MethodWindow::Scope,
    ),
    ("scene/screenshot", MethodOcc::Read, MethodWindow::Path),
    ("scene/scroll", MethodOcc::Read, MethodWindow::Scope),
    ("scene/scroll_state", MethodOcc::Read, MethodWindow::Scope),
    ("scene/set_caret", MethodOcc::Mutate, MethodWindow::Scope),
    ("scene/set_fps", MethodOcc::Mutate, MethodWindow::Scope),
    (
        "scene/set_scroll_offset",
        MethodOcc::Mutate,
        MethodWindow::Scope,
    ),
    (
        "scene/set_selection",
        MethodOcc::Mutate,
        MethodWindow::Scope,
    ),
    ("scene/set_text", MethodOcc::Mutate, MethodWindow::Scope),
    (
        "scene/set_theme_mode",
        MethodOcc::Mutate,
        MethodWindow::Scope,
    ),
    (
        "scene/set_theme_palettes",
        MethodOcc::Mutate,
        MethodWindow::Scope,
    ),
    ("scene/simulate", MethodOcc::Read, MethodWindow::Path),
    (
        "scene/smart_zoom_gesture",
        MethodOcc::Read,
        MethodWindow::Scope,
    ),
    ("scene/snapshot", MethodOcc::Read, MethodWindow::Path),
    ("scene/subscribe", MethodOcc::Read, MethodWindow::Scope),
    ("scene/subscriptions", MethodOcc::Read, MethodWindow::Scope),
    (
        "scene/text_backgrounds",
        MethodOcc::Read,
        MethodWindow::Scope,
    ),
    ("scene/text_blocks", MethodOcc::Read, MethodWindow::Scope),
    (
        "scene/text_cache_stats",
        MethodOcc::Read,
        MethodWindow::Scope,
    ),
    ("scene/text_lists", MethodOcc::Read, MethodWindow::Scope),
    ("scene/text_painted", MethodOcc::Read, MethodWindow::Scope),
    ("scene/text_state", MethodOcc::Read, MethodWindow::Scope),
    ("scene/text_tables", MethodOcc::Read, MethodWindow::Scope),
    ("scene/theme_tokens", MethodOcc::Read, MethodWindow::Scope),
    ("scene/tick", MethodOcc::Mutate, MethodWindow::Scope),
    ("scene/type", MethodOcc::Read, MethodWindow::Scope),
    ("scene/unsubscribe", MethodOcc::Read, MethodWindow::Scope),
    ("scene/waitFor", MethodOcc::Read, MethodWindow::Path),
    ("scene/wheel", MethodOcc::Read, MethodWindow::Scope),
    ("scene/window_declare", MethodOcc::Read, MethodWindow::Scope),
    ("scene/window_focus", MethodOcc::Read, MethodWindow::Scope),
    ("scene/window_move", MethodOcc::Read, MethodWindow::Scope),
    ("scene/windows", MethodOcc::Read, MethodWindow::Scope),
    ("text/normalize", MethodOcc::Read, MethodWindow::Scope),
];

/// Wire legend for the [`MethodEntry::occ`] field, carried in every
/// `rpc/methods` response so the "`read` is NOT side-effect-free" caveat
/// reaches a wire-only consumer (an AI reading the JSON), not just a reader
/// of this crate's source.
pub const OCC_DOC: &str = "occ is the SceneRevision optimistic-concurrency class, NOT a side-effect flag: \
     'mutate' bumps the OCC revision token synchronously; 'read' does not — but 'read' \
     still includes methods that change state (deferred input, async effects like \
     scene/window_move, focus, self-bumping), so re-read state after any call you are \
     unsure about. occ answers 'must I re-read the revision', not 'is this call inert'.";

/// One catalog entry on the wire: a method name + its [`MethodOcc`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MethodEntry {
    /// The method name (`"<ns>/<method>"`).
    pub name: String,
    /// The `SceneRevision` OCC class — see [`MethodOcc`]. NOT an effect flag.
    pub occ: MethodOcc,
    /// How a window is named to this method — see [`MethodWindow`] and
    /// [`WINDOW_DOC`].
    pub window: MethodWindow,
}

/// Response payload for `rpc/methods`: the discoverable method catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RpcMethods {
    /// Every dispatch method + its `occ`, sorted by name (the
    /// [`RPC_METHODS`] catalog).
    pub methods: Vec<MethodEntry>,
    /// `methods.len()` — a convenience count so a client need not re-count.
    pub count: usize,
    /// The [`OCC_DOC`] legend — the `occ`-semantics caveat carried ON THE
    /// WIRE so a JSON consumer does not misread `occ: "read"` as "inert".
    /// Owned (not `&'static str`) so the response stays `Deserialize`,
    /// matching `MethodEntry`.
    pub occ_doc: String,
    /// The [`WINDOW_DOC`] legend — the two window spellings, on the wire
    /// rather than in a rustdoc an agent never reads.
    pub window_doc: String,
}

/// Build the `rpc/methods` response from the [`RPC_METHODS`] catalog.
#[must_use]
pub fn rpc_methods() -> RpcMethods {
    RpcMethods {
        methods: RPC_METHODS
            .iter()
            .map(|(name, occ, window)| MethodEntry {
                name: (*name).to_owned(),
                occ: *occ,
                window: *window,
            })
            .collect(),
        count: RPC_METHODS.len(),
        occ_doc: OCC_DOC.to_owned(),
        window_doc: WINDOW_DOC.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Strip a trailing `//` line comment so `HandlerKind::…` mentioned in
    /// prose (e.g. the `scene/apply_preview` arm's explanatory comment)
    /// cannot be mistaken for the arm's real kind tag.
    fn strip_comment(line: &str) -> &str {
        line.split("//").next().unwrap_or(line)
    }

    /// The `occ` a source line's CODE (comment stripped) tags via its
    /// `HandlerKind`, if any.
    fn line_occ(line: &str) -> Option<MethodOcc> {
        let code = strip_comment(line);
        if code.contains("HandlerKind::Mutate") {
            Some(MethodOcc::Mutate)
        } else if code.contains("HandlerKind::Read") {
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
            // Arm start: a line beginning with `"ns/method" =>`. The name
            // charset is ALPHANUMERIC (camelCase included, e.g.
            // `scene/waitFor`) + `_` + `/` — a structural check, NOT a
            // lowercase filter. R1091: the prior `is_ascii_lowercase`
            // filter did double duty (scoping AND silent exclusion), so a
            // camelCase arm was dropped from BOTH the parser and the
            // catalog and the cross-check passed green over an incomplete
            // surface. The charset now admits every routable arm.
            if let Some(rest) = t.strip_prefix('"') {
                if let Some(close) = rest.find('"') {
                    let name = &rest[..close];
                    let after = rest[close + 1..].trim_start();
                    if after.starts_with("=>")
                        && name.contains('/')
                        && name
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '/')
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
            .map(|(n, occ, _)| ((*n).to_owned(), *occ))
            .collect();
        assert_eq!(
            from_source, catalog,
            "rpc/methods catalog drifted from the dispatch routing match \
             (name set OR an occ/HandlerKind disagrees — update RPC_METHODS)"
        );
    }

    #[test]
    fn catalog_is_sorted_and_unique_by_name() {
        let names: Vec<&str> = RPC_METHODS.iter().map(|(n, ..)| *n).collect();
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
            RPC_METHODS.contains(&("rpc/methods", MethodOcc::Read, MethodWindow::Scope)),
            "rpc/methods must list itself (occ read)"
        );
    }

    #[test]
    fn occ_is_honest_not_an_effect_flag() {
        // The documented trap: these methods DO change state but are
        // `occ: Read` (deferred / async / out-of-OCC), so a consumer must
        // not read `Read` as "side-effect-free".
        for effecting_read in [
            "scene/window_declare",     // async — the patch lands on reconcile
            "scene/window_move",        // async — move lands on reconcile
            "scene/window_focus",       // out-of-band OS-focus gate/mirror drive
            "scene/key",                // deferred input
            "scene/wheel",              // deferred input
            "scene/pinch_gesture",      // deferred input
            "scene/rotation_gesture",   // deferred input
            "scene/pan_gesture",        // deferred input
            "scene/smart_zoom_gesture", // deferred input
            "scene/resize",             // async resize
            "focus/set",                // focus is out of OCC scope
            "scene/apply_preview",      // self-bumping
        ] {
            assert_eq!(
                RPC_METHODS
                    .iter()
                    .find(|(n, ..)| *n == effecting_read)
                    .map(|(_, occ, _)| *occ),
                Some(MethodOcc::Read),
                "{effecting_read} is occ Read (does not bump OCC) despite changing state"
            );
        }
        // A synchronous revision-bumping mutate, for contrast.
        assert_eq!(
            RPC_METHODS
                .iter()
                .find(|(n, ..)| *n == "scene/set_text")
                .map(|(_, occ, _)| *occ),
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

    #[test]
    fn camelcase_method_is_catalogued() {
        // R1091 M1 regression: `scene/waitFor` is a routed camelCase method
        // (§5.12 wait-for). A lowercase-only parser+catalog silently dropped
        // it while the cross-check passed green. It must now be present —
        // `catalog_matches_dispatch_match_arms` would also fail if absent,
        // but this pins the specific camelCase case so it can't regress
        // silently again.
        assert!(
            RPC_METHODS
                .iter()
                .any(|(n, occ, _)| *n == "scene/waitFor" && *occ == MethodOcc::Read),
            "the camelCase scene/waitFor must be in the catalog (M1 blind spot)"
        );
        assert!(
            RPC_METHODS
                .iter()
                .any(|(n, ..)| n.chars().any(|c| c.is_ascii_uppercase())),
            "at least one camelCase method exists; the parser must not exclude it"
        );
    }

    #[test]
    fn comment_mentioning_handlerkind_does_not_bind_occ() {
        // R1091 S3: a `// … HandlerKind::Mutate …` comment must NOT be read
        // as a kind tag. strip_comment removes it before line_occ.
        assert_eq!(
            line_occ("    // contrast: HandlerKind::Mutate would bump here"),
            None
        );
        assert_eq!(
            line_occ("        (handle(), HandlerKind::Read), // not Mutate"),
            Some(MethodOcc::Read)
        );
    }

    #[test]
    fn response_carries_occ_legend_on_the_wire() {
        // R1091 Finding 1: the occ caveat must ride ON THE WIRE, not only in
        // rustdoc. The legend names the trap explicitly.
        let resp = rpc_methods();
        assert_eq!(resp.occ_doc, OCC_DOC);
        assert!(resp.occ_doc.contains("NOT a side-effect flag"));
        assert!(resp.occ_doc.contains("read"));
        let json = serde_json::to_value(&resp).unwrap();
        assert!(
            json.get("occ_doc").and_then(|v| v.as_str()).is_some(),
            "occ_doc must serialize onto the wire"
        );
    }
}
