//! R715 §5.16 §5.40 — shared **dismiss barrier**: the non-modal,
//! fully-transparent click-catcher every *light-dismiss* overlay places
//! behind itself so a click anywhere outside the overlay closes it.
//!
//! ## Dismiss barrier vs modal scrim
//!
//! [`crate::scrim::scrim_backdrop`] is the *modal* backdrop: it **dims** (a translucent black fill) and
//! **traps** (it is the full-window topmost hit target, swallowing every
//! background interaction while a dialog / drawer is up). A dismiss barrier is
//! its non-modal sibling — **invisible** ([`Color::TRANSPARENT`], no fill at all) and
//! **passive** (its only job is to catch the *outside* click that dismisses
//! the popup; the rest of the UI keeps painting and reading normally behind
//! it). This is the desktop "popup grab" layer: `Popup`'s mouse grab, another
//! retained-mode toolkit's transparent route barrier, and Jetpack another
//! declarative toolkit's focusable `Popup` are all this same full-window
//! catch-the-outside rectangle.
//!
//! ## Why this module exists (R715 SSOT lift)
//!
//! R714 `hello-combobox` introduced the transparent barrier by reusing
//! the modal scrim at zero opacity — clever, but semantically a *modal
//! scrim pretending to be a dismiss barrier* (it dimmed by 0 and routed
//! through the scrim's fill helper for a popup that is not modal). R715
//! `hello-menu` is the 2nd consumer (click-outside-to-dismiss for the
//! menubar), so per `[[abstraction-needs-second-consumer]]` the shared
//! shape lifts to one home that owns its own transparency — a
//! [`Color::TRANSPARENT`], tagged, absolutely-positioned region
//! container with **no dependency on the scrim module**.
//!
//! Only the **paint construction** is shared — *dispatch* stays
//! per-binding. `ComboBox` tags its barrier with an extra `ButtonExternal`
//! (`combo_barrier`) whose `click` intent a reducer turns into "close";
//! the menubar tags its barrier `menu#barrier` so the R51.42
//! composite-tag router feeds `send("barrier:PointerUp")` straight to
//! the one `MenuBarExternal`, which closes the open dropdown. Both paint
//! the identical rectangle; each wires the dismiss its own way.
//!
//! ## Region, not always full-window
//!
//! `ComboBox` covers the whole window (its single trigger sits *under* the
//! barrier, so re-clicking the trigger while open dismisses — the
//! toggle-close convention). A menubar must keep its title strip
//! clickable *above* the barrier so a click on a sibling title switches
//! menus instead of dismissing, so its barrier covers only the region
//! **below** the bar. Hence the `origin` + `size` parameters rather than
//! a hard-coded full viewport.

use pinion_core::scene::ContainerNode;
use pinion_core::style::{BoxStyle, LayoutStyle, Size};
use pinion_core::{Color, Scene};

/// R715 §5.16 — build a non-modal **dismiss barrier**: a transparent
/// [`Scene::Container`] tagged `tag`, absolutely positioned at `origin`
/// and sized to `size`, with no children. The barrier is a passive
/// click-catcher — it carries a fully transparent fill
/// ([`Color::TRANSPARENT`], so it paints nothing) and exists only to be
/// the topmost hit target over its region.
///
/// # Arguments
///
/// - `tag` — the barrier's hit-target tag. Bind it to a real `External`
///   (`ComboBox`'s `combo_barrier` `ButtonExternal`) or use the R51.42
///   composite form (`menu#barrier`) to route the outside click to an
///   existing composite handle. The dismiss wiring is the caller's.
/// - `origin` — `(left, top)` of the barrier rectangle. `(0, 0)` for a
///   full-window grab; `(0, bar_height)` to leave a menubar title strip
///   interactive above it.
/// - `size` — `(width, height)` of the barrier rectangle.
///
/// # Placement
///
/// Push the barrier into the root child list **after** the rest of the
/// page but **before** the overlay it guards (the dropdown / popup), so
/// the overlay hit-tests above the barrier (last sibling wins) while the
/// barrier hit-tests above the page beneath it.
#[must_use]
pub fn dismiss_barrier(tag: &'static str, origin: (u32, u32), size: (u32, u32)) -> Scene {
    let (left, top) = origin;
    let (width, height) = size;
    Scene::Container(
        ContainerNode::new(vec![])
            .with_tag(tag)
            .with_style(BoxStyle::filled(Color::TRANSPARENT))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(left, top)
                    .with_size(Size::px(width, height)),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::style::SizeValue;

    #[test]
    fn r715_barrier_is_transparent_tagged_and_childless() {
        let scene = dismiss_barrier("combo_barrier", (0, 0), (560, 420));
        let Scene::Container(c) = &scene else {
            panic!("dismiss barrier is a container");
        };
        assert_eq!(c.tag.as_deref(), Some("combo_barrier"));
        assert_eq!(
            c.style.fill,
            Color::TRANSPARENT,
            "dismiss barrier owns its transparency directly (no scrim dependency)"
        );
        assert!(c.children.is_empty(), "barrier is a passive click-catcher");
    }

    #[test]
    fn r715_barrier_anchors_at_origin_and_covers_region() {
        // Below-bar menubar region: leave the 40 px title strip above.
        let scene = dismiss_barrier("menu#barrier", (0, 40), (520, 280));
        let Scene::Container(c) = &scene else {
            panic!("dismiss barrier is a container");
        };
        assert_eq!(c.layout.absolute_position, Some((0, 40)));
        assert_eq!(c.layout.size.width, SizeValue::Px(520));
        assert_eq!(c.layout.size.height, SizeValue::Px(280));
    }

    #[test]
    fn r715_barrier_keeps_composite_tag_verbatim() {
        // The R51.42 router splits `menu#barrier` -> ("menu", "barrier");
        // the tag must survive the paint node untouched for that to work.
        let scene = dismiss_barrier("menu#barrier", (0, 40), (520, 280));
        assert_eq!(scene.tag(), Some("menu#barrier"));
    }
}
