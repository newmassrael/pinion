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
    "__pinion.reactive.repaint_sink",
    "__pinion.reactive.quit_sink",
    "__pinion.reactive.monospace_metrics",
    "__pinion.reactive.viewport_size",
    "__pinion.reactive.pane_viewport_registry",
    "__pinion.reactive.local_task_pump",
    "__pinion.reactive.frame_timings",
    "__pinion.core.scene_revision",
    "__pinion.rpc.waiter_registry",
    "__pinion.shell.window_control_sink",
];

/// Every `"__pinion.…"` literal in workspace PRODUCTION source, with the file it
/// came from. Walks source text rather than reflecting linked crates: the census
/// is open-world, and a collector only ever sees the crates that got linked, so a
/// new crate's slot would be silently absent instead of loudly missing.
fn production_slot_literals() -> Vec<(String, String)> {
    // Assembled at run time so this needle is not itself a hit.
    let needle = format!("\"{}.", "__pinion");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root is two levels above this crate");
    let mut found: Vec<(String, String)> = Vec::new();
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
            for line in src.lines() {
                let t = line.trim_start();
                // `commit.rs:168`'s rule: a test may spell the key by hand — that
                // is the pin. Production may not.
                if t.starts_with("#[cfg(test)]") {
                    break;
                }
                if t.starts_with("//") {
                    continue;
                }
                let Some(i) = line.find(&needle) else {
                    continue;
                };
                let tail = &line[i + 1..];
                let Some(end) = tail.find('"') else { continue };
                let key = &tail[..end];
                // The type's own predicate, not a hand-rolled one: what counts
                // as a slot key is `ProviderSlot`'s business, and reusing it
                // means the scan cannot drift from the constructors' assert.
                // It also excludes `SLOT_KEY_PREFIX`'s own definition — the bare
                // prefix is not a key, which is exactly what `is_slot_key` says.
                if !super::is_slot_key(key) {
                    continue;
                }
                found.push((key.to_string(), where_.clone()));
            }
        }
    }
    found
}

#[test]
fn r1366_no_production_slot_key_escapes_the_declaration_type() {
    let literals = production_slot_literals();
    assert!(
        !literals.is_empty(),
        "the walk found no slot keys at all — it is not reading the workspace",
    );
    let offenders: Vec<&(String, String)> = literals
        .iter()
        .filter(|(k, _)| !LEGACY_SLOT_KEYS.contains(&k.as_str()))
        .collect();
    assert!(
        offenders.is_empty(),
        "a provider slot key is spelled in production outside a `ProviderSlot` \
         declaration, and outside the shrinking legacy list. Declare it with \
         `ProviderSlot::inherited` / `::per_scope` — the scope argument is how \
         the verdict stops being a comment:\n{offenders:#?}",
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
        10,
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
fn r1366_every_legacy_key_still_exists() {
    // The other half of the staleness assert: a legacy entry whose slot was
    // deleted or renamed would sit here forever, quietly making the list look
    // longer than the real debt.
    let literals = production_slot_literals();
    let live: Vec<&str> = literals.iter().map(|(k, _)| k.as_str()).collect();
    let ghosts: Vec<&&str> = LEGACY_SLOT_KEYS
        .iter()
        .filter(|k| !live.contains(*k))
        .collect();
    assert!(
        ghosts.is_empty(),
        "the legacy list names keys that no production source spells — they were \
         migrated or removed, so delete these entries and lower the count: \
         {ghosts:?}",
    );
}
