//! R1651 — the lab's node taxonomy, over the crate's graph model.
//!
//! The document, the links, the frames and the reachability all come from
//! `pinion_node_graph::Document`. What this module supplies is the *taxonomy* —
//! which roles exist, what pins each one has, and what a pin carries — which is
//! precisely the part the crate declines to own, and the census row for the
//! node palette says so: *"which roles exist is the application's"*.
//!
//! A pin here is a transport endpoint, and the type relation is the one the
//! reference draws in its legend: a link may be authored from a **dial** pin to
//! an **accept** pin of the same transport. That single rule is what makes the
//! canvas's three pin appearances mean something rather than decorate.

use pinion_node_graph::{
    Admission, Admits, Composition, Conversion, Drawn, NodeKind, Objection, Port, PortName,
    PortRef, Refusal, Side, Tint, Variadic,
};
use serde::{Deserialize, Serialize};

/// A transport a link can be carried over.
///
/// The socket type: two pins may be wired when they agree on it, which is why
/// the reference colours an accept pin by protocol — the colour *is* the type.
/// ★ R1689 — serialisable, with [`Role`] and [`LabNode`]: a document that can
/// be saved is one whose taxonomy can be, and the taxonomy is the half the
/// substrate declines to own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Transport {
    /// Plain stream transport.
    Tcp,
    /// Stream transport with transport-layer security.
    Tls,
    /// Multiplexed datagram transport.
    Quic,
    /// Bare datagram transport.
    Udp,
    /// Stream transport tunnelled over a web socket.
    Ws,
}

impl Transport {
    /// Every transport, in the order the palette's legend lists them.
    pub const ALL: [Self; 5] = [Self::Tcp, Self::Tls, Self::Quic, Self::Udp, Self::Ws];

    /// ★★★★★ R1926 — the colour this transport is drawn in.
    ///
    /// Moved here from the screen, which is what closing the reference's pin
    /// colour rows forced: the colour is a fact about the **socket type**, so
    /// it belongs beside the taxonomy that owns the type. `transport_ink` in
    /// the view now derives its `Color` from this, so the canvas and
    /// [`NodeKind::type_colour`]
    /// cannot answer differently.
    #[must_use]
    pub const fn tint(self) -> Tint {
        match self {
            Self::Tcp => Tint::rgb(0x2D, 0x6C, 0xDF),
            Self::Tls => Tint::rgb(0x1F, 0x8A, 0x4C),
            Self::Quic => Tint::rgb(0x7C, 0x4D, 0xEF),
            Self::Udp => Tint::rgb(0xC7, 0x78, 0x00),
            Self::Ws => Tint::rgb(0x3E, 0x7C, 0x8C),
        }
    }

    /// The word the legend and a locator both spell it with.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Tls => "tls",
            Self::Quic => "quic",
            Self::Udp => "udp",
            Self::Ws => "ws",
        }
    }

    /// The transport a locator names, or `None` when it names none.
    #[must_use]
    pub fn of_locator(locator: &str) -> Option<Self> {
        let (scheme, _) = locator.split_once('/')?;
        Self::ALL.into_iter().find(|t| t.word() == scheme)
    }
}

/// ★★★★★ R1914 — **what a pin carries**: a whole locator, or one half of one.
///
/// The socket type, and it grew an inside this round because the split ACT
/// needed one. A locator on this screen is already two things — the model says
/// so itself, in [`Transport::of_locator`], which splits one on the `/` — and
/// until now nothing could ask *what is this made of*, so a pin was an atom and
/// the split question answered `atom` on every card.
///
/// [`Transport`] is kept whole rather than folded in here: it is what the
/// palette's legend lists and what the canvas colours a pin by, and a legend
/// that had to skip two of its own entries would be a legend that lies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Endpoint {
    /// A whole locator over this transport — `scheme/host:service`.
    Locator(Transport),
    /// The host half of a locator: where to reach it.
    Host,
    /// The service half: which port on that host.
    Service,
}

impl Endpoint {
    /// ★★★★★ R1960 — **the socket type a written locator carries**, or `None`
    /// when the string names no transport this taxonomy has.
    ///
    /// # Why this exists, and why it refuses instead of defaulting
    ///
    /// Two sites built `Endpoint::Locator(Transport::of_locator(one).unwrap_or(
    /// Transport::Tcp))` — growing a pin for a link that dials an address, and
    /// landing one on an existing link — so the same string was read into a
    /// type twice, with the same escape hatch written twice. **A default is a
    /// classification nobody made**: a locator with no scheme is not TCP, it is
    /// a locator this screen cannot type, and answering `Tcp` gives the canvas
    /// a colour to draw a pin by that no address supports.
    ///
    /// So this answers `Option` and the callers say what an unreadable address
    /// means to them — which for both of them is *the pin carries no type*, the
    /// same thing they already do for `endpoint: None`.
    ///
    /// ⚠ Part of the count `debt-every-card-on-the-opening-graph-speaks-one-
    /// transport` records: a node's transport was decided at FIVE sites, and
    /// the two this replaces are the pair that read it out of a string.
    #[must_use]
    pub fn of_written_locator(locator: &str) -> Option<Self> {
        Transport::of_locator(locator).map(Self::Locator)
    }

    /// The transport this endpoint speaks, or `None` for a half of one.
    ///
    /// A half carries no transport, and that is not an omission: a host name is
    /// the same host name whether it is dialled over a stream or a datagram, so
    /// giving the halves a transport would have invented a fact for the canvas
    /// to colour a pin by wrongly.
    #[must_use]
    pub const fn transport(self) -> Option<Transport> {
        match self {
            Self::Locator(transport) => Some(transport),
            Self::Host | Self::Service => None,
        }
    }

    /// ★★★★★ R1926 — every socket type this taxonomy has, **derived** from
    /// [`Transport::ALL`] plus the two halves.
    ///
    /// Derived rather than listed, so a transport added later joins every
    /// register built from this without anyone remembering to extend a second
    /// list. A hand-written roster is the shape whose omissions are invisible,
    /// which is the escape hatch this workspace refuses at the door.
    #[must_use]
    pub fn all() -> Vec<Self> {
        let mut out: Vec<Self> = Transport::ALL.into_iter().map(Self::Locator).collect();
        out.push(Self::Host);
        out.push(Self::Service);
        out
    }

