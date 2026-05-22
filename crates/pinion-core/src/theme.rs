//! R57.0 §5.50 — Theming substrate. R57.1 §5.50 — `ThemeMode` +
//! `prefers-color-scheme` OS bridge. R57.X.theme-fade §5.50 — palette
//! cross-fade via spring-driven linear-space interpolation.
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
//! canonical inter-palette fade and remains linear-space, so
//! theme-fade animations render perceptually correct without
//! theme-specific special-casing.
//!
//! ## Fade animation (R57.X.theme-fade)
//!
//! [`ThemeProvider::theme_animated`] is the opt-in animated mirror of
//! [`ThemeProvider::theme`]. It returns the **currently displayed**
//! palette, interpolated from the previous resolved palette toward
//! the active one via a critically-damped spring tuned to the
//! Material 3 "Standard" easing duration (~200 ms settle —
//! [`THEME_FADE_SPRING`]). The interpolation runs in linear-light
//! [`AnimVec4`] space via the private [`ThemeLinear`] carrier so the
//! perceptual quality matches [`Color::lerp`](crate::style::Color::lerp)
//! (R51.151) — no muddy-grey artifact on a light↔dark swap.
//!
//! The accessor uses the spring's velocity-preserving
//! [`Animation::set_target`](crate::animation::Animation::set_target)
//! interrupt semantic (`SwiftUI` / `Compose` canonical), so a mid-fade
//! mode flip transitions visually continuous from the in-flight value
//! to the new target — no discontinuity, no double-snap. At rest the
//! accessor short-circuits to the cached sRGB target so widget cascade
//! tests asserting against palette fields keep an exact-equality
//! contract: the linear-light round-trip is only on the wire during
//! the fade.
//!
//! Callers retain the instant [`ThemeProvider::theme`] accessor for
//! tests and snapshot reads. Opt-in keeps the substrate layered
//! ([[abstraction-needs-second-consumer]]); widget retrofit to
//! [`Self::theme_animated`] is a follow-up cascade with explicit
//! visible-affordance scope.
//!
//! [`use_text_edit_state`]: crate::widgets::text_edit::use_text_edit_state
//! [`AnimVec4`]: crate::animation::AnimVec4
//! [`Animation::set_target`]: crate::animation::Animation::set_target

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::animation::{AnimVec4, Animatable, Animation, SpringConfig};
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
// ThemeLinear — linear-light AnimVec4 carrier for the fade animation
// ────────────────────────────────────────────────────────────────────

/// Module-private linear-light mirror of [`Theme`]. Each field carries
/// the corresponding role's [`Color`] decoded through
/// [`Color::to_linear`](crate::style::Color::to_linear) into an
/// [`AnimVec4`]. The spring solver operates here so the integration
/// stays in colorimetrically-linear space; the result re-encodes back
/// to sRGB on every read via [`Self::to_theme`].
///
/// Mirrors the §5.28 substrate decision (`Color`/`Rect` Animatable
/// impls deferred per the animation module docstring) by carrying the
/// linear-space conversion as a Theme-specific bridge rather than
/// implementing [`Animatable`] for [`Color`] directly. The deferred
/// carry stays deferred — a future 2nd consumer evidences the
/// per-color generalization ([[abstraction-needs-second-consumer]]).
#[derive(
    Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize,
)]
struct ThemeLinear {
    surface: AnimVec4,
    on_surface: AnimVec4,
    on_surface_muted: AnimVec4,
    accent: AnimVec4,
    on_accent: AnimVec4,
    outline: AnimVec4,
    surface_container_highest: AnimVec4,
    surface_container_low: AnimVec4,
    surface_container: AnimVec4,
    surface_container_high: AnimVec4,
}

impl ThemeLinear {
    /// Lift an sRGB-encoded [`Theme`] into linear-light space per the
    /// IEC 61966-2-1 EOTF ([`Color::to_linear`](crate::style::Color::to_linear)).
    /// Pure function — `to_theme(from_theme(t))` is the canonical
    /// sRGB round-trip and reproduces every Theme field within
    /// 8-bit rounding.
    fn from_theme(t: Theme) -> Self {
        Self {
            surface: t.surface.to_linear(),
            on_surface: t.on_surface.to_linear(),
            on_surface_muted: t.on_surface_muted.to_linear(),
            accent: t.accent.to_linear(),
            on_accent: t.on_accent.to_linear(),
            outline: t.outline.to_linear(),
            surface_container_highest: t.surface_container_highest.to_linear(),
            surface_container_low: t.surface_container_low.to_linear(),
            surface_container: t.surface_container.to_linear(),
            surface_container_high: t.surface_container_high.to_linear(),
        }
    }

