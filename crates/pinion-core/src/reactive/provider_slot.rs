//! R1366 §5.22 — the provider-slot DECLARATION: key, default and inherit-scope
//! as one expression, in the crate that owns the slot.
//!
//! # Why this is a type and not a doc table
//!
//! A provider slot is a capability the shell drives on the root [`Owner`] and a
//! binding resolves from wherever its `view` happens to run. Four facts have to
//! agree for one to work: its key, its default, whether it inherits, and that the
//! shell seeded it at root before anyone read it. Until R1366 those lived in four
//! places — a `const`, a `resolve_*` fn, a `provide_*` fn, and a markdown row in
//! `Owner::cache_inherited`'s rustdoc, in a different crate from most of them.
//!
//! They drifted, exactly as far as you would predict:
//!
//! * R1362 left the family root-only, reasoning from where the view fn happened
//!   to run ("not a live hazard today"). R1364 paid for that.
//! * R1364 fixed the four slots it had enumerated, and the enumeration was short.
//! * R1365 fixed the rest, wrote a source-text census to stop the drift, and
//!   claimed "slot #11 cannot land without a verdict" — which was **false**: the
//!   `__pinion.` prefix the census keys off was a convention nothing enforced,
//!   and `text_scale.rs` already broke it. An audit then found that 5 of the 8
//!   inheriting slots had no behavioural test at all, so their verdicts were
//!   asserted only by that markdown row.
//!
//! Every one of those is the same shape: a fact about a slot, kept somewhere the
//! compiler cannot see. So put it where the compiler can.
//!
//! ```ignore
//! static REPAINT_SINK: ProviderSlot<Arc<dyn RepaintSink>> =
//!     ProviderSlot::inherited("__pinion.reactive.repaint_sink", || Arc::new(NullRepaintSink));
//! ```
//!
//! # What the type actually forecloses
//!
//! * **The scope cannot be omitted** — [`ProviderSlot::inherited`] and
//!   [`ProviderSlot::per_scope`] are the only constructors, so a new slot states
//!   its verdict or does not compile. This is the claim R1365 made falsely.
//! * **The prefix is a compile error** — the `const fn` constructors assert it,
//!   and a `static` initializer is evaluated at compile time, so a key that skips
//!   `__pinion.` fails the build with `error[E0080]` rather than slipping past a
//!   source-text scan.
//! * **The key and both writers are one expression** — `window_control.rs` named
//!   this invariant in prose as "the `waiter.rs` discipline" and restated the
//!   first-write-wins hazard it guards against a dozen times across the tree.
//! * **A late seed panics** rather than being silently dropped. `Owner::cache`'s
//!   first-write-wins made `provide_*` a no-op after any read, which five sites
//!   excused as "never observed" without checking.
//!
//! # What it does NOT foreclose (say it plainly; R1365 did not)
//!
//! A human can still choose [`ProviderSlot::per_scope`] where `inherited` is
//! right. Nothing here makes that impossible. The choice is merely forced, made
//! at the declaration, one line to review, and — via
//! [`provider_slot_tests!`](crate::provider_slot_tests) — shipped with a test
//! that says what the choice means behaviourally. That is as far as types reach.

use std::borrow::Cow;
use std::rc::Rc;

use super::owner::Owner;

/// Whether a slot's value is the binding's or the scope's.
///
/// The predicate is mechanical and deliberately not a judgement about what the
/// slot "is": **does the shell DRIVE this slot at the root owner?** Seed it, poll
/// it, publish into it, write it — any of those. R1364 first published a taxonomy
/// instead ("capabilities inherit, values do not"), which got the four sinks right
/// by luck of category and then missed `local_task_pump` and misclassified
/// `pane_viewport_registry`. A taxonomy invites argument about which bucket a slot
/// falls in; "who drives it" is a fact you can grep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotScope {
    /// The shell drives it at root; a child scope resolves the ROOT's value.
    ///
    /// Without this, the deferred R680 atomic — which wraps each window's `view`
    /// in `window_owner(id).run(..)` — hands every secondary window a freshly
    /// minted default: a Quit button that no-ops, a producer bumping a revision
    /// nobody observes. Silent, and it names nothing that would make the author
    /// of R680 think of quitting.
    Inherited,
    /// Deliberately per-scope: the shell's WRITE is primary-gated, so the root
    /// only ever holds the PRIMARY window's value.
    ///
    /// Inheriting would hand a secondary window the primary's data — confidently
    /// wrong — where a per-scope empty default is an honest "no data". Be precise
    /// that it is the write that is gated: R1364 wrote "the READ is primary-gated"
    /// and R1365 propagated it to three places, but no read gates anything, and
    /// "the read is primary-scoped" is this verdict restated rather than a reason
    /// for it.
    PerScope,
}

