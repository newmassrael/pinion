//! §5.16 §5.39 client-side window chrome as introspectable overlay nodes (R1121).
//!
//! ## Why this module exists
//!
//! A torn-off dock panel opens as a borderless window (`decorations:
//! false`, R1115) so pinion — not the OS window manager — owns its
//! chrome. Until R1121 pinion drew only a drag strip and deferred the
//! actual window controls (close / minimize / maximize) as "future
//! axes" (the dock-header doc note), so a borderless window had no
//! visible way to be closed, minimized, or maximized. That is the
//! asymmetry this module closes: the OS-decorated main window had a
//! title bar with controls for free (winit's default), while a torn-off
//! window had none.
//!
//! [`inject_window_chrome`] promotes the chrome to real [`Scene`] nodes
//! layered on top of the window content: a title-bar strip ([`Scene::Box`]),
//! a title [`Scene::Text`], and close / minimize / maximize buttons
//! (each a tagged [`Scene::Box`] hit region with a font-free vector
//! [`Scene::Path`] glyph). The result is (a) introspectable via
//! `scene/snapshot` so an AI agent can observe and drive the window
//! controls (§2 #7) — the reason custom chrome beats OS chrome, whose
//! buttons live in the window manager outside the scene tree — and (b)
//! painted by the generic box / path / text walk, not an opaque callback
//! (§2 #1).
//!
//! ## The interactive-overlay distinction
//!
//! This is the first **interactive** overlay. [`crate::focus_ring`],
//! [`crate::highlight`], and [`crate::drag_image`] are all
//! pointer-transparent decorations layered for the eye only. Chrome
//! buttons must instead RECEIVE clicks, so they are NOT marked
//! `pointer_transparent`: they sit in the live hit-tested paint scene
//! and the shell routes a click on the composite tag
//! `ai-overlay/window-chrome#close` (etc.) to the matching winit action.
//! The strip background between the title and the buttons is itself a
//! hit region (`#grip`) — the title-bar drag that moves the window.
//!
//! ## Content inset (the shell's half)
//!
//! Unlike the pure-outset focus ring, the chrome strip OCCUPIES the top
//! `height_px` of the window. This module only draws the strip; the
//! shell insets the window content below it (lays the content out in a
//! viewport shortened by `height_px` and offset down) so the strip never
//! covers the content. The two land together in one round.

use pinion_core::scene::{
    BoxNode, ContainerNode, PathCommand, PathNode, PathPoint, Rect, Scene, TextNode,
};
use pinion_core::style::{BoxStyle, Color, LayoutStyle, PathStyle, Stroke, TextStyle};

use crate::highlight::{push_top_level, strip_tag, wrap_into_container};

/// Tag carried by the injected chrome strip container. Shares the
/// `ai-overlay/` family prefix ([`crate::HIGHLIGHT_TAG_PREFIX`]) so the
/// same "strip every overlay node before re-injecting" idempotency
/// discipline applies. There is at most one chrome strip per window, so
/// this is a single fixed tag rather than a per-target suffix.
pub const WINDOW_CHROME_TAG: &str = "ai-overlay/window-chrome";

/// Composite tag of the close button (`{WINDOW_CHROME_TAG}#close`). The
/// shell splits a composite paint tag at `#` (the R51.42 router
/// convention) so a click resolves to the `close` sub-tag, which the
/// shell routes to closing the window.
pub const WINDOW_CHROME_CLOSE_TAG: &str = "ai-overlay/window-chrome#close";

/// Composite tag of the minimize button. Routed to `Window::set_minimized(true)`.
pub const WINDOW_CHROME_MINIMIZE_TAG: &str = "ai-overlay/window-chrome#minimize";

/// Composite tag of the maximize / restore button. Routed to
/// `Window::set_maximized(toggle)`.
pub const WINDOW_CHROME_MAXIMIZE_TAG: &str = "ai-overlay/window-chrome#maximize";