    /// The one spelling a client reads this type under.
    #[must_use]
    pub fn wire_word(self) -> String {
        match self {
            Self::Locator(transport) => format!("locator/{}", transport.word()),
            Self::Host => "host".to_owned(),
            Self::Service => "service".to_owned(),
        }
    }
}

/// What a node is for.
///
/// The two groups are the reference's: infrastructure that carries traffic, and
/// the traffic itself. Which group a role is in is [`Role::group`], derived
/// rather than stored beside the palette, so a role cannot be listed in one
/// group and behave like the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    /// Listens and routes between everything that dials it.
    Router,
    /// Joins the mesh as an equal.
    Peer,
    /// Depends on exactly one router.
    Client,
    /// Keeps what it is told, over a range of keys.
    Store,
    /// Sends on a key, at a rate.
    Publisher,
    /// Receives on a key pattern.
    Subscriber,
    /// Asks on a period.
    Querier,
    /// Answers what a querier asks.
    Responder,
}

impl Role {
    /// Every role, in palette order.
    pub const ALL: [Self; 8] = [
        Self::Router,
        Self::Peer,
        Self::Client,
        Self::Store,
        Self::Publisher,
        Self::Subscriber,
        Self::Querier,
        Self::Responder,
    ];

    /// The role's name, which is what the palette shows.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Router => "Router",
            Self::Peer => "Peer",
            Self::Client => "Client",
            Self::Store => "Store",
            Self::Publisher => "Publisher",
            Self::Subscriber => "Subscriber",
            Self::Querier => "Querier",
            Self::Responder => "Responder",
        }
    }

    /// The three-letter badge the canvas card carries.
    #[must_use]
    pub const fn badge(self) -> &'static str {
        match self {
            Self::Router => "RTR",
            Self::Peer => "PEER",
            Self::Client => "CLI",
            Self::Store => "STO",
            Self::Publisher => "PUB",
            Self::Subscriber => "SUB",
            Self::Querier => "QRY",
            Self::Responder => "RSP",
        }
    }

    /// Which palette group this role sits in.
    #[must_use]
    pub const fn group(self) -> &'static str {
        match self {
            Self::Router | Self::Peer | Self::Client | Self::Store => "infrastructure",
            Self::Publisher | Self::Subscriber | Self::Querier | Self::Responder => "traffic",
        }
    }

    /// The one line the palette has room for.
    #[must_use]
    pub const fn gist(self) -> &'static str {
        match self {
            Self::Router => "listens, routes",
            Self::Peer => "joins the mesh",
            Self::Client => "one router only",
            Self::Store => "volume, key range",
            Self::Publisher => "sends, with a class",
            Self::Subscriber => "receives",
            Self::Querier => "asks, on a period",
            Self::Responder => "answers",
        }
    }

    /// Whether a node of this role can be **dialled** — whether it is the sort
    /// of thing that listens at all.
    ///
    /// Not the same question as whether a *particular* node is reachable: a
    /// role that can listen and has no endpoint configured shows the closed
    /// pin, which is the warning the reference's gate raises. This is the
    /// role's half of that, and the node's half lives in its form.
    #[must_use]
    pub const fn accepts(self) -> bool {
        match self {
            Self::Router | Self::Peer | Self::Store | Self::Responder | Self::Subscriber => true,
            Self::Client | Self::Publisher | Self::Querier => false,
        }
    }

    /// The session mode this role implies, when it implies one.
    ///
    /// ★★★ R1716 — the value the inspector's `mode` row is **worked out
    /// from**, and the reason that row is derived rather than typed: a router
    /// that ran in client mode would not be the node the canvas draws. The
    /// behaviour canon holds the same map for the same four roles and treats
    /// its absence as the definition of a traffic node.
    ///
    /// `None` is not "no mode" — it is *this role does not decide one*, and the
    /// screen then shows the mode a traffic node comes up in, worked out from
    /// the example programs rather than from the role.
    #[must_use]
    pub const fn mode(self) -> Option<&'static str> {
        match self {
            Self::Router => Some("router"),
            Self::Peer | Self::Store => Some("peer"),
            Self::Client => Some("client"),
            Self::Publisher | Self::Subscriber | Self::Querier | Self::Responder => None,
        }
    }

    /// Every mode a session can be in — the options the `mode` row offers a
    /// person who takes it over.
    pub const MODES: [&'static str; 3] = ["router", "peer", "client"];

    /// The role by name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.name() == name)
    }
}

/// The taxonomy this lab authors graphs in.
///
/// One arm carrying a [`Role`] rather than eight kinds: every node in this tool
/// is a process with the same two pins, and what differs is what it does with
/// them. The reference's palette is the same shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabNode {
    /// What this node is for.
    pub role: Role,
    /// The transport its pins speak, taken from its configured endpoint.
    pub transport: Transport,
    /// Whether *this* node has somewhere to listen. A role that accepts and a
    /// node with no endpoint is the closed pin.
    pub listening: bool,
    /// Which codebase this node runs, and the wire revisions it speaks (R1885).
    ///
    /// ★★★★★ The axis that makes this a **compatibility test graph** rather
    /// than a drawing of one deployment. Every node here used to be implicitly
    /// the same implementation, so "will these two actually talk?" had nowhere
    /// to be asked — and the question is the whole reason an analyst builds a
    /// graph of peers in the first place.
    ///
    /// It is a field of the node and not a row of its configuration form,
    /// because the form's rows are paths of the thing being *configured* and
    /// this is a fact about which program is running at all. A person does not
    /// set it by editing a config file; they set it by deploying a different
    /// build.
    #[serde(default)]
    pub implementation: Implementation,
}

/// Which codebase a node runs, and what it can speak to (R1885).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Implementation {
    /// The codebase.
    pub stack: Stack,
    /// The wire revisions it can negotiate, inclusive at both ends.
    pub speaks: Revisions,
}

impl Default for Implementation {
    /// The reference build at the revisions this lab's opening graph negotiates.
    ///
    /// A default exists so that a node authored without thinking about this
    /// axis is the ordinary case rather than an incompatible one — the axis
    /// must not make a graph harder to draw until somebody uses it.
    fn default() -> Self {
        Self {
            stack: Stack::Reference,
            speaks: Revisions::new(6, 8),
        }
    }
}

