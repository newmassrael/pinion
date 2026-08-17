//! `hello-overflow-clip` — R1685 §5.21 §5.45 §2 #6 §2 #7: **a column whose
//! body yields, and cuts what does not fit.**
//!
//! This is the shape a consumer reported the framework could not express, in
//! the fewest nodes that still show it. Three CSS lines describe it:
//!
//! ```css
//! .body   { flex: 1 1 auto; overflow: hidden; }
//! .action { flex: 0 0 auto; }
//! .tabbar { flex: 0 0 auto; }
//! ```
//!
//! — the body gives up space when the window is short, and whatever no longer
//! fits inside it is not drawn. Before R1685 only the first half of that was
//! reachable ([`LayoutStyle::min_size`] `= Px(0)`, the effect of `overflow:
//! hidden` written as arithmetic); the second half existed only on
//! [`Scene::Scroll`], so a region that had to clip had to also become
//! scrollable, or its layout budget had to be balanced by hand in pixels.
//!
//! # What this binary is for
//!
//! Every claim R1685 makes is observable here over the wire, and the demo
//! `tools/demos/r1685_a_body_yields_and_what_it_cuts_is_gone.py` drives them:
//!
//! * **the declaration** — `scene/snapshot` publishes `clips` per container,
//!   so a client reading a child rect that leaves its parent can tell whether
//!   that ink reaches the screen;
//! * **the layout half** — the chrome rows keep their declared heights at
//!   every window size and the body absorbs the whole difference;
//! * **the paint half** — `scene/containment` reports what the body cut, as
//!   `clipped` rather than `smeared`;
//! * **reachability** — `scene/scroll_reach` calls those rows `lost`, because
//!   nothing moves a hidden box. A [`Scene::Scroll`] of the same shape would
//!   call them `scrollable`, and the difference between those two answers is
//!   why the workaround was refused rather than documented.
//!
//! The rows are deliberately taller than the body at the opening size, so the
//! screen is in the interesting state the moment it boots.

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{External, StubExternal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Overflow, Size, SizeValue,
    TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloOverflowClipRenderer, HelloOverflowClipRendererError);

const WIN_W: u32 = 420;
const WIN_H: u32 = 560;
/// [`ThemeProvider`](pinion_core::theme::ThemeProvider) cache key — the `"app"`
/// convention the example gallery shares.
const THEME_TAG: &str = "app";
const VIEW_TAG: &str = "overflow_clip";

/// The two fixed rows and the yielding one, as the design states them.
const HEADER_H: u32 = 48;
const ACTION_H: u32 = 64;
const TABBAR_H: u32 = 44;
/// Body rows: nine of them at 56 = 504 tall, against a body that gets
/// `560 - 48 - 64 - 44 = 404` at the opening size. The overflow is the point,
/// so it is present at boot rather than only after a resize.
const ROWS: usize = 9;
const ROW_H: u32 = 56;
const ROW_GAP: u32 = 0;

/// The composite hit-target tag for entry `index` — `overflow_clip#3`.
fn entry_tag(index: usize) -> String {
    format!("{VIEW_TAG}#{index}")
}

/// One body row: a filled box with its ordinal, tall enough that a row either
/// clearly fits or clearly does not.
///
/// Tagged with the [[composite-paint-root-tag-convention]] `<widget>#<n>`
/// rather than a name of its own, because that is what an entry IS here: a
/// sub-region of this screen's one widget. The router splits at the `'#'` and
/// delivers to `overflow_clip` with the index in the payload, so the entries
/// stay hit-testable — which the demo's last step depends on, since a
/// pointer-transparent row could not tell a cut entry from a painted one.
fn row(index: usize, theme: &Theme) -> Scene {
    let label = Scene::Text(TextNode::styled(
        format!("entry {index}"),
        Rect::default(),
        TextStyle::new()
            .with_size_px(15)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    Scene::Container(
        ContainerNode::new(vec![label])
            .with_tag(entry_tag(index))
            // An entry contrasts with the body it sits in, because the point
            // of this screen is to SHOW the cut: the entry the body cuts in
            // half is a half-drawn card, and against a body of the same colour
            // it would be a half-drawn card nobody can see. Measured by
            // looking at the window — every gate was green with both fills the
            // same.
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHighest))
                    .with_corner_radius(8),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_padding(Rect::new(12, 0, 12, 0))
                    .with_size(Size::auto().with_height(SizeValue::Px(ROW_H)))
                    // A row never gives up its height — the BODY is what
                    // yields. Without this the rows would shrink to fit and
                    // the screen would never demonstrate a cut at all.
                    .with_flex_shrink(0.0),
            ),
    )
}

