//! ★★★★★ R1724 — **the roster: which destination is which screen, and the one
//! rule that makes the others hold.**
//!
//! The rule: *the screen the journey is at is the only one anything reaches.*
//! Every accessor here is keyed by a [`Journey`], so there is no expression in
//! this crate that hands out a screen the application is not showing — which is
//! the difference between this and the reference toolkit's paged container,
//! where a hidden page counted a press, a key and a wheel, appeared in the
//! accessibility tree with its children, and left its floating windows on
//! screen (all four measured at 6.11.1).

use std::cell::Cell;
use std::collections::BTreeMap;

use pinion_core::external::with_surface_extent;
use pinion_core::shrink::pan;
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::destination::{Destinations, Journey};
use pinion_core::{Frame, Scene};

use crate::Screen;

/// A host's cached projection: where it is, and how far the screen it is
/// showing has moved.
///
/// This is what a host declares as its
/// [`WidgetCore::State`](pinion_core::WidgetCore::State). Neither field is read
/// as a quantity — together they are the change detector that makes the
/// framework repaint a host whose own state is constant while the screen inside
/// it is not. A host with `State = ()` mounting a screen with a text field
/// would otherwise paint the field's first frame and no other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ScreenState {
    /// The destination's position in the roster.
    ///
    /// The position and not the key, because a host's state must be `Copy`.
    /// It changes on every arrival, which is the half of the detector that
    /// notices *navigation*.
    pub at: u32,
    /// The current screen's latch revision — the half that notices the screen
    /// itself moving.
    pub revision: u64,
}

/// What is wrong with a pairing of destinations and screens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MountDefect {
    /// A screen was mounted at a key the destination roster does not hold, so
    /// nothing could ever navigate to it.
    NoSuchDestination {
        /// The key the screen was mounted at.
        key: String,
    },
    /// A screen was mounted at a destination that is closed, so the roster
    /// says one thing and the mount says another.
    ///
    /// The direction that matters: a seat declared
    /// [`Unavailable::elsewhere`](pinion_core::availability::Unavailable::elsewhere)
    /// — *built, shipping, and not here* — with the screen mounted right here
    /// is a sentence the application would be telling a reader while showing
    /// them the opposite.
    DestinationIsClosed {
        /// The key the screen was mounted at.
        key: String,
    },
    /// Two screens were mounted at one key.
    DuplicateMount {
        /// The key both claim.
        key: String,
    },
}

impl core::fmt::Display for MountDefect {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            MountDefect::NoSuchDestination { key } => {
                write!(f, "no destination `{key}` to mount a screen at")
            }
            MountDefect::DestinationIsClosed { key } => write!(
                f,
                "destination `{key}` is closed, and a closed destination with a \
                 screen behind it tells a reader the opposite of what it shows"
            ),
            MountDefect::DuplicateMount { key } => {
                write!(f, "two screens mounted at destination `{key}`")
            }
        }
    }
}

impl std::error::Error for MountDefect {}

/// The destinations of an application, and the screens behind the ones that
/// have one.
///
/// Not every destination needs a screen: a host that paints some of its own
/// pages inline (a dashboard whose cards are the host's) leaves those keys
/// unmounted, and every accessor here answers `None` for them. That is what
/// lets an application be assembled from screens *and* pages without a second
/// vocabulary for the difference.
pub struct ScreenRoster {
    destinations: Destinations,
    screens: BTreeMap<String, Box<dyn Screen>>,
    /// The extent the page region was last laid out at, so every delegated
    /// call reads the rectangle the screen is actually in.
    ///
    /// `(0, 0)` before the first paint — "the region has no extent yet", which
    /// is the one reading [`with_surface_extent`] refuses and this type must
    /// therefore not make.
    placed_extent: Cell<(u32, u32)>,
}