/// Composite tag of the draggable title-bar region. A press here begins a
/// window move (the R1116 borderless-floater title-bar move).
pub const WINDOW_CHROME_GRIP_TAG: &str = "ai-overlay/window-chrome#grip";

/// Visual style of the window chrome strip. All sizes are logical pixels.
/// The colours default to a neutral dark title bar so a borderless window
/// reads as chrome rather than content; a binding overrides them to match
/// its theme.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowChromeStyle {
    /// Strip height along the window's top edge.
    pub height_px: u32,
    /// Strip background fill. ARGB.
    pub bg: Color,
    /// Title text colour.
    pub title_color: Color,
    /// Title text size.
    pub title_font_size_px: u32,
    /// Button glyph stroke colour (the X / line / square).
    pub glyph: Color,
    /// Width of each control button (Win11-style ~46px hit target).
    pub button_width_px: u32,
    /// Whether the minimize button is drawn + hit-tested.
    pub show_minimize: bool,
    /// Whether the maximize / restore button is drawn + hit-tested.
    pub show_maximize: bool,
    /// Whether the close button is drawn + hit-tested.
    pub show_close: bool,
}

impl WindowChromeStyle {
    /// Default chrome: 32px strip, dark `#2B2B2B` background, light glyphs,
    /// all three controls shown — the VS Code / Blender custom title bar.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            height_px: 32,
            bg: Color::rgb(0x2B, 0x2B, 0x2B),
            title_color: Color::rgb(0xE0, 0xE0, 0xE0),
            title_font_size_px: 13,
            glyph: Color::rgb(0xE0, 0xE0, 0xE0),
            button_width_px: 46,
            show_minimize: true,
            show_maximize: true,
            show_close: true,
        }
    }

    /// Builder: override the strip height.
    #[must_use]
    pub const fn with_height_px(mut self, height_px: u32) -> Self {
        self.height_px = height_px;
        self
    }

    /// Builder: override the strip background colour.
    #[must_use]
    pub const fn with_bg(mut self, bg: Color) -> Self {
        self.bg = bg;
        self
    }

    /// Builder: override the title text colour.
    #[must_use]
    pub const fn with_title_color(mut self, title_color: Color) -> Self {
        self.title_color = title_color;
        self
    }

    /// Builder: override the button glyph stroke colour.
    #[must_use]
    pub const fn with_glyph(mut self, glyph: Color) -> Self {
        self.glyph = glyph;
        self
    }

    /// Builder: hide the minimize button (a tool window that should not
    /// minimize independently sets this `false`).
    #[must_use]
    pub const fn with_minimize(mut self, show: bool) -> Self {
        self.show_minimize = show;
        self
    }

    /// Builder: hide the maximize / restore button.
    #[must_use]
    pub const fn with_maximize(mut self, show: bool) -> Self {
        self.show_maximize = show;
        self
    }

    /// Builder: hide the close button.
    #[must_use]
    pub const fn with_close(mut self, show: bool) -> Self {
        self.show_close = show;
        self
    }
}

impl Default for WindowChromeStyle {
    fn default() -> Self {
        Self::new()
    }
}

/// Inject a client-side window-chrome strip across the top of `scene`.
///
/// `title` is the window title drawn left-aligned in the strip.
/// `is_maximized` selects the maximize button's glyph (a single square
/// when the window is restorable-to-maximized, two overlapping squares —
/// the "restore" affordance — when already maximized). `viewport` is the
/// layout viewport `(width, height)` the shell fed `compute_layout`; the
/// strip spans the full `width` and the control buttons anchor to its
/// right edge.
///
/// Returns the scene **unchanged** when `viewport` is `None` — the strip
/// cannot place its right-anchored buttons without a known window width
/// (a headless RPC drive that never sized a window). Every shell paint
/// passes `Some((w, h))`, so this only short-circuits the degenerate case,
/// mirroring [`crate::inject_focus_ring`]'s unknown-geometry guard.
///
/// Idempotent: any pre-existing strip (same [`WINDOW_CHROME_TAG`]) is
/// stripped before the fresh one is appended, so re-injecting replaces
/// rather than duplicates.
#[must_use]
pub fn inject_window_chrome(
    scene: Scene,
    title: &str,
    is_maximized: bool,
    viewport: Option<(u32, u32)>,
    style: WindowChromeStyle,
) -> Scene {
    let Some((width, _height)) = viewport else {
        return scene;
    };
    if width == 0 || style.height_px == 0 {
        return scene;
    }

    let strip = build_chrome_strip(width, title, is_maximized, style);

    let mut wrapped = wrap_into_container(scene);
    strip_tag(&mut wrapped, WINDOW_CHROME_TAG);
    push_top_level(&mut wrapped, strip);
    wrapped
}

