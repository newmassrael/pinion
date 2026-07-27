//! R1448 §5.36 — `FontSourceReport`: where this process's faces came from, as
//! a fact a pure `view` can read.
//!
//! # Why this exists
//!
//! Qt's `QFontDatabase` reports "there are no fonts on this host" by writing a
//! `qWarning` to stderr. A line on a stream is not a fact anyone can query: an
//! agent driving the app over §2 #2 cannot read it, a screen-QA tool cannot
//! assert on it, and a headless capture produces blank text with the
//! explanation sitting in a log nobody parsed. Blank text with no reachable
//! reason is the worst of the three possible outcomes.
//!
//! So the same condition rides the [`MONOSPACE_METRICS`](super::font_metrics::MONOSPACE_METRICS)
//! edge instead: the shell owns the font context, learns the answer when it
//! probes, and seeds it on the root [`Owner`](super::owner::Owner) at boot; a
//! binding reads it from its view fn with [`font_sources()`] and publishes it
//! into its scene like any other text. Then §2 #7 holds — whatever a developer
//! could conclude about this process's faces, an agent reads off
//! `scene/snapshot`.
//!
//! # Why plain data, not a capability trait
//!
//! [`MonospaceMetrics`](super::font_metrics::MonospaceMetrics) is a trait
//! because measuring is work that must happen on demand, in the layer that owns
//! parley. This is not: it is two facts already known by the time the shell
//! finishes booting. A trait here would buy an indirection and nothing else, so
//! the slot carries the report itself.
//!
//! The report is a **boot-time snapshot**, and both of its fields are settled
//! by the time it is taken. `application_families` is settled because the only
//! way an application supplies a face is by declaring it before the shell starts
//! (`ShellConfig::with_application_font`, mirroring Qt apps calling
//! `addApplicationFont` in `main()`). `system` is settled because the shell
//! **probes for it**, deliberately, while building — `LayoutCache::probe_system_fonts`.
//!
//! That probe is not incidental. R1447 defers the platform scan to the first
//! shape, so a shell that merely read the status off a freshly built cache would
//! publish `NotProbed` whenever the application declared no font of its own, and
//! would go on saying so after the first frame had shaped and learned otherwise
//! — a status line reading "not-probed" on a host proven font-less. Reporting a
//! fact requires looking for it. A GUI shell reaches the shaper on its first
//! `Scene::Text` anyway, so looking early costs it nothing.
//!
//! Were a mid-run registration seam ever added, this slot's payload would have
//! to become a [`Signal`](super::signal::Signal) — that is the round which adds
//! it, not a defect of this one.
//!
//! # Purity
//!
//! Reading it from `view` preserves §6.3: the value is fixed for the process's
//! lifetime, so it cannot make two `view` calls on the same state disagree, and
//! `dry_run` is unaffected. Off the live shell (headless, RPC, unit tests) no
//! provider is seeded and the default reports
//! [`SystemFontStatus::NotProbed`] with no application families — honest rather
//! than optimistic, since in that case nobody has in fact probed.

use super::provider_slot::ProviderSlot;
use std::rc::Rc;

/// R1448 §5.36 — whether the platform font database was reachable.
///
/// Lives here, not in `pinion-text`, because it is a backend-neutral fact: the
/// TUI has an answer for it (`NotProbed`, forever — it never shapes) and so
/// does a headless capture. `pinion-core` stays parley-free, so the enum is
/// here and the code that produces it is in the layer that owns the shaper —
/// the same split as [`CellMetric`](crate::cell_metric::CellMetric) and its
/// measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SystemFontStatus {
    /// Nothing has shaped, so the scan has not run.
    ///
    /// The default, and the honest one: off the live shell nobody has probed,
    /// and reporting `Available` there would be a guess. R1447 defers the scan
    /// to the first shape, so a process that never shapes — every `pinion-tui`
    /// frame — stays here for its whole life.
    #[default]
    NotProbed,
    /// The platform font database was read.
    Available,
    /// The platform font database could not be read on this host: no font
    /// package installed, a container built without one, a `FONTCONFIG_FILE`
    /// pointing at an empty tree.
    ///
    /// Text still shapes and still lays out — a caller gets boxes, not a
    /// crash, which is Qt's behavior and, before R1448, not pinion's. To get
    /// glyphs, declare a face at boot.
    Unavailable,
}

/// R1448 §5.36 — the faces available to this process and where they came from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FontSourceReport {
    /// Whether the platform font database was reachable.
    pub system: SystemFontStatus,
    /// Families the application supplied itself, in declaration order.
    ///
    /// Qt's `QFontDatabase::applicationFontFamilies`, except answered for the
    /// process rather than per opaque registration id. Non-empty here with
    /// `system: Unavailable` is the interesting state: a host with no fonts
    /// where the application shipped its own and text renders anyway.
    pub application_families: Vec<String>,
}