    /// Encode the linear-light state back to sRGB [`Theme`]. Each
    /// field clamps out-of-range linear components per
    /// [`Color::from_linear`](crate::style::Color::from_linear); the
    /// spring solver may transiently produce values slightly outside
    /// `[0.0, 1.0]` during overshoot, and the saturating encode
    /// keeps the rendered output valid sRGB without wrapping
    /// channels.
    fn to_theme(self) -> Theme {
        Theme {
            surface: Color::from_linear(self.surface),
            on_surface: Color::from_linear(self.on_surface),
            on_surface_muted: Color::from_linear(self.on_surface_muted),
            accent: Color::from_linear(self.accent),
            on_accent: Color::from_linear(self.on_accent),
            outline: Color::from_linear(self.outline),
            surface_container_highest: Color::from_linear(self.surface_container_highest),
            surface_container_low: Color::from_linear(self.surface_container_low),
            surface_container: Color::from_linear(self.surface_container),
            surface_container_high: Color::from_linear(self.surface_container_high),
        }
    }
}

impl Animatable for ThemeLinear {
    fn zero() -> Self {
        Self {
            surface: AnimVec4::zero(),
            on_surface: AnimVec4::zero(),
            on_surface_muted: AnimVec4::zero(),
            accent: AnimVec4::zero(),
            on_accent: AnimVec4::zero(),
            outline: AnimVec4::zero(),
            surface_container_highest: AnimVec4::zero(),
            surface_container_low: AnimVec4::zero(),
            surface_container: AnimVec4::zero(),
            surface_container_high: AnimVec4::zero(),
        }
    }

    fn add(self, other: Self) -> Self {
        Self {
            surface: self.surface.add(other.surface),
            on_surface: self.on_surface.add(other.on_surface),
            on_surface_muted: self.on_surface_muted.add(other.on_surface_muted),
            accent: self.accent.add(other.accent),
            on_accent: self.on_accent.add(other.on_accent),
            outline: self.outline.add(other.outline),
            surface_container_highest: self
                .surface_container_highest
                .add(other.surface_container_highest),
            surface_container_low: self
                .surface_container_low
                .add(other.surface_container_low),
            surface_container: self.surface_container.add(other.surface_container),
            surface_container_high: self
                .surface_container_high
                .add(other.surface_container_high),
        }
    }

    fn sub(self, other: Self) -> Self {
        Self {
            surface: self.surface.sub(other.surface),
            on_surface: self.on_surface.sub(other.on_surface),
            on_surface_muted: self.on_surface_muted.sub(other.on_surface_muted),
            accent: self.accent.sub(other.accent),
            on_accent: self.on_accent.sub(other.on_accent),
            outline: self.outline.sub(other.outline),
            surface_container_highest: self
                .surface_container_highest
                .sub(other.surface_container_highest),
            surface_container_low: self
                .surface_container_low
                .sub(other.surface_container_low),
            surface_container: self.surface_container.sub(other.surface_container),
            surface_container_high: self
                .surface_container_high
                .sub(other.surface_container_high),
        }
    }

    fn scale(self, factor: f32) -> Self {
        Self {
            surface: self.surface.scale(factor),
            on_surface: self.on_surface.scale(factor),
            on_surface_muted: self.on_surface_muted.scale(factor),
            accent: self.accent.scale(factor),
            on_accent: self.on_accent.scale(factor),
            outline: self.outline.scale(factor),
            surface_container_highest: self.surface_container_highest.scale(factor),
            surface_container_low: self.surface_container_low.scale(factor),
            surface_container: self.surface_container.scale(factor),
            surface_container_high: self.surface_container_high.scale(factor),
        }
    }

