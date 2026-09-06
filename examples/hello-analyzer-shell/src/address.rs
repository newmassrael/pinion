//! ★★★★★ R2051 §5.2 §5.11 — **where this application's own painted addresses
//! come from.**
//!
//! The third instalment of an address debt. R2049 gave a screen's own family a
//! declaring site; R2050 found the next family's owner was the FRAMEWORK, not
//! the screen, and put the declaration where the composition happens. This one
//! is the application's: the rail is chrome this binary paints, and its seats
//! are addressed by a prefix that was typed at every reader — the painter, the
//! router, the description index, the accessibility roster, the specification
//! tables, the gates, and thirteen walks.
//!
//! ⇒ one wrong letter compiles, paints, and makes every query looking for the
//! seat answer nothing, which reads as *the rail did not paint it*.
//!
//! # ★ Why this family is the one the debt singled out
//!
//! Its clause tying this debt to the structural gap is that a rail seat is ONE
//! screen's address and it was being spelled by more than one binary. Measured
//! at R2049 that had already stopped being true — the integration campaign took
//! the other three — so what is left is one application spelling its own
//! address 28 times in its own source. That is the ordinary case, and it is
//! what this closes.
//!
//! # ⚠ What a walk does instead
//!
//! A walk is Python and cannot call this, so the application publishes each
//! seat's address beside the seat and a walk is handed it.

/// The prefix every rail seat address carries.
pub const RAIL: &str = "shell.rail.";

/// [`RAIL`] with the population's placeholder, for a specification table whose
/// rows must be `&'static str`. A test holds it against the derivation.
pub const RAIL_TEMPLATE: &str = "shell.rail.{}";

/// The rail's account block, as a `&'static str` for a specification table.
///
/// Held against [`rail_account`] by a test, the same way [`RAIL_TEMPLATE`] is
/// held against [`RAIL`].
pub const RAIL_ACCOUNT: &str = "shell.rail.account";

/// The address of the rail seat for `key`.
/// ★ Takes anything that reads as a string, because the seat rosters this is
/// called over hold their keys differently — the specification's are `&'static
/// str` and the canon's arrive as `Cow` — and a caller should not have to know
/// which it is holding to spell an address.
#[must_use]
pub fn rail_seat(key: impl AsRef<str>) -> String {
    format!("{RAIL}{}", key.as_ref())
}

/// The seat a rail address names, or `None` when the tag is not one.
///
/// ★★ The inverse, here rather than at the router. R2049's lesson: a parse
/// written against a separately-typed prefix is the second speller, and its
/// mismatch is silent the other way round — the press lands on nothing and the
/// application simply does not navigate.
#[must_use]
pub fn rail_seat_key(tag: &str) -> Option<&str> {
    tag.strip_prefix(RAIL)
}

/// The rail's account block, which belongs to no seat.
///
/// Its own function rather than a caller writing `rail_seat("account")`,
/// because it is NOT a seat: it takes no navigation and the roster does not
/// hold it. A reader that treated it as one would count the rail's seats wrong.
#[must_use]
pub fn rail_account() -> String {
    RAIL_ACCOUNT.to_owned()
}
