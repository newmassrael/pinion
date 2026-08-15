//! R1697 §5.20 §5.35 §5.38 — **what a screen can be asked to do, declared.**
//!
//! A screen's widget catalogue is a census of what it *has*. This is the census
//! of what it *does* — one row per operation, naming the action an agent
//! invokes, whether a person has a way in, and the introspection slot that must
//! move once it has run.
//!
//! ## Why the framework holds the shape
//!
//! R1678 built this table for the node laboratory, and it earned its keep
//! immediately: every defect a person reported on that screen lived between its
//! two cause columns — the wire routed the operation and the pointer did not,
//! or the reverse. Three rounds later the sibling screen produced the same
//! defect for the same reason. A detached panel could be torn off, closed and
//! re-docked, and **could not be moved**: the press arm that would have started
//! the drag was written `Hit::Float(_) | Hit::Nothing => {}` — a gesture folded
//! in with hitting nothing at all — and every gate on that screen was green,
//! correctly, because each was asking a different question. Somebody had to
//! open the window and try to drag the panel.
//!
//! That is the shape [[debt-a-shape-two-screens-hand-roll-is-a-substrate-hole-nobody-censuses]]
//! names, and this module is the answer to it: the second screen does not get a
//! second table type, it gets this one.
//!
//! ## What the table is FOR, and why two columns of cause
//!
//! The gate a table like this exists to allow is one no widget census can ask:
//! **for every way the table says an operation can be caused, causing it that
//! way changes something observable.** Two columns rather than one, because a
//! test naturally drives the column that works — [`verb`](Operation::verb) is
//! what an agent uses, [`gesture`](Operation::gesture) is whether a person has
//! a way in, and only driving both finds the half that is missing.
//!
//! ## An absent operation is a row, not a gap
//!
//! An operation a screen cannot perform is written down with `verb: None`,
//! which is what lets it be counted, ratcheted and — the direction that matters
//! more — **falsified**: a `None` row that turns out to work is a stale
//! declaration and the gate fails on that too. A table listing only what works
//! would leave what is missing exactly as invisible as it was.

/// One operation a screen declares, and the evidence that it happens.
///
/// Every field is measured against the screen as it stands rather than wished
/// for; the gate drives both cause columns and fails on an optimistic entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Operation {
    /// What the operation is called, in the words the screen's own
    /// specification uses throughout.
    pub name: &'static str,
    /// The action an agent invokes and an argument that exercises it, or `None`
    /// when this screen has no path to the operation at all.
    pub verb: Option<(&'static str, &'static str)>,
    /// Whether a person can cause it with the pointer or the keyboard.
    ///
    /// A bool here and the driver beside the gate, because the driver is a
    /// gesture — a press at a place, a drag between two — and a gesture is not
    /// a value a table can hold. The gate asserts the two agree in **both**
    /// directions: a `true` with no driver, and a driver for a `false`, are
    /// both failures.
    pub gesture: bool,
    /// The introspection slot whose value must DIFFER once the operation has
    /// run.
    ///
    /// Named per operation rather than derived, because "what changed" is the
    /// part of an operation a reader most needs and the part a test is most
    /// tempted to skip. It is also what makes a gesture provable without
    /// reading pixels: drive the gesture, read the slot, compare.
    pub witness: &'static str,
    /// The operation that has to have run first, by name, or `None` when this
    /// one can be caused from the screen as it opens.
    ///
    /// A real property of the tool rather than a convenience for the gate:
    /// putting something back is only possible once it has been changed, and a
    /// table that recorded the operation without its precondition would
    /// describe a screen whose affordances are always there.
    ///
    /// It names an operation rather than describing a state, so the gate
    /// reaches the precondition the way a person would — by doing the earlier
    /// thing — and a `needs` naming an operation the table does not hold is a
    /// failure.
    pub needs: Option<&'static str>,
}

impl Operation {
    /// Whether this screen can perform the operation **at all**, by either
    /// cause. The complement is what a ratchet counts.
    #[must_use]
    pub const fn reachable(&self) -> bool {
        self.verb.is_some() || self.gesture
    }

    /// The action name an agent invokes, when there is one.
    #[must_use]
    pub const fn action(&self) -> Option<&'static str> {
        match self.verb {
            Some((action, _)) => Some(action),
            None => None,
        }
    }
}

/// Which operations a table declares absent — the rows with no cause at all.
///
/// Returned as a list rather than a count so a ratchet can say *which* one
/// appeared, and so a round that closes one can be told it closed the one it
/// meant to.
#[must_use]
pub fn absent(table: &[Operation]) -> Vec<&'static str> {
    table
        .iter()
        .filter(|op| !op.reachable())
        .map(|op| op.name)
        .collect()
}

/// Check a table's internal consistency, returning one message per fault.
///
/// This is the half of the gate that needs no screen: a `needs` naming an
/// operation the table does not hold, a duplicated name, and a row with no
/// witness are all faults a reader of the table alone can find. The half that
/// needs the screen — that driving each cause moves the witness — belongs to
/// the screen's own driver, because only it can press.
///
/// Empty means consistent.
#[must_use]
pub fn faults(table: &[Operation]) -> Vec<String> {
    let mut out = Vec::new();
    for (i, op) in table.iter().enumerate() {
        if op.name.is_empty() {
            out.push(format!("row {i} has no name"));
        }
        if op.witness.is_empty() {
            out.push(format!("{:?} names no witness", op.name));
        }
        if table.iter().filter(|other| other.name == op.name).count() > 1 {
            out.push(format!("{:?} is declared more than once", op.name));
        }
        if let Some(needs) = op.needs {
            if !table.iter().any(|other| other.name == needs) {
                out.push(format!(
                    "{:?} needs {needs:?}, which this table does not hold",
                    op.name
                ));
            }
            if needs == op.name {
                out.push(format!("{:?} needs itself", op.name));
            }
        }
        if !op.reachable() && op.needs.is_some() {
            out.push(format!(
                "{:?} cannot be caused at all, so its precondition says nothing",
                op.name
            ));
        }
    }
    out
}

