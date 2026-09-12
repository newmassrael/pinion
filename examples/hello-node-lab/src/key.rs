//! ★★★★★ R2155 §5.11 §5.40 — **where a CONFIGURATION key comes from.**
//!
//! # What was missing, and how it hid
//!
//! [`address`](crate::address) declares the dotted names of painted *marks*.
//! This screen names a second dotted vocabulary that looks exactly like the
//! first and is not it: the **configuration keys** of the document the lab
//! edits — `transport.link.tx.batch_size`, `admin.permissions.read`. They are
//! the argument a form row is built from, never the address of a mark.
//!
//! That vocabulary already has an authority. `settings::sourced_surface` reads
//! `docs/analyzer-config-surface.json` — the target's own declared option
//! surface, compiled in — and `settings::schema` **asserts** every refinement
//! is keyed at a path it declares. So one reader of these keys was held to the
//! declaration.
//!
//! ⚠ Those two are named in code spans rather than linked, because `settings`
//! is a private module and this page is public: rustdoc refuses a public page
//! linking into a private one, and the push gate is what said so.
//!
//! **Exactly one.** Every other reader re-typed the key: the row builder, the
//! per-role default table, the palette's add-key groups, the screen
//! specification, the CLI argument pin, the paint tests. Measured at R2155,
//! **35 Rust sites across four files** spell a key this file could hand them.
//!
//! ⇒ the failure is the same one the address campaign is named for, in the
//! other vocabulary: a wrong letter compiles, and the form then edits a key
//! the schema does not carry — so the control paints, the edit lands nowhere,
//! and the screen looks like it simply did not apply the change.
//!
//! # ★★★★★ Why this file exists rather than a filter in the census
//!
//! `tools/painted_addresses.py` counted all 35 as *painted addresses left to
//! convert*, because its needle reads shape (`<word>.<word>.`) and a config
//! key has exactly that shape. The tempting repair is to teach the census to
//! skip them. That is the wrong direction twice over: it would delete a real
//! retyping defect from the queue, and it would make the count depend on a
//! heuristic rather than on what the tree declares.
//!
//! The right repair is the one this file is: **give the second vocabulary the
//! same declaring site the first one has.** The census then falls because the
//! keys genuinely stopped being spelled, not because it stopped looking.
//!
//! # ★★ What holds it
//!
//! Two assertions in `tests.rs`, and they answer different questions:
//!
//! * `r2155_a_config_key_is_sourced` — every const here is a path
//!   `settings::sourced_surface` declares. This is what turns a typo into a
//!   **test failure** instead of a control that quietly edits nothing.
//! * `r2155_a_config_key_is_typed_in_one_place` — no source file outside this
//!   one spells any of them. This is the half that stops the defect coming
//!   back: without it, converting today's 35 sites just clears the way for the
//!   thirty-sixth.

/// Whether a router may grant write access — a boolean leaf, not a set.
pub const ADMIN_PERMISSIONS_READ: &str = "admin.permissions.read";

/// Its write half. Declared separately because the target holds the two as an
/// object of booleans rather than as an array at a path that is not a leaf.
pub const ADMIN_PERMISSIONS_WRITE: &str = "admin.permissions.write";

/// The transmit batch ceiling — the row this screen uses to demonstrate a
/// numeric bound, so it is also the key the CLI validation pin names.
pub const TRANSPORT_LINK_TX_BATCH_SIZE: &str = "transport.link.tx.batch_size";

/// The transports the palette's own legend has.
pub const TRANSPORT_LINK_PROTOCOLS: &str = "transport.link.protocols";

/// The receive buffer ceiling.
pub const TRANSPORT_LINK_RX_BUFFER_SIZE: &str = "transport.link.rx.buffer_size";

/// Whether this node shouts for peers.
pub const DISCOVERY_MULTICAST_ENABLED: &str = "discovery.multicast.enabled";

/// Where it shouts — an endpoint with no transport word.
pub const DISCOVERY_MULTICAST_ADDRESS: &str = "discovery.multicast.address";

/// How far the shout carries.
pub const DISCOVERY_MULTICAST_TTL: &str = "discovery.multicast.ttl";

/// Whether unicast links compress.
pub const TRANSPORT_UNICAST_COMPRESSION_ENABLED: &str = "transport.unicast.compression.enabled";

/// How many unicast links this node will hold.
pub const TRANSPORT_UNICAST_MAX_LINKS: &str = "transport.unicast.max_links";

/// The key this screen ADDS to show a row appearing — sourced like the rest,
/// so the add path is exercised against a real leaf rather than a made-up one.
pub const TRANSPORT_UNICAST_LOWLATENCY: &str = "transport.unicast.lowlatency";

/// The routing mode — the enumerated row, and the one key that already had a
/// const before this file existed (`spec::ENUM_KEY`), named for its use rather
/// than for itself and spelled again twice elsewhere.
pub const ROUTING_PEER_MODE: &str = "routing.peer.mode";

/// How long an interest is held.
pub const ROUTING_INTERESTS_TIMEOUT: &str = "routing.interests.timeout";

/// The addresses this node listens on — a list-valued row, which is why its
/// form parts carry an item suffix (see [`item`]).
pub const LISTEN_ENDPOINTS: &str = "listen.endpoints";

/// The addresses it dials out to.
pub const CONNECT_ENDPOINTS: &str = "connect.endpoints";

/// Every key declared here, for the gates that read the set rather than one
/// member.
///
/// ⚠ A list rather than a derivation from the surface: the surface declares
/// **111** paths and this screen names fifteen of them. Deriving the list
/// would make the gate assert that this screen uses every key the target has,
/// which is false and would never pass. What the gate derives instead is the
/// other direction — that every member here IS sourced.
pub const ALL: &[&str] = &[
    ADMIN_PERMISSIONS_READ,
    ADMIN_PERMISSIONS_WRITE,
    TRANSPORT_LINK_TX_BATCH_SIZE,
    TRANSPORT_LINK_PROTOCOLS,
    TRANSPORT_LINK_RX_BUFFER_SIZE,
    DISCOVERY_MULTICAST_ENABLED,
    DISCOVERY_MULTICAST_ADDRESS,
    DISCOVERY_MULTICAST_TTL,
    TRANSPORT_UNICAST_COMPRESSION_ENABLED,
    TRANSPORT_UNICAST_MAX_LINKS,
    TRANSPORT_UNICAST_LOWLATENCY,
    ROUTING_PEER_MODE,
    ROUTING_INTERESTS_TIMEOUT,
    LISTEN_ENDPOINTS,
    CONNECT_ENDPOINTS,
];

/// One item of a list-valued row, as the form addresses it.
///
/// A list row's parts are addressed at `<key>.<which>` — `listen.endpoints.0`
/// for the first item, `listen.endpoints.add` for the seat that grows one.
/// That suffix is a position in the form, not part of the configuration key,
/// so it is composed here rather than spelled into a longer const: there is no
/// sourced path called `listen.endpoints.add` and declaring one would put a
/// key in the vocabulary that the target does not have.
#[must_use]
pub fn item(key: &str, which: &str) -> String {
    format!("{key}.{which}")
}

/// The seat that adds an item to a list-valued row.
#[must_use]
pub fn add_item(key: &str) -> String {
    item(key, "add")
}

/// One key and a value, in the `key=value` spelling the CLI takes.
///
/// ⚠ The CLI argument pin used to carry the key inside a longer literal, which
/// is the shape no needle catches and no reader can re-derive: the key was
/// there, spelled, and invisible to every gate that counts spellers.
#[must_use]
pub fn assign(key: &str, value: &str) -> String {
    format!("{key}={value}")
}