impl ScreenRoster {
    /// Pair a destination roster with the screens behind its keys.
    ///
    /// # Errors
    ///
    /// [`MountDefect`] — a screen at a key the roster does not hold, at a key
    /// the roster declares closed, or two screens at one key.
    pub fn new(
        destinations: Destinations,
        mounts: Vec<(&str, Box<dyn Screen>)>,
    ) -> Result<Self, MountDefect> {
        let mut screens: BTreeMap<String, Box<dyn Screen>> = BTreeMap::new();
        for (key, screen) in mounts {
            match destinations.get(key) {
                None => {
                    return Err(MountDefect::NoSuchDestination {
                        key: key.to_owned(),
                    });
                }
                Some(destination) if !destination.standing.is_open() => {
                    return Err(MountDefect::DestinationIsClosed {
                        key: key.to_owned(),
                    });
                }
                Some(_) => {}
            }
            if screens.insert(key.to_owned(), screen).is_some() {
                return Err(MountDefect::DuplicateMount {
                    key: key.to_owned(),
                });
            }
        }
        Ok(Self {
            destinations,
            screens,
            placed_extent: Cell::new((0, 0)),
        })
    }

    /// The destinations, which are what the rail is painted from and what
    /// [`Journey::navigate`] refuses against.
    #[must_use]
    pub fn destinations(&self) -> &Destinations {
        &self.destinations
    }

    /// Whether this destination's page is a mounted screen rather than one the
    /// host paints itself.
    #[must_use]
    pub fn is_mounted(&self, key: &str) -> bool {
        self.screens.contains_key(key)
    }

    /// The keys with a screen behind them, in roster order.
    pub fn mounted_keys(&self) -> impl Iterator<Item = &str> {
        self.destinations
            .keys()
            .filter(|key| self.screens.contains_key(*key))
    }

