//! Theme-axis RPC method dispatch — §5.50 + §5.7.
//!
//! Houses every `scene/*` method that reads or mutates the
//! application's bound [`ThemeProvider`] state, mirroring the
//! `focus.rs` module's pattern (one module per axis, multiple
//! methods inside).
//!
//! Method ledger:
//!
//! | Method                  | Direction | Round  |
//! |-------------------------|-----------|--------|
//! | `scene/theme_tokens`    | read      | R598   |
//! | `scene/set_theme_mode`  | mutate    | R599   |
//!
//! The mutate side bumps [`SceneRevision`](pinion_core::SceneRevision)
//! on success because a mode flip changes every subscriber's
//! rendered palette (re-paint required).
//!
//! ## `scene/theme_tokens` — read-side
//!
//! Second consumer of [`ColorRole::all`] /
//! [`ColorRole::name`](pinion_core::theme::ColorRole::name) (R595)
//! and of [`DispatchContext::runtime_owner`](crate::DispatchContext)
//! (R597), per the [[abstraction-needs-second-consumer]] discipline —
//! the substrate-side primitives only ratify once a second site
//! reaches for them, and the AI-first §2#2 introspection contract
//! needs the bound palette surfaced through JSON-RPC.
//!
//! ## What the AI agent sees
//!
//! The call snapshots the [`ThemeProvider`] cached on the substrate's
//! root [`Owner`] under the supplied `tag` (default `"app"`, matching
//! the [`THEME_TAG`](pinion_core::theme::use_theme) convention every
//! `examples/hello-*` binary uses) and projects every [`ColorRole`]
//! against both palettes plus the active resolution decision.
//!
//! ## Wire shape
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 1,
//!   "result": {
//!     "tag": "app",
//!     "mode": "system",
//!     "system_scheme": "light",
//!     "active": "light",
//!     "palettes": {
//!       "light": [
//!         { "role": "surface",    "color": "#fefbff" },
//!         { "role": "on_surface", "color": "#1a1a1a" }
//!       ],
//!       "dark": [
//!         { "role": "surface",    "color": "#121212" },
//!         { "role": "on_surface", "color": "#e6e6e6" }
//!       ]
//!     }
//!   }
//! }
//! ```
//!
//! Each `palettes.{light,dark}[*]` array carries every variant of
//! [`ColorRole::all`] in declaration order; `tag` echoes the
//! cache-key the application bound; `mode` is the active
//! [`ThemeMode`]; `system_scheme` is the global
//! [`system_color_scheme`] reading; `active` resolves to whichever
//! `palettes` key the application is currently rendering (the same
//! choice [`ThemeProvider::theme`] makes).
//!
//! ## Why include both palettes
//!
//! AI-first §2#2 + scene-as-data §2#7: an agent should be able to
//! plan a palette swap (e.g. "what would the surface color be in
//! dark mode?") without first calling
//! [`ThemeProvider::set_mode`](pinion_core::theme::ThemeProvider::set_mode)
//! and re-querying. Echoing both palettes keeps the introspection
//! call side-effect-free, matching the dry-run primitive's spirit
//! (§2#3).
//!
//! ## Non-draining + side-effect-free
//!
//! The call neither subscribes the framework's reactive scopes nor
//! mutates any signal. The `Owner::run` wrap is purely an
//! `Owner::current()` resolver for [`use_theme`] — the
//! [`Owner::cache`](pinion_core::reactive::Owner::cache) slot the
//! application already populated is reused; no new reactive
//! computation runs.

use pinion_core::reactive::Owner;
use pinion_core::style::Color;
use pinion_core::theme::{
    system_color_scheme, ColorRole, SystemColorScheme, Theme, ThemeMode, ThemeProvider,
};
use serde::Serialize;

/// Default cache tag the [`use_theme`] hook uses across every
/// `examples/hello-*` binary and the canonical application
/// convention. Used when the JSON-RPC request omits `params.tag`.
pub const DEFAULT_THEME_TAG: &str = "app";

