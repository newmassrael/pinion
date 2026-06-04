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
//!
//! R56.2.a §5.13 §5.38 — extends the surface with [`CompositionEvent`],
//! the W3C `CompositionEvent` mirror that platform IME bridges feed
//! into [`WidgetCore::apply_composition`](crate::widget_core::WidgetCore::apply_composition).
//! The four phases (`Start` / `Update` / `Commit` / `Cancel`) map 1:1
//! to the [`TextFieldExternal::apply_composition_*`](crate::widgets::text_field::TextFieldExternal)
//! substrate landed in R56.1.g. winit 0.30's
//! [`WindowEvent::Ime`](https://docs.rs/winit/0.30/winit/event/enum.WindowEvent.html#variant.Ime)
//! cross-platform abstraction is the canonical desktop bridge (Wayland
//! `text-input-v3` + X11 XIM + macOS `NSTextInputContext` + Windows
//! TSF all funnel through winit's four-variant `Ime` enum); the
//! pinion-shell `app.rs` Ime arm performs the
//! `winit::Ime → CompositionEvent` mapping with `was_composing` state
//! tracking so empty `Preedit` triggers `Cancel` and `Disabled`
//! cancels an in-flight session.

/// R56.1.f.0 §5.13 — abstract modifier-key state, mirroring
/// `winit::keyboard::ModifiersState` and W3C DOM Level 3
/// `getModifierState` without the winit dependency. Four modifier
/// bits cover the desktop-portable baseline (Shift / Control / Alt /
/// Meta). Closed-form: future modifiers (`CapsLock` / `NumLock` /
/// Hyper) are rare enough that a `SemVer` minor bump is the
/// textbook extension path (rather than the §5.13
/// `#[non_exhaustive]`-style hedge which only applies cleanly to enum
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

    /// R781 §5.35 §5.41 — encode the held modifiers into the compact wire
    /// token the R51.42 composite-send payload carries (`"<key>:<Event>:<token>"`).
    ///
    /// The token is the canonical-order subset of `scam` (shift, ctrl, alt,
    /// meta), so `Modifiers { shift: true, ctrl: true, .. }` → `"sc"`. An
    /// empty modifier state yields `""`, and the router omits the trailing
    /// `":<token>"` segment entirely (the two-segment back-compat wire every
    /// pre-R781 composite consumer already parses). Inverse of
    /// [`from_wire_token`](Self::from_wire_token) — the R773 encode↔decode
    /// SSOT discipline applied to the pointer modifier axis.
    #[must_use]
    pub fn as_wire_token(self) -> String {
        let mut token = String::new();
        if self.shift {
            token.push('s');
        }
        if self.ctrl {
            token.push('c');
        }
        if self.alt {
            token.push('a');
        }
        if self.meta {
            token.push('m');
        }
        token
    }

    /// R781 §5.35 §5.41 — decode a wire modifier token (any order of the
    /// `scam` letters) back into [`Modifiers`]. Inverse of
    /// [`as_wire_token`](Self::as_wire_token). Returns `None` on any letter
    /// outside `scam` so a malformed token is rejected rather than silently
    /// dropping bits (a stale wire from an older protocol revision surfaces
    /// as "no modifiers handled" at the decode site, not a misparse).
    #[must_use]
    pub fn from_wire_token(token: &str) -> Option<Self> {
        let mut m = Self::empty();
        for ch in token.chars() {
            match ch {
                's' => m.shift = true,
                'c' => m.ctrl = true,
                'a' => m.alt = true,
                'm' => m.meta = true,
                _ => return None,
            }
        }
        Some(m)
    }
}