/// Build the chrome strip: a tagged container holding the background
/// (also the `#grip` move region), the title text, and the right-anchored
/// control buttons. The container is tagged [`WINDOW_CHROME_TAG`] so the
/// idempotency strip removes the whole strip as one unit.
fn build_chrome_strip(
    width: u32,
    title: &str,
    is_maximized: bool,
    style: WindowChromeStyle,
) -> Scene {
    let h = style.height_px;
    let mut children: Vec<Scene> = Vec::new();

    // Background == the draggable grip. Hit-testable (not pointer-
    // transparent): a press anywhere on the strip that misses a button
    // begins a window move.
    children.push(Scene::Box(
        BoxNode::new(Rect::new(0, 0, width, h), BoxStyle::filled(style.bg))
            .with_tag(WINDOW_CHROME_GRIP_TAG),
    ));

    // Title text, left-padded, vertically centred against the strip.
    // Pointer-transparent: it overlaps the grip, but a press on the title bar
    // should MOVE the window (the grip behind it owns the drag), not snag on the
    // decorative text. [`Scene::hit_test`] skips pointer-transparent nodes.
    let title_y = h.saturating_sub(style.title_font_size_px) / 2;
    let mut title_style = TextStyle::new();
    title_style.fg_color = style.title_color;
    title_style.font_size_px = style.title_font_size_px;
    children.push(Scene::Text(
        TextNode::styled(
            title.to_string(),
            Rect::new(TITLE_PAD_X, title_y, width.saturating_sub(TITLE_PAD_X), h),
            title_style,
        )
        .with_layout(LayoutStyle::new().with_pointer_transparent(true)),
    ));

    // Control buttons, right-to-left: close (rightmost), maximize, minimize.
    let bw = style.button_width_px;
    let mut right = width;
    if style.show_close {
        right = right.saturating_sub(bw);
        push_control(
            &mut children,
            Rect::new(right, 0, bw, h),
            ButtonKind::Close,
            is_maximized,
            style.glyph,
            WINDOW_CHROME_CLOSE_TAG,
        );
    }
    if style.show_maximize {
        right = right.saturating_sub(bw);
        push_control(
            &mut children,
            Rect::new(right, 0, bw, h),
            ButtonKind::Maximize,
            is_maximized,
            style.glyph,
            WINDOW_CHROME_MAXIMIZE_TAG,
        );
    }
    if style.show_minimize {
        right = right.saturating_sub(bw);
        push_control(
            &mut children,
            Rect::new(right, 0, bw, h),
            ButtonKind::Minimize,
            is_maximized,
            style.glyph,
            WINDOW_CHROME_MINIMIZE_TAG,
        );
    }

    // The strip Container MUST carry its own rect: [`Scene::hit_test`] gates
    // descent on the node's rect, so a `(0,0,0,0)` default would make the whole
    // strip (grip + buttons) invisible to hit-testing. Spanning the full strip
    // lets the cursor descend to the tagged button / grip beneath.
    let mut strip = ContainerNode::new(children).with_tag(WINDOW_CHROME_TAG);
    strip.rect = Rect::new(0, 0, width, h);
    Scene::Container(strip)
}

/// Left padding of the title text from the strip's left edge.
const TITLE_PAD_X: u32 = 12;

/// The three window-control glyphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ButtonKind {
    Minimize,
    Maximize,
    Close,
}

