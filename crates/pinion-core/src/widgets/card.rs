//! R1648 §5.21 — a dashboard **card**: what its header offers, and what its
//! body is currently showing.
//!
//! [`TileGrid`](crate::widgets::tile_grid::TileGrid) says *where* a card sits.
//! Nothing said what a card **is**, so every application that wanted the shape
//! a monitoring dashboard has — a titled panel with a few header affordances,
//! whose body is sometimes a chart and sometimes an explanation of why there is
//! no chart — assembled it out of boxes and labels, once per card kind.
//!
//! Two vocabularies close that, and they are independent on purpose: a card's
//! header does not change when its body has nothing to show.
//!
//! # The header: one card type that offers all four affordances
//!
//! A mature toolkit splits this across two class hierarchies that cannot be
//! combined. Its dock widget publishes exactly three capabilities — closable,
//! movable, floatable — plus a *presentation* option (a vertical title bar)
//! mixed into the same flag set; its MDI child publishes minimise, maximise and
//! close but cannot dock. So a card that both tears off **and** maximises is
//! not expressible there: the application picks a base class and gives up the
//! other half.
//!
//! [`CardChrome`] is one set over [`CardAffordance`], which is four values and
//! no presentation option:
//!
//! * the order buttons appear in is the enum's declaration order, so two cards
//!   in two applications lay their headers out the same way — where the toolkit
//!   leaves the order to the platform style and it differs between them;
//! * a set is [enumerable](CardChrome::offered) rather than a bitmask a caller
//!   must test one flag at a time against;
//! * [`CardAffordance::TearOff`] is the one that leaves the board, and it is
//!   named as the trigger of the existing
//!   [`DockPanelPolicy`](crate::widgets::dock_panel::DockPanelPolicy)
//!   lifecycle rather than being a second float model beside it.
//!
//! # The body: a state that says what can be done about it
//!
//! The capability list this axis is judged against asks a widget to have
//! loading, empty and error states, a no-permission state, and a state for a
//! link whose content is encrypted and therefore unavailable. Measured on the
//! toolkit at 6.11: **there is no such concept at all** — no content-state
//! enumeration on any panel or view class, and its item views have no
//! placeholder of any kind. The nearest thing it ships is a busy *indicator*
//! widget, which is a spinner the application positions itself.
//!
//! Left to the application that is six paint paths per card kind, so twelve
//! widget kinds is seventy-two of them, each free to disagree about whether an
//! encrypted link deserves a retry button. [`CardState`] is those six as one
//! value, and the reason it is worth being a type rather than a `String` is
//! [`CardState::remedy`]:
//!
//! | state | what the person can do |
//! |---|---|
//! | [`Ready`](CardState::Ready) | nothing is wrong, so there is no remedy — `None` |
//! | [`Loading`](CardState::Loading) | [`Wait`](Remedy::Wait) |
//! | [`Empty`](CardState::Empty) | [`Widen`](Remedy::Widen) — the filter excluded everything |
//! | [`Failed`](CardState::Failed) | [`Retry`](Remedy::Retry) |
//! | [`Denied`](CardState::Denied) | [`Authorize`](Remedy::Authorize) — someone can grant it |
//! | [`Opaque`](CardState::Opaque) | [`Nothing`](Remedy::Nothing) — nobody can show it |
//!
//! The last two rows are why the vocabulary has six arms rather than one
//! `Error`. A denial and an encrypted link both render as "no content", and
//! they are opposite: one is a permission somebody holds and the other is
//! arithmetic. A card that offers "request access" on an encrypted link is
//! lying to the person reading it.
//!
//! `remedy` returning `Option` makes that a three-way rather than a two-way
//! (R1610): *no remedy is needed*, *a remedy exists*, and *a remedy is needed
//! and there is none*. The last is [`Remedy::Nothing`], and it is a value
//! precisely so that a shell can render it — "this cannot be shown" is the
//! honest thing to say, and the shape that has no word for it says nothing.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::widgets::tile_grid::TileId;

