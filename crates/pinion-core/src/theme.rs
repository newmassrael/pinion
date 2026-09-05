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
//! ## First-slice scope (R57.0 + R57.X.toggle extension + R57.1 + R590)
//!
//! Tier 1 color roles: [`ColorRole::Surface`], [`ColorRole::OnSurface`],
//! [`ColorRole::OnSurfaceMuted`], [`ColorRole::Accent`],
//! [`ColorRole::OnAccent`], [`ColorRole::Outline`], plus the four
//! Material 3 surface-elevation tiers ([`ColorRole::SurfaceContainerLow`]
//! ... [`ColorRole::SurfaceContainerHighest`]) the `hello-listbox`
//! retrofit (R57.X.listbox) surfaced. R590 adds the Material 3 error
//! tier ([`ColorRole::Error`], [`ColorRole::OnError`],
//! [`ColorRole::ErrorContainer`], [`ColorRole::OnErrorContainer`]) so
//! widgets in invalid / destructive state — disabled-button tonal
//! signalling, validation banners, destructive confirm dialogs — share
//! one canonical role family instead of hard-coding hex literals. The
//! role enum's `#[non_exhaustive]` annotation keeps every future
//! extension `SemVer`-safe.
//!
//! Subsequent slices (R57.2+) layer in the remaining Material 3
//! container / variant pairs (`primaryContainer` /
//! `onPrimaryContainer`, secondary / tertiary role families), the
//! typography token surface (font-size / line-height roles), and the
//! spacing token surface — every extension lands behind the same
//! `#[non_exhaustive]` shape on [`ColorRole`].
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
//! [`AnimVec4`] space via the private `ThemeLinear` carrier so the
//! perceptual quality matches [`Color::lerp`](crate::style::Color::lerp)
//! (R51.151) — no muddy-grey artifact on a light↔dark swap.
//!
//! The accessor uses the spring's velocity-preserving
//! [`Animation::set_target`](crate::animation::Animation::set_target) interrupt semantic (`SwiftUI` /
//! `another declarative toolkit` canonical), so a mid-fade mode flip transitions visually continuous
//! from the in-flight value to the new target — no discontinuity, no
//! double-snap. At rest the accessor short-circuits to the cached sRGB target
//! so widget cascade tests asserting against palette fields keep an
//! exact-equality contract: the linear-light round-trip is only on the wire
//! during the fade.
//!
//! Callers retain the instant [`ThemeProvider::theme`] accessor for
//! tests and snapshot reads. Opt-in keeps the substrate layered
//! ([[abstraction-needs-second-consumer]]); widget retrofit to
//! `Self::theme_animated` is a follow-up cascade with explicit
//! visible-affordance scope.
//!
//! [`use_text_edit_state`]: crate::widgets::text_edit::use_text_edit_state
//! [`AnimVec4`]: crate::animation::AnimVec4
//! [`Animation::set_target`]: crate::animation::Animation::set_target

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use crate::animation::{AnimVec4, Animatable, Animation, SpringConfig};
use crate::reactive::{Owner, Signal, batch};
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
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
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

impl SystemColorScheme {
    /// R606 §5.50 — every [`SystemColorScheme`] variant in a fixed,
    /// schema-stable order. Mirrors [`ColorRole::all`] (R595) so a
    /// downstream consumer (RPC introspection, doc generator, AT
    /// bridge) can iterate the slice once and trust the pinion-side
    /// enumeration as the canonical answer. A future Tier-2 variant
    /// lands at the end of the slice for the same reason the enum
    /// carries `#[non_exhaustive]`: callers that match on
    /// [`Self::name`] keep working without source edits.
    ///
    /// Pinned by `r606_system_color_scheme_all_enumerates_every_variant`.
    #[must_use]
    pub const fn all() -> &'static [SystemColorScheme] {
        &[
            SystemColorScheme::NoPreference,
            SystemColorScheme::Light,
            SystemColorScheme::Dark,
        ]
    }

    /// R606 §5.50 — canonical `snake_case` wire identifier mirroring
    /// the W3C `prefers-color-scheme` media-query value names
    /// (`no-preference` is rendered as `no_preference` to keep the
    /// pinion wire shape `snake_case`-consistent with [`ColorRole::name`]).
    ///
    /// The match is hand-written rather than derived (variant name
    /// `CamelCase` → `snake_case`) so the wire id stays stable across
    /// future enum-variant renames. A `Debug` / `strum` derivation
    /// would silently leak a rename into the wire — the opposite of
    /// what introspection consumers want.
    ///
    /// Internal to pinion-core: the match is exhaustive on the
    /// `#[non_exhaustive]` enum (intra-crate patterns can be
    /// exhaustive); a future variant addition fails to compile here
    /// and forces the maintainer to choose a wire id deliberately
    /// rather than fall through a silent default in a downstream
    /// crate.
    ///
    /// Pinned by `r606_system_color_scheme_name_round_trips_with_all`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            SystemColorScheme::NoPreference => "no_preference",
            SystemColorScheme::Light => "light",
            SystemColorScheme::Dark => "dark",
        }
    }
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
    /// **Component boundary** — the stroke on the edge of a control: an input
    /// field's border, a card's outline, a frame. Material 3 `outline`, and
    /// WCAG 1.4.11's 3:1 non-text floor applies to it
    /// ([`legibility::PAIRINGS`](crate::legibility::PAIRINGS) declares it).
    ///
    /// ★ R1839 — this said *"hairline / divider / input-border color"*, naming
    /// two jobs a design system separates, and R1807 carried an open debt
    /// about the ambiguity. Counted over six painted screens the role draws
    /// **97 boundaries and 2 dividers**, so it is a boundary role and is
    /// documented as one. A hairline that must read as *below* the surface is
    /// deliberately not this: see `hello-analyzer-shell`'s canvas grid ink,
    /// whose own comment says it is not a theme role.
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
    /// Destructive / invalid signal color — used by validation
    /// banners, error icons, destructive button fills (delete /
    /// discard), disabled-button red accent the existing
    /// `hello-button` carry surfaces. Material 3 `error`.
    Error,
    /// Foreground rendered on top of an [`Self::Error`] fill — paired
    /// for guaranteed contrast against the red signal tone. Material 3
    /// `onError`.
    OnError,
    /// Filled-but-low-emphasis error surface — used by tinted error
    /// containers (helper text strip beneath a `TextField` in invalid
    /// state, error-banner background). Material 3 `errorContainer`.
    ErrorContainer,
    /// Foreground rendered on top of an [`Self::ErrorContainer`] fill —
    /// the legible body / icon color a tinted error surface should
    /// carry. Material 3 `onErrorContainer`.
    OnErrorContainer,
    /// A surface tone *inverse* to the active scheme — dark on a light
    /// theme, light on a dark theme. Material 3 `inverseSurface`; the
    /// canonical container for plain tooltips and snackbars, which sit
    /// "above" the UI and read against the opposite tone for emphasis.
    InverseSurface,
    /// Foreground rendered on top of an [`Self::InverseSurface`] fill —
    /// paired for contrast against the inverted surface. Material 3
    /// `inverseOnSurface`.
    InverseOnSurface,
    /// The [`Self::Accent`] tone re-toned to read against an
    /// [`Self::InverseSurface`] fill — e.g. a snackbar's action label.
    /// Material 3 `inversePrimary`.
    InversePrimary,
    /// R1651 — a state that is wrong and does **not** stop the user.
    ///
    /// Material 3's role set has an error tier and no warning tier, and this
    /// project follows it everywhere else; the divergence is argued rather
    /// than assumed. `pinion_core::widgets::config_form::ConfigDefect` is a
    /// vocabulary whose arms differ in exactly one way — whether the defect
    /// blocks — and a palette with one alarm tone can only paint half of it.
    /// A gate that showed a non-blocking defect in the error tone would say
    /// "you cannot start" about the one case where you can, and the mature
    /// toolkit's palette has no warning role either, so there is nothing to
    /// borrow. Two roles, not four: nothing here fills a warning *container*
    /// yet, and a role no surface resolves is a token nobody can be wrong
    /// about.
    Warning,
    /// Foreground drawn on a [`Self::Warning`] fill.
    OnWarning,
    /// R2012 — a state that is **right**: an act that happened, a check that
    /// passed, a link that is up.
    ///
    /// ★★★★★ The argument is a count, not a convention. A design system
    /// authored for this project's own screens declares its state colours as a
    /// **closed enumeration of four** — right / caution / wrong / informational
    /// — and paints all four; this vocabulary carried the middle two and had
    /// nowhere to put the outer ones. Measured over that system's screens, the
    /// right-tone ink is the most-used state colour of the four (58 text sites
    /// against the wrong tone's 42), so it is not a rounding-out of the tier: it
    /// is the state a tool that watches something healthy says most often.
    ///
    /// ⚠ Material 3 has no such role and neither does the mature toolkit at
    /// 6.11, so there is nothing to borrow and the default tones below are
    /// argued in [`Theme::light`] rather than cited.
    Success,
    /// Foreground drawn on a [`Self::Success`] fill.
    OnSuccess,
    /// R2012 — a state that is **neither right nor wrong**: a fact the reader
    /// is being told, which no act is asked about.
    ///
    /// The arm exists because painting such a mark in [`Self::Warning`] tells a
    /// person something is off when nothing is: measured on the capture screen,
    /// a message that is simply one piece of a larger one was drawn in the
    /// caution tone, beside the genuinely faulty `Drop` marker in the wrong
    /// tone, so the two faults and the one non-fault came in two colours
    /// instead of three.
    Info,
    /// Foreground drawn on an [`Self::Info`] fill.
    OnInfo,
}