impl Implementation {
    /// Whether these two can negotiate a revision they both speak.
    ///
    /// Overlap of the inclusive ranges — the honest model, because a peer
    /// announces a span and the pair settles on one they share. A single
    /// version number would make every unequal pair incompatible, which is not
    /// how a protocol that ships more than once behaves.
    #[must_use]
    pub const fn negotiates_with(self, other: Self) -> bool {
        self.speaks.first <= other.speaks.last && other.speaks.first <= self.speaks.last
    }
}

/// A codebase a node can be running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Stack {
    /// The protocol's own reference build.
    Reference,
    /// An independent re-implementation.
    Independent,
    /// An older release still deployed in the field.
    Legacy,
}

impl Stack {
    /// Every stack, in the order the inspector lists them.
    pub const ALL: [Self; 3] = [Self::Reference, Self::Independent, Self::Legacy];

    /// The word the inspector spells it with.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Reference => "reference",
            Self::Independent => "independent",
            Self::Legacy => "legacy",
        }
    }

    /// The stack by name.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.word() == word)
    }
}

/// An inclusive span of wire revisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revisions {
    /// The oldest it speaks.
    pub first: u32,
    /// The newest it speaks.
    pub last: u32,
}

impl Revisions {
    /// A span. `last` is raised to `first` if it was given below it, so a span
    /// is never empty — an empty one would refuse every wire including a node's
    /// own kind, which [`NodeKind::admits`] requires to be admitted.
    #[must_use]
    pub const fn new(first: u32, last: u32) -> Self {
        Self {
            first,
            last: if last < first { first } else { last },
        }
    }

    /// The span as the inspector writes it.
    #[must_use]
    pub fn word(self) -> String {
        if self.first == self.last {
            format!("v{}", self.first)
        } else {
            format!("v{}-v{}", self.first, self.last)
        }
    }
}

/// The **host** and **service** halves of a locator, with or without a scheme.
///
/// One splitter, shared by [`NodeKind::explode`] and R1939's repairs, so a
/// locator the split takes apart and a locator a refusal repairs cannot come
/// apart along different seams.
///
/// ⚠ The tail is a service only when it is all digits, and that guard is not
/// decoration: an IPv6 host is written `[::]`, whose last colon is INSIDE the
/// host — before R1939 the bare form came apart as host `[:` and service `]`,
/// which no test had ever asked, because a split always ran on a whole locator
/// where the trailing `:7447` masked it.
fn halves(value: &str) -> (&str, &str) {
    let rest = value.split_once('/').map_or(value, |(_, rest)| rest);
    match rest.rsplit_once(':') {
        Some((host, service))
            if !service.is_empty() && service.bytes().all(|b| b.is_ascii_digit()) =>
        {
            (host, service)
        }
        _ => (rest, ""),
    }
}

/// R1939 — the locator `transport` would take in place of `value`, or `None`
/// when there is nothing near enough to offer.
///
/// A host with no service, a service that is not a number, and a host carrying
/// a stray `/` are all refused with no repair: inventing a service number or
/// dropping part of a host would hand a person a value they did not write and
/// could not tell from one they did.
fn relocated(transport: Transport, value: &str) -> Option<String> {
    let (host, service) = halves(value);
    if host.is_empty() || host.contains('/') || service.is_empty() {
        return None;
    }
    let number: u32 = service.parse().ok()?;
    Some(format!(
        "{}/{host}:{}",
        transport.word(),
        number.clamp(1, 65535)
    ))
}

/// R1939 — [`relocated`] for one transport, as a plain function pointer.
///
/// A `match` and not a captured closure: [`Admits::Shaped`] holds a `fn`
/// pointer for [`Conversion::Converted`]'s reason, and the exhaustive match is
/// what makes a transport added later a COMPILE error here rather than a
/// silently unrepairable pin.
const fn relocating(transport: Transport) -> fn(&String) -> Option<String> {
    match transport {
        Transport::Tcp => |value| relocated(Transport::Tcp, value),
        Transport::Tls => |value| relocated(Transport::Tls, value),
        Transport::Quic => |value| relocated(Transport::Quic, value),
        Transport::Udp => |value| relocated(Transport::Udp, value),
        Transport::Ws => |value| relocated(Transport::Ws, value),
    }
}

/// R1939 — the host a [`Endpoint::Host`] pin would take in place of `value`.
///
/// A whole locator pasted onto a host pin is repaired to its host half, which
/// is [`NodeKind::explode`]'s answer for the same string — the repair a person
/// almost always meant.
fn host_of(value: &str) -> Option<String> {
    let (host, _) = halves(value);
    (!host.is_empty() && !host.contains('/')).then(|| host.to_owned())
}

/// R1939 — the service a [`Endpoint::Service`] pin would take in place of
/// `value`.
///
/// Out of range is CLAMPED and not refused, because the number a person typed
/// says which end they meant; text that is not a number at all has no nearest
/// service and is refused with none.
fn service_of(value: &str) -> Option<String> {
    let (_, service) = halves(value);
    let text = if service.is_empty() { value } else { service };
    let number: u32 = text.trim().parse().ok()?;
    Some(number.clamp(1, 65535).to_string())
}

impl NodeKind for LabNode {
    /// ★ R1914 — an [`Endpoint`], not a [`Transport`]: a pin carries a whole
    /// locator or one half of one, and the split act needs the difference.
    type Type = Endpoint;
    /// A locator: what a pin hands the pin it is wired to.
    type Value = String;

    fn name(&self) -> String {
        self.role.name().to_owned()
    }

    /// ★★★★★ R1937 — **give one pin a transport, and this node becomes the node
    /// that speaks it.**
    ///
    /// The engine's per-pin type choice, in this taxonomy's own vocabulary: a
    /// pin's type IS the endpoint it carries, and an endpoint's transport is a
    /// fact about the node rather than about the pin, so choosing one on any
    /// pin is choosing it for the peer.
    ///
    /// ⚠ A HALF of a locator is refused, and that refusal is the point rather
    /// than an omission: `Host` and `Service` are what a split produces
    /// (R1914), so offering them here would let a person ask for a peer that
    /// speaks "the host half of something", which is not a transport a peer can
    /// speak. This is what makes *the kind may decline a particular type*
    /// reachable on a real screen instead of only in a fixture.
    fn retyped(&self, _port: pinion_node_graph::PortRef, ty: &Endpoint) -> Option<Self> {
        match ty {
            Endpoint::Locator(transport) => Some(Self {
                transport: *transport,
                ..self.clone()
            }),
            Endpoint::Host | Endpoint::Service => None,
        }
    }