/// Push one control as TWO flat children of the strip: a pointer-transparent
/// glyph [`Scene::Path`], then a tagged, transparent hit [`Scene::Box`] on top.
/// Flat (not wrapped in a Container) because [`Scene::hit_test`] gates descent
/// on a node's own rect — a wrapper Container would default to `(0,0,0,0)` and
/// hide the button. The hit Box carries `rect`, so the cursor resolves to `tag`.
fn push_control(
    children: &mut Vec<Scene>,
    rect: Rect,
    kind: ButtonKind,
    is_maximized: bool,
    glyph: Color,
    tag: &'static str,
) {
    children.push(Scene::Path(
        glyph_path(rect, kind, is_maximized, glyph)
            .with_layout(LayoutStyle::new().with_pointer_transparent(true)),
    ));
    children.push(Scene::Box(
        BoxNode::new(rect, BoxStyle::filled(Color::TRANSPARENT)).with_tag(tag),
    ));
}

/// Build the font-free vector glyph centred in `rect`. The glyph spans a
/// `GLYPH_PX` square box centred in the button; a 1px stroke reads
/// crisply against the dark strip.
// Logical-pixel coordinates are far below f32's 2^23 exact-integer ceiling,
// so the u32 -> f32 casts are lossless in practice (the house idiom shared
// with `pinion_core::scene` / `paint_adapter` geometry).
#[allow(clippy::cast_precision_loss)]
fn glyph_path(rect: Rect, kind: ButtonKind, is_maximized: bool, color: Color) -> PathNode {
    // Centre a GLYPH_PX square in the button rect.
    let cx = rect.x + rect.w / 2;
    let cy = rect.y + rect.h / 2;
    let half = GLYPH_PX / 2;
    let cyf = cy as f32;
    let left = cx.saturating_sub(half) as f32;
    let top = cy.saturating_sub(half) as f32;
    let right = (cx + half) as f32;
    let bottom = (cy + half) as f32;

    let commands = match kind {
        ButtonKind::Close => vec![
            PathCommand::MoveTo(PathPoint::new(left, top)),
            PathCommand::LineTo(PathPoint::new(right, bottom)),
            PathCommand::MoveTo(PathPoint::new(right, top)),
            PathCommand::LineTo(PathPoint::new(left, bottom)),
        ],
        ButtonKind::Minimize => {
            // A single horizontal bar across the vertical centre.
            vec![
                PathCommand::MoveTo(PathPoint::new(left, cyf)),
                PathCommand::LineTo(PathPoint::new(right, cyf)),
            ]
        }
        ButtonKind::Maximize if is_maximized => {
            // "Restore": two offset square outlines.
            let off = 2.0_f32;
            let mut c = square_outline(left + off, top - off, right + off, bottom - off);
            c.extend(square_outline(
                left - off,
                top + off,
                right - off,
                bottom + off,
            ));
            c
        }
        ButtonKind::Maximize => square_outline(left, top, right, bottom),
    };

    PathNode::new(
        rect,
        commands,
        PathStyle::stroked(Stroke::new(color, GLYPH_STROKE_PX)),
    )
}

/// A closed rectangle outline as a `MoveTo`/`LineTo`×4/`Close` command run.
fn square_outline(left: f32, top: f32, right: f32, bottom: f32) -> Vec<PathCommand> {
    vec![
        PathCommand::MoveTo(PathPoint::new(left, top)),
        PathCommand::LineTo(PathPoint::new(right, top)),
        PathCommand::LineTo(PathPoint::new(right, bottom)),
        PathCommand::LineTo(PathPoint::new(left, bottom)),
        PathCommand::Close,
    ]
}