/// A fixed chrome band: `flex: 0 0 auto` with a declared height.
fn band(tag: &'static str, height: u32, fill: pinion_core::Color, content: Scene) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![content])
            .with_tag(tag)
            .with_style(BoxStyle::filled(fill))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Center)
                    .with_size(Size::auto().with_height(SizeValue::Px(height)))
                    // `flex: 0 0 auto` — the band neither grows nor gives way.
                    // This is the half of the design the body's `overflow`
                    // makes keepable: something has to yield, and the
                    // declaration says which.
                    .with_flex_grow(0.0)
                    .with_flex_shrink(0.0),
            ),
    )
}

/// view-fn (§6.3): pure sync `() -> Scene`.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "the `WidgetCore::view` signature"
)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);

    let header = band(
        "chrome.header",
        HEADER_H,
        theme.resolve(ColorRole::Surface),
        Scene::Text(
            TextNode::styled(
                "Session",
                Rect::default(),
                TextStyle::new().with_size_px(17).with_fg(on_surface),
            )
            .with_tag("chrome.header.title"),
        ),
    );

    // ★★★ R1685 — the whole subject of this binary, in one declaration.
    //
    // `flex: 1 1 auto` makes the body the row that absorbs whatever the window
    // does not have; `Overflow::Hidden` says what happens to the entries that
    // no longer fit inside it. Both halves are this one word: it zeroes the
    // CSS automatic minimum size (so the body may shrink below its content at
    // all) and it declares the clip every renderer observes.
    let body = Scene::Container(
        ContainerNode::new(vec![Scene::Container(
            ContainerNode::new((0..ROWS).map(|i| row(i, &theme)).collect())
                .with_tag("body.content")
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Column)
                        .with_gap(ROW_GAP)
                        .with_flex_shrink(0.0),
                ),
        )])
        .with_tag("body")
        .with_style(BoxStyle::filled(
            theme.resolve(ColorRole::SurfaceContainerLow),
        ))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_padding(Rect::new(16, 0, 16, 0))
                .with_flex_grow(1.0)
                .with_overflow(Overflow::Hidden),
        ),
    );

    let action = band(
        "chrome.action",
        ACTION_H,
        theme.resolve(ColorRole::Accent),
        Scene::Text(
            TextNode::styled(
                "Continue",
                Rect::default(),
                TextStyle::new()
                    .with_size_px(16)
                    .with_fg(theme.resolve(ColorRole::OnAccent)),
            )
            .with_tag("action.primary"),
        ),
    );

    let tabbar = band(
        "chrome.tabbar",
        TABBAR_H,
        theme.resolve(ColorRole::Surface),
        Scene::Text(
            TextNode::styled(
                "Today   History   Settings",
                Rect::default(),
                TextStyle::new()
                    .with_size_px(13)
                    .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
            )
            .with_tag("tabbar.items"),
        ),
    );

    Scene::Container(
        ContainerNode::new(vec![header, body, action, tabbar])
            .with_tag(VIEW_TAG)
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
    )
}

struct OverflowClipView;

impl WidgetCore for OverflowClipView {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal)
    }

    fn tag() -> &'static str {
        VIEW_TAG
    }

    fn read_state(_scene: &Scene) {}

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "none"
    }

    fn title() -> &'static str {
        "pinion hello-overflow-clip (R1685 §5.45 a body that yields and cuts)"
    }
}

