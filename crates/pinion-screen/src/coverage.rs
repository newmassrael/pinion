//! ★★★★★ R1724 — **the census that keeps mounting total.**
//!
//! A [`Screen`](crate::Screen) hook with no default stops
//! [`Mount`](crate::Mount) compiling until it is forwarded. Nothing
//! covers the other direction: a hook added to
//! [`WidgetCore`](pinion_core::WidgetCore),
//! [`WidgetA11y`](pinion_a11y::WidgetA11y) or
//! [`WidgetView`](pinion_shell::WidgetView) and never mirrored here would leave
//! every mounted screen silently missing a behaviour it declares — the exact
//! shape of defect this tree keeps finding, where a screen publishes a rule it
//! does not keep.
//!
//! So the mirror is measured against the source of the three traits rather than
//! against a list somebody remembered to update. Each of their hooks must be
//! one of three things, and the test names which:
//!
//! * mirrored on [`Screen`](crate::Screen) under the same name,
//! * mirrored under a different name because several hooks fold into one
//!   ([`FOLDED`]), or
//! * pinned as the host's rather than a screen's, **with the reason**
//!   ([`WINDOW_LEVEL`]).
//!
//! There is no fourth answer, and adding a hook to any of the three traits
//! turns the census red until somebody gives one.
//!
//! # ★★★★★ R1888 — and the guarantee's own escape hatch is data now
//!
//! The sentence this header opened with was *"[`Screen`](crate::Screen) has no
//! default methods"*, stated as an absolute in two files. Measured at R1888 by
//! reading the trait's own source: **it had been false since R1808**, which
//! defaulted [`Screen::poses`](crate::Screen::poses) and
//! [`Screen::pose`](crate::Screen::pose) — with a good reason, written in their
//! own doc, and with neither header updated. R1888 then defaulted a third hook
//! before catching itself.
//!
//! A default is precisely the compile-time guarantee above being waived, so it
//! is the thing this module exists to make impossible to do quietly.
//! [`DEFAULTED`] names every one and why; an unlisted default fails, and so
//! does a listed hook that has stopped being defaulted. And because the waived
//! guarantee was *`Mount` cannot forget to forward it*, each listed hook must
//! be shown to be forwarded anyway — by reading `mount.rs`, since the compiler
//! no longer answers.

/// Hooks of the binding traits that are the *application's* and not a page's,
/// each with the reason it is not on [`Screen`](crate::Screen).
///
/// A pin is a decision, so it carries its justification here rather than in a
/// commit message: a reader deciding whether a new hook belongs on `Screen`
/// needs to see what kind of thing was ruled out before.
///
/// ★★★★★ **One entry was here and was wrong, and running the thing is what
/// said so.** `shrink_policy` was pinned as the window's — *"what a window
/// concedes to get smaller is the window's declaration; a page inside it does
/// not get a second one"* — and then the first real mount measured the
/// consequence: the node lab, whose layout stops reflowing at 1625 wide,
/// placed in a 1388-wide region, painted its inspector from x=1365 to x=1677
/// while the window ends at 1440. **51 of its regions were outside the
/// rectangle it was placed in**, and the pane a person configures a node with
/// was off the screen entirely.
///
/// The screen had already said what to do about that — `Recourse::Pan` — and
/// the pin is what stopped the region hearing it. A shrink policy is not a
/// statement about a window; it is a statement about **what this content needs
/// and what it concedes when it does not get it**, and whatever is showing the
/// content owes it the recourse. So it is a [`Screen`](crate::Screen) method,
/// and [`ScreenRoster::page_scene`](crate::ScreenRoster::page_scene) applies
/// `pinion_core::shrink::pan` for the same reason the shell applies it to a
/// window.
pub const WINDOW_LEVEL: &[(&str, &str)] = &[
    (
        "initial_size_strategy",
        "how big the window opens is asked once, before any journey exists",
    ),
    (
        "quit_on_last_window_closed",
        "an application-lifetime decision — a page cannot hold an opinion \
         about what closing the last window means",
    ),
    (
        "app_quit_requested",
        "the application asking to end, which outlives whichever page is \
         showing at the time",
    ),
    (
        "external_set_is_dynamic",
        "a host that mounts screens is dynamic by construction: its surface \
         set IS the current screen's, so the answer is never a page's to give",
    ),
];

