//! Embeds the commit this generator was built from, as
//! [`pinion_forge::GENERATOR_COMMIT`].
//!
//! # Why a commit and not the crate version
//!
//! Every pinion crate carries `version = "0.0.1"` from the workspace, so
//! the version identifies nothing: two builds a hundred rounds apart
//! report the same string. `pinion-forge` emits a machine-readable
//! diagnostic stream whose `v` field only moves on a *breaking* shape
//! change, which leaves a consumer holding a record with no way to ask
//! which build produced it — and pinion's wire surfaces are pre-release,
//! so "pin a commit" is the only guarantee on offer. A guarantee a
//! consumer cannot act on is not one, and it cannot act on this without
//! the payload naming the commit.
//!
//! This mirrors `sce-build`'s `SCE_GIT_COMMIT` (see its `build.rs`), for
//! the same reason and with the same shape — the SCE v1 diagnostic
//! schema `pinion-forge` mirrors makes `generator` a *required* field,
//! and a mirror that drops a required field is no longer a mirror.
//!
//! # Scope: the committed state, deliberately
//!
//! No dirty-worktree flag. The rerun triggers below watch the ref, not
//! the worktree, so a cleanliness claim computed here would go stale on
//! the next edit — and a stale "clean" reads exactly like a true one.
//! The stamp names the committed state the build started from, which is
//! what a pinned or hermetic build has.
//!
//! `unknown` when there is no git checkout to read — a consumer taking
//! `pinion-forge` as a git or vendored dependency builds from a cargo
//! checkout with no `.git`, and that is a supported build, not an error.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let commit = git_commit().unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=PINION_GIT_COMMIT={commit}");
}

/// Short commit of `HEAD`, or `None` when git cannot answer.
///
/// Width is 12 hex digits, matching `sce-build` so the two stamps that
/// travel together in one diagnostic stream read alike.
fn git_commit() -> Option<String> {
    let git_dir = git_dir()?;
    // Rebuild when the ref moves, so the embedded value cannot go stale
    // within a working session. A stamp that silently names the wrong
    // commit is the failure mode this surface exists to remove.
    watch(&git_dir.join("HEAD"));
    if let Some(reference) = head_ref(&git_dir) {
        watch(&git_dir.join(&reference));
        // A ref that lives in `packed-refs` has no file of its own.
        watch(&git_dir.join("packed-refs"));
    }

    let out = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let commit = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!commit.is_empty()).then_some(commit)
}

/// Resolve the `.git` directory, following the `gitdir:` indirection a
/// worktree or submodule checkout uses. `cargo` runs this script with
/// the crate manifest directory as the working directory, so the answer
/// is the enclosing pinion checkout.
fn git_dir() -> Option<PathBuf> {
    let out = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let dir = PathBuf::from(String::from_utf8(out.stdout).ok()?.trim());
    Some(if dir.is_absolute() {
        dir
    } else {
        std::env::current_dir().ok()?.join(dir)
    })
}

/// Symbolic ref `HEAD` points at (`refs/heads/main`), if any. A detached
/// HEAD has none, and the `HEAD` watch alone covers that case.
fn head_ref(git_dir: &Path) -> Option<String> {
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let reference = head.trim().strip_prefix("ref: ")?;
    Some(reference.to_string())
}

/// Ask cargo to rerun this script when `path` changes.
fn watch(path: &Path) {
    println!("cargo:rerun-if-changed={}", path.display());
}