/// What a card's header offers.
///
/// Closed, and enumerable through [`CardAffordance::ALL`] — whose length the
/// build checks against the enum's own arm count, so an arm added here without
/// being listed there fails to compile rather than silently narrowing every
/// consumer that walks the vocabulary (R1630).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    pinion_derive::VariantCensus,
)]
#[serde(rename_all = "snake_case")]
#[variant_census(all)]
pub enum CardAffordance {
    /// Open this card's own configuration.
    ///
    /// The toolkit's dock widget has no equivalent: a per-panel settings
    /// affordance there is a custom title-bar widget the application supplies.
    Settings,
    /// Detach the card into its own window.
    ///
    /// The trigger of [`DockPanelEvent::Escaped`](crate::widgets::dock_panel::DockPanelEvent::Escaped)
    /// — this names that lifecycle rather than starting a second one.
    TearOff,
    /// Fill the board with this card, hiding the rest.
    ///
    /// A toggle, and the way back is [`Maximized`](crate::widgets::tile_grid::Maximized),
    /// which is the arrangement the board had before.
    Maximize,
    /// Remove the card from the board.
    Close,
}

impl CardAffordance {
    /// Every affordance, in the order a header lays them out.
    pub const ALL: [Self; 4] = [Self::Settings, Self::TearOff, Self::Maximize, Self::Close];

    /// This affordance's wire spelling.
    ///
    /// Derived from the definition by exhaustive match rather than written out
    /// as a table beside it: a table is a census of the type, and a census of a
    /// type is the thing the type should answer (R1638).
    #[must_use]
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Settings => "settings",
            Self::TearOff => "tear_off",
            Self::Maximize => "maximize",
            Self::Close => "close",
        }
    }

    /// The affordance that wire spelling names.
    ///
    /// The inverse of [`wire`](Self::wire). Both directions exist so that what
    /// the schema publishes is exactly what an `invoke` accepts — a vocabulary
    /// published without its parser is two definitions of one set (R1642).
    #[must_use]
    pub fn from_wire(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.wire() == word)
    }

    /// Whether acting on this affordance takes the card off the board.
    ///
    /// [`TearOff`](Self::TearOff) and [`Close`](Self::Close) both do, and the
    /// difference between them is whether the card still exists afterwards —
    /// which is why a board cannot treat "gone from the grid" as "gone".
    #[must_use]
    pub const fn leaves_the_board(self) -> bool {
        matches!(self, Self::TearOff | Self::Close)
    }
}

/// The set of affordances one card's header offers.
///
/// Normalised on construction: declaration order, no repeats. The order is the
/// point — a set built from `[Close, Settings]` and one built from
/// `[Settings, Close]` are the same header, and two applications that spell
/// their card definitions differently still lay out identically.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "Vec<CardAffordance>", into = "Vec<CardAffordance>")]
pub struct CardChrome {
    /// One slot per [`CardAffordance::ALL`] entry, in that order.
    offered: [bool; CardAffordance::ARMS],
}

impl CardChrome {
    /// A header with no affordances — a card that can only be read.
    #[must_use]
    pub const fn bare() -> Self {
        Self {
            offered: [false; CardAffordance::ARMS],
        }
    }

    /// A header offering every affordance.
    ///
    /// Expressible in one value here, and in the toolkit only by choosing which
    /// half to lose (see the module docs).
    #[must_use]
    pub const fn full() -> Self {
        Self {
            offered: [true; CardAffordance::ARMS],
        }
    }

    /// A header offering exactly these, in whatever order they were given.
    #[must_use]
    pub fn of(affordances: impl IntoIterator<Item = CardAffordance>) -> Self {
        let mut chrome = Self::bare();
        for affordance in affordances {
            chrome.offered[Self::slot(affordance)] = true;
        }
        chrome
    }

    /// Which slot an affordance occupies.
    ///
    /// Derived from [`CardAffordance::ALL`], which is the single statement of
    /// the layout order — a second table here would be a second definition of
    /// it. The lookup cannot miss: the build checks `ALL` against the enum's
    /// arm count and this module's tests check its entries are distinct, so
    /// `ALL` is a bijection onto the vocabulary.
    fn slot(affordance: CardAffordance) -> usize {
        CardAffordance::ALL
            .iter()
            .position(|a| *a == affordance)
            .expect("CardAffordance::ALL covers the vocabulary; the build checks its length")
    }

