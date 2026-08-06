//! R1573 §5.36 — the deterministic shaping fixture for this crate's own tests.
//!
//! # Why it exists
//!
//! Every test that shapes used to build a [`LayoutCache::new`], whose
//! `FontContext` is the **platform** font database — so the metrics it measured
//! were a function of whatever faces the machine happened to have. R835
//! registered that as debt when GitHub Actions first ran the suite and
//! `r766_goal_column_restores_after_crossing_short_line` failed on a
//! pixel tolerance tuned to a local font, and it stayed open with the note
//! "latent fragile".
//!
//! It was not latent. **Measured at R1573**, by pointing `FONTCONFIG_FILE` at a
//! config with no font directories: **40 of 94** unit tests changed their answer
//! or panicked. That is 43% of the suite reading the host, which is the exact
//! shape [[zero-flake-policy]] exists to forbid — a green local gate that says
//! nothing about CI, and a CI gate that says nothing about the next runner
//! image.
//!
//! # The fix, and why it is a constructor rather than a convention
//!
//! [`LayoutCache::with_own_fonts`] builds a cache whose collection is never
//! scanned from the platform, so a registered face is not merely *preferred* —
//! it is the only face that exists. A test using [`own_font_cache`] therefore
//! **cannot** read the host: there is nothing to read.
//!
//! ## The attribution, measured rather than claimed
//!
//! A counterfactual that kept the fixture on `LayoutCache::new()` — registering
//! the face and making it the default, which is exactly what R835's own note
//! prescribed — took the failure count from **40 to 5**, not to 4. So the bulk
//! of the determinism comes from the note's plan, and `with_own_fonts` closes
//! one more test.
//!
//! That one test is not the reason for the constructor. The reason is that
//! register-and-default makes the guarantee **conditional**: it holds only while
//! every `TextStyle` resolves to the named default, so a test that names another
//! family, or that shapes a glyph the face lacks, silently falls back to the
//! machine and no assertion anywhere notices. With the scan off there is nothing
//! to fall back to, so the property is structural instead of conventional —
//! which is the difference between a debt closed and a debt narrowed.
//! `r1573_an_unregistered_family_cannot_reach_the_host` demonstrates it.
//!
//! `r1573_no_test_shapes_through_the_host_without_saying_so` is the gate that
//! keeps it true of tests added later.

use crate::LayoutCache;

/// The face every deterministic test shapes through: the same
/// `NotoSans-Regular.ttf` `pinion-text-font` vendors for its parser fixtures and
/// `pinion-shell` installs for its pixel guards.
///
/// One face across the tree rather than one per crate, so a metric measured in
/// a unit test and a metric measured in a screenshot guard are the *same*
/// metric.
pub(crate) const NOTO_SANS: &[u8] =
    include_bytes!("../../pinion-text-font/tests/fonts/NotoSans-Regular.ttf");

/// The family name [`NOTO_SANS`] registers under, asserted rather than assumed
/// by [`own_font_cache`].
pub(crate) const NOTO_SANS_FAMILY: &str = "Noto Sans";

