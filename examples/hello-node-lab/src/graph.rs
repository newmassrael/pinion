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

use pinion_node_graph::{Conversion, NodeKind, Port, Side, Variadic};
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
}

impl NodeKind for LabNode {
    type Type = Transport;
    /// A locator: what a pin hands the pin it is wired to.
    type Value = String;

    fn name(&self) -> String {
        self.role.name().to_owned()
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
        vec![Port::new("dial", self.transport)]
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
            Side::Input if self.role.accepts() => {
                Some(Variadic::at(0, vec![Port::new("accept", self.transport)]).at_least(1))
            }
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
}

#[cfg(test)]
mod tests {
    use super::{LabNode, Role, Transport};
    use pinion_node_graph::NodeKind;

    #[test]
    fn r1651_a_role_that_never_listens_has_no_accept_pin_at_all() {
        for role in Role::ALL {
            let node = LabNode {
                role,
                transport: Transport::Tcp,
                listening: true,
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
        assert!(
            !LabNode::conversion(&Transport::Tcp, &Transport::Tcp).is_refused(),
            "same transport crosses"
        );
        for other in Transport::ALL {
            if other == Transport::Tcp {
                continue;
            }
            assert!(
                LabNode::conversion(&Transport::Tcp, &other).is_refused(),
                "tcp must not reach {}",
                other.word()
            );
        }
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
}
