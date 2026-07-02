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

use crate::highlight::{
    has_top_level_tag, push_top_level, strip_children_with_prefix, strip_tag, wrap_into_container,
};

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

/// (R1188 §5.16 §5.49 §2 #2) The three DISCRETE window-control actions a click
/// on a control tag requests — minimize / maximize-toggle / close.
///
/// Shell-neutral vocabulary (no winit types), so BOTH press paths speak it:
/// the winit pointer path (`AppShell::try_chrome_press`) and the headless RPC
/// click drain (`ShellCore`), which detects a control hit and queues it for the
/// windowed shell to execute — the §2 #2 drive-parity leg of the R1121 chrome
/// contract ("an AI agent observes AND DRIVES via a click on the control tag").
/// Deliberately excludes the grip / resize regions: those are pointer-session
/// gestures (an OS-interactive `drag_window` / `drag_resize_window` needs a live
/// pointer), whose RPC peers are the dedicated `scene/window_move` /
/// `scene/resize` methods, not a click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowControl {
    /// `Window::set_minimized(true)`.
    Minimize,
    /// `Window::set_maximized(toggle)`.
    Maximize,
    /// The per-window close seam (`WidgetView::window_close_requested`,
    /// app-exit fallback when unhandled).
    Close,
}

/// (R1190 §5.16 §5.39 §5.49) A window's eight resize edges / corners as a
/// SHELL-NEUTRAL enum — the winit-free peer of `winit::window::ResizeDirection`,
/// so pinion-overlay (which cannot name a winit type — it deps only pinion-core)
/// owns the tag→edge mapping and the shell does only the trivial edge→winit
/// conversion. Before R1190 the tag→direction half lived in the shell's
/// `chrome_action_for_tag`, splitting the meaning of the `WINDOW_RESIZE_*`
/// constants (defined HERE) across two crates with only tests guarding
/// cross-crate exhaustiveness; folding it into [`ChromeTag`] makes overlay the
/// single tag→semantic source (the session-audit structural fix).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowResizeEdge {
    /// Top edge.
    North,
    /// Bottom edge.
    South,
    /// Left edge.
    West,
    /// Right edge.
    East,
    /// Top-left corner.
    NorthWest,
    /// Top-right corner.
    NorthEast,
    /// Bottom-left corner.
    SouthWest,
    /// Bottom-right corner.
    SouthEast,
}

/// (R1190 §5.16 §5.39 §5.49) The complete SHELL-NEUTRAL meaning of a window-chrome
/// hit-test tag — the SINGLE source of truth for "what does this tag mean," owned
/// by pinion-overlay (which defines the tag constants). The shell's
/// `chrome_action_for_tag` and the RPC click drain both resolve through
/// [`chrome_tag_semantic`] and then apply only the winit-typed conversions they
/// need (`WindowResizeEdge`→`ResizeDirection`, `WindowControl` execution), so the
/// tag vocabulary can never drift across the crate boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromeTag {
    /// A discrete control button — minimize / maximize-toggle / close.
    Control(WindowControl),
    /// A resize edge / corner.
    Resize(WindowResizeEdge),
    /// The draggable title-bar grip (window move).
    MoveGrip,
}

/// (R1190 §5.16 §5.39 §5.49) Map a hit-test tag to its SHELL-NEUTRAL
/// [`ChromeTag`] meaning, or `None` for every non-chrome tag. The single
/// tag→semantic source of truth (overlay owns the constants AND their meaning);
/// the shell adds only winit-type conversions on top. Adding a ninth region now
/// means a new [`ChromeTag`] arm HERE, and the shell's exhaustive `match` on
/// `ChromeTag` then fails to compile until it is handled — cross-crate
/// exhaustiveness enforced by the type system, not just tests.
#[must_use]
pub fn chrome_tag_semantic(tag: &str) -> Option<ChromeTag> {
    match tag {
        WINDOW_CHROME_MINIMIZE_TAG => Some(ChromeTag::Control(WindowControl::Minimize)),
        WINDOW_CHROME_MAXIMIZE_TAG => Some(ChromeTag::Control(WindowControl::Maximize)),
        WINDOW_CHROME_CLOSE_TAG => Some(ChromeTag::Control(WindowControl::Close)),
        WINDOW_CHROME_GRIP_TAG => Some(ChromeTag::MoveGrip),
        WINDOW_RESIZE_NORTH_TAG => Some(ChromeTag::Resize(WindowResizeEdge::North)),
        WINDOW_RESIZE_SOUTH_TAG => Some(ChromeTag::Resize(WindowResizeEdge::South)),
        WINDOW_RESIZE_WEST_TAG => Some(ChromeTag::Resize(WindowResizeEdge::West)),
        WINDOW_RESIZE_EAST_TAG => Some(ChromeTag::Resize(WindowResizeEdge::East)),
        WINDOW_RESIZE_NORTH_WEST_TAG => Some(ChromeTag::Resize(WindowResizeEdge::NorthWest)),
        WINDOW_RESIZE_NORTH_EAST_TAG => Some(ChromeTag::Resize(WindowResizeEdge::NorthEast)),
        WINDOW_RESIZE_SOUTH_WEST_TAG => Some(ChromeTag::Resize(WindowResizeEdge::SouthWest)),
        WINDOW_RESIZE_SOUTH_EAST_TAG => Some(ChromeTag::Resize(WindowResizeEdge::SouthEast)),
        _ => None,
    }
}