/// Typed errors the [`theme_tokens`] dispatcher can return.
///
/// Each variant maps onto a JSON-RPC `-32602 Invalid params` response
/// at the dispatch layer with the variant name surfaced in
/// `error.data` so AI agents can pattern-match without parsing prose.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeTokensError {
    /// The embedder did not register a
    /// [`runtime_owner`](crate::DispatchContext) on the dispatch
    /// context. Without the substrate's root [`Owner`] there is no
    /// reactive scope to consult for the cached
    /// [`ThemeProvider`].
    RuntimeOwnerUnavailable,
    /// The runtime owner is bound but no [`ThemeProvider`] has been
    /// cached under `tag` yet — typically because the application's
    /// first view-fn run has not happened, or the application uses a
    /// non-default tag and the client did not supply it. Carries the
    /// tag the lookup tried so the agent can retry with the correct
    /// value.
    NotBound { tag: String },
}

/// Per-role projection inside `palettes.{light,dark}`. Each entry
/// pairs the canonical [`ColorRole::name`] wire identifier with the
/// `#rrggbbaa` hex string from the palette field. The alpha byte is
/// included only when it is not fully opaque (the common case for
/// every palette in [`Theme::light`] / [`Theme::dark`]) so the human
/// reading of the JSON stays the `#rrggbb` shape Material 3 / W3C
/// stylesheets ship.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ThemeTokenView {
    /// [`ColorRole::name`] `snake_case` wire identifier.
    pub role: String,
    /// Hex string — `#rrggbb` when alpha is `0xff`, otherwise
    /// `#rrggbbaa`. Matches the CSS Color Module Level 4 form so
    /// downstream tooling (web inspector / IDE color preview / AI
    /// agent diff) parses without a custom decoder.
    pub color: String,
}

/// One palette's view — sequence of [`ThemeTokenView`] entries in
/// [`ColorRole::all`] declaration order.
pub type PaletteTokens = Vec<ThemeTokenView>;

/// Both palettes side-by-side. Keyed by `"light"` / `"dark"` so the
/// `active` field in the outcome can be used as a direct lookup
/// into this map without a separate enum-name decoder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PaletteCatalogue {
    /// Tokens resolved against [`ThemeProvider::light_palette`].
    pub light: PaletteTokens,
    /// Tokens resolved against [`ThemeProvider::dark_palette`].
    pub dark: PaletteTokens,
}

/// Snapshot of the bound [`ThemeProvider`]'s state and projected
/// token catalogues. The shape mirrors the JSON wire shape in this
/// module's documentation one-to-one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ThemeTokensOutcome {
    /// Cache tag the lookup resolved against (echoes the request's
    /// `params.tag`, or [`DEFAULT_THEME_TAG`] when the request
    /// omitted it).
    pub tag: String,
    /// [`ThemeProvider::mode`] at snapshot time — `"light"`,
    /// `"dark"`, or `"system"`.
    pub mode: String,
    /// Global [`system_color_scheme`] reading at snapshot time —
    /// `"light"`, `"dark"`, or `"no_preference"`. The OS hint;
    /// independent of `mode` because the application may pin
    /// `Light` / `Dark` and ignore the OS.
    pub system_scheme: String,
    /// Which `palettes` key the application is currently rendering
    /// — `"light"` or `"dark"`. Equal to the resolution
    /// [`ThemeProvider::theme`] would return: `Light` mode → `"light"`,
    /// `Dark` mode → `"dark"`, `System` mode → `system_scheme` (with
    /// the W3C `no_preference` → `"light"` fallback).
    pub active: String,
    /// Both palettes' role-to-color projections.
    pub palettes: PaletteCatalogue,
}

