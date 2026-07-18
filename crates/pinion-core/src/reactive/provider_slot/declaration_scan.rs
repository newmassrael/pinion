//! R1366 §5.22 — every framework slot key is declared through [`ProviderSlot`],
//! and the ones that are not yet are listed, shrinking, here.
//!
//! # Why this replaces R1365's census
//!
//! R1365 checked the opposite direction: that every `__pinion.` literal had a row
//! in a markdown table. That check and [`ProviderSlot`] **cannot coexist**, and
//! finding out why is what made this round's shape obvious. The type mandates the
//! prefix on every slot — including the probes in its own tests. The census
//! mandates a table row per prefixed literal. So the type's own test probes broke
//! the census the moment the type existed, and no naming trick fixes it: a probe
//! must carry the prefix (the constructor asserts it) and must therefore demand a
//! row it has no business having.
//!
//! The direction was wrong. "Every literal is documented" is a claim about prose,
//! and R1365 shipped it with three holes (a key need not carry the prefix — the
//! census's own subject was a convention nothing enforced; a key need not be a
//! literal; the row projection was lossy). "Every literal is DECLARED" is a claim
//! about code, and the declaration carries the verdict with it.
//!
//! This is `commit.rs:168`'s shape — the negative scan — and it borrows that
//! module's rule verbatim: **stop at `#[cfg(test)]`**. A test SHOULD spell a key
//! by hand; that is the wire pin. Only production code must go through the type.
//!
//! # R1366.1: the scan did not implement the rule it documented
//!
//! R1366 wrote the sentence above and shipped a scan whose only filter was the
//! legacy list — nothing looked for a constructor. It was green because nothing
//! had been migrated yet, and the first migration made the migrated declaration
//! itself an "escape": a check that forbids the state it exists to reach. Its
//! counterfactual (rename a key to a prefixed one, watch the scan fail) passes
//! with or without the constructor check, so it never discriminated between the
//! rule and the code.
//!
//! That is the same failure R1365 shipped and this module was written to end —
//! a claim about the check, kept where the check cannot see it — so
//! `r1366_1_a_declared_slot_is_not_an_offender` now pins the missing half.
//!
//! # The legacy list may only SHRINK
//!
//! Migration is per-slot and each one is a behaviour change (a `provide_*` that
//! silently no-opped now panics). Rather than one unreviewable commit, the
//! un-migrated slots are named here and the count is asserted, the
//! `UNCLASSIFIED_TERMINAL_GAPS` shape R1364.5 used: a list that may only shrink
//! is a debt you cannot lose track of, where a list that may grow is a habit.

/// Slot keys still declared as a bare `const` + hand-written `provide`/`resolve`
/// pair rather than a [`ProviderSlot`](super::ProviderSlot).
///
/// **This list may only shrink.** Each entry is one R1366.x migration. Do not add
/// to it: a NEW slot has no excuse, because the type exists now.
const LEGACY_SLOT_KEYS: &[&str] = &[
    "__pinion.reactive.viewport_size",
    "__pinion.reactive.frame_timings",
    "__pinion.core.scene_revision",
    "__pinion.rpc.waiter_registry",
];

/// This file, workspace-relative — the one file the walk must NOT read.
///
/// This module is test-only (`#[cfg(test)] mod declaration_scan;` in the
/// parent), but the walk reads FILES and the `cfg` sits on the parent's `mod`
/// line, so nothing in here trips the `#[cfg(test)]` rule. [`LEGACY_SLOT_KEYS`]
/// would then read as production spellings of the very keys it lists —
/// which is what made R1366's staleness assert **vacuous**: every legacy key
/// "still existed" because this list spells it, so a ghost was undetectable and
/// the test could never fail. Skipping the file is the `#[cfg(test)]` rule
/// applied where the walk cannot see the attribute.
const THIS_FILE: &str = file!();

