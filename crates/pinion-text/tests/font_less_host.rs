//! R1448 §5.36 — pinion shapes on a host with no usable font database.
//!
//! ## Why a launcher + re-exec child, not a plain unit test
//!
//! The condition under test is selected by `FONTCONFIG_FILE`, and the
//! workspace forbids `unsafe_code` — so `std::env::set_var` is unavailable and
//! the variable has to be provisioned in a **child's** environment. That is
//! the same shape R1267 used to test the tray's SNI path under
//! `dbus-run-session` (`pinion-platform-tray/tests/sni_integration.rs`): a
//! launcher `#[test]` re-execs this binary's `#[ignore]`d inner test with the
//! environment it needs. The inner test is guarded by a marker variable so it
//! never runs in the normal suite, where it would assert about the developer's
//! own installed fonts.
//!
//! The launcher is not a skip-if-convenient: it **fails** if the scenario
//! fails. The only environmental gate is whether a font-less fontconfig can be
//! synthesised at all (a temp dir), which is always true where the suite runs.
//!
//! ## What is asserted
//!
//! Qt parity: a font-less host does not take the process down, and text still
//! shapes. Beyond Qt: the condition is readable as typed data, and an
//! application-supplied face makes real glyphs appear where the platform
//! offered none.

use pinion_core::reactive::SystemFontStatus;
use pinion_core::style::TextStyle;
use pinion_text::LayoutCache;
use std::path::{Path, PathBuf};
use std::process::Command;

const INNER_TEST: &str = "font_less_host_shapes_and_reports";
const INNER_MARKER: &str = "PINION_R1448_FONT_LESS_CHILD";

const RESCAN_INNER_TEST: &str = "system_scan_reruns_for_each_context";
const RESCAN_MARKER: &str = "PINION_R1448_1_RESCAN_CHILD";
/// Where the rescan child finds the font directory its config points at.
const RESCAN_DIR_ENV: &str = "PINION_R1448_1_FONT_DIR";

/// A face with NO Hangul coverage, and one with it. The pair is the whole
/// discriminator below: U+AC00 is the codepoint that separates them.
const NOTO_FONT: &str = "../pinion-text-font/tests/fonts/NotoSans-Regular.ttf";
const HANGUL: &str = "\u{AC00}";

/// A font this repo already ships as a shaping fixture. Registered from memory
/// in the scenario below, which is the point: on a font-less host it is the
/// only source of glyphs. Path is relative to this crate's root, matching the
/// convention in `pinion-text-font`'s own tests.
const FIXTURE_FONT: &str = "../pinion-text-font/tests/fonts/NanumGothic-Regular.ttf";

/// Write a valid fontconfig whose font directory is empty, and return its path.
/// Not "a broken config" — a well-formed one describing a host with no fonts,
/// which is what a slim container is.
fn write_font_less_config(root: &Path) -> PathBuf {
    let fonts = root.join("fonts");
    let cache = root.join("cache");
    std::fs::create_dir_all(&fonts).expect("fixture font dir");
    std::fs::create_dir_all(&cache).expect("fixture cache dir");
    let conf = root.join("no-fonts.conf");
    std::fs::write(
        &conf,
        format!(
            "<?xml version=\"1.0\"?>\n\
             <!DOCTYPE fontconfig SYSTEM \"fonts.dtd\">\n\
             <fontconfig>\n\
             \x20 <dir>{}</dir>\n\
             \x20 <cachedir>{}</cachedir>\n\
             </fontconfig>\n",
            fonts.display(),
            cache.display()
        ),
    )
    .expect("fixture fontconfig");
    conf
}

/// Launcher: run the scenario in a child whose `FONTCONFIG_FILE` describes a
/// host with no fonts.
#[test]
fn font_less_fixture_drives_a_host_with_no_fonts() {
    let tmp = std::env::temp_dir().join(format!("pinion-r1448-{}", std::process::id()));
    let conf = write_font_less_config(&tmp);

    let exe = std::env::current_exe().expect("test binary path");
    let status = Command::new(&exe)
        .args([INNER_TEST, "--exact", "--ignored", "--nocapture"])
        .env("FONTCONFIG_FILE", &conf)
        .env(INNER_MARKER, "1")
        .status()
        .expect("re-exec of the font-less scenario");

    let _ = std::fs::remove_dir_all(&tmp);
    assert!(
        status.success(),
        "the font-less-host scenario failed (see the child's output above)"
    );
}

