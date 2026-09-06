//! ★★★★★ R2049 §5.2 §5.11 — **where a painted mark's address comes from.**
//!
//! # What was missing
//!
//! The screens in this workspace name painted marks with dotted addresses —
//! `lab.palette.role.Router`, `lab.form.control.<path>` — and **nothing
//! declared them**. Every reader that needed one re-typed a `format!` string:
//! the painter, the accessibility roster, the screen specification, the family
//! census, the gates, and the walks that check the frame from outside.
//!
//! ⇒ one wrong letter compiles, paints, and makes every query that looks for
//! the mark answer **nothing** — quietly. The mark is on the screen and no
//! reader can find it, which is the shape this repository has already paid for
//! twice (a prefix that swallowed chrome, and a family census that counted a
//! caption as a member).
//!
//! # ★ One family at a time, and counted
//!
//! The debt that opened this asked for exactly that, because *"change them
//! all"* has stopped half-way here before. This module holds the **palette
//! role row** and its swatch and nothing else yet, and
//! `r2049_a_role_address_is_typed_in_one_place` is what says so — it reads the
//! sources and refuses a second speller.
//!
//! # ★★ The inverse belongs here too
//!
//! A router turning a tag back into a role was a *second* place the prefix was
//! typed, and the one where a mismatch is silent in the other direction: the
//! press lands on nothing and the screen simply does not respond. Address and
//! parse are one pair, so they live together and are tested against each other.
//!
//! # ⚠ What a walk does instead
//!
//! A demo is Python and cannot call this. Its answer is to **read the address
//! off the wire** rather than to spell it: the screen's specification publishes
//! each role's row and swatch addresses, derived from here, so a walk names a
//! role and is handed the address the paint used.

use crate::graph::Role;

/// The prefix every palette role row carries.
///
/// Declared once. [`ROLE_ROW_TEMPLATE`] is the same address in the form the
/// voice specification's population expansion takes, and a test holds the two
/// together so they cannot drift.
pub const ROLE_ROW: &str = "lab.palette.role.";

/// The prefix every palette role swatch carries.
pub const ROLE_SWATCH: &str = "lab.palette.swatch.";

/// [`ROLE_ROW`] with the population's placeholder, for a specification table
/// whose rows must be `&'static str`.
pub const ROLE_ROW_TEMPLATE: &str = "lab.palette.role.{}";

/// [`ROLE_SWATCH`] with the population's placeholder.
pub const ROLE_SWATCH_TEMPLATE: &str = "lab.palette.swatch.{}";

/// The address of the palette row a person presses to add this role.
#[must_use]
pub fn role_row(role: Role) -> String {
    role_row_named(role.name())
}

/// The same address for a role held as a NAME.
///
/// The specification tables and the wire carry a role by its name rather than
/// as a value, and a caller holding one should not have to find the value again
/// just to spell an address it is about to hand back.
#[must_use]
pub fn role_row_named(name: &str) -> String {
    format!("{ROLE_ROW}{name}")
}

/// The address of the colour band on that row.
#[must_use]
pub fn role_swatch(role: Role) -> String {
    role_swatch_named(role.name())
}

/// The same address for a role held as a name.
#[must_use]
pub fn role_swatch_named(name: &str) -> String {
    format!("{ROLE_SWATCH}{name}")
}

/// The role a palette row address names, or `None` when the tag is not one.
///
/// ★ The inverse of [`role_row`], here rather than at the router, so the two
/// cannot be written against different prefixes. `None` is a real answer — most
/// tags are not palette rows — and a name no role answers to is also `None`,
/// which is what stops a press on a tag this screen does not own from
/// resolving to whichever role happened to sort first.
#[must_use]
pub fn role_of_row(tag: &str) -> Option<Role> {
    let name = tag.strip_prefix(ROLE_ROW)?;
    Role::ALL.into_iter().find(|role| role.name() == name)
}