/// The reserved prefix for a framework provider-slot key.
///
/// `Owner::cache` is open to any key — a widget's `"hover_anim"` is not a
/// provider slot and must not inherit. This prefix marks the ones that are, and
/// [`ProviderSlot`]'s constructors make it structural instead of conventional.
pub const SLOT_KEY_PREFIX: &str = "__pinion.";

/// `true` when `key` carries [`SLOT_KEY_PREFIX`]. A `const fn` so the
/// constructors can assert it at compile time.
#[must_use]
pub const fn is_slot_key(key: &str) -> bool {
    let k = key.as_bytes();
    let p = SLOT_KEY_PREFIX.as_bytes();
    if k.len() <= p.len() {
        return false;
    }
    let mut i = 0;
    while i < p.len() {
        if k[i] != p[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// A framework provider slot: its key, its default, and whether it inherits.
///
/// Declare one as a `static` in the crate that owns the slot — `pinion-core`
/// never needs to name that crate, which is what lets `pinion-shell` and
/// `pinion-runtime` own slots without core reaching upward. See the module doc.
///
/// The payload `V` may be `!Send`/`!Sync` (an `Rc`, an `Arc<dyn Trait>` whose
/// trait is neither) even though this lives in a `static`: only `fn() -> V`
/// mentions `V`, and fn pointers are unconditionally `Sync`, so the `static`'s
/// `Sync` requirement never reaches the payload.
pub struct ProviderSlot<V: 'static> {
    key: &'static str,
    scope: SlotScope,
    default: fn() -> V,
}

impl<V: 'static> ProviderSlot<V> {
    /// Declare a slot the shell drives at root, resolved through the parent walk.
    ///
    /// # Panics
    ///
    /// At **compile time** (`error[E0080]`) when `key` lacks [`SLOT_KEY_PREFIX`],
    /// for a `static` or any other const-evaluated initializer.
    #[must_use]
    pub const fn inherited(key: &'static str, default: fn() -> V) -> Self {
        assert!(
            is_slot_key(key),
            "a ProviderSlot key must start with `__pinion.`",
        );
        Self {
            key,
            scope: SlotScope::Inherited,
            default,
        }
    }

    /// Declare a slot that stays per-scope because the shell's write is
    /// primary-gated — see [`SlotScope::PerScope`], and state the gate at the
    /// declaration, because this is the verdict a reader will doubt.
    ///
    /// # Panics
    ///
    /// At **compile time** (`error[E0080]`) when `key` lacks [`SLOT_KEY_PREFIX`].
    #[must_use]
    pub const fn per_scope(key: &'static str, default: fn() -> V) -> Self {
        assert!(
            is_slot_key(key),
            "a ProviderSlot key must start with `__pinion.`",
        );
        Self {
            key,
            scope: SlotScope::PerScope,
            default,
        }
    }

    /// This slot's key. For diagnostics and the source-text negative scan.
    #[must_use]
    pub const fn key(&self) -> &'static str {
        self.key
    }

    /// This slot's verdict.
    #[must_use]
    pub const fn scope(&self) -> SlotScope {
        self.scope
    }

    /// Resolve the slot for `owner`, creating the default if nothing provides it.
    ///
    /// `Inherited` walks to the nearest ancestor that has it; `PerScope` reads
    /// this owner only. The whole point of the type is that a caller cannot pick
    /// the wrong one — the declaration already did.
    ///
    /// # Panics
    ///
    /// Inherits [`Owner::cache`]'s nested-factory rule: a factory that re-enters
    /// the cache panics with an actionable message. Pre-resolve dependent slots
    /// before the factory runs (`[[owner-cache-no-nested-factory]]`).
    #[must_use]
    pub fn resolve(&'static self, owner: &Owner) -> Rc<V> {
        match self.scope {
            SlotScope::Inherited => owner.cache_inherited(Cow::Borrowed(self.key), self.default),
            SlotScope::PerScope => owner.cache(Cow::Borrowed(self.key), self.default),
        }
    }

    /// Resolve the slot for the *current* owner scope.
    ///
    /// # Panics
    ///
    /// When there is no active [`Owner::run`] scope — the strict shape every
    /// binding-facing `use_*` hook already has, so a missing scope fails loudly
    /// instead of silently minting a default nobody drives.
    #[must_use]
    pub fn resolve_current(&'static self) -> Rc<V> {
        let owner = Owner::current().unwrap_or_else(|| {
            panic!(
                "resolving the provider slot {} requires an active Owner scope",
                self.key,
            )
        });
        self.resolve(&owner)
    }

    /// Read the slot on `owner` WITHOUT creating it — `None` when nothing has
    /// resolved or provided it yet. This owner only (no ancestor walk).
    ///
    /// The demand-gate read: a writer that must not pay for a slot no reader
    /// asked for. `frame_timings`' publish uses this — a window whose binding
    /// never called `use_frame_timings` has no holder, so the publish returns
    /// early and the O(window) sample copy is never charged to it.
    /// [`resolve`](Self::resolve) always CREATES on a miss, which is wrong for
    /// that gate; this is the missing get-only leg, so the demand-gate no longer
    /// has to reach around the type via [`key`](Self::key) + `Owner::cache_get`.
    #[must_use]
    pub fn get(&'static self, owner: &Owner) -> Option<Rc<V>> {
        owner.cache_get_by_str::<V>(self.key)
    }

    /// Seed the slot on `owner` with the shell's real value.
    ///
    /// # Panics
    ///
    /// When the slot is already populated on this owner. [`Owner::cache`] is
    /// first-write-wins, so before R1366 a late `provide_*` silently dropped its
    /// argument and the binding kept the Null — a failure five call sites
    /// excused as "never observed" without ever checking. The shell seeds once,
    /// at boot, before any read; anything else is a wiring bug and says so.
    pub fn provide(&'static self, owner: &Owner, value: V) {
        assert!(
            !owner.cache_contains::<V>(self.key),
            "the provider slot {} was already seeded on this owner; \
             `Owner::cache` is first-write-wins, so this call would have been \
             silently dropped and every reader would keep the earlier value",
            self.key,
        );
        owner.cache(Cow::Borrowed(self.key), || value);
    }

    /// Create the slot on `owner` now, so descendants inherit it rather than
    /// minting their own.
    ///
    /// The walk cannot help a slot that does not exist yet: on a total miss
    /// `Owner::cache_inherited` creates at the CALLING scope, so a child that
    /// resolves before the shell first touches root would keep its own copy
    /// forever. Slots with a `provide` get seeded by that; the ones without —
    /// `local_task_pump`, `pane_viewport_registry` — need this at boot.
    /// Idempotent: seeding twice is the same instance, not a panic.
    pub fn seed_root(&'static self, root: &Owner) {
        drop(self.resolve(root));
    }
}

