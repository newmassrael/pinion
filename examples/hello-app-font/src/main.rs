//! `hello-app-font` — R1448 §5.36 — **an application ships its own face**, and
//! its font-source state is data rather than a log line.
//!
//! ## The Qt call this mirrors
//!
//! Qt applications that cannot rely on the host's fonts call
//! `QFontDatabase::addApplicationFont` (or `…FromData`) in `main()`, before any
//! widget exists, then select the returned family by name. This binding does
//! the same through [`ShellConfig::with_application_font`], which registers the
//! face into the shell's render cache while the shell is being built — so the
//! "before any widget" ordering is structural instead of a rule to remember.
//!
//! ## What it shows that Qt cannot
//!
//! Qt answers "does this host have fonts?" with a `qWarning` on stderr. Nobody
//! downstream can read that: not an agent driving the app over §2 #2, not a
//! screen-QA tool, not a headless capture — which just produces blank text with
//! the reason in a log nobody parsed.
//!
//! Here the answer is a [`FontSourceReport`] the view fn reads with
//! [`font_sources()`] and paints into the scene, so it travels over
//! `scene/snapshot` like any other node (§2 #7). Three tagged rows:
//!
//!   * **`afd_system`** — the platform-scan verdict, `available` /
//!     `unavailable` / `not-probed`.
//!   * **`afd_families`** — the families this application supplied, or
//!     `(none)`.
//!   * **`afd_sample`** — the same string laid out in the **application's**
//!     family. Its measured width is the honest end-to-end proof: a face that
//!     were reported but not really registered would report fine and measure
//!     like the empty fallback.
//!
//! Run it on a normal host — `cargo run -p hello-app-font` — and the system row
//! says `available`. Run it with `FONTCONFIG_FILE` pointing at an empty font
//! tree and it says `unavailable`, the window still comes up, and `afd_sample`
//! still has glyphs, because they came from the declared face rather than from
//! the platform. Before R1448 that second run did not reach a window at all: it
//! aborted inside fontique.
//!
//! ## Where the face comes from
//!
//! The repo's existing shaping fixture, read from disk at startup and passed as
//! **bytes** — which is the point of the memory-based call: an application
//! normally has these in an asset bundle, not at a path.
//! `PINION_APP_FONT=<path>` overrides it, and a missing file is reported in the
//! rows rather than panicking, since "the application's asset is absent" is
//! exactly one of the states this demo exists to display.
//!
//! [`ShellConfig::with_application_font`]: pinion_shell::ShellConfig::with_application_font
//! [`FontSourceReport`]: pinion_core::reactive::FontSourceReport
//! [`font_sources()`]: pinion_core::reactive::font_sources()

use pinion_a11y::WidgetA11y;
use pinion_core::external::{External, StubExternal};
use pinion_core::reactive::{SystemFontStatus, font_sources};
use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, TextStyle,
};
use pinion_core::widgets::button::{ButtonEvent, ButtonState};
use pinion_core::{Frame, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(AppFontRenderer, AppFontRendererError);

const WIN_W: u32 = 620;
const WIN_H: u32 = 300;

const SYSTEM_TAG: &str = "afd_system";
const FAMILIES_TAG: &str = "afd_families";
const SAMPLE_TAG: &str = "afd_sample";

/// The face this application "ships". A fixture already in the repo — declaring
/// a *second* copy of a font file would vendor an external font, which this
/// workspace forbids, and the bytes are what the API takes anyway.
const DEFAULT_FONT: &str = "crates/pinion-text-font/tests/fonts/NanumGothic-Regular.ttf";

/// The sample string, shaped twice: once in the application's family and once
/// with no family pinned. Latin-only so its advance is comparable across both.
const SAMPLE: &str = "Shaped by the application's own face";

/// The platform-scan verdict as the one word the status row publishes.
///
/// A total function over the enum rather than a `match` with a fallback, so a
/// future variant is a compile error here instead of silently reading as
/// "unavailable" in a demo whose whole job is to report this accurately.
fn status_word(status: SystemFontStatus) -> &'static str {
    match status {
        SystemFontStatus::NotProbed => "not-probed",
        SystemFontStatus::Available => "available",
        SystemFontStatus::Unavailable => "unavailable",
    }
}

