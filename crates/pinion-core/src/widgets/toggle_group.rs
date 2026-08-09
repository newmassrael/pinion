//! `toggle_group` — the interaction substrate for a **WAI-ARIA
//! toggle-button group** (R753 §5.38): N independent toggle buttons, each
//! its own [`ToggleExternal`] and its own Tab stop, sitting under a `group`
//! parent, where any subset may be pressed at once (multi-select, no mutual
//! exclusion).
//!
//! This is the shared interaction core behind `hello-segmented-multi`
//! (R733, a joined tonal track) and `hello-filter-chip` (R753, detached M3
//! chip pills). Both consumers bind *identical* keyboard, introspection and
//! boot wiring — only their paint diverges — so the WAI-ARIA APG
//! toggle-button keyboard model, the §5.15 introspect reader and the
//! residue-free boot seed live here as one source of truth. A divergence
//! between two consumers of a *standard* a11y interaction would be a bug,
//! not a style choice, so this is lifted at the 2nd consumer (the
//! R743.1 / R745 "divergence-is-a-bug" rule), not deferred to a 3rd like an
//! opinionated paint skin.
//!
//! Paint stays per-consumer (a joined track vs detached pills are genuinely
//! different skins, not duplication). The AccessKit tree builder lives in
//! `pinion_a11y::toggle_group` because it needs `AccessNode`; this module
//! is paint-free and a11y-free pure interaction.

use crate::WidgetStateName;
use crate::external::IntrospectValue;
use crate::focus_request;
use crate::scene::Scene;
use crate::widget_core::ExtraExternal;
use crate::widgets::toggle::{ToggleEvent, ToggleExternal, ToggleState};

/// Seed a [`ToggleExternal`] to `on` without leaving a hover / pressed
/// residue: a `KeyboardActivate` flips the value Off → On while the
/// interaction state stays `Idle`, so a boot `PINION_SCREENSHOT` frame
/// paints a clean pressed pill (the R728 boot-seed lesson — seed through
/// the activate edge, never through a synthetic pointer cycle).
#[must_use]
pub fn boot_toggle(on: bool) -> ToggleExternal {
    let mut ext = ToggleExternal::new();
    if on {
        ext.send(ToggleEvent::KeyboardActivate);
    }
    ext
}