/// (R1188 §5.16 §5.49) Map a hit-test tag to the discrete [`WindowControl`] it
/// requests, or `None` for every non-control tag. A thin projection of the
/// [`chrome_tag_semantic`] SSOT for the RPC click drain, which cares only about
/// the discrete controls (grip / resize are pointer-session gestures whose RPC
/// peers are `scene/window_move` / `scene/resize`).
#[must_use]
pub fn window_control_for_tag(tag: &str) -> Option<WindowControl> {
    match chrome_tag_semantic(tag) {
        Some(ChromeTag::Control(control)) => Some(control),
        _ => None,
    }
}

/// (R1122) Shared tag prefix of the eight window-resize hit regions. All
/// edge / corner tags start with this, so the idempotent strip-before-inject
/// removes the whole resize border by prefix (the regions are flat siblings,
/// not one container — see [`inject_resize_border`]).
pub const WINDOW_RESIZE_TAG_PREFIX: &str = "ai-overlay/window-resize";

/// Composite tag of the north (top) resize edge. The shell maps it to
/// `winit::window::ResizeDirection::North` and drives `drag_resize_window`.
pub const WINDOW_RESIZE_NORTH_TAG: &str = "ai-overlay/window-resize#north";
/// Composite tag of the south (bottom) resize edge.
pub const WINDOW_RESIZE_SOUTH_TAG: &str = "ai-overlay/window-resize#south";
/// Composite tag of the west (left) resize edge.
pub const WINDOW_RESIZE_WEST_TAG: &str = "ai-overlay/window-resize#west";
/// Composite tag of the east (right) resize edge.
pub const WINDOW_RESIZE_EAST_TAG: &str = "ai-overlay/window-resize#east";
/// Composite tag of the north-west (top-left) resize corner.
pub const WINDOW_RESIZE_NORTH_WEST_TAG: &str = "ai-overlay/window-resize#north-west";
/// Composite tag of the north-east (top-right) resize corner.
pub const WINDOW_RESIZE_NORTH_EAST_TAG: &str = "ai-overlay/window-resize#north-east";
/// Composite tag of the south-west (bottom-left) resize corner.
pub const WINDOW_RESIZE_SOUTH_WEST_TAG: &str = "ai-overlay/window-resize#south-west";
/// Composite tag of the south-east (bottom-right) resize corner.
pub const WINDOW_RESIZE_SOUTH_EAST_TAG: &str = "ai-overlay/window-resize#south-east";

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
    // R1121.1 — the fluent `with_*` builders were removed as speculative API
    // (YAGNI): the only consumer is `WindowChromeStyle::default()`. The struct
    // stays `#[non_exhaustive]` so a builder is re-added (one method, the
    // specific field) the round a binding actually customizes that token —
    // e.g. a tool window that hides minimize, or a themed title-bar colour.
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

/// Border thickness of a resize edge in logical pixels.
const RESIZE_EDGE_PX: u32 = 6;
/// Side length of a resize corner hit region in logical pixels. Larger than
/// the edge so the diagonal (two-axis) corner resize has a forgiving target.
const RESIZE_CORNER_PX: u32 = 12;

