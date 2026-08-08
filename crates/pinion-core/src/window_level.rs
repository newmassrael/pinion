//! R1610 §5.16 §5.41 §2 #7 — a window's **level**: where it sits in the
//! window manager's front-to-back order, and whether the windowing system
//! actually running can honour that.
//!
//! A monitoring tool's first request is a small window that stays visible over
//! whatever else the user is doing — a KPI readout, a packet counter, a
//! transport bar. The capability is ordinary; what varies between toolkits is
//! whether the program can find out that it did not work.
//!
//! # Why a level is one value and not two flags
//!
//! The usual encoding is a pair of independent bits in a window-flags word,
//! one for "stay above" and one for "stay below". That makes the contradictory
//! value — both bits set — representable, and a value a program can build is a
//! value some layer has to resolve. Measured against a mainstream toolkit's
//! own platform backends, the three resolve it three different ways: one sets
//! both stacking hints on a mapped window while its show path takes only the
//! first that matches, so the same pair means one thing at map time and
//! another on a live change; one warns and picks "above"; and one ignores
//! "below" entirely, having never implemented it.
//!
//! [`WindowLevel`] is one value with three arms, so the contradiction is not
//! representable and no platform gets to arbitrate it.
//!
//! # Why the outcome is reported
//!
//! The third behaviour above is the real defect, and it is not a missing
//! platform capability — that platform does have levels below normal, and the
//! window backend used here drives one. It is that a flags accessor returns
//! the value the program **stored**, so a program sets stay-below, reads it
//! back as set, and is wrong. A declaration the platform drops is worse than
//! no declaration, because the program believes it took.
//!
//! So a level here is published together with what became of it —
//! [`LevelOutcome`], the same shape [`crate::display::Anchored`] gives a
//! declared display. The declaration stays exactly what the binding wrote;
//! the outcome says whether the running backend drives it.
//!
//! # The error direction is chosen
//!
//! [`WindowingBackend::outcome`] is a model of the window backend beneath us,
//! not a query — no windowing system reports "I ignored that", and the backend
//! exposes no getter for a window's level. The table can therefore go stale,
//! and it is arranged so that staleness fails in the **self-correcting**
//! direction: a backend that gains support is over-reported as
//! [`LevelOutcome::Unsupported`], which the next person to reach for it
//! notices, where the reverse — claiming [`LevelOutcome::Applied`] for a
//! backend that drops it — is the silent lie described above and nobody would
//! ever notice it. Wrong-absent self-corrects; wrong-present inflates
//! quietly (R1602).

/// R1610 §5.16 — where a window sits in the window manager's front-to-back
/// order.
///
/// One value with three arms rather than a pair of independent hint bits: a
/// window is on top, or at the bottom, or neither, and "both" is not a thing
/// that can be said. See the module docs for what a two-flag encoding costs.
///
/// `Normal` is the [`Default`] and every pre-R1610 window, so a binding that
/// never mentions a level behaves exactly as before.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum WindowLevel {
    /// Below ordinary windows. The desktop-widget position: a system monitor
    /// that sits on the wallpaper and never covers real work.
    AlwaysOnBottom,
    /// Ordinary stacking — the window rises when it is used and is covered
    /// when it is not. The default, and what every window was before R1610.
    #[default]
    Normal,
    /// Above ordinary windows. The floating-readout position: a torn-off KPI
    /// panel that stays visible over the application being watched.
    AlwaysOnTop,
}

impl WindowLevel {
    /// Every level, in bottom-to-top order.
    ///
    /// Enumerable on purpose. Where this is a pair of bits inside a flags word
    /// carrying twenty unrelated hints, "offer the user the window levels" is
    /// not a question the program can ask its own vocabulary — it has to know
    /// the two spellings and that they are exclusive, a rule that lives in the
    /// platform backends rather than in the type. Here a level picker is
    /// `WindowLevel::ALL`.
    pub const ALL: [Self; 3] = [Self::AlwaysOnBottom, Self::Normal, Self::AlwaysOnTop];