/// view-fn (§6.3): three tagged rows over the process's font-source report.
///
/// Pure and sync — [`font_sources()`] reads a boot-time snapshot from the owner
/// scope, so two calls on the same state agree and `dry_run` is unaffected.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: ButtonState, _frame: &Frame) -> Scene {
    let report = font_sources();

    let mut label = TextStyle::new();
    label.fg_color = Color::rgb(0x20, 0x20, 0x20);
    label.font_size_px = 15;

    let mut sample_style = TextStyle::new();
    sample_style.fg_color = Color::rgb(0x10, 0x30, 0x60);
    sample_style.font_size_px = 26;

    // Pin the application's family — on BOTH the status rows and the sample —
    // when there is one. The status rows are pinned deliberately: on a host
    // with no font database an unpinned label resolves to no face and shapes to
    // zero width, so a demo that reported "unavailable" in unpinned text would
    // print its own diagnosis invisibly. An application that ships a face
    // should have a legible UI on a bare host, and that is the whole argument
    // of this round rendered rather than argued.
    //
    // With no family the rows stay unpinned rather than naming one that does
    // not exist: asking for a missing name would measure the fallback and
    // report on that instead of on the registration.
    if let Some(family) = report.application_families.first() {
        label = label.with_font_family(family.clone());
        sample_style = sample_style.with_font_family(family.clone());
    }

    let families = if report.application_families.is_empty() {
        "(none)".to_owned()
    } else {
        report.application_families.join(", ")
    };

    let rows = ContainerNode::new(vec![
        Scene::Text(
            TextNode::styled(
                format!("system fonts: {}", status_word(report.system)),
                Rect::default(),
                label.clone(),
            )
            .with_tag(SYSTEM_TAG),
        ),
        Scene::Text(
            TextNode::styled(
                format!("application families: {families}"),
                Rect::default(),
                label,
            )
            .with_tag(FAMILIES_TAG),
        ),
        Scene::Text(TextNode::styled(SAMPLE, Rect::default(), sample_style).with_tag(SAMPLE_TAG)),
    ])
    .with_style(BoxStyle::filled(Color::rgb(0xF6, 0xF6, 0xF8)))
    .with_layout(
        LayoutStyle::new()
            .flex(FlexDirection::Column)
            .with_justify(JustifyContent::Center)
            .with_align_items(AlignItems::Center)
            .with_gap(14),
    );
    Scene::Container(rows)
}

struct AppFontDemo;

impl WidgetCore for AppFontDemo {
    type State = ButtonState;
    type Event = ButtonEvent;
    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal)
    }
    fn tag() -> &'static str {
        "app_font_demo"
    }
    fn read_state(_scene: &Scene) -> Self::State {
        ButtonState::Idle
    }
    fn view(state: Self::State, frame: &Frame) -> Scene {
        view(state, frame)
    }
    fn event_name(_event: Self::Event) -> &'static str {
        "__internal__"
    }
    fn title() -> &'static str {
        "pinion application font demo"
    }
}

impl WidgetA11y for AppFontDemo {}

impl WidgetView for AppFontDemo {
    type Renderer = AppFontRenderer;
    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

/// `PINION_APP_FONT=<path>` — the face to declare. Defaults to
/// [`DEFAULT_FONT`]. An env var rather than an argv flag so the RPC demo
/// harness, which owns the child's command line, can still steer it.
fn declared_font_path() -> String {
    std::env::var("PINION_APP_FONT").unwrap_or_else(|_| DEFAULT_FONT.to_owned())
}

fn main() {
    let path = declared_font_path();
    // An absent asset is a state to display, not a crash: the rows will say
    // `(none)` and the sample will shape against whatever the platform offers.
    // Reporting it beats aborting, which is the whole argument of this round.
    let config = match std::fs::read(&path) {
        Ok(data) => pinion_shell::ShellConfig::new().with_application_font(data),
        Err(err) => {
            eprintln!("hello-app-font: no application face at {path}: {err}");
            pinion_shell::ShellConfig::new()
        }
    };
    pinion_shell::run_with_config::<AppFontDemo>(config);
}