    fn approx_zero(self, epsilon: f32) -> bool {
        self.surface.approx_zero(epsilon)
            && self.on_surface.approx_zero(epsilon)
            && self.on_surface_muted.approx_zero(epsilon)
            && self.accent.approx_zero(epsilon)
            && self.on_accent.approx_zero(epsilon)
            && self.outline.approx_zero(epsilon)
            && self.surface_container_highest.approx_zero(epsilon)
            && self.surface_container_low.approx_zero(epsilon)
            && self.surface_container.approx_zero(epsilon)
            && self.surface_container_high.approx_zero(epsilon)
    }
}

// ────────────────────────────────────────────────────────────────────
// THEME_FADE_SPRING — M3 "Standard" easing approximation
// ────────────────────────────────────────────────────────────────────

/// Spring tuning for [`ThemeProvider::theme_animated`] — a
/// critically-damped spring approximating Material 3's "Standard"
/// easing token at the M3 short4 duration token (~200 ms).
///
/// ## Derivation
///
/// - `stiffness = 400`, `damping = 40`, `mass = 1` →
///   `ω_n = √(k/m) = 20 rad/s`, `ζ = c/(2·√(k·m)) = 1.0`
///   (critically damped — fastest settle without overshoot).
/// - Settling time (within 1 % of target) ≈ `4 / (ζ·ω_n) ≈ 200 ms`,
///   matching the Material 3 short4 motion-duration token used for
///   color-token transitions.
/// - Curve shape closely approximates the M3 "Standard" easing
///   `cubic-bezier(0.2, 0.0, 0, 1.0)` — both have a slow start and a
///   soft asymptotic approach (the standard / pinion-canon spring↔tween
///   substitution per the animation module docstring).
///
/// Exposed `pub` so downstream applications can tune their own
/// animations against the same M3-compliant preset — the [`Animation`]
/// substrate accepts any [`SpringConfig`], so the tuning is reusable
/// outside theme fade.
pub const THEME_FADE_SPRING: SpringConfig = SpringConfig::new(400.0, 40.0, 1.0);

// ────────────────────────────────────────────────────────────────────
// ThemeFadeState — lazy-initialised fade animation + target cache
// ────────────────────────────────────────────────────────────────────

/// Module-private fade state held inside [`ThemeProvider`]. Created on
/// the first [`ThemeProvider::theme_animated`] call that happens
/// inside an active [`Owner`] scope; the [`Animation`] field registers
/// for tick dispatch with that owner so the existing paint-loop
/// `tick_animations` driver (R51.142) drives the fade with no
/// theme-specific plumbing.
struct ThemeFadeState {
    /// Spring-driven linear-light palette. Re-targeted on every
    /// detected target swap; velocity carries through interrupts per
    /// the [`Animation::set_target`] contract so a mid-fade mode flip
    /// stays visually continuous.
    animation: Animation<ThemeLinear>,
    /// Most recent sRGB target observed by
    /// [`ThemeProvider::theme_animated`]. Compared against the next
    /// resolved target to detect a swap — comparing in sRGB
    /// (lossless `PartialEq` + `Eq` on [`Theme`]) avoids false-positives
    /// from the lossy linear round-trip.
    last_target: Cell<Theme>,
}