/// Inject the eight client-side window-resize hit regions around the border
/// of `scene` (R1122).
///
/// A borderless window (`decorations: false`) has no OS frame, so the
/// edge / corner drag-resize the window manager normally provides is gone.
/// This restores it as introspectable [`Scene`] nodes: four edges (N / S /
/// W / E) and four corners (NW / NE / SW / SE), each a transparent, tagged
/// [`Scene::Box`] the shell maps to a `winit::window::ResizeDirection` and
/// drives via `Window::drag_resize_window`. The regions are visible in
/// `scene/snapshot` so an AI agent can observe them (§2 #7) — the same
/// reason custom chrome beats OS chrome.
///
/// Returns the scene **unchanged** when `viewport` is `None` (a headless RPC
/// drive that never sized a window) or degenerate, mirroring
/// [`inject_window_chrome`].
///
/// ## Why flat siblings, not one container
///
/// The regions are pushed as FLAT top-level children, not wrapped in a
/// bounding sub-container. A resize border bounds the whole window, and
/// [`Scene::hit_test`] resolves a container that contains the point but
/// whose children do not as the container ITSELF — so a full-window resize
/// container would absorb every click in the window center. Flat thin
/// regions let a center click fall through to the content sibling, while a
/// border click still resolves to its region. Corners are pushed AFTER the
/// edges so they win in the overlap (last child = topmost in `hit_test`).
///
/// ## Layering with the chrome strip
///
/// The caller injects the resize border BEFORE [`inject_window_chrome`], so
/// the chrome strip's controls and drag grip sit on top of the north edge /
/// top corners and keep receiving clicks. The window resizes from its sides,
/// bottom, and bottom corners; the two TOP corners stay owned by the strip
/// (their diagonal resize yields to the corner controls). The NORTH edge,
/// however, is lifted back ON TOP by [`raise_top_resize_edge`] (R1195) so the
/// outermost `RESIZE_EDGE_PX` of the title bar still resize the window
/// vertically — the VS Code / Win11 / GTK behaviour (a title bar is moved from
/// its bulk, resized from its very top edge), NOT the "top owned by the title
/// bar" trade-off the pre-R1195 layering left.
///
/// Idempotent: any pre-existing resize region (tag prefixed
/// [`WINDOW_RESIZE_TAG_PREFIX`]) is stripped before the fresh set is appended.
#[must_use]
pub fn inject_resize_border(scene: Scene, viewport: Option<(u32, u32)>) -> Scene {
    inject_resize_regions(scene, viewport, true)
}

/// (R1186 §5.16 §5.39) Resize border for a window whose TOP EDGE is owned by a
/// CONTENT title bar — a dock panel HEADER that hosts the window controls
/// (min / max / close) and is itself the move handle (the R1171
/// controls-in-header design, `window_chrome == None`). The north edge and the
/// two TOP corners are OMITTED, so the header keeps its full width — including
/// the right-anchored close button — unshadowed; the window resizes from the
/// sides, the bottom, and the two BOTTOM corners.
///
/// ## Contract for the header's controls
///
/// Dropping the north edge + top corners clears the header top. The west / east
/// edges still span the FULL height (their top `RESIZE_EDGE_PX` overlaps the
/// header's left / right border), so a binding hosting controls in this header
/// must keep them at least `RESIZE_EDGE_PX` (6 px) from the left / right window
/// edge — the standard title-bar side padding every real CSD app uses, e.g. the R1171
/// header's 8 px side padding clears the 6 px edge. (This is the same property
/// the side edges have in every window: content flush to a side edge is a resize
/// grab, so controls are never placed flush to it.)
///
/// Contrast [`inject_resize_border`] (all eight regions), used when a SHELL
/// chrome strip is layered ON TOP of the border and reclaims the top edge
/// itself — there the north regions are dead under the strip, so keeping them is
/// harmless. A content header cannot be layered over the border (it is painted
/// BEFORE the shell overlays, so `hit_test`'s last-child-wins would let the top
/// resize regions shadow the close button at the very corner a user reaches for
/// to close). Dropping the top regions is the same "the top edge is owned by the
/// title bar" CSD trade-off (GTK / Win11 caption windows, VS Code / Blender
/// floating panels) the chrome case achieves by layering — a title-bar window is
/// MOVED from its top, not resized.
#[must_use]
pub fn inject_resize_border_below_titlebar(scene: Scene, viewport: Option<(u32, u32)>) -> Scene {
    inject_resize_regions(scene, viewport, false)
}