/// A [`LayoutCache`] that shapes **only** through [`NOTO_SANS`].
///
/// The registered face is also made the default family, so a `TextStyle` that
/// names no family resolves to it rather than to parley's platform stack — which
/// is what lets the existing tests keep their styles unchanged and still be
/// deterministic.
///
/// # Panics
///
/// If the vendored face does not register, or does not register under
/// [`NOTO_SANS_FAMILY`]. Either would mean the fixture is silently shaping
/// through nothing, and a test suite whose fixture has quietly stopped working
/// reports green for the wrong reason.
pub(crate) fn own_font_cache() -> LayoutCache {
    let mut cache = LayoutCache::with_own_fonts();
    let families = cache.register_font_data(NOTO_SANS.to_vec());
    assert!(
        families.iter().any(|f| f == NOTO_SANS_FAMILY),
        "the vendored face must register as {NOTO_SANS_FAMILY:?} (got \
         {families:?}); without it this fixture shapes through an empty \
         collection and every metric it measures is meaningless",
    );
    cache.set_default_font_family(Some(pinion_core::style::FontFamily::Named(
        NOTO_SANS_FAMILY.into(),
    )));
    cache
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::reactive::SystemFontStatus;
    use pinion_core::style::TextStyle;

    #[test]
    fn r1573_the_fixture_never_touches_the_platform_database() {
        let mut cache = own_font_cache();
        assert_eq!(
            cache.font_scans(),
            0,
            "an own-fonts cache pays no platform scan — registering a face must \
             not build a scanning context by the back door",
        );
        assert_eq!(
            cache.system_font_status(),
            SystemFontStatus::NotProbed,
            "and nothing was probed, so neither Available nor Unavailable is \
             true of it",
        );
        // It really does shape: a fixture that measured zero for everything
        // would satisfy every assertion above and be useless.
        let width = cache
            .layout("Hamburgefonstiv", &TextStyle::new().with_size_px(16), None)
            .width();
        assert!(
            width > 10.0,
            "the registered face produces real advances (got {width})",
        );
        assert_eq!(cache.font_scans(), 0, "and shaping still scans nothing");
    }

    /// Files whose test regions this gate reads. Named rather than globbed
    /// because `include_str!` needs literal paths, and asserted non-empty so a
    /// rename cannot turn the gate into a no-op.
    const SCANNED: [(&str, &str); 3] = [
        ("cache.rs", include_str!("cache.rs")),
        ("caret.rs", include_str!("caret.rs")),
        ("font_metrics.rs", include_str!("font_metrics.rs")),
    ];

    /// How many test-region `LayoutCache::new()` sites are legitimate, i.e. how
    /// many tests in this crate have the HOST as their actual subject.
    ///
    /// A number rather than a name list, and the difference matters: a curated
    /// list of *names* goes stale silently when a test is renamed, while a count
    /// forces the next person who adds a host-reading test to come here and say
    /// why. R1570.5's lesson — a derived population is only as wide as what it
    /// derives from — so the population is derived from the source and only the
    /// *budget* is written down.
    const HOST_SUBJECT_SITES: usize = 9;

    #[test]
    fn r1573_no_test_shapes_through_the_host_without_saying_so() {
        // The gate that keeps R1573 true of tests written later. Without it the
        // 97th test would reach for `LayoutCache::new()` — the obvious
        // constructor — and read the host again, and nothing would notice until
        // a CI runner changed its font package. That is precisely how this debt
        // survived from R835: the fix was applied to the one test that had
        // already failed.
        let mut unmarked: Vec<String> = Vec::new();
        let mut marked = 0usize;
        for (name, src) in SCANNED {
            let lines: Vec<&str> = src.lines().collect();
            let test_region_starts = lines
                .iter()
                .position(|l| l.trim_start().starts_with("#[cfg(test)]"))
                .unwrap_or_else(|| panic!("{name} has a test region"));
            for (i, line) in lines.iter().enumerate().skip(test_region_starts) {
                if !line.contains("LayoutCache::new()") {
                    continue;
                }
                // A COMMENT naming the constructor is not a call of it. The
                // gate's own exception notes say `LayoutCache::new()` out loud,
                // so without this it reports every reasoned exception as an
                // unreasoned one — found by running it, not by reading it.
                let code = line.trim_start();
                if code.starts_with("//") {
                    continue;
                }
                // An exception must be annotated within the few lines above it,
                // so the reason is at the site rather than in a list somewhere.
                let window = lines[i.saturating_sub(6)..i].join("\n");
                if window.contains("R1573") {
                    marked += 1;
                } else {
                    unmarked.push(format!("{name}:{}", i + 1));
                }
            }
        }
        assert!(
            unmarked.is_empty(),
            "these test sites shape through the HOST font database with no \
             stated reason — use `crate::test_font::own_font_cache()`, or write \
             a `R1573` comment saying why the host IS the subject: {unmarked:?}",
        );
        assert_eq!(
            marked, HOST_SUBJECT_SITES,
            "the number of tests whose subject is the host has changed. If you \
             added one, say why here; if you removed one, lower the budget. A \
             gate whose budget drifts silently is the R835 defect again",
        );
    }

    #[test]
    fn r1573_an_unregistered_family_cannot_reach_the_host() {
        // The property that makes `with_own_fonts` a fix rather than a
        // narrowing. Naming a family the fixture never registered is the shape
        // of every way register-and-default leaks: a style that names another
        // face, or text the face does not cover. With the platform scan off,
        // both resolve to the SAME thing on every machine.
        let style =
            |family: &'static str| TextStyle::new().with_size_px(16).with_font_family(family);
        let mut cache = own_font_cache();
        let registered = cache
            .layout("width", &style(NOTO_SANS_FAMILY), None)
            .width();
        let unregistered = cache
            .layout("width", &style("A Family This Host May Well Have"), None)
            .width();
        assert!(
            registered > 0.0,
            "premise: the registered family measures something ({registered})",
        );
        assert!(
            unregistered.abs() < f32::EPSILON,
            "an unregistered family measures ZERO, because there is no face to \
             fall back to (got {unregistered}). That is the honest cost of \
             `with_own_fonts` and it is also the point: on a host-reading cache \
             this same style resolves through the platform stack and returns a \
             width that is a property of the MACHINE, which no assertion in \
             this suite would flag",
        );
        assert_eq!(
            cache.font_scans(),
            0,
            "and no amount of naming families makes it scan",
        );
    }

    #[test]
    fn r1573_the_same_text_measures_the_same_in_two_fresh_fixtures() {
        // Determinism across cache instances, which is the property the CI
        // failure was about — two runs of the same suite are two processes.
        let style = TextStyle::new().with_size_px(17);
        let a = own_font_cache().layout("determinism", &style, None).width();
        let b = own_font_cache().layout("determinism", &style, None).width();
        assert!(
            (a - b).abs() < f32::EPSILON,
            "two fresh own-fonts caches measure identically ({a} vs {b})",
        );
    }
}
