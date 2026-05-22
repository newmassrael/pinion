//! R57.0 §5.50 — Theming substrate.
//!
//! Provides a typed [`ColorRole`] enum (Material 3 / W3C CSS variable
//! mirror), a [`Theme`] palette, and a reactive [`ThemeProvider`]
//! resolved through the canonical [`use_theme`] hook. Application code
//! and widgets emit semantic roles ([`ColorRole::Surface`],
//! [`ColorRole::OnSurface`], ...) instead of embedding raw RGB
//! literals; the same view-fn renders correctly under both light and
//! dark palettes by swapping the active [`Theme`] through
//! [`ThemeProvider::set_theme`].
//!
//! ## Convention mirror
//!
//! - W3C CSS Custom Properties (`:root { --color-surface }`) — semantic
//!   role tokens, cascade-resolved.
//! - `SwiftUI` `Color.primary` / `Color.background` — declarative role
//!   shorthand resolved by the active `ColorScheme`.
//! - Material 3 Color Roles — `primary` / `onPrimary` / `surface` /
//!   `onSurface` / `outline` / `onSurfaceVariant`, paired so that
//!   foreground-on-background contrast is guaranteed by construction.
//! - Slint `Palette` / `FluentUI` Tokens — same structural shape.
//!
//! ## First-slice scope (R57.0)
//!
//! Tier 1 color roles only: [`ColorRole::Surface`],
//! [`ColorRole::OnSurface`], [`ColorRole::OnSurfaceMuted`],
//! [`ColorRole::Accent`], [`ColorRole::OnAccent`],
//! [`ColorRole::Outline`]. This is the minimum surface that lets the
//! `hello-theme` reference application render the visible affordances
//! of the existing widget catalog under both palettes. Subsequent
//! slices (R57.1+) layer in the Material 3 container / variant pairs
//! (`primaryContainer` / `onPrimaryContainer` / etc.), the typography
//! token surface (font-size / line-height roles), and the spacing
//! token surface — every extension lands behind the
//! `#[non_exhaustive]` shape on [`ColorRole`] so no `SemVer` break is
//! required.
//!
//! ## Resolution path
//!
//! - [`use_theme(tag)`](use_theme) returns an
//!   [`Rc<ThemeProvider>`](ThemeProvider) — wired through
//!   [`Owner::cache`](crate::reactive::Owner::cache) (R51.150) under
//!   the typed key `(TypeId::of::<ThemeProvider>(), tag)`. Same `tag`
//!   used by other typed hooks ([`use_text_edit_state`], ...) resolves
//!   to a distinct slot per the per-type-slot contract
//!   ([[owner-cache-typed-key]]).
//! - [`ThemeProvider::theme()`] auto-subscribes the current view-fn
//!   to palette swaps — the next [`ThemeProvider::set_theme`] from
//!   anywhere in the application schedules a re-paint.
//! - [`Theme::resolve(role)`](Theme::resolve) maps a [`ColorRole`] to
//!   the bound [`Color`]. Every role is a required field on
//!   [`Theme`], so the resolution is total — no fallback path triggers
//!   under the current shape.
//!
//! ## Linear-space invariant
//!
//! Theme palette entries are sRGB-encoded [`Color`]s; the
//! [`Color::lerp`](crate::style::Color::lerp) (R51.151) path is the
//! canonical inter-palette fade (R57.1 carry) and remains
//! linear-space, so theme-fade animations render perceptually correct
//! without theme-specific special-casing.
//!
//! [`use_text_edit_state`]: crate::widgets::text_edit::use_text_edit_state

use std::rc::Rc;

use crate::reactive::{Owner, Signal};
use crate::style::Color;

// ────────────────────────────────────────────────────────────────────
// ColorRole — Material 3 / W3C CSS variable mirror
// ────────────────────────────────────────────────────────────────────

