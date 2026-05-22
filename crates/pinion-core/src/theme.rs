//! R57.0 §5.50 — Theming substrate. R57.1 §5.50 — `ThemeMode` +
//! `prefers-color-scheme` OS bridge.
//!
//! Provides a typed [`ColorRole`] enum (Material 3 / W3C CSS variable
//! mirror), a [`Theme`] palette, a [`ThemeMode`] enum
//! (`Light` / `Dark` / `System`), a global [`SystemColorScheme`]
//! signal (W3C `matchMedia("(prefers-color-scheme: dark)")` mirror),
//! and a reactive [`ThemeProvider`] resolved through the canonical
//! [`use_theme`] hook. Application code and widgets emit semantic
//! roles ([`ColorRole::Surface`], [`ColorRole::OnSurface`], ...)
//! instead of embedding raw RGB literals; the same view-fn renders
//! correctly under both light and dark palettes — the provider holds
//! both palettes side-by-side and dispatches to one of them through
//! the active [`ThemeMode`] (and, when the mode is
//! [`ThemeMode::System`], the platform's
//! [`SystemColorScheme`] signal).
//!
//! ## Convention mirror
//!
//! - W3C CSS Custom Properties (`:root { --color-surface }`) — semantic
//!   role tokens, cascade-resolved.
//! - W3C `prefers-color-scheme` media query — process-global OS hint
//!   that [`ThemeMode::System`] tracks via [`system_color_scheme`].
//! - W3C `color-scheme` CSS property — the page-level mode declaration
//!   (`light`, `dark`, `light dark`); [`ThemeMode`] is the same shape.
//! - `SwiftUI` `Color.primary` / `Color.background` — declarative role
//!   shorthand resolved by the active `ColorScheme`.
//! - `SwiftUI` `Environment(\.colorScheme)` + `preferredColorScheme` —
//!   the same `Light` / `Dark` / `follow-system` triple [`ThemeMode`]
//!   exposes.
//! - Material 3 Color Roles — `primary` / `onPrimary` / `surface` /
//!   `onSurface` / `outline` / `onSurfaceVariant`, paired so that
//!   foreground-on-background contrast is guaranteed by construction.
//! - Material 3 "Follow system" theme toggle — the recommended app
//!   default ([`ThemeMode::System`]).
//! - Slint `Palette` / `FluentUI` Tokens — same structural shape.
//!
//! ## First-slice scope (R57.0 + R57.X.toggle extension + R57.1)
//!
//! Tier 1 color roles: [`ColorRole::Surface`], [`ColorRole::OnSurface`],
//! [`ColorRole::OnSurfaceMuted`], [`ColorRole::Accent`],
//! [`ColorRole::OnAccent`], [`ColorRole::Outline`], plus the four
//! Material 3 surface-elevation tiers ([`ColorRole::SurfaceContainerLow`]
//! ... [`ColorRole::SurfaceContainerHighest`]) the `hello-listbox`
//! retrofit (R57.X.listbox) surfaced. The role enum's
//! `#[non_exhaustive]` annotation keeps every future extension
//! `SemVer`-safe.
//!
//! Subsequent slices (R57.2+) layer in the remaining Material 3
//! container / variant pairs (`primaryContainer` /
//! `onPrimaryContainer`, the error role family), the typography token
//! surface (font-size / line-height roles), and the spacing token
//! surface — every extension lands behind the same `#[non_exhaustive]`
//! shape on [`ColorRole`].
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
//! - [`ThemeProvider::theme`] auto-subscribes the current view-fn to
//!   the active [`ThemeMode`] signal, the relevant
//!   light/dark palette signal, and (when the mode is
//!   [`ThemeMode::System`]) the global [`SystemColorScheme`] signal —
//!   any of those changing schedules a re-paint.
//! - [`Theme::resolve(role)`](Theme::resolve) maps a [`ColorRole`] to
//!   the bound [`Color`]. Every role is a required field on
//!   [`Theme`], so the resolution is total — no fallback path triggers
//!   under the current shape.
//!
//! ## OS bridge (R57.1)
//!
//! The platform backend ([`pinion-shell`](../../pinion_shell/index.html)
//! Vello binding) translates the OS `prefers-color-scheme` signal
//! (winit `WindowEvent::ThemeChanged` on desktop;
//! `window.theme()` at window-creation time) into a
//! [`SystemColorScheme`] value and calls
//! [`set_system_color_scheme`]. The setter writes the global
//! thread-local [`Signal<SystemColorScheme>`](Signal) so every
//! [`ThemeProvider`] in [`ThemeMode::System`] re-resolves
//! transparently; widgets see exactly one re-paint per OS theme flip.
//!
//! The signal is **process-thread-local**: pinion's UI thread is the
//! sole writer (the backend's `WindowEvent` arm runs there) and the
//! sole reader (view-fn invocation runs there). Tests run in their
//! own thread, so they start at the
//! [`SystemColorScheme::NoPreference`] default and explicit
//! [`set_system_color_scheme`] calls in test setup are isolated from
//! every other test on every other thread.
//!
//! ## Linear-space invariant
//!
//! Theme palette entries are sRGB-encoded [`Color`]s; the
//! [`Color::lerp`](crate::style::Color::lerp) (R51.151) path is the
//! canonical inter-palette fade (R57.X.theme-fade carry) and remains
//! linear-space, so theme-fade animations render perceptually correct
//! without theme-specific special-casing.
//!
//! [`use_text_edit_state`]: crate::widgets::text_edit::use_text_edit_state