/// The order the preconditions impose, or the names of the rows that cannot be
/// ordered because their `needs` form a cycle.
///
/// A gate driving the table has to run "add a widget" before "move the widget
/// it added", and deriving that from the declaration is what keeps the driver
/// from encoding a second, silently different order. Stable: rows with no
/// precondition keep their declared order, which is the order the screen's
/// specification groups them in.
///
/// # Errors
///
/// The names still unplaced when no further row's precondition is satisfiable —
/// a cycle, which [`faults`] cannot see because every individual `needs` in a
/// cycle names a row that exists.
pub fn in_order(table: &[Operation]) -> Result<Vec<&'static str>, Vec<&'static str>> {
    let mut ordered: Vec<&'static str> = Vec::with_capacity(table.len());
    let mut left: Vec<&Operation> = table.iter().collect();
    while !left.is_empty() {
        let ready: Vec<&Operation> = left
            .iter()
            .copied()
            .filter(|op| op.needs.is_none_or(|needs| ordered.contains(&needs)))
            .collect();
        if ready.is_empty() {
            return Err(left.iter().map(|op| op.name).collect());
        }
        for op in ready {
            ordered.push(op.name);
        }
        left.retain(|op| !ordered.contains(&op.name));
    }
    Ok(ordered)
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn op(name: &'static str, witness: &'static str) -> Operation {
        Operation {
            name,
            verb: None,
            gesture: true,
            witness,
            needs: None,
        }
    }

    #[test]
    fn r1697_an_operation_with_neither_cause_is_absent() {
        let table = [
            Operation {
                gesture: false,
                ..op("cannot", "w")
            },
            op("by hand", "w"),
            Operation {
                verb: Some(("v", "a")),
                gesture: false,
                ..op("by wire", "w")
            },
        ];
        assert_eq!(absent(&table), vec!["cannot"]);
        assert!(!table[0].reachable());
        assert!(table[1].reachable());
        assert!(table[2].reachable());
        assert_eq!(table[2].action(), Some("v"));
        assert_eq!(table[1].action(), None);
    }

    #[test]
    fn r1697_a_consistent_table_has_no_faults() {
        let table = [
            op("first", "a"),
            Operation {
                needs: Some("first"),
                ..op("second", "b")
            },
        ];
        assert!(faults(&table).is_empty(), "{:?}", faults(&table));
    }

    #[test]
    fn r1697_a_precondition_naming_nothing_is_a_fault() {
        let table = [Operation {
            needs: Some("nowhere"),
            ..op("only", "a")
        }];
        let faults = faults(&table);
        assert_eq!(faults.len(), 1, "{faults:?}");
        assert!(faults[0].contains("does not hold"), "{faults:?}");
    }

    #[test]
    fn r1697_a_duplicate_name_is_a_fault_because_needs_addresses_by_name() {
        let table = [op("same", "a"), op("same", "b")];
        assert_eq!(faults(&table).len(), 2, "both rows are reported");
    }

    #[test]
    fn r1697_a_row_with_no_witness_is_a_fault() {
        let table = [op("nameless witness", "")];
        assert!(faults(&table)[0].contains("names no witness"));
    }

    #[test]
    fn r1697_an_absent_row_may_not_carry_a_precondition() {
        let table = [
            op("first", "a"),
            Operation {
                gesture: false,
                needs: Some("first"),
                ..op("absent", "b")
            },
        ];
        let faults = faults(&table);
        assert_eq!(faults.len(), 1, "{faults:?}");
        assert!(faults[0].contains("says nothing"), "{faults:?}");
    }

    #[test]
    fn r1697_a_row_needing_itself_is_a_fault() {
        let table = [Operation {
            needs: Some("me"),
            ..op("me", "a")
        }];
        let faults = faults(&table);
        assert!(
            faults.iter().any(|f| f.contains("needs itself")),
            "{faults:?}"
        );
    }

    #[test]
    fn r1697_the_order_puts_a_precondition_before_what_needs_it() {
        let table = [
            Operation {
                needs: Some("open"),
                ..op("move", "a")
            },
            op("open", "b"),
        ];
        let order = in_order(&table).expect("acyclic");
        let at = |name| order.iter().position(|n| *n == name).expect("present");
        assert!(at("open") < at("move"));
        assert_eq!(order.len(), 2);
    }

    #[test]
    fn r1697_rows_with_no_precondition_keep_the_declared_order() {
        let table = [op("a", "w"), op("b", "w"), op("c", "w")];
        assert_eq!(in_order(&table).expect("acyclic"), vec!["a", "b", "c"]);
    }

    #[test]
    fn r1697_a_cycle_is_reported_and_not_ordered() {
        let table = [
            Operation {
                needs: Some("second"),
                ..op("first", "a")
            },
            Operation {
                needs: Some("first"),
                ..op("second", "b")
            },
        ];
        let Err(stuck) = in_order(&table) else {
            panic!("a cycle must not order");
        };
        assert_eq!(stuck.len(), 2);
        assert!(faults(&table).is_empty(), "and `faults` cannot see it");
    }
}
