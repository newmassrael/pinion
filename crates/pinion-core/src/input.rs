//! R56.1.f.0 §5.13 — Input-event modifier primitives shared between
//! the runtime (winit ↔ shell bridge) and the widget catalog
//! (`WidgetCore::apply_key`). Lives in `pinion-core` so widget code
//! can name [`Modifiers`] without a `pinion-runtime` dependency
//! (which would invert the crate graph — `pinion-runtime` depends on
//! `pinion-core`, not the reverse).
//!
//! Originally defined in `pinion-runtime/src/input.rs` (R51.108 §5.41
//! winit-free abstract surface); R56.1.f.0 lifted the type so the
//! `WidgetCore::apply_key` signature can carry the four-bit modifier
//! state directly into widget keystroke handling. The W3C
//! `KeyboardEvent` modifier surface (`shiftKey` / `ctrlKey` /
//! `altKey` / `metaKey`) is the industry-portable vocabulary every
//! desktop toolkit (winit, GTK, Qt, Cocoa) and every browser exposes
//! as independent booleans — refactoring to a bitflag here would
//! diverge from that substrate.
//!
//! The §5.41 TUI bridge (`pinion-tui::input::modifiers_from_crossterm`)
//! and the §5.35 GUI bridge (`pinion-runtime::input::InputRouter` via
//! `pinion-shell::app::modifiers_from_winit`) both construct
//! [`Modifiers`] from their respective platform vocabularies and
//! forward through the [`WidgetCore::apply_key`](crate::widget_core::WidgetCore::apply_key)
//! dispatch path — the substrate stays platform-agnostic.

/// R56.1.f.0 §5.13 — abstract modifier-key state, mirroring
/// `winit::keyboard::ModifiersState` and W3C DOM Level 3
/// `getModifierState` without the winit dependency. Four modifier
/// bits cover the desktop-portable baseline (Shift / Control / Alt /
/// Meta). Closed-form: future modifiers (`CapsLock` / `NumLock` /
/// Hyper) are rare enough that a `SemVer` minor bump is the
/// textbook extension path (rather than the §5.13-style
/// `#[non_exhaustive]` hedge which only applies cleanly to enum
/// variants where a wildcard arm has a meaningful default).
///
/// `clippy::struct_excessive_bools` lint is intentionally suppressed:
/// the four-bool shape mirrors the W3C `KeyboardEvent` modifier
/// surface (`shiftKey` / `ctrlKey` / `altKey` / `metaKey`), which
/// every browser and every desktop windowing toolkit (winit, GTK,
/// Qt, Cocoa) exposes as independent booleans — refactoring to a
/// bitflag or state-machine here would diverge from the industry
/// vocabulary substrate callers expect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct Modifiers {
    /// Shift key (left or right) currently held.
    pub shift: bool,
    /// Control key (left or right) currently held.
    pub ctrl: bool,
    /// Alt / Option key (left or right) currently held.
    pub alt: bool,
    /// Meta / Cmd / Super / Windows key currently held.
    pub meta: bool,
}

impl Modifiers {
    /// Zero-modifier state, matching `winit::keyboard::ModifiersState::empty`.
    /// Used by the substrate's `ShellCore::new` to initialise the
    /// modifier cache before the first `ModifiersChanged` event, and
    /// by RPC dispatch paths that surface a no-modifier keystroke
    /// (the `IntrospectValue::Text(key)` variant of
    /// `invoke("key", ...)` — see R56.1.d).
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            shift: false,
            ctrl: false,
            alt: false,
            meta: false,
        }
    }

    /// Shift-bit accessor matching the winit `ModifiersState::shift_key`
    /// method shape. Substrate callers read this for Tab-reverse focus
    /// traversal (R51.83 §5.40) and R56.1.f Shift-Arrow text selection.
    #[must_use]
    pub const fn shift_key(self) -> bool {
        self.shift
    }

    /// Control-bit accessor mirroring `winit::keyboard::ModifiersState::control_key`.
    #[must_use]
    pub const fn control_key(self) -> bool {
        self.ctrl
    }

    /// Alt-bit accessor mirroring `winit::keyboard::ModifiersState::alt_key`.
    #[must_use]
    pub const fn alt_key(self) -> bool {
        self.alt
    }

    /// Meta-bit accessor mirroring `winit::keyboard::ModifiersState::super_key`.
    #[must_use]
    pub const fn meta_key(self) -> bool {
        self.meta
    }

    /// `true` iff no modifier is held — convenience for the canonical
    /// "plain keystroke" branch in `apply_key` implementations
    /// (Shift+Arrow extends a text selection; plain Arrow moves the
    /// caret without selection).
    #[must_use]
    pub const fn is_empty(self) -> bool {
        !self.shift && !self.ctrl && !self.alt && !self.meta
    }
}

#[cfg(test)]
mod tests {
    //! R56.1.f.0 §5.13 — `Modifiers` regression battery. Covers the
    //! W3C `KeyboardEvent` accessor surface, `Default == empty()`
    //! identity, and the `is_empty` predicate used by `apply_key`
    //! plain-keystroke branches.

    use super::Modifiers;

    #[test]
    fn r56_1_f_0_empty_has_no_bits_set() {
        let m = Modifiers::empty();
        assert!(!m.shift);
        assert!(!m.ctrl);
        assert!(!m.alt);
        assert!(!m.meta);
        assert!(m.is_empty());
    }

    #[test]
    fn r56_1_f_0_default_equals_empty() {
        assert_eq!(Modifiers::default(), Modifiers::empty());
    }

    #[test]
    fn r56_1_f_0_accessors_mirror_w3c_surface() {
        let m = Modifiers {
            shift: true,
            ctrl: false,
            alt: true,
            meta: false,
        };
        assert!(m.shift_key());
        assert!(!m.control_key());
        assert!(m.alt_key());
        assert!(!m.meta_key());
        assert!(!m.is_empty());
    }

    #[test]
    fn r56_1_f_0_any_bit_breaks_is_empty() {
        for m in [
            Modifiers { shift: true, ctrl: false, alt: false, meta: false },
            Modifiers { shift: false, ctrl: true, alt: false, meta: false },
            Modifiers { shift: false, ctrl: false, alt: true, meta: false },
            Modifiers { shift: false, ctrl: false, alt: false, meta: true },
        ] {
            assert!(!m.is_empty(), "any single bit must break is_empty");
        }
    }

    #[test]
    fn r56_1_f_0_clone_copy_eq_round_trip() {
        let m = Modifiers {
            shift: true,
            ctrl: true,
            alt: false,
            meta: false,
        };
        let n = m;
        assert_eq!(m, n);
        let o = m;
        assert_eq!(m, o);
    }
}
