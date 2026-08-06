//! R1448 §5.36 — where a [`LayoutCache`]'s faces come from, and what happens
//! when the platform has none.
//!
//! ## Qt reference: `QFontDatabase`
//!
//! Two things Qt does here that pinion did not:
//!
//! - **It does not die when the platform font database is unusable.** Qt
//!   reports the condition and keeps running; a Qt app on a font-less host
//!   draws no glyphs but stays up. pinion aborted, because fontique unwraps
//!   an `Err(NoMatch)` while it populates its generic-family map
//!   (`fontique-0.9.0/src/backend/fontconfig.rs:685`). R1447 measured the
//!   consequence: `hello-button-tui` exited 101 on such a host having painted
//!   nothing, and R1447 removed it only for the TUI, by never building the
//!   context there. This module removes it for **every** backend, by making
//!   the platform scan a probe whose failure is a *state* rather than an exit.
//! - **`QFontDatabase::addApplicationFontFromData`** — an application ships a
//!   face and registers it from memory, no system database involved.
//!   [`LayoutCache::register_font_data`] is that call.
//!
//! ## Where pinion is better
//!
//! Qt answers "are there fonts?" by writing a `qWarning` to stderr. A warning
//! on a stream is not a fact anyone can *query*: an agent driving the app over
//! §2 #2 cannot read it, a screen-QA tool cannot assert on it, and a headless
//! capture silently produces blank text with the explanation in a log nobody
//! parsed. Here the same condition is typed data — [`SystemFontStatus`] and
//! [`LayoutCache::application_font_families`] — so §2 #7 holds: whatever a
//! human could conclude about this cache's faces, an agent reads from the same
//! surface. A binding publishes it into its scene and the answer travels over
//! the wire like any other node.
//!
//! Qt's registration call is also lossier than it needs to be:
//! `addApplicationFontFromData` returns an opaque `int` id, and the caller
//! makes a second call (`applicationFontFamilies(id)`) to learn what it just
//! added. [`LayoutCache::register_font_data`] returns the family names
//! directly — the question "what did I just make selectable?" is answered by
//! the call that made it selectable.
//!
//! [`LayoutCache`]: crate::LayoutCache
//! [`LayoutCache::register_font_data`]: crate::LayoutCache::register_font_data
//! [`LayoutCache::application_font_families`]: crate::LayoutCache::application_font_families

use parley::FontContext;
use parley::fontique::{Collection, CollectionOptions, SourceCache};
use pinion_core::reactive::SystemFontStatus;
use std::panic::catch_unwind;
use std::sync::OnceLock;

/// Process-level verdict on the platform font database.
///
/// The scan's outcome is a property of the host, not of a cache, so the
/// **first** cache in the process to probe records it for the rest. This is
/// not a cache of the scan itself — a `Collection` is neither `Send` nor
/// `Sync` and each cache owns its own (see the [`crate::cache`] module docs on
/// per-thread contexts). It caches only the yes/no, which is what lets the
/// second cache skip `catch_unwind` and, more importantly, keeps the *first*
/// probe from being wasted work: the collection that answered the question is
/// the collection the caller keeps.
static SYSTEM_FONTS_USABLE: OnceLock<bool> = OnceLock::new();