    /// ★★★★★ R1923 — what a node of this role IS, DERIVED from the one line the
    /// palette already shows rather than written a second time.
    ///
    /// `Role::gist` was built for the palette; a separate sentence here would
    /// be a second statement about the same role, free to disagree with the one
    /// a reader saw when they placed the node. This is the crate's standing
    /// rule about two facts that must not drift, applied to prose.
    fn description(&self) -> Option<String> {
        Some(self.role.gist().to_owned())
    }

    /// The accept pin, present only when the role can be dialled.
    ///
    /// Absent rather than disabled for a role that never listens: a pin that
    /// exists and can never be used is a pin a person will try to drag to.
    fn inputs(&self) -> Vec<Port<Self::Type, Self::Value>> {
        // The fixed part is empty: the accept pin is the variadic run below, so
        // declaring it here as well would give every listening node two.
        Vec::new()
    }

    /// The dial pin. Every role has one — even a store dials the router it
    /// registers with.
    fn outputs(&self) -> Vec<Port<Self::Type, Self::Value>> {
        vec![
            Port::new("dial", Endpoint::Locator(self.transport))
                .describing("the address this node hands on to whatever it reaches"),
        ]
    }

    /// **The accept pin repeats.**
    ///
    /// A dataflow input takes one wire, because a value has one source; a
    /// *listening endpoint* is dialled by as many peers as reach it, and the
    /// reference's router shows four inbound links on one pin. The crate
    /// derives multiplicity from the flow and offers no many-to-one value
    /// input — which is right for a value — so the many-ness is expressed the
    /// way the crate expresses it: the accept port is a **run**, one port per
    /// link, drawn as one pin because a person authoring a topology is not
    /// choosing which slot to land in.
    fn variadic(&self, side: Side) -> Option<Variadic<Self::Type, Self::Value>> {
        match side {
            Side::Input if self.role.accepts() => Some(
                Variadic::at(
                    0,
                    vec![
                        Port::new("accept", Endpoint::Locator(self.transport))
                            .describing("an address this node listens on"),
                    ],
                )
                .at_least(1),
            ),
            _ => None,
        }
    }

    /// A node hands on the locator it was reached by, so a run's trace shows
    /// the path a message took rather than a value nobody chose.
    fn evaluate(&self, inputs: &[Option<Self::Value>]) -> Vec<Option<Self::Value>> {
        vec![inputs.first().cloned().flatten()]
    }

    /// **The transports must agree.**
    ///
    /// The rule the pin legend draws: an accept pin's colour is its transport,
    /// and a dial of one transport cannot reach an accept of another. Stating
    /// it here rather than at each authoring site is what makes the canvas, the
    /// wire and the validation gate answer the same question.
    fn conversion(from: &Self::Type, to: &Self::Type) -> Conversion<Self::Value> {
        if from == to {
            Conversion::Direct
        } else {
            Conversion::Refused
        }
    }

    /// ★★★★★ R1914 — **a locator is made of a host and a service.**
    ///
    /// The model already said so — [`Transport::of_locator`] takes a locator
    /// apart on the `/` to read its scheme — and this is the same fact declared
    /// where the crate can ask it, which is what lets a pin on this screen come
    /// apart into the two things an analyst actually edits.
    ///
    /// The halves are atoms: a host name has no inside, and neither does a
    /// service number. So the tree here is one level deep, and that is a
    /// property of this taxonomy rather than of the model — the crate's own
    /// fixture carries a depth-2 type, because the reference's recombine
    /// recurses and a model that could not would be the wrong shape.
    fn composition(ty: &Self::Type) -> Composition<Self::Type, Self::Value> {
        match ty {
            Endpoint::Locator(_) => Composition::Members(vec![
                // ★ R1916 — each member says what IT is for. The reference's
                // sub-pins are pins and have nowhere to carry this.
                Port::new("host", Endpoint::Host)
                    .with_default("localhost".to_owned())
                    .describing("where to reach it"),
                Port::new("service", Endpoint::Service)
                    .with_default("7447".to_owned())
                    .describing("which port on that host"),
            ]),
            Endpoint::Host | Endpoint::Service => Composition::Atom,
        }
    }

    /// ★★★★★ R1928 — **what this node calls its own ports.**
    ///
    /// One rule, and it is the reference's commonest use of this hook rather
    /// than its rarest: **a node that listens nowhere shows no name on its
    /// accept pin.** Measured this round, four of the reference's six
    /// overriders use the capability to suppress a name, and the fact here is
    /// exactly of that shape — the accept run's ports are named for the address
    /// each one listens on, and a node with no `listen.endpoints` has no
    /// address to name them with. The ordinal-derived stand-in the model would
    /// otherwise show (`accept 0`) is a name for a seat that is not yet
    /// anything, and a reader who cannot see the canvas is better told there is
    /// no name than given one that means nothing.
    ///
    /// ⚠ It suppresses the NAME, not the pin: the pin is still drawn, still
    /// announced, and still says what it is for — which is the distinction the
    /// reference's empty text cannot make and [`PortName::Silent`] does.
    ///
    /// Everything else keeps its declaration. The dial pin's name is the
    /// kind's, and an accept slot that a link has landed on carries the address
    /// as its own item label — a name this node gives it, through the OTHER of
    /// the three sources, which is why this hook does not answer for it.
    fn port_name(&self, at: PortRef, declared: &str) -> PortName {
        let _ = declared;
        match at.side {
            Side::Input if !self.listening => PortName::Silent,
            Side::Input | Side::Output => PortName::Declared,
        }
    }

