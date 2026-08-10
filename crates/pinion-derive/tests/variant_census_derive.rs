//! R1630 — fixture tests for `#[derive(VariantCensus)]`.
//!
//! The derive exists to make a hand-written `ALL` vocabulary list checkable
//! against the definition it claims to enumerate. These fixtures pin what it
//! emits; what it *refuses* is a compile error and so is stated in the derive's
//! own rustdoc rather than exercised here (this crate has no `trybuild`, and
//! adding one to assert three error strings would be a heavier dependency than
//! the fact is worth — the refusals are one `syn::Error` each and are read at
//! the call site immediately).

use pinion_derive::VariantCensus;

#[derive(Debug, Clone, Copy, PartialEq, Eq, VariantCensus)]
#[variant_census(all)]
enum WithList {
    // ★ Named so declaration order and alphabetical order DISAGREE. The first
    // draft used First / Second / Third, which sort into their own declaration
    // order, so the ordering assertion below could not fail — R1629's CF-5
    // finding, reproduced by the round that recorded it one day earlier.
    Zebra,
    Apple,
    Mango,
}

impl WithList {
    const ALL: [Self; 3] = [Self::Zebra, Self::Apple, Self::Mango];
}

/// Payload-carrying variants are censused too — counting arms is well defined
/// whatever they hold, and this is the shape the opt-in exists for: no value
/// list is possible, so `ARMS` is the only statement of the size.
#[derive(Debug, Clone, PartialEq, VariantCensus)]
#[allow(
    dead_code,
    reason = "the fixture exists to be COUNTED — constructing a variant would \
              prove nothing the census does not already state"
)]
enum WithPayloads {
    Unit,
    Tuple(u8, u8),
    Named { x: i32 },
}

#[test]
fn r1630_the_census_counts_the_definition_not_the_list() {
    assert_eq!(WithList::ARMS, 3);
    assert_eq!(WithList::ARM_NAMES, ["Zebra", "Apple", "Mango"]);
    // The point of the opt-in: the hand-written list is held to the count.
    assert_eq!(WithList::ALL.len(), WithList::ARMS);
}

#[test]
fn r1630_arms_are_counted_whatever_they_carry() {
    assert_eq!(WithPayloads::ARMS, 3);
    assert_eq!(WithPayloads::ARM_NAMES, ["Unit", "Tuple", "Named"]);
}

#[test]
fn r1630_arm_names_are_the_idents_in_declaration_order() {
    // Declaration order, not sorted: a reader comparing this against the
    // source reads down the file, and a sorted list would silently reorder
    // a vocabulary whose order is meaningful elsewhere (`ALL` is iterated).
    let sorted = {
        let mut names = WithList::ARM_NAMES;
        names.sort_unstable();
        names
    };
    assert_eq!(WithList::ARM_NAMES, ["Zebra", "Apple", "Mango"]);
    assert_eq!(sorted, ["Apple", "Mango", "Zebra"]);
    assert_ne!(
        sorted,
        WithList::ARM_NAMES,
        "the fixture distinguishes them"
    );
}