/// Build a [`FontContext`], reporting whether the platform scan worked.
///
/// # Why `catch_unwind`
///
/// One of the two ways this host can have no usable font database is an
/// `unwrap` inside fontique, not a `Result` pinion can match on, so there is no
/// return value to inspect — surviving the scan is the only way to observe it.
/// `catch_unwind` is the established in-repo boundary for exactly this shape
/// (`pinion_core::reactive::owner` isolates each cleanup closure the same way),
/// and the workspace sets no `panic = "abort"` profile.
///
/// # Why surviving it is not the answer (R1574.4)
///
/// The *other* way is that the scan completes and the database is empty, and
/// which of the two a given host takes is a property of its fontconfig rather
/// than of pinion — see [`scan_yields_a_family`], where both hosts are
/// measured. So the verdict is the family count, and the unwind boundary only
/// keeps a failed scan from taking the process with it.
///
/// ## What is actually known about the unwind (R1448.1)
///
/// Whether fontique's own `Config` destructor runs as it unwinds is **not
/// observable from here** — it is a private type inside a dependency, so any
/// claim about it would be a guess dressed as a comment. Two things are
/// observable, and they are what the recovery rests on:
///
/// - **The recovered process is sound.** After the unwind it still shapes,
///   still accepts a [`register_font_data`](crate::LayoutCache::register_font_data)
///   face, and still measures it correctly — asserted in
///   `tests/font_less_host.rs`, not assumed.
/// - **At most one unwind per process is reachable.** [`SYSTEM_FONTS_USABLE`]
///   records the verdict, so every later cache takes the early return and never
///   re-enters the scan. So even if that path did leak an `FcConfig`, the leak
///   is bounded to one per process rather than growing per cache — which is why
///   "is it a leak?" does not need answering to justify this. The second-cache
///   assertions in that same test are what pin the bound.
///
/// This is a boundary against an upstream defect, and it is worth naming as
/// one: the textbook fix is for fontique to return `Result` from its backend
/// constructor, and that is an upstream change pinion cannot make from a
/// `cargo` dependency. If it lands, this recovery becomes dead and
/// `r1448_font_less_host_does_not_abort` is what will notice.
///
/// fontique's own panic message reaches stderr once per process on such a
/// host. pinion does not suppress it: silencing it means installing a global
/// panic hook, and a process-wide hook swapped for the duration of a scan
/// would swallow the message of any *real* panic racing it on another thread.
/// A misleading line is a smaller cost than a hidden panic, so the recovery is
/// announced through `tracing` instead and the fontique line is left to stand.
pub(crate) fn build_font_context() -> (FontContext, SystemFontStatus) {
    if SYSTEM_FONTS_USABLE.get() == Some(&false) {
        // Already known bad on this host: skip straight to the font-less
        // collection rather than re-running a scan that can only reach the same
        // verdict again (a panic, or a database with nothing in it).
        return (context(false), SystemFontStatus::Unavailable);
    }
    // The closure captures nothing and `CollectionOptions` is `Copy`, so it is
    // `UnwindSafe` without an assertion.
    //
    // R1574.4 — the scan AND the family count are inside one `catch_unwind`,
    // because surviving the scan is not the fact this reports. See
    // `scan_yields_a_family` below.
    if let Ok(Some(cx)) = catch_unwind(|| {
        let mut cx = context(true);
        scan_yields_a_family(&mut cx).then_some(cx)
    }) {
        let _ = SYSTEM_FONTS_USABLE.set(true);
        (cx, SystemFontStatus::Available)
    } else {
        let _ = SYSTEM_FONTS_USABLE.set(false);
        tracing::warn!(
            "platform font database yields no font family; continuing without \
             system fonts. Text shapes but renders no glyphs until a face is \
             supplied via LayoutCache::register_font_data (or declared at boot \
             with ShellConfig::with_application_font). Either the scan failed \
             (any fontique panic line above is upstream's and has been \
             recovered from) or it succeeded over an empty database — both are \
             the same fact for a caller asking whether it can draw text."
        );
        (context(false), SystemFontStatus::Unavailable)
    }
}