impl<V: 'static> core::fmt::Debug for ProviderSlot<V> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ProviderSlot")
            .field("key", &self.key)
            .field("scope", &self.scope)
            .finish_non_exhaustive()
    }
}

/// Emit the wiring-consistency test for a slot, so it cannot be forgotten.
///
/// R1365 wrote three of these by hand and forgot five, which meant five slots'
/// verdicts were asserted by nothing but a markdown row — a silent revert to
/// plain `cache` passed all four gates. A generated test cannot be forgotten;
/// that is the only thing a macro here earns over the bare type.
///
/// `Inherited` ⇒ a child scope resolves the ROOT's instance. `PerScope` ⇒ it
/// does not. `Owner::new_child` is exactly what R680 will run a secondary
/// window's `view` in.
///
/// # What this does NOT verify (R1366.8.1 audit — say it plainly)
///
/// This is **not** a verdict *discriminator*. Both the outcome (`ptr_eq`) and
/// the assertion branch derive from the same `self.scope`: `resolve` picks
/// `cache_inherited` vs `cache` from the scope, and the `match` below picks
/// `assert!(same)` vs `assert!(!same)` from the scope — so flipping the verdict
/// flips both together and the test STILL passes. It catches a broken
/// `cache_inherited`/`cache` (a wiring regression) and a forgotten test, not a
/// *wrong* verdict. Whether a slot SHOULD inherit is a semantic fact no generic
/// test can know; the real discriminator is the slot's own value-based
/// binding-path test (seed root with a distinct value, assert what a child
/// reads through the `use_*` hook — e.g. `viewport_size`'s
/// `..does_not_inherit_the_primary_viewport`). Every migrated slot ships one.
///
/// ```ignore
/// provider_slot_tests!(r1366_repaint_sink, REPAINT_SINK, || Arc::new(CountingSink::default()));
/// ```
#[macro_export]
macro_rules! provider_slot_tests {
    ($name:ident, $slot:path, $seed:expr) => {
        #[test]
        fn $name() {
            use $crate::reactive::{Owner, SlotScope};
            let root = Owner::new();
            let seeded = {
                let slot: &'static _ = &$slot;
                slot.provide(&root, ($seed)());
                slot.resolve(&root)
            };
            let child = Owner::new_child(&root);
            let from_child = {
                let slot: &'static _ = &$slot;
                slot.resolve(&child)
            };
            let same = std::rc::Rc::ptr_eq(&seeded, &from_child);
            match $slot.scope() {
                SlotScope::Inherited => assert!(
                    same,
                    "{} is declared Inherited but a child scope minted its own \
                     — post-R680 a secondary window would silently desync from \
                     the shell that drives this slot",
                    $slot.key(),
                ),
                SlotScope::PerScope => assert!(
                    !same,
                    "{} is declared PerScope but a child scope resolved the \
                     root's — the shell's write is primary-gated, so this hands \
                     a secondary window the PRIMARY's data",
                    $slot.key(),
                ),
            }
        }
    };
}

