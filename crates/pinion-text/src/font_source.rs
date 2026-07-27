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
/// The failure is an `unwrap` inside fontique, not a `Result` pinion can
/// match on, so there is no return value to inspect — the only way to learn
/// that this host has no usable font database is to attempt the scan and
/// survive it. `catch_unwind` is the established in-repo boundary for exactly
/// this shape (`pinion_core::reactive::owner` isolates each cleanup closure
/// the same way), the workspace sets no `panic = "abort"` profile, and the
/// unwind crosses only fontique's own frames — the FFI calls have already
/// returned by the time it unwraps, so `Config`'s `Drop` runs normally.
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
        // collection rather than re-running a scan that will only panic again.
        return (context(false), SystemFontStatus::Unavailable);
    }
    // The closure captures nothing and `CollectionOptions` is `Copy`, so it is
    // `UnwindSafe` without an assertion.
    if let Ok(cx) = catch_unwind(|| context(true)) {
        let _ = SYSTEM_FONTS_USABLE.set(true);
        (cx, SystemFontStatus::Available)
    } else {
        let _ = SYSTEM_FONTS_USABLE.set(false);
        tracing::warn!(
            "platform font database unusable; continuing without system fonts. \
             Text shapes but renders no glyphs until a face is supplied via \
             LayoutCache::register_font_data (or declared at boot with \
             ShellConfig::with_application_font). Any fontique panic line above \
             is upstream's and has been recovered from."
        );
        (context(false), SystemFontStatus::Unavailable)
    }
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