    /// The current screen's paint-root tag, when the journey is at a mounted
    /// destination.
    #[must_use]
    pub fn current_tag(&self, journey: &Journey) -> Option<&'static str> {
        self.screens.get(journey.at()).map(|s| s.tag())
    }

    /// The current screen's title — what the host publishes as the window's
    /// title while this screen is showing.
    ///
    /// The reference toolkit keeps a mounted window's title and shows it
    /// nowhere; measured at 6.11.1, the host window went on announcing its own
    /// name while a whole other application filled it.
    #[must_use]
    pub fn current_title(&self, journey: &Journey) -> Option<&'static str> {
        self.screens.get(journey.at()).map(|s| s.title())
    }

    /// ★★★★★ R1724 §2 #2 — the roster an agent reads, saying which of an
    /// application's destinations are **whole screens**.
    ///
    /// [`Destinations::wire`] answers what the rail contains and where the
    /// journey is; it cannot answer this, because a destination's page being
    /// another binding is a fact about the *pairing* rather than about the
    /// destination. Published rather than left to be inferred for the reason
    /// §2 #2 exists: an agent that has to guess whether a section is a screen
    /// guesses from the tag prefixes it happens to see, which is a rule nobody
    /// wrote down.
    ///
    /// Additive over [`Destinations::wire`]'s shape — each row gains `mounted`,
    /// and a mounted row gains the screen's own `tag` and `title`, which is
    /// what lets a client address that screen's surfaces at all.
    #[must_use]
    pub fn wire(&self, journey: &Journey) -> serde_json::Value {
        let mut value = self.destinations.wire(journey);
        if let Some(rows) = value
            .get_mut("destinations")
            .and_then(serde_json::Value::as_array_mut)
        {
            for row in rows {
                let key = row
                    .get("key")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let screen = self.screens.get(&key);
                row["mounted"] = serde_json::Value::Bool(screen.is_some());
                row["screen"] = screen.map_or(
                    serde_json::Value::Null,
                    |s| serde_json::json!({ "tag": s.tag(), "title": s.title() }),
                );
            }
        }
        value
    }

    /// Run `body` against the screen the journey is at, inside that screen's
    /// extent grant.
    ///
    /// **The only way to reach a screen.** Two properties come from that and
    /// neither is a convention:
    ///
    /// * a screen the journey is not at cannot be reached at all, so it cannot
    ///   take a press, answer a key, or appear in a tree;
    /// * every hook runs inside
    ///   [`with_surface_extent`], so a screen that hit-tests its own rectangles
    ///   reads the region it was placed in rather than the window it is inside
    ///   — including from the hooks the shell wraps in an owner scope, where
    ///   [`layout_size`](pinion_core::external::layout_size) would otherwise
    ///   answer the whole window.
    ///
    /// Answers `None` at an unmounted destination, which is a host's own page.
    pub fn with_current<R>(
        &self,
        journey: &Journey,
        body: impl FnOnce(&dyn Screen) -> R,
    ) -> Option<R> {
        let screen = self.screens.get(journey.at())?;
        let extent = self.placed_extent.get();
        if extent.0 == 0 || extent.1 == 0 {
            // Nothing has placed the region yet, so there is no rectangle to
            // grant and the pre-R1724 reading is the honest one.
            return Some(body(screen.as_ref()));
        }
        Some(with_surface_extent(screen.tag(), extent, || {
            body(screen.as_ref())
        }))
    }

    /// The current screen's scene, laid out in `extent` and given the recourse
    /// it declared for not fitting.
    ///
    /// Records `extent` as the region the screen is placed in, which is what
    /// every later [`Self::with_current`] grants — so the paint and the gesture
    /// halves of a mounted screen read one rectangle by construction.
    ///
    /// # ★★★★★ The region owes the screen its recourse
    ///
    /// A screen lays out at `max(extent, its own comfortable size)` — that is
    /// [`layout_size`](pinion_core::external::layout_size)'s rule and it does
    /// not change because the screen is a page. So a region smaller than the
    /// screen's layout minimum has content it cannot show, and what happens to
    /// that content is the screen's own declaration:
    /// [`pan`] is applied here for exactly the reason
    /// the shell applies it to a window.
    ///
    /// Measured on the first real mount, before this existed: the node lab,
    /// whose layout stops reflowing at 1625 wide, placed in a 1388-wide region,
    /// painted **51 of its regions outside that rectangle** — its inspector ran
    /// from x=1365 to x=1677 in a window that ends at 1440, so the pane a
    /// person configures a node with was off the screen with no way to reach
    /// it. The screen had declared `Recourse::Pan` since R1714 and the region
    /// was not listening.
    ///
    /// `pan` is the identity for a screen that fits, one that declares no
    /// policy, and one that clips — so this costs nothing where there is
    /// nothing to pan over.
    ///
    /// `frame` is passed through to the screen unchanged.
    #[must_use]
    pub fn page_scene(
        &self,
        journey: &Journey,
        extent: (u32, u32),
        frame: &Frame,
    ) -> Option<Scene> {
        if extent.0 > 0 && extent.1 > 0 {
            self.placed_extent.set(extent);
        }
        self.with_current(journey, |screen| {
            pan(
                screen.shrink_policy(),
                screen.tag(),
                extent,
                screen.view(frame),
            )
        })
    }

    /// The current screen's scene for a window of its own.
    #[must_use]
    pub fn window_scene(&self, journey: &Journey, window_id: &str, frame: &Frame) -> Option<Scene> {
        self.with_current(journey, |screen| screen.view_for_window(window_id, frame))
    }

    /// Read the current screen's projection out of the state scene, and report
    /// where the application is as one `Copy` value.
    ///
    /// This is a host's
    /// [`WidgetCore::read_state`](pinion_core::WidgetCore::read_state), and the
    /// only place [`Screen::latch`] is called from.
    #[must_use]
    pub fn latch(&self, journey: &Journey, state_scene: &Scene) -> ScreenState {
        let at = self
            .destinations
            .keys()
            .position(|key| key == journey.at())
            .unwrap_or(0);
        let revision = self
            .with_current(journey, |screen| screen.latch(state_scene))
            .unwrap_or(0);
        ScreenState {
            at: u32::try_from(at).unwrap_or(u32::MAX),
            revision,
        }
    }

    /// The externals that are live while the journey is where it is: the
    /// current screen's, and nobody else's.
    ///
    /// This is the whole of "a screen you are not at is not routable". There is
    /// no filtering step and no visibility flag — an external that is not in
    /// the returned list is not in the state scene, so the §5.35 router has
    /// nothing to resolve a press to and the wire has no slot to address.
    #[must_use]
    pub fn externals(&self, journey: &Journey) -> Vec<ExtraExternal> {
        self.with_current(journey, |screen| screen.externals())
            .unwrap_or_default()
    }
}
