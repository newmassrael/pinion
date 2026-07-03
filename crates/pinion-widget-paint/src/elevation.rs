//! R711 §5.50 — shared Material-style elevation shadow ramp.
//!
//! The single home for the drop-shadow [`BoxShadow`] list a surface
//! casts at a given Material elevation level. Lifted from the R710
//! `hello-elevation` binding once a second / third / fourth consumer
//! appeared — the modal [`crate::dialog`] panel (Level 3), the
//! [`crate::menu`] dropdown (Level 2), and the modal [`crate::drawer`]
//! panel (Level 1) — well past the Rule of Three, so a single
//! authoritative ramp replaces the per-binding copy
//! ([[abstraction-needs-second-consumer]]).
//!
//! ## What is and isn't a Material claim
//!
//! The component *levels* below ([`DIALOG_LEVEL`] / [`MENU_LEVEL`] /
//! [`DRAWER_LEVEL`]) are the real MD3 elevation tokens each surface sits
//! at. The level → shadow mapping ([`elevation`]) is a *Material-style*
//! key + ambient approximation, **not** a claim of bit-exact MD3 dp
//! values — it is a deliberate, replaceable rendering choice. Surfaces
//! that MD3 paints flat (a plain `tooltip`, Level 0) intentionally take
//! no shadow rather than being forced into one.

use pinion_core::style::{BoxShadow, Color};

/// Key (umbra) shadow colour — black at ~30 % alpha. The sharper,
/// further-offset of the two casts.
const KEY_SHADOW: Color = Color::rgba(0x00, 0x00, 0x00, 0x4d);
/// Ambient (penumbra) shadow colour — black at ~15 % alpha. The softer,
/// wider cast.
const AMBIENT_SHADOW: Color = Color::rgba(0x00, 0x00, 0x00, 0x26);

/// MD3 modal-dialog elevation (Level 3).
pub const DIALOG_LEVEL: u8 = 3;
/// MD3 menu / dropdown elevation (Level 2).
pub const MENU_LEVEL: u8 = 2;
/// MD3 modal navigation-drawer elevation (Level 1).
pub const DRAWER_LEVEL: u8 = 1;

// Compile-time lock on the MD3 token order: a dialog floats above a menu
// floats above a modal drawer. A future edit that reorders these fails
// to compile rather than silently inverting the visual hierarchy.
const _: () = assert!(DIALOG_LEVEL > MENU_LEVEL && MENU_LEVEL > DRAWER_LEVEL);

/// The Material-style key + ambient drop-shadow list for `level`
/// (empty for level 0). The key cast is sharper and offset further; the
/// ambient is softer and wider. Both offset and blur scale linearly
/// with the level, so a higher surface casts a larger, darker shadow.
///
/// Lowered by the Vello `paint_adapter` to a native
/// blurred-rounded-rect primitive (R710 §5.50).
#[must_use]
pub fn elevation(level: u8) -> Vec<BoxShadow> {
    if level == 0 {
        return Vec::new();
    }
    let l = f32::from(level);
    let key = BoxShadow::new(KEY_SHADOW)
        .with_offset(0.0, l)
        .with_blur(l * 1.5);
    let ambient = BoxShadow::new(AMBIENT_SHADOW)
        .with_offset(0.0, l * 0.5)
        .with_blur(l * 3.0);
    vec![key, ambient]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_zero_casts_nothing() {
        assert!(elevation(0).is_empty());
    }

    #[test]
    fn ramp_is_key_plus_ambient() {
        let e = elevation(DIALOG_LEVEL);
        assert_eq!(e.len(), 2, "key + ambient pair");
        assert_eq!(e[0].color, KEY_SHADOW, "key first");
        assert_eq!(e[1].color, AMBIENT_SHADOW, "ambient second");
    }

    #[test]
    fn higher_level_casts_larger() {
        let drawer = elevation(DRAWER_LEVEL);
        let dialog = elevation(DIALOG_LEVEL);
        assert!(dialog[0].blur > drawer[0].blur, "blur scales with level");
        assert!(dialog[0].offset_y > drawer[0].offset_y, "offset scales");
    }
    // The MD3 component-level ordering is locked at compile time by the
    // `const _: () = assert!(..)` above, not a runtime test.
}