/// The scenario, run only as the launcher's re-exec child.
#[test]
#[ignore = "re-exec entry for font_less_fixture_drives_a_host_with_no_fonts"]
fn font_less_host_shapes_and_reports() {
    if std::env::var_os(INNER_MARKER).is_none() {
        return;
    }

    // --- Qt parity 1: constructing and shaping does not abort. ---
    // Pre-R1448 the first line that shaped panicked inside fontique and took
    // the process with it; reaching the assertion below at all is the claim.
    let mut cache = LayoutCache::new();
    assert_eq!(
        cache.system_font_status(),
        SystemFontStatus::NotProbed,
        "R1447: construction defers the scan, so nothing is known yet",
    );

    let plain = TextStyle::new().with_size_px(32);
    let line_count = cache.layout("shape me", &plain, None).lines().count();
    assert!(
        line_count >= 1,
        "text shapes on a font-less host and yields a line box, as in Qt",
    );

    // --- Beyond Qt: the condition is data, not a stderr warning. ---
    assert_eq!(
        cache.system_font_status(),
        SystemFontStatus::Unavailable,
        "premise + claim: this child really has no font database, and the \
         cache reports that as a value a caller can branch on",
    );
    assert_eq!(
        cache.font_scans(),
        1,
        "the failed probe still counts as the one scan this cache paid",
    );
    assert!(
        cache.application_font_families().is_empty(),
        "nothing registered yet",
    );

    // Whatever the platform offered, it offered no glyphs — the width of a
    // shaped run is the measurable form of that. This is the state Qt leaves
    // an application in silently.
    let bare_width = cache.layout("AB", &plain, None).width();

    // --- Qt parity 2: addApplicationFontFromData, and then real glyphs. ---
    let data = std::fs::read(FIXTURE_FONT).unwrap_or_else(|e| {
        panic!("fixture font {FIXTURE_FONT} must be readable from the crate root: {e}")
    });
    let families = cache.register_font_data(data);
    assert!(
        !families.is_empty(),
        "registering a real font reports the families it made selectable — \
         Qt returns an opaque id and needs a second call",
    );
    assert_eq!(
        cache.application_font_families(),
        families.as_slice(),
        "the cumulative view agrees with what the call returned",
    );
    assert_eq!(
        cache.system_font_status(),
        SystemFontStatus::Unavailable,
        "registering a face does not claim the platform database came back",
    );

    let named = TextStyle::new()
        .with_size_px(32)
        .with_font_family(families[0].clone());
    let shaped = cache.layout("AB", &named, None);
    let named_width = shaped.width();
    assert!(
        named_width > 0.0,
        "the registered family shapes to a positive advance: {named_width}",
    );
    assert!(
        named_width > bare_width,
        "the registered face produced glyphs the font-less platform could \
         not: bare={bare_width} registered={named_width}",
    );

    // --- the process-level verdict: a SECOND cache takes the cached-no path ---
    // `build_font_context`'s early return (`SYSTEM_FONTS_USABLE == Some(false)`)
    // had no coverage: the first cache above goes through `catch_unwind`, so
    // nothing exercised the branch that skips it. A regression there would
    // re-run a scan already known to panic — recovered from, so still green,
    // but paying the failed scan again per cache.
    let mut second = LayoutCache::new();
    assert_eq!(
        second.system_font_status(),
        SystemFontStatus::NotProbed,
        "a fresh cache has not looked yet, whatever the process already knows",
    );
    assert_eq!(
        second.probe_system_fonts(),
        SystemFontStatus::Unavailable,
        "the second cache reaches the same verdict through the cached-no path",
    );
    assert_eq!(
        second.font_scans(),
        1,
        "one context build, not one per probe attempt",
    );
    // The verdict is process-level, not inherited state: this cache has its own
    // (empty) registration list, so the two caches agree about the HOST and
    // disagree about what the application gave THEM.
    assert!(
        second.application_font_families().is_empty(),
        "the process verdict is shared; a cache's registrations are its own",
    );

    // The face is selectable by name because it was registered, not because
    // the platform has it — the discriminator for the claim above.
    let absent = TextStyle::new()
        .with_size_px(32)
        .with_font_family("A Family No Host Has 12345");
    let absent_width = cache.layout("AB", &absent, None).width();
    assert!(
        absent_width < named_width,
        "premise: an UNregistered name still finds nothing here, so the width \
         above came from the registration and not from the platform: \
         absent={absent_width} registered={named_width}",
    );
}