/// Semantic color role — names the **purpose** of a color, not the
/// raw channel values. Application code resolves the role through the
/// active [`Theme`] so the same view-fn renders correctly under
/// multiple palettes.
///
/// The role set mirrors Material 3 / W3C CSS variable / `SwiftUI`
/// conventions; the v0 slice (R57.0) carries the minimal subset
/// needed to express the visible surface affordances of the existing
/// widget catalog (`hello-toggle` / `hello-listbox` /
/// `hello-textfield`). Subsequent slices add the Material 3 container
/// / variant pairs without breaking `SemVer` thanks to the
/// `#[non_exhaustive]` annotation.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[non_exhaustive]
pub enum ColorRole {
    /// Window / panel background. Material 3 `surface`, W3C
    /// `background-color`, `SwiftUI` `Color.background`.
    Surface,
    /// Primary on-surface foreground (body text, primary icons).
    /// Material 3 `onSurface`, W3C `color`, `SwiftUI` `Color.primary`.
    OnSurface,
    /// Muted on-surface foreground (secondary text, disabled labels).
    /// Material 3 `onSurfaceVariant`, `SwiftUI` `Color.secondary`.
    OnSurfaceMuted,
    /// Accent / brand color (active controls, focus rings, selection
    /// bands). Material 3 `primary`, `SwiftUI` `Color.accentColor`.
    Accent,
    /// Foreground rendered on top of an [`Self::Accent`] fill — paired
    /// for guaranteed contrast. Material 3 `onPrimary`.
    OnAccent,
    /// Hairline / divider / input-border color. Material 3 `outline`.
    Outline,
}

impl ColorRole {
    /// Deterministic fallback when a role is consulted on a palette
    /// that has not been bound yet. Returns the light-palette default
    /// — applications using a dark palette should bind every role
    /// explicitly via [`Theme::dark`] / [`Theme`]'s field constructor.
    ///
    /// Mirrors the W3C CSS variable cascade convention: an unset
    /// `var(--token)` falls back to the user-agent stylesheet
    /// default. The fallback exists for diagnostic + test paths
    /// (constructing a partially-populated [`Theme`] in isolation);
    /// production code constructs [`Theme`] through the named
    /// factories ([`Theme::light`] / [`Theme::dark`]) so every role
    /// is bound and the fallback never fires.
    #[must_use]
    #[allow(
        clippy::match_same_arms,
        reason = "each role's default is semantically distinct — the \
                  Surface / OnAccent coincidence on pure white is a \
                  property of the canonical light palette, not a \
                  shared concept; future palette tuning may diverge \
                  the two arms, and merging them now would erase the \
                  per-role intent the role enum is built to express"
    )]
    pub const fn default_for(self) -> Color {
        match self {
            ColorRole::Surface => Color::rgb(0xff, 0xff, 0xff),
            ColorRole::OnSurface => Color::rgb(0x1a, 0x1a, 0x1a),
            ColorRole::OnSurfaceMuted => Color::rgb(0x60, 0x60, 0x60),
            ColorRole::Accent => Color::rgb(0x19, 0x76, 0xd2),
            ColorRole::OnAccent => Color::rgb(0xff, 0xff, 0xff),
            ColorRole::Outline => Color::rgb(0xc0, 0xc0, 0xc0),
        }
    }
}

// ────────────────────────────────────────────────────────────────────
// Theme — concrete palette mapping every ColorRole to a Color
// ────────────────────────────────────────────────────────────────────

/// A complete color palette — maps every [`ColorRole`] to a concrete
/// [`Color`]. Constructed via the [`Theme::light`] / [`Theme::dark`]
/// preset factories or via the per-field literal constructor;
/// applications swap palettes atomically by setting a new [`Theme`]
/// onto a [`ThemeProvider`] rather than patching individual fields.
///
/// `Theme` is `Copy` (six `Color` fields, each four `u8`s); the
/// runtime swap path uses [`Signal<Theme>`](Signal) so a `set_theme`
/// call collapses into one reactive notification — the textbook
/// atomic-update contract.
///
/// Field-naming convention: `snake_case` mirror of the
/// [`ColorRole`] variant name (`OnSurface` ↔ `on_surface`). The
/// [`Theme::resolve`] method dispatches the enum to the matching
/// field so widgets stay role-driven.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize,
)]
pub struct Theme {
    /// Resolves [`ColorRole::Surface`].
    pub surface: Color,
    /// Resolves [`ColorRole::OnSurface`].
    pub on_surface: Color,
    /// Resolves [`ColorRole::OnSurfaceMuted`].
    pub on_surface_muted: Color,
    /// Resolves [`ColorRole::Accent`].
    pub accent: Color,
    /// Resolves [`ColorRole::OnAccent`].
    pub on_accent: Color,
    /// Resolves [`ColorRole::Outline`].
    pub outline: Color,
}