/// Shared implementation of the two resize-border variants. `include_top` gates
/// the north edge + the two top corners (kept for a chrome-covered top, dropped
/// for a content-header-owned top — see [`inject_resize_border_below_titlebar`]).
fn inject_resize_regions(scene: Scene, viewport: Option<(u32, u32)>, include_top: bool) -> Scene {
    let Some((w, h)) = viewport else {
        return scene;
    };
    if w < RESIZE_CORNER_PX || h < RESIZE_CORNER_PX {
        return scene;
    }
    let e = RESIZE_EDGE_PX;
    let c = RESIZE_CORNER_PX;

    // Edges first (each spans the full side); corners after so they win in the
    // overlap. The north edge + the two top corners are gated on `include_top`:
    // a content-header title bar owns the top (R1186), a shell chrome strip
    // covers it. The side edges span the full height either way — their top
    // `RESIZE_EDGE_PX` sit over the header's left / right PADDING (controls are
    // kept clear of it, per this fn's rustdoc contract), never over a control.
    let mut regions: Vec<(Rect, &'static str)> = Vec::with_capacity(8);
    if include_top {
        regions.push((Rect::new(0, 0, w, e), WINDOW_RESIZE_NORTH_TAG));
    }
    regions.push((Rect::new(0, h - e, w, e), WINDOW_RESIZE_SOUTH_TAG));
    regions.push((Rect::new(0, 0, e, h), WINDOW_RESIZE_WEST_TAG));
    regions.push((Rect::new(w - e, 0, e, h), WINDOW_RESIZE_EAST_TAG));
    if include_top {
        regions.push((Rect::new(0, 0, c, c), WINDOW_RESIZE_NORTH_WEST_TAG));
        regions.push((Rect::new(w - c, 0, c, c), WINDOW_RESIZE_NORTH_EAST_TAG));
    }
    regions.push((Rect::new(0, h - c, c, c), WINDOW_RESIZE_SOUTH_WEST_TAG));
    regions.push((Rect::new(w - c, h - c, c, c), WINDOW_RESIZE_SOUTH_EAST_TAG));

    let mut wrapped = wrap_into_container(scene);
    strip_children_with_prefix(&mut wrapped, WINDOW_RESIZE_TAG_PREFIX);
    for (rect, tag) in regions {
        push_top_level(
            &mut wrapped,
            Scene::Box(BoxNode::new(rect, BoxStyle::filled(Color::TRANSPARENT)).with_tag(tag)),
        );
    }
    wrapped
}

/// (R1195 §5.16 §5.39) Re-layer the north resize edge ON TOP of a shell chrome
/// strip so a chromed window's TOP EDGE stays a live resize band.
///
/// [`inject_resize_border`] injects all eight regions UNDER the strip (border
/// first, strip next), so the strip's grip shadows the north band and the top
/// edge cannot resize. That was documented as "the conventional CSD trade-off"
/// — but it is not: VS Code, Win11, GTK, and macOS all keep the outermost
/// `RESIZE_EDGE_PX` of a custom title bar a resize grab (a title bar is MOVED
/// from its bulk, RESIZED from its very edge). This lifts JUST the north edge
/// back above the strip, so the top `RESIZE_EDGE_PX` resize the window — even
/// over the controls, exactly as VS Code — while the rest of the strip still
/// moves it. The existing R1189 hover-cursor mapping (`WINDOW_RESIZE_NORTH_TAG`
/// → `NsResize`) and the R1122 press routing (`drag_resize_window(North)`) light
/// up for free once the band is hit-reachable, so no new cursor / press wiring.
///
/// The top corners (NW / NE) are deliberately NOT raised: the NE corner would
/// shadow the close button at the very corner a user reaches for (the R1186
/// concern), so the top edge resizes vertically only — the two top corners keep
/// diagonal resize off, matching the "title bar owns the corner controls" shape.
///
/// **Self-gating:** a `WINDOW_RESIZE_NORTH_TAG` band exists iff the window is a
/// resizable, non-maximized, chromed window (the only branch
/// [`inject_resize_border`] injects it), so this is a pure no-op for every other
/// window — no window-policy re-resolution needed. Idempotent: the existing
/// north band is stripped and re-pushed as the topmost child. Returns the scene
/// unchanged for a `None` / degenerate viewport, mirroring the sibling injectors.
#[must_use]
pub fn raise_top_resize_edge(scene: Scene, viewport: Option<(u32, u32)>) -> Scene {
    let Some((w, h)) = viewport else {
        return scene;
    };
    if w < RESIZE_CORNER_PX || h < RESIZE_CORNER_PX {
        return scene;
    }
    // Only a resizable chromed window has a north band under its strip; every
    // other window is left untouched (no top resize band is conjured for a
    // non-resizable / chrome-less / maximized window).
    if !has_top_level_tag(&scene, WINDOW_RESIZE_NORTH_TAG) {
        return scene;
    }
    let mut wrapped = wrap_into_container(scene);
    strip_tag(&mut wrapped, WINDOW_RESIZE_NORTH_TAG);
    push_top_level(
        &mut wrapped,
        Scene::Box(
            BoxNode::new(
                Rect::new(0, 0, w, RESIZE_EDGE_PX),
                BoxStyle::filled(Color::TRANSPARENT),
            )
            .with_tag(WINDOW_RESIZE_NORTH_TAG),
        ),
    );
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

    // ---- R1122 resize border ----

    const ALL_RESIZE_TAGS: [&str; 8] = [
        WINDOW_RESIZE_NORTH_TAG,
        WINDOW_RESIZE_SOUTH_TAG,
        WINDOW_RESIZE_WEST_TAG,
        WINDOW_RESIZE_EAST_TAG,
        WINDOW_RESIZE_NORTH_WEST_TAG,
        WINDOW_RESIZE_NORTH_EAST_TAG,
        WINDOW_RESIZE_SOUTH_WEST_TAG,
        WINDOW_RESIZE_SOUTH_EAST_TAG,
    ];

    #[test]
    fn resize_unknown_or_degenerate_viewport_returns_scene_unchanged() {
        let none = inject_resize_border(empty(), None);
        let tiny = inject_resize_border(empty(), Some((4, 4)));
        for tag in ALL_RESIZE_TAGS {
            assert!(find_tag(&none, tag).is_none(), "no {tag} for None viewport");
            assert!(find_tag(&tiny, tag).is_none(), "no {tag} for tiny viewport");
        }
    }

    #[test]
    fn injects_all_eight_resize_regions() {
        let out = inject_resize_border(empty(), Some((800, 600)));
        for tag in ALL_RESIZE_TAGS {
            assert!(find_tag(&out, tag).is_some(), "resize region {tag} present");
            // Every resize region shares the strip-by-prefix family tag.
            assert!(
                tag.starts_with(WINDOW_RESIZE_TAG_PREFIX),
                "{tag} carries the resize family prefix",
            );
        }
    }

    // ---- R1195 raise_top_resize_edge (VS Code / Win11 top-edge resize band) ----

    fn last_top_level_tag(scene: &Scene) -> Option<&str> {
        match scene {
            Scene::Container(c) => c.children.last().and_then(pinion_core::Scene::tag),
            _ => None,
        }
    }

    /// A resizable chromed window: resize border (north UNDER) then the strip
    /// (layered OVER), matching the shell's `apply_resize_border` +
    /// `apply_window_chrome` order.
    fn chromed_resizable(w: u32, h: u32) -> Scene {
        inject_window_chrome(
            inject_resize_border(empty(), Some((w, h))),
            "t",
            false,
            Some((w, h)),
            WindowChromeStyle::default(),
        )
    }

    #[test]
    fn raise_lifts_the_north_band_above_the_chrome_strip() {
        let chromed = chromed_resizable(800, 600);
        // Before: the strip is the topmost (last) child, shadowing the north band.
        assert_eq!(last_top_level_tag(&chromed), Some(WINDOW_CHROME_TAG));
        let raised = raise_top_resize_edge(chromed, Some((800, 600)));
        // After: the north band is the topmost child → it wins the top edge in
        // `hit_test`'s last-child-wins.
        assert_eq!(last_top_level_tag(&raised), Some(WINDOW_RESIZE_NORTH_TAG));
        // Exactly one north band survives (moved, not duplicated).
        assert_eq!(count_tag(&raised, WINDOW_RESIZE_NORTH_TAG), 1);
        // It spans the full width at the very top, `RESIZE_EDGE_PX` tall.
        let r = rect_of(find_tag(&raised, WINDOW_RESIZE_NORTH_TAG).unwrap());
        assert_eq!((r.x, r.y, r.w, r.h), (0, 0, 800, RESIZE_EDGE_PX));
        // The strip and every other resize region survive the raise.
        assert_eq!(count_tag(&raised, WINDOW_CHROME_TAG), 1);
        for tag in ALL_RESIZE_TAGS {
            assert!(find_tag(&raised, tag).is_some(), "{tag} survives the raise");
        }
    }

    #[test]
    fn raise_is_idempotent() {
        let once = raise_top_resize_edge(chromed_resizable(640, 480), Some((640, 480)));
        let twice = raise_top_resize_edge(once, Some((640, 480)));
        assert_eq!(
            count_tag(&twice, WINDOW_RESIZE_NORTH_TAG),
            1,
            "still exactly one north band after a second raise",
        );
        assert_eq!(last_top_level_tag(&twice), Some(WINDOW_RESIZE_NORTH_TAG));
    }

    #[test]
    fn raise_is_a_noop_without_a_north_band() {
        // A chrome-less resizable window uses the below-titlebar border (no
        // north), so there is nothing to raise — untouched.
        let below = inject_resize_border_below_titlebar(empty(), Some((800, 600)));
        assert!(find_tag(&below, WINDOW_RESIZE_NORTH_TAG).is_none());
        let raised = raise_top_resize_edge(below, Some((800, 600)));
        assert!(
            find_tag(&raised, WINDOW_RESIZE_NORTH_TAG).is_none(),
            "no north band is conjured for a chrome-less window",
        );
        // A bare scene (a non-resizable window has no border at all) is likewise
        // untouched.
        assert_eq!(
            count_tag(
                &raise_top_resize_edge(empty(), Some((800, 600))),
                WINDOW_RESIZE_NORTH_TAG
            ),
            0,
        );
    }

    #[test]
    fn raise_unknown_or_degenerate_viewport_returns_scene_unchanged() {
        // None / degenerate viewport is a pure no-op: the north band stays under
        // the strip (strip remains the topmost child).
        let none = raise_top_resize_edge(chromed_resizable(640, 480), None);
        assert_eq!(
            last_top_level_tag(&none),
            Some(WINDOW_CHROME_TAG),
            "None = no-op"
        );
        let tiny = raise_top_resize_edge(chromed_resizable(640, 480), Some((4, 4)));
        assert_eq!(
            last_top_level_tag(&tiny),
            Some(WINDOW_CHROME_TAG),
            "tiny = no-op"
        );
    }

    #[test]
    fn window_control_mapping_covers_exactly_the_three_discrete_controls() {
        // R1188 — the shared tag→control vocabulary: the three discrete control
        // tags map, and every pointer-session tag (grip / resize) maps to None
        // (their RPC peers are scene/window_move / scene/resize, not a click).
        assert_eq!(
            window_control_for_tag(WINDOW_CHROME_MINIMIZE_TAG),
            Some(WindowControl::Minimize)
        );
        assert_eq!(
            window_control_for_tag(WINDOW_CHROME_MAXIMIZE_TAG),
            Some(WindowControl::Maximize)
        );
        assert_eq!(
            window_control_for_tag(WINDOW_CHROME_CLOSE_TAG),
            Some(WindowControl::Close)
        );
        assert_eq!(window_control_for_tag(WINDOW_CHROME_GRIP_TAG), None);
        for tag in ALL_RESIZE_TAGS {
            assert_eq!(window_control_for_tag(tag), None, "{tag} is not discrete");
        }
        assert_eq!(window_control_for_tag("some-widget"), None);
    }

    #[test]
    fn chrome_tag_semantic_is_the_full_tag_to_meaning_ssot() {
        // R1190 — the single tag→semantic source: every chrome tag (controls,
        // grip, all 8 resize edges/corners) maps to its shell-neutral ChromeTag,
        // and non-chrome tags to None. The shell's exhaustive match on ChromeTag
        // + the type system then enforce cross-crate coverage.
        assert_eq!(
            chrome_tag_semantic(WINDOW_CHROME_MINIMIZE_TAG),
            Some(ChromeTag::Control(WindowControl::Minimize)),
        );
        assert_eq!(
            chrome_tag_semantic(WINDOW_CHROME_CLOSE_TAG),
            Some(ChromeTag::Control(WindowControl::Close)),
        );
        assert_eq!(
            chrome_tag_semantic(WINDOW_CHROME_GRIP_TAG),
            Some(ChromeTag::MoveGrip),
        );
        let resize_cases = [
            (WINDOW_RESIZE_NORTH_TAG, WindowResizeEdge::North),
            (WINDOW_RESIZE_SOUTH_TAG, WindowResizeEdge::South),
            (WINDOW_RESIZE_WEST_TAG, WindowResizeEdge::West),
            (WINDOW_RESIZE_EAST_TAG, WindowResizeEdge::East),
            (WINDOW_RESIZE_NORTH_WEST_TAG, WindowResizeEdge::NorthWest),
            (WINDOW_RESIZE_NORTH_EAST_TAG, WindowResizeEdge::NorthEast),
            (WINDOW_RESIZE_SOUTH_WEST_TAG, WindowResizeEdge::SouthWest),
            (WINDOW_RESIZE_SOUTH_EAST_TAG, WindowResizeEdge::SouthEast),
        ];
        for (tag, edge) in resize_cases {
            assert_eq!(
                chrome_tag_semantic(tag),
                Some(ChromeTag::Resize(edge)),
                "{tag}"
            );
        }
        // The strip container + resize family prefix are NOT themselves tags.
        assert_eq!(chrome_tag_semantic(WINDOW_CHROME_TAG), None);
        assert_eq!(chrome_tag_semantic(WINDOW_RESIZE_TAG_PREFIX), None);
        assert_eq!(chrome_tag_semantic("some-widget"), None);
    }

    #[test]
    fn below_titlebar_omits_north_regions_keeps_sides_and_bottom() {
        // R1186 — the content-header variant drops the north edge + the two top
        // corners (the dock header owns the top: move handle + close button), and
        // keeps the south / west / east edges + the two bottom corners.
        let out = inject_resize_border_below_titlebar(empty(), Some((800, 600)));
        for tag in [
            WINDOW_RESIZE_SOUTH_TAG,
            WINDOW_RESIZE_WEST_TAG,
            WINDOW_RESIZE_EAST_TAG,
            WINDOW_RESIZE_SOUTH_WEST_TAG,
            WINDOW_RESIZE_SOUTH_EAST_TAG,
        ] {
            assert!(find_tag(&out, tag).is_some(), "below-titlebar keeps {tag}");
        }
        for tag in [
            WINDOW_RESIZE_NORTH_TAG,
            WINDOW_RESIZE_NORTH_WEST_TAG,
            WINDOW_RESIZE_NORTH_EAST_TAG,
        ] {
            assert!(find_tag(&out, tag).is_none(), "below-titlebar omits {tag}");
        }
    }

    #[test]
    fn resize_region_geometry_spans_edges_and_corners() {
        let out = inject_resize_border(empty(), Some((800, 600)));
        let e = RESIZE_EDGE_PX;
        let c = RESIZE_CORNER_PX;
        // Edges: each spans its full side, EDGE px thick.
        assert_eq!(
            rect_of(find_tag(&out, WINDOW_RESIZE_NORTH_TAG).unwrap()),
            Rect::new(0, 0, 800, e)
        );
        assert_eq!(
            rect_of(find_tag(&out, WINDOW_RESIZE_SOUTH_TAG).unwrap()),
            Rect::new(0, 600 - e, 800, e),
        );
        assert_eq!(
            rect_of(find_tag(&out, WINDOW_RESIZE_WEST_TAG).unwrap()),
            Rect::new(0, 0, e, 600)
        );
        assert_eq!(
            rect_of(find_tag(&out, WINDOW_RESIZE_EAST_TAG).unwrap()),
            Rect::new(800 - e, 0, e, 600),
        );
        // Corners: CORNER px square anchored at each window corner.
        assert_eq!(
            rect_of(find_tag(&out, WINDOW_RESIZE_NORTH_WEST_TAG).unwrap()),
            Rect::new(0, 0, c, c)
        );
        assert_eq!(
            rect_of(find_tag(&out, WINDOW_RESIZE_NORTH_EAST_TAG).unwrap()),
            Rect::new(800 - c, 0, c, c),
        );
        assert_eq!(
            rect_of(find_tag(&out, WINDOW_RESIZE_SOUTH_WEST_TAG).unwrap()),
            Rect::new(0, 600 - c, c, c),
        );
        assert_eq!(
            rect_of(find_tag(&out, WINDOW_RESIZE_SOUTH_EAST_TAG).unwrap()),
            Rect::new(800 - c, 600 - c, c, c),
        );
    }

    #[test]
    fn resize_regions_are_flat_siblings_not_a_bounding_container() {
        // The regions must NOT be wrapped in one full-window container, which
        // `Scene::hit_test` would resolve as the hit for any center click
        // (no-child-hit ⇒ the container itself). Flat siblings let the center
        // fall through to content. Assert there is no container tagged with the
        // resize family prefix, only leaf Boxes.
        let out = inject_resize_border(empty(), Some((800, 600)));
        if let Scene::Container(c) = &out {
            for child in &c.children {
                if let Some(t) = child.tag() {
                    if t.starts_with(WINDOW_RESIZE_TAG_PREFIX) {
                        assert!(
                            matches!(child, Scene::Box(_)),
                            "resize region {t} is a flat Box sibling, not a container",
                        );
                    }
                }
            }
        } else {
            panic!("wrapped scene is a container");
        }
    }

    /// A full-window container (real shell scenes span the window, so the
    /// wrap container's rect contains the perimeter regions and `hit_test`
    /// can descend into them; a default `empty()` container has a `(0,0,0,0)`
    /// rect and would gate descent off).
    fn full_window(w: u32, h: u32) -> Scene {
        let mut c = ContainerNode::new(vec![]);
        c.rect = Rect::new(0, 0, w, h);
        Scene::Container(c)
    }

    #[test]
    fn corner_wins_over_edge_in_the_overlap() {
        // Corners are pushed after edges, so `Scene::hit_test` (topmost = last
        // child) resolves the top-right corner over the north / east edges.
        let out = inject_resize_border(full_window(800, 600), Some((800, 600)));
        // A point inside the NE corner square (and also inside the N + E edges).
        let hit = out.hit_test(795, 3).expect("hit inside NE corner");
        assert_eq!(
            hit.segments.last().map(String::as_str),
            Some(WINDOW_RESIZE_NORTH_EAST_TAG),
            "the corner wins where it overlaps the edges",
        );
    }

    #[test]
    fn center_click_falls_through_resize_border_to_content() {
        // A tagged content box under a full-window resize border: a center
        // click must reach the content, not snag on the (flat, thin) regions.
        let mut content = ContainerNode::new(vec![Scene::Box(
            BoxNode::new(
                Rect::new(0, 0, 800, 600),
                BoxStyle::filled(Color::rgb(0, 0, 0)),
            )
            .with_tag("content"),
        )]);
        content.rect = Rect::new(0, 0, 800, 600);
        let out = inject_resize_border(Scene::Container(content), Some((800, 600)));
        let hit = out.hit_test(400, 300).expect("center hits something");
        assert_eq!(
            hit.segments.last().map(String::as_str),
            Some("content"),
            "center click falls through the resize border to content",
        );
    }

    #[test]
    fn resize_idempotent_reinjection_replaces() {
        let once = inject_resize_border(empty(), Some((640, 480)));
        let twice = inject_resize_border(once, Some((640, 480)));
        for tag in ALL_RESIZE_TAGS {
            assert_eq!(
                count_tag(&twice, tag),
                1,
                "exactly one {tag} after re-inject"
            );
        }
    }

    #[test]
    fn maximize_glyph_switches_to_restore_when_maximized() {
        // R1123 — the maximize button draws a single square outline (5 path
        // commands: MoveTo + 4 LineTo/Close) when restorable-to-maximized, and
        // two offset square outlines (10 commands = the "restore" affordance)
        // when already maximized. This is the glyph the threaded `is_maximized`
        // flag selects.
        let rect = Rect::new(0, 0, 46, 32);
        let c = Color::rgb(0xE0, 0xE0, 0xE0);
        let maximize = glyph_path(rect, ButtonKind::Maximize, false, c);
        let restore = glyph_path(rect, ButtonKind::Maximize, true, c);
        assert_eq!(maximize.commands.len(), 5, "maximize = one square outline");
        assert_eq!(restore.commands.len(), 10, "restore = two offset squares");
    }
}