    /// ★★★★★ R1927 — **when a node of this taxonomy is in a questionable
    /// state**, in this application's own words.
    ///
    /// One rule, and it is the mirror of a finding this screen already makes:
    /// `dials outside` says an address was written that nothing here listens
    /// on, so the drawing is not the whole picture. This says the other half —
    /// **this node listens and nothing on this canvas dials it**, so a service
    /// is drawn that the drawing shows nobody using. Neither blocks a launch;
    /// both mean a conclusion drawn from this canvas is drawn from a partial
    /// graph, which is worth saying.
    ///
    /// ★ It needs the wiring, and that is why it is here rather than in the
    /// view: the rule is a fact about **this node in this graph**, and the
    /// framework hands it over ([`Surroundings`](pinion_node_graph::Surroundings))
    /// instead of making a screen work it out. The reference's own equivalent
    /// rule needs the same fact and has to climb out of the node to get it.
    /// ★★★★★ R1941 — and the answer carries its WEIGHT. This one **warns**
    /// rather than blocks, deliberately: a node listening with nothing drawn
    /// dialling it is a picture that is incomplete, not a graph that cannot be
    /// run — the peer may exist off this canvas, which is the whole reason the
    /// sentence says *the drawing is not the whole picture*. A rule that
    /// blocked here would refuse to start a deployment that is perfectly
    /// legitimate.
    fn warning(&self, around: &pinion_node_graph::Surroundings) -> Option<Objection> {
        if self.role.accepts() && self.listening && !around.any_wired(Side::Input) {
            return Some(Objection::Warns(
                "listening, and nothing on this canvas dials it — the drawing is \
                 not the whole picture"
                    .to_owned(),
            ));
        }
        // ⚠★★★★★ R1941 — AND THIS TAXONOMY DELIBERATELY DECLARES NO BLOCKING
        // RULE, which is a measured decision rather than an omission.
        //
        // A blocking arm was drafted here — a card that neither accepts nor
        // reaches anything — and then removed, because NO GESTURE THIS SCREEN
        // OFFERS CAN REACH THAT STATE: a card's role is fixed when it is built,
        // there is no verb that removes a link, and the opening canvas has no
        // such card. Measured by driving the assembled shell: the gate reports
        // `blocking: 0` and four non-blocking findings, and no sequence of the
        // published actions moves it.
        //
        // ⇒ a rule nothing can reach is not a gate, it is decoration that reads
        // like one. The weight axis is real and proven where it CAN be driven
        // (`pinion-node-graph`'s own census proof exercises all three arms and
        // the `may_run` gate); what this screen owes is a gesture that reaches
        // the state, and that is a round of its own rather than a rule written
        // here in advance of one.
        None
    }

    /// ★★★★★ R1926 — **what colour a value of this socket type is drawn in.**
    ///
    /// The three halves of this taxonomy each answer for themselves, and that
    /// is the whole point: before this round the canvas coloured every pin by
    /// the **node's** transport, so splitting a locator drew its two halves in
    /// one colour — the parent's — and a reader could not tell the host from
    /// the service, nor either from the whole.
    ///
    /// ★ `Host` and `Service` get colours of their own rather than derived
    /// ones, and that follows from a fact this file already records: a half
    /// carries **no transport**, deliberately, because a host name is the same
    /// host name over a stream or a datagram. So there is nothing to derive
    /// from, and the two must simply be distinguishable — from each other and
    /// from every transport. `r1926_the_socket_palette_is_injective` is what
    /// holds that, so it is a checked property rather than a claim in prose.
    fn type_colour(ty: &Endpoint) -> Option<Tint> {
        Some(match ty {
            Endpoint::Locator(transport) => transport.tint(),
            // A place to reach.
            Endpoint::Host => Tint::rgb(0x5A, 0xA7, 0xB8),
            // Which port on it.
            Endpoint::Service => Tint::rgb(0xD1, 0x6A, 0x5A),
        })
    }

    /// ★★★★★ R1940 — **a card is drawn in the colour of the transport it
    /// speaks.**
    ///
    /// `LikeType` and not a colour written out here, which is the whole reason
    /// that arm exists: the transport palette is already declared once, for
    /// PINS ([`type_colour`](NodeKind::type_colour)), and a card naming its own
    /// copy would be a second vocabulary free to disagree with the legend the
    /// same screen draws. Recolour a transport and the pins AND the cards that
    /// speak it move together, because there is one declaration.
    ///
    /// ⚠ Answered from `self`, so this is per CARD and not per kind: R1937's
    /// verb gives one card another transport, and its colour follows the same
    /// turn. That is the capability the reference spells as a per-node class
    /// override, and it is why an authored colour has to outrank this rather
    /// than the other way round — a person who chose a colour did not choose it
    /// to be recomputed under them ([`Document::faces`](
    /// pinion_node_graph::Document::faces)).
    fn drawn_as(&self) -> Drawn<Self::Type> {
        Drawn::LikeType(Endpoint::Locator(self.transport))
    }

    /// ★★★★★ R1916 — what a value of this socket type IS.
    ///
    /// The half the reference's `ConstructBasicPinTooltip` promises in its
    /// comment ("things like the pin's type") and does not do — read this
    /// round, its base implementation hands the description straight back
    /// unchanged. Here it is the taxonomy's, so every port carrying the type
    /// gets it and none of them can disagree about it.
    fn type_description(ty: &Self::Type) -> Option<String> {
        Some(match ty {
            Endpoint::Locator(transport) => {
                format!(
                    "a {} address, written `scheme/host:service`",
                    transport.word()
                )
            }
            Endpoint::Host => "a host name or address".to_owned(),
            Endpoint::Service => "a service port number".to_owned(),
        })
    }

    /// ★★★★★ R1939 — **what this pin will TAKE as its resting locator.**
    ///
    /// Every socket type this taxonomy has answers, and each answer is a rule
    /// that produces the value the pin would have taken — so a screen offering
    /// a repair cannot offer one the same declaration would then refuse.
    ///
    /// ⚠ The scheme is part of the rule, and that is the point rather than
    /// strictness: a pin's TYPE is the endpoint it carries (R1937), and the
    /// canvas colours the pin by that transport (R1926), so a pin drawn as one
    /// transport while resting on another transport's address is a card saying
    /// two things. The repair re-schemes rather than refusing outright, because
    /// the address a person pasted is almost always the right host and service.
    ///
    /// ★ The halves come from the SAME splitter [`explode`](NodeKind::explode)
    /// uses, so a locator this repairs and a locator the split takes apart
    /// cannot come apart along different seams.
    fn takes(&self, _at: PortRef, ty: &Self::Type) -> Admits<Self::Value> {
        match ty {
            Endpoint::Locator(transport) => Admits::Shaped {
                wants: format!(
                    "an address this pin can speak, like `{}/host:service`",
                    transport.word()
                ),
                nearest: relocating(*transport),
            },
            Endpoint::Host => Admits::Shaped {
                wants: "a host on its own, with no scheme and no service".to_owned(),
                nearest: |value| host_of(value),
            },
            Endpoint::Service => Admits::Shaped {
                wants: "a service number from 1 to 65535".to_owned(),
                nearest: |value| service_of(value),
            },
        }
    }

