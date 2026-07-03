//! R1067 §5.37.11 — OS system-font *enumeration* for the §5.37 self-hosted text
//! engine.
//!
//! The §5.37 engine (`pinion-text-font`) parses + shapes + rasterizes fonts but,
//! by charter, knows nothing about *where fonts live on disk* — its `fallback`
//! module notes that "OS enumeration (discovering installed fonts) is a separate
//! platform layer". This crate is that layer, and ONLY that layer: it walks the
//! operating system's standard font directories and hands back the font files it
//! finds (paths + raw bytes). It is the engine's analogue of the directory-walk
//! half of `fontconfig` / `CoreText` / `DirectWrite`.
//!
//! ## Scope: enumerate, do not select or parse
//!
//! This crate deliberately does NOT choose "the default sans" or judge a font's
//! family / weight / style. That is *font* knowledge (the `name` / `OS/2` tables)
//! and belongs to the layer that can parse — `pinion_runtime::text_engine`, which
//! depends on `pinion-text-font`. Selecting a face from filenames here would put
//! engine knowledge in a parser-free OS layer and split the "what is a regular
//! sans" authority across two crates. So the API is: list the font files
//! ([`enumerate_fonts`]) and read one ([`read_font_bytes`]); the consumer parses
//! and selects. There is likewise no sfnt magic-byte sniff — `Font::from_bytes`
//! is the one authority on "is this a usable font" (parse = validation), so a
//! second copy of the sfnt version tags here would be redundant SSOT.
//!
//! ## Why pure `std::fs` (no `fontconfig`, no `fc-match` subprocess)
//!
//! §5.37 is "lifetime canonical — no black box, fully introspectable". A C
//! `fontconfig` link or a shell-out to `fc-match` would reintroduce exactly the
//! opaque / environment-non-deterministic dependency §5.37 exists to remove. A
//! direct directory scan is deterministic given the filesystem and keeps the
//! crate dependency-free, matching the thin OS-focused shape of its
//! `pinion-platform-*` siblings.
//!
//! ## Font policy (§5.37.11): system source, not bundled
//!
//! The production font source is the **OS's installed fonts**, discovered here —
//! NOT fonts vendored into the repo. The Noto Sans / Nanum Gothic files under
//! `pinion-text-font/tests/fonts/` are §5.37.1 *parser fixtures* only; committing
//! a production font is disallowed, so system discovery is the canonical (and
//! final) design rather than a stopgap.
//!
//! ## Platform coverage
//!
//! Linux is implemented and verified. macOS / Windows use the same fs-scan shape
//! but are deferred until those OS CI runners exist to verify them (the project's
//! Mac/Win-native deferral) — [`system_font_dirs`] returns no directories there,
//! making the deferral explicit rather than scanning an unverified path.

use std::path::{Path, PathBuf};

/// The maximum directory-recursion depth when enumerating fonts. Standard font
/// trees are shallow (family → style files); this bounds the walk against a
/// pathological deep tree without needing per-path cycle bookkeeping (symlinked
/// directories are never recursed — see [`enumerate_fonts`]).
const MAX_SCAN_DEPTH: u32 = 8;

/// The operating system's standard font directories that currently exist.
///
/// Linux: `/usr/share/fonts`, `/usr/local/share/fonts`, and the per-user
/// `~/.fonts`, `~/.local/share/fonts`, and `$XDG_DATA_HOME/fonts`. Only existing
/// directories are returned (a machine without `~/.fonts` simply omits it).
///
/// On non-Linux targets this returns an empty list — see the crate docs'
/// "Platform coverage" note (Mac/Win deferred).
#[must_use]
#[cfg(target_os = "linux")]
pub fn system_font_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/usr/share/fonts"),
        PathBuf::from("/usr/local/share/fonts"),
    ];
    if let Some(home) = home_dir() {
        dirs.push(home.join(".fonts"));
        dirs.push(home.join(".local/share/fonts"));
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        if !xdg.is_empty() {
            dirs.push(PathBuf::from(xdg).join("fonts"));
        }
    }
    dirs.retain(|d| d.is_dir());
    dirs
}

/// macOS / Windows font directories are deferred (no OS CI runner to verify the
/// scan against) — return none so the deferral is explicit. See the crate docs.
#[must_use]
#[cfg(not(target_os = "linux"))]
pub fn system_font_dirs() -> Vec<PathBuf> {
    Vec::new()
}

/// Every `.ttf` / `.otf` file found beneath the [`system_font_dirs`], returned
/// sorted + de-duplicated for a deterministic order across runs.
///
/// Matching is by extension only — validation that a file is a real, usable font
/// is the consumer's parse step (`Font::from_bytes`), not this layer's job.
/// Symlinked *files* are followed (font families commonly symlink their members),
/// but symlinked *directories* are never recursed into — that, plus the
/// `MAX_SCAN_DEPTH` bound, keeps the walk cycle-free without per-path
/// bookkeeping. Unreadable directories are skipped silently (a permission error
/// on one font dir must not abort enumeration).
#[must_use]
pub fn enumerate_fonts() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in system_font_dirs() {
        collect_fonts(&dir, MAX_SCAN_DEPTH, &mut out);
    }
    out.sort();
    out.dedup();
    out
}

/// Read a font file's bytes.
///
/// # Errors
///
/// Propagates the [`std::io::Error`] from reading `path` (missing file,
/// permission denied, etc.).
pub fn read_font_bytes(path: &Path) -> std::io::Result<Vec<u8>> {
    std::fs::read(path)
}

/// Recursively collect `.ttf` / `.otf` files under `dir` into `out`, bounded by
/// `depth`. See [`enumerate_fonts`] for the symlink / cycle policy.
fn collect_fonts(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_symlink() {
            // Follow a symlinked file (resolve its target's type), but never
            // recurse a symlinked directory — that is the cycle guard.
            if std::fs::metadata(&path).is_ok_and(|m| m.is_file()) && has_font_ext(&path) {
                out.push(path);
            }
        } else if file_type.is_dir() {
            collect_fonts(&path, depth - 1, out);
        } else if file_type.is_file() && has_font_ext(&path) {
            out.push(path);
        }
    }
}

/// `true` when `path`'s extension is `ttf` or `otf` (case-insensitive).
fn has_font_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("ttf") || e.eq_ignore_ascii_case("otf"))
}

#[cfg(target_os = "linux")]
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_font_dirs_are_existing_directories() {
        // The `retain(is_dir)` contract: every returned path exists + is a dir.
        for dir in system_font_dirs() {
            assert!(dir.is_dir(), "{} should be a directory", dir.display());
        }
    }

    #[test]
    fn enumerate_returns_only_ttf_otf_sorted() {
        let fonts = enumerate_fonts();
        for window in fonts.windows(2) {
            assert!(window[0] <= window[1], "enumerate_fonts must be sorted");
        }
        for path in &fonts {
            assert!(has_font_ext(path), "{} is not a .ttf/.otf", path.display());
        }
    }

    #[test]
    fn enumerated_fonts_are_readable_when_present() {
        // Forcing consumer (FS-deterministic): on a font-bearing host, at least
        // the first enumerated file reads into a non-empty buffer. A font-less
        // environment yields an empty list (the parse-side proof lives in the
        // pinion-runtime selection test). Validation that the bytes ARE a font is
        // the consumer's parse step, not this layer's.
        if let Some(path) = enumerate_fonts().first() {
            let bytes = read_font_bytes(path).expect("enumerated font is readable");
            assert!(!bytes.is_empty(), "a font file is non-empty");
        }
    }
}