/// Snapshot the [`ThemeProvider`] cached at `tag` on `runtime_owner`
/// and project its state into a [`ThemeTokensOutcome`].
///
/// Pass `tag = None` (or the JSON-RPC client passes a request with no
/// `params.tag`) to resolve against [`DEFAULT_THEME_TAG`].
///
/// # Errors
///
/// - [`ThemeTokensError::RuntimeOwnerUnavailable`] — `runtime_owner`
///   not registered on the dispatch context.
/// - [`ThemeTokensError::NotBound`] — the owner has no
///   [`ThemeProvider`] cached under `tag`. The application typically
///   binds it on the first view-fn run via [`use_theme`].
///
/// # Side effects
///
/// None. The call wraps an [`Owner::run`] scope only so the
/// [`use_theme`] hook can resolve [`Owner::current`]; no new
/// [`Owner::cache`] entry is inserted because the
/// [`Owner::cache_contains`] gate above this call returns early
/// when the slot is empty. No signal is read inside an active
/// reactive computation (the `Owner::run` body does not establish a
/// tracker), so the call neither subscribes the framework nor
/// schedules any re-paint.
pub fn theme_tokens(
    runtime_owner: Option<&Owner>,
    tag: Option<&str>,
) -> Result<ThemeTokensOutcome, ThemeTokensError> {
    let Some(owner) = runtime_owner else {
        return Err(ThemeTokensError::RuntimeOwnerUnavailable);
    };
    let resolved_tag = tag.unwrap_or(DEFAULT_THEME_TAG);
    // R605 §5.22 — non-leaking lookup. `Owner::cache_get_by_str`
    // accepts an arbitrarily-scoped `&str` and walks the cache for
    // a `(TypeId, key)` match without requiring `&'static str`
    // (which would force `Box::leak` and grow an unbounded process
    // leak proportional to unique JSON-RPC tags).
    let provider: std::rc::Rc<ThemeProvider> = owner
        .cache_get_by_str::<ThemeProvider>(resolved_tag)
        .ok_or_else(|| ThemeTokensError::NotBound {
            tag: resolved_tag.to_string(),
        })?;
    let mode = provider.mode();
    let system_scheme = system_color_scheme();
    let active = active_palette_key(mode, system_scheme);
    let light_palette = provider.light_palette();
    let dark_palette = provider.dark_palette();
    Ok(ThemeTokensOutcome {
        tag: resolved_tag.to_string(),
        mode: mode_name(mode).to_string(),
        system_scheme: system_scheme_name(system_scheme).to_string(),
        active: active.to_string(),
        palettes: PaletteCatalogue {
            light: project_palette(&light_palette),
            dark: project_palette(&dark_palette),
        },
    })
}

/// `"light"` / `"dark"` lookup key matching the resolution
/// [`ThemeProvider::theme`] performs: `Light` / `Dark` short-circuit;
/// `System` defers to the OS [`SystemColorScheme`] with the W3C
/// `no_preference` → light fallback.
///
/// Both [`ThemeMode`] and [`SystemColorScheme`] are
/// `#[non_exhaustive]`; the wildcard arms fall back to `"light"`
/// (the W3C `prefers-color-scheme: no-preference` default) so the
/// resolution never panics if pinion-core grows a new variant.
/// A future variant should land an explicit arm here in the same
/// commit that grows the enum — the existing R598 tests pin every
/// current variant so the addition surfaces as a missing wire id
/// (`"unknown"` in [`mode_name`] / [`system_scheme_name`]).
#[allow(
    clippy::match_same_arms,
    reason = "the explicit arms enumerate every current ThemeMode \
              / SystemColorScheme variant so a future variant \
              addition surfaces as a documentation+test signal; \
              the `_` arm only defends the W3C `no-preference` → \
              light fallback for the unknown-variant case. \
              Merging the explicit arm into the wildcard would erase \
              the per-variant intent the resolution is built to \
              express."
)]
fn active_palette_key(mode: ThemeMode, scheme: SystemColorScheme) -> &'static str {
    match mode {
        ThemeMode::Light => "light",
        ThemeMode::Dark => "dark",
        ThemeMode::System => match scheme {
            SystemColorScheme::Dark => "dark",
            SystemColorScheme::Light | SystemColorScheme::NoPreference => "light",
            _ => "light",
        },
        _ => "light",
    }
}

/// `snake_case` wire identifier for [`ThemeMode`]. Mirrors the
/// [`ColorRole::name`] convention so the JSON consumer can rely on a
/// single naming style across the entire surface. Future
/// `#[non_exhaustive]` variants render as `"unknown"` until the
/// match grows — the corresponding round should land both the new
/// enum variant in pinion-core and the new arm here.
fn mode_name(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::Light => "light",
        ThemeMode::Dark => "dark",
        ThemeMode::System => "system",
        _ => "unknown",
    }
}