    /// This header, plus that affordance.
    #[must_use]
    pub fn with(mut self, affordance: CardAffordance) -> Self {
        self.offered[Self::slot(affordance)] = true;
        self
    }

    /// This header, minus that affordance.
    #[must_use]
    pub fn without(mut self, affordance: CardAffordance) -> Self {
        self.offered[Self::slot(affordance)] = false;
        self
    }

    /// Whether the header offers it.
    #[must_use]
    pub fn offers(&self, affordance: CardAffordance) -> bool {
        self.offered[Self::slot(affordance)]
    }

    /// What the header offers, in layout order.
    #[must_use]
    pub fn offered(&self) -> Vec<CardAffordance> {
        CardAffordance::ALL
            .into_iter()
            .filter(|a| self.offers(*a))
            .collect()
    }

    /// How many affordances the header offers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.offered.iter().filter(|on| **on).count()
    }

    /// Whether the header offers nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl From<Vec<CardAffordance>> for CardChrome {
    fn from(affordances: Vec<CardAffordance>) -> Self {
        Self::of(affordances)
    }
}

impl From<CardChrome> for Vec<CardAffordance> {
    fn from(chrome: CardChrome) -> Self {
        chrome.offered()
    }
}

/// What a card's body is currently showing.
///
/// Six values, five of which are reasons there is no content. The vocabulary is
/// the capability list's, and the split between the last two is the one a
/// single `Error` arm loses — see the module docs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, pinion_derive::VariantCensus)]
#[serde(rename_all = "snake_case", tag = "state", content = "detail")]
#[variant_census(all)]
pub enum CardState {
    /// The content is here.
    Ready,
    /// The content is on its way.
    Loading,
    /// It arrived and there is none — the query matched nothing.
    Empty,
    /// It did not arrive. The reason is particular to this attempt, so it is
    /// carried.
    Failed(Cow<'static, str>),
    /// This viewer is not permitted to see it. Which right is missing is
    /// particular, so it is carried — a denial the person cannot name is one
    /// they cannot act on.
    Denied(Cow<'static, str>),
    /// The link's content is encrypted, so there is nothing to show and no
    /// permission that would change that.
    ///
    /// Carries nothing: unlike a failure or a denial, the explanation is the
    /// same every time.
    Opaque,
}

impl CardState {
    /// One representative of every arm, for a consumer that must cover the
    /// vocabulary. The two carried arms get an empty reason, which is the only
    /// thing a representative can say — a stand-in that invented a plausible
    /// message would read as a real one in a test's failure output.
    ///
    /// Its length is checked against the enum's arm count by the build
    /// (`#[variant_census(all)]`), so an arm added above without a
    /// representative here does not compile.
    pub const ALL: [Self; 6] = [
        Self::Ready,
        Self::Loading,
        Self::Empty,
        Self::Failed(Cow::Borrowed("")),
        Self::Denied(Cow::Borrowed("")),
        Self::Opaque,
    ];

    /// This state's wire spelling — the discriminant alone, without the detail.
    #[must_use]
    pub const fn wire(&self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Loading => "loading",
            Self::Empty => "empty",
            Self::Failed(_) => "failed",
            Self::Denied(_) => "denied",
            Self::Opaque => "opaque",
        }
    }

    /// The particular explanation this state carries, if its arm carries one.
    ///
    /// `None` for the four arms whose meaning is complete without one, which is
    /// a different answer from `Some("")` — a failure that reports no reason is
    /// a failure whose reason was lost.
    #[must_use]
    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Failed(why) | Self::Denied(why) => Some(why),
            Self::Ready | Self::Loading | Self::Empty | Self::Opaque => None,
        }
    }

    /// Whether the card has content to paint.
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }

    /// What the person looking at this card can do about it.
    ///
    /// `None` for [`Ready`](Self::Ready), because nothing is wrong. Every other
    /// arm answers, and the answers are distinct — see the module docs for why
    /// [`Denied`](Self::Denied) and [`Opaque`](Self::Opaque) must not collapse.
    #[must_use]
    pub const fn remedy(&self) -> Option<Remedy> {
        match self {
            Self::Ready => None,
            Self::Loading => Some(Remedy::Wait),
            Self::Empty => Some(Remedy::Widen),
            Self::Failed(_) => Some(Remedy::Retry),
            Self::Denied(_) => Some(Remedy::Authorize),
            Self::Opaque => Some(Remedy::Nothing),
        }
    }
}

