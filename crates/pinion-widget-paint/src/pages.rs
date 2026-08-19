//! ★★★★★ R1695 §5.16 §5.38 §5.40 — the **paged region**: the part of a window
//! that shows one of a product's destinations.
//!
//! The model half is
//! [`destination`](pinion_core::widgets::destination) — a keyed roster, a
//! standing per destination, and a navigation that refuses with a reason. This
//! is where a destination becomes a rectangle.
//!
//! # One page is built, and that is the guarantee
//!
//! [`view_page_region`] takes a builder and hands it **only the destination the
//! journey is at**. The builder has no handle to any other destination, so the
//! pages that are not current are not constructed, are not in the scene, are
//! not in the accessibility tree, cannot be hit by the pointer router, and do
//! not appear in `scene/snapshot`.
//!
//! That is worth stating as a property rather than an implementation detail,
//! because the alternative is what every comparable toolkit does. Measured by
//! building a probe against the reference toolkit at 6.11.1 and running it: its
//! paged container keeps every page alive as a child and hides the
//! non-current ones with geometry alone — a hidden page, sent a press, a key
//! and a wheel, **counted all three**. Input scoping there is a guard the
//! author has to remember at every handler, and the prototype this project's
//! analysis tool is modelled on does exactly that: its wheel handler opens by
//! testing that the active section is its own. A guard that must be repeated is
//! a guard that will be forgotten once.
//!
//! # Why a tab strip is not a consumer of this
//!
//! Asked because two screens already render only their active panel
//! (`hello-tabs`, `hello-tabbed-chart`) and the rule of three would otherwise
//! point here. They are a different relationship, not a smaller version of this
//! one: WAI-ARIA binds a `tabpanel` to an owning `tab` in a `tablist`, their
//! selection is a [`RadioGroupExternal`] index rather than a key, and a tab that
//! cannot be opened *for a stated reason* is not a thing a tab strip has. This
//! function takes a [`Destination`] precisely because a destination is the thing
//! that can be closed and can say why. If a third consumer arrives that is
//! neither a rail nor a tab strip, the parameter is what should generalise — not
//! this module's shape.
//!
//! [`RadioGroupExternal`]: pinion_core::widgets::radio_group::RadioGroupExternal
//!
//! # Why the region carries a tag
//!
//! So that *arriving* is observable. A rail that highlights a seat moves a
//! string; a rail that navigates changes what is inside this rectangle, and a
//! gate can only tell those apart if the rectangle has a name. Measured on this
//! tree the day this module landed, one analysis-tool screen answered a press
//! on four of its seven destinations by moving the string and painting the same
//! 193 tagged regions as before.

use std::borrow::Cow;

use pinion_core::scene::{ContainerNode, Rect};
use pinion_core::style::{BoxStyle, LayoutStyle, Size};
use pinion_core::widgets::destination::Destination;
use pinion_core::{Color, Scene};

/// Build the region for the destination a journey is at.
///
/// `viewport` places and sizes the region in the enclosing frame. `page`
/// receives the current destination and returns its children in region-local
/// coordinates unless they declare an absolute position of their own — the same
/// convention every container in this crate uses.
///
/// The builder is called **once**, with one destination. A page that is not
/// current is never constructed.
///
/// # Who resolves a press inside the region is the caller's fact
///
/// It carries a tag, because *arriving* has to be observable and an unnamed
/// rectangle cannot be compared across destinations. The §5.35 router resolves
/// a press to the **deepest tagged node** under the cursor and then looks that
/// tag up as an `External` — so what the region must do with a press depends
/// entirely on what its page is made of, and [`PagePointer`] is the caller
/// saying which.
#[must_use]
pub fn view_page_region(
    tag: impl Into<Cow<'static, str>>,
    viewport: Rect,
    fill: Color,
    here: &Destination,
    pointer: PagePointer,
    page: impl FnOnce(&Destination) -> Vec<Scene>,
) -> Scene {
    Scene::Container(
        ContainerNode::new(page(here))
            .with_tag(tag)
            .with_style(BoxStyle::filled(fill))
            .with_layout(
                LayoutStyle::new()
                    .with_size(Size::px(viewport.w, viewport.h))
                    .with_absolute_position(viewport.x, viewport.y)
                    .with_pointer_transparent(matches!(pointer, PagePointer::HostResolves)),
            ),
    )
}