/// `snake_case` wire identifier for [`SystemColorScheme`]. Same
/// rationale as [`mode_name`]; the `no_preference` slug mirrors the
/// W3C `prefers-color-scheme: no-preference` media query.
fn system_scheme_name(scheme: SystemColorScheme) -> &'static str {
    match scheme {
        SystemColorScheme::Light => "light",
        SystemColorScheme::Dark => "dark",
        SystemColorScheme::NoPreference => "no_preference",
        _ => "unknown",
    }
}

/// Walk [`ColorRole::all`] and pair each variant with its
/// [`Theme::resolve`] value rendered as a hex string.
fn project_palette(theme: &Theme) -> PaletteTokens {
    ColorRole::all()
        .iter()
        .map(|role| ThemeTokenView {
            role: role.name().to_string(),
            color: color_to_hex(theme.resolve(*role)),
        })
        .collect()
}

/// `#rrggbb` when `color.a == 0xff`; `#rrggbbaa` otherwise. Matches
/// the CSS Color Module Level 4 form.
fn color_to_hex(color: Color) -> String {
    if color.a == 0xff {
        format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
    } else {
        format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            color.r, color.g, color.b, color.a,
        )
    }
}

// ────────────────────────────────────────────────────────────────────
// scene/set_theme_mode — mutate-side (R599)
// ────────────────────────────────────────────────────────────────────

/// Typed request payload for [`set_theme_mode`]. Carries the requested
/// [`ThemeMode`] (wire `mode = "light" | "dark" | "system"`) plus the
/// cache tag the mutation applies to (defaults to
/// [`DEFAULT_THEME_TAG`] when [`None`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetThemeModeParams {
    /// Target [`ThemeMode`] resolved from the wire `mode` string.
    pub mode: ThemeMode,
    /// Cache tag the [`use_theme`] lookup resolves against. [`None`]
    /// → [`DEFAULT_THEME_TAG`] (`"app"`).
    pub tag: Option<String>,
}

/// Snapshot returned to the caller after [`set_theme_mode`] commits
/// the requested mode. Echoes the post-mutation state so a follow-up
/// [`theme_tokens`] call is unnecessary when the client only needs
/// confirmation.
///
/// `active` mirrors [`ThemeTokensOutcome::active`] — the same
/// resolution [`ThemeProvider::theme`] would perform under the new
/// `mode` and the current [`system_color_scheme`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetThemeModeOutcome {
    /// Cache tag the mutation applied to.
    pub tag: String,
    /// Post-mutation [`ThemeMode`] as the canonical `snake_case` wire
    /// identifier — round-trips with the request's `params.mode`.
    pub mode: String,
    /// `"light"` / `"dark"` — which palette the application is now
    /// rendering under the new mode + current OS scheme.
    pub active: String,
}

/// Typed errors the [`set_theme_mode`] dispatcher can return.
///
/// Mirrors the [`ThemeTokensError`] shape — every variant maps onto
/// JSON-RPC `-32602 Invalid params` at the dispatch layer with the
/// variant name surfaced in `error.data`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetThemeModeError {
    /// No [`runtime_owner`](crate::DispatchContext) registered on the
    /// dispatch context — see [`ThemeTokensError::RuntimeOwnerUnavailable`].
    RuntimeOwnerUnavailable,
    /// The owner is bound but no [`ThemeProvider`] is cached under
    /// `tag` yet — see [`ThemeTokensError::NotBound`].
    NotBound { tag: String },
}