/// Launcher: a fontconfig over a directory holding **one** face, so the child
/// can add a second one mid-process and see whether a new context notices.
#[test]
fn rescan_fixture_drives_a_config_whose_directory_changes() {
    let tmp = std::env::temp_dir().join(format!("pinion-r1448-1-{}", std::process::id()));
    let conf = write_font_less_config(&tmp);
    let fonts = tmp.join("fonts");
    // Seed the directory with the Hangul-less face only.
    std::fs::copy(NOTO_FONT, fonts.join("seed.ttf")).expect("seed face copies in");

    let exe = std::env::current_exe().expect("test binary path");
    let status = Command::new(&exe)
        .args([RESCAN_INNER_TEST, "--exact", "--ignored", "--nocapture"])
        .env("FONTCONFIG_FILE", &conf)
        .env(RESCAN_MARKER, "1")
        .env(RESCAN_DIR_ENV, &fonts)
        .status()
        .expect("re-exec of the rescan scenario");

    let _ = std::fs::remove_dir_all(&tmp);
    assert!(
        status.success(),
        "the per-context rescan scenario failed (see the child's output above)"
    );
}

/// R1448.1 §5.36 — **each `FontContext` runs its own platform scan.**
///
/// `LayoutCache`'s doc asserts this ("fontique caches nothing across instances
/// — each `Collection::new` runs a fresh `FcInitLoadConfig` scan"), and it is
/// the reason the R1447 deferral matters at all: if the scan were
/// process-cached, building a context would be cheap and there would be nothing
/// to defer. Until now it was read off fontique's source, not observed.
///
/// Observed here without a clock, so there is nothing to be flaky about. The
/// config points at a directory holding only a Hangul-less face. Shape U+AC00
/// through one cache; add a Hangul face to the directory; shape it again
/// through a **second** cache in the same process. A wider advance means that
/// second context went and looked — the file did not exist when the first one
/// did.
#[test]
#[ignore = "re-exec entry for rescan_fixture_drives_a_config_whose_directory_changes"]
fn system_scan_reruns_for_each_context() {
    if std::env::var_os(RESCAN_MARKER).is_none() {
        return;
    }
    let dir = std::path::PathBuf::from(
        std::env::var_os(RESCAN_DIR_ENV).expect("launcher passes the font dir"),
    );
    let style = TextStyle::new().with_size_px(32);

    let mut before = LayoutCache::new();
    assert_eq!(
        before.probe_system_fonts(),
        SystemFontStatus::Available,
        "premise: this config HAS a font, so the scan succeeds — a font-less \
         config would park the process verdict and make the second probe moot",
    );
    let width_before = before.layout(HANGUL, &style, None).width();

    // The change the second context must notice.
    std::fs::copy(
        "../pinion-text-font/tests/fonts/NanumGothic-Regular.ttf",
        dir.join("hangul.ttf"),
    )
    .expect("the Hangul face copies into the watched directory");

    let mut after = LayoutCache::new();
    assert_eq!(
        after.probe_system_fonts(),
        SystemFontStatus::Available,
        "the second context also scanned successfully",
    );
    let width_after = after.layout(HANGUL, &style, None).width();

    assert!(
        width_after > width_before,
        "a context built AFTER the directory gained a Hangul face shapes U+AC00 \
         wider than one built before it: before={width_before} after={width_after}. \
         Equal widths would mean the platform scan is cached across contexts, \
         which is the opposite of what LayoutCache's doc claims and would make \
         R1447's deferral pointless",
    );

    // The first cache is unaffected — it holds the collection it scanned, so
    // this is per-context state and not a process-wide snapshot that mutates
    // under a live cache.
    let width_before_again = before.layout(HANGUL, &style, None).width();
    assert!(
        (width_before_again - width_before).abs() < f32::EPSILON,
        "the older context keeps the world it scanned: {width_before} then \
         {width_before_again}",
    );
}
