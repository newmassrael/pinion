//! Theme-axis RPC method dispatch — §5.50 + §5.7.
//!
//! Houses every `scene/*` method that reads or mutates the
//! application's bound [`ThemeProvider`] state, mirroring the
//! `focus.rs` module's pattern (one module per axis, multiple
//! methods inside).
//!
//! Method ledger:
//!
//! | Method                      | Direction | Round  |
//! |-----------------------------|-----------|--------|
//! | `scene/theme_tokens`        | read      | R598   |
//! | `scene/set_theme_mode`      | mutate    | R599   |
//! | `scene/set_theme_palettes`  | mutate    | R608   |
//!
//! Each mutate-side method bumps [`SceneRevision`](pinion_core::SceneRevision)
//! on success because the write changes every subscriber's rendered
//! palette (re-paint required) — see [`crate::DispatchContext`] for
//! the OCC token contract.
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
//! `Owner::current()` resolver for [`use_theme`](pinion_core::theme::use_theme) — the
//! [`Owner::cache`](pinion_core::reactive::Owner::cache) slot the
//! application already populated is reused; no new reactive
//! computation runs.

use pinion_core::reactive::Owner;
use pinion_core::style::Color;
use pinion_core::theme::{
    ColorRole, SystemColorScheme, Theme, ThemeMode, ThemeProvider, system_color_scheme,
};
use serde::Serialize;

/// Default cache tag the [`use_theme`](pinion_core::theme::use_theme) hook uses across every
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
///   binds it on the first view-fn run via [`use_theme`](pinion_core::theme::use_theme).
///
/// # Side effects
///
/// None. The call wraps an [`Owner::run`] scope only so the
/// [`use_theme`](pinion_core::theme::use_theme) hook can resolve [`Owner::current`]; no new
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
        mode: mode.name().to_string(),
        system_scheme: system_scheme.name().to_string(),
        active: active.to_string(),
        palettes: PaletteCatalogue {
            light: project_palette(&light_palette),
            dark: project_palette(&dark_palette),
        },
    })
}