/// R56.2.a §5.13 §5.38 — abstract IME composition phase event,
/// mirroring W3C UI Events `CompositionEvent` without a winit
/// dependency. Carries one of four phases that map 1:1 to the
/// [`TextFieldExternal::apply_composition_*`](crate::widgets::text_field::TextFieldExternal)
/// substrate landed in R56.1.g:
///
/// - [`CompositionEvent::Start`]: begin composition. Mirrors the W3C
///   `compositionstart` event (data is empty / not yet known). Callers
///   should fire this once per composition session before any
///   `Update`; the substrate is defensive against missing-start
///   (`Update` without a prior `Start` is a no-op at the
///   [`TextEditState`](crate::widgets::text_edit::TextEditState)
///   layer), but the SCXML transition that gates the caret-blink
///   posture only fires through `Start`.
/// - [`CompositionEvent::Update`]: replace the active preedit with
///   `text`. Mirrors W3C `compositionupdate` (the `data` field carries
///   the new preedit). Empty `text` is canonically just an empty
///   preedit (the user has deleted all in-flight characters but
///   composition stays open); use [`CompositionEvent::Commit`] or
///   [`CompositionEvent::Cancel`] to end the session.
/// - [`CompositionEvent::Commit`]: end composition by inserting `text`
///   at the caret. Mirrors W3C `compositionend` with non-empty `data`.
///   Empty `text` is the canonical "no-data compositionend" shape and
///   the substrate routes it through `preedit_cancel` (matches the
///   Wayland `text-input-v3` cancel-via-empty-commit behaviour).
/// - [`CompositionEvent::Cancel`]: end composition without inserting
///   any text. Mirrors IME cancel (Escape during preedit, blur with
///   discarded composition, `WindowEvent::Ime::Disabled` mid-flight).
///
/// `#[non_exhaustive]` reserves room for future Wayland-style
/// `delete_surrounding` (text replacement) and explicit
/// `set_surrounding` (context-aware IME) variants without a `SemVer`
/// break — winit 0.30's `Ime` enum stays at the four-variant shape
/// the cross-platform LCD supports today, so the substrate matches.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CompositionEvent {
    /// Begin a composition session. Substrate seeds the
    /// preedit buffer via [`TextEditState::preedit_start`](crate::widgets::text_edit::TextEditState::preedit_start)
    /// AND drives [`TextFieldEvent::BeginEdit`](crate::widgets::text_field::TextFieldEvent::BeginEdit)
    /// through the SCXML.
    Start,
    /// Replace the active preedit with the carried `String`. Updates
    /// the reactive preedit sidecar; the SCXML stays in `Editing`.
    Update(String),
    /// End the composition by inserting the carried `String` at the
    /// caret position. Empty string is the no-data compositionend
    /// shape and routes through cancel.
    Commit(String),
    /// End the composition without inserting any text.
    Cancel,
}

/// R773 §5.35 §5.13 — the W3C pointer-event name subset that the
/// composite-tag input router emits over the `send` wire to
/// command-class [`External`](crate::external::External) widgets.
///
/// This is the **wire vocabulary** for the `invoke("send", "<name>")`
/// channel: the [`InputRouter`](pinion_runtime::InputRouter) rewrites a
/// paint hit-target into a bare event name (or a `"<sub>:<name>"`
/// composite, see [`composite_tag`](crate::composite_tag)) and forwards
/// it; the receiving widget decodes the `<name>` half. Lifting the five
/// names into one enum makes the **encode** site (the router, via
/// [`as_wire_name`](Self::as_wire_name)) and every **decode** site (via
/// [`from_wire_name`](Self::from_wire_name)) reference a single
/// vocabulary instead of independent string literals — a divergence
/// between producer and consumer would be a silent wire bug (the router
/// emits a name no decoder recognises and the event vanishes), not a
/// style choice, so the pair lives once here (`decode == inverse(encode)`,
/// the R743.1 / R745 / R770.1 SSOT class).
///
/// Lives in `pinion-core::input` alongside [`Modifiers`] and
/// [`CompositionEvent`] — the shared input-event primitives both the
/// `pinion-runtime` router (producer) and the `pinion-core` /
/// `pinion-widget-paint` widget catalog (consumers) name without
/// inverting the crate graph.
///
/// Scope boundary: the per-widget SCE-emitted event enums
/// (`ButtonEvent`, `CheckboxEvent`, …) carry the *same* five pointer
/// names but derive them from `stringify!(VariantIdent)` via
/// [`widget_event_name!`](crate::widget_event_name) — a self-consistent,
/// SCXML-canonical vocabulary owned by each statechart, a *different*
/// decision (wire name → SCXML transition) that this enum does not fold.
/// The two vocabularies are pinned together by a cross-vocab test in
/// `widgets::button` so a rename on either side is caught at test time.
/// The keyboard-side `"KeyboardActivate"` token is a separate wire
/// vocabulary (not a pointer event) and is left to its callers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerWireEvent {
    /// `"PointerEnter"` — cursor entered the target (hover begins).
    Enter,
    /// `"PointerDown"` — primary button pressed over the target.
    Down,
    /// `"PointerUp"` — primary button released (the activate edge).
    Up,
    /// `"PointerLeave"` — cursor left the target (hover ends, or a
    /// mid-press stray under capture).
    Leave,
    /// `"PointerCancel"` — the pointer interaction was aborted.
    Cancel,
}