/// Glyph square extent in logical pixels.
const GLYPH_PX: u32 = 10;
/// Glyph stroke width in logical pixels.
const GLYPH_STROKE_PX: u32 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> Scene {
        Scene::Container(pinion_core::scene::ContainerNode::new(vec![]))
    }

    fn find_tag<'a>(scene: &'a Scene, tag: &str) -> Option<&'a Scene> {
        if scene.tag() == Some(tag) {
            return Some(scene);
        }
        match scene {
            Scene::Container(c) => c.children.iter().find_map(|s| find_tag(s, tag)),
            Scene::Scroll(s) => find_tag(&s.content, tag),
            _ => None,
        }
    }

    #[test]
    fn unknown_viewport_returns_scene_unchanged() {
        let out = inject_window_chrome(empty(), "x", false, None, WindowChromeStyle::default());
        assert!(find_tag(&out, WINDOW_CHROME_TAG).is_none());
    }

    #[test]
    fn injects_strip_title_and_three_buttons() {
        let out = inject_window_chrome(
            empty(),
            "Terminal",
            false,
            Some((800, 600)),
            WindowChromeStyle::default(),
        );
        assert!(find_tag(&out, WINDOW_CHROME_TAG).is_some(), "strip present");
        assert!(
            find_tag(&out, WINDOW_CHROME_GRIP_TAG).is_some(),
            "grip present"
        );
        assert!(
            find_tag(&out, WINDOW_CHROME_CLOSE_TAG).is_some(),
            "close present"
        );
        assert!(
            find_tag(&out, WINDOW_CHROME_MINIMIZE_TAG).is_some(),
            "min present"
        );
        assert!(
            find_tag(&out, WINDOW_CHROME_MAXIMIZE_TAG).is_some(),
            "max present"
        );
    }

    #[test]
    fn buttons_anchor_right_in_order() {
        let style = WindowChromeStyle::default();
        let out = inject_window_chrome(empty(), "t", false, Some((800, 600)), style);
        let close = find_tag(&out, WINDOW_CHROME_CLOSE_TAG).unwrap();
        let max = find_tag(&out, WINDOW_CHROME_MAXIMIZE_TAG).unwrap();
        let min = find_tag(&out, WINDOW_CHROME_MINIMIZE_TAG).unwrap();
        let bw = style.button_width_px;
        // Close is rightmost, then maximize, then minimize, left of it.
        assert_eq!(rect_of(close).x, 800 - bw);
        assert_eq!(rect_of(max).x, 800 - 2 * bw);
        assert_eq!(rect_of(min).x, 800 - 3 * bw);
        // All span the full strip height.
        assert_eq!(rect_of(close).h, style.height_px);
    }

    #[test]
    fn hidden_buttons_are_absent_and_close_shifts_right() {
        let style = WindowChromeStyle::default()
            .with_minimize(false)
            .with_maximize(false);
        let out = inject_window_chrome(empty(), "t", false, Some((400, 300)), style);
        assert!(find_tag(&out, WINDOW_CHROME_MINIMIZE_TAG).is_none());
        assert!(find_tag(&out, WINDOW_CHROME_MAXIMIZE_TAG).is_none());
        let close = find_tag(&out, WINDOW_CHROME_CLOSE_TAG).unwrap();
        assert_eq!(rect_of(close).x, 400 - style.button_width_px);
    }

    #[test]
    fn idempotent_reinjection_replaces() {
        let once = inject_window_chrome(
            empty(),
            "a",
            false,
            Some((640, 480)),
            WindowChromeStyle::default(),
        );
        let twice = inject_window_chrome(
            once,
            "b",
            false,
            Some((640, 480)),
            WindowChromeStyle::default(),
        );
        // Exactly one strip container survives.
        assert_eq!(count_tag(&twice, WINDOW_CHROME_TAG), 1);
    }

    fn rect_of(scene: &Scene) -> Rect {
        match scene {
            Scene::Box(b) => b.rect,
            _ => panic!("expected a Box"),
        }
    }

    fn count_tag(scene: &Scene, tag: &str) -> usize {
        let here = usize::from(scene.tag() == Some(tag));
        let nested: usize = match scene {
            Scene::Container(c) => c.children.iter().map(|s| count_tag(s, tag)).sum(),
            Scene::Scroll(s) => count_tag(&s.content, tag),
            _ => 0,
        };
        here + nested
    }
}
