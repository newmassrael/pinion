//! The committed statechart emit is the code this framework actually runs, and
//! this is the gate that keeps it equal to what the pinned generator produces.
//!
//! Before R1766 the fifteen `{widget}_sm.rs` modules were generated into
//! `OUT_DIR` on every build and never tracked, so 9,591 lines of the state
//! machine deciding what every widget does existed only inside a hash-named
//! temporary directory: git could not see it, no diff had ever been read, and
//! answering "did this SCE pin bump change what we run" cost two builds and a
//! hand comparison (R1765 paid exactly that).
//!
//! The emit is tracked now, and this file is both halves of keeping it honest:
//!
//! * default — regenerate in memory and assert the committed bytes are equal,
//!   which catches a hand edit, a stale tree and a non-deterministic generator;
//! * with `PINION_REGEN_STATECHARTS=1` — write instead of assert.
//!
//! They are one piece of code on purpose. A separate regenerator and checker
//! are two accounts of the same fact and can disagree; here the thing that
//! writes the file is the thing that judges it.
//!
//! `build.rs` carries the cheap half — it compares the drift hashes embedded in
//! each committed file against the current inputs on every build, so an edited
//! chart is reported without waiting for a test run.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use sce_build::forge::drift::{self, DriftHashes, SourceSet};
use sce_build::generator::{Language, StatechartCodegenOptions};

/// Setting this to `1` turns the gate into the regenerator. Named rather than
/// implied because rewriting a tracked artifact is not something a test run
/// should be able to do by accident.
const REGEN_ENV: &str = "PINION_REGEN_STATECHARTS";

/// `generated-at` is documented by SCE as informational and feeds neither
/// hash, so it is pinned to zero: a tracked artifact whose bytes move with the
/// wall clock cannot be byte-compared, which is this file's whole job.
const GENERATED_AT: u64 = 0;

/// ★★★★★ R1796 §5.4 §3 — the Event I/O Processor types THIS BUILD serves.
///
/// W3C SCXML 6.2.5 leaves the processor set open, and SCE decides at COMPILE time
/// which way a `<send type="…">` goes: a declared type emits a dispatch to the
/// host, an undeclared one emits an `error.execution` raise. So this list is
/// not configuration — it is the difference between a door and a wall, and it
/// has to be here rather than at a call site because every chart in the crate
/// is emitted by this one function.
///
/// ⚠ **It is the FRAMEWORK's own list and must stay that way.** §3 fixes
/// `Effect(opaque)` and `External(opaque)` as the only two escape hatches; a
/// host-served send whose handler an APPLICATION registers would be a third,
/// and adding one is a spec round rather than an edit here. See
/// `pinion_core::host_served` for the argument in full.
const HOST_PROCESSOR_TYPES: &[&str] = &["pinion.fixture.host"];

/// Injected onto every generated `{machine}State` enum. The rationale for each
/// lives on the SCE-002 / SCE-004 ledger entries; what matters here is that
/// this list is an input to the emit, so changing it without regenerating is a
/// staleness the drift hashes cannot see — only this gate can.
const STATE_EXTRA_DERIVES: [&str; 3] = [
    "serde::Serialize",
    "serde::Deserialize",
    "pinion_derive::WidgetStateName",
];

/// As above, for the generated `{machine}Event` enum.
const EVENT_EXTRA_DERIVES: [&str; 1] = ["pinion_derive::WidgetEventName"];

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn generated_dir() -> PathBuf {
    crate_root().join("generated")
}

/// SCE's `template-hash` rule folds the generator's own `Cargo.lock` in beside
/// its template tree as the binary-identity surrogate, and
/// `find_template_dir()` resolves to `<sce>/tools/codegen/templates/rust`.
fn sce_cargo_lock(template_dir: &Path) -> PathBuf {
    template_dir
        .join("..")
        .join("..")
        .join("..")
        .join("..")
        .join("Cargo.lock")
}