/// `"light"` / `"dark"` lookup key matching the resolution
/// [`ThemeProvider::theme`] performs: `Light` short-circuits to
/// `"light"`, `Dark` to `"dark"`, `System` defers to the OS
/// [`SystemColorScheme`] with the W3C `no_preference` → light
/// fallback.
///
/// Post-R606 the [`ThemeMode`] → wire-id and [`SystemColorScheme`]
/// → wire-id mappings live on the substrate's exhaustive
/// [`ThemeMode::name`] / [`SystemColorScheme::name`] (R606 §5.50)
/// — see `outcome.mode` / `outcome.system_scheme` serialization.
/// This helper resolves the *active palette key* (a separate
/// concept from wire-id naming) and intentionally folds every
/// non-Dark variant onto `"light"` per the W3C
/// `prefers-color-scheme: no-preference` fallback convention,
/// including future `#[non_exhaustive]` additions.
fn active_palette_key(mode: ThemeMode, scheme: SystemColorScheme) -> &'static str {
    match mode {
        ThemeMode::Dark => "dark",
        ThemeMode::System => match scheme {
            SystemColorScheme::Dark => "dark",
            // Light / NoPreference / future scheme variants →
            // W3C `prefers-color-scheme: no-preference` fallback.
            _ => "light",
        },
        // Light + future ThemeMode variants → light palette per the
        // same W3C fallback convention applied at the mode level.
        _ => "light",
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

/// R615 §5.50 — wire-side delegate to [`Color::to_hex`] (substrate
/// primitive). Pre-R615 the encoder lived RPC-side; R615 lifted both
/// reader + writer to `pinion-core::style::Color` as the canonical
/// home for CSS Color Module Level 4 hex parsing.
fn color_to_hex(color: Color) -> String {
    color.to_hex()
}

// ────────────────────────────────────────────────────────────────────
// scene/set_theme_mode — mutate-side (R599)
// ────────────────────────────────────────────────────────────────────

/// Typed request payload for [`set_theme_mode`]. Carries the requested
/// [`ThemeMode`] (wire `mode = "light" | "dark" | "system"`) plus the
/// cache tag the mutation applies to (defaults to
/// [`DEFAULT_THEME_TAG`] when [`None`]).
///
/// R618 §5.50 — `tag` borrows from the request's JSON payload via
/// the `'a` lifetime, mirroring [`SetThemePalettesParams`] and the
/// widget-axis setters ([`SetTextParams`](crate::text_state::SetTextParams)
/// et al). Pre-R618 the field was `Option<String>`, forcing a
/// `.to_owned()` allocation at the dispatch boundary for every
/// `scene/set_theme_mode` call even when the request omitted
/// `params.tag` entirely — pure allocation cost with no semantic
/// gain. Aligning the lifetime here unifies the typed-params
/// surface across every R608+ setter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetThemeModeParams<'a> {
    /// Cache tag the [`use_theme`](pinion_core::theme::use_theme) lookup resolves against. [`None`]
    /// → [`DEFAULT_THEME_TAG`] (`"app"`).
    ///
    /// R619 §5.50 — `tag` field moved to first position to match the
    /// Rust identity-first convention every R608+ widget-axis setter
    /// uses (`SetScrollOffsetParams.tag` / `SetTextParams.tag` / …).
    /// `std::fs::File::open(path)` / `std::process::Command::new(program)`
    /// are the canonical analogues. Tag is the scope identifier; the
    /// mutation arguments follow.
    pub tag: Option<&'a str>,
    /// Target [`ThemeMode`] resolved from the wire `mode` string.
    pub mode: ThemeMode,
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
    params: &SetThemeModeParams<'_>,
) -> Result<SetThemeModeOutcome, SetThemeModeError> {
    let Some(owner) = runtime_owner else {
        return Err(SetThemeModeError::RuntimeOwnerUnavailable);
    };
    let resolved_tag: &str = params.tag.unwrap_or(DEFAULT_THEME_TAG);
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
        mode: params.mode.name().to_string(),
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

// ────────────────────────────────────────────────────────────────────
// scene/set_theme_palettes — mutate-side (R608)
// ────────────────────────────────────────────────────────────────────

/// Typed request payload for [`set_theme_palettes`]. Carries the full
/// light + dark [`Theme`] pair the caller wants to swap into the bound
/// [`ThemeProvider`], plus the cache tag the mutation applies to
/// (defaults to [`DEFAULT_THEME_TAG`] when [`None`]).
///
/// The setter accepts a fully-populated [`Theme`] per palette rather
/// than a partial role map: the [`ThemeProvider::set_palettes`] contract
/// replaces the whole palette pair, and a partial / merge wire shape
/// would create a second, ambiguous semantics. AI agents performing a
/// per-role tweak round-trip through
/// [`scene/theme_tokens`](crate::theme::theme_tokens) → modify locally
/// → `scene/set_theme_palettes` — the same shape that
/// [`scene/theme_tokens`] returns is what this method consumes
/// (`{"role": "...", "color": "#rrggbb"}` array per palette).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetThemePalettesParams<'a> {
    /// Cache tag the [`use_theme`](pinion_core::theme::use_theme)
    /// lookup resolves against. [`None`] → [`DEFAULT_THEME_TAG`].
    ///
    /// R619 §5.50 — tag-first convention; see [`SetThemeModeParams`]
    /// for the rationale.
    pub tag: Option<&'a str>,
    /// Complete light palette — every [`ColorRole`] field bound.
    pub light: Theme,
    /// Complete dark palette — every [`ColorRole`] field bound.
    pub dark: Theme,
}

/// Snapshot returned to the caller after [`set_theme_palettes`] commits
/// the requested palette pair. Echoes the post-mutation state so a
/// follow-up [`theme_tokens`] call is unnecessary when the client only
/// needs confirmation of the swap.
///
/// Mirrors the [`SetThemeModeOutcome`] shape — `tag` + `mode` +
/// `system_scheme` + `active` — because the palette swap, like a mode
/// flip, is a state-changing call whose post-state the agent typically
/// wants to read back. The `palettes` field is intentionally omitted
/// (clients call `scene/theme_tokens` for that) to keep the wire
/// response narrow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetThemePalettesOutcome {
    /// Cache tag the mutation applied to.
    pub tag: String,
    /// Current [`ThemeMode`] (`"light"` / `"dark"` / `"system"`) — the
    /// palette swap does not change the mode, so this echoes the
    /// pre-call value.
    pub mode: String,
    /// Global [`system_color_scheme`] reading at snapshot time.
    pub system_scheme: String,
    /// `"light"` / `"dark"` — which of the two new palettes the
    /// application is now rendering under the current `mode` +
    /// `system_scheme`.
    pub active: String,
}