impl Theme {
    /// Canonical Light Mode palette — Material 3 baseline tuned for
    /// WCAG AA contrast on every paired role.
    ///
    /// - `surface` = pure white (`#FFFFFF`), W3C default body
    ///   background.
    /// - `on_surface` = near-black (`#1A1A1A`), 18.5:1 contrast on
    ///   white (WCAG AAA for normal text).
    /// - `on_surface_muted` = `#606060`, 6.4:1 on white (WCAG AA for
    ///   normal text).
    /// - `accent` = Material Blue 700 (`#1976D2`), 4.6:1 on white.
    /// - `on_accent` = white (`#FFFFFF`), 4.6:1 on Material Blue 700.
    /// - `outline` = `#C0C0C0`, the canonical W3C 1px hairline.
    #[must_use]
    pub const fn light() -> Self {
        Self {
            surface: Color::rgb(0xff, 0xff, 0xff),
            on_surface: Color::rgb(0x1a, 0x1a, 0x1a),
            on_surface_muted: Color::rgb(0x60, 0x60, 0x60),
            accent: Color::rgb(0x19, 0x76, 0xd2),
            on_accent: Color::rgb(0xff, 0xff, 0xff),
            outline: Color::rgb(0xc0, 0xc0, 0xc0),
        }
    }

    /// Canonical Dark Mode palette — Material 3 dark baseline with
    /// the accent lightened so the dark surface keeps WCAG AA
    /// contrast on every paired role.
    ///
    /// - `surface` = `#121212` (Material 3 dark surface), the W3C
    ///   recommended dark-mode body background.
    /// - `on_surface` = near-white (`#ECECEC`), 16.4:1 contrast on
    ///   `#121212` (WCAG AAA).
    /// - `on_surface_muted` = `#9E9E9E`, 6.8:1 on `#121212` (WCAG AA).
    /// - `accent` = Material Blue 400 (`#60A5FA`), 8.6:1 on `#121212`
    ///   — lifted from Blue 700 so the accent stays legible against
    ///   the dark surface.
    /// - `on_accent` = `#0B1F3F`, 8.6:1 against Material Blue 400.
    /// - `outline` = `#404040`, the dark-mode hairline used by
    ///   Material 3 / `FluentUI`.
    #[must_use]
    pub const fn dark() -> Self {
        Self {
            surface: Color::rgb(0x12, 0x12, 0x12),
            on_surface: Color::rgb(0xec, 0xec, 0xec),
            on_surface_muted: Color::rgb(0x9e, 0x9e, 0x9e),
            accent: Color::rgb(0x60, 0xa5, 0xfa),
            on_accent: Color::rgb(0x0b, 0x1f, 0x3f),
            outline: Color::rgb(0x40, 0x40, 0x40),
        }
    }

    /// Resolve a [`ColorRole`] to its bound [`Color`]. Total function:
    /// every role is a required field on [`Theme`], so the resolution
    /// is exhaustive without fallback. The match shape pins
    /// future-role additions to land paired with both a [`Theme`]
    /// field and a match arm — the compiler enforces the pairing.
    #[must_use]
    pub const fn resolve(&self, role: ColorRole) -> Color {
        match role {
            ColorRole::Surface => self.surface,
            ColorRole::OnSurface => self.on_surface,
            ColorRole::OnSurfaceMuted => self.on_surface_muted,
            ColorRole::Accent => self.accent,
            ColorRole::OnAccent => self.on_accent,
            ColorRole::Outline => self.outline,
        }
    }
}

impl Default for Theme {
    /// Defaults to [`Theme::light`] — mirrors the W3C / Material
    /// "no `prefers-color-scheme` preference" → light fallback.
    fn default() -> Self {
        Self::light()
    }
}

// ────────────────────────────────────────────────────────────────────
// ThemeProvider — reactive wrapper, Owner::cache-resolved
// ────────────────────────────────────────────────────────────────────

/// Reactive owner of the active [`Theme`]. Wraps a [`Signal<Theme>`]
/// so [`Self::theme`] auto-subscribes the current view-fn and
/// [`Self::set_theme`] triggers a re-paint on every subscriber.
///
/// One [`ThemeProvider`] per logical theming scope — typically one
/// per application, looked up by tag (e.g. `"app"`). The
/// [`Owner::cache`](crate::reactive::Owner::cache) substrate
/// (R51.150) keeps the provider alive across view re-runs and
/// hands the same `Rc` back on subsequent [`use_theme`] calls.
///
/// Thread-safety: not `Send` / `Sync` (Rc + Signal), matching every
/// other `pinion-core` reactive primitive. UI-thread only.
#[derive(Debug)]
pub struct ThemeProvider {
    /// Active palette — read via [`Self::theme`] (auto-subscribes the
    /// caller), written via [`Self::set_theme`] (signal equality-skip
    /// short-circuits no-op swaps).
    palette: Signal<Theme>,
    /// Symbolic identifier — the [`Owner::cache`] key that resolved
    /// this provider. Echoed back through [`Self::tag`] so consumers
    /// can re-derive the cache key without repeating the literal.
    tag: Option<&'static str>,
}