impl ColorRole {
    /// Every [`ColorRole`] variant in a fixed, schema-stable order.
    /// First introduced in R595 §5.50 to support introspection
    /// surfaces (RPC clients enumerating theme tokens, AT bridges
    /// listing role labels, doc generators) without each call site
    /// re-listing the enum and risking drift when a future Tier 2
    /// addition lands behind `#[non_exhaustive]`.
    ///
    /// The order matches the variant declaration order — `Surface`
    /// first, the surface-elevation tiers next, the error tier last
    /// — so a downstream consumer can iterate the slice once and
    /// trust the pinion-side schema as the canonical answer. A future
    /// extension lands at the end of the slice for the same reason
    /// the enum carries `#[non_exhaustive]`: callers that match on
    /// `name()` keep working without source edits.
    ///
    /// Pinned by `r595_all_enumerates_every_variant`.
    #[must_use]
    pub const fn all() -> &'static [ColorRole] {
        &[
            ColorRole::Surface,
            ColorRole::OnSurface,
            ColorRole::OnSurfaceMuted,
            ColorRole::Accent,
            ColorRole::OnAccent,
            ColorRole::Outline,
            ColorRole::SurfaceContainerHighest,
            ColorRole::SurfaceContainerLow,
            ColorRole::SurfaceContainer,
            ColorRole::SurfaceContainerHigh,
            ColorRole::Error,
            ColorRole::OnError,
            ColorRole::ErrorContainer,
            ColorRole::OnErrorContainer,
            ColorRole::InverseSurface,
            ColorRole::InverseOnSurface,
            ColorRole::InversePrimary,
            ColorRole::Warning,
            ColorRole::OnWarning,
            ColorRole::Success,
            ColorRole::OnSuccess,
            ColorRole::Info,
            ColorRole::OnInfo,
        ]
    }

    /// Canonical `snake_case` identifier — mirrors the [`Theme`] field
    /// name and the Material 3 token slug downstream consumers
    /// (RPC introspection, AT a11y labels, doc generators) already
    /// expect.
    ///
    /// The mapping is hand-written rather than derived (variant name
    /// `CamelCase` → field `snake_case`) so renames stay one-way: a
    /// future variant rename triggers the compiler's exhaustive
    /// match arm here, surfacing the rename to anyone relying on the
    /// stable wire identifier. A `Debug` / `strum` derivation would
    /// silently track the variant name and leak the rename into the
    /// wire — the opposite of what introspection consumers want.
    ///
    /// Pinned by `r595_name_round_trips_with_theme_field_naming`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            ColorRole::Surface => "surface",
            ColorRole::OnSurface => "on_surface",
            ColorRole::OnSurfaceMuted => "on_surface_muted",
            ColorRole::Accent => "accent",
            ColorRole::OnAccent => "on_accent",
            ColorRole::Outline => "outline",
            ColorRole::SurfaceContainerHighest => "surface_container_highest",
            ColorRole::SurfaceContainerLow => "surface_container_low",
            ColorRole::SurfaceContainer => "surface_container",
            ColorRole::SurfaceContainerHigh => "surface_container_high",
            ColorRole::Error => "error",
            ColorRole::OnError => "on_error",
            ColorRole::ErrorContainer => "error_container",
            ColorRole::OnErrorContainer => "on_error_container",
            ColorRole::InverseSurface => "inverse_surface",
            ColorRole::InverseOnSurface => "inverse_on_surface",
            ColorRole::InversePrimary => "inverse_primary",
            ColorRole::Warning => "warning",
            ColorRole::OnWarning => "on_warning",
            ColorRole::Success => "success",
            ColorRole::OnSuccess => "on_success",
            ColorRole::Info => "info",
            ColorRole::OnInfo => "on_info",
        }
    }

    /// Parse a canonical `snake_case` wire identifier back into a
    /// [`ColorRole`]. Inverse of [`Self::name`]: `name(from_name(s)) == s`
    /// for every variant in [`Self::all`], and `from_name(name(v)) == v`
    /// for every variant. Returns [`None`] for any string that does not
    /// match a registered variant's `name()` — including misspellings,
    /// case-folded forms (`"Surface"`), and dropped underscores
    /// (`"onsurface"`). Match is `&str`-equality, not heuristic.
    ///
    /// The implementation walks [`Self::all`] and tests `name() == s`
    /// rather than a hand-written reverse `match`, so a future variant
    /// addition that lands at the end of `all()` lights up
    /// automatically. The cost is linear in the variant count, which is
    /// acceptable since the slice is small and the call sits behind
    /// JSON-RPC parsing (already JSON-deserialization-bound).
    ///
    /// Introduced in R608 §5.50 as the parsing pair to [`Self::name`]:
    /// the AI-first write path (`scene/set_theme_palettes`) deserializes
    /// per-role entries `{"role": "<name>", "color": "<hex>"}` and
    /// needs the canonical name → variant inverse to validate the
    /// palette before handing it to
    /// [`ThemeProvider::set_palettes`](crate::theme::ThemeProvider::set_palettes).
    /// Stays paired with `name()` so adding a `ColorRole` variant
    /// requires only extending `all()` + the field/factory pairing on
    /// [`Theme`] — every consumer that goes through the
    /// `name()` ↔ `from_name()` surface keeps working.
    #[must_use]
    pub fn from_name(s: &str) -> Option<Self> {
        Self::all().iter().copied().find(|role| role.name() == s)
    }

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
    /// ★★★★★ R2012 — this now READS [`Theme::light`] instead of re-spelling it.
    ///
    /// It used to be a nineteen-arm `match` carrying its own copy of every
    /// light-palette value, under a doc comment saying it *returns the
    /// light-palette default*. That sentence was a claim about two hand-written
    /// lists agreeing, and a claim like that is exactly what this workspace has
    /// paid for repeatedly: nothing compared them, so the day one moved was the
    /// day the sentence became false, silently and in the direction of a
    /// palette tweak that landed in one place.
    ///
    /// ⚠ Measured before the change rather than assumed: the two lists agreed
    /// on all nineteen roles, so this is not a bug fix — it is the removal of
    /// the second copy BEFORE it drifts, prompted by the compiler asking for
    /// four more values that would have deepened it. The sentence is now true
    /// by construction, and a new role gets one default rather than two.
    #[must_use]
    pub const fn default_for(self) -> Color {
        Theme::light().resolve(self)
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    /// Resolves [`ColorRole::Error`].
    pub error: Color,
    /// Resolves [`ColorRole::OnError`].
    pub on_error: Color,
    /// Resolves [`ColorRole::ErrorContainer`].
    pub error_container: Color,
    /// Resolves [`ColorRole::OnErrorContainer`].
    pub on_error_container: Color,
    /// Resolves [`ColorRole::InverseSurface`].
    pub inverse_surface: Color,
    /// Resolves [`ColorRole::InverseOnSurface`].
    pub inverse_on_surface: Color,
    /// Resolves [`ColorRole::InversePrimary`].
    pub inverse_primary: Color,
    /// Resolves [`ColorRole::Warning`].
    pub warning: Color,
    /// Resolves [`ColorRole::OnWarning`].
    pub on_warning: Color,
    /// Resolves [`ColorRole::Success`].
    pub success: Color,
    /// Resolves [`ColorRole::OnSuccess`].
    pub on_success: Color,
    /// Resolves [`ColorRole::Info`].
    pub info: Color,
    /// Resolves [`ColorRole::OnInfo`].
    pub on_info: Color,
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
    /// - `outline` = `#949494`, 3.03:1 on white. ★ R1841 — this line said
    ///   `#C0C0C0` ("the canonical W3C 1px hairline") until R1841 measured it:
    ///   R1839 raised the value here for the WCAG 1.4.11 3:1 boundary floor and
    ///   did not carry the change into this list two hundred lines above it. A
    ///   doc that names a colour is a second declaration of it.
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
    /// - `error` = `#B3261E`, the Material 3 light `error` (Error 40)
    ///   tone — destructive / invalid signal at 5.9:1 against
    ///   `surface`.
    /// - `on_error` = pure white (`#FFFFFF`), 5.9:1 against `error`.
    /// - `error_container` = `#F9DEDC`, the Material 3 light
    ///   `errorContainer` (Error 90) tone — tinted error surface for
    ///   helper-text strips and banners (1.1:1 against `surface`,
    ///   distinguished by chroma rather than luminance).
    /// - `on_error_container` = `#410E0B`, the Material 3 light
    ///   `onErrorContainer` (Error 10) tone — 11.6:1 against
    ///   `error_container` (WCAG AAA).
    #[must_use]
    pub const fn light() -> Self {
        Self {
            surface: Color::rgb(0xff, 0xff, 0xff),
            on_surface: Color::rgb(0x1a, 0x1a, 0x1a),
            on_surface_muted: Color::rgb(0x60, 0x60, 0x60),
            accent: Color::rgb(0x19, 0x76, 0xd2),
            on_accent: Color::rgb(0xff, 0xff, 0xff),
            outline: Color::rgb(0x94, 0x94, 0x94),
            surface_container_highest: Color::rgb(0xe6, 0xe0, 0xe9),
            surface_container_low: Color::rgb(0xf7, 0xf2, 0xfa),
            surface_container: Color::rgb(0xf3, 0xed, 0xf7),
            surface_container_high: Color::rgb(0xec, 0xe6, 0xf0),
            error: Color::rgb(0xb3, 0x26, 0x1e),
            on_error: Color::rgb(0xff, 0xff, 0xff),
            error_container: Color::rgb(0xf9, 0xde, 0xdc),
            on_error_container: Color::rgb(0x41, 0x0e, 0x0b),
            inverse_surface: Color::rgb(0x32, 0x2f, 0x35),
            inverse_on_surface: Color::rgb(0xf5, 0xef, 0xf7),
            inverse_primary: Color::rgb(0x9e, 0xca, 0xff),
            warning: Color::rgb(0x7a, 0x53, 0x00),
            on_warning: Color::rgb(0xff, 0xff, 0xff),
            // ★★★★★ R2012 — the two state tones this vocabulary was short of,
            // and the reason each hue was picked rather than inherited.
            //
            // `success` is the Material 3 green-40 tone. It reads 6.47 on this
            // palette's `surface` and carries white at the same 6.47, headroom
            // comparable to `error`'s 6.54 and `warning`'s 6.85 — the tier is
            // deliberately uniform, because a state colour that is legible only
            // in some of its arms is worse than one that is legible in none: a
            // reader learns to trust it.
            //
            // ⚠ `info` is a TEAL and not the conventional blue, and that is a
            // fact about THIS palette rather than about informational marks.
            // The default `accent` here is `#1976D2`; an informational tone in
            // the same hue window would read as an interactive one, which is
            // the one thing a mark that asks for nothing must not do. A palette
            // whose accent is not blue can put its `info` back in the blue
            // window by supplying its own values — that is what the role is
            // for. Measured 6.47 on `surface`, and white on it at 6.47.
            //
            // ⚠⚠ STATED LIMIT: nothing here checks colour-vision separation.
            // These two clear the CONTRAST floors that `crate::legibility`
            // declares, and no gate in this tree asks whether `success` and
            // `info` stay apart under deuteranopia or tritanopia. Choosing the
            // hues from four separated windows is a convention, not a solved
            // optimisation, and a palette that needs the solved answer supplies
            // it rather than deriving it from here.
            success: Color::rgb(0x00, 0x6e, 0x1c),
            on_success: Color::rgb(0xff, 0xff, 0xff),
            info: Color::rgb(0x00, 0x69, 0x6e),
            on_info: Color::rgb(0xff, 0xff, 0xff),
        }
    }

    /// Canonical Dark Mode palette — Material 3 dark baseline with
    /// the accent lightened so the dark surface keeps WCAG AA
    /// contrast on every paired **text** role.
    ///
    /// ★★★★★ R1807 — that sentence used to say "every paired role" and was
    /// **false**, for a year and by nobody's fault in particular: no gate could
    /// read it. `inverse_primary` on `inverse_surface` measured `3.56` here
    /// against the light palette's `7.75` for the same pairing. It is now
    /// checked rather than claimed — [`crate::legibility`] declares the
    /// ink-over-ground table and its tests fail if the two palettes ever again
    /// disagree about whether a pairing is legible. The word `text` above is
    /// load-bearing: the one BOUNDARY pairing (`outline` on `surface`) is short
    /// of its floor in both palettes, which that module reports separately and
    /// deliberately does not fold into the parity verdict.
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
    /// - `outline` = `#616161`, 3.03:1 on `#121212`. ★ R1841 — as with the
    ///   light palette above, this said `#404040` after R1839 had raised it,
    ///   which is the same defect twice in one file.
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
    /// - `error` = `#F2B8B5`, the Material 3 dark `error` (Error 80)
    ///   tone — lifted from Error 40 so the red signal stays legible
    ///   against `#121212` (8.6:1 contrast, WCAG AAA).
    /// - `on_error` = `#601410` (Error 20), 8.6:1 against `error` for
    ///   the typography that sits on top of the lifted red signal.
    /// - `error_container` = `#8C1D18` (Error 30), the Material 3 dark
    ///   `errorContainer` tone — deeper red tinted surface for helper-
    ///   text strips and banners.
    /// - `on_error_container` = `#F9DEDC` (Error 90), 7.8:1 against
    ///   `error_container` (WCAG AAA).
    #[must_use]
    pub const fn dark() -> Self {
        Self {
            surface: Color::rgb(0x12, 0x12, 0x12),
            on_surface: Color::rgb(0xec, 0xec, 0xec),
            on_surface_muted: Color::rgb(0x9e, 0x9e, 0x9e),
            accent: Color::rgb(0x60, 0xa5, 0xfa),
            on_accent: Color::rgb(0x0b, 0x1f, 0x3f),
            outline: Color::rgb(0x61, 0x61, 0x61),
            surface_container_highest: Color::rgb(0x36, 0x34, 0x3b),
            surface_container_low: Color::rgb(0x1d, 0x1b, 0x20),
            surface_container: Color::rgb(0x21, 0x1f, 0x26),
            surface_container_high: Color::rgb(0x2b, 0x29, 0x30),
            error: Color::rgb(0xf2, 0xb8, 0xb5),
            on_error: Color::rgb(0x60, 0x14, 0x10),
            error_container: Color::rgb(0x8c, 0x1d, 0x18),
            on_error_container: Color::rgb(0xf9, 0xde, 0xdc),
            inverse_surface: Color::rgb(0xe6, 0xe1, 0xe5),
            inverse_on_surface: Color::rgb(0x32, 0x2f, 0x35),
            // ★★★★★ R1807 — was `#1976D2` (the light palette's own accent), and
            // measured against this palette's light `inverse_surface` it read
            // **3.56**, under the 4.5 an action label needs and under what this
            // constructor's own doc comment claims for "every paired role". The
            // light palette's mirror pairing reads 7.75, so the two themes
            // disagreed about whether a snackbar's action label is legible —
            // the exact defect `light and dark parity` is a claim about.
            //
            // Material Blue 900 rather than Blue 800 (`#1565C0`, measured 4.45):
            // 4.45 is under the floor, and picking a value that lands within a
            // rounding error of it would make the gate a coin toss on the next
            // palette tweak. This reads 6.69, comparable headroom to the light
            // side's 7.75. Pinned by `pinion_core::legibility`.
            inverse_primary: Color::rgb(0x0d, 0x47, 0xa1),
            warning: Color::rgb(0xe8, 0xc0, 0x77),
            on_warning: Color::rgb(0x41, 0x2d, 0x00),
            // R2012 — the dark halves of the two tones the light palette
            // argues. Each is the light tone's hue lifted until it clears this
            // surface, and each carries a foreground dark enough to sit on it:
            // `success` 10.97 on `surface` and 7.74 under `on_success`, `info`
            // 10.95 and 7.65. The light palette's own two tones read 2.90 each
            // here — under even the 3.0 non-text floor — which is why a dark
            // half exists at all.
            success: Color::rgb(0x78, 0xdc, 0x77),
            on_success: Color::rgb(0x00, 0x39, 0x0a),
            info: Color::rgb(0x4f, 0xd8, 0xe4),
            on_info: Color::rgb(0x00, 0x37, 0x39),
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
            ColorRole::Error => self.error,
            ColorRole::OnError => self.on_error,
            ColorRole::ErrorContainer => self.error_container,
            ColorRole::OnErrorContainer => self.on_error_container,
            ColorRole::InverseSurface => self.inverse_surface,
            ColorRole::InverseOnSurface => self.inverse_on_surface,
            ColorRole::InversePrimary => self.inverse_primary,
            ColorRole::Warning => self.warning,
            ColorRole::OnWarning => self.on_warning,
            ColorRole::Success => self.success,
            ColorRole::OnSuccess => self.on_success,
            ColorRole::Info => self.info,
            ColorRole::OnInfo => self.on_info,
        }
    }
}