/// Build the sibling [`ExtraExternal`]s for `tags[1..]` (tag `tags[0]` is
/// the primary [`crate::WidgetCore::create_external`]). `boot[i]` seeds
/// segment `i` via [`boot_toggle`]. The substrate wraps the root as
/// `Scene::Container([primary, ...extras])`, and the input router's
/// depth-first walk dispatches each pill click by tag.
///
/// **For the binding's FIRST toggle group only.** A binding with a second
/// group wants [`toggles`]: this one drops `tags[0]` because that tag is
/// already the primary external, and calling it on a second group instead
/// produces a chip that is painted, focusable, announced to assistive
/// technology — and wired to nothing. R1625 built exactly that and the demo
/// caught it, which is why the two forms are now named apart.
///
/// # Panics
/// Panics if `boot.len() < tags.len()` (every tag needs a boot default).
#[must_use]
pub fn extra_toggles(tags: &[&'static str], boot: &[bool]) -> Vec<ExtraExternal> {
    assert!(
        boot.len() >= tags.len(),
        "every segment tag needs a boot default"
    );
    let Some((_, rest)) = tags.split_first() else {
        return Vec::new();
    };
    toggles(rest, &boot[1..])
}

/// R1625 — an [`ExtraExternal`] for **every** tag, seeded from `boot`.
///
/// The form a second toggle group needs, where no tag is the primary
/// external. [`extra_toggles`] is this with the first tag dropped.
///
/// # Panics
/// Panics if `boot.len() < tags.len()` (every tag needs a boot default).
#[must_use]
pub fn toggles(tags: &[&'static str], boot: &[bool]) -> Vec<ExtraExternal> {
    assert!(
        boot.len() >= tags.len(),
        "every segment tag needs a boot default"
    );
    tags.iter()
        .enumerate()
        .map(|(i, tag)| ExtraExternal::new(*tag, Box::new(boot_toggle(boot[i]))))
        .collect()
}

/// Read one toggle's `(interaction-state, on)` from the live scene via the
/// §5.15 introspect channel — the same path an RPC
/// `scene/query /{tag}/external/value` request walks, so a cached
/// projection and an AI client never diverge. A missing or
/// non-introspectable node reads as `(ToggleState::Idle, false)`.
#[must_use]
pub fn read_toggle(scene: &Scene, tag: &str) -> (ToggleState, bool) {
    let Some(node) = scene.find_external_with_tag(tag) else {
        return (ToggleState::Idle, false);
    };
    let Some(intro) = node.handle.introspect() else {
        return (ToggleState::Idle, false);
    };
    let state = match intro.query("state") {
        Some(IntrospectValue::Text(name)) => ToggleState::from_name_or_default(&name),
        _ => ToggleState::Idle,
    };
    let on = matches!(intro.query("value"), Some(IntrospectValue::Bool(true)));
    (state, on)
}

/// Apply a WAI-ARIA APG **toggle-button group** key to the focused
/// segment. Each segment is its own Tab stop, so `Tab` / `Shift+Tab` (the
/// shell's job) move between segments; this handles the within-group keys:
///
/// - `Space` / `Enter` — toggle the focused segment (independent, no mutual
///   exclusion);
/// - `ArrowRight` / `ArrowDown` and `ArrowLeft` / `ArrowUp` — move focus to
///   the next / previous segment, wrapping at the ends;
/// - `Home` / `End` — move focus to the first / last segment.
///
/// Returns `true` when the key was consumed. Focus moves funnel through the
/// R664 [`focus_request`] mailbox. Keys route only when one of `tags` owns
/// focus (the roving-tabindex `apply_key` discipline), so a key arriving
/// while a sibling widget is focused returns `false` untouched.
pub fn apply_key(
    scene: &mut Scene,
    focused: Option<&str>,
    key: &str,
    tags: &[&'static str],
) -> bool {
    let Some(focused_tag) = focused else {
        return false;
    };
    let Some(idx) = tags.iter().position(|t| *t == focused_tag) else {
        return false;
    };
    let n = tags.len();
    match key {
        "Space" | "Enter" => {
            let Some(node) = scene.find_external_with_tag_mut(tags[idx]) else {
                return false;
            };
            let Some(intro) = node.handle.introspect_mut() else {
                return false;
            };
            intro
                .invoke(
                    "send",
                    IntrospectValue::Text("KeyboardActivate".to_string()),
                )
                .is_ok()
        }
        "ArrowRight" | "ArrowDown" => {
            focus_request::request(tags[(idx + 1) % n]);
            true
        }
        "ArrowLeft" | "ArrowUp" => {
            focus_request::request(tags[(idx + n - 1) % n]);
            true
        }
        "Home" => {
            focus_request::request(tags[0]);
            true
        }
        "End" => {
            focus_request::request(tags[n - 1]);
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::ExternalNode;

    const TAGS: [&str; 3] = ["tg_0", "tg_1", "tg_2"];

    /// Build a fresh scene mirroring the shell's boot composition
    /// (`Scene::Container([primary, ...extras])`) so `apply_key` /
    /// `read_toggle` walk the live topology.
    fn fixture(boot: [bool; 3]) -> Scene {
        let primary =
            Scene::External(ExternalNode::new(Box::new(boot_toggle(boot[0]))).with_tag(TAGS[0]));
        let mut children = vec![primary];
        for extra in extra_toggles(&TAGS, &boot) {
            children.push(Scene::External(
                ExternalNode::new(extra.handle).with_tag(extra.tag.into_owned()),
            ));
        }
        Scene::Container(crate::scene::ContainerNode::new(children))
    }

    #[test]
    fn boot_toggle_seeds_on_without_interaction_residue() {
        let on = boot_toggle(true);
        assert!(on.is_on(), "seeded On");
        assert_eq!(on.state(), ToggleState::Idle, "no hover/pressed residue");
        let off = boot_toggle(false);
        assert!(!off.is_on());
        assert_eq!(off.state(), ToggleState::Idle);
    }

    #[test]
    fn extra_toggles_builds_siblings_for_tail_tags() {
        let extras = extra_toggles(&TAGS, &[true, false, true]);
        assert_eq!(extras.len(), TAGS.len() - 1, "tag[0] is the primary");
        assert_eq!(extras[0].tag.as_ref(), TAGS[1]);
        assert_eq!(extras[1].tag.as_ref(), TAGS[2]);
    }

    #[test]
    fn read_toggle_round_trips_boot_values() {
        let scene = fixture([true, false, true]);
        assert!(read_toggle(&scene, TAGS[0]).1, "tag 0 boots On");
        assert!(!read_toggle(&scene, TAGS[1]).1, "tag 1 boots Off");
        assert!(read_toggle(&scene, TAGS[2]).1, "tag 2 boots On");
    }

    #[test]
    fn read_toggle_missing_tag_reads_idle_off() {
        let scene = fixture([true, true, true]);
        assert_eq!(read_toggle(&scene, "nope"), (ToggleState::Idle, false));
    }

    #[test]
    fn space_toggles_only_the_focused_segment() {
        let mut scene = fixture([true, true, false]);
        assert!(apply_key(&mut scene, Some(TAGS[2]), "Space", &TAGS));
        assert!(read_toggle(&scene, TAGS[2]).1, "segment 2 toggled On");
        assert!(read_toggle(&scene, TAGS[0]).1, "segment 0 untouched");
        assert!(read_toggle(&scene, TAGS[1]).1, "segment 1 untouched");
    }

    #[test]
    fn enter_is_independent_not_exclusive() {
        let mut scene = fixture([true, true, false]);
        assert!(apply_key(&mut scene, Some(TAGS[0]), "Enter", &TAGS));
        assert!(
            !read_toggle(&scene, TAGS[0]).1,
            "Enter toggled segment 0 Off"
        );
        assert!(
            read_toggle(&scene, TAGS[1]).1,
            "segment 1 stays On — not exclusive"
        );
    }

    #[test]
    fn arrows_rove_with_wrap() {
        let mut scene = fixture([false, false, false]);
        let _ = focus_request::drain();
        assert!(apply_key(&mut scene, Some(TAGS[2]), "ArrowRight", &TAGS));
        assert_eq!(
            focus_request::drain().as_deref(),
            Some(TAGS[0]),
            "wraps last -> first"
        );
        assert!(apply_key(&mut scene, Some(TAGS[0]), "ArrowLeft", &TAGS));
        assert_eq!(
            focus_request::drain().as_deref(),
            Some(TAGS[2]),
            "wraps first -> last"
        );
    }

    #[test]
    fn home_and_end_jump_to_ends() {
        let mut scene = fixture([false, false, false]);
        let _ = focus_request::drain();
        assert!(apply_key(&mut scene, Some(TAGS[1]), "Home", &TAGS));
        assert_eq!(focus_request::drain().as_deref(), Some(TAGS[0]));
        assert!(apply_key(&mut scene, Some(TAGS[1]), "End", &TAGS));
        assert_eq!(focus_request::drain().as_deref(), Some(TAGS[2]));
    }

    #[test]
    fn keys_ignored_when_no_segment_focused() {
        let mut scene = fixture([false, false, false]);
        let _ = focus_request::drain();
        assert!(!apply_key(&mut scene, None, "Space", &TAGS));
        assert!(!apply_key(&mut scene, Some("sibling"), "ArrowRight", &TAGS));
        assert_eq!(focus_request::drain(), None, "no focus request emitted");
    }
    /// R1625 — the two builders differ by exactly one tag, and the
    /// difference is the whole reason both exist. Nothing checked it before,
    /// which is how a second toggle group ended up with a dead chip.
    #[test]
    fn r1625_toggles_keeps_every_tag_and_extra_toggles_drops_the_primary() {
        const TAGS: [&str; 3] = ["a", "b", "c"];
        const BOOT: [bool; 3] = [true, false, true];
        let all = toggles(&TAGS, &BOOT);
        assert_eq!(all.len(), TAGS.len(), "every tag gets an external");
        let extras = extra_toggles(&TAGS, &BOOT);
        assert_eq!(
            extras.len(),
            TAGS.len() - 1,
            "the primary tag is already the widget's own external",
        );
        assert_eq!(toggles(&[], &[]).len(), 0);
        assert_eq!(
            extra_toggles(&[], &[]).len(),
            0,
            "and an empty group is empty"
        );
    }
}