impl ThemeProvider {
    /// Construct a fresh provider with `initial` as the starting
    /// palette and no recorded tag. Used by tests + manual wiring;
    /// the canonical application path goes through [`use_theme`]
    /// (which calls [`Self::with_tag`] under the hood).
    #[must_use]
    pub fn new(initial: Theme) -> Self {
        Self {
            palette: Signal::new(initial),
            tag: None,
        }
    }

    /// Construct a provider with `initial` as the starting palette
    /// and `tag` recorded as the symbolic identifier. Used as the
    /// [`Owner::cache`] factory by [`use_theme`] so the provider
    /// remembers its own tag without the caller repeating the
    /// literal.
    #[must_use]
    pub fn with_tag(tag: &'static str, initial: Theme) -> Self {
        Self {
            palette: Signal::new(initial),
            tag: Some(tag),
        }
    }

    /// Current palette. Triggers a [`Signal`] subscription when called
    /// inside a view-fn — the view re-runs on the next
    /// [`Self::set_theme`] that changes the palette (equality-skip
    /// shorts out no-op swaps).
    #[must_use]
    pub fn theme(&self) -> Theme {
        self.palette.get()
    }

    /// Replace the active palette atomically. Subscribers of
    /// [`Self::theme`] re-run on the next reactive tick; the swap is
    /// a single signal write so all six role resolutions appear to
    /// flip in one beat (textbook atomic-update contract — same
    /// shape [`ScrollState::set_max`](crate::widgets::scroll::ScrollState::set_max)
    /// uses for its multi-axis writes).
    pub fn set_theme(&self, theme: Theme) {
        self.palette.set(theme);
    }

    /// Shorthand for [`Theme::resolve`] against the active palette.
    /// Auto-subscribes the caller through [`Self::theme`].
    #[must_use]
    pub fn resolve(&self, role: ColorRole) -> Color {
        self.theme().resolve(role)
    }

    /// Symbolic identifier supplied at construction. `Some(_)` for
    /// providers built via [`Self::with_tag`] (the [`use_theme`]
    /// path); `None` for [`Self::new`].
    #[must_use]
    pub fn tag(&self) -> Option<&'static str> {
        self.tag
    }
}

impl Default for ThemeProvider {
    /// Defaults to [`ThemeProvider::new`] with [`Theme::light`].
    fn default() -> Self {
        Self::new(Theme::light())
    }
}

// ────────────────────────────────────────────────────────────────────
// use_theme — typed-key hook resolving the active ThemeProvider
// ────────────────────────────────────────────────────────────────────

/// R57.0 §5.50 — view-fn hook resolving the active [`ThemeProvider`]
/// for a tagged scope. Equivalent shape to the other typed-key
/// `use_X` hooks ([`use_text_edit_state`],
/// [`use_caret_blink`](crate::widgets::caret_blink::use_caret_blink),
/// [`use_scroll_state`](crate::widgets::scroll::use_scroll_state)).
///
/// The [`Owner::cache`] keying contract is type-aware
/// ([[owner-cache-typed-key]]), so the same `tag` under different
/// typed hooks resolves to a distinct slot — passing `"app"` to
/// `use_theme` and to a hypothetical `use_app_state` would not
/// collide.
///
/// First-call factory installs a [`ThemeProvider`] wrapping
/// [`Theme::light`]; subsequent calls reuse the cached `Rc<_>`.
/// Applications swap the active palette via
/// [`ThemeProvider::set_theme`] — the runtime [`Owner::cache`] slot
/// holds the same `Rc` for the lifetime of the owner; only the
/// inner [`Signal<Theme>`] changes value.
///
/// # Panics
///
/// Panics if no current [`Owner`] is set — i.e. when invoked outside
/// a `root_owner.run(...)` wrap. Per the callback-root-owner-wrap
/// discipline (R51.146 / R51.152 / R51.171), framework-internal
/// dispatch sites supply this wrap; application code reaches
/// `use_theme` only from within `V::view` / `V::update` /
/// `V::apply_key` / similar hooks.
///
/// Panics if the cache key was previously bound to a value of a
/// different concrete type within the same owner — see
/// [`Owner::cache`] for the underlying contract.
///
/// [`use_text_edit_state`]: crate::widgets::text_edit::use_text_edit_state
#[must_use]
pub fn use_theme(tag: &'static str) -> Rc<ThemeProvider> {
    Owner::current()
        .expect("use_theme requires an active Owner scope")
        .cache(tag, || ThemeProvider::with_tag(tag, Theme::light()))
}