/// Typed errors the [`set_theme_palettes`] dispatcher can return.
///
/// Mirrors the [`SetThemeModeError`] shape — every variant maps onto
/// JSON-RPC `-32602 Invalid params` at the dispatch layer with the
/// variant name surfaced in `error.data` for pattern-matching clients.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetThemePalettesError {
    /// No [`runtime_owner`](crate::DispatchContext) registered on the
    /// dispatch context — see [`ThemeTokensError::RuntimeOwnerUnavailable`].
    RuntimeOwnerUnavailable,
    /// The owner is bound but no [`ThemeProvider`] is cached under
    /// `tag` yet — see [`ThemeTokensError::NotBound`].
    NotBound { tag: String },
}

/// Mutate the bound [`ThemeProvider`]'s light + dark palettes under
/// `params.tag` (default [`DEFAULT_THEME_TAG`]) and return the
/// post-mutation snapshot.
///
/// # Side effects
///
/// Calls [`ThemeProvider::set_palettes`], which writes both palette
/// [`Signal`](pinion_core::reactive::Signal)s inside a single
/// [`batch`](pinion_core::reactive::batch). Every subscriber to
/// [`ThemeProvider::theme`] re-runs **at most once** on the next
/// reactive tick even though two signals were mutated — the
/// `r593_set_palettes_atomic_batches_subscribers` regression pins
/// this property. The dispatcher bumps
/// [`SceneRevision`](pinion_core::SceneRevision) after this call
/// returns `Ok` so an in-flight preview's `base_revision` can detect
/// the concurrent mutation at apply time.
///
/// # Errors
///
/// - [`SetThemePalettesError::RuntimeOwnerUnavailable`] — context has
///   no substrate root [`Owner`].
/// - [`SetThemePalettesError::NotBound`] — no [`ThemeProvider`] is
///   cached under `tag` yet (the application's first view-fn run has
///   not happened, or the client used a non-default tag and the
///   application's `use_theme(_)` call has not run for it).
pub fn set_theme_palettes(
    runtime_owner: Option<&Owner>,
    params: &SetThemePalettesParams<'_>,
) -> Result<SetThemePalettesOutcome, SetThemePalettesError> {
    let Some(owner) = runtime_owner else {
        return Err(SetThemePalettesError::RuntimeOwnerUnavailable);
    };
    let resolved_tag: &str = params.tag.unwrap_or(DEFAULT_THEME_TAG);
    // R605 §5.22 — non-leaking lookup (see `theme_tokens` for the
    // rationale).
    let provider: std::rc::Rc<ThemeProvider> = owner
        .cache_get_by_str::<ThemeProvider>(resolved_tag)
        .ok_or_else(|| SetThemePalettesError::NotBound {
            tag: resolved_tag.to_string(),
        })?;
    provider.set_palettes(params.light, params.dark);
    let mode = provider.mode();
    let system_scheme = system_color_scheme();
    let active = active_palette_key(mode, system_scheme);
    Ok(SetThemePalettesOutcome {
        tag: resolved_tag.to_string(),
        mode: mode.name().to_string(),
        system_scheme: system_scheme.name().to_string(),
        active: active.to_string(),
    })
}

/// Typed error variants emitted by [`parse_palette_value`].
///
/// The dispatcher surfaces each variant as a JSON-RPC
/// `-32602 Invalid params` with the variant name in `error.data` and a
/// human-readable detail in `message`, so AI agents pattern-match on
/// the variant tag rather than parsing prose.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteParseError {
    /// `params.<which>` is not a JSON array.
    NotArray { which: String },
    /// Entry at `index` inside `params.<which>` is not a JSON object.
    EntryNotObject { which: String, index: usize },
    /// Entry at `index` is missing the `role` field or it is not a
    /// string.
    EntryMissingRole { which: String, index: usize },
    /// Entry at `index` is missing the `color` field or it is not a
    /// string.
    EntryMissingColor { which: String, index: usize },
    /// `role` field value does not match any [`ColorRole::name`].
    UnknownRole {
        which: String,
        index: usize,
        role: String,
    },
    /// Same [`ColorRole`] is bound twice inside the same palette
    /// array.
    DuplicateRole { which: String, role: String },
    /// `color` hex string does not parse — not `#rrggbb` /
    /// `#rrggbbaa`, or contains a non-hex digit.
    InvalidColor {
        which: String,
        role: String,
        value: String,
    },
    /// Palette array is missing one or more [`ColorRole`] entries —
    /// the setter requires a complete palette (every role bound) so
    /// the round-trip with [`scene/theme_tokens`](crate::theme::theme_tokens)
    /// stays total.
    MissingRoles { which: String, missing: Vec<String> },
}