#[cfg(test)]
mod declaration_scan;

#[cfg(test)]
mod tests {
    use super::*;

    static PROBE: ProviderSlot<Rc<u32>> =
        ProviderSlot::inherited("__pinion.test.probe", || Rc::new(7));
    static PER_SCOPE_PROBE: ProviderSlot<Rc<u32>> =
        ProviderSlot::per_scope("__pinion.test.per_scope_probe", || Rc::new(7));

    #[test]
    fn r1366_the_prefix_is_checked() {
        // The compile-time half cannot be asserted from a test (that is the
        // point — it is `error[E0080]`, verified by feeding a bad key to a
        // `static`). This pins the predicate the const assert calls.
        assert!(is_slot_key("__pinion.reactive.repaint_sink"));
        assert!(!is_slot_key("__r668_text_scale"));
        assert!(!is_slot_key("hover_anim"));
        // The prefix alone is not a key: a slot needs a name after it.
        assert!(!is_slot_key(SLOT_KEY_PREFIX));
    }

    #[test]
    fn r1366_an_inherited_slot_resolves_the_roots_value_from_a_child() {
        let root = Owner::new();
        PROBE.provide(&root, Rc::new(42));
        let child = Owner::new_child(&root);
        assert_eq!(**PROBE.resolve(&child), 42);
    }

    #[test]
    fn r1366_a_per_scope_slot_does_not_see_the_roots_value() {
        let root = Owner::new();
        PER_SCOPE_PROBE.provide(&root, Rc::new(42));
        let child = Owner::new_child(&root);
        assert_eq!(
            **PER_SCOPE_PROBE.resolve(&child),
            7,
            "a PerScope slot must mint its own default, not read the root's",
        );
    }

    #[test]
    #[should_panic(expected = "already seeded")]
    fn r1366_a_late_seed_panics_instead_of_being_dropped() {
        // `Owner::cache` is first-write-wins, so this call USED to be a silent
        // no-op that left every reader on the earlier value.
        let root = Owner::new();
        PROBE.provide(&root, Rc::new(1));
        PROBE.provide(&root, Rc::new(2));
    }

    #[test]
    fn r1366_seed_root_is_idempotent() {
        // Unlike `provide`, seeding is the shell asking for the slot to exist;
        // asking twice is not a wiring bug.
        let root = Owner::new();
        PROBE.seed_root(&root);
        PROBE.seed_root(&root);
        assert_eq!(**PROBE.resolve(&root), 7);
    }
}