/// `src` with every top-level `#[cfg(test)]` item, and every comment line,
/// removed.
///
/// A test SHOULD spell a key by hand — that is the wire pin, `commit.rs:168`'s
/// rule — so only production must go through the type. R1366 implemented that as
/// "stop reading at the first line whose TRIMMED text starts with
/// `#[cfg(test)]`", which is wrong twice over, and both ways are FALSE
/// NEGATIVES — the one failure mode a scan must not have:
///
/// * it stops at an **indented** attribute, and `owner.rs` carries
///   `#[cfg(test)]` on a test-only accessor at line 817 of 3305 — so three
///   quarters of that file, `local_task_pump`'s key included, was invisible; and
/// * it **stops** rather than resumes, so any production item written after a
///   test module would be invisible too.
///
/// Neither was noticed because the staleness assert that would have caught the
/// first was simultaneously vacuous (see [`THIS_FILE`]) — two defects, each
/// hiding the other.
///
/// A top-level item's closing brace sits at column 0 in `rustfmt`ed source, and
/// `cargo fmt --all --check` is a CI gate, so a test block can be skipped by
/// finding that brace rather than by counting braces — which a `{` inside a
/// string literal would defeat.
fn production_only(src: &str) -> String {
    const ATTR: &str = "#[cfg(test)]";
    let mut out = String::new();
    let mut lines = src.lines();
    while let Some(line) = lines.next() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        if !line.starts_with(ATTR) {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        // The attribute may carry its item (`#[cfg(test)] mod tests {`) or sit
        // on its own line above it. A `mod x;` / `use x;` item ends there; a
        // block item ends at the next column-0 `}`.
        let rest = line[ATTR.len()..].trim();
        let item = if rest.is_empty() {
            lines.next()
        } else {
            Some(rest)
        };
        if item.is_none_or(|i| i.trim_end().ends_with(';')) {
            continue;
        }
        for skipped in lines.by_ref() {
            if skipped.trim_end() == "}" {
                break;
            }
        }
    }
    out
}

/// One `"__pinion.…"` literal found in workspace production source.
#[derive(Debug)]
struct SlotLiteral {
    key: String,
    file: String,
    /// The literal sits inside a [`ProviderSlot`](super::ProviderSlot)
    /// constructor call — it IS a declaration, rather than a key hand-rolled
    /// next to a `cache_inherited` call that could say anything.
    declared: bool,
}

/// Every `"__pinion.…"` literal in workspace PRODUCTION source, each classified
/// as declared-through-the-type or not. Walks source text rather than reflecting
/// linked crates: the census is open-world, and a collector only ever sees the
/// crates that got linked, so a new crate's slot would be silently absent
/// instead of loudly missing.
///
/// Classification is per **statement**, not per line: `rustfmt` splits a
/// constructor call across lines the moment the type name grows, so a line-local
/// check would flip with the width of an unrelated identifier. A `static`
/// declaration is exactly one `;`-terminated statement, so that is the unit.
fn production_slot_literals() -> Vec<SlotLiteral> {
    // Assembled at run time so these needles are not themselves hits.
    let needle = format!("\"{}.", "__pinion");
    let ctors = [
        format!("{}::inherited(", "ProviderSlot"),
        format!("{}::per_scope(", "ProviderSlot"),
    ];
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root is two levels above this crate");
    let mut found: Vec<SlotLiteral> = Vec::new();
    let mut skipped_self = false;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                let name = p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if !matches!(name.as_str(), "target" | "vendor" | ".git") {
                    stack.push(p);
                }
                continue;
            }
            if p.extension().is_none_or(|x| x != "rs") {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(&p) else {
                continue;
            };
            let where_ = p
                .strip_prefix(root)
                .unwrap_or(&p)
                .to_string_lossy()
                .to_string();
            if where_ == THIS_FILE {
                skipped_self = true;
                continue;
            }
            for stmt in production_only(&src).split(';') {
                let mut at = 0;
                while let Some(i) = stmt[at..].find(&needle) {
                    let start = at + i;
                    let tail = &stmt[start + 1..];
                    let Some(end) = tail.find('"') else { break };
                    let key = &tail[..end];
                    at = start + 1 + end;
                    // The type's own predicate, not a hand-rolled one: what
                    // counts as a slot key is `ProviderSlot`'s business, and
                    // reusing it means the scan cannot drift from the
                    // constructors' assert. It also excludes `SLOT_KEY_PREFIX`'s
                    // own definition — the bare prefix is not a key, which is
                    // exactly what `is_slot_key` says.
                    if !super::is_slot_key(key) {
                        continue;
                    }
                    let head = &stmt[..start];
                    found.push(SlotLiteral {
                        key: key.to_string(),
                        file: where_.clone(),
                        declared: ctors.iter().any(|c| head.contains(c.as_str())),
                    });
                }
            }
        }
    }
    // Without this the exclusion above could stop matching — a moved file, a
    // changed `file!()` form — and every assert below would go quietly vacuous
    // again, green and meaningless. The walk must have SEEN this file to skip it.
    assert!(
        skipped_self,
        "the walk never reached {THIS_FILE}, so it never skipped it: either the \
         walk is not reading the workspace, or `file!()` no longer matches the \
         path form it builds. Both make every assert in this module vacuous.",
    );
    found
}

