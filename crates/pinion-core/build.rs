//! Verifies the committed statechart emit; it no longer produces it.
//!
//! Until R1766 this script ran the SCE code generator on every build and wrote
//! fifteen `{widget}_sm.rs` modules into `OUT_DIR`. That made the code this
//! framework actually runs invisible to git: never reviewed, never blamed, and
//! readable only by building. R1765 had to build this crate twice against two
//! pins and diff two `OUT_DIR`s by hand just to answer whether an engine bump
//! changed anything here.
//!
//! The emit is tracked under `generated/` now. `tests/statechart_emit.rs` is
//! what writes it and what proves it equals a fresh run of the pinned
//! generator. This script carries the other half — the check cheap enough to
//! run on *every* build, so a chart edited without regenerating is reported at
//! `cargo build` rather than waiting for `cargo test`.
//!
//! It compares only hashes, and that bounds what it can catch:
//!
//! * a chart edited without regenerating — caught, via `source-hash`;
//! * the SCE pin moved to a revision with different Rust templates — caught,
//!   via `template-hash`;
//! * the emit hand-edited, the generator gone non-deterministic, or the derive
//!   list in the test changed — NOT caught here, because none of them move an
//!   input hash. Those belong to the regenerate-and-compare test, which CI runs
//!   on every push.

use std::path::{Path, PathBuf};

use sce_build::forge::drift::{self, DriftHashes, SourceSet};

/// Set by the regenerator so it can build this crate in order to rewrite the
/// very files this check is about. Without the escape hatch, editing a chart
/// would leave the tree unable to compile the test that repairs it.
const REGEN_ENV: &str = "PINION_REGEN_STATECHARTS";

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let generated = root.join("generated");

    println!("cargo::rerun-if-env-changed={REGEN_ENV}");
    println!("cargo::rerun-if-changed={}", generated.display());

    let set = SourceSet::collect(&root, None)
        .unwrap_or_else(|e| panic!("cannot read this crate's SCXML inputs: {e}"));
    for chart in set.contributing_paths() {
        println!("cargo::rerun-if-changed={}", chart.display());
    }

    if std::env::var(REGEN_ENV).is_ok_and(|v| v == "1") {
        return;
    }

    let template_dir = sce_build::find_template_dir();
    let hashes = DriftHashes {
        source_hash: set.digest(),
        template_hash: drift::compute_template_hash(&template_dir, &sce_cargo_lock(&template_dir))
            .unwrap_or_else(|e| panic!("cannot hash the pinned generator's templates: {e}")),
    };

    verify(&generated, &hashes);
}

/// SCE folds the generator's own `Cargo.lock` into `template-hash` as the
/// binary-identity surrogate; `find_template_dir()` resolves to
/// `<sce>/sce-build/../tools/codegen/templates/rust`, so the lock is four
/// levels up from there.
fn sce_cargo_lock(template_dir: &Path) -> PathBuf {
    template_dir
        .join("..")
        .join("..")
        .join("..")
        .join("..")
        .join("Cargo.lock")
}

/// Refuse the build when a tracked module was emitted from inputs that have
/// since moved. The message names the command that repairs it, because a gate
/// that reports a problem without saying what to run costs the reader the round
/// it was meant to save (R1760).
fn verify(generated: &Path, hashes: &DriftHashes) {
    let hint = format!("{REGEN_ENV}=1 cargo test -p pinion-core --test statechart_emit");

    let entries = std::fs::read_dir(generated).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\n\
             the committed statechart emit is missing; run `{hint}`",
            generated.display()
        )
    });

    let mut seen = 0usize;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_none_or(|x| x != "rs") {
            continue;
        }
        seen += 1;
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let embedded = drift::parse_embedded_hashes(&body).unwrap_or_else(|| {
            panic!(
                "{} carries no readable SCE drift header; run `{hint}`",
                path.display()
            )
        });
        assert!(
            embedded.source_hash_hex == hashes.source_hex(),
            "{} was emitted from a different set of SCXML charts \
             (header {}, inputs now {}); run `{hint}`",
            path.display(),
            embedded.source_hash_hex,
            hashes.source_hex(),
        );
        assert!(
            embedded.template_hash_hex == hashes.template_hex(),
            "{} was emitted by a different generator \
             (header {}, pinned generator now {}); run `{hint}`",
            path.display(),
            embedded.template_hash_hex,
            hashes.template_hex(),
        );
    }

    assert!(
        seen > 0,
        "{} holds no generated module; run `{hint}`",
        generated.display()
    );
}