/// What a person can do about a card that is not showing content.
///
/// A projection of [`CardState`] into the reader's vocabulary, derived once
/// here so that twelve widget kinds do not each decide whether an encrypted
/// link deserves a retry button.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    pinion_derive::VariantCensus,
)]
#[serde(rename_all = "snake_case")]
#[variant_census(all)]
pub enum Remedy {
    /// It is coming. The card's own progress is the affordance.
    Wait,
    /// Ask again — the attempt failed and another one may not.
    Retry,
    /// Loosen the filter: the content exists and this view excluded it.
    Widen,
    /// Obtain the right. Somebody can grant it; this viewer does not hold it.
    Authorize,
    /// Nothing, and saying so is the point.
    ///
    /// Reached only from [`CardState::Opaque`]. A shell renders this as an
    /// explanation with no control beside it, which is the honest shape and the
    /// one a vocabulary without this word cannot express.
    Nothing,
}

impl Remedy {
    /// Every remedy. Every one of them is reachable from some
    /// [`CardState`] — asserted in this module's tests, because a remedy no
    /// state produces is dead vocabulary a shell would paint a control for
    /// (R1629).
    pub const ALL: [Self; 5] = [
        Self::Wait,
        Self::Retry,
        Self::Widen,
        Self::Authorize,
        Self::Nothing,
    ];

    /// This remedy's wire spelling.
    #[must_use]
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Wait => "wait",
            Self::Retry => "retry",
            Self::Widen => "widen",
            Self::Authorize => "authorize",
            Self::Nothing => "nothing",
        }
    }

    /// Whether the person is expected to act.
    ///
    /// False for [`Wait`](Self::Wait) (the card acts) and
    /// [`Nothing`](Self::Nothing) (nobody can), true for the three that are a
    /// request to the person. This is what a shell keys the presence of a
    /// control on, rather than each card kind guessing.
    #[must_use]
    pub const fn is_actionable(self) -> bool {
        matches!(self, Self::Retry | Self::Widen | Self::Authorize)
    }
}

/// A titled panel on a board: an identity, a header, and a body state.
///
/// The identity is a [`TileId`] rather than a type of its own. A card's
/// identity and its placement's identity are **one fact** — the grid is where
/// uniqueness is enforced — and a second id type would need a bijection nobody
/// checks (R1631).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    id: TileId,
    title: String,
    chrome: CardChrome,
    state: CardState,
}

impl Card {
    /// A card with that id and title, offering nothing and showing content.
    #[must_use]
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: TileId::new(id),
            title: title.into(),
            chrome: CardChrome::bare(),
            state: CardState::Ready,
        }
    }

    /// This card, with that header.
    #[must_use]
    pub fn with_chrome(mut self, chrome: CardChrome) -> Self {
        self.chrome = chrome;
        self
    }

    /// This card, showing that.
    #[must_use]
    pub fn with_state(mut self, state: CardState) -> Self {
        self.state = state;
        self
    }

    /// The card's identity, which is also its tile's.
    #[must_use]
    pub const fn id(&self) -> &TileId {
        &self.id
    }

    /// The card's title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The card's header.
    #[must_use]
    pub const fn chrome(&self) -> &CardChrome {
        &self.chrome
    }

    /// What the card is showing.
    #[must_use]
    pub const fn state(&self) -> &CardState {
        &self.state
    }

    /// Show something else.
    pub fn set_state(&mut self, state: CardState) {
        self.state = state;
    }

    /// What the person can do about this card, if anything.
    #[must_use]
    pub const fn remedy(&self) -> Option<Remedy> {
        self.state.remedy()
    }
}

#[cfg(test)]
mod tests {
    use super::{Card, CardAffordance, CardChrome, CardState, Remedy};