    /// ★★★★★ R1914 — take a locator apart into its host and its service.
    ///
    /// The scheme is **dropped rather than shared out**, and that is the right
    /// answer rather than a shortcut: which transport a pin speaks is the
    /// PORT'S TYPE here, not a piece of its value, so a member port carrying
    /// the scheme would be a second place the same fact lived. [`implode`]
    /// puts it back from the type, which is why the round trip holds.
    ///
    /// [`implode`]: NodeKind::implode
    fn explode(ty: &Self::Type, value: &Self::Value) -> Vec<Option<Self::Value>> {
        if ty.transport().is_none() {
            return Vec::new();
        }
        let (host, service) = halves(value);
        vec![
            (!host.is_empty()).then(|| host.to_owned()),
            (!service.is_empty()).then(|| service.to_owned()),
        ]
    }

    /// ★★★★★ R1914 — put a host and a service back into a locator, **with the
    /// scheme the port's type names**.
    ///
    /// The half the reference does not have for any type outside a
    /// hand-written chain of four. Here it is one line and it cannot disagree
    /// with [`explode`](NodeKind::explode), because both are declared on the
    /// taxonomy that owns the type and `round_trips` is a law a consumer can
    /// run over them.
    fn implode(ty: &Self::Type, members: &[Option<Self::Value>]) -> Option<Self::Value> {
        let transport = ty.transport()?;
        let [Some(host), Some(service)] = members else {
            return None;
        };
        Some(format!("{}/{host}:{service}", transport.word()))
    }