impl PointerWireEvent {
    /// Encode `self` into its canonical W3C wire name — the single
    /// source the router emits. Inverse of [`from_wire_name`](Self::from_wire_name).
    #[must_use]
    pub fn as_wire_name(self) -> &'static str {
        match self {
            PointerWireEvent::Enter => "PointerEnter",
            PointerWireEvent::Down => "PointerDown",
            PointerWireEvent::Up => "PointerUp",
            PointerWireEvent::Leave => "PointerLeave",
            PointerWireEvent::Cancel => "PointerCancel",
        }
    }

    /// Decode a W3C pointer-event name into a [`PointerWireEvent`];
    /// `None` for any other name (the caller rejects the `send` payload
    /// or treats it as out-of-vocabulary). Inverse of
    /// [`as_wire_name`](Self::as_wire_name).
    #[must_use]
    pub fn from_wire_name(name: &str) -> Option<Self> {
        match name {
            "PointerEnter" => Some(PointerWireEvent::Enter),
            "PointerDown" => Some(PointerWireEvent::Down),
            "PointerUp" => Some(PointerWireEvent::Up),
            "PointerLeave" => Some(PointerWireEvent::Leave),
            "PointerCancel" => Some(PointerWireEvent::Cancel),
            _ => None,
        }
    }
}

/// The keyboard-side activation token: the `send`-payload event name a focused
/// command widget receives on keyboard activation (Enter / Space), the
/// keyboard peer of the pointer-release activation edge ([`PointerWireEvent::Up`]).
/// One home for the literal on the **decode** side (the `*Event::KeyboardActivate`
/// SCE enums on the *emit* side own their own `stringify!` form — a separate,
/// statechart-bound vocabulary, per [`PointerWireEvent`]'s scope note).
pub const KEYBOARD_ACTIVATE_EVENT: &str = "KeyboardActivate";

/// R778 §5.35 — does this `send`-payload event name denote a command widget's
/// **activation edge**? True for the pointer release ([`PointerWireEvent::Up`])
/// and the keyboard activation token ([`KEYBOARD_ACTIVATE_EVENT`]).
///
/// The shared decode predicate for the command-widget `handle_send` decoders
/// that have no per-item SCE statechart — [`VirtualSelectExternal`](crate::widgets::virtual_select),
/// [`ViewSortFilterExternal`](crate::widgets::view_order), and
/// [`GridSortExternal`](crate::widgets::grid_sort) — lifted on the third
/// consumer (R778) so the set of events that count as "activate" cannot drift
/// between them (a divergence would be a routing bug, not a style choice). The
/// per-widget statecharts decode their own activation through
/// [`widget_event_name!`](crate::widget_event_name) + `detect`, a different
/// vocabulary this predicate does not fold.
#[must_use]
pub fn is_activation_event(event_name: &str) -> bool {
    event_name == KEYBOARD_ACTIVATE_EVENT || event_name == PointerWireEvent::Up.as_wire_name()
}