/// Hooks that are mirrored under a different name because several of them
/// fold into one screen-level question.
///
/// `(binding hook, the `Screen` method it folds into, why)`.
pub const FOLDED: &[(&str, &str, &str)] = &[
    (
        "primary_surface",
        "externals",
        "an application assembled from screens has no primary surface of its \
         own; which of a screen's surfaces is primary is a fact about a \
         binding that fills a window",
    ),
    (
        "create_external",
        "externals",
        "the primary surface is the head of the screen's surface list",
    ),
    (
        "create_extra_externals",
        "externals",
        "and the extras are its tail, in the order the binding declared them",
    ),
    (
        "read_state",
        "latch",
        "a screen's projection cannot travel in the host's `Copy` state, so \
         reading it and parking it are one call",
    ),
    (
        "event_name",
        "keybinding",
        "a typed `Event` cannot cross a roster of differently-typed screens; \
         the name it would be turned into is what crosses instead",
    ),
];

/// ★★★★★ R1888 — the [`Screen`](crate::Screen) hooks that carry a **default
/// body**, each with the reason, because a default is this module's own
/// guarantee being waived for that hook.
///
/// `(hook, why a default is right for it)`.
///
/// # Why an enumerated exception rather than a rule
///
/// Because the alternative was tried and it silently stopped being true. Two
/// module headers asserted *`Screen` defaults nothing* as an absolute; R1808
/// defaulted two hooks for a reason it wrote into their own documentation and
/// updated neither header; nothing anywhere performed the sentence, so it read
/// as settled for eighty rounds while being false.
///
/// The entries below are the reason, not a permission. What each one is
/// weighing is the same trade in both directions: requiring an answer costs
/// every implementor a line that says nothing, and defaulting costs the
/// mechanical proof that [`Mount`](crate::Mount) forwards it. A hook where the
/// second matters more belongs here; a hook that exists to make a section
/// *explain itself* does not, which is why
/// [`Screen::unjudged_because`](crate::Screen::unjudged_because) is absent from
/// this table and was the round's own near-miss.
pub const DEFAULTED: &[(&str, &str)] = &[
    (
        "poses",
        "answered `1` correctly for almost every screen, so requiring it \
         would be that many sites writing the same number — and a wrong \
         answer here is loud, since a pose a screen cannot reach refuses",
    ),
    (
        "pose",
        "the other half of the same decision: a screen with one pose has \
         nothing to do when asked for it, and `poses` already said so",
    ),
    (
        "paint_stems",
        "★★★★★ R1911 — every screen has at least its root tag, so the default \
         is a floor rather than a guess; what keeps it from being an escape \
         hatch is NOT this default but the host's gate, which requires every \
         painted mark to belong to some section or to declared host chrome, so \
         an undeclared family is red rather than invisible. Measured the round \
         it was added: all four mounted screens of the analysis tool needed to \
         override it, because a screen's marks are not under its root tag",
    ),
];

#[cfg(test)]
mod tests {
    use super::{DEFAULTED, FOLDED, WINDOW_LEVEL};
    use std::collections::BTreeSet;

    /// The three binding traits, read from their own source so the census
    /// cannot be satisfied by a list that stopped being true.
    const WIDGET_CORE: &str = include_str!("../../pinion-core/src/widget_core.rs");
    const WIDGET_A11Y: &str = include_str!("../../pinion-a11y/src/widget_a11y.rs");
    const WIDGET_VIEW: &str = include_str!("../../pinion-shell/src/lib.rs");
    const SCREEN: &str = include_str!("lib.rs");
    /// ★ R1888 — read as source because for a defaulted hook the compiler no
    /// longer says whether `Mount` forwards it.
    const MOUNT: &str = include_str!("mount.rs");