/// The two drift digests, in SCE's own spelling, for the current state of this
/// crate's charts
/// and the pinned generator's Rust templates.
///
/// The template root is the **Rust** subdirectory rather than the whole
/// template tree, because that is the directory the Rust arm is rooted at — a
/// C++ or Kotlin template cannot reach this emit. R1765 measured the same fact
/// from the other side: that bump moved three base-level templates and this
/// tree's generated Rust did not change by one byte.
fn current_hashes() -> DriftHashes {
    let template_dir = sce_build::find_template_dir();
    let source_hash =
        drift::compute_source_hash(&crate_root(), None).expect("hash this crate's SCXML inputs");
    let template_hash = drift::compute_template_hash(&template_dir, &sce_cargo_lock(&template_dir))
        .expect("hash the pinned generator's Rust templates");
    DriftHashes {
        source_hash,
        template_hash,
    }
}

/// Every chart this crate compiles, named the way the generator must see it.
///
/// The population is `SourceSet`'s own, not a list kept beside it: the same
/// walk that feeds `source-hash` decides what gets emitted, so a chart added to
/// the tree cannot be hashed-but-not-compiled. The path is re-expressed
/// relative to the crate root because the generator writes it verbatim into
/// each file's `// From:` line, and an absolute one would make the artifact
/// machine-specific.
fn chart_paths() -> Vec<String> {
    let root = crate_root();
    let set = SourceSet::collect(&root, None).expect("collect this crate's SCXML inputs");
    let mut rel: Vec<String> = set
        .contributing_paths()
        .into_iter()
        .map(|abs| {
            abs.strip_prefix(&root)
                .unwrap_or(&abs)
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    rel.sort();
    rel
}

/// Compile one chart to the exact bytes the tracked file must hold.
///
/// Two adjustments happen at this boundary, and both are the same kind of
/// thing: what the generator writes for ITS repository is not valid in ours.
///
/// The inner-attribute strip is not cosmetic — `include!()` does not permit
/// `#![...]` or `//!` in expansion position, and every one of these modules is
/// consumed through an `include!`. It ran in `build.rs` before R1766, which
/// meant the post-processed form — the form actually compiled — was not what
/// any tool could see. It runs here now, where that form *is* the artifact.
///
/// The section-sigil rewrite is the citation half. In a file this workspace
/// tracks, `§x` is a claim about THIS store, and every one the generator emits
/// is a claim about SCE's — `scxml-D`, `synth-5-J-2`. Tracking the emit turned
/// those into 67 false citations overnight and the citation gate rejected the
/// commit, which is the gate being right. Re-spelling them on import is the
/// honest repair: the reference survives, the sigil that means "pinion's store"
/// does not. It is total rather than per-token because none of the generator's
/// section numbers can ever be ours, and it is confined to comment lines
/// because a blind replace would reach string literals — measured before it was
/// written: all 67 sit on lines whose trimmed form starts with `//`.
fn respell_foreign_section_sigils(line: &str) -> String {
    if line.trim_start().starts_with("//") {
        line.replace('§', "SCE ")
    } else {
        line.to_string()
    }
}

fn emit_one(rel_path: &str, template_dir: &Path, hashes: &DriftHashes) -> String {
    let options = StatechartCodegenOptions {
        no_std: false,
        state_extra_derives: STATE_EXTRA_DERIVES
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        event_extra_derives: EVENT_EXTRA_DERIVES
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        host_processor_types: HOST_PROCESSOR_TYPES
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        host_invoker_types: Vec::new(),
    };
    let output = sce_build::compile_scxml_lang_typed_with_section(
        rel_path,
        template_dir,
        Language::Rust,
        None,
        None,
        &options,
    )
    .unwrap_or_else(|e| panic!("compile {rel_path}: {e}"));

    let (_filename, code) = output
        .files
        .into_iter()
        .next()
        .expect("the Rust arm emits exactly one file per chart");

    let stripped: String = code
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("#![") && !trimmed.starts_with("//!")
        })
        .map(respell_foreign_section_sigils)
        .collect::<Vec<_>>()
        .join("\n");

    drift::prepend_or_replace_header(&format!("{stripped}\n"), hashes, GENERATED_AT, "//")
}

/// Filename the chart at `rel_path` is emitted to.
fn emitted_name(rel_path: &str) -> String {
    let stem = Path::new(rel_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_else(|| panic!("chart path has no stem: {rel_path}"));
    format!("{stem}_sm.rs")
}

/// The whole emit, keyed by filename.
fn emit_all() -> BTreeMap<String, String> {
    let template_dir = sce_build::find_template_dir();
    let hashes = current_hashes();
    chart_paths()
        .into_iter()
        .map(|rel| {
            let name = emitted_name(&rel);
            let code = emit_one(&rel, &template_dir, &hashes);
            (name, code)
        })
        .collect()
}

/// What `generated/` currently holds.
fn committed_files() -> BTreeMap<String, String> {
    let dir = generated_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return BTreeMap::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "rs"))
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let body = std::fs::read_to_string(e.path())
                .unwrap_or_else(|err| panic!("read {}: {err}", e.path().display()));
            (name, body)
        })
        .collect()
}