/// (R2017 §5.50) One role two palettes answer differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoleDifference {
    /// The role they disagree about.
    pub role: ColorRole,
    /// What the palette asked answered.
    pub mine: Color,
    /// What the palette compared against answered.
    pub theirs: Color,
}

impl fmt::Display for RoleDifference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {:02X}{:02X}{:02X} vs {:02X}{:02X}{:02X}",
            self.role.name(),
            self.mine.r,
            self.mine.g,
            self.mine.b,
            self.theirs.r,
            self.theirs.g,
            self.theirs.b
        )
    }
}

/// (R2016 §5.50) Why an authored palette could not be taken.
///
/// ★★★★★ It names EVERY role the document is short of, and serde does not.
/// `serde_json::from_str::<Theme>` stops at the first missing field and says
/// only *missing field: info*, which tells a person to add one thing and send
/// it again — and then tells them the next one. A palette arrives from a design
/// system as a whole document, so the useful answer is the whole shortfall at
/// once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeGap {
    /// Roles this vocabulary requires that the document does not bind, in
    /// [`ColorRole::all`] order.
    pub missing: Vec<ColorRole>,
    /// Keys the document binds that name no role, sorted.
    ///
    /// Reported rather than ignored: a key that resolves to nothing is
    /// usually a role the AUTHOR has and this vocabulary does not, which is
    /// the more interesting half of the mismatch and the one a silent parser
    /// throws away.
    pub unknown: Vec<String>,
}

impl fmt::Display for ThemeGap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the palette binds no ")?;
        for (i, role) in self.missing.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "`{}`", role.name())?;
        }
        if self.missing.is_empty() {
            write!(f, "role that is missing")?;
        }
        if !self.unknown.is_empty() {
            write!(f, "; and binds {:?}, which name no role", self.unknown)?;
        }
        Ok(())
    }
}

impl Theme {
    /// (R2016 §5.50) Take a palette authored elsewhere, or say what it owes.
    ///
    /// The document is a JSON object keyed by [`ColorRole::name`], each value a
    /// [`Color`] — which is what `serde`'s own derive on this struct already
    /// accepts, so an authoring tool that targets the derive needs no adapter.
    /// What this adds is the REFUSAL: every missing role at once, and every key
    /// that names none.
    ///
    /// ★★★★★ WHY THIS EXISTS AT ALL, measured rather than assumed. A design
    /// system authored for this project emits exactly this shape and has done
    /// for some time; the framework could always have parsed it and NOTHING IN
    /// THE TREE CALLED THE PARSER — the bridge was built from one side and
    /// nobody crossed. The first thing that happens when you do cross is that
    /// the document turns out to be short, because this vocabulary grew two
    /// state tiers after the exporter was written. So the crossing and the
    /// naming of the shortfall are the same act, and a reader that could only
    /// say *missing field* would have made the second half somebody's manual
    /// diff.
    ///
    /// # Errors
    ///
    /// [`ThemeGap`] when any role is unbound, listing them all. A document with
    /// unknown keys and no missing roles is ACCEPTED — the extra keys are
    /// reported through [`Self::take_palette`] rather than refused, because a
    /// palette that binds everything this vocabulary has is usable whatever
    /// else it carries.
    pub fn from_wire(document: &str) -> Result<Self, ThemeGap> {
        Self::take_palette(document).map(|(theme, _)| theme)
    }