/// Parse a JSON array of `{"role": "<name>", "color": "<hex>"}`
/// entries into a complete [`Theme`] struct. `which` names the side
/// (`"light"` / `"dark"`) and is echoed back into every
/// [`PaletteParseError`] so the caller can locate which palette
/// failed.
///
/// # Errors
///
/// Returns the first failure encountered — see [`PaletteParseError`]
/// for the variant list. A palette must contain exactly one entry per
/// [`ColorRole::all`] variant; partial palettes surface as
/// [`PaletteParseError::MissingRoles`] with the list of un-bound role
/// names.
///
/// # Determinism
///
/// `MissingRoles::missing` is ordered by [`ColorRole::all`]
/// declaration order, not the request's entry order, so the error
/// payload is stable across calls.
pub fn parse_palette_value(
    value: &serde_json::Value,
    which: &str,
) -> Result<Theme, PaletteParseError> {
    let Some(entries) = value.as_array() else {
        return Err(PaletteParseError::NotArray {
            which: which.to_string(),
        });
    };
    let mut bound_by_role: std::collections::HashMap<ColorRole, Color> =
        std::collections::HashMap::with_capacity(ColorRole::all().len());
    for (index, entry) in entries.iter().enumerate() {
        let Some(obj) = entry.as_object() else {
            return Err(PaletteParseError::EntryNotObject {
                which: which.to_string(),
                index,
            });
        };
        let role_name = obj
            .get("role")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| PaletteParseError::EntryMissingRole {
                which: which.to_string(),
                index,
            })?;
        let color_str = obj
            .get("color")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| PaletteParseError::EntryMissingColor {
                which: which.to_string(),
                index,
            })?;
        let role =
            ColorRole::from_name(role_name).ok_or_else(|| PaletteParseError::UnknownRole {
                which: which.to_string(),
                index,
                role: role_name.to_string(),
            })?;
        if bound_by_role.contains_key(&role) {
            return Err(PaletteParseError::DuplicateRole {
                which: which.to_string(),
                role: role_name.to_string(),
            });
        }
        let color = parse_color_hex(color_str).ok_or_else(|| PaletteParseError::InvalidColor {
            which: which.to_string(),
            role: role_name.to_string(),
            value: color_str.to_string(),
        })?;
        bound_by_role.insert(role, color);
    }
    // Walk ColorRole::all() in declaration order and consult the
    // HashMap. The double-pass shape avoids any `expect` / `unwrap`:
    // a role missing from the map lands in `missing` (and we error);
    // a role present lands in the corresponding Theme field via the
    // total `theme_from_role_map` consumer.
    let missing: Vec<String> = ColorRole::all()
        .iter()
        .filter(|role| !bound_by_role.contains_key(*role))
        .map(|role| role.name().to_string())
        .collect();
    if !missing.is_empty() {
        return Err(PaletteParseError::MissingRoles {
            which: which.to_string(),
            missing,
        });
    }
    Ok(theme_from_role_map(&bound_by_role))
}