#[cfg(test)]
mod tests {
    //! R56.1.f.0 §5.13 — `Modifiers` regression battery. Covers the
    //! W3C `KeyboardEvent` accessor surface, `Default == empty()`
    //! identity, and the `is_empty` predicate used by `apply_key`
    //! plain-keystroke branches.

    use super::{is_activation_event, Modifiers, PointerWireEvent, KEYBOARD_ACTIVATE_EVENT};

    #[test]
    fn r778_activation_edge_is_pointer_up_or_keyboard_activate() {
        // The lifted command-widget activation predicate (R778): the two
        // events that count as "activate", and nothing else.
        assert!(is_activation_event(PointerWireEvent::Up.as_wire_name()));
        assert!(is_activation_event(KEYBOARD_ACTIVATE_EVENT));
        for name in ["PointerDown", "PointerEnter", "PointerLeave", "PointerCancel", ""] {
            assert!(!is_activation_event(name), "{name} is not an activation edge");
        }
    }

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
    fn r781_wire_token_round_trips_every_combination() {
        // Encode ↔ decode are inverses for all 16 combinations (the
        // divergence-is-a-bug guard for the pointer modifier wire).
        for bits in 0u8..16 {
            let m = Modifiers {
                shift: bits & 1 != 0,
                ctrl: bits & 2 != 0,
                alt: bits & 4 != 0,
                meta: bits & 8 != 0,
            };
            let token = m.as_wire_token();
            assert_eq!(Modifiers::from_wire_token(&token), Some(m), "round-trip {m:?}");
        }
        // Canonical order + empty-state contract.
        assert_eq!(Modifiers::empty().as_wire_token(), "");
        assert_eq!(
            Modifiers { shift: true, ctrl: true, alt: false, meta: false }.as_wire_token(),
            "sc",
        );
        // Decode is order-tolerant; a non-scam letter rejects the whole token.
        assert_eq!(
            Modifiers::from_wire_token("cs"),
            Some(Modifiers { shift: true, ctrl: true, alt: false, meta: false }),
        );
        assert_eq!(Modifiers::from_wire_token("sx"), None, "unknown letter rejects");
        assert_eq!(Modifiers::from_wire_token(""), Some(Modifiers::empty()));
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

#[cfg(test)]
mod r56_2_a_composition_event_tests {
    //! R56.2.a §5.13 §5.38 — [`CompositionEvent`] enum surface tests.
    //! Pins the four W3C-mirrored variants + `Debug` + `PartialEq` +
    //! `Clone` derives so downstream pattern-matching call sites
    //! (`WidgetCore::apply_composition` dispatch in widget bindings,
    //! pinion-shell `WindowEvent::Ime` arm) stay stable.

    use super::CompositionEvent;
    use super::PointerWireEvent;

    #[test]
    fn r56_2_a_four_variants_construct_and_compare() {
        assert_eq!(CompositionEvent::Start, CompositionEvent::Start);
        assert_eq!(
            CompositionEvent::Update("ha".to_owned()),
            CompositionEvent::Update("ha".to_owned()),
        );
        assert_eq!(
            CompositionEvent::Commit("han".to_owned()),
            CompositionEvent::Commit("han".to_owned()),
        );
        assert_eq!(CompositionEvent::Cancel, CompositionEvent::Cancel);
    }

    #[test]
    fn r56_2_a_variants_are_distinct() {
        assert_ne!(CompositionEvent::Start, CompositionEvent::Cancel);
        assert_ne!(
            CompositionEvent::Update("ha".to_owned()),
            CompositionEvent::Commit("ha".to_owned()),
        );
        assert_ne!(
            CompositionEvent::Update("ha".to_owned()),
            CompositionEvent::Update("han".to_owned()),
        );
    }

    #[test]
    fn r56_2_a_clone_round_trip_preserves_data() {
        let original = CompositionEvent::Commit("\u{D55C}".to_owned()); // Korean syllable "한"
        let cloned = original.clone();
        assert_eq!(original, cloned);
        if let CompositionEvent::Commit(text) = cloned {
            assert_eq!(text.len(), 3, "Korean syllable is 3 UTF-8 bytes");
        } else {
            panic!("Clone must preserve variant tag");
        }
    }

    #[test]
    fn r56_2_a_empty_update_is_distinct_from_cancel() {
        // Empty Update is a "preedit cleared but composition still open"
        // signal — distinct from explicit Cancel which ends the session.
        // The substrate routes them through different `apply_composition_*`
        // methods on the External (Update("") → preedit_update("") vs
        // Cancel → preedit_cancel + SCXML CancelEdit).
        assert_ne!(
            CompositionEvent::Update(String::new()),
            CompositionEvent::Cancel,
        );
    }

    #[test]
    fn r56_2_a_four_known_variants_pattern_match() {
        // The `#[non_exhaustive]` attribute matters at the crate
        // boundary (external crates must include a wildcard arm);
        // inside `pinion-core` the four-variant match stays exhaustive.
        // This test pins the in-crate matchable surface so a future
        // variant addition is caught here at compile time and the
        // author updates the per-arm dispatch (and adds the
        // downstream wildcard-arm regression in the relevant
        // consumer crate).
        let events = [
            CompositionEvent::Start,
            CompositionEvent::Update("x".to_owned()),
            CompositionEvent::Commit("x".to_owned()),
            CompositionEvent::Cancel,
        ];
        for e in &events {
            let label = match e {
                CompositionEvent::Start => "start",
                CompositionEvent::Update(_) => "update",
                CompositionEvent::Commit(_) => "commit",
                CompositionEvent::Cancel => "cancel",
            };
            assert!(!label.is_empty());
        }
    }

    const ALL_POINTER_WIRE_EVENTS: [PointerWireEvent; 5] = [
        PointerWireEvent::Enter,
        PointerWireEvent::Down,
        PointerWireEvent::Up,
        PointerWireEvent::Leave,
        PointerWireEvent::Cancel,
    ];

    #[test]
    fn r773_pointer_wire_event_encode_decode_round_trips() {
        // decode(encode(e)) == e for every variant — the SSOT pairing
        // guard: a name added to one direction but not the other fails
        // here at compile/test time.
        for e in ALL_POINTER_WIRE_EVENTS {
            assert_eq!(PointerWireEvent::from_wire_name(e.as_wire_name()), Some(e));
        }
    }

    #[test]
    fn r773_pointer_wire_event_names_are_canonical() {
        assert_eq!(PointerWireEvent::Enter.as_wire_name(), "PointerEnter");
        assert_eq!(PointerWireEvent::Down.as_wire_name(), "PointerDown");
        assert_eq!(PointerWireEvent::Up.as_wire_name(), "PointerUp");
        assert_eq!(PointerWireEvent::Leave.as_wire_name(), "PointerLeave");
        assert_eq!(PointerWireEvent::Cancel.as_wire_name(), "PointerCancel");
    }

    #[test]
    fn r773_pointer_wire_event_rejects_unknown_names() {
        // Names outside the pointer vocabulary (a different wire
        // vocabulary, or a typo) decode to None so callers can fall
        // through to their own handling or reject the payload.
        assert_eq!(PointerWireEvent::from_wire_name("PointerWheel"), None);
        assert_eq!(PointerWireEvent::from_wire_name("PointerMove"), None);
        assert_eq!(PointerWireEvent::from_wire_name("KeyboardActivate"), None);
        assert_eq!(PointerWireEvent::from_wire_name("DoubleClick"), None);
        assert_eq!(PointerWireEvent::from_wire_name(""), None);
    }
}