impl WidgetA11y for OverflowClipView {
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        vec![
            AccessNode::new(VIEW_TAG, AriaRole::Group)
                .with_name("Session")
                .with_value(AccessValue::Text(format!(
                    "{ROWS} entries in a body that hides what does not fit"
                ))),
        ]
    }
}

impl WidgetView for OverflowClipView {
    type Renderer = HelloOverflowClipRenderer;

    /// R1710 §5.16 — `OpenResizable` with **no declared floor**, not `Fixed`.
    ///
    /// `Fixed` pins the OS-resize floor AT the open size, and this screen exists
    /// to show what a body yields when it runs out of room — it is driven
    /// smaller on purpose. The declaration was the default rather than a
    /// decision, and it went unnoticed until R1710 made the framework resolve a
    /// resize against it (a window manager had been enforcing it all along; the
    /// bare display CI runs on never did). `min: None` declares the absence of a
    /// floor rather than inventing a number nobody measured.
    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::OpenResizable {
            size: (WIN_W, WIN_H),
            min: None,
        }
    }
}

fn main() {
    pinion_shell::run::<OverflowClipView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    fn painted(w: u32, h: u32) -> Scene {
        let mut scene = Owner::new().run(|| view((), &Frame::new()));
        let mut cache = pinion_text::LayoutCache::new();
        pinion_runtime::layout::compute_layout(&mut scene, &mut cache, w, h);
        scene
    }

    fn rect_of(scene: &Scene, tag: &str) -> Rect {
        scene
            .find_with_tag(tag)
            .unwrap_or_else(|| panic!("{tag} is painted"))
            .rect()
    }

    /// ★★★ The layout half, at two sizes: the chrome keeps the heights it
    /// declares and the BODY absorbs the whole difference.
    ///
    /// This is what `flex: 0 0 auto` beside a yielding row is supposed to buy,
    /// and what a hand-balanced pixel budget gives up the moment anything in
    /// the body changes size.
    #[test]
    fn r1685_the_chrome_keeps_its_heights_and_the_body_absorbs_the_difference() {
        for (w, h) in [(WIN_W, WIN_H), (WIN_W, 360)] {
            let scene = painted(w, h);
            assert_eq!(rect_of(&scene, "chrome.header").h, HEADER_H, "at {w}x{h}");
            assert_eq!(rect_of(&scene, "chrome.action").h, ACTION_H, "at {w}x{h}");
            assert_eq!(rect_of(&scene, "chrome.tabbar").h, TABBAR_H, "at {w}x{h}");
            let body = rect_of(&scene, "body");
            assert_eq!(
                body.h,
                h - HEADER_H - ACTION_H - TABBAR_H,
                "the body is what yields, at {w}x{h}"
            );
            // And the bands stay on screen, in order, at both sizes.
            let tabbar = rect_of(&scene, "chrome.tabbar");
            assert_eq!(
                tabbar.y + tabbar.h,
                h,
                "the tab bar ends at the window's bottom edge at {w}x{h}"
            );
        }
    }

    /// ★★★ The paint half: what the body cut is nowhere on screen, and what
    /// fits is exactly where it was placed.
    #[test]
    fn r1685_an_entry_past_the_body_is_not_painted_anywhere() {
        let scene = painted(WIN_W, WIN_H);
        let body = rect_of(&scene, "body");
        assert!(
            rect_of(&scene, "body.content").h > body.h,
            "the fixture must overflow, or this test asserts nothing"
        );
        let visible: Vec<usize> = (0..ROWS)
            .filter(|i| {
                scene
                    .rect_for_tag_absolute(&entry_tag(*i))
                    .is_some_and(|r| r.h > 0)
            })
            .collect();
        assert!(
            !visible.is_empty() && visible.len() < ROWS,
            "some entries fit and some do not: {visible:?}"
        );
        assert_eq!(
            visible,
            (0..visible.len()).collect::<Vec<_>>(),
            "the ones that survive are the ones at the top"
        );
    }
}