    #[test]
    fn r1648_the_affordance_vocabulary_counts_its_own_arms() {
        // The `ALL`-length assertion is a build gate (`#[variant_census(all)]`),
        // so this only has to state the thing the length cannot: the entries
        // are DISTINCT. A list of the right length naming one arm twice passes
        // a length check and covers less than it claims — the hole R1643 left
        // and R1644 closed by construction elsewhere.
        let mut seen = std::collections::BTreeSet::new();
        for affordance in CardAffordance::ALL {
            assert!(
                seen.insert(affordance),
                "{affordance:?} appears twice in CardAffordance::ALL"
            );
        }
        assert_eq!(seen.len(), CardAffordance::ARMS);
    }

    #[test]
    fn r1648_every_affordance_wire_word_round_trips_and_is_distinct() {
        // Publishing a vocabulary without its parser is two definitions of one
        // set: what the schema advertises must be what an invoke accepts.
        let mut seen = std::collections::BTreeSet::new();
        for affordance in CardAffordance::ALL {
            let word = affordance.wire();
            assert!(seen.insert(word), "{word} spells two affordances");
            assert_eq!(
                CardAffordance::from_wire(word),
                Some(affordance),
                "{word} does not parse back"
            );
        }
        assert_eq!(CardAffordance::from_wire("float"), None, "not in the set");
    }

    #[test]
    fn r1648_a_header_lays_out_in_declaration_order_whatever_order_it_was_built() {
        // The property the toolkit's flag set does not have: two applications
        // that spell their card definitions differently still lay out the same.
        let forward = CardChrome::of([CardAffordance::Settings, CardAffordance::Close]);
        let backward = CardChrome::of([CardAffordance::Close, CardAffordance::Settings]);
        assert_eq!(forward, backward, "a set has no order of its own");
        assert_eq!(
            forward.offered(),
            vec![CardAffordance::Settings, CardAffordance::Close],
            "and it reports declaration order"
        );
    }

    #[test]
    fn r1648_a_repeated_affordance_is_one_affordance() {
        let chrome = CardChrome::of([
            CardAffordance::Close,
            CardAffordance::Close,
            CardAffordance::Close,
        ]);
        assert_eq!(chrome.len(), 1);
        assert_eq!(chrome.offered(), vec![CardAffordance::Close]);
    }

    #[test]
    fn r1648_one_card_offers_tear_off_and_maximize_at_once() {
        // The claim this module makes against the toolkit, as an assertion:
        // there, floating lives on the dock widget and maximising on the MDI
        // child, and no class has both. Here it is one value.
        let chrome = CardChrome::full();
        assert!(chrome.offers(CardAffordance::TearOff));
        assert!(chrome.offers(CardAffordance::Maximize));
        assert_eq!(chrome.len(), CardAffordance::ARMS, "all four, on one card");
    }

    #[test]
    fn r1648_without_removes_and_with_adds() {
        let chrome = CardChrome::full().without(CardAffordance::Close);
        assert!(!chrome.offers(CardAffordance::Close));
        assert_eq!(chrome.len(), CardAffordance::ARMS - 1);
        let back = chrome.with(CardAffordance::Close);
        assert_eq!(back, CardChrome::full());
    }

    #[test]
    fn r1648_a_bare_header_is_empty_and_a_full_one_is_not() {
        assert!(CardChrome::bare().is_empty());
        assert!(!CardChrome::full().is_empty());
    }

    #[test]
    fn r1648_a_header_round_trips_through_its_wire_form_normalised() {
        // The serde form is a list, and a hostile or hand-edited list is
        // normalised on the way in rather than trusted — the shape R1412 gave
        // the dock topology, for the same reason: a preset is a stored value.
        let json = serde_json::to_string(&CardChrome::full()).expect("serialize");
        let back: CardChrome = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, CardChrome::full());

        let scrambled: CardChrome =
            serde_json::from_str(r#"["close","close","settings"]"#).expect("deserialize");
        assert_eq!(
            scrambled.offered(),
            vec![CardAffordance::Settings, CardAffordance::Close],
            "a stored list is normalised, not trusted"
        );
    }