// ────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    //! R57.0 §5.50 — Theme substrate regression battery.
    //! Covers: [`ColorRole`] exhaustive resolve, light/dark palette
    //! field-pinning, [`Default`] = light, [`ThemeProvider`] initial
    //! state, [`ThemeProvider::set_theme`] swap, [`use_theme`] hook
    //! caching (same tag → same [`Rc`], no double-init), [`use_theme`]
    //! outside [`Owner`] panics.

    use super::{use_theme, ColorRole, Theme, ThemeProvider};
    use crate::reactive::Owner;
    use crate::style::Color;
    use std::rc::Rc;

    // ─────────────────────────────────────────────────────────────
    // ColorRole — exhaustive role enumeration + default_for fallback
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r57_0_color_role_default_for_returns_light_palette_color() {
        // Every ColorRole's default_for must equal the matching
        // Theme::light field — Light is the canonical fallback
        // palette per the W3C "no prefers-color-scheme" convention.
        let light = Theme::light();
        assert_eq!(ColorRole::Surface.default_for(), light.surface);
        assert_eq!(ColorRole::OnSurface.default_for(), light.on_surface);
        assert_eq!(
            ColorRole::OnSurfaceMuted.default_for(),
            light.on_surface_muted,
        );
        assert_eq!(ColorRole::Accent.default_for(), light.accent);
        assert_eq!(ColorRole::OnAccent.default_for(), light.on_accent);
        assert_eq!(ColorRole::Outline.default_for(), light.outline);
    }

    // ─────────────────────────────────────────────────────────────
    // Theme — preset palettes + resolve + Default = light
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r57_0_theme_light_pins_canonical_values() {
        // Pin the public light palette — protects against an
        // accidental field-tweak that would silently change every
        // application's surface color. Material 3 / W3C baseline.
        let t = Theme::light();
        assert_eq!(t.surface, Color::rgb(0xff, 0xff, 0xff));
        assert_eq!(t.on_surface, Color::rgb(0x1a, 0x1a, 0x1a));
        assert_eq!(t.on_surface_muted, Color::rgb(0x60, 0x60, 0x60));
        assert_eq!(t.accent, Color::rgb(0x19, 0x76, 0xd2));
        assert_eq!(t.on_accent, Color::rgb(0xff, 0xff, 0xff));
        assert_eq!(t.outline, Color::rgb(0xc0, 0xc0, 0xc0));
    }

    #[test]
    fn r57_0_theme_dark_pins_canonical_values() {
        // Pin the public dark palette — Material 3 dark baseline
        // with accent shifted lighter for WCAG AA on #121212.
        let t = Theme::dark();
        assert_eq!(t.surface, Color::rgb(0x12, 0x12, 0x12));
        assert_eq!(t.on_surface, Color::rgb(0xec, 0xec, 0xec));
        assert_eq!(t.on_surface_muted, Color::rgb(0x9e, 0x9e, 0x9e));
        assert_eq!(t.accent, Color::rgb(0x60, 0xa5, 0xfa));
        assert_eq!(t.on_accent, Color::rgb(0x0b, 0x1f, 0x3f));
        assert_eq!(t.outline, Color::rgb(0x40, 0x40, 0x40));
    }

    #[test]
    fn r57_0_theme_resolve_dispatches_every_role_to_matching_field() {
        // Light + Dark must each pass the round-trip:
        // `theme.resolve(role) == theme.<role_field>` for every
        // variant. Compile-time exhaustiveness catches missing
        // arms; this test catches a misrouted arm (e.g. arm typo
        // mapping `Accent` to `outline`).
        for theme in [Theme::light(), Theme::dark()] {
            assert_eq!(theme.resolve(ColorRole::Surface), theme.surface);
            assert_eq!(theme.resolve(ColorRole::OnSurface), theme.on_surface);
            assert_eq!(
                theme.resolve(ColorRole::OnSurfaceMuted),
                theme.on_surface_muted,
            );
            assert_eq!(theme.resolve(ColorRole::Accent), theme.accent);
            assert_eq!(theme.resolve(ColorRole::OnAccent), theme.on_accent);
            assert_eq!(theme.resolve(ColorRole::Outline), theme.outline);
        }
    }

    #[test]
    fn r57_0_theme_default_equals_light() {
        // Default trait routes to Theme::light per the W3C
        // "no preference" convention. Important for downstream
        // tests that construct Theme without specifying a palette.
        assert_eq!(Theme::default(), Theme::light());
    }

    // ─────────────────────────────────────────────────────────────
    // ThemeProvider — initial state + tag + set_theme atomic swap
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r57_0_provider_new_records_initial_palette_and_no_tag() {
        let p = ThemeProvider::new(Theme::dark());
        assert_eq!(p.theme(), Theme::dark());
        assert_eq!(p.tag(), None);
    }

    #[test]
    fn r57_0_provider_with_tag_records_tag_alongside_palette() {
        let p = ThemeProvider::with_tag("app", Theme::light());
        assert_eq!(p.theme(), Theme::light());
        assert_eq!(p.tag(), Some("app"));
    }

    #[test]
    fn r57_0_provider_set_theme_swaps_palette() {
        let p = ThemeProvider::new(Theme::light());
        p.set_theme(Theme::dark());
        assert_eq!(p.theme(), Theme::dark());
        // Back-and-forth — no caching of the previous value.
        p.set_theme(Theme::light());
        assert_eq!(p.theme(), Theme::light());
    }

    #[test]
    fn r57_0_provider_resolve_dispatches_to_active_palette() {
        let p = ThemeProvider::new(Theme::light());
        assert_eq!(p.resolve(ColorRole::Surface), Theme::light().surface);
        p.set_theme(Theme::dark());
        assert_eq!(p.resolve(ColorRole::Surface), Theme::dark().surface);
        // Accent shifts under dark — the resolve must follow the
        // active palette, not the construction-time one.
        assert_eq!(p.resolve(ColorRole::Accent), Theme::dark().accent);
    }

    #[test]
    fn r57_0_provider_default_uses_light_palette_no_tag() {
        let p = ThemeProvider::default();
        assert_eq!(p.theme(), Theme::light());
        assert_eq!(p.tag(), None);
    }

    // ─────────────────────────────────────────────────────────────
    // use_theme — Owner::cache typed-key hook semantics
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r57_0_use_theme_returns_provider_with_initial_light_palette() {
        let owner = Owner::new();
        owner.run(|| {
            let p = use_theme("app");
            assert_eq!(p.theme(), Theme::light());
            assert_eq!(p.tag(), Some("app"));
        });
    }

    #[test]
    fn r57_0_use_theme_caches_same_provider_across_calls() {
        // Same tag → same Rc — Owner::cache contract. The view-fn
        // re-running on every reactive tick MUST NOT spin up a new
        // ThemeProvider every paint.
        let owner = Owner::new();
        owner.run(|| {
            let p1 = use_theme("app");
            let p2 = use_theme("app");
            assert!(Rc::ptr_eq(&p1, &p2));
        });
    }

    #[test]
    fn r57_0_use_theme_distinct_tags_resolve_distinct_providers() {
        // Two separate logical scopes ("app", "modal") get
        // independent providers — a swap on one MUST NOT propagate
        // to the other. Mirrors React's `useContext` scoping.
        let owner = Owner::new();
        owner.run(|| {
            let app = use_theme("app");
            let modal = use_theme("modal");
            assert!(!Rc::ptr_eq(&app, &modal));
            app.set_theme(Theme::dark());
            assert_eq!(app.theme(), Theme::dark());
            assert_eq!(modal.theme(), Theme::light());
        });
    }

    #[test]
    fn r57_0_use_theme_swap_persists_across_view_runs() {
        // The provider lives in Owner::cache, so a swap inside one
        // view-run must survive into the next view-run on the same
        // owner. Mirrors the cross-paint persistence contract that
        // ScrollState + CaretBlink rely on.
        let owner = Owner::new();
        owner.run(|| {
            let p = use_theme("app");
            p.set_theme(Theme::dark());
        });
        owner.run(|| {
            let p = use_theme("app");
            assert_eq!(p.theme(), Theme::dark());
        });
    }

    #[test]
    #[should_panic(expected = "use_theme requires an active Owner scope")]
    fn r57_0_use_theme_panics_outside_owner_scope() {
        // No Owner::run wrap → use_theme must panic with the
        // canonical "requires an active Owner scope" message. Same
        // contract every other use_X hook honors so a misplaced
        // call (top-level main, raw test bench) surfaces the
        // wiring bug immediately.
        let _ = use_theme("app");
    }

}