    /// The wire spelling, matching the serde representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AlwaysOnBottom => "always_on_bottom",
            Self::Normal => "normal",
            Self::AlwaysOnTop => "always_on_top",
        }
    }

    /// Parse a wire spelling — the read inverse of [`Self::as_str`], derived
    /// from [`Self::ALL`] so the two cannot drift.
    ///
    /// `None` for anything else: a refusal the RPC layer turns into a named
    /// error rather than a silent fallback to [`Self::Normal`], because "the
    /// level you asked for is not one" and "you asked for normal" are
    /// different facts.
    #[must_use]
    pub fn from_wire(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.as_str() == s)
    }

    /// Does this level ask the window manager for anything?
    ///
    /// [`Self::Normal`] does not — it is the absence of a request — which is
    /// why a backend with no level support still honours it. Everything in
    /// [`WindowingBackend::outcome`] turns on this distinction.
    #[must_use]
    pub const fn is_stacking_request(self) -> bool {
        !matches!(self, Self::Normal)
    }
}

/// R1610 §5.16 — the windowing system a window is actually on.
///
/// Not a build target: a Linux binary is one build and can be running on
/// either X11 or Wayland, and that is precisely the pair whose level support
/// differs. `pinion-shell` reads it from the live event loop.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowingBackend {
    /// X11, through the `_NET_WM_STATE_ABOVE` / `_NET_WM_STATE_BELOW` hints.
    X11,
    /// Wayland. There is no stacking request in the core shell protocol, and
    /// the window backend's Wayland implementation of `set_window_level` is
    /// accordingly an empty body.
    Wayland,
    /// macOS, through the native window's level property.
    MacOs,
    /// Windows, through the top-most / bottom z-order insert positions.
    Windows,
    /// A backend this adapter has not measured. Reported rather than guessed
    /// — see [`LevelOutcome::Unknown`].
    Other,
}

impl WindowingBackend {
    /// The wire spelling, matching the serde representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::X11 => "x11",
            Self::Wayland => "wayland",
            Self::MacOs => "macos",
            Self::Windows => "windows",
            Self::Other => "other",
        }
    }

    /// What this backend does with a declared level.
    ///
    /// [`WindowLevel::Normal`] is always [`LevelOutcome::Applied`]: it asks
    /// for nothing, so there is nothing for a backend to fail at. Only a
    /// stacking request can be dropped.
    #[must_use]
    pub const fn outcome(self, level: WindowLevel) -> LevelOutcome {
        if !level.is_stacking_request() {
            return LevelOutcome::Applied {
                level,
                backend: self,
            };
        }
        match self {
            Self::X11 | Self::MacOs | Self::Windows => LevelOutcome::Applied {
                level,
                backend: self,
            },
            Self::Wayland => LevelOutcome::Unsupported {
                declared: level,
                backend: self,
            },
            Self::Other => LevelOutcome::Unknown {
                declared: level,
                backend: self,
            },
        }
    }
}

/// R1610 §5.16 §2 #7 — what became of a window's declared [`WindowLevel`] on
/// the windowing system actually running.
///
/// The declaration is untouched by this: a binding reads back exactly what it
/// wrote (that is what makes a saved layout a layout). This is the *second*
/// fact, the one a stored-flags accessor has no channel for — see the module
/// docs.
///
/// Three arms rather than a bool, because "this backend cannot do it" and
/// "nobody here has measured this backend" are different, and collapsing them
/// would mean either inventing a failure or claiming a success.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LevelOutcome {
    /// The backend drives this level; the window is where it says it is.
    Applied {
        /// The level in force — equal to the one declared.
        level: WindowLevel,
        /// The backend driving it.
        backend: WindowingBackend,
    },
    /// The backend has no way to express this level. The window sits at the
    /// compositor's discretion, i.e. as though [`WindowLevel::Normal`] had
    /// been declared, and the declaration stands as unhonoured intent.
    Unsupported {
        /// The level the binding asked for, which is not in force.
        declared: WindowLevel,
        /// The backend that cannot express it.
        backend: WindowingBackend,
    },
    /// The level was commanded, and this adapter has no measurement for what
    /// this backend does with it. Deliberately not folded into either of the
    /// other two arms: an unmeasured backend is not a known failure and not a
    /// known success.
    Unknown {
        /// The level the binding asked for.
        declared: WindowLevel,
        /// The backend, unmeasured here.
        backend: WindowingBackend,
    },
}

impl LevelOutcome {
    /// The level the binding declared, whatever became of it.
    #[must_use]
    pub const fn declared(&self) -> WindowLevel {
        match self {
            Self::Applied { level, .. } => *level,
            Self::Unsupported { declared, .. } | Self::Unknown { declared, .. } => *declared,
        }
    }