    #[test]
    fn r1648_the_state_vocabulary_is_total_over_remedies() {
        // Totality: every state answers `remedy`. `Ready` answers None, which
        // is an answer — the three-way absent/none/value (R1610).
        assert_eq!(CardState::ALL.len(), CardState::ARMS, "ALL covers the enum");
        let mut answered = 0;
        for state in CardState::ALL {
            if state.is_ready() {
                assert_eq!(state.remedy(), None, "nothing is wrong with a ready card");
            } else {
                assert!(state.remedy().is_some(), "{state:?} owes a remedy");
                answered += 1;
            }
        }
        assert_eq!(answered, CardState::ARMS - 1);
    }

    #[test]
    fn r1648_every_remedy_is_reachable_from_some_state() {
        // Surjectivity. A remedy no state produces is dead vocabulary, and a
        // shell would paint a control for it that nothing can ever show
        // (R1629's dead candle caps, in a second place).
        let reached: std::collections::BTreeSet<_> = CardState::ALL
            .iter()
            .filter_map(CardState::remedy)
            .collect();
        assert_eq!(
            reached.len(),
            Remedy::ARMS,
            "unreachable remedies: {:?}",
            Remedy::ALL
                .into_iter()
                .filter(|r| !reached.contains(r))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn r1648_a_denial_and_an_encrypted_link_do_not_share_a_remedy() {
        // The distinction the whole six-arm vocabulary exists for. Collapsing
        // them into one `Error` arm makes a shell offer "request access" on a
        // link no permission can open.
        let denied = CardState::Denied("read scope".into());
        let opaque = CardState::Opaque;
        assert_eq!(denied.remedy(), Some(Remedy::Authorize));
        assert_eq!(opaque.remedy(), Some(Remedy::Nothing));
        assert!(
            denied.remedy().is_some_and(Remedy::is_actionable),
            "somebody can grant it"
        );
        assert!(
            !opaque.remedy().is_some_and(Remedy::is_actionable),
            "nobody can decrypt it"
        );
    }

    #[test]
    fn r1648_only_the_particular_states_carry_a_detail() {
        // `None` and `Some("")` are different answers: a failure that reports
        // no reason is a failure whose reason was lost, and the four arms whose
        // meaning is complete without one must not fake having one.
        assert_eq!(
            CardState::Failed("timeout".into()).detail(),
            Some("timeout")
        );
        assert_eq!(
            CardState::Denied("read scope".into()).detail(),
            Some("read scope")
        );
        for state in [
            CardState::Ready,
            CardState::Loading,
            CardState::Empty,
            CardState::Opaque,
        ] {
            assert_eq!(state.detail(), None, "{state:?} explains itself");
        }
    }

    #[test]
    fn r1648_every_state_wire_word_is_distinct() {
        let mut seen = std::collections::BTreeSet::new();
        for state in CardState::ALL {
            assert!(
                seen.insert(state.wire()),
                "{} spells two states",
                state.wire()
            );
        }
        assert_eq!(seen.len(), CardState::ARMS);
    }

    #[test]
    fn r1648_every_remedy_wire_word_is_distinct() {
        let mut seen = std::collections::BTreeSet::new();
        for remedy in Remedy::ALL {
            assert!(seen.insert(remedy.wire()), "{} spells two", remedy.wire());
        }
        assert_eq!(seen.len(), Remedy::ARMS);
    }

    #[test]
    fn r1648_a_card_round_trips_through_its_wire_form() {
        let card = Card::new("latency", "Latency distribution")
            .with_chrome(CardChrome::full().without(CardAffordance::Close))
            .with_state(CardState::Failed("collector unreachable".into()));
        let json = serde_json::to_string(&card).expect("serialize");
        let back: Card = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, card);
        assert_eq!(back.remedy(), Some(Remedy::Retry));
        assert_eq!(back.id().as_str(), "latency");
    }

    #[test]
    fn r1648_the_two_affordances_that_leave_the_board_differ_in_survival() {
        // A board cannot read "gone from the grid" as "gone": a torn-off card
        // still exists and a closed one does not.
        assert!(CardAffordance::TearOff.leaves_the_board());
        assert!(CardAffordance::Close.leaves_the_board());
        assert!(!CardAffordance::Settings.leaves_the_board());
        assert!(!CardAffordance::Maximize.leaves_the_board());
    }
}