#[test]
fn r1366_no_production_slot_key_escapes_the_declaration_type() {
    let literals = production_slot_literals();
    assert!(
        !literals.is_empty(),
        "the walk found no slot keys at all — it is not reading the workspace",
    );
    let offenders: Vec<String> = literals
        .iter()
        .filter(|l| !l.declared && !LEGACY_SLOT_KEYS.contains(&l.key.as_str()))
        .map(|l| format!("  {} spelled in {}", l.key, l.file))
        .collect();
    assert!(
        offenders.is_empty(),
        "a provider slot key is spelled in production outside a `ProviderSlot` \
         declaration, and outside the shrinking legacy list. Declare it with \
         `ProviderSlot::inherited` / `::per_scope` — the scope argument is how \
         the verdict stops being a comment:\n{}",
        offenders.join("\n"),
    );
}

#[test]
fn r1366_1_a_declared_slot_is_not_an_offender() {
    // R1366 shipped this scan with `declared` missing entirely: its only filter
    // was the legacy list, so the FIRST migration — the thing the list exists to
    // track — made the migrated declaration itself an "escape". The scan was
    // green because nothing had been migrated yet, and R1366's counterfactual
    // (rename a key to a prefixed one, watch it fail) passes with or without the
    // constructor check, so it never discriminated. This is the discriminating
    // one: the type's own declaration must READ as a declaration.
    let literals = production_slot_literals();
    let repaint: Vec<&SlotLiteral> = literals
        .iter()
        .filter(|l| l.key == super::super::repaint::REPAINT_SINK.key())
        .collect();
    let where_: Vec<&str> = repaint.iter().map(|l| l.file.as_str()).collect();
    assert_eq!(
        repaint.len(),
        1,
        "the migrated repaint slot's key should be spelled exactly once in \
         production — at its declaration; found: {where_:?}",
    );
    assert!(
        repaint[0].declared,
        "the key in {} sits inside `ProviderSlot::inherited(..)` and read as \
         UNDECLARED, so the scan bills the type's own declarations and no slot \
         can ever migrate",
        repaint[0].file,
    );
}

#[test]
fn r1366_the_legacy_list_may_only_shrink() {
    // The staleness assert. Without it the list is a place debt goes to be
    // forgotten; with it, finishing the migration is the only way to keep the
    // suite green, and every entry removed is a real slot that gained a
    // compiler-checked verdict.
    assert_eq!(
        LEGACY_SLOT_KEYS.len(),
        4,
        "the legacy list changed. It may only SHRINK — if you migrated a slot, \
         lower this number; if you are adding a key here, do not: declare it as \
         a `ProviderSlot` instead. Remaining: {LEGACY_SLOT_KEYS:?}",
    );
    let mut sorted = LEGACY_SLOT_KEYS.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), LEGACY_SLOT_KEYS.len(), "duplicate legacy key");
}

#[test]
fn r1366_every_legacy_key_still_names_a_hand_rolled_slot() {
    // The other half of the staleness assert: a legacy entry whose slot was
    // deleted, renamed — or MIGRATED — would sit here forever, quietly making
    // the list look longer than the real debt.
    //
    // The migrated case is the one that bites, and R1366's version could not see
    // it: after a migration the key is still spelled (in the declaration), so
    // "the key still exists" stayed true, the length stayed 10, and a forgotten
    // entry passed every assert. Requiring an UNDECLARED spelling is what makes
    // the list shrink with the debt rather than with someone's memory.
    let literals = production_slot_literals();
    let hand_rolled: Vec<&str> = literals
        .iter()
        .filter(|l| !l.declared)
        .map(|l| l.key.as_str())
        .collect();
    let ghosts: Vec<&&str> = LEGACY_SLOT_KEYS
        .iter()
        .filter(|k| !hand_rolled.contains(*k))
        .collect();
    assert!(
        ghosts.is_empty(),
        "the legacy list names keys that no production source spells by hand — \
         they were migrated to `ProviderSlot` (or removed), so delete these \
         entries and lower the count: {ghosts:?}",
    );
}