/// Mutate the bound [`ThemeProvider`]'s active [`ThemeMode`] under
/// `params.tag` (default [`DEFAULT_THEME_TAG`]) and return the
/// post-mutation snapshot.
///
/// # Side effects
///
/// Calls [`ThemeProvider::set_mode`], which writes a
/// [`Signal`](pinion_core::reactive::Signal) — every subscriber to
/// [`ThemeProvider::theme`] (typically every view-fn that reads a
/// palette role) is scheduled for re-run on the next reactive tick.
/// The dispatcher bumps [`SceneRevision`](pinion_core::SceneRevision)
/// after this call returns `Ok` so an in-flight preview's
/// `base_revision` can detect the concurrent mutation at apply time.
///
/// # Errors
///
/// - [`SetThemeModeError::RuntimeOwnerUnavailable`] — context has no
///   substrate root [`Owner`].
/// - [`SetThemeModeError::NotBound`] — no [`ThemeProvider`] is
///   cached under `tag` yet (the application's first view-fn run has
///   not happened, or the client used a non-default tag and the
///   application's `use_theme(_)` call has not run for it).
pub fn set_theme_mode(
    runtime_owner: Option<&Owner>,
    params: &SetThemeModeParams,
) -> Result<SetThemeModeOutcome, SetThemeModeError> {
    let Some(owner) = runtime_owner else {
        return Err(SetThemeModeError::RuntimeOwnerUnavailable);
    };
    let resolved_tag: &str = params.tag.as_deref().unwrap_or(DEFAULT_THEME_TAG);
    // R605 §5.22 — non-leaking lookup; see `theme_tokens` for the
    // rationale.
    let provider: std::rc::Rc<ThemeProvider> = owner
        .cache_get_by_str::<ThemeProvider>(resolved_tag)
        .ok_or_else(|| SetThemeModeError::NotBound {
            tag: resolved_tag.to_string(),
        })?;
    provider.set_mode(params.mode);
    let system_scheme = system_color_scheme();
    let active = active_palette_key(params.mode, system_scheme);
    Ok(SetThemeModeOutcome {
        tag: resolved_tag.to_string(),
        mode: mode_name(params.mode).to_string(),
        active: active.to_string(),
    })
}