/// R1574.4 §5.36 — does this scanned collection actually offer a family?
///
/// The question [`SystemFontStatus`] exists to answer is "can this process draw
/// text from the platform?", and until R1574.4 the code answered a *different*
/// one: "did the scan return without unwinding?". Those are the same fact only
/// on a host where an empty font database makes fontique panic — which is a
/// property of the host's fontconfig, not of pinion.
///
/// The two hosts were measured against each other rather than reasoned about.
/// Under one identical, deliberately empty `FONTCONFIG_FILE`
/// (`tools/demos/r1447_font_free_tui.py`'s zero-face config, `fc-list` = 0 on
/// both), a 635-face developer box unwound inside fontique and reported
/// `Unavailable`, while a 53-face CI runner completed the scan and reported
/// **`Available` with nothing in it**. So a caller on that runner was told the
/// platform database was usable, and every string it shaped came back blank.
///
/// That is also why the count runs inside the same [`catch_unwind`] as the
/// scan: on a host where the backend is left half-initialised, asking for the
/// families is exactly where the second unwind would come from, and both
/// unwinds mean the identical thing to the caller.
///
/// `family_names()` is `Collection`'s own enumeration, so this asks the
/// database the question directly instead of inferring it. It stops at the
/// first name — the answer is a yes/no, and a host with 10,000 faces should not
/// pay for a full walk to learn that it has more than none.
fn scan_yields_a_family(cx: &mut FontContext) -> bool {
    cx.collection.family_names().next().is_some()
}

/// R1573 §5.36 — a [`FontContext`] that will consult **only** the faces it is
/// given: the platform scan is not run, so
/// [`register_font_data`](crate::LayoutCache::register_font_data) is the sole
/// source of glyphs.
///
/// Distinct from the [`SystemFontStatus::Unavailable`] path above, which reaches
/// the same collection because the host's database *failed*. Here it is a
/// **declaration**: the caller has decided that its own faces are the whole
/// font world, so the metrics it measures are a function of the bytes it shipped
/// and of nothing on the machine. That is what
/// [`LayoutCache::with_own_fonts`](crate::LayoutCache::with_own_fonts) is for,
/// and why it reports [`SystemFontStatus::NotProbed`] rather than `Unavailable`
/// — nothing was probed, and nothing failed.
pub(crate) fn own_fonts_context() -> FontContext {
    context(false)
}

/// A [`FontContext`] over a collection with or without the platform scan.
///
/// parley's own `FontContext::new()` is hardcoded to `CollectionOptions`'
/// defaults (`system_fonts: true`); its fields are public, so the font-less
/// variant needs no fork — only a constructor parley did not spell out.
fn context(system_fonts: bool) -> FontContext {
    FontContext {
        collection: Collection::new(CollectionOptions {
            shared: false,
            system_fonts,
        }),
        source_cache: SourceCache::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::{context, scan_yields_a_family};
    use parley::fontique::Blob;

    /// R1574.4 §5.36 — an unscanned collection offers no family, and a
    /// registered face makes it offer one.
    ///
    /// This is the predicate the [`super::SystemFontStatus`] verdict now rests
    /// on, asserted where it is **host-independent**: `context(false)` runs no
    /// platform scan, so "empty" is a property of the code rather than of the
    /// machine, and it is the exact collection a font-less host ends up with.
    ///
    /// The second half is what makes it an instrument instead of a tautology.
    /// Without it this test would also pass against a `scan_yields_a_family`
    /// that returned `false` unconditionally — which would report `Unavailable`
    /// on every host in the world, the mirror-image defect of the one R1574.4
    /// fixes. Registering the crate's own fixture face is what pins the
    /// direction.
    #[test]
    fn r1574_4_family_presence_is_what_the_predicate_reads() {
        let mut cx = context(false);
        assert!(
            !scan_yields_a_family(&mut cx),
            "a collection built with no platform scan offers no family — this \
             is the state a font-less host reaches, and the one that must not \
             report Available",
        );

        let registered = cx
            .collection
            .register_fonts(Blob::from(crate::test_font::NOTO_SANS.to_vec()), None);
        assert!(
            !registered.is_empty(),
            "premise: the fixture face registered, so the second assertion is \
             about the predicate rather than about a rejected blob",
        );
        assert!(
            scan_yields_a_family(&mut cx),
            "the same collection now offers a family, so the predicate reads \
             the database rather than answering a constant",
        );
    }
}