use std::rc::Rc;

use crate::reactive::{Owner, Signal};
use crate::style::Color;

// ────────────────────────────────────────────────────────────────────
// SystemColorScheme — W3C `prefers-color-scheme` mirror
// ────────────────────────────────────────────────────────────────────

/// Process-thread-global OS color-scheme preference — the same shape
/// W3C's `matchMedia("(prefers-color-scheme: dark)")` reports
/// (`light` / `dark` / `no-preference`). [`ThemeMode::System`]
/// resolves through this signal so applications that opt into the
/// canonical Material 3 / `SwiftUI` "follow system" default swap
/// palettes the moment the user toggles their OS dark-mode setting
/// — no per-app polling, no per-widget plumbing.
///
/// Read via [`system_color_scheme`] (subscribe-aware inside a
/// view-fn), written via [`set_system_color_scheme`] (called by the
/// platform backend on `WindowEvent::ThemeChanged` and once at window
/// creation). The default is [`Self::NoPreference`], which resolves
/// the same as [`Self::Light`] per W3C — the page falls back to the
/// light palette when the OS reports no preference.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[non_exhaustive]
pub enum SystemColorScheme {
    /// W3C `prefers-color-scheme: no-preference`. The user has not
    /// expressed a preference; per the W3C spec, the page renders in
    /// its light baseline. The default for fresh threads (tests, the
    /// first frame before the platform backend has reported in).
    #[default]
    NoPreference,
    /// W3C `prefers-color-scheme: light`. The user (or platform
    /// default) has indicated a preference for light surfaces.
    Light,
    /// W3C `prefers-color-scheme: dark`. The user has indicated a
    /// preference for dark surfaces — the canonical "OS dark mode"
    /// signal on macOS / Linux / Windows.
    Dark,
}

thread_local! {
    /// R57.1 §5.50 — global OS color-scheme signal. One per UI
    /// thread; written by the platform backend on
    /// `WindowEvent::ThemeChanged`, read by every
    /// [`ThemeProvider`] in [`ThemeMode::System`] from inside a
    /// view-fn (and so subscribed to by every such view).
    static SYSTEM_COLOR_SCHEME: Signal<SystemColorScheme> =
        Signal::new(SystemColorScheme::NoPreference);
}

/// Read the global OS [`SystemColorScheme`]. Auto-subscribes the
/// current view-fn (or other reactive scope) so the next
/// [`set_system_color_scheme`] from the platform backend schedules a
/// re-paint of every subscriber.
///
/// Returns [`SystemColorScheme::NoPreference`] on every fresh thread
/// (the default the [`Signal`] starts at). The platform backend
/// pushes the actual OS value into the signal at window creation
/// time; before that point, view-fns see the default and consequently
/// render their light palette (W3C fallback).
#[must_use]
pub fn system_color_scheme() -> SystemColorScheme {
    SYSTEM_COLOR_SCHEME.with(Signal::get)
}