/// Build a [`Theme`] from a fully-populated role → color map.
///
/// Caller contract: every [`ColorRole`] variant is bound in `map`. The
/// only call site is [`parse_palette_value`], which verifies coverage
/// via the [`PaletteParseError::MissingRoles`] gate immediately above
/// the call. Missing roles short-circuit to that error before this
/// function ever runs.
///
/// # Panics
///
/// Panics if `map` is missing any [`ColorRole`] variant — i.e. the
/// caller bypassed the `MissingRoles` precondition. Cannot happen
/// through the public [`parse_palette_value`] surface.
fn theme_from_role_map(map: &std::collections::HashMap<ColorRole, Color>) -> Theme {
    let lookup =
        |role: ColorRole| -> Color { *map.get(&role).expect("role bound by parse_palette_value") };
    Theme {
        surface: lookup(ColorRole::Surface),
        on_surface: lookup(ColorRole::OnSurface),
        on_surface_muted: lookup(ColorRole::OnSurfaceMuted),
        accent: lookup(ColorRole::Accent),
        on_accent: lookup(ColorRole::OnAccent),
        outline: lookup(ColorRole::Outline),
        surface_container_highest: lookup(ColorRole::SurfaceContainerHighest),
        surface_container_low: lookup(ColorRole::SurfaceContainerLow),
        surface_container: lookup(ColorRole::SurfaceContainer),
        surface_container_high: lookup(ColorRole::SurfaceContainerHigh),
        error: lookup(ColorRole::Error),
        on_error: lookup(ColorRole::OnError),
        error_container: lookup(ColorRole::ErrorContainer),
        on_error_container: lookup(ColorRole::OnErrorContainer),
        inverse_surface: lookup(ColorRole::InverseSurface),
        inverse_on_surface: lookup(ColorRole::InverseOnSurface),
        inverse_primary: lookup(ColorRole::InversePrimary),
    }
}