/// ★★★★★ R1724 — **what a press inside the page region is for.**
///
/// Two arms, because a paged region has two kinds of page and they need
/// opposite answers — and the first of them was written as an unconditional
/// truth until the second existed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PagePointer {
    /// The host hit-tests the page itself, from its own root surface.
    ///
    /// The region is pointer-transparent. Without that, a tagged region with a
    /// hit box becomes the target for every press anywhere on the page, finds
    /// no `External`, and forwards nothing — the whole destination dead to a
    /// mouse while every wire path keeps working. Not hypothetical: the first
    /// draft of this function omitted it and the consuming screen's R1649 gate
    /// went red on the first run.
    HostResolves,
    /// The page is a screen with surfaces of its own
    /// (`pinion_screen::ScreenRoster::page_scene`).
    ///
    /// ★★★★★ The region must be an ORDINARY container here, and getting this
    /// wrong is invisible in every gate that does not press. Measured on the
    /// day the first screen was mounted: pointer transparency in
    /// [`Scene::hit_test`](pinion_core::Scene::hit_test) skips a child **and
    /// its whole subtree** — the arm exists for overlays, where that is exactly
    /// right — so the mounted screen painted 139 regions, answered every wire
    /// path, appeared in the accessibility tree, reported `routed_by:
    /// node_lab`, and **not one press in it reached anything**. Neither the
    /// screen nor the host: the press resolved to the region's own tag, which
    /// no `External` answers.
    ///
    /// Ordinary is safe for the reason transparency was needed in the other
    /// arm: `hit_test` descends into children first and returns the container
    /// only when no child hits, and a mounted screen's root covers the region.
    PageResolves,
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use pinion_core::availability::Unavailable;
    use pinion_core::widgets::destination::{Destination, Destinations, Journey};

    use super::{PagePointer, Rect, Scene, view_page_region};

    fn roster() -> Destinations {
        Destinations::new(vec![
            Destination::open("dashboard", "Dashboard"),
            Destination::open("settings", "Settings"),
            Destination::closed(
                "stream",
                "Stream",
                Unavailable::elsewhere("the packet viewer"),
            ),
        ])
        .expect("roster")
    }

    /// ★★★★★ R1695 — **the page that is not current is not built.**
    ///
    /// Written as a counter rather than as a look at the scene, because the
    /// claim is about work that never happens: a scene assertion would pass
    /// equally well against a region that built all three pages and dropped
    /// two. The floor this is measured against builds and keeps every page, so
    /// this is the row of that comparison that a test can hold.
    #[test]
    fn r1695_only_the_current_destination_is_built() {
        let roster = roster();
        let mut journey = Journey::begin(&roster, "dashboard").expect("begin");
        let built: RefCell<Vec<String>> = RefCell::new(Vec::new());

        let make = |journey: &Journey| {
            view_page_region(
                "region",
                Rect::new(0, 0, 100, 100),
                pinion_core::Color::TRANSPARENT,
                journey.here(&roster),
                PagePointer::HostResolves,
                |here| {
                    built.borrow_mut().push(here.key.to_string());
                    Vec::new()
                },
            )
        };

        let _ = make(&journey);
        assert_eq!(built.borrow().as_slice(), ["dashboard"]);

        journey.navigate(&roster, "settings").expect("open");
        let _ = make(&journey);
        assert_eq!(
            built.borrow().as_slice(),
            ["dashboard", "settings"],
            "the region builds the destination it is at, and only that one"
        );

        // And a closed destination never reaches the builder at all, because
        // the journey refuses before the region is asked.
        journey.navigate(&roster, "stream").expect_err("closed");
        let _ = make(&journey);
        assert_eq!(
            built.borrow().as_slice(),
            ["dashboard", "settings", "settings"]
        );
    }

    /// R1695 — the region is a named rectangle, so a gate can ask what is
    /// inside it and a press can be aimed at it.
    #[test]
    fn r1695_the_region_is_tagged_and_placed() {
        let roster = roster();
        let journey = Journey::begin(&roster, "dashboard").expect("begin");
        let scene = view_page_region(
            "shell.page",
            Rect::new(52, 56, 800, 600),
            pinion_core::Color::TRANSPARENT,
            journey.here(&roster),
            PagePointer::HostResolves,
            |_| {
                vec![Scene::Container(pinion_core::scene::ContainerNode::new(
                    Vec::new(),
                ))]
            },
        );
        let Scene::Container(node) = &scene else {
            panic!("a region is a container");
        };
        assert_eq!(node.tag.as_deref(), Some("shell.page"));
        assert_eq!(node.children.len(), 1);
        assert_eq!(node.layout.absolute_position, Some((52, 56)));
        // ★★ The tag is an ADDRESS, never a hit target. Without this the router
        // resolves every press on the page to the region, finds no `External`
        // carrying that tag, and forwards nothing — the destination is dead to a
        // mouse while every wire path keeps working. The consuming screen's own
        // gate caught the first draft of this file omitting it.
        assert!(
            node.layout.pointer_transparent,
            "a page region that is its own hit target kills every control on \
             the page it holds",
        );
    }

    /// ★★★★★ R1724 — **and the opposite page needs the opposite answer, which
    /// is why the line above is a parameter now.**
    ///
    /// `pointer_transparent` in `Scene::hit_test` skips a child AND ITS WHOLE
    /// SUBTREE — the arm exists for overlays, where that is exactly right. In
    /// front of a page that is a screen with surfaces of its own it means the
    /// screen cannot be pressed at all. Measured the day the first screen was
    /// mounted: it painted 139 regions, answered every wire path, appeared in
    /// the accessibility tree, and not one press in it reached anything —
    /// neither the screen nor the host.
    ///
    /// Written against `hit_test` rather than against the flag, because the
    /// flag is the mechanism and reaching the child is the property.
    #[test]
    fn r1724_a_page_that_is_a_screen_is_reachable_by_the_pointer() {
        use pinion_core::scene::ContainerNode;
        use pinion_core::style::{LayoutStyle, Size};

        let roster = roster();
        let journey = Journey::begin(&roster, "dashboard").expect("begin");
        let viewport = Rect::new(52, 56, 800, 600);
        // A screen's root: a tagged container filling the region, the shape
        // `ScreenRoster::page_scene` hands back.
        let screen = || {
            vec![Scene::Container(
                ContainerNode::new(Vec::new())
                    .with_tag("mounted_screen")
                    .with_layout(
                        LayoutStyle::new()
                            .with_size(Size::px(viewport.w, viewport.h))
                            .with_absolute_position(viewport.x, viewport.y),
                    ),
            )]
        };
        let region = |pointer| {
            let mut scene = Scene::Container(ContainerNode::new(vec![view_page_region(
                "shell.page",
                viewport,
                pinion_core::Color::TRANSPARENT,
                journey.here(&roster),
                pointer,
                |_| screen(),
            )]));
            let mut cache = pinion_runtime::LayoutCache::new();
            pinion_runtime::compute_layout(&mut scene, &mut cache, 1440, 900);
            scene
        };

        let inside = (viewport.x + viewport.w / 2, viewport.y + viewport.h / 2);
        let deepest = |scene: &Scene| -> Option<String> {
            let hit = scene.hit_test(inside.0, inside.1)?;
            (0..=hit.segments.len()).rev().find_map(|k| {
                scene
                    .lookup_path_ref(&hit.segments[..k])
                    .and_then(|node| node.tag().map(str::to_owned))
            })
        };

        assert_eq!(
            deepest(&region(PagePointer::PageResolves)).as_deref(),
            Some("mounted_screen"),
            "a press inside the region reaches the SCREEN, which is the only \
             thing that can answer it",
        );
        assert_eq!(
            deepest(&region(PagePointer::HostResolves)),
            None,
            "and where the host resolves, the region and its page are both \
             invisible to the hit test, so the press falls through to whatever \
             the host paints it on -- its own root surface",
        );
    }
}