impl std::fmt::Debug for ThemeFadeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThemeFadeState")
            .field("animation_at_rest", &self.animation.is_at_rest())
            .field("last_target", &self.last_target.get())
            .finish()
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
    /// R57.X.theme-fade — lazily-initialised palette cross-fade
    /// animation. [`None`] until the first [`Self::theme_animated`]
    /// call inside an active [`Owner`] scope; thereafter holds the
    /// [`ThemeFadeState`] for the lifetime of the provider. Wrapped in
    /// [`RefCell`] for interior mutation — every access happens on the
    /// UI thread (the runtime is not `Send` / `Sync`), so a borrow
    /// conflict would already imply a re-entrant view-fn invocation
    /// which the substrate forbids elsewhere.
    fade: RefCell<Option<ThemeFadeState>>,
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
            fade: RefCell::new(None),
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
            fade: RefCell::new(None),
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

    /// R57.X.theme-fade §5.50 — animated counterpart of [`Self::theme`].
    /// Returns the **currently displayed** palette: a critically-damped
    /// spring ([`THEME_FADE_SPRING`]) interpolates from the previous
    /// resolved palette toward the active one in linear-light space,
    /// settling within ~200 ms (Material 3 short4 motion-duration token,
    /// "Standard" easing approximation).
    ///
    /// ## Reactive subscriptions
    ///
    /// Auto-subscribes the calling view-fn to every signal
    /// [`Self::theme`] subscribes to (mode + active palette + the
    /// global [`SystemColorScheme`] when mode is [`ThemeMode::System`])
    /// **plus** the spring's [`Animation::signal`] — so frame ticks
    /// during the fade re-run the view, and the view stops re-running
    /// the moment the spring settles (per the [`Signal::set`]
    /// equality-skip + `Animation` rest-epsilon contract).
    ///
    /// ## Interrupt semantics
    ///
    /// Mid-fade mode / palette / OS-scheme flips re-target the spring
    /// via [`Animation::set_target`](crate::animation::Animation::set_target);
    /// the existing velocity carries through so the displayed palette
    /// stays visually continuous across the interruption — the canonical
    /// `SwiftUI` `.animation(_)` / `Compose` `animateColorAsState`
    /// continuity contract.
    ///
    /// ## Lazy initialisation + Owner-less fallback
    ///
    /// The fade animation registers with the active [`Owner`] (via
    /// [`Owner::current`]) on the first call inside an
    /// [`Owner::run`](crate::reactive::Owner::run) scope. When called
    /// outside any active owner — typically diagnostic / snapshot reads
    /// from a test bench or RPC introspection path — the method falls
    /// back to the instant [`Self::theme`] value, no animation, no
    /// reactive subscription. This keeps the accessor safe to call
    /// anywhere without the `Owner::current().expect(...)` panic the
    /// callback-root-owner-wrap discipline guards against in
    /// [`use_theme`].
    ///
    /// ## At-rest exact snap
    ///
    /// Whenever the spring is settled
    /// ([`Animation::is_at_rest`](crate::animation::Animation::is_at_rest)),
    /// the accessor returns the cached sRGB [`Self::theme`] target
    /// directly rather than re-encoding the spring's linear-light
    /// state through [`ThemeLinear::to_theme`]. The
    /// [`Color::to_linear`](crate::style::Color::to_linear) /
    /// [`Color::from_linear`](crate::style::Color::from_linear)
    /// round-trip can drift midrange-channel values by ±1 8-bit unit
    /// (verified by `crate::style::tests::srgb_round_trip_midrange_close`),
    /// which would silently break widget cascade tests asserting
    /// exact equality against palette field values
    /// (e.g. [`Theme::dark`]`.surface = #121212`). Snapping at rest
    /// mirrors the canonical `SwiftUI` `.animation(_)` / `Compose`
    /// `animateColorAsState` contract: while the animation is running
    /// the returned value is the interpolated state; once settled
    /// the value equals the target exactly. The intermediate
    /// in-flight frames still go through the linear-light path so
    /// the perceptual interpolation quality is preserved.
    #[must_use]
    pub fn theme_animated(&self) -> Theme {
        let target = self.theme();
        let Some(owner) = Owner::current() else {
            // Outside any owner scope — return instant target so the
            // accessor remains safe to call from diagnostic paths.
            return target;
        };
        let target_linear = ThemeLinear::from_theme(target);
        let mut fade_borrow = self.fade.borrow_mut();
        let fade = fade_borrow.get_or_insert_with(|| ThemeFadeState {
            animation: Animation::new(&owner, target_linear, THEME_FADE_SPRING),
            last_target: Cell::new(target),
        });
        if fade.last_target.get() != target {
            fade.animation.set_target(target_linear);
            fade.last_target.set(target);
        }
        if fade.animation.is_at_rest() {
            // At rest — snap to the cached sRGB target rather than
            // round-trip the spring's linear-light state, which would
            // drift midrange-channel values by ±1 8-bit unit.
            target
        } else {
            // Mid-fade — re-encode the spring's interpolated linear
            // state to sRGB so the animation renders perceptually
            // correct. The animation signal subscription this read
            // establishes drives the per-frame view-fn re-run that
            // animates the fade.
            fade.animation.value().to_theme()
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

    // ─────────────────────────────────────────────────────────────
    // R57.X.theme-fade — Animation<ThemeLinear> spring substrate
    // ─────────────────────────────────────────────────────────────

    use super::{THEME_FADE_SPRING, ThemeLinear};

    /// Per-channel tolerance for the sRGB <-> linear round-trip plus
    /// the spring rest-epsilon expressed in 8-bit channel space.
    /// `Animation`'s default `DEFAULT_REST_EPSILON` is 0.01 in
    /// linear-light; the inverse-EOTF maps that to roughly one 8-bit
    /// channel step at the brightest tones.
    const ROUND_TRIP_TOLERANCE: i32 = 1;

    /// Component-wise close-equality on [`Color`] within
    /// [`ROUND_TRIP_TOLERANCE`]. Used by the fade tests so a sRGB
    /// round-trip through [`ThemeLinear`] does not flake the
    /// assertion at 8-bit precision.
    #[track_caller]
    fn assert_color_close(actual: Color, expected: Color) {
        let diff = |x: u8, y: u8| (i32::from(x) - i32::from(y)).abs();
        assert!(
            diff(actual.r, expected.r) <= ROUND_TRIP_TOLERANCE
                && diff(actual.g, expected.g) <= ROUND_TRIP_TOLERANCE
                && diff(actual.b, expected.b) <= ROUND_TRIP_TOLERANCE
                && diff(actual.a, expected.a) <= ROUND_TRIP_TOLERANCE,
            "expected {expected:?} +/- {ROUND_TRIP_TOLERANCE}, got {actual:?}"
        );
    }

    /// Component-wise close-equality on [`Theme`] across every field.
    #[track_caller]
    fn assert_theme_close(actual: Theme, expected: Theme) {
        assert_color_close(actual.surface, expected.surface);
        assert_color_close(actual.on_surface, expected.on_surface);
        assert_color_close(actual.on_surface_muted, expected.on_surface_muted);
        assert_color_close(actual.accent, expected.accent);
        assert_color_close(actual.on_accent, expected.on_accent);
        assert_color_close(actual.outline, expected.outline);
        assert_color_close(
            actual.surface_container_highest,
            expected.surface_container_highest,
        );
        assert_color_close(
            actual.surface_container_low,
            expected.surface_container_low,
        );
        assert_color_close(actual.surface_container, expected.surface_container);
        assert_color_close(
            actual.surface_container_high,
            expected.surface_container_high,
        );
    }

    #[test]
    fn r57_x_theme_fade_spring_is_critically_damped() {
        // ζ = damping / (2 * sqrt(stiffness * mass)). For 400 / 40 / 1:
        // 40 / (2 * sqrt(400)) = 40 / 40 = 1.0 exactly. Pin the
        // critical-damping property so a future tuning that drifts
        // the damping ratio (re-introducing overshoot) is caught at
        // test time. Also pin the M3 short4 ≈ 200 ms settling time
        // via the natural frequency.
        let c = THEME_FADE_SPRING;
        let zeta = c.damping / (2.0 * (c.stiffness * c.mass).sqrt());
        assert!(
            (zeta - 1.0).abs() < 1e-6,
            "expected critically damped, got zeta = {zeta}"
        );
        // omega_n = sqrt(k/m) = sqrt(400) = 20 rad/s. Settling time
        // (1 %) ≈ 4 / (zeta * omega_n) = 200 ms.
        let omega_n = (c.stiffness / c.mass).sqrt();
        assert!(
            (omega_n - 20.0).abs() < 1e-6,
            "expected omega_n = 20 rad/s, got {omega_n}"
        );
    }

    #[test]
    fn r57_x_theme_linear_round_trip_is_within_8bit_tolerance() {
        // ThemeLinear::from_theme followed by ::to_theme must
        // reproduce the original Theme within 8-bit rounding for
        // every field on both canonical presets — the round-trip is
        // the carrier between the spring solver's linear-space state
        // and the sRGB Theme surface widgets render.
        for theme in [Theme::light(), Theme::dark()] {
            let round_tripped = ThemeLinear::from_theme(theme).to_theme();
            assert_theme_close(round_tripped, theme);
        }
    }

    #[test]
    fn r57_x_theme_animated_outside_owner_returns_instant_target() {
        // Diagnostic / snapshot call outside any Owner::run scope
        // must fall back to the instant target with no animation and
        // no panic — the contract enables `theme_animated` to be
        // safely called from test benches and RPC introspection
        // paths that do not establish a root owner.
        set_system_color_scheme(SystemColorScheme::NoPreference);
        let p = ThemeProvider::new();
        p.set_mode(ThemeMode::Dark);
        // No Owner::current() → returns Theme::dark() exactly (no
        // round-trip), so an exact-equality assertion is valid here.
        assert_eq!(p.theme_animated(), Theme::dark());
        p.set_mode(ThemeMode::Light);
        assert_eq!(p.theme_animated(), Theme::light());
    }

    #[test]
    fn r57_x_theme_animated_first_call_returns_current_target() {
        // First call inside an owner scope lazy-inits the fade with
        // current = target, which leaves the spring at rest
        // immediately. The at-rest snap path engages and returns the
        // exact `theme()` target rather than the lossy linear-space
        // round-trip — so an exact-equality assertion is valid here.
        set_system_color_scheme(SystemColorScheme::NoPreference);
        let owner = Owner::new();
        let p = ThemeProvider::new();
        p.set_mode(ThemeMode::Dark);
        owner.run(|| {
            assert_eq!(p.theme_animated(), Theme::dark());
        });
    }

    #[test]
    fn r57_x_theme_animated_mode_flip_settles_to_new_target() {
        // After a mode flip, ticking the animation past the M3 short4
        // settling time (~200 ms) brings the spring to rest. With the
        // at-rest snap path engaged, the displayed palette equals the
        // new target exactly (not just within the round-trip
        // tolerance). We tick generously (1 s @ 60 Hz) so a future
        // spring re-tune that widens the settle window slightly does
        // not flake.
        set_system_color_scheme(SystemColorScheme::NoPreference);
        let owner = Owner::new();
        let p = ThemeProvider::new();
        // Anchor at Light first.
        p.set_mode(ThemeMode::Light);
        owner.run(|| {
            let _ = p.theme_animated();
        });
        // Re-target to Dark.
        p.set_mode(ThemeMode::Dark);
        owner.run(|| {
            let _ = p.theme_animated();
        });
        // Tick well past the 200 ms settle.
        for _ in 0..60 {
            owner.tick_animations(1.0 / 60.0);
        }
        owner.run(|| {
            assert_eq!(p.theme_animated(), Theme::dark());
        });
    }

    #[test]
    fn r57_x_theme_animated_first_post_flip_frame_is_near_previous_palette() {
        // Right after a re-target — before any tick has advanced the
        // spring — the displayed palette must read close to the
        // previous target (the in-flight anchor), not the new
        // target. Pins the velocity-preserving interrupt semantic
        // at the boundary: the very first frame after the flip
        // should not snap to the destination.
        set_system_color_scheme(SystemColorScheme::NoPreference);
        let owner = Owner::new();
        let p = ThemeProvider::new();
        p.set_mode(ThemeMode::Light);
        owner.run(|| {
            let _ = p.theme_animated();
        });
        // Settle so the previous anchor is exactly Light.
        for _ in 0..60 {
            owner.tick_animations(1.0 / 60.0);
        }
        p.set_mode(ThemeMode::Dark);
        owner.run(|| {
            // First call after flip — re-target happens, but the
            // spring has not stepped yet, so the displayed value is
            // still the Light anchor.
            assert_theme_close(p.theme_animated(), Theme::light());
        });
    }

    #[test]
    fn r57_x_theme_animated_palette_swap_triggers_fade() {
        // Application brand override via set_light_palette must be
        // a swap source the fade reacts to — pinned because the
        // application path (custom brand colors) is just as
        // important as the mode flip path.
        set_system_color_scheme(SystemColorScheme::NoPreference);
        let owner = Owner::new();
        let p = ThemeProvider::new();
        p.set_mode(ThemeMode::Light);
        let custom_light = Theme {
            accent: Color::rgb(0xff, 0x00, 0x00),
            ..Theme::light()
        };
        owner.run(|| {
            let _ = p.theme_animated();
        });
        p.set_light_palette(custom_light);
        owner.run(|| {
            let _ = p.theme_animated();
        });
        for _ in 0..60 {
            owner.tick_animations(1.0 / 60.0);
        }
        owner.run(|| {
            // Settled — at-rest snap returns the exact custom palette,
            // not the lossy linear-space round-trip.
            assert_eq!(p.theme_animated(), custom_light);
        });
    }

    #[test]
    fn r57_x_theme_animated_system_scheme_flip_triggers_fade() {
        // OS dark mode signal flips while mode is System — the fade
        // must react. Pins the R57.1 OS bridge cascade end-to-end
        // through the new accessor.
        set_system_color_scheme(SystemColorScheme::Light);
        let owner = Owner::new();
        let p = ThemeProvider::new();
        // ThemeMode::System is the default; do not call set_mode.
        owner.run(|| {
            let _ = p.theme_animated();
        });
        // Settle on light.
        for _ in 0..60 {
            owner.tick_animations(1.0 / 60.0);
        }
        set_system_color_scheme(SystemColorScheme::Dark);
        owner.run(|| {
            let _ = p.theme_animated();
        });
        for _ in 0..60 {
            owner.tick_animations(1.0 / 60.0);
        }
        owner.run(|| {
            // Settled — at-rest snap returns the exact dark palette.
            assert_eq!(p.theme_animated(), Theme::dark());
        });
        // Restore baseline for the next test on this thread.
        set_system_color_scheme(SystemColorScheme::NoPreference);
    }

    #[test]
    fn r57_x_theme_animated_at_rest_returns_exact_target_for_midrange_channels() {
        // Pin the at-rest exact-equality contract for midrange-channel
        // palette colors (`#121212`, `#E6E0E9`, `#36343B`, ...). The
        // ThemeLinear sRGB-to-linear-to-sRGB round-trip can drift ±1
        // 8-bit unit per channel for midrange values (verified by
        // `crate::style::tests::srgb_round_trip_midrange_close`). The
        // theme_animated() at-rest snap path returns the cached
        // target directly, bypassing the round-trip, so widget cascade
        // tests can assert exact equality against the active palette
        // (`==` rather than tolerance-based `assert_color_close`).
        //
        // Without the snap, R57.X.theme-fade cascade widget tests on
        // dark surface (`#121212`), light surface_container_highest
        // (`#E6E0E9`), light outline (`#C0C0C0`), and dark outline
        // (`#404040`) would fail intermittently — the lossy round-trip
        // would land at `#111111` / `#E7E1EA` / ... on some platforms.
        set_system_color_scheme(SystemColorScheme::NoPreference);
        let owner = Owner::new();
        let p = ThemeProvider::new();
        for (mode, expected) in [
            (ThemeMode::Light, Theme::light()),
            (ThemeMode::Dark, Theme::dark()),
        ] {
            p.set_mode(mode);
            owner.run(|| {
                let _ = p.theme_animated();
            });
            for _ in 0..60 {
                owner.tick_animations(1.0 / 60.0);
            }
            owner.run(|| {
                assert_eq!(
                    p.theme_animated(),
                    expected,
                    "at-rest snap must return exact palette for {mode:?}",
                );
            });
        }
    }

    #[test]
    fn r57_x_theme_animated_no_flip_stays_at_rest() {
        // After settling, repeated theme_animated() calls without a
        // target change must return the same value — the spring is
        // at rest and the spring-step is a no-op (Signal equality
        // skip + tickable rest-predicate). Guards against an
        // accidental "always retarget" wiring that would re-fire
        // the spring every paint.
        set_system_color_scheme(SystemColorScheme::NoPreference);
        let owner = Owner::new();
        let p = ThemeProvider::new();
        p.set_mode(ThemeMode::Dark);
        owner.run(|| {
            let _ = p.theme_animated();
        });
        for _ in 0..60 {
            owner.tick_animations(1.0 / 60.0);
        }
        owner.run(|| {
            let a = p.theme_animated();
            for _ in 0..30 {
                owner.tick_animations(1.0 / 60.0);
            }
            let b = p.theme_animated();
            // Within the at-rest epsilon — any drift here would be
            // a substrate regression, not 8-bit rounding noise.
            assert_eq!(a, b);
        });
    }
}