    /// Two peers may be wired when they can negotiate a wire revision (R1885).
    ///
    /// ★★★★★ The rule this screen could not state before, and the reason is
    /// worth keeping: [`NodeKind::conversion`] is handed two port *types* and no
    /// nodes, so a rule written there is blind to which peers the wire runs
    /// between. The transport a pin speaks and the protocol revision a build
    /// speaks are different facts — two nodes can agree on `tcp` and still have
    /// nothing to say to each other — and until this hook existed the second
    /// one had nowhere to live.
    ///
    /// The refusal blames the **older** end, because that is the one a person
    /// upgrades; when neither is older than the other the ranges overlap and
    /// there is no refusal to blame anybody for.
    fn admits(source: &Self, sink: &Self) -> Admission {
        if source.implementation.negotiates_with(sink.implementation) {
            return Admission::Allowed;
        }
        let (out, into) = (source.implementation, sink.implementation);
        let end = if out.speaks.last < into.speaks.first {
            Side::Output
        } else {
            Side::Input
        };
        Admission::Refused(Refusal {
            end,
            because: format!(
                "{} speaks {} and {} speaks {}, so they share no wire revision",
                out.stack.word(),
                out.speaks.word(),
                into.stack.word(),
                into.speaks.word(),
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Endpoint, Implementation, LabNode, Revisions, Role, Stack, Transport, halves};
    use pinion_node_graph::{Admission, Judged, NodeKind, PortRef, Side};

    /// A node of one transport, for the R1939 assertions below.
    fn speaking(transport: Transport) -> LabNode {
        LabNode {
            role: Role::ALL[0],
            transport,
            listening: true,
            implementation: Implementation::default(),
        }
    }

    /// ★★★★★ R1939 — **every socket type this taxonomy has says what its pin
    /// will take, and the rule carries the pin's own transport.**
    ///
    /// The population is `Endpoint::all()` and `Transport::ALL` rather than a
    /// chosen pair, so a type or a transport added later joins this assertion
    /// without anyone remembering it — the escape hatch a hand-written list is.
    #[test]
    fn r1939_every_socket_type_says_what_its_pin_will_take() {
        for transport in Transport::ALL {
            let node = speaking(transport);
            for ty in Endpoint::all() {
                let declared = node.takes(PortRef::output(0), &ty);
                let wants = declared.wants();
                assert!(
                    !wants.is_empty(),
                    "★ {ty:?} says nothing about what it will take"
                );
                if let Endpoint::Locator(carried) = ty {
                    assert!(
                        wants.contains(&format!("{}/host:service", carried.word())),
                        "★★★★★ the sentence names the transport the PIN carries, \
                         not the one the node was built with: {wants:?}"
                    );
                    // ★ And the rule and the sentence agree, which is what a
                    // second statement could not guarantee: a well-formed
                    // address of ANOTHER transport is refused, and the repair
                    // is the same address under this one.
                    let other = if carried == Transport::Tcp {
                        Transport::Udp
                    } else {
                        Transport::Tcp
                    };
                    let wrong = format!("{}/10.0.0.4:7447", other.word());
                    assert_eq!(
                        declared.judge(&wrong),
                        Judged::Refused {
                            wants: wants.clone(),
                            instead: Some(format!("{}/10.0.0.4:7447", carried.word())),
                        },
                        "★ {carried:?} refuses {wrong:?} and offers its own scheme"
                    );
                    assert!(
                        declared
                            .judge(&format!("{}/10.0.0.4:7447", carried.word()))
                            .stands(),
                        "★ and the address it offered is one it takes"
                    );
                }
            }
        }
    }

    /// ★★★★★ R1939 — the two HALVES of a locator take what a split produces
    /// and refuse the whole, offering the half.
    ///
    /// ⚠ The service is CLAMPED and the host is not: a number out of range says
    /// which end a person meant, and a host with a stray `/` says nothing about
    /// what they meant, so inventing one would hand back a value they cannot
    /// tell from their own.
    #[test]
    fn r1939_the_halves_take_a_half_and_offer_one_for_a_whole() {
        let node = speaking(Transport::Quic);
        let host = node.takes(PortRef::input(0), &Endpoint::Host);
        assert!(host.judge(&"10.0.0.4".to_owned()).stands());
        assert_eq!(
            host.judge(&"quic/10.0.0.4:7447".to_owned()),
            Judged::Refused {
                wants: host.wants(),
                instead: Some("10.0.0.4".to_owned()),
            },
            "★ a whole locator on a host pin is repaired to its host half"
        );
        assert!(
            matches!(
                host.judge(&String::new()),
                Judged::Refused { instead: None, .. }
            ),
            "★ and an empty host has no nearest, which is a real answer"
        );

        let service = node.takes(PortRef::input(0), &Endpoint::Service);
        assert!(service.judge(&"7447".to_owned()).stands());
        assert_eq!(
            service.judge(&"99999".to_owned()),
            Judged::Refused {
                wants: service.wants(),
                instead: Some("65535".to_owned()),
            },
            "★ out of range is clamped rather than refused outright"
        );
        assert!(
            matches!(
                service.judge(&"not-a-number".to_owned()),
                Judged::Refused { instead: None, .. }
            ),
            "★ and text that is no number at all has no nearest service"
        );
    }

    /// ★★★★★ R1939 — the splitter treats a colon as a service separator only
    /// when what follows it is a NUMBER.
    ///
    /// ⚠ This is a latent defect R1939 found and repaired rather than a new
    /// rule: an IPv6 host is written `[::]`, whose last colon is inside the
    /// host, and before this the bare form came apart as host `[:` and service
    /// `]`. No test had ever asked, because a split always ran on a WHOLE
    /// locator, where the trailing `:7447` masked it.
    #[test]
    fn r1939_a_colon_inside_a_host_is_not_a_service_separator() {
        assert_eq!(halves("[::]"), ("[::]", ""));
        assert_eq!(halves("[::]:7447"), ("[::]", "7447"));
        assert_eq!(halves("quic/[::]:7447"), ("[::]", "7447"));
        assert_eq!(halves("10.0.0.4:7447"), ("10.0.0.4", "7447"));
        assert_eq!(
            halves("tcp/10.0.0.4:7447/x"),
            ("10.0.0.4:7447/x", ""),
            "trailing rubbish leaves no service, so the repair refuses"
        );
        // ★ And the split agrees with it, because they share the one splitter.
        assert_eq!(
            LabNode::explode(&Endpoint::Locator(Transport::Quic), &"[::]".to_owned()),
            vec![Some("[::]".to_owned()), None],
            "★★★★★ the host survives whole, which is what the guard is for"
        );
    }

    #[test]
    fn r1651_a_role_that_never_listens_has_no_accept_pin_at_all() {
        for role in Role::ALL {
            let node = LabNode {
                role,
                transport: Transport::Tcp,
                listening: true,
                implementation: Implementation::default(),
            };
            assert_eq!(
                node.variadic(pinion_node_graph::Side::Input).is_some(),
                role.accepts(),
                "{} declares its accept run exactly when it can be dialled",
                role.name()
            );
            assert!(
                node.inputs().is_empty(),
                "and never as a fixed port too, which would give it two pins"
            );
            assert_eq!(node.outputs().len(), 1, "every role can dial");
        }
    }

    #[test]
    fn r1651_a_link_across_two_transports_is_refused_by_the_taxonomy() {
        // The legend's rule, and the reason an accept pin is coloured.
        let locator = Endpoint::Locator;
        assert!(
            !LabNode::conversion(&locator(Transport::Tcp), &locator(Transport::Tcp)).is_refused(),
            "same transport crosses"
        );
        for other in Transport::ALL {
            if other == Transport::Tcp {
                continue;
            }
            assert!(
                LabNode::conversion(&locator(Transport::Tcp), &locator(other)).is_refused(),
                "tcp must not reach {}",
                other.word()
            );
        }
        // ★ R1914 — and the halves of a locator do not cross into a whole one,
        // which is what keeps a split pin from being wired to an unsplit one.
        assert!(
            LabNode::conversion(&Endpoint::Host, &locator(Transport::Tcp)).is_refused(),
            "a host name is not a locator",
        );
        assert!(
            !LabNode::conversion(&Endpoint::Host, &Endpoint::Host).is_refused(),
            "a host reaches a host",
        );
    }

    /// ★★★★★ R1914 — the taxonomy's own round-trip law, run over its one
    /// composite type.
    ///
    /// The check the reference cannot be given: its two halves are chains in an
    /// editor's schema with nothing that owns the pair. Here both are declared
    /// on the taxonomy, so this screen can hold itself to them — and it is the
    /// screen's own types the law runs over, not the crate's fixture.
    #[test]
    fn r1914_a_locator_comes_apart_and_goes_back_together() {
        use pinion_node_graph::{RoundTrip, round_trips};

        for transport in Transport::ALL {
            let ty = Endpoint::Locator(transport);
            let locator = format!("{}/10.0.0.4:7447", transport.word());
            assert_eq!(
                round_trips::<LabNode>(&ty, &locator),
                RoundTrip::Holds,
                "{locator} must survive being taken apart and put back",
            );
        }
        assert_eq!(
            LabNode::explode(
                &Endpoint::Locator(Transport::Tcp),
                &"tcp/host:7447".to_owned()
            ),
            vec![Some("host".to_owned()), Some("7447".to_owned())],
        );
        // ★ A half is an atom, so the law reports it was never exercised
        // rather than answering "fine" — the escape hatch the crate refuses.
        assert_eq!(
            round_trips::<LabNode>(&Endpoint::Host, &"host".to_owned()),
            RoundTrip::NotComposite,
        );
    }

    #[test]
    fn r1651_every_role_is_in_exactly_one_group_and_the_groups_are_the_palette() {
        let mut infra = 0;
        let mut traffic = 0;
        for role in Role::ALL {
            match role.group() {
                "infrastructure" => infra += 1,
                "traffic" => traffic += 1,
                other => panic!("{} is in {other:?}, which is not a group", role.name()),
            }
            assert_eq!(Role::from_name(role.name()), Some(role), "round trip");
        }
        assert_eq!((infra, traffic), (4, 4));
    }

    #[test]
    fn r1651_a_locator_names_its_transport_and_an_unknown_scheme_names_none() {
        assert_eq!(
            Transport::of_locator("tcp/0.0.0.0:7447"),
            Some(Transport::Tcp)
        );
        assert_eq!(
            Transport::of_locator("quic/[::]:7447"),
            Some(Transport::Quic)
        );
        assert_eq!(
            Transport::of_locator("smoke/0.0.0.0:1"),
            None,
            "a scheme the legend does not list is not silently a default"
        );
        assert_eq!(Transport::of_locator("0.0.0.0:7447"), None);
    }

    /// A node with the given build, for the compatibility rule's cases.
    fn peer(stack: Stack, first: u32, last: u32) -> LabNode {
        LabNode {
            role: Role::Peer,
            transport: Transport::Tcp,
            listening: true,
            implementation: Implementation {
                stack,
                speaks: Revisions::new(first, last),
            },
        }
    }

    /// ★★★★★ **A node is always compatible with another of its own kind** — the
    /// reflexivity [`NodeKind::admits`] requires, over every build and every
    /// span this lab can express.
    ///
    /// Written as a property rather than as one case because the rule is a
    /// range overlap, and the one way to get an overlap rule wrong that a
    /// hand-picked pair will not show is an empty span: `first > last` makes a
    /// range that intersects nothing, *including itself*, so a node would
    /// refuse a wire to its own twin. [`Revisions::new`] is what forbids that,
    /// and this is what says so.
    #[test]
    fn r1885_a_build_always_negotiates_with_itself_at_every_span() {
        for stack in Stack::ALL {
            for first in 0..12u32 {
                for last in 0..12u32 {
                    let node = peer(stack, first, last);
                    assert_eq!(
                        LabNode::admits(&node, &node),
                        Admission::Allowed,
                        "{} {:?} refuses its own twin",
                        stack.word(),
                        node.implementation.speaks,
                    );
                }
            }
        }
    }

    /// Two builds are wireable exactly when their spans overlap, at every pair
    /// of spans — and the refusal blames the **older** end.
    ///
    /// ⚠ The two halves are asserted together on purpose. A rule that refused
    /// the right pairs while always blaming the same end would pass a check of
    /// either half alone, and "which node do I change?" is the only part of a
    /// refusal an author can act on.
    #[test]
    fn r1885_two_builds_are_wireable_exactly_when_their_revisions_overlap() {
        for a in 0..8u32 {
            for b in a..8u32 {
                for c in 0..8u32 {
                    for d in c..8u32 {
                        let out = peer(Stack::Reference, a, b);
                        let into = peer(Stack::Independent, c, d);
                        let overlap = a <= d && c <= b;
                        match LabNode::admits(&out, &into) {
                            Admission::Allowed => assert!(
                                overlap,
                                "v{a}-v{b} and v{c}-v{d} share no revision and were admitted"
                            ),
                            Admission::Refused(why) => {
                                assert!(
                                    !overlap,
                                    "v{a}-v{b} and v{c}-v{d} overlap and were refused"
                                );
                                assert_eq!(
                                    why.end,
                                    if b < c { Side::Output } else { Side::Input },
                                    "v{a}-v{b} -> v{c}-v{d} blames the wrong end",
                                );
                                assert!(
                                    why.because.contains("reference")
                                        && why.because.contains("independent"),
                                    "the sentence names both builds: {:?}",
                                    why.because,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// ★ A span given backwards is not an empty span.
    #[test]
    fn r1885_a_span_written_backwards_is_the_single_revision_it_starts_at() {
        assert_eq!(Revisions::new(7, 3), Revisions::new(7, 7));
        assert_eq!(Revisions::new(7, 3).word(), "v7");
        assert_eq!(Revisions::new(4, 8).word(), "v4-v8");
    }

    /// Every build's word round-trips, so the inspector can offer them.
    #[test]
    fn r1885_a_build_is_named_by_a_word_that_round_trips() {
        for stack in Stack::ALL {
            assert_eq!(Stack::from_word(stack.word()), Some(stack));
        }
        assert_eq!(Stack::from_word("no such build"), None);
    }

    /// ★★★★★ R1960 — **a node's transport is decided in ONE place, and that
    /// place is named in the failure.**
    ///
    /// # Why a source count and not a behaviour test
    ///
    /// The defect this ratchets is not something a screen does wrong — it is
    /// the same decision spelled at several sites, which behaves identically
    /// until one of them is edited. `debt-every-card-on-the-opening-graph-
    /// speaks-one-transport` measured FIVE such sites; two read a locator
    /// string into a port type, two read a form into a node's transport, and
    /// one re-read the node while drawing its pins. This project's rule 13
    /// says the repair for that is a derivation, and a derivation nothing
    /// guards is one the next round re-splits.
    ///
    /// ⚠ **The escape hatch is what is counted, not the derivation.**
    /// `unwrap_or(Transport::Tcp)` is a classification nobody made — the thing
    /// R1921 forbade — so the honest floor is ZERO and the pin is what remains
    /// on the way there. Two are left: `transport_of_form`, which needs a
    /// dialling node's transport to come from its LINK (R1716's direction), and
    /// the pin-drawing read, which needs a card that cannot be drawn for a node
    /// that is not there.
    ///
    /// ⚠⚠ Counted from the source text, which is coarse: a comment mentioning
    /// the call would count. That is why the number is a RATCHET and the test
    /// prints the lines — a reader can see whether a rise is real. The
    /// alternative, parsing Rust here, is a second compiler.
    #[test]
    fn r1960_a_nodes_transport_is_decided_in_one_place() {
        /// The sites left, and the round that must remove each. Falls to zero;
        /// never rises.
        const ESCAPES: usize = 2;

        let source = include_str!("lib.rs");
        let sites: Vec<(usize, &str)> = source
            .lines()
            .enumerate()
            .filter(|(_, line)| line.contains("unwrap_or(Transport::Tcp)"))
            .filter(|(_, line)| !line.trim_start().starts_with("///"))
            .map(|(n, line)| (n + 1, line.trim()))
            .collect();
        assert_eq!(
            sites.len(),
            ESCAPES,
            "`unwrap_or(Transport::Tcp)` is a transport nobody chose; the pin \
             says {ESCAPES} are left to remove and the source holds {}: {sites:#?}",
            sites.len(),
        );
    }
}