/// Write the global OS [`SystemColorScheme`]. The platform backend
/// (pinion-shell on the Vello side) calls this once at window
/// creation (translating winit's `Window::theme()` readout) and again
/// on every `WindowEvent::ThemeChanged`. Equality-skips: a no-op
/// write (the OS event-dispatch path can emit the same value twice
/// after focus regain on some platforms) does not flag the signal
/// dirty and does not re-run subscribers.
///
/// Application code should not call this directly — it is the
/// platform's responsibility to translate the OS signal. The setter
/// is exposed `pub` rather than `pub(crate)` only because the
/// platform backend lives in a sibling crate (`pinion-shell`), which
/// the closed-core dependency direction (§6.3) keeps outside
/// `pinion-core`.
pub fn set_system_color_scheme(scheme: SystemColorScheme) {
    SYSTEM_COLOR_SCHEME.with(|s| s.set(scheme));
}

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
    /// Filled-but-inactive container surface — the chip / track / pill
    /// background a widget shows in its **Off** posture, before any
    /// activation. Distinct from [`Self::Outline`] (a stroke role) and
    /// from [`Self::Surface`] (the panel background): a filled control
    /// in its rest state sits visually *above* the surface and
    /// *behind* its activated [`Self::Accent`] counterpart. Material 3
    /// `surfaceContainerHighest` — the highest-elevation surface tone
    /// in the M3 tonal-elevation scale, used by `Switch` (Off track),
    /// `Chip` (unselected fill), `Slider` (inactive segment).
    SurfaceContainerHighest,
    /// Lowest-elevation filled container above [`Self::Surface`].
    /// Material 3 `surfaceContainerLow` — the default tone for list-row
    /// backgrounds, card surfaces resting near the panel, and any
    /// "sits slightly above the surface" affordance. Visually closer
    /// to [`Self::Surface`] than [`Self::SurfaceContainerHighest`].
    SurfaceContainerLow,
    /// Mid-elevation filled container — between
    /// [`Self::SurfaceContainerLow`] and [`Self::SurfaceContainerHigh`].
    /// Material 3 `surfaceContainer` — used for focused list rows,
    /// raised cards inside a list, dialog content surfaces.
    SurfaceContainer,
    /// High-elevation filled container — between
    /// [`Self::SurfaceContainer`] and [`Self::SurfaceContainerHighest`].
    /// Material 3 `surfaceContainerHigh` — used for hovered list rows,
    /// drawer panels, sheet headers.
    SurfaceContainerHigh,
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
            ColorRole::SurfaceContainerHighest => Color::rgb(0xe6, 0xe0, 0xe9),
            ColorRole::SurfaceContainerLow => Color::rgb(0xf7, 0xf2, 0xfa),
            ColorRole::SurfaceContainer => Color::rgb(0xf3, 0xed, 0xf7),
            ColorRole::SurfaceContainerHigh => Color::rgb(0xec, 0xe6, 0xf0),
        }
    }
}

// ────────────────────────────────────────────────────────────────────
// Theme — concrete palette mapping every ColorRole to a Color
// ────────────────────────────────────────────────────────────────────