/// R615 §5.50 — wire-side delegate to [`Color::from_hex`] (substrate
/// primitive). Pre-R615 the parser lived RPC-side with a narrower
/// strict-subset accept policy (`#rrggbb` / `#rrggbbaa` only); R615
/// lifted to `pinion-core::style::Color::from_hex` and the wire now
/// follows the full CSS Color Module Level 4 spec — 3-digit `#rgb`
/// and 4-digit `#rgba` shorthand expand canonically (`#fff` →
/// `#ffffff` byte-equivalent). Round-trip with [`color_to_hex`] is
/// preserved because the encoder still emits 6-digit / 8-digit.
fn parse_color_hex(input: &str) -> Option<Color> {
    Color::from_hex(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::theme::{Theme, ThemeMode, set_system_color_scheme};

    // R622 §5.7 — 1-line typed wrapper specializing
    // crate::test_fixtures::bind_state to ThemeProvider (with the
    // per-axis name `bind_provider` preserved for theme's 21 call
    // sites — the substrate hook is `use_theme`, the cached type
    // is `ThemeProvider`, so `bind_provider` is the local idiom).
    fn bind_provider(owner: &Owner, tag: &'static str) -> std::rc::Rc<ThemeProvider> {
        crate::test_fixtures::bind_state::<ThemeProvider>(owner, tag)
    }

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

    fn params_with_mode<'a>(mode: ThemeMode) -> SetThemeModeParams<'a> {
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
            tag: Some("ghost"),
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
        let outcome = set_theme_mode(Some(&owner), &params_with_mode(ThemeMode::System)).unwrap();
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
        let outcome = set_theme_mode(Some(&owner), &params_with_mode(ThemeMode::Dark)).unwrap();
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
                tag: Some("studio"),
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

    // ─────────────────────────────────────────────────────────────────
    // R608 §5.50 — set_theme_palettes setter + parse helpers
    // ─────────────────────────────────────────────────────────────────

    fn theme_to_palette_json(theme: &Theme) -> serde_json::Value {
        let entries: Vec<serde_json::Value> = ColorRole::all()
            .iter()
            .map(|role| {
                serde_json::json!({
                    "role": role.name(),
                    "color": color_to_hex(theme.resolve(*role)),
                })
            })
            .collect();
        serde_json::Value::Array(entries)
    }

    #[test]
    fn r608_parse_color_hex_accepts_six_digit_lowercase() {
        let c = parse_color_hex("#fefbff").unwrap();
        assert_eq!(c, Color::rgb(0xfe, 0xfb, 0xff));
    }

    #[test]
    fn r608_parse_color_hex_accepts_six_digit_uppercase() {
        // CSS Color Module Level 4: hex digits are case-insensitive.
        let c = parse_color_hex("#FEFBFF").unwrap();
        assert_eq!(c, Color::rgb(0xfe, 0xfb, 0xff));
    }

    #[test]
    fn r608_parse_color_hex_accepts_eight_digit_with_alpha() {
        let c = parse_color_hex("#10203080").unwrap();
        assert_eq!(c, Color::rgba(0x10, 0x20, 0x30, 0x80));
    }

    #[test]
    fn r608_parse_color_hex_implies_opaque_alpha_on_six_digit() {
        let c = parse_color_hex("#000000").unwrap();
        assert_eq!(c.a, 0xff, "six-digit form implies fully-opaque alpha");
    }

    #[test]
    fn r608_parse_color_hex_rejects_missing_hash() {
        assert_eq!(parse_color_hex("fefbff"), None);
    }

    #[test]
    fn r615_parse_color_hex_accepts_three_digit_shorthand_via_substrate_lift() {
        // R615 §5.50 — wire is now a thin delegate to
        // Color::from_hex (substrate). CSS Color Module Level 4
        // shorthand expansion lights up: `#fff` → 0xffffff. Pre-R615
        // the wire rejected shorthand for symmetric strictness with
        // the writer; R615 relaxed the reader to full CSS spec while
        // keeping the writer emitting 6/8-digit. Round-trip property
        // (read response → modify → write back) is preserved because
        // the writer's output always falls under the strict subset
        // the pre-R615 reader accepted.
        assert_eq!(parse_color_hex("#fff"), Some(Color::rgb(0xff, 0xff, 0xff)));
        assert_eq!(parse_color_hex("#f0a"), Some(Color::rgb(0xff, 0x00, 0xaa)));
    }

    #[test]
    fn r608_parse_color_hex_rejects_invalid_hex_digit() {
        assert_eq!(parse_color_hex("#zzzzzz"), None);
        assert_eq!(parse_color_hex("#12345g"), None);
    }

    #[test]
    fn r608_parse_color_hex_rejects_wrong_length() {
        // CSS Color Module Level 4 defines only the four shapes
        // (#RGB / #RGBA / #RRGGBB / #RRGGBBAA). 1/2/5/7/9-digit
        // inputs surface as None.
        assert_eq!(parse_color_hex("#1"), None);
        assert_eq!(parse_color_hex("#12"), None);
        assert_eq!(parse_color_hex("#12345"), None);
        assert_eq!(parse_color_hex("#1234567"), None);
        assert_eq!(parse_color_hex("#123456789"), None);
    }

    #[test]
    fn r608_parse_color_hex_round_trips_with_color_to_hex_opaque() {
        // Property: parse(color_to_hex(c)) == c for every opaque c.
        for c in [
            Color::rgb(0x00, 0x00, 0x00),
            Color::rgb(0xff, 0xff, 0xff),
            Color::rgb(0x19, 0x76, 0xd2),
            Color::rgb(0xb3, 0x26, 0x1e),
        ] {
            assert_eq!(parse_color_hex(&color_to_hex(c)), Some(c));
        }
    }

    #[test]
    fn r608_parse_color_hex_round_trips_with_color_to_hex_translucent() {
        // Property holds for translucent colors too — color_to_hex
        // emits the 8-digit form when alpha != 0xff.
        let c = Color::rgba(0x10, 0x20, 0x30, 0x80);
        assert_eq!(parse_color_hex(&color_to_hex(c)), Some(c));
    }

    // Parse-palette failure shape pins.

    #[test]
    fn r608_parse_palette_rejects_non_array_value() {
        let v = serde_json::json!({"role": "surface"});
        let err = parse_palette_value(&v, "light").unwrap_err();
        assert_eq!(
            err,
            PaletteParseError::NotArray {
                which: "light".into()
            },
        );
    }

    #[test]
    fn r608_parse_palette_rejects_non_object_entry() {
        let v = serde_json::json!([42]);
        let err = parse_palette_value(&v, "dark").unwrap_err();
        assert_eq!(
            err,
            PaletteParseError::EntryNotObject {
                which: "dark".into(),
                index: 0,
            },
        );
    }

    #[test]
    fn r608_parse_palette_rejects_missing_role_field() {
        let v = serde_json::json!([{"color": "#ffffff"}]);
        let err = parse_palette_value(&v, "light").unwrap_err();
        assert_eq!(
            err,
            PaletteParseError::EntryMissingRole {
                which: "light".into(),
                index: 0,
            },
        );
    }

    #[test]
    fn r608_parse_palette_rejects_missing_color_field() {
        let v = serde_json::json!([{"role": "surface"}]);
        let err = parse_palette_value(&v, "light").unwrap_err();
        assert_eq!(
            err,
            PaletteParseError::EntryMissingColor {
                which: "light".into(),
                index: 0,
            },
        );
    }

    #[test]
    fn r608_parse_palette_rejects_unknown_role_name() {
        let v = serde_json::json!([{"role": "Surface", "color": "#ffffff"}]);
        let err = parse_palette_value(&v, "light").unwrap_err();
        assert_eq!(
            err,
            PaletteParseError::UnknownRole {
                which: "light".into(),
                index: 0,
                role: "Surface".into(),
            },
        );
    }

    #[test]
    fn r608_parse_palette_rejects_duplicate_role() {
        let v = serde_json::json!([
            {"role": "surface", "color": "#ffffff"},
            {"role": "surface", "color": "#000000"},
        ]);
        let err = parse_palette_value(&v, "light").unwrap_err();
        assert_eq!(
            err,
            PaletteParseError::DuplicateRole {
                which: "light".into(),
                role: "surface".into(),
            },
        );
    }

    #[test]
    fn r608_parse_palette_rejects_invalid_color_hex() {
        let v = serde_json::json!([{"role": "surface", "color": "not-a-color"}]);
        let err = parse_palette_value(&v, "light").unwrap_err();
        assert_eq!(
            err,
            PaletteParseError::InvalidColor {
                which: "light".into(),
                role: "surface".into(),
                value: "not-a-color".into(),
            },
        );
    }

    #[test]
    fn r608_parse_palette_rejects_partial_palette_with_missing_role_list() {
        // Only the surface field bound; every other role is missing.
        let v = serde_json::json!([{"role": "surface", "color": "#ffffff"}]);
        let err = parse_palette_value(&v, "dark").unwrap_err();
        let PaletteParseError::MissingRoles { which, missing } = err else {
            panic!("expected MissingRoles");
        };
        assert_eq!(which, "dark");
        // Surface is the only one bound, so 13 missing.
        assert_eq!(missing.len(), ColorRole::all().len() - 1);
        // Declaration-order — on_surface is next after surface.
        assert_eq!(missing[0], "on_surface");
        assert!(
            !missing.contains(&"surface".to_string()),
            "surface was bound; cannot appear in missing list",
        );
    }

    #[test]
    fn r608_parse_palette_happy_path_round_trips_with_color_to_hex() {
        let light = Theme::light();
        let v = theme_to_palette_json(&light);
        let parsed = parse_palette_value(&v, "light").unwrap();
        assert_eq!(parsed, light, "round-trip Theme::light() through wire form");
    }

    // set_theme_palettes failure modes.

    #[test]
    fn r608_set_theme_palettes_missing_runtime_owner_errors() {
        let params = SetThemePalettesParams {
            light: Theme::light(),
            dark: Theme::dark(),
            tag: None,
        };
        let err = set_theme_palettes(None, &params).unwrap_err();
        assert_eq!(err, SetThemePalettesError::RuntimeOwnerUnavailable);
    }

    #[test]
    fn r608_set_theme_palettes_unbound_tag_errors_with_tag_echoed() {
        let owner = Owner::new();
        let params = SetThemePalettesParams {
            light: Theme::light(),
            dark: Theme::dark(),
            tag: Some("ghost"),
        };
        let err = set_theme_palettes(Some(&owner), &params).unwrap_err();
        assert_eq!(
            err,
            SetThemePalettesError::NotBound {
                tag: "ghost".into(),
            },
        );
    }

    #[test]
    fn r608_set_theme_palettes_does_not_insert_a_new_cache_slot() {
        let owner = Owner::new();
        let params = SetThemePalettesParams {
            light: Theme::light(),
            dark: Theme::dark(),
            tag: None,
        };
        let _ = set_theme_palettes(Some(&owner), &params).unwrap_err();
        assert!(
            !owner.cache_contains::<ThemeProvider>("app"),
            "set_theme_palettes must not materialize a ThemeProvider on a failed lookup",
        );
    }

    // set_theme_palettes happy path.

    #[test]
    fn r608_set_theme_palettes_swaps_both_palettes_atomically() {
        let owner = Owner::new();
        let provider = bind_provider(&owner, "app");
        // Custom palettes — distinct from canonical light/dark so we
        // can probe the swap by checking the surface field.
        let custom_light = Theme {
            surface: Color::rgb(0xab, 0xcd, 0xef),
            ..Theme::light()
        };
        let custom_dark = Theme {
            surface: Color::rgb(0x12, 0x34, 0x56),
            ..Theme::dark()
        };
        let _ = set_theme_palettes(
            Some(&owner),
            &SetThemePalettesParams {
                light: custom_light,
                dark: custom_dark,
                tag: None,
            },
        )
        .unwrap();
        assert_eq!(
            provider.light_palette().surface,
            Color::rgb(0xab, 0xcd, 0xef)
        );
        assert_eq!(
            provider.dark_palette().surface,
            Color::rgb(0x12, 0x34, 0x56)
        );
    }

    #[test]
    fn r608_set_theme_palettes_echoes_post_mode_active_resolution() {
        let _guard = SystemSchemeGuard::pinned_to(SystemColorScheme::Light);
        let owner = Owner::new();
        let provider = bind_provider(&owner, "app");
        provider.set_mode(ThemeMode::Dark);
        let outcome = set_theme_palettes(
            Some(&owner),
            &SetThemePalettesParams {
                light: Theme::light(),
                dark: Theme::dark(),
                tag: None,
            },
        )
        .unwrap();
        // The palette swap does not change the mode — dark stays dark.
        assert_eq!(outcome.mode, "dark");
        assert_eq!(outcome.active, "dark");
        assert_eq!(outcome.tag, "app");
        assert_eq!(outcome.system_scheme, "light");
    }

    #[test]
    fn r608_set_theme_palettes_custom_tag_round_trips() {
        let owner = Owner::new();
        let _provider = bind_provider(&owner, "studio");
        let outcome = set_theme_palettes(
            Some(&owner),
            &SetThemePalettesParams {
                light: Theme::light(),
                dark: Theme::dark(),
                tag: Some("studio"),
            },
        )
        .unwrap();
        assert_eq!(outcome.tag, "studio");
    }

    #[test]
    fn r608_set_theme_palettes_outcome_serializes_to_expected_keys() {
        let owner = Owner::new();
        let _provider = bind_provider(&owner, "app");
        let outcome = set_theme_palettes(
            Some(&owner),
            &SetThemePalettesParams {
                light: Theme::light(),
                dark: Theme::dark(),
                tag: None,
            },
        )
        .unwrap();
        let json = serde_json::to_value(&outcome).unwrap();
        let obj = json.as_object().expect("outcome is a JSON object");
        let mut keys: Vec<&String> = obj.keys().collect();
        keys.sort();
        let key_strs: Vec<&str> = keys.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            key_strs,
            vec!["active", "mode", "system_scheme", "tag"],
            "wire response carries the 4-field post-state echo",
        );
    }

    #[test]
    fn r608_set_theme_palettes_subscribers_re_run_once_per_swap() {
        // Atomic-batch contract carry from R593 — replacing both
        // palettes coalesces into one subscriber re-run. The RPC entry
        // calls ThemeProvider::set_palettes() which wraps the two
        // signal writes in `reactive::batch`; verifying the count here
        // pins the RPC layer to that contract (a future refactor that
        // dropped the wrap would inflate downstream view-fn work
        // during a palette swap, silently doubling the cost).
        //
        // The whole test body runs inside `owner.run(...)` so the
        // Effect's re-run (which fires synchronously during the
        // `set_palettes` batch flush) sees an active `Owner::current()`
        // — `use_theme` inside the closure body requires it. The
        // production wire (Window / App render loop) wraps the same
        // way before issuing reactive writes.
        use pinion_core::reactive::Effect;
        use std::cell::Cell;
        use std::rc::Rc;
        let owner = Owner::new();
        let runs = Rc::new(Cell::new(0u32));
        let runs_clone = runs.clone();
        owner.run(|| {
            let _provider = pinion_core::theme::use_theme("app");
            let _effect = Effect::new(&owner, move || {
                let p = pinion_core::theme::use_theme("app");
                let _light = p.light_palette();
                let _dark = p.dark_palette();
                runs_clone.set(runs_clone.get() + 1);
            });
            let baseline = runs.get();
            let _ = set_theme_palettes(
                Some(&owner),
                &SetThemePalettesParams {
                    light: Theme {
                        surface: Color::rgb(0x01, 0x02, 0x03),
                        ..Theme::light()
                    },
                    dark: Theme {
                        surface: Color::rgb(0x04, 0x05, 0x06),
                        ..Theme::dark()
                    },
                    tag: None,
                },
            )
            .unwrap();
            assert_eq!(
                runs.get(),
                baseline + 1,
                "set_palettes must coalesce both writes into one Effect re-run",
            );
        });
    }
}