impl FontSourceReport {
    /// Whether any face is available at all — the platform's, or one the
    /// application supplied.
    ///
    /// `false` means text will lay out and paint no glyphs. A binding that
    /// wants to warn its user, or a test that wants to skip a pixel
    /// assertion, asks this rather than re-deriving it from two fields.
    #[must_use]
    pub fn has_any_face(&self) -> bool {
        self.system == SystemFontStatus::Available || !self.application_families.is_empty()
    }
}

/// R1448 §5.36 — the font-source slot: key, default and inherit verdict as one
/// expression, in the module that owns the fact.
///
/// **`Inherited`** by the mechanical predicate — the shell DRIVES this at the
/// root owner, seeding it at boot before any binding reads it, exactly as
/// [`MONOSPACE_METRICS`](super::font_metrics::MONOSPACE_METRICS) is seeded.
/// Under R680 per-window owners a `PerScope` verdict would hand a secondary
/// window a freshly minted default reporting `NotProbed`, so that window's
/// status line would contradict the primary's about a process-wide fact — the
/// silent-desync class R1362 fixed.
/// [`provider_slot_tests!`](crate::provider_slot_tests) emits the verdict from
/// this declaration so it cannot be forgotten.
pub static FONT_SOURCES: ProviderSlot<FontSourceReport> =
    ProviderSlot::inherited("__pinion.reactive.font_sources", FontSourceReport::default);

/// R1448 §5.36 — read this process's font-source report from the active owner
/// scope; the process-wide default (`NotProbed`, no application families) when
/// called outside an [`Owner`](super::owner::Owner) scope or when no shell
/// seeded one.
///
/// Graceful rather than the strict `use_repaint_sink` shape, matching
/// [`measured_monospace_cell`](super::font_metrics::measured_monospace_cell): a
/// binding's view fn is routinely exercised in unit tests with no provider
/// installed, and the default is a correct answer there rather than a reason to
/// panic.
#[must_use]
pub fn font_sources() -> Rc<FontSourceReport> {
    super::owner::Owner::current().map_or_else(
        || Rc::new(FontSourceReport::default()),
        |o| FONT_SOURCES.resolve(&o),
    )
}

#[cfg(test)]
mod tests {
    use super::super::owner::Owner;
    use super::*;

    fn seeded() -> FontSourceReport {
        FontSourceReport {
            system: SystemFontStatus::Unavailable,
            application_families: vec!["Fixture Sans".to_owned()],
        }
    }

    // The verdict, EMITTED from the declaration rather than remembered.
    crate::provider_slot_tests!(
        r1448_font_sources_inherits,
        super::FONT_SOURCES,
        || -> FontSourceReport { seeded() }
    );

    /// R1448 — off the shell the reader answers `NotProbed` with no families.
    /// Honest, not optimistic: nobody probed, so nothing is claimed.
    #[test]
    fn r1448_default_report_claims_nothing() {
        let report = FontSourceReport::default();
        assert_eq!(report.system, SystemFontStatus::NotProbed);
        assert!(report.application_families.is_empty());
        assert!(
            !report.has_any_face(),
            "an unprobed process must not claim a face is available",
        );
    }

    /// R1448 — the state this round exists for: a host with no font database
    /// where the application supplied its own face still has one.
    #[test]
    fn r1448_application_face_counts_on_a_font_less_host() {
        let report = FontSourceReport {
            system: SystemFontStatus::Unavailable,
            application_families: vec!["Shipped Sans".to_owned()],
        };
        assert!(
            report.has_any_face(),
            "an application-supplied face is a face even with no system fonts",
        );
        // Discriminator: the same host WITHOUT the declaration has none, so
        // `has_any_face` is reading the families and not just the status.
        let bare = FontSourceReport {
            system: SystemFontStatus::Unavailable,
            application_families: Vec::new(),
        };
        assert!(!bare.has_any_face());
    }

    /// R1448 — a binding reads what the shell seeded, through the same
    /// root-owner edge the monospace metrics ride.
    #[test]
    fn r1448_binding_reads_the_seeded_report() {
        let root = Owner::new();
        FONT_SOURCES.provide(&root, seeded());
        let read = root.run(font_sources);
        assert_eq!(read.system, SystemFontStatus::Unavailable);
        assert_eq!(read.application_families, vec!["Fixture Sans".to_owned()]);
    }
}