fn regen_requested() -> bool {
    std::env::var(REGEN_ENV).is_ok_and(|v| v == "1")
}

fn regen_hint() -> String {
    format!("{REGEN_ENV}=1 cargo test -p pinion-core --test statechart_emit")
}

#[test]
fn committed_statechart_emit_equals_what_the_pinned_generator_produces() {
    let fresh = emit_all();
    assert!(
        !fresh.is_empty(),
        "no chart was compiled — the SourceSet walk found nothing under {}",
        crate_root().display()
    );

    if regen_requested() {
        let dir = generated_dir();
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("create {}: {e}", dir.display()));
        for (name, body) in &fresh {
            let path = dir.join(name);
            std::fs::write(&path, body).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
        }
        for name in committed_files().keys() {
            if !fresh.contains_key(name) {
                let path = dir.join(name);
                std::fs::remove_file(&path)
                    .unwrap_or_else(|e| panic!("remove stale {}: {e}", path.display()));
            }
        }
        return;
    }

    let committed = committed_files();
    let fresh_names: BTreeSet<&String> = fresh.keys().collect();
    let committed_names: BTreeSet<&String> = committed.keys().collect();
    assert_eq!(
        fresh_names,
        committed_names,
        "the tracked emit and the chart set disagree; run `{}`",
        regen_hint()
    );

    for (name, expected) in &fresh {
        let actual = &committed[name];
        assert!(
            actual == expected,
            "generated/{name} is not what the pinned generator produces \
             ({} committed bytes vs {} fresh); run `{}`",
            actual.len(),
            expected.len(),
            regen_hint()
        );
    }
}

/// The header is the cheap check `build.rs` runs on every build, so it has to
/// be present and current in every tracked file — a file whose header cannot be
/// parsed would make that check silently vacuous.
#[test]
fn every_committed_module_carries_a_current_drift_header() {
    if regen_requested() {
        return;
    }
    let hashes = current_hashes();
    let committed = committed_files();
    assert!(
        !committed.is_empty(),
        "generated/ holds no module; run `{}`",
        regen_hint()
    );
    for (name, body) in &committed {
        let embedded = drift::parse_embedded_hashes(body)
            .unwrap_or_else(|| panic!("generated/{name} carries no readable drift header"));
        assert_eq!(
            embedded.source_hash_hex,
            hashes.source_hex(),
            "generated/{name} was emitted from different charts; run `{}`",
            regen_hint()
        );
        assert_eq!(
            embedded.template_hash_hex,
            hashes.template_hex(),
            "generated/{name} was emitted by a different generator; run `{}`",
            regen_hint()
        );
    }
}

/// No committed module may carry the section sigil. In a file this workspace
/// tracks, `§x` is a citation of THIS store, and `validate-code-refs` rejects
/// one that resolves to nothing — which is what it did the first time these
/// files were staged. The generator's own section numbers are re-spelled on
/// import; this is what keeps that true as the emit changes.
#[test]
fn no_committed_module_claims_a_section_of_this_store() {
    if regen_requested() {
        return;
    }
    for (name, body) in &committed_files() {
        assert!(
            !body.contains('§'),
            "generated/{name} carries a section sigil, which reads as a citation \
             of pinion's own store; the generator's numbers are SCE's"
        );
    }
}

/// The emit must not depend on where it was produced. A tracked artifact that
/// embeds an absolute path is one that cannot be reproduced on another machine,
/// and the `.include.rs` shims SCE also writes are exactly that — which is why
/// this tree stopped carrying them.
#[test]
fn the_committed_emit_names_no_absolute_path() {
    if regen_requested() {
        return;
    }
    let root = crate_root();
    let root_str = root.to_string_lossy();
    for (name, body) in &committed_files() {
        assert!(
            !body.contains(root_str.as_ref()),
            "generated/{name} embeds this machine's crate path"
        );
    }
}