    /// (R2016 §5.50) [`Self::from_wire`], and the keys it did not recognise.
    ///
    /// Two returns rather than two functions because the parse is one pass and
    /// a caller that wants both should not run it twice.
    ///
    /// # Errors
    ///
    /// [`ThemeGap`] when a role is unbound — see [`Self::from_wire`].
    pub fn take_palette(document: &str) -> Result<(Self, Vec<String>), ThemeGap> {
        let bound: std::collections::BTreeMap<String, Color> =
            serde_json::from_str(document).unwrap_or_default();
        let missing: Vec<ColorRole> = ColorRole::all()
            .iter()
            .copied()
            .filter(|role| !bound.contains_key(role.name()))
            .collect();
        let unknown: Vec<String> = bound
            .keys()
            .filter(|key| ColorRole::from_name(key).is_none())
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Err(ThemeGap { missing, unknown });
        }
        // Every role is bound, so the derive cannot fail on a missing field and
        // the only remaining shape error is one this map already rejected.
        let mut theme = Self::light();
        for role in ColorRole::all() {
            let colour = bound[role.name()];
            theme.bind(*role, colour);
        }
        Ok((theme, unknown))
    }

    /// (R2018 §5.50) The four container tiers, lowest elevation first.
    ///
    /// The ladder proper. [`ColorRole::Surface`] is deliberately NOT in it —
    /// see [`Self::elevation_inversions`].
    pub const ELEVATION: [ColorRole; 4] = [
        ColorRole::SurfaceContainerLow,
        ColorRole::SurfaceContainer,
        ColorRole::SurfaceContainerHigh,
        ColorRole::SurfaceContainerHighest,
    ];

    /// (R2018 §5.50) Adjacent elevation tiers that step the wrong way, if any.
    ///
    /// A raised surface must read as raised: whichever way this palette's
    /// ladder runs — lighter with elevation on a dark ground, darker on a light
    /// one — every step has to run the same way, or two tiers a person is
    /// meant to tell apart swap places. Empty means the ladder is a ladder.
    ///
    /// ★★★★★ THIS IS THE HALF OF THE ELEVATION RULE THAT GENERALISES, AND
    /// SEPARATING IT OUT IS A MEASUREMENT RATHER THAN A TIDY-UP. R57.X pinned
    /// the progression INCLUDING `surface`, on the grounds that it is the
    /// lightest tone in a light palette and the darkest in a dark one — which
    /// is Material 3's baseline and is true of [`Self::light`] and
    /// [`Self::dark`], the only two palettes that pin ever ran on.
    ///
    /// Run against palettes real screens BIND, it fails: measured at R2018,
    /// the analysis shell's light palette and an authored design system's
    /// export — two sources that were written independently — BOTH put
    /// `surface` between `surface_container_low` and `surface_container`,
    /// because a grey page carrying white cards is a light theme people
    /// actually design. Their containers are monotonic; it is only `surface`'s
    /// place in the sequence that differs, and it differs the same way in both.
    ///
    /// ⇒ `surface`'s position is a design decision this framework does not get
    /// to make, and the containers' order is a property every palette here
    /// satisfies. The R57.X pins keep the stronger claim and keep it where it
    /// is true — on the two canonical palettes, whose whole job is to be the
    /// Material baseline.
    ///
    /// ⚠ Luminance is [`crate::contrast::relative_luminance`], the perceptual
    /// one, and not the `r + g + b` the R57.X pins use. Those two can disagree
    /// about a pair of near-equal tones; this reports what a reader's eye
    /// orders, which is what "reads as raised" means.
    #[must_use]
    pub fn elevation_inversions(&self) -> Vec<(ColorRole, ColorRole)> {
        let lum = |role: ColorRole| crate::contrast::relative_luminance(self.resolve(role));
        let (first, last) = (lum(Self::ELEVATION[0]), lum(Self::ELEVATION[3]));
        let rising = last >= first;
        Self::ELEVATION
            .windows(2)
            .filter_map(|pair| {
                let (a, b) = (pair[0], pair[1]);
                let steps_right = if rising {
                    lum(b) >= lum(a)
                } else {
                    lum(b) <= lum(a)
                };
                (!steps_right).then_some((a, b))
            })
            .collect()
    }

    /// (R2017 §5.50) Where this palette and `other` disagree, role by role.
    ///
    /// ★★★★★ THIS EXISTS BECAUSE A COMPARISON NOBODY CAN RUN IS A COMPARISON
    /// NOBODY HAS RUN. Two palettes for one product had been maintained side by
    /// side for a long time — one hand-authored in a screen, one exported by
    /// the design system that screen is drawn from — and the question *do they
    /// agree* had never been asked, because asking it meant somebody writing
    /// the loop. Measured the first time it was asked: they differ on **nine**
    /// of nineteen roles in the light palette and **fourteen** of nineteen in
    /// the dark, and part of that is systematic rather than noise — the
    /// screen's dark elevation ladder sits one rung off, its
    /// `surface_container_low` being exactly the other's `surface`.
    ///
    /// So the loop lives here, once, and the answer is a value a test or a tool
    /// can assert on. In [`ColorRole::all`] order, so two runs are comparable.
    #[must_use]
    pub fn differences(&self, other: &Self) -> Vec<RoleDifference> {
        ColorRole::all()
            .iter()
            .copied()
            .filter_map(|role| {
                let mine = self.resolve(role);
                let theirs = other.resolve(role);
                (mine != theirs).then_some(RoleDifference { role, mine, theirs })
            })
            .collect()
    }

    /// (R2016 §5.50) Set one role's colour.
    ///
    /// The write half of [`Self::resolve`], and exhaustive for the same reason:
    /// a role added to the enum must be placed here or the match does not
    /// compile.
    pub const fn bind(&mut self, role: ColorRole, colour: Color) {
        match role {
            ColorRole::Surface => self.surface = colour,
            ColorRole::OnSurface => self.on_surface = colour,
            ColorRole::OnSurfaceMuted => self.on_surface_muted = colour,
            ColorRole::Accent => self.accent = colour,
            ColorRole::OnAccent => self.on_accent = colour,
            ColorRole::Outline => self.outline = colour,
            ColorRole::SurfaceContainerHighest => self.surface_container_highest = colour,
            ColorRole::SurfaceContainerLow => self.surface_container_low = colour,
            ColorRole::SurfaceContainer => self.surface_container = colour,
            ColorRole::SurfaceContainerHigh => self.surface_container_high = colour,
            ColorRole::Error => self.error = colour,
            ColorRole::OnError => self.on_error = colour,
            ColorRole::ErrorContainer => self.error_container = colour,
            ColorRole::OnErrorContainer => self.on_error_container = colour,
            ColorRole::InverseSurface => self.inverse_surface = colour,
            ColorRole::InverseOnSurface => self.inverse_on_surface = colour,
            ColorRole::InversePrimary => self.inverse_primary = colour,
            ColorRole::Warning => self.warning = colour,
            ColorRole::OnWarning => self.on_warning = colour,
            ColorRole::Success => self.success = colour,
            ColorRole::OnSuccess => self.on_success = colour,
            ColorRole::Info => self.info = colour,
            ColorRole::OnInfo => self.on_info = colour,
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
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
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
    error: AnimVec4,
    on_error: AnimVec4,
    error_container: AnimVec4,
    on_error_container: AnimVec4,
    inverse_surface: AnimVec4,
    inverse_on_surface: AnimVec4,
    inverse_primary: AnimVec4,
    warning: AnimVec4,
    on_warning: AnimVec4,
    success: AnimVec4,
    on_success: AnimVec4,
    info: AnimVec4,
    on_info: AnimVec4,
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
            error: t.error.to_linear(),
            on_error: t.on_error.to_linear(),
            error_container: t.error_container.to_linear(),
            on_error_container: t.on_error_container.to_linear(),
            inverse_surface: t.inverse_surface.to_linear(),
            inverse_on_surface: t.inverse_on_surface.to_linear(),
            inverse_primary: t.inverse_primary.to_linear(),
            warning: t.warning.to_linear(),
            on_warning: t.on_warning.to_linear(),
            success: t.success.to_linear(),
            on_success: t.on_success.to_linear(),
            info: t.info.to_linear(),
            on_info: t.on_info.to_linear(),
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
            error: Color::from_linear(self.error),
            on_error: Color::from_linear(self.on_error),
            error_container: Color::from_linear(self.error_container),
            on_error_container: Color::from_linear(self.on_error_container),
            inverse_surface: Color::from_linear(self.inverse_surface),
            inverse_on_surface: Color::from_linear(self.inverse_on_surface),
            inverse_primary: Color::from_linear(self.inverse_primary),
            warning: Color::from_linear(self.warning),
            on_warning: Color::from_linear(self.on_warning),
            success: Color::from_linear(self.success),
            on_success: Color::from_linear(self.on_success),
            info: Color::from_linear(self.info),
            on_info: Color::from_linear(self.on_info),
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
            error: AnimVec4::zero(),
            on_error: AnimVec4::zero(),
            error_container: AnimVec4::zero(),
            on_error_container: AnimVec4::zero(),
            inverse_surface: AnimVec4::zero(),
            inverse_on_surface: AnimVec4::zero(),
            inverse_primary: AnimVec4::zero(),
            warning: AnimVec4::zero(),
            on_warning: AnimVec4::zero(),
            success: AnimVec4::zero(),
            on_success: AnimVec4::zero(),
            info: AnimVec4::zero(),
            on_info: AnimVec4::zero(),
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
            surface_container_low: self.surface_container_low.add(other.surface_container_low),
            surface_container: self.surface_container.add(other.surface_container),
            surface_container_high: self
                .surface_container_high
                .add(other.surface_container_high),
            error: self.error.add(other.error),
            on_error: self.on_error.add(other.on_error),
            error_container: self.error_container.add(other.error_container),
            on_error_container: self.on_error_container.add(other.on_error_container),
            inverse_surface: self.inverse_surface.add(other.inverse_surface),
            inverse_on_surface: self.inverse_on_surface.add(other.inverse_on_surface),
            inverse_primary: self.inverse_primary.add(other.inverse_primary),
            warning: self.warning.add(other.warning),
            on_warning: self.on_warning.add(other.on_warning),
            success: self.success.add(other.success),
            on_success: self.on_success.add(other.on_success),
            info: self.info.add(other.info),
            on_info: self.on_info.add(other.on_info),
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
            surface_container_low: self.surface_container_low.sub(other.surface_container_low),
            surface_container: self.surface_container.sub(other.surface_container),
            surface_container_high: self
                .surface_container_high
                .sub(other.surface_container_high),
            error: self.error.sub(other.error),
            on_error: self.on_error.sub(other.on_error),
            error_container: self.error_container.sub(other.error_container),
            on_error_container: self.on_error_container.sub(other.on_error_container),
            inverse_surface: self.inverse_surface.sub(other.inverse_surface),
            inverse_on_surface: self.inverse_on_surface.sub(other.inverse_on_surface),
            inverse_primary: self.inverse_primary.sub(other.inverse_primary),
            warning: self.warning.sub(other.warning),
            on_warning: self.on_warning.sub(other.on_warning),
            success: self.success.sub(other.success),
            on_success: self.on_success.sub(other.on_success),
            info: self.info.sub(other.info),
            on_info: self.on_info.sub(other.on_info),
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
            error: self.error.scale(factor),
            on_error: self.on_error.scale(factor),
            error_container: self.error_container.scale(factor),
            on_error_container: self.on_error_container.scale(factor),
            inverse_surface: self.inverse_surface.scale(factor),
            inverse_on_surface: self.inverse_on_surface.scale(factor),
            inverse_primary: self.inverse_primary.scale(factor),
            warning: self.warning.scale(factor),
            on_warning: self.on_warning.scale(factor),
            success: self.success.scale(factor),
            on_success: self.on_success.scale(factor),
            info: self.info.scale(factor),
            on_info: self.on_info.scale(factor),
        }
    }

    /// ★★★★★ R2012 — this DESTRUCTURES, and the other five arithmetic methods
    /// deliberately do not need to.
    ///
    /// Every other method here builds a `Self { .. }`, so a field added to the
    /// struct and forgotten in one of them is a COMPILE ERROR. This one folds
    /// over fields instead, and a fold that forgets a channel still compiles —
    /// so it is the one place a new role can go missing quietly, and it did:
    /// R1651 added the warning pair to all six sites and this fold had
    /// **seventeen of nineteen** channels. A spring is settled when its
    /// remaining distance and its velocity are both near zero
    /// ([`crate::animation`]'s `is_settled`), so a fold short of two channels
    /// reports a theme fade FINISHED while those two are still travelling —
    /// the caution tone freezing part-way between the two palettes, in the one
    /// role whose whole job is to be noticed.
    ///
    /// Binding every field by name is what makes the omission impossible: the
    /// pattern below fails to compile the moment [`ThemeLinear`] grows a field,
    /// which is the same idiom this workspace uses wherever a hand-written
    /// field list would otherwise rot.
    fn approx_zero(self, epsilon: f32) -> bool {
        let Self {
            surface,
            on_surface,
            on_surface_muted,
            accent,
            on_accent,
            outline,
            surface_container_highest,
            surface_container_low,
            surface_container,
            surface_container_high,
            error,
            on_error,
            error_container,
            on_error_container,
            inverse_surface,
            inverse_on_surface,
            inverse_primary,
            warning,
            on_warning,
            success,
            on_success,
            info,
            on_info,
        } = self;
        [
            surface,
            on_surface,
            on_surface_muted,
            accent,
            on_accent,
            outline,
            surface_container_highest,
            surface_container_low,
            surface_container,
            surface_container_high,
            error,
            on_error,
            error_container,
            on_error_container,
            inverse_surface,
            inverse_on_surface,
            inverse_primary,
            warning,
            on_warning,
            success,
            on_success,
            info,
            on_info,
        ]
        .into_iter()
        .all(|channel| channel.approx_zero(epsilon))
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
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
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

impl ThemeMode {
    /// R606 §5.50 — every [`ThemeMode`] variant in a fixed,
    /// schema-stable order. Same rationale + carry contract as
    /// [`SystemColorScheme::all`] and [`ColorRole::all`].
    ///
    /// Pinned by `r606_theme_mode_all_enumerates_every_variant`.
    #[must_use]
    pub const fn all() -> &'static [ThemeMode] {
        &[ThemeMode::Light, ThemeMode::Dark, ThemeMode::System]
    }

    /// R606 §5.50 — canonical `snake_case` wire identifier. Same
    /// rationale + maintenance contract as [`SystemColorScheme::name`]
    /// and [`ColorRole::name`].
    ///
    /// Pinned by `r606_theme_mode_name_round_trips_with_all`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
            ThemeMode::System => "system",
        }
    }
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
    /// animation. [`None`] until the first `Self::theme_animated`
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

    /// Replace both palettes in a single reactive batch. Equivalent
    /// to calling [`Self::set_light_palette`] then
    /// [`Self::set_dark_palette`] inside a [`batch`],
    /// but folds the two signal writes into one coalesced flush — every
    /// subscriber re-runs at most once even though two distinct
    /// signals were mutated.
    ///
    /// Use this when the application wants the light + dark palettes
    /// kept in lock-step (Material 3 dynamic-color: both tonal
    /// palettes derive from the same seed; an in-app `Settings` screen
    /// that lets the user pick a brand color produces both at once;
    /// a `prefers-color-scheme` flip is already deterministic via
    /// [`Self::set_mode`] and should not use this primitive).
    ///
    /// # Why a dedicated method instead of caller-side `batch`
    ///
    /// Two reasons. (a) Discoverability — a caller browsing the
    /// `ThemeProvider` surface should not have to know about the
    /// `crate::reactive::batch` helper to update palettes atomically;
    /// the substrate exposes the canonical action directly, mirroring
    /// the way `set_mode` + `set_X_palette` already do for individual
    /// writes. (b) Intent encoding — the call site reads as "swap the
    /// palette pair", not as "two independent writes that happen to
    /// be coalesced", which matches the Material 3 tonal-palette
    /// shape (light + dark derive together).
    ///
    /// `R593` regression `r593_set_palettes_atomic_batches_subscribers`
    /// pins the one-re-run contract; without it a refactor that
    /// dropped the `batch` wrap would silently double the work
    /// downstream view-fns do during a palette swap.
    pub fn set_palettes(&self, light: Theme, dark: Theme) {
        batch(|| {
            self.light_palette.set(light);
            self.dark_palette.set(dark);
        });
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
        if self.is_dark() {
            self.dark_palette.get()
        } else {
            self.light_palette.get()
        }
    }

    /// R906 §5.50 — whether the **effective** scheme resolves to dark: an
    /// explicit [`ThemeMode::Dark`] / [`ThemeMode::Light`], or — under
    /// [`ThemeMode::System`] — the global [`SystemColorScheme`]
    /// (`prefers-color-scheme`, `NoPreference` → light per the W3C baseline).
    /// The single resolution SSOT [`theme`](Self::theme) /
    /// [`theme_animated`](Self::theme_animated) pick the palette through, and
    /// the read a binding uses to choose a *non-`ColorRole`* asset by
    /// light/dark (e.g. a syntax-highlight scheme). Reactive: subscribes the
    /// caller to `mode` (+ the global `SystemColorScheme` when mode is
    /// `System`), so a `prefers-color-scheme` flip re-runs it.
    #[must_use]
    pub fn is_dark(&self) -> bool {
        match self.mode.get() {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => system_color_scheme() == SystemColorScheme::Dark,
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
    /// Mid-fade mode / palette / OS-scheme flips re-target the spring via
    /// [`Animation::set_target`](crate::animation::Animation::set_target); the existing velocity
    /// carries through so the displayed palette stays visually continuous
    /// across the interruption — the canonical `SwiftUI` `.animation(_)` / `another declarative toolkit` `animateColorAsState` continuity
    /// contract.
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
    /// ([`Animation::is_at_rest`](crate::animation::Animation::is_at_rest)), the accessor returns
    /// the cached sRGB [`Self::theme`] target directly rather than re-encoding the
    /// spring's linear-light state through `ThemeLinear::to_theme`. The
    /// [`Color::to_linear`](crate::style::Color::to_linear) /
    /// [`Color::from_linear`](crate::style::Color::from_linear) round-trip can drift
    /// midrange-channel values by ±1 8-bit unit (verified by `crate::style::tests::srgb_round_trip_midrange_close`), which would
    /// silently break widget cascade tests asserting exact equality against
    /// palette field values (e.g. [`Theme::dark`]`.surface = #121212`). Snapping at rest mirrors the
    /// canonical `SwiftUI` `.animation(_)` / `another declarative toolkit` `animateColorAsState` contract: while the animation is running
    /// the returned value is the interpolated state; once settled the value
    /// equals the target exactly. The intermediate in-flight frames still go
    /// through the linear-light path so the perceptual interpolation quality
    /// is preserved.
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
    use crate::reactive::{Effect, Owner};
    use crate::style::Color;
    use crate::test_fixtures::settle_owner_animations;
    use std::cell::Cell;
    use std::rc::Rc;

    // ─────────────────────────────────────────────────────────────
    // R606 §5.50 — ThemeMode / SystemColorScheme ::name + ::all
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r606_theme_mode_all_enumerates_every_variant() {
        // The slice must list every current variant in declaration
        // order. A future #[non_exhaustive] addition lands at the
        // end; this test will then need its expected list updated,
        // surfacing the new variant to anyone relying on ::all.
        let all = ThemeMode::all();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0], ThemeMode::Light);
        assert_eq!(all[1], ThemeMode::Dark);
        assert_eq!(all[2], ThemeMode::System);
    }

    #[test]
    fn r606_theme_mode_name_round_trips_with_all() {
        // Every variant in ::all must map to a stable wire id; no
        // "unknown" / "" sentinels. When a future variant lands,
        // ::all grows and this loop runs the new variant through
        // ::name — the exhaustive match in ::name fails to compile
        // until the maintainer chooses a wire id deliberately.
        let pairs: &[(ThemeMode, &str)] = &[
            (ThemeMode::Light, "light"),
            (ThemeMode::Dark, "dark"),
            (ThemeMode::System, "system"),
        ];
        assert_eq!(pairs.len(), ThemeMode::all().len());
        for (mode, expected) in pairs {
            assert_eq!(mode.name(), *expected);
        }
    }

    #[test]
    fn r606_system_color_scheme_all_enumerates_every_variant() {
        let all = SystemColorScheme::all();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0], SystemColorScheme::NoPreference);
        assert_eq!(all[1], SystemColorScheme::Light);
        assert_eq!(all[2], SystemColorScheme::Dark);
    }

    #[test]
    fn r606_system_color_scheme_name_round_trips_with_all() {
        let pairs: &[(SystemColorScheme, &str)] = &[
            (SystemColorScheme::NoPreference, "no_preference"),
            (SystemColorScheme::Light, "light"),
            (SystemColorScheme::Dark, "dark"),
        ];
        assert_eq!(pairs.len(), SystemColorScheme::all().len());
        for (scheme, expected) in pairs {
            assert_eq!(scheme.name(), *expected);
        }
    }

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
        // R1839 — raised from `#c0c0c0`, which measured 1.82 against a 3:1
        // boundary floor. The value is derived; see `legibility`.
        assert_eq!(t.outline, Color::rgb(0x94, 0x94, 0x94));
        assert_eq!(t.surface_container_highest, Color::rgb(0xe6, 0xe0, 0xe9),);
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
        // R1839 — raised from `#404040`, which measured 1.81. Derived.
        assert_eq!(t.outline, Color::rgb(0x61, 0x61, 0x61));
        assert_eq!(t.surface_container_highest, Color::rgb(0x36, 0x34, 0x3b),);
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
    ///
    /// ★★★★★ R2018.1 — **this reads the same lightness the rest of the tree
    /// reads.** It used to compare `r + g + b`, which is not a measure of how
    /// light a tone looks and was the tree's SECOND spelling of *lighter than*
    /// beside [`crate::contrast::relative_luminance`] — the one
    /// [`Theme::elevation_inversions`] uses, and the one every contrast floor
    /// is judged with. Two spellings of one property is the defect this
    /// workspace has paid for repeatedly; the channel-sum form was counted at
    /// exactly two sites, both here, and is now at none. Safe to unify because
    /// it was MEASURED first: on all six palettes this tree can reach — the
    /// two canonical, the two the analysis shell binds, the two an authored
    /// design system exports — the two arithmetics put the five tiers in the
    /// same order, so nothing that passed before fails now. That they CAN
    /// disagree is what the test below shows, which is why one had to be
    /// picked rather than either being fine.
    #[test]
    fn r57_x_surface_tiers_light_lightness_progression() {
        let t = Theme::light();
        let lum = crate::contrast::relative_luminance;
        assert!(lum(t.surface) >= lum(t.surface_container_low));
        assert!(lum(t.surface_container_low) >= lum(t.surface_container));
        assert!(lum(t.surface_container) >= lum(t.surface_container_high));
        assert!(lum(t.surface_container_high) >= lum(t.surface_container_highest));
    }

    /// (R57.X.listbox §5.50) Material 3 dark surface tier progression:
    /// `surface` is the darkest tone (panel background) and the four
    /// containers lighten progressively toward
    /// `surface_container_highest` (inverse of light).
    ///
    /// R2018.1 — reads [`crate::contrast::relative_luminance`], for the
    /// reasons on the light pin above.
    #[test]
    fn r57_x_surface_tiers_dark_lightness_progression() {
        let t = Theme::dark();
        let lum = crate::contrast::relative_luminance;
        assert!(lum(t.surface) <= lum(t.surface_container_low));
        assert!(lum(t.surface_container_low) <= lum(t.surface_container));
        assert!(lum(t.surface_container) <= lum(t.surface_container_high));
        assert!(lum(t.surface_container_high) <= lum(t.surface_container_highest));
    }

    /// ★★★★★ R2018.1 §5.50 — **the two ways this tree has ordered tones by
    /// lightness are not interchangeable**, which is what makes picking one a
    /// decision rather than a tidy-up.
    ///
    /// A channel sum weights the three primaries equally; the eye does not,
    /// and green carries most of what it reads as brightness. So a tone can be
    /// the DIMMER of two by `r + g + b` and the LIGHTER of the two to look at.
    /// This drives exactly that pair, so the two pins above cannot be said to
    /// have kept their meaning by luck.
    ///
    /// The second half is the measurement that licensed the change: on the two
    /// palettes this crate owns, both arithmetics order the five surface tiers
    /// identically. It is asserted here rather than written in a comment,
    /// because a palette edit is precisely what would end it — and the day it
    /// does, the recorded fork has come due and the pins are the half that
    /// stays.
    #[test]
    fn r2018_1_a_channel_sum_and_the_eye_can_order_two_tones_differently() {
        let sum = |c: Color| u32::from(c.r) + u32::from(c.g) + u32::from(c.b);
        let lum = crate::contrast::relative_luminance;

        // 730 against 739: dimmer by channel sum, and the green-heavy one is
        // plainly the lighter of the two to look at.
        let green_heavy = Color::rgb(0xE6, 0xFF, 0xF5);
        let even = Color::rgb(0xF7, 0xF2, 0xFA);
        assert!(
            sum(green_heavy) < sum(even),
            "the fixture must be the dimmer of the two by channel sum: {} vs {}",
            sum(green_heavy),
            sum(even)
        );
        assert!(
            lum(green_heavy) > lum(even),
            "and the lighter of the two to the eye: {} vs {}",
            lum(green_heavy),
            lum(even)
        );

        // And on what this crate ships, the two agree — which is why moving
        // the pins to the perceptual one changed no verdict.
        // The population the pins cover: the plain surface and, rather than a
        // second list of the containers, `ELEVATION` itself.
        let tiers: Vec<ColorRole> = std::iter::once(ColorRole::Surface)
            .chain(Theme::ELEVATION)
            .collect();
        for (word, palette) in [("light", Theme::light()), ("dark", Theme::dark())] {
            let mut by_sum = tiers.clone();
            let mut by_lum = tiers.clone();
            by_sum.sort_by_key(|role| sum(palette.resolve(*role)));
            by_lum.sort_by(|a, b| {
                lum(palette.resolve(*a))
                    .partial_cmp(&lum(palette.resolve(*b)))
                    .expect("palette colours are ordinary numbers")
            });
            assert_eq!(
                by_sum, by_lum,
                "{word}: the two arithmetics order this palette's surface tiers differently"
            );
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
    // R595 — ColorRole::all() + name() introspection surface
    // ─────────────────────────────────────────────────────────────

    /// (R595 §5.50) `ColorRole::all()` must enumerate every variant
    /// declared on the enum. Without this pin, a future Tier 2
    /// addition that forgets to extend the slice would silently
    /// truncate every introspection consumer's role list.
    ///
    /// The exhaustive match below produces a compiler error rather
    /// than a test failure when a new variant lands without slice
    /// coverage — exactly the developer experience the helper exists
    /// to provide.
    #[test]
    fn r595_all_enumerates_every_variant() {
        // Touch every variant in a match — adding a Tier 2 variant
        // requires extending both this arm and `ColorRole::all`, so
        // the compiler enforces the pairing.
        for role in ColorRole::all() {
            let _: () = match role {
                ColorRole::Surface
                | ColorRole::OnSurface
                | ColorRole::OnSurfaceMuted
                | ColorRole::Accent
                | ColorRole::OnAccent
                | ColorRole::Outline
                | ColorRole::SurfaceContainerHighest
                | ColorRole::SurfaceContainerLow
                | ColorRole::SurfaceContainer
                | ColorRole::SurfaceContainerHigh
                | ColorRole::Error
                | ColorRole::OnError
                | ColorRole::ErrorContainer
                | ColorRole::OnErrorContainer
                | ColorRole::InverseSurface
                | ColorRole::InverseOnSurface
                | ColorRole::InversePrimary
                | ColorRole::Warning
                | ColorRole::OnWarning
                | ColorRole::Success
                | ColorRole::OnSuccess
                | ColorRole::Info
                | ColorRole::OnInfo => (),
            };
        }
        // Variant count = Tier 1 + R590 error tier + R723 inverse tier
        // + R1651 warning pair + R2012 success and informational pairs (23).
        assert_eq!(ColorRole::all().len(), 23);
        // No duplicates — pure-set semantics.
        let mut names: Vec<_> = ColorRole::all().iter().map(|r| r.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 23, "names must be unique");
    }

    /// ★★★★★ R2016 §5.50 — **a palette authored elsewhere is taken, or every
    /// role it is short of is named at once.**
    ///
    /// A design system authored for this project emits a palette in exactly
    /// the shape this struct's own `serde` derive accepts, and had done for
    /// some time before anything here called the parser: the bridge was built
    /// from one side and nobody crossed it. The first thing crossing finds is
    /// that the document is SHORT, because this vocabulary grew two state
    /// tiers after that exporter was written — so the crossing and the naming
    /// of the shortfall are one act.
    ///
    /// ⚠ `serde` alone would not do. It stops at the first missing field and
    /// says only *missing field: info*, which sends a person round the loop
    /// once per role. A palette arrives as a whole document; the useful answer
    /// is the whole shortfall.
    ///
    /// The round trip is asserted first, because a refusal test whose happy
    /// path nobody checks is a parser that might refuse everything.
    #[test]
    fn r2016_an_authored_palette_is_taken_or_says_what_it_owes() {
        // Round trip: this vocabulary's own light palette, written out and
        // read back. Derived from `ColorRole::all` so a new role joins.
        let complete = serde_json::to_string(&Theme::light()).expect("a theme serialises");
        let (taken, unknown) = Theme::take_palette(&complete).expect("a complete palette is taken");
        assert_eq!(taken, Theme::light(), "the round trip must be exact");
        assert!(unknown.is_empty(), "and name nothing this vocabulary lacks");

        // The shortfall, whole. The document below is the shape an authoring
        // tool emits — an object keyed by role name — with the two state tiers
        // this vocabulary added afterwards left out, which is the real state
        // of the bridge as this round found it.
        let short: std::collections::BTreeMap<String, Color> = ColorRole::all()
            .iter()
            .filter(|role| {
                !matches!(
                    role,
                    ColorRole::Success | ColorRole::OnSuccess | ColorRole::Info | ColorRole::OnInfo
                )
            })
            .map(|role| (role.name().to_owned(), Theme::light().resolve(*role)))
            .collect();
        let gap = Theme::from_wire(&serde_json::to_string(&short).expect("serialises"))
            .expect_err("a palette short of four roles is not a palette");
        assert_eq!(
            gap.missing,
            vec![
                ColorRole::Success,
                ColorRole::OnSuccess,
                ColorRole::Info,
                ColorRole::OnInfo
            ],
            "all four at once, in declaration order — not the first one",
        );
        assert!(
            gap.unknown.is_empty(),
            "and it binds nothing this vocabulary does not have: {:?}",
            gap.unknown
        );
        // The refusal is a sentence, because it is what a person reads.
        let said = gap.to_string();
        for role in &gap.missing {
            assert!(
                said.contains(role.name()),
                "the refusal must name `{}`: {said}",
                role.name()
            );
        }

        // A key naming no role is REPORTED, not refused: a palette that binds
        // everything this vocabulary has is usable whatever else it carries,
        // and the extra key is usually a role the author has and this does not
        // — the more interesting half, and the one a silent parser discards.
        let mut extra: std::collections::BTreeMap<String, Color> = ColorRole::all()
            .iter()
            .map(|role| (role.name().to_owned(), Theme::light().resolve(*role)))
            .collect();
        extra.insert("tertiary".to_owned(), Color::rgb(1, 2, 3));
        let (_, unknown) = Theme::take_palette(&serde_json::to_string(&extra).expect("serialises"))
            .expect("a complete palette is taken whatever else it carries");
        assert_eq!(unknown, vec!["tertiary".to_owned()]);
        println!("[r2016] refusal reads: {said}");
    }

    /// ★★★★★ R2018 §5.50 — **the elevation ladder runs one way, and an
    /// inverted step is named.**
    ///
    /// The R57.X pins two functions up assert the same thing about these two
    /// palettes and assert it with `r + g + b` on a hand-written chain of
    /// comparisons. This asserts the property that a palette a SCREEN binds can
    /// also be held to — see `elevation_inversions` for why `surface` is not in
    /// it — and it asserts the detector as well as the palettes, because a
    /// function that returns nothing whatever it is given would satisfy the
    /// first half on its own.
    #[test]
    fn r2018_the_elevation_ladder_runs_one_way() {
        for (word, palette) in [("light", Theme::light()), ("dark", Theme::dark())] {
            assert!(
                palette.elevation_inversions().is_empty(),
                "the {word} palette's containers must step one way: {:?}",
                palette.elevation_inversions()
            );
        }
        // The two directions are real and opposite, so the detector cannot be
        // hard-coded to one of them.
        let lum = |c: Color| crate::contrast::relative_luminance(c);
        assert!(
            lum(Theme::light().surface_container_highest)
                < lum(Theme::light().surface_container_low),
            "the light ladder darkens with elevation"
        );
        assert!(
            lum(Theme::dark().surface_container_highest) > lum(Theme::dark().surface_container_low),
            "and the dark one lightens, which is the case a one-sided check misses"
        );

        // The detector's own failing path: one middle tier moved past its
        // neighbour, in each direction. Without this a `Vec::new()` body passes
        // everything above.
        for (word, mut palette) in [("light", Theme::light()), ("dark", Theme::dark())] {
            let swapped = palette.resolve(ColorRole::SurfaceContainerHighest);
            palette.bind(ColorRole::SurfaceContainer, swapped);
            let found = palette.elevation_inversions();
            assert!(
                !found.is_empty(),
                "{word}: a tier moved to the far end of the ladder is an inversion"
            );
            assert!(
                found
                    .iter()
                    .any(|(a, b)| *a == ColorRole::SurfaceContainer
                        || *b == ColorRole::SurfaceContainer),
                "{word}: and the pair reported names the tier that moved: {found:?}"
            );
        }
    }

    /// ★★★★★ R2017 §5.50 — **a difference in ANY channel is a difference**,
    /// and this exists because the round's first gate could not say so.
    ///
    /// The consumer gate over in the analysis shell counts how many roles that
    /// screen's palette departs on and got the right number — but its
    /// counterfactual, comparing only the RED channel, PASSED: every role that
    /// screen overrides happens to differ in red as well, so the count could
    /// not tell a whole-colour comparison from a third of one. A gate whose
    /// mutation survives is measuring something narrower than it claims.
    ///
    /// So the property is asserted where it lives, on data chosen to need it:
    /// one role moved in a single channel, once per channel, each of which a
    /// red-only comparison misses.
    #[test]
    fn r2017_a_difference_in_any_channel_is_reported() {
        let base = Theme::light();
        let role = ColorRole::Surface;
        let from = base.resolve(role);
        for (channel, moved) in [
            ("red", Color::rgba(from.r ^ 1, from.g, from.b, from.a)),
            ("green", Color::rgba(from.r, from.g ^ 1, from.b, from.a)),
            ("blue", Color::rgba(from.r, from.g, from.b ^ 1, from.a)),
            ("alpha", Color::rgba(from.r, from.g, from.b, from.a ^ 1)),
        ] {
            let mut other = base;
            other.bind(role, moved);
            let differences = base.differences(&other);
            assert_eq!(
                differences.len(),
                1,
                "a palette that moved only {channel} differs on exactly one role"
            );
            assert_eq!(differences[0].role, role, "and it is the one that moved");
            assert_eq!(
                (differences[0].mine, differences[0].theirs),
                (from, moved),
                "reported from the asking palette's side"
            );
        }
    }

    /// ★★★★★ R2016 §5.50 — **`bind` and `resolve` are inverses over every
    /// role**, which is what lets `take_palette` place a document without a
    /// per-role list of its own.
    ///
    /// The write half is exhaustive by construction — a role added to the enum
    /// fails to compile until it is placed — but exhaustive is not the same as
    /// CORRECT: a mis-typed arm that writes `on_error` where it means `error`
    /// compiles perfectly. Driving every role with a value nothing else has is
    /// what separates the two.
    #[test]
    fn r2016_binding_a_role_is_the_inverse_of_resolving_it() {
        let mut theme = Theme::light();
        // A distinct colour per role, so a mis-aimed arm cannot coincide.
        for (i, role) in ColorRole::all().iter().enumerate() {
            let n = u8::try_from(i).expect("fewer than 256 roles");
            theme.bind(*role, Color::rgb(n, n, n));
        }
        for (i, role) in ColorRole::all().iter().enumerate() {
            let n = u8::try_from(i).expect("fewer than 256 roles");
            assert_eq!(
                theme.resolve(*role),
                Color::rgb(n, n, n),
                "`{}` resolves to what was bound to it",
                role.name()
            );
        }
    }

    /// (R1651 §5.50) The warning tier reads **apart** from the error
    /// tier in both palettes, which is the entire reason it exists:
    /// `ConfigDefect`'s arms differ in whether they block, and a palette
    /// with one alarm tone can only paint half of that vocabulary.
    #[test]
    fn r1651_warning_is_a_distinct_tone_from_error_in_both_palettes() {
        for palette in [Theme::light(), Theme::dark()] {
            assert_ne!(
                palette.resolve(ColorRole::Warning),
                palette.resolve(ColorRole::Error),
                "a defect that warns must not be painted as one that blocks"
            );
            assert_ne!(
                palette.resolve(ColorRole::Warning),
                palette.resolve(ColorRole::OnSurface),
                "nor as ordinary text"
            );
            assert_ne!(
                palette.resolve(ColorRole::OnWarning),
                palette.resolve(ColorRole::Warning),
                "and its foreground has to be legible on it"
            );
        }
        assert_ne!(
            Theme::light().resolve(ColorRole::Warning),
            Theme::dark().resolve(ColorRole::Warning),
            "the two schemes tone it differently, like every other role"
        );
    }

    /// (R595 §5.50) `ColorRole::name()` returns the canonical
    /// `snake_case` identifier — every value matches the `snake_case`
    /// form of the [`Theme`] field that backs the role under
    /// [`Theme::resolve`]. Pinned so the wire identifier and the
    /// in-process field name stay in sync; a rename of either side
    /// without the other would break RPC introspection consumers
    /// silently.
    #[test]
    fn r595_name_round_trips_with_theme_field_naming() {
        // Spot-check each axis: the surface tier, the elevation
        // family, the accent / on_accent pair, the outline hairline,
        // and the R590 error tier.
        assert_eq!(ColorRole::Surface.name(), "surface");
        assert_eq!(ColorRole::OnSurface.name(), "on_surface");
        assert_eq!(ColorRole::OnSurfaceMuted.name(), "on_surface_muted");
        assert_eq!(ColorRole::Accent.name(), "accent");
        assert_eq!(ColorRole::OnAccent.name(), "on_accent");
        assert_eq!(ColorRole::Outline.name(), "outline");
        assert_eq!(
            ColorRole::SurfaceContainerHighest.name(),
            "surface_container_highest",
        );
        assert_eq!(
            ColorRole::SurfaceContainerLow.name(),
            "surface_container_low",
        );
        assert_eq!(ColorRole::SurfaceContainer.name(), "surface_container");
        assert_eq!(
            ColorRole::SurfaceContainerHigh.name(),
            "surface_container_high",
        );
        assert_eq!(ColorRole::Error.name(), "error");
        assert_eq!(ColorRole::OnError.name(), "on_error");
        assert_eq!(ColorRole::ErrorContainer.name(), "error_container");
        assert_eq!(ColorRole::OnErrorContainer.name(), "on_error_container");
    }

    /// (R608 §5.50) `ColorRole::from_name()` is the inverse of
    /// [`ColorRole::name`] — every variant in [`ColorRole::all`]
    /// round-trips through `name() → from_name()`. Pinned so a future
    /// rename of either side without the other breaks compilation
    /// (via the exhaustive `match` on `name()`) instead of silently
    /// dropping the wire identifier the RPC write path
    /// (`scene/set_theme_palettes`) depends on.
    #[test]
    fn r608_from_name_round_trips_with_name_for_every_variant() {
        for &role in ColorRole::all() {
            assert_eq!(
                ColorRole::from_name(role.name()),
                Some(role),
                "round-trip failed for {role:?}",
            );
        }
    }

    /// (R608 §5.50) `from_name()` rejects strings that are not a
    /// canonical `snake_case` wire identifier — case-folded variant
    /// names, missing underscores, prefix/suffix typos, and the empty
    /// string all surface as `None`. Pinned so the AI-first write path
    /// produces a typed parse error instead of accepting a near-miss
    /// silently.
    #[test]
    fn r608_from_name_rejects_non_canonical_inputs() {
        assert_eq!(ColorRole::from_name(""), None);
        assert_eq!(ColorRole::from_name("Surface"), None, "case-sensitive");
        assert_eq!(
            ColorRole::from_name("onsurface"),
            None,
            "missing underscore"
        );
        assert_eq!(ColorRole::from_name("on-surface"), None, "kebab-case");
        assert_eq!(ColorRole::from_name("surface_typo"), None);
        assert_eq!(ColorRole::from_name(" surface "), None, "whitespace");
    }

    // ─────────────────────────────────────────────────────────────
    // R590 — Material 3 error tier exact-value + role-mapping pins
    // ─────────────────────────────────────────────────────────────

    /// (R590 §5.50) Pin the Material 3 light error tier hex values.
    /// A palette tweak that drifts the error tone breaks the
    /// destructive-signal affordance for every consumer at once;
    /// the exact-value assertion guarantees the family stays on the
    /// M3 baseline until a future round explicitly retones it.
    #[test]
    fn r590_error_tier_light_exact_palette_pins() {
        let t = Theme::light();
        assert_eq!(t.error, Color::rgb(0xb3, 0x26, 0x1e));
        assert_eq!(t.on_error, Color::rgb(0xff, 0xff, 0xff));
        assert_eq!(t.error_container, Color::rgb(0xf9, 0xde, 0xdc));
        assert_eq!(t.on_error_container, Color::rgb(0x41, 0x0e, 0x0b));
    }

    /// (R590 §5.50) Pin the Material 3 dark error tier hex values.
    /// Each tone is lifted toward the upper half of the Error tonal
    /// scale (80 / 20 / 30 / 90) so the red signal stays legible
    /// against `#121212`.
    #[test]
    fn r590_error_tier_dark_exact_palette_pins() {
        let t = Theme::dark();
        assert_eq!(t.error, Color::rgb(0xf2, 0xb8, 0xb5));
        assert_eq!(t.on_error, Color::rgb(0x60, 0x14, 0x10));
        assert_eq!(t.error_container, Color::rgb(0x8c, 0x1d, 0x18));
        assert_eq!(t.on_error_container, Color::rgb(0xf9, 0xde, 0xdc));
    }

    /// (R590 §5.50) `Theme::resolve(ColorRole::Error/...)` returns
    /// the matching palette field on both light and dark presets.
    /// Pins the enum-arm-to-field wiring so a future re-numbering of
    /// the enum cannot silently swap the role-to-field map.
    #[test]
    fn r590_error_tier_resolve_dispatches_to_palette_fields() {
        for palette in [Theme::light(), Theme::dark()] {
            assert_eq!(palette.resolve(ColorRole::Error), palette.error);
            assert_eq!(palette.resolve(ColorRole::OnError), palette.on_error);
            assert_eq!(
                palette.resolve(ColorRole::ErrorContainer),
                palette.error_container,
            );
            assert_eq!(
                palette.resolve(ColorRole::OnErrorContainer),
                palette.on_error_container,
            );
        }
    }

    // ─────────────────────────────────────────────────────────────
    // R592 — R57.0 Effect-rerun substrate (Signal auto-subscribe pin)
    // ─────────────────────────────────────────────────────────────

    /// (R592 §5.50) `ThemeProvider::set_mode` mutates the active
    /// mode signal; an `Effect` that reads `theme()` (or any path
    /// derived from `mode()`) must re-run exactly once per mutation.
    ///
    /// Pins the R57.0 reactivity contract: `theme()` auto-subscribes
    /// the current reactive scope to the mode signal, so the
    /// `set_mode` write reaches every dependent computation through
    /// the same `Signal::set` notification path the rest of the
    /// substrate uses (no special-cased re-run channel).
    ///
    /// Without this regression a future `ThemeProvider` refactor that
    /// drops the signal read inside `theme()` (e.g. caching the
    /// resolved palette on a field) would silently break view-fn
    /// auto-repaint on theme changes.
    #[test]
    fn r592_effect_reruns_on_set_mode() {
        let owner = Owner::new();
        let runs = Rc::new(Cell::new(0u32));
        let runs_inner = Rc::clone(&runs);
        owner.run(|| {
            let provider = use_theme("r592_set_mode");
            // Force a determinate starting mode — System would also
            // subscribe to the global SystemColorScheme signal, which
            // we are not exercising here. Light is the canonical W3C
            // fallback.
            provider.set_mode(ThemeMode::Light);
            // Eager construction runs the closure once + subscribes
            // to whatever signals `theme()` reads.
            let _effect = Effect::new(&owner, move || {
                let _ = use_theme("r592_set_mode").theme();
                runs_inner.set(runs_inner.get() + 1);
            });
            assert_eq!(runs.get(), 1, "Effect::new eager initial run");
            provider.set_mode(ThemeMode::Dark);
            assert_eq!(runs.get(), 2, "set_mode(Dark) re-runs Effect");
            provider.set_mode(ThemeMode::Light);
            assert_eq!(runs.get(), 3, "set_mode(Light) re-runs Effect");
            // Same-value write: Signal::set short-circuits when
            // `new == old` via PartialEq — no re-run.
            provider.set_mode(ThemeMode::Light);
            assert_eq!(runs.get(), 3, "same-value set_mode does not re-run");
        });
    }

    /// (R592 §5.50) `ThemeProvider::set_light_palette` mutates the
    /// light palette signal. While the active mode is `Light`
    /// (resolved palette comes from the light signal), an `Effect`
    /// that reads `theme()` must re-run on every light palette write.
    #[test]
    fn r592_effect_reruns_on_set_light_palette() {
        let owner = Owner::new();
        let runs = Rc::new(Cell::new(0u32));
        let runs_inner = Rc::clone(&runs);
        owner.run(|| {
            let provider = use_theme("r592_set_light");
            provider.set_mode(ThemeMode::Light);
            let _effect = Effect::new(&owner, move || {
                let _ = use_theme("r592_set_light").theme();
                runs_inner.set(runs_inner.get() + 1);
            });
            assert_eq!(runs.get(), 1, "eager initial run");
            // Distinct palette so Signal::set does not short-circuit.
            let mut tweaked = Theme::light();
            tweaked.accent = Color::rgb(0x00, 0xff, 0x00);
            provider.set_light_palette(tweaked);
            assert_eq!(runs.get(), 2, "set_light_palette re-runs Effect");
        });
    }

    /// (R592 §5.50) Mirror of [`r592_effect_reruns_on_set_light_palette`]
    /// for the dark palette signal under `ThemeMode::Dark`.
    #[test]
    fn r592_effect_reruns_on_set_dark_palette() {
        let owner = Owner::new();
        let runs = Rc::new(Cell::new(0u32));
        let runs_inner = Rc::clone(&runs);
        owner.run(|| {
            let provider = use_theme("r592_set_dark");
            provider.set_mode(ThemeMode::Dark);
            let _effect = Effect::new(&owner, move || {
                let _ = use_theme("r592_set_dark").theme();
                runs_inner.set(runs_inner.get() + 1);
            });
            assert_eq!(runs.get(), 1, "eager initial run");
            let mut tweaked = Theme::dark();
            tweaked.accent = Color::rgb(0xff, 0x00, 0x00);
            provider.set_dark_palette(tweaked);
            assert_eq!(runs.get(), 2, "set_dark_palette re-runs Effect");
        });
    }

    /// (R593 §5.50) `ThemeProvider::set_palettes(light, dark)` must
    /// fold the two palette signal writes into a single reactive
    /// batch — subscribers re-run at most once per call even though
    /// two distinct signals mutate. Pins the atomic-batch contract a
    /// future refactor could silently drop, doubling the per-swap
    /// work for every downstream view-fn.
    #[test]
    fn r593_set_palettes_atomic_batches_subscribers() {
        let owner = Owner::new();
        let runs = Rc::new(Cell::new(0u32));
        let runs_inner = Rc::clone(&runs);
        owner.run(|| {
            let provider = use_theme("r593_atomic");
            // Light mode so theme() reads the light palette signal;
            // the dark palette write inside the batch still mutates a
            // distinct signal, so the batch coalescing is what keeps
            // the re-run count at 1.
            provider.set_mode(ThemeMode::Light);
            let _effect = Effect::new(&owner, move || {
                let p = use_theme("r593_atomic");
                let _ = p.light_palette();
                let _ = p.dark_palette();
                runs_inner.set(runs_inner.get() + 1);
            });
            assert_eq!(runs.get(), 1, "eager initial run");
            let mut new_light = Theme::light();
            new_light.accent = Color::rgb(0x00, 0x80, 0x00);
            let mut new_dark = Theme::dark();
            new_dark.accent = Color::rgb(0x80, 0xff, 0x80);
            provider.set_palettes(new_light, new_dark);
            assert_eq!(
                runs.get(),
                2,
                "set_palettes coalesces both signal writes into one re-run",
            );
        });
    }

    /// (R593 §5.50) `set_palettes` is a pure replacement — the new
    /// light + dark palettes round-trip through
    /// [`ThemeProvider::light_palette`] / [`ThemeProvider::dark_palette`]
    /// reads. Pinned so a future implementation that, say, only wrote
    /// the light side cannot regress silently.
    #[test]
    fn r593_set_palettes_round_trips_both_signals() {
        let owner = Owner::new();
        owner.run(|| {
            let provider = use_theme("r593_round_trip");
            let mut light = Theme::light();
            light.surface = Color::rgb(0xee, 0xee, 0xee);
            let mut dark = Theme::dark();
            dark.surface = Color::rgb(0x11, 0x11, 0x11);
            provider.set_palettes(light, dark);
            assert_eq!(provider.light_palette(), light);
            assert_eq!(provider.dark_palette(), dark);
        });
    }

    /// (R592 §5.50) When the active mode is `Light` (or `Dark`), the
    /// global `SystemColorScheme` signal must NOT trigger a re-run —
    /// `theme()` only subscribes to the system signal when the mode is
    /// `System`. Pins the careful auto-subscribe contract: reading the
    /// system signal under the wrong branch would over-subscribe and
    /// spuriously repaint every Light-mode app on every OS theme flip.
    #[test]
    fn r592_effect_in_light_mode_ignores_system_signal() {
        // Isolate the global system signal from sibling tests.
        set_system_color_scheme(SystemColorScheme::NoPreference);
        let owner = Owner::new();
        let runs = Rc::new(Cell::new(0u32));
        let runs_inner = Rc::clone(&runs);
        owner.run(|| {
            let provider = use_theme("r592_no_system_subscribe");
            provider.set_mode(ThemeMode::Light);
            let _effect = Effect::new(&owner, move || {
                let _ = use_theme("r592_no_system_subscribe").theme();
                runs_inner.set(runs_inner.get() + 1);
            });
            assert_eq!(runs.get(), 1, "eager initial run");
            set_system_color_scheme(SystemColorScheme::Dark);
            assert_eq!(
                runs.get(),
                1,
                "Light mode must not subscribe to system signal",
            );
            // Confirm the system signal write actually landed — guards
            // against a future ThemeProvider refactor that drops the
            // write path entirely (would also keep the counter at 1
            // by accident).
            assert_eq!(system_color_scheme(), SystemColorScheme::Dark);
            // Restore so the next sibling test starts clean.
            set_system_color_scheme(SystemColorScheme::NoPreference);
        });
    }

    /// (R590 §5.50) The `ThemeLinear` carrier covers the error tier:
    /// `to_theme(from_theme(t))` round-trips every error field within
    /// the 8-bit sRGB rounding tolerance the existing
    /// `r57_x_theme_linear_round_trip_is_within_8bit_tolerance`
    /// assertion uses for the rest of the palette. Pinning per-field
    /// keeps a missed entry in `from_theme` / `to_theme` from sliding
    /// through.
    #[test]
    fn r590_error_tier_linear_round_trip_preserves_error_fields() {
        for palette in [Theme::light(), Theme::dark()] {
            let round_trip = ThemeLinear::from_theme(palette).to_theme();
            // 8-bit sRGB EOTF round-trip is exact within ±1 channel.
            let close = |a: Color, b: Color| {
                let da = i16::from(a.r) - i16::from(b.r);
                let db = i16::from(a.g) - i16::from(b.g);
                let dc = i16::from(a.b) - i16::from(b.b);
                da.abs() <= 1 && db.abs() <= 1 && dc.abs() <= 1
            };
            assert!(close(round_trip.error, palette.error));
            assert!(close(round_trip.on_error, palette.on_error));
            assert!(close(round_trip.error_container, palette.error_container));
            assert!(close(
                round_trip.on_error_container,
                palette.on_error_container,
            ));
        }
    }

    // ─────────────────────────────────────────────────────────────
    // R57.1 — SystemColorScheme + ThemeMode defaults
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r57_1_system_color_scheme_default_is_no_preference() {
        // W3C `prefers-color-scheme: no-preference` is the spec
        // default — pin it so a future enum reshuffle does not
        // silently flip the application's first-frame appearance.
        assert_eq!(
            SystemColorScheme::default(),
            SystemColorScheme::NoPreference
        );
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
        // propagate to the other. Mirrors the web UI library's `useContext`
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
    /// round-trip through `ThemeLinear` does not flake the
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
        assert_color_close(actual.surface_container_low, expected.surface_container_low);
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
        settle_owner_animations(&owner);
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
        settle_owner_animations(&owner);
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
        settle_owner_animations(&owner);
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
        settle_owner_animations(&owner);
        set_system_color_scheme(SystemColorScheme::Dark);
        owner.run(|| {
            let _ = p.theme_animated();
        });
        settle_owner_animations(&owner);
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
        // (`#E6E0E9`), light outline (`#949494`), and dark outline
        // (`#616161`) would fail intermittently — the lossy round-trip
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
        settle_owner_animations(&owner);
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