/// A complete color palette — maps every [`ColorRole`] to a concrete
/// [`Color`]. Constructed via the [`Theme::light`] / [`Theme::dark`]
/// preset factories or via the per-field literal constructor;
/// applications hold both light and dark palettes on a
/// [`ThemeProvider`] and swap which is active through
/// [`ThemeProvider::set_mode`] rather than patching individual fields.
///
/// `Theme` is `Copy` (ten `Color` fields, each four `u8`s); the
/// runtime swap path uses [`Signal<Theme>`](Signal) so a per-palette
/// re-tone collapses into one reactive notification — the textbook
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
    /// Resolves [`ColorRole::SurfaceContainerHighest`].
    pub surface_container_highest: Color,
    /// Resolves [`ColorRole::SurfaceContainerLow`].
    pub surface_container_low: Color,
    /// Resolves [`ColorRole::SurfaceContainer`].
    pub surface_container: Color,
    /// Resolves [`ColorRole::SurfaceContainerHigh`].
    pub surface_container_high: Color,
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
    /// - `surface_container_highest` = `#E6E0E9`, the Material 3 light
    ///   `surfaceContainerHighest` tone — 1.1:1 against `surface`, the
    ///   highest-elevation chip surface that stays visibly distinct
    ///   from the panel background without competing with `accent`.
    /// - `surface_container_low` = `#F7F2FA`, the Material 3 light
    ///   `surfaceContainerLow` tone — the default raised-row surface,
    ///   sits just above `surface` on the M3 tonal-elevation scale.
    /// - `surface_container` = `#F3EDF7`, the Material 3 light
    ///   `surfaceContainer` tone — focused-row / mid-elevation tier.
    /// - `surface_container_high` = `#ECE6F0`, the Material 3 light
    ///   `surfaceContainerHigh` tone — hovered-row / drawer-panel tier.
    #[must_use]
    pub const fn light() -> Self {
        Self {
            surface: Color::rgb(0xff, 0xff, 0xff),
            on_surface: Color::rgb(0x1a, 0x1a, 0x1a),
            on_surface_muted: Color::rgb(0x60, 0x60, 0x60),
            accent: Color::rgb(0x19, 0x76, 0xd2),
            on_accent: Color::rgb(0xff, 0xff, 0xff),
            outline: Color::rgb(0xc0, 0xc0, 0xc0),
            surface_container_highest: Color::rgb(0xe6, 0xe0, 0xe9),
            surface_container_low: Color::rgb(0xf7, 0xf2, 0xfa),
            surface_container: Color::rgb(0xf3, 0xed, 0xf7),
            surface_container_high: Color::rgb(0xec, 0xe6, 0xf0),
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
    /// - `surface_container_highest` = `#36343B`, the Material 3 dark
    ///   `surfaceContainerHighest` tone — the highest-elevation chip
    ///   surface that stays visibly distinct from the `#121212` panel
    ///   surface, matching the M3 dark tonal-elevation scale.
    /// - `surface_container_low` = `#1D1B20`, the Material 3 dark
    ///   `surfaceContainerLow` tone — list-row default in dark mode.
    /// - `surface_container` = `#211F26`, the Material 3 dark
    ///   `surfaceContainer` tone — focused-row / mid-elevation tier.
    /// - `surface_container_high` = `#2B2930`, the Material 3 dark
    ///   `surfaceContainerHigh` tone — hovered-row / drawer-panel tier.
    #[must_use]
    pub const fn dark() -> Self {
        Self {
            surface: Color::rgb(0x12, 0x12, 0x12),
            on_surface: Color::rgb(0xec, 0xec, 0xec),
            on_surface_muted: Color::rgb(0x9e, 0x9e, 0x9e),
            accent: Color::rgb(0x60, 0xa5, 0xfa),
            on_accent: Color::rgb(0x0b, 0x1f, 0x3f),
            outline: Color::rgb(0x40, 0x40, 0x40),
            surface_container_highest: Color::rgb(0x36, 0x34, 0x3b),
            surface_container_low: Color::rgb(0x1d, 0x1b, 0x20),
            surface_container: Color::rgb(0x21, 0x1f, 0x26),
            surface_container_high: Color::rgb(0x2b, 0x29, 0x30),
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
            ColorRole::SurfaceContainerHighest => self.surface_container_highest,
            ColorRole::SurfaceContainerLow => self.surface_container_low,
            ColorRole::SurfaceContainer => self.surface_container,
            ColorRole::SurfaceContainerHigh => self.surface_container_high,
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
// ThemeMode — application choice of which palette is active
// ────────────────────────────────────────────────────────────────────

/// Application-level theme mode. Mirrors the W3C `color-scheme` CSS
/// property values (`light` / `dark` / `light dark`) and the
/// `SwiftUI` `preferredColorScheme` enum (`.light` / `.dark` /
/// `nil = follow system`).
///
/// The default is [`Self::System`] — the canonical Material 3 / iOS /
/// macOS app behavior of following the OS-level `prefers-color-scheme`
/// signal so the application visually matches the rest of the user's
/// desktop without any per-app setting.
///
/// `#[non_exhaustive]` lets future hint variants (`HighContrast`,
/// `Sepia`, ...) land in a `SemVer` minor — both real Material 3 /
/// `FluentUI` design systems already specify high-contrast accessibility
/// modes that mirror the same enum-extension shape.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[non_exhaustive]
pub enum ThemeMode {
    /// Force the light palette regardless of the OS signal. The
    /// application opts out of "follow system"; equivalent to the
    /// `SwiftUI` `preferredColorScheme(.light)` page-level pin.
    Light,
    /// Force the dark palette regardless of the OS signal. Equivalent
    /// to `SwiftUI` `preferredColorScheme(.dark)`.
    Dark,
    /// Follow the OS [`SystemColorScheme`] signal. The default — the
    /// recommended Material 3 / iOS / macOS application behavior.
    /// When the signal is [`SystemColorScheme::NoPreference`] the
    /// provider resolves to the light palette per the W3C
    /// `prefers-color-scheme` fallback convention.
    #[default]
    System,
}

// ────────────────────────────────────────────────────────────────────
// ThemeProvider — reactive wrapper, Owner::cache-resolved
// ────────────────────────────────────────────────────────────────────

/// Reactive owner of the application's theming state. Carries both a
/// light palette and a dark palette side-by-side (the W3C
/// `<meta name="color-scheme" content="light dark">` shape) and an
/// active [`ThemeMode`] choosing which is rendered. [`Self::theme`]
/// auto-subscribes the current view-fn to every signal the resolution
/// reads (mode + the active palette + the global
/// [`SystemColorScheme`] when the mode is [`ThemeMode::System`]) so a
/// swap on any of them schedules a re-paint.
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
    /// Active mode — read via [`Self::mode`] (auto-subscribes the
    /// caller), written via [`Self::set_mode`]. Default
    /// [`ThemeMode::System`].
    mode: Signal<ThemeMode>,
    /// Light-mode palette — applied when [`Self::mode`] is
    /// [`ThemeMode::Light`], or [`ThemeMode::System`] resolves to
    /// [`SystemColorScheme::NoPreference`] / [`SystemColorScheme::Light`].
    /// Defaults to [`Theme::light`]; applications customize via
    /// [`Self::set_light_palette`].
    light_palette: Signal<Theme>,
    /// Dark-mode palette — applied when [`Self::mode`] is
    /// [`ThemeMode::Dark`], or [`ThemeMode::System`] resolves to
    /// [`SystemColorScheme::Dark`]. Defaults to [`Theme::dark`];
    /// applications customize via [`Self::set_dark_palette`].
    dark_palette: Signal<Theme>,
    /// Symbolic identifier — the [`Owner::cache`] key that resolved
    /// this provider. Echoed back through [`Self::tag`] so consumers
    /// can re-derive the cache key without repeating the literal.
    tag: Option<&'static str>,
}

impl ThemeProvider {
    /// Construct a fresh provider with no recorded tag and the
    /// canonical defaults — mode [`ThemeMode::System`], light palette
    /// [`Theme::light`], dark palette [`Theme::dark`]. Used by tests
    /// and manual wiring; the canonical application path goes through
    /// [`use_theme`] (which calls [`Self::with_tag`] under the hood).
    #[must_use]
    pub fn new() -> Self {
        Self {
            mode: Signal::new(ThemeMode::default()),
            light_palette: Signal::new(Theme::light()),
            dark_palette: Signal::new(Theme::dark()),
            tag: None,
        }
    }

    /// Construct a provider with `tag` recorded as the symbolic
    /// identifier and the same defaults [`Self::new`] uses. Used as
    /// the [`Owner::cache`] factory by [`use_theme`] so the provider
    /// remembers its own tag without the caller repeating the
    /// literal.
    #[must_use]
    pub fn with_tag(tag: &'static str) -> Self {
        Self {
            mode: Signal::new(ThemeMode::default()),
            light_palette: Signal::new(Theme::light()),
            dark_palette: Signal::new(Theme::dark()),
            tag: Some(tag),
        }
    }

    /// Active mode. Triggers a [`Signal`] subscription when called
    /// inside a view-fn — the view re-runs on the next
    /// [`Self::set_mode`] that changes the mode (equality-skip shorts
    /// out no-op writes).
    #[must_use]
    pub fn mode(&self) -> ThemeMode {
        self.mode.get()
    }

    /// Replace the active mode atomically. Subscribers of
    /// [`Self::theme`] / [`Self::mode`] re-run on the next reactive
    /// tick.
    pub fn set_mode(&self, mode: ThemeMode) {
        self.mode.set(mode);
    }

    /// Active light palette. Auto-subscribes the caller. The
    /// application customizes the light palette by setting it once at
    /// startup via [`Self::set_light_palette`] (brand override) — the
    /// provider then keeps both palettes side-by-side and dispatches
    /// through [`Self::mode`].
    #[must_use]
    pub fn light_palette(&self) -> Theme {
        self.light_palette.get()
    }

    /// Replace the light palette atomically. Triggers a re-paint of
    /// every subscriber when the mode is currently resolving to the
    /// light side (mode [`ThemeMode::Light`], or
    /// [`ThemeMode::System`] +
    /// [`SystemColorScheme::Light`] / [`SystemColorScheme::NoPreference`]).
    pub fn set_light_palette(&self, theme: Theme) {
        self.light_palette.set(theme);
    }

    /// Active dark palette. Mirror of [`Self::light_palette`] for the
    /// dark side.
    #[must_use]
    pub fn dark_palette(&self) -> Theme {
        self.dark_palette.get()
    }

    /// Replace the dark palette atomically. Mirror of
    /// [`Self::set_light_palette`] for the dark side.
    pub fn set_dark_palette(&self, theme: Theme) {
        self.dark_palette.set(theme);
    }

    /// Resolved active palette — dispatches through [`Self::mode`]:
    /// [`ThemeMode::Light`] returns [`Self::light_palette`],
    /// [`ThemeMode::Dark`] returns [`Self::dark_palette`], and
    /// [`ThemeMode::System`] consults the global
    /// [`system_color_scheme`] signal (so the caller auto-subscribes
    /// to every OS theme flip in addition to the local provider
    /// signals).
    ///
    /// The [`SystemColorScheme::NoPreference`] OS value resolves to
    /// the light palette per the W3C `prefers-color-scheme` fallback
    /// convention.
    #[must_use]
    pub fn theme(&self) -> Theme {
        match self.mode.get() {
            ThemeMode::Light => self.light_palette.get(),
            ThemeMode::Dark => self.dark_palette.get(),
            ThemeMode::System => match system_color_scheme() {
                SystemColorScheme::Dark => self.dark_palette.get(),
                SystemColorScheme::Light | SystemColorScheme::NoPreference => {
                    self.light_palette.get()
                }
            },
        }
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
    /// Defaults to [`ThemeProvider::new`] — mode [`ThemeMode::System`]
    /// + [`Theme::light`] light palette + [`Theme::dark`] dark palette.
    fn default() -> Self {
        Self::new()
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
/// First-call factory installs a fresh [`ThemeProvider`] — mode
/// [`ThemeMode::System`], light palette [`Theme::light`], dark palette
/// [`Theme::dark`]; subsequent calls reuse the cached `Rc<_>`.
/// Applications swap the active mode via [`ThemeProvider::set_mode`]
/// — the runtime [`Owner::cache`] slot holds the same `Rc` for the
/// lifetime of the owner; only the inner signals change.
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
        .cache(tag, || ThemeProvider::with_tag(tag))
}

// ────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    //! R57.0 + R57.1 §5.50 — Theme substrate regression battery.
    //! Covers: [`ColorRole`] exhaustive resolve, light/dark palette
    //! field-pinning, [`Default`] = light, [`ThemeMode`] +
    //! [`SystemColorScheme`] defaults, [`system_color_scheme`] /
    //! [`set_system_color_scheme`] mutation, [`ThemeProvider`] new
    //! defaults, [`ThemeProvider::set_mode`] +
    //! [`ThemeProvider::set_light_palette`] +
    //! [`ThemeProvider::set_dark_palette`] mutations,
    //! [`ThemeProvider::theme`] mode-driven resolution incl. System +
    //! global signal, [`use_theme`] hook caching (same tag → same
    //! [`Rc`], no double-init), [`use_theme`] outside [`Owner`] panics.

    use super::{
        ColorRole, SystemColorScheme, Theme, ThemeMode, ThemeProvider, set_system_color_scheme,
        system_color_scheme, use_theme,
    };
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
        assert_eq!(
            ColorRole::SurfaceContainerHighest.default_for(),
            light.surface_container_highest,
        );
        assert_eq!(
            ColorRole::SurfaceContainerLow.default_for(),
            light.surface_container_low,
        );
        assert_eq!(
            ColorRole::SurfaceContainer.default_for(),
            light.surface_container,
        );
        assert_eq!(
            ColorRole::SurfaceContainerHigh.default_for(),
            light.surface_container_high,
        );
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
        assert_eq!(
            t.surface_container_highest,
            Color::rgb(0xe6, 0xe0, 0xe9),
        );
        assert_eq!(t.surface_container_low, Color::rgb(0xf7, 0xf2, 0xfa));
        assert_eq!(t.surface_container, Color::rgb(0xf3, 0xed, 0xf7));
        assert_eq!(t.surface_container_high, Color::rgb(0xec, 0xe6, 0xf0));
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
        assert_eq!(
            t.surface_container_highest,
            Color::rgb(0x36, 0x34, 0x3b),
        );
        assert_eq!(t.surface_container_low, Color::rgb(0x1d, 0x1b, 0x20));
        assert_eq!(t.surface_container, Color::rgb(0x21, 0x1f, 0x26));
        assert_eq!(t.surface_container_high, Color::rgb(0x2b, 0x29, 0x30));
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
            assert_eq!(
                theme.resolve(ColorRole::SurfaceContainerHighest),
                theme.surface_container_highest,
            );
            assert_eq!(
                theme.resolve(ColorRole::SurfaceContainerLow),
                theme.surface_container_low,
            );
            assert_eq!(
                theme.resolve(ColorRole::SurfaceContainer),
                theme.surface_container,
            );
            assert_eq!(
                theme.resolve(ColorRole::SurfaceContainerHigh),
                theme.surface_container_high,
            );
        }
    }

    /// (R57.X.listbox §5.50) Material 3 light surface tier progression:
    /// `surface` is the lightest tone (panel background) and the four
    /// containers darken progressively toward
    /// `surface_container_highest`. Pinning this ordering protects
    /// against a palette tweak that would silently invert the
    /// elevation visual.
    #[test]
    fn r57_x_surface_tiers_light_lightness_progression() {
        let t = Theme::light();
        let lum = |c: Color| u32::from(c.r) + u32::from(c.g) + u32::from(c.b);
        assert!(lum(t.surface) >= lum(t.surface_container_low));
        assert!(lum(t.surface_container_low) >= lum(t.surface_container));
        assert!(lum(t.surface_container) >= lum(t.surface_container_high));
        assert!(lum(t.surface_container_high) >= lum(t.surface_container_highest));
    }

    /// (R57.X.listbox §5.50) Material 3 dark surface tier progression:
    /// `surface` is the darkest tone (panel background) and the four
    /// containers lighten progressively toward
    /// `surface_container_highest` (inverse of light).
    #[test]
    fn r57_x_surface_tiers_dark_lightness_progression() {
        let t = Theme::dark();
        let lum = |c: Color| u32::from(c.r) + u32::from(c.g) + u32::from(c.b);
        assert!(lum(t.surface) <= lum(t.surface_container_low));
        assert!(lum(t.surface_container_low) <= lum(t.surface_container));
        assert!(lum(t.surface_container) <= lum(t.surface_container_high));
        assert!(lum(t.surface_container_high) <= lum(t.surface_container_highest));
    }

    #[test]
    fn r57_0_theme_default_equals_light() {
        // Default trait routes to Theme::light per the W3C
        // "no preference" convention. Important for downstream
        // tests that construct Theme without specifying a palette.
        assert_eq!(Theme::default(), Theme::light());
    }

    // ─────────────────────────────────────────────────────────────
    // R57.1 — SystemColorScheme + ThemeMode defaults
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r57_1_system_color_scheme_default_is_no_preference() {
        // W3C `prefers-color-scheme: no-preference` is the spec
        // default — pin it so a future enum reshuffle does not
        // silently flip the application's first-frame appearance.
        assert_eq!(SystemColorScheme::default(), SystemColorScheme::NoPreference);
    }

    #[test]
    fn r57_1_theme_mode_default_is_system() {
        // Material 3 / iOS / macOS canonical default — follow the OS
        // signal. Pinning the Default impl protects against a future
        // refactor that might quietly hardcode Light or Dark.
        assert_eq!(ThemeMode::default(), ThemeMode::System);
    }

    #[test]
    fn r57_1_set_system_color_scheme_round_trips() {
        // The global setter must round-trip: write Dark, read Dark;
        // write Light, read Light; write NoPreference, read
        // NoPreference. Pinned to guard the thread-local Signal
        // wiring at the bottom of [[r57-1-prefers-color-scheme-os-bridge]].
        set_system_color_scheme(SystemColorScheme::Dark);
        assert_eq!(system_color_scheme(), SystemColorScheme::Dark);
        set_system_color_scheme(SystemColorScheme::Light);
        assert_eq!(system_color_scheme(), SystemColorScheme::Light);
        set_system_color_scheme(SystemColorScheme::NoPreference);
        assert_eq!(system_color_scheme(), SystemColorScheme::NoPreference);
    }

    // ─────────────────────────────────────────────────────────────
    // R57.1 — ThemeProvider new defaults + mode/palette mutators
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r57_1_provider_new_uses_default_mode_and_baseline_palettes() {
        // Fresh provider starts at the textbook defaults — mode
        // System, light = Theme::light(), dark = Theme::dark(). No
        // tag recorded for the bare new() path.
        let p = ThemeProvider::new();
        assert_eq!(p.mode(), ThemeMode::System);
        assert_eq!(p.light_palette(), Theme::light());
        assert_eq!(p.dark_palette(), Theme::dark());
        assert_eq!(p.tag(), None);
    }

    #[test]
    fn r57_1_provider_with_tag_records_tag_alongside_defaults() {
        let p = ThemeProvider::with_tag("app");
        assert_eq!(p.mode(), ThemeMode::System);
        assert_eq!(p.light_palette(), Theme::light());
        assert_eq!(p.dark_palette(), Theme::dark());
        assert_eq!(p.tag(), Some("app"));
    }

    #[test]
    fn r57_1_provider_set_mode_round_trips() {
        let p = ThemeProvider::new();
        p.set_mode(ThemeMode::Light);
        assert_eq!(p.mode(), ThemeMode::Light);
        p.set_mode(ThemeMode::Dark);
        assert_eq!(p.mode(), ThemeMode::Dark);
        p.set_mode(ThemeMode::System);
        assert_eq!(p.mode(), ThemeMode::System);
    }

    #[test]
    fn r57_1_provider_set_palettes_round_trip_independently() {
        // Setting the light palette does not touch the dark palette,
        // and vice-versa — pinned because both signals share the
        // `Signal<Theme>` shape and a refactor that consolidated
        // them could silently break this independence.
        let p = ThemeProvider::new();
        let custom_light = Theme {
            accent: Color::rgb(0x00, 0xff, 0x00),
            ..Theme::light()
        };
        p.set_light_palette(custom_light);
        assert_eq!(p.light_palette(), custom_light);
        assert_eq!(p.dark_palette(), Theme::dark());

        let custom_dark = Theme {
            accent: Color::rgb(0xff, 0x00, 0x00),
            ..Theme::dark()
        };
        p.set_dark_palette(custom_dark);
        assert_eq!(p.dark_palette(), custom_dark);
        assert_eq!(p.light_palette(), custom_light);
    }

    // ─────────────────────────────────────────────────────────────
    // R57.1 — ThemeProvider::theme mode-driven resolution
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r57_1_theme_mode_light_resolves_to_light_palette() {
        // Force System=Dark first to prove `Light` overrides the OS
        // signal — that is the spec contract for the
        // `preferredColorScheme(.light)` override case.
        set_system_color_scheme(SystemColorScheme::Dark);
        let p = ThemeProvider::new();
        p.set_mode(ThemeMode::Light);
        assert_eq!(p.theme(), Theme::light());
        // Restore baseline for the next test on this thread.
        set_system_color_scheme(SystemColorScheme::NoPreference);
    }

    #[test]
    fn r57_1_theme_mode_dark_resolves_to_dark_palette() {
        set_system_color_scheme(SystemColorScheme::Light);
        let p = ThemeProvider::new();
        p.set_mode(ThemeMode::Dark);
        assert_eq!(p.theme(), Theme::dark());
        set_system_color_scheme(SystemColorScheme::NoPreference);
    }

    #[test]
    fn r57_1_theme_mode_system_light_resolves_to_light_palette() {
        set_system_color_scheme(SystemColorScheme::Light);
        let p = ThemeProvider::new();
        // System mode is the default — no explicit set_mode needed.
        assert_eq!(p.theme(), Theme::light());
        set_system_color_scheme(SystemColorScheme::NoPreference);
    }

    #[test]
    fn r57_1_theme_mode_system_dark_resolves_to_dark_palette() {
        set_system_color_scheme(SystemColorScheme::Dark);
        let p = ThemeProvider::new();
        assert_eq!(p.theme(), Theme::dark());
        set_system_color_scheme(SystemColorScheme::NoPreference);
    }

    #[test]
    fn r57_1_theme_mode_system_no_preference_falls_back_to_light() {
        // W3C `prefers-color-scheme: no-preference` falls back to the
        // light palette — the spec contract pinned here.
        set_system_color_scheme(SystemColorScheme::NoPreference);
        let p = ThemeProvider::new();
        assert_eq!(p.theme(), Theme::light());
    }

    #[test]
    fn r57_1_provider_resolve_routes_through_active_palette() {
        // resolve() must dispatch to the active palette — under
        // System+Dark, ColorRole::Accent resolves to Dark accent.
        set_system_color_scheme(SystemColorScheme::Dark);
        let p = ThemeProvider::new();
        assert_eq!(p.resolve(ColorRole::Accent), Theme::dark().accent);
        p.set_mode(ThemeMode::Light);
        assert_eq!(p.resolve(ColorRole::Accent), Theme::light().accent);
        set_system_color_scheme(SystemColorScheme::NoPreference);
    }

    #[test]
    fn r57_1_provider_default_uses_system_mode_and_baseline_palettes() {
        let p = ThemeProvider::default();
        assert_eq!(p.mode(), ThemeMode::System);
        assert_eq!(p.light_palette(), Theme::light());
        assert_eq!(p.dark_palette(), Theme::dark());
        assert_eq!(p.tag(), None);
    }

    // ─────────────────────────────────────────────────────────────
    // use_theme — Owner::cache typed-key hook semantics
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r57_0_use_theme_returns_provider_with_default_mode_and_palettes() {
        let owner = Owner::new();
        owner.run(|| {
            let p = use_theme("app");
            assert_eq!(p.mode(), ThemeMode::System);
            assert_eq!(p.light_palette(), Theme::light());
            assert_eq!(p.dark_palette(), Theme::dark());
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
        // independent providers — a mode flip on one MUST NOT
        // propagate to the other. Mirrors React's `useContext`
        // scoping.
        let owner = Owner::new();
        owner.run(|| {
            let app = use_theme("app");
            let modal = use_theme("modal");
            assert!(!Rc::ptr_eq(&app, &modal));
            app.set_mode(ThemeMode::Dark);
            assert_eq!(app.mode(), ThemeMode::Dark);
            assert_eq!(modal.mode(), ThemeMode::System);
        });
    }

    #[test]
    fn r57_0_use_theme_mode_persists_across_view_runs() {
        // The provider lives in Owner::cache, so a mode flip inside
        // one view-run must survive into the next view-run on the
        // same owner. Mirrors the cross-paint persistence contract
        // that ScrollState + CaretBlink rely on.
        let owner = Owner::new();
        owner.run(|| {
            let p = use_theme("app");
            p.set_mode(ThemeMode::Dark);
        });
        owner.run(|| {
            let p = use_theme("app");
            assert_eq!(p.mode(), ThemeMode::Dark);
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