    /// The wire spelling of which outcome this is: `applied`, `unsupported`
    /// or `unknown`, matching the serde tag.
    ///
    /// Lives HERE, next to the arms, so the match is exhaustive and the
    /// compiler enumerates a new arm. The wire layer is a different crate and
    /// this type is `#[non_exhaustive]`, so a mapper written over there would
    /// need a wildcard — and a wildcard at a wire boundary silently reports a
    /// new arm as an old one, which is R1600's lesson: adding an arm is weaker
    /// than changing a type, because only an exhaustive match enumerates.
    /// Keeping `#[non_exhaustive]` still buys what it is for, which is that
    /// only [`WindowingBackend::outcome`] can construct one.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Applied { .. } => "applied",
            Self::Unsupported { .. } => "unsupported",
            Self::Unknown { .. } => "unknown",
        }
    }

    /// The backend this outcome was derived against.
    #[must_use]
    pub const fn backend(&self) -> WindowingBackend {
        match self {
            Self::Applied { backend, .. }
            | Self::Unsupported { backend, .. }
            | Self::Unknown { backend, .. } => *backend,
        }
    }

    /// Is the declared level known to be in force?
    ///
    /// [`Self::Unknown`] answers `false` — a caller asking "can I rely on
    /// this window staying on top?" gets a no when nobody knows, which is the
    /// same conservatism [`crate::display::Anchored::is_declared`] applies to
    /// a substituted display.
    #[must_use]
    pub const fn is_honoured(&self) -> bool {
        matches!(self, Self::Applied { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r1610_level_wire_spellings_round_trip() {
        for level in WindowLevel::ALL {
            assert_eq!(
                WindowLevel::from_wire(level.as_str()),
                Some(level),
                "{level:?} must parse back from its own wire spelling",
            );
            let json = serde_json::to_string(&level).unwrap();
            assert_eq!(
                json,
                format!("\"{}\"", level.as_str()),
                "serde and as_str must agree for {level:?}",
            );
            assert_eq!(
                serde_json::from_str::<WindowLevel>(&json).unwrap(),
                level,
                "{level:?} must survive a serde round trip",
            );
        }
    }

    #[test]
    fn r1610_an_unknown_spelling_is_refused_not_defaulted() {
        // The refusal is the point: falling back to Normal would make
        // "always_on_topp" indistinguishable from asking for normal.
        assert_eq!(WindowLevel::from_wire("always_on_topp"), None);
        assert_eq!(WindowLevel::from_wire(""), None);
        assert_eq!(WindowLevel::from_wire("AlwaysOnTop"), None);
        assert_eq!(WindowLevel::from_wire("top"), None);
    }

    #[test]
    fn r1610_default_level_is_normal() {
        assert_eq!(WindowLevel::default(), WindowLevel::Normal);
        assert!(!WindowLevel::default().is_stacking_request());
    }

    #[test]
    fn r1610_all_enumerates_every_level_bottom_to_top() {
        assert_eq!(
            WindowLevel::ALL,
            [
                WindowLevel::AlwaysOnBottom,
                WindowLevel::Normal,
                WindowLevel::AlwaysOnTop
            ],
        );
        // Distinct spellings — an enumeration whose members collide is not one.
        let mut spellings: Vec<&str> = WindowLevel::ALL.iter().map(|l| l.as_str()).collect();
        spellings.sort_unstable();
        spellings.dedup();
        assert_eq!(spellings.len(), WindowLevel::ALL.len());
    }

    #[test]
    fn r1610_only_a_stacking_request_can_be_dropped() {
        // Normal asks the window manager for nothing, so every backend
        // honours it — including the one that can express nothing.
        for backend in [
            WindowingBackend::X11,
            WindowingBackend::Wayland,
            WindowingBackend::MacOs,
            WindowingBackend::Windows,
            WindowingBackend::Other,
        ] {
            let outcome = backend.outcome(WindowLevel::Normal);
            assert!(
                outcome.is_honoured(),
                "{backend:?} must honour Normal — it is the absence of a request",
            );
            assert_eq!(outcome.declared(), WindowLevel::Normal);
            assert_eq!(outcome.backend(), backend);
        }
    }

    #[test]
    fn r1610_wayland_reports_a_stacking_request_as_unsupported() {
        // The window backend's Wayland implementation of set_window_level is
        // an empty body; the core shell protocol has no stacking request.
        for level in [WindowLevel::AlwaysOnTop, WindowLevel::AlwaysOnBottom] {
            let outcome = WindowingBackend::Wayland.outcome(level);
            assert_eq!(
                outcome,
                LevelOutcome::Unsupported {
                    declared: level,
                    backend: WindowingBackend::Wayland,
                },
            );
            assert!(!outcome.is_honoured());
            // The DECLARATION survives the drop — a layout preset that says
            // "on top" still says it after being told the desk cannot.
            assert_eq!(outcome.declared(), level);
        }
    }

    #[test]
    fn r1610_the_three_driving_backends_apply_both_stacking_requests() {
        for backend in [
            WindowingBackend::X11,
            WindowingBackend::MacOs,
            WindowingBackend::Windows,
        ] {
            for level in [WindowLevel::AlwaysOnTop, WindowLevel::AlwaysOnBottom] {
                assert_eq!(
                    backend.outcome(level),
                    LevelOutcome::Applied { level, backend },
                    "{backend:?} drives {level:?}",
                );
            }
        }
    }

    #[test]
    fn r1610_macos_bottom_is_the_arm_a_two_flag_encoding_loses() {
        // This is the arm the reference encoding drops: its macOS backend
        // never reads the stay-below bit at all, so a program declares it,
        // reads it back as set from its own stored flags, and is wrong. Here
        // the declaration is driven (the backend maps it to one level below
        // normal) and, either way, REPORTED.
        assert_eq!(
            WindowingBackend::MacOs.outcome(WindowLevel::AlwaysOnBottom),
            LevelOutcome::Applied {
                level: WindowLevel::AlwaysOnBottom,
                backend: WindowingBackend::MacOs,
            },
        );
    }

    #[test]
    fn r1610_unmeasured_backend_is_neither_a_failure_nor_a_success() {
        let outcome = WindowingBackend::Other.outcome(WindowLevel::AlwaysOnTop);
        assert_eq!(
            outcome,
            LevelOutcome::Unknown {
                declared: WindowLevel::AlwaysOnTop,
                backend: WindowingBackend::Other,
            },
        );
        // Conservative: a caller asking whether it can rely on the level
        // gets a no, exactly as a substituted display is not is_declared.
        assert!(!outcome.is_honoured());
    }

    #[test]
    fn r1610_an_outcome_round_trips_through_serde() {
        for backend in [
            WindowingBackend::X11,
            WindowingBackend::Wayland,
            WindowingBackend::MacOs,
            WindowingBackend::Windows,
            WindowingBackend::Other,
        ] {
            for level in WindowLevel::ALL {
                let outcome = backend.outcome(level);
                let json = serde_json::to_string(&outcome).unwrap();
                assert_eq!(
                    serde_json::from_str::<LevelOutcome>(&json).unwrap(),
                    outcome,
                    "{backend:?} x {level:?} must survive the wire",
                );
                // The kind is what a wire reader branches on.
                assert!(
                    json.contains("\"kind\""),
                    "outcome must be a tagged union on the wire: {json}",
                );
            }
        }
    }

    #[test]
    fn r1610_the_outcome_kind_matches_its_serde_tag() {
        // Two spellings of one fact, so a test holds them together: `kind()`
        // is what the wire layer reads and the serde tag is what the JSON
        // carries, and nothing else would notice them diverging.
        for backend in [
            WindowingBackend::X11,
            WindowingBackend::Wayland,
            WindowingBackend::Other,
        ] {
            for level in WindowLevel::ALL {
                let outcome = backend.outcome(level);
                let json: serde_json::Value =
                    serde_json::from_str(&serde_json::to_string(&outcome).unwrap()).unwrap();
                assert_eq!(
                    json["kind"].as_str(),
                    Some(outcome.kind()),
                    "{backend:?} x {level:?}",
                );
            }
        }
    }

    #[test]
    fn r1610_backend_wire_spellings_are_distinct() {
        let mut spellings: Vec<&str> = [
            WindowingBackend::X11,
            WindowingBackend::Wayland,
            WindowingBackend::MacOs,
            WindowingBackend::Windows,
            WindowingBackend::Other,
        ]
        .iter()
        .map(|b| b.as_str())
        .collect();
        let n = spellings.len();
        spellings.sort_unstable();
        spellings.dedup();
        assert_eq!(spellings.len(), n, "two backends must not share a spelling");
    }
}