    /// The `fn` names declared directly in `pub trait <name>`'s body.
    ///
    /// A trait body's method declarations sit at exactly one indent level and
    /// the block ends at a `}` in column zero — both true of every file read
    /// here, and both checked by the emptiness assertion at each call site, so
    /// a reshaped source file fails the census rather than passing it with
    /// nothing found.
    fn trait_hooks(source: &str, trait_name: &str) -> BTreeSet<String> {
        let opener = format!("pub trait {trait_name}");
        let mut hooks = BTreeSet::new();
        let mut inside = false;
        for line in source.lines() {
            if !inside {
                if line.starts_with(&opener) && line.trim_end().ends_with('{') {
                    inside = true;
                }
                continue;
            }
            if line == "}" {
                break;
            }
            if let Some(rest) = line.strip_prefix("    fn ") {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    hooks.insert(name);
                }
            }
        }
        hooks
    }

    /// The `fn` names declared directly in `pub trait <name>`'s body that carry
    /// a **default body** rather than ending the declaration at a `;`.
    ///
    /// A signature can span several lines, so the declaration is joined until
    /// the character that ends it, and it is that character which answers: `{`
    /// is a default, `;` is a requirement. Reading only the `fn` line would
    /// call every multi-line signature a requirement — including the four this
    /// trait has — which is the direction that would make the census pass while
    /// blind.
    fn defaulted_hooks(source: &str, trait_name: &str) -> BTreeSet<String> {
        let opener = format!("pub trait {trait_name}");
        let mut defaulted = BTreeSet::new();
        let mut inside = false;
        let mut pending: Option<String> = None;
        for line in source.lines() {
            if !inside {
                if line.starts_with(&opener) && line.trim_end().ends_with('{') {
                    inside = true;
                }
                continue;
            }
            if line == "}" {
                break;
            }
            if pending.is_none() {
                if let Some(rest) = line.strip_prefix("    fn ") {
                    let name: String = rest
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if name.is_empty() {
                        continue;
                    }
                    pending = Some(name);
                } else {
                    continue;
                }
            }
            // The declaration ends at the first `;` or `{` on or after the `fn`
            // line. A `where` clause or a wrapped argument list carries
            // neither, so it simply falls through to the next line.
            let ends_defaulted = match (line.find(';'), line.find('{')) {
                (Some(_), None) => Some(false),
                (None, Some(_)) => Some(true),
                (Some(semi), Some(brace)) => Some(brace < semi),
                (None, None) => None,
            };
            if let Some(is_default) = ends_defaulted {
                let name = pending.take().expect("a declaration is open");
                if is_default {
                    defaulted.insert(name);
                }
            }
        }
        defaulted
    }

    /// ★★★★★ R1888 — a `Screen` hook that carries a default waives this
    /// module's compile-time guarantee for itself, so it must be one this
    /// crate has named and given a reason for.
    ///
    /// An EQUALITY, like every other census in this tree: an unlisted default
    /// is the escape hatch opening quietly, and a listed hook that has stopped
    /// being defaulted is a reason nobody needs any more standing where a
    /// reader will take it for a live decision.
    #[test]
    fn r1888_every_screen_default_is_named_and_justified() {
        let hooks = trait_hooks(SCREEN, "Screen");
        assert!(
            hooks.len() > 20,
            "the `Screen` trait was not found in its own source; this census \
             would otherwise pass by finding nothing"
        );

        let found = defaulted_hooks(SCREEN, "Screen");
        let named: BTreeSet<String> = DEFAULTED
            .iter()
            .map(|(hook, _)| (*hook).to_owned())
            .collect();
        assert_eq!(
            found, named,
            "`Screen` hooks carrying a default body must be exactly the ones \
             `coverage::DEFAULTED` names. A default is the guarantee that \
             `Mount` cannot forget a hook, waived for that hook — the header \
             of this module asserted there were none while two had been \
             defaulted for eighty rounds."
        );
        for (hook, why) in DEFAULTED {
            assert!(
                !why.trim().is_empty(),
                "`{hook}` is defaulted with no reason given"
            );
        }
    }

    /// ★★★★★ R1888 — and the waived guarantee, restored by reading.
    ///
    /// The whole cost of a default is that `Mount` compiles without forwarding
    /// the hook, so the binding's answer is replaced by the trait's silently.
    /// The compiler cannot say it any more; `mount.rs` can.
    #[test]
    fn r1888_a_defaulted_hook_is_forwarded_by_mount_anyway() {
        assert!(
            MOUNT.contains("impl<V: WidgetView> Screen for Mount<V> {"),
            "`Mount`'s `Screen` impl was not found in its own source, so this \
             census read nothing and would have passed"
        );
        let missing: Vec<&str> = DEFAULTED
            .iter()
            .map(|(hook, _)| *hook)
            .filter(|hook| !MOUNT.contains(&format!("fn {hook}(")))
            .collect();
        assert!(
            missing.is_empty(),
            "these `Screen` hooks are defaulted AND not forwarded by `Mount`, \
             so a binding that answers them is overridden by the trait's \
             default with nothing saying so: {missing:?}"
        );
    }

    #[test]
    fn r1724_every_binding_hook_is_mirrored_folded_or_pinned() {
        let screen = trait_hooks(SCREEN, "Screen");
        assert!(
            screen.len() > 20,
            "the `Screen` trait was not found in its own source; the census \
             would otherwise pass by finding nothing"
        );

        let folded: BTreeSet<&str> = FOLDED.iter().map(|(hook, _, _)| *hook).collect();
        let pinned: BTreeSet<&str> = WINDOW_LEVEL.iter().map(|(hook, _)| *hook).collect();

        let mut unanswered: Vec<String> = Vec::new();
        for (source, name) in [
            (WIDGET_CORE, "WidgetCore"),
            (WIDGET_A11Y, "WidgetA11y"),
            (WIDGET_VIEW, "WidgetView"),
        ] {
            let hooks = trait_hooks(source, name);
            assert!(
                hooks.len() > 2,
                "`{name}` was not found in its own source, so this census read \
                 nothing and would have passed"
            );
            for hook in hooks {
                if screen.contains(&hook)
                    || folded.contains(hook.as_str())
                    || pinned.contains(hook.as_str())
                {
                    continue;
                }
                unanswered.push(format!("{name}::{hook}"));
            }
        }
        assert!(
            unanswered.is_empty(),
            "these binding hooks are neither mirrored on `Screen`, folded into \
             one of its methods, nor pinned as window-level with a reason — a \
             mounted screen that overrides one of them would silently lose it: \
             {unanswered:?}"
        );
    }

    /// The other direction: a `Screen` method that mirrors nothing is a mirror
    /// of a hook that has been removed, and it would go on being forwarded to
    /// a binding surface nobody calls.
    #[test]
    fn r1724_every_screen_method_mirrors_a_binding_hook() {
        let mut binding: BTreeSet<String> = BTreeSet::new();
        for (source, name) in [
            (WIDGET_CORE, "WidgetCore"),
            (WIDGET_A11Y, "WidgetA11y"),
            (WIDGET_VIEW, "WidgetView"),
        ] {
            binding.extend(trait_hooks(source, name));
        }
        let fold_targets: BTreeSet<&str> = FOLDED.iter().map(|(_, into, _)| *into).collect();

        let orphans: Vec<String> = trait_hooks(SCREEN, "Screen")
            .into_iter()
            .filter(|hook| !binding.contains(hook) && !fold_targets.contains(hook.as_str()))
            .collect();
        assert!(
            orphans.is_empty(),
            "these `Screen` methods mirror no binding hook: {orphans:?}"
        );
    }

    /// A fold whose target is not a `Screen` method, or a pin of a hook that
    /// no trait declares, is a stale entry in one of the two tables above —
    /// which would let a real hook slip through under its name.
    #[test]
    fn r1724_the_fold_and_pin_tables_are_not_stale() {
        let screen = trait_hooks(SCREEN, "Screen");
        let mut binding: BTreeSet<String> = BTreeSet::new();
        for (source, name) in [
            (WIDGET_CORE, "WidgetCore"),
            (WIDGET_A11Y, "WidgetA11y"),
            (WIDGET_VIEW, "WidgetView"),
        ] {
            binding.extend(trait_hooks(source, name));
        }
        for (hook, into, _) in FOLDED {
            assert!(
                binding.contains(*hook),
                "`{hook}` is folded but no binding trait declares it any more"
            );
            assert!(
                screen.contains(*into),
                "`{hook}` folds into `Screen::{into}`, which does not exist"
            );
        }
        for (hook, _) in WINDOW_LEVEL {
            assert!(
                binding.contains(*hook),
                "`{hook}` is pinned window-level but no binding trait declares it"
            );
            assert!(
                !screen.contains(*hook),
                "`{hook}` is pinned as the host's AND mirrored on `Screen` — \
                 one of the two is wrong"
            );
        }
    }
}