/// Parse a wire `mode` string into a [`ThemeMode`] variant. Returns
/// [`None`] on an unknown slug so the dispatcher can surface a
/// typed invalid-params error.
#[must_use]
pub fn parse_theme_mode(wire: &str) -> Option<ThemeMode> {
    match wire {
        "light" => Some(ThemeMode::Light),
        "dark" => Some(ThemeMode::Dark),
        "system" => Some(ThemeMode::System),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::theme::{set_system_color_scheme, use_theme, Theme, ThemeMode};

    /// Snapshot the global `system_color_scheme` for the duration of a
    /// test. Drop-time restore so an early panic leaves the global
    /// pristine for the next test. Tests that mutate the global wrap
    /// it in this guard to keep the shared thread-local consistent.
    struct SystemSchemeGuard {
        original: SystemColorScheme,
    }

    impl SystemSchemeGuard {
        fn pinned_to(scheme: SystemColorScheme) -> Self {
            let original = system_color_scheme();
            set_system_color_scheme(scheme);
            Self { original }
        }
    }

    impl Drop for SystemSchemeGuard {
        fn drop(&mut self) {
            set_system_color_scheme(self.original);
        }
    }

    fn bind_provider(owner: &Owner, tag: &'static str) -> std::rc::Rc<ThemeProvider> {
        owner.run(|| use_theme(tag))
    }

    // ─────────────────────────────────────────────────────────────────
    // Failure modes
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r598_missing_runtime_owner_errors() {
        let err = theme_tokens(None, None).unwrap_err();
        assert_eq!(err, ThemeTokensError::RuntimeOwnerUnavailable);
    }

    #[test]
    fn r598_unbound_tag_errors_with_tag_echoed() {
        let owner = Owner::new();
        let err = theme_tokens(Some(&owner), Some("unbound-tag")).unwrap_err();
        assert_eq!(
            err,
            ThemeTokensError::NotBound {
                tag: "unbound-tag".into(),
            },
        );
    }

    #[test]
    fn r598_default_tag_unbound_errors_with_default_tag() {
        let owner = Owner::new();
        let err = theme_tokens(Some(&owner), None).unwrap_err();
        assert_eq!(
            err,
            ThemeTokensError::NotBound {
                tag: DEFAULT_THEME_TAG.into(),
            },
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // Happy path: shape + role catalogue completeness
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r598_bound_provider_returns_both_palettes_with_every_role() {
        let owner = Owner::new();
        let _provider = bind_provider(&owner, "app");
        let outcome = theme_tokens(Some(&owner), None).unwrap();
        // Every variant in `ColorRole::all` appears once in each
        // palette, in declaration order, with snake_case names.
        let expected_roles: Vec<String> = ColorRole::all()
            .iter()
            .map(|r| r.name().to_string())
            .collect();
        let light_roles: Vec<String> = outcome
            .palettes
            .light
            .iter()
            .map(|t| t.role.clone())
            .collect();
        let dark_roles: Vec<String> = outcome
            .palettes
            .dark
            .iter()
            .map(|t| t.role.clone())
            .collect();
        assert_eq!(light_roles, expected_roles);
        assert_eq!(dark_roles, expected_roles);
        assert_eq!(outcome.palettes.light.len(), ColorRole::all().len());
        assert_eq!(outcome.palettes.dark.len(), ColorRole::all().len());
    }

    #[test]
    fn r598_palettes_carry_canonical_light_and_dark_field_colors() {
        let owner = Owner::new();
        let _provider = bind_provider(&owner, "app");
        let outcome = theme_tokens(Some(&owner), None).unwrap();
        // Light surface = Theme::light().surface — pin one canonical
        // role per palette to catch a mis-ordered project_palette.
        assert_eq!(
            outcome.palettes.light[0].role, "surface",
            "ColorRole::all order = Surface first",
        );
        assert_eq!(
            outcome.palettes.light[0].color,
            color_to_hex(Theme::light().surface),
        );
        assert_eq!(
            outcome.palettes.dark[0].color,
            color_to_hex(Theme::dark().surface),
        );
    }

    #[test]
    fn r598_tag_echoes_request_value() {
        let owner = Owner::new();
        let _provider = bind_provider(&owner, "custom");
        let outcome = theme_tokens(Some(&owner), Some("custom")).unwrap();
        assert_eq!(outcome.tag, "custom");
    }

    // ─────────────────────────────────────────────────────────────────
    // Mode + system_scheme + active resolution
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r598_mode_light_resolves_active_to_light() {
        let owner = Owner::new();
        let provider = bind_provider(&owner, "app");
        provider.set_mode(ThemeMode::Light);
        let outcome = theme_tokens(Some(&owner), None).unwrap();
        assert_eq!(outcome.mode, "light");
        assert_eq!(outcome.active, "light");
    }

    #[test]
    fn r598_mode_dark_resolves_active_to_dark() {
        let owner = Owner::new();
        let provider = bind_provider(&owner, "app");
        provider.set_mode(ThemeMode::Dark);
        let outcome = theme_tokens(Some(&owner), None).unwrap();
        assert_eq!(outcome.mode, "dark");
        assert_eq!(outcome.active, "dark");
    }

    #[test]
    fn r598_mode_system_with_light_os_resolves_active_to_light() {
        let _guard = SystemSchemeGuard::pinned_to(SystemColorScheme::Light);
        let owner = Owner::new();
        let provider = bind_provider(&owner, "app");
        provider.set_mode(ThemeMode::System);
        let outcome = theme_tokens(Some(&owner), None).unwrap();
        assert_eq!(outcome.mode, "system");
        assert_eq!(outcome.system_scheme, "light");
        assert_eq!(outcome.active, "light");
    }

    #[test]
    fn r598_mode_system_with_dark_os_resolves_active_to_dark() {
        let _guard = SystemSchemeGuard::pinned_to(SystemColorScheme::Dark);
        let owner = Owner::new();
        let provider = bind_provider(&owner, "app");
        provider.set_mode(ThemeMode::System);
        let outcome = theme_tokens(Some(&owner), None).unwrap();
        assert_eq!(outcome.system_scheme, "dark");
        assert_eq!(outcome.active, "dark");
    }

    #[test]
    fn r598_mode_system_with_no_preference_falls_back_to_light() {
        // W3C `prefers-color-scheme: no-preference` → light per
        // ThemeProvider::theme convention; mirror it in `active`.
        let _guard = SystemSchemeGuard::pinned_to(SystemColorScheme::NoPreference);
        let owner = Owner::new();
        let provider = bind_provider(&owner, "app");
        provider.set_mode(ThemeMode::System);
        let outcome = theme_tokens(Some(&owner), None).unwrap();
        assert_eq!(outcome.system_scheme, "no_preference");
        assert_eq!(outcome.active, "light");
    }

    // ─────────────────────────────────────────────────────────────────
    // Hex encoding shape
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r598_color_to_hex_renders_opaque_as_six_digit() {
        assert_eq!(color_to_hex(Color::rgb(0xff, 0xfb, 0xff)), "#fffbff");
        assert_eq!(color_to_hex(Color::rgb(0x12, 0x12, 0x12)), "#121212");
    }

    #[test]
    fn r598_color_to_hex_renders_translucent_with_alpha_byte() {
        assert_eq!(
            color_to_hex(Color::rgba(0x10, 0x20, 0x30, 0x80)),
            "#10203080",
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // Side-effect contract: the call must not subscribe or mutate
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r598_call_does_not_insert_a_new_cache_slot() {
        // Pre-binding the provider populates exactly one cache slot.
        // The theme_tokens call must not insert a second one — the
        // `cache_contains` gate routes the no-slot case to NotBound.
        let owner = Owner::new();
        let _provider = bind_provider(&owner, "app");
        let _outcome = theme_tokens(Some(&owner), None).unwrap();
        let _outcome2 = theme_tokens(Some(&owner), None).unwrap();
        // Indirect probe: a second call against an unbound tag still
        // errors NotBound, proving the prior calls did not create a
        // slot for that tag.
        let err = theme_tokens(Some(&owner), Some("never-bound")).unwrap_err();
        assert!(matches!(err, ThemeTokensError::NotBound { .. }));
    }

    #[test]
    fn r598_call_is_idempotent_two_consecutive_snapshots_match() {
        let owner = Owner::new();
        let _provider = bind_provider(&owner, "app");
        let a = theme_tokens(Some(&owner), None).unwrap();
        let b = theme_tokens(Some(&owner), None).unwrap();
        assert_eq!(a, b);
    }

    // ─────────────────────────────────────────────────────────────────
    // Outcome serialization shape (JSON wire pin)
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r598_outcome_serializes_to_expected_keys() {
        let owner = Owner::new();
        let provider = bind_provider(&owner, "app");
        provider.set_mode(ThemeMode::Light);
        let outcome = theme_tokens(Some(&owner), None).unwrap();
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["tag"], "app");
        assert_eq!(json["mode"], "light");
        assert_eq!(json["active"], "light");
        assert!(json["palettes"]["light"].is_array());
        assert!(json["palettes"]["dark"].is_array());
        assert_eq!(json["palettes"]["light"][0]["role"], "surface");
        // Hex string shape: leading '#' + 6 hex digits when opaque.
        let color_str = json["palettes"]["light"][0]["color"]
            .as_str()
            .expect("color is a string");
        assert!(color_str.starts_with('#'));
        assert_eq!(color_str.len(), 7);
    }

    // ─────────────────────────────────────────────────────────────────
    // R599 §5.50 — set_theme_mode setter
    // ─────────────────────────────────────────────────────────────────

    fn params_with_mode(mode: ThemeMode) -> SetThemeModeParams {
        SetThemeModeParams { mode, tag: None }
    }

    #[test]
    fn r599_set_theme_mode_missing_runtime_owner_errors() {
        let params = params_with_mode(ThemeMode::Dark);
        let err = set_theme_mode(None, &params).unwrap_err();
        assert_eq!(err, SetThemeModeError::RuntimeOwnerUnavailable);
    }

    #[test]
    fn r599_set_theme_mode_unbound_tag_errors_with_tag_echoed() {
        let owner = Owner::new();
        let params = SetThemeModeParams {
            mode: ThemeMode::Dark,
            tag: Some("ghost".into()),
        };
        let err = set_theme_mode(Some(&owner), &params).unwrap_err();
        assert_eq!(
            err,
            SetThemeModeError::NotBound {
                tag: "ghost".into()
            },
        );
    }

    #[test]
    fn r599_set_theme_mode_flips_mode_and_echoes_post_state() {
        let owner = Owner::new();
        let provider = bind_provider(&owner, "app");
        provider.set_mode(ThemeMode::Light);
        let outcome = set_theme_mode(Some(&owner), &params_with_mode(ThemeMode::Dark)).unwrap();
        assert_eq!(outcome.mode, "dark");
        assert_eq!(outcome.active, "dark");
        assert_eq!(outcome.tag, "app");
        // Provider's own mode reflects the mutation immediately.
        assert_eq!(provider.mode(), ThemeMode::Dark);
    }

    #[test]
    fn r599_set_theme_mode_system_with_dark_os_resolves_active_to_dark() {
        let _guard = SystemSchemeGuard::pinned_to(SystemColorScheme::Dark);
        let owner = Owner::new();
        let provider = bind_provider(&owner, "app");
        provider.set_mode(ThemeMode::Light);
        let outcome =
            set_theme_mode(Some(&owner), &params_with_mode(ThemeMode::System)).unwrap();
        assert_eq!(outcome.mode, "system");
        assert_eq!(outcome.active, "dark");
    }

    #[test]
    fn r599_set_theme_mode_is_idempotent_when_same_mode() {
        // Setting mode to its current value is a valid call — the
        // Signal::set equality-skip short-circuits, but the outcome
        // is still the post-state echo.
        let owner = Owner::new();
        let provider = bind_provider(&owner, "app");
        provider.set_mode(ThemeMode::Dark);
        let outcome =
            set_theme_mode(Some(&owner), &params_with_mode(ThemeMode::Dark)).unwrap();
        assert_eq!(outcome.mode, "dark");
        assert_eq!(provider.mode(), ThemeMode::Dark);
    }

    #[test]
    fn r599_set_theme_mode_custom_tag_round_trips() {
        let owner = Owner::new();
        let _provider = bind_provider(&owner, "studio");
        let outcome = set_theme_mode(
            Some(&owner),
            &SetThemeModeParams {
                mode: ThemeMode::Light,
                tag: Some("studio".into()),
            },
        )
        .unwrap();
        assert_eq!(outcome.tag, "studio");
        assert_eq!(outcome.mode, "light");
    }

    #[test]
    fn r599_set_theme_mode_does_not_insert_a_new_cache_slot() {
        // Same side-effect contract as theme_tokens — the call uses
        // cache_contains() to gate before owner.run(use_theme(...)),
        // so an unbound tag does not silently materialize a provider.
        let owner = Owner::new();
        let _ = set_theme_mode(Some(&owner), &params_with_mode(ThemeMode::Dark)).unwrap_err();
        assert!(
            !owner.cache_contains::<ThemeProvider>("app"),
            "set_theme_mode must not materialize a ThemeProvider on a failed lookup",
        );
    }

    #[test]
    fn r599_parse_theme_mode_accepts_three_canonical_slugs() {
        assert_eq!(parse_theme_mode("light"), Some(ThemeMode::Light));
        assert_eq!(parse_theme_mode("dark"), Some(ThemeMode::Dark));
        assert_eq!(parse_theme_mode("system"), Some(ThemeMode::System));
    }

    #[test]
    fn r599_parse_theme_mode_rejects_unknown_slug() {
        assert_eq!(parse_theme_mode("AUTO"), None);
        assert_eq!(parse_theme_mode(""), None);
        assert_eq!(parse_theme_mode("Light"), None, "case-sensitive");
    }

    #[test]
    fn r599_set_theme_mode_outcome_serializes_to_expected_keys() {
        let owner = Owner::new();
        let _provider = bind_provider(&owner, "app");
        let outcome = set_theme_mode(Some(&owner), &params_with_mode(ThemeMode::Light)).unwrap();
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["tag"], "app");
        assert_eq!(json["mode"], "light");
        assert_eq!(json["active"], "light");
        // Wire response is the 3-field outcome shape — no palettes
        // field (clients call scene/theme_tokens for that).
        let obj = json.as_object().expect("outcome is a JSON object");
        let mut keys: Vec<&String> = obj.keys().collect();
        keys.sort();
        let key_strs: Vec<&str> = keys.iter().map(|s| s.as_str()).collect();
        assert_eq!(key_strs, vec!["active", "mode", "tag"]);
    }
}
