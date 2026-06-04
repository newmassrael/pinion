//! R787 §3 §5.15 — Directory-listing substrate for the External(opaque)
//! escape hatch: the read side of the filesystem an own-rendered file
//! browser / picker walks. Pure-Rust trait + in-memory implementation;
//! the real-filesystem bridge (`std::fs::read_dir`) lives in the
//! `pinion-platform-storage` crate, exactly as [`crate::Storage`]'s
//! [`InMemoryStorage`](crate::storage::InMemoryStorage) /
//! `FileStorage` split keeps `std::fs` out of `pinion-core`.
//!
//! ## Why a peer of `Storage`, not an extension of it
//!
//! [`Storage`](crate::Storage) is a flat key→bytes store (the
//! `localStorage` shape — no hierarchy, no enumeration). A file picker
//! needs the orthogonal capability: enumerate the *children* of a
//! directory path. Folding "list a directory" into the key-value trait
//! would bake a filesystem tree model into a store whose whole point is
//! to be path-free, so this is its own minimal trait (one method) the
//! same way `Clipboard` is separate from `Storage`.
//!
//! ## Why total + read-only (no `Result`, no mkdir/write)
//!
//! The browse surface is read-only this round (R787): a picker lists
//! and navigates; creating / deleting entries is a later axis. IO
//! failures (missing dir, permission denied, not-a-directory) surface
//! as `None` — the same total-surface shape as `Storage::load`, so the
//! widget treats "cannot list" as "empty / unreachable" without a
//! fail-loud path. A caller that needs the underlying `std::io::Error`
//! instantiates the concrete `FsDirectory` and consults it directly.
//!
//! ## Path model
//!
//! Paths are opaque `&str` slices the implementation interprets.
//! [`InMemoryDirectory`] keys its synthetic tree by the verbatim path
//! string; the real `FsDirectory` hands the slice to `std::fs`. The
//! widget layer ([`crate::widgets::file_browser`]) owns the join /
//! parent arithmetic over `'/'`-separated paths so the trait stays a
//! pure "list the children of this path" primitive.

use std::cell::RefCell;
use std::collections::BTreeMap;

/// R787 §5.15 — one entry in a directory listing: its leaf `name` (no
/// path prefix) plus whether it is itself a directory (a navigable
/// child) or a leaf file (a selectable target). Deliberately minimal —
/// size / mtime / permissions are later axes a richer picker adds; the
/// browse + select flow needs only name + kind.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DirEntry {
    /// Leaf name (the final path component), e.g. `"main.rs"` or `"src"`.
    pub name: String,
    /// `true` when this entry is a directory (navigable); `false` for a
    /// regular file / symlink-to-file (a selectable leaf).
    pub is_dir: bool,
}

impl DirEntry {
    /// A directory entry (navigable child).
    #[must_use]
    pub fn dir(name: impl Into<String>) -> Self {
        Self { name: name.into(), is_dir: true }
    }

    /// A file entry (selectable leaf).
    #[must_use]
    pub fn file(name: impl Into<String>) -> Self {
        Self { name: name.into(), is_dir: false }
    }
}

/// Canonical listing order: directories first, then files, each group
/// sorted case-insensitively by name (the Files/Finder/Explorer
/// convention). Shared by every [`Directory`] impl so the browse order
/// is one source of truth (an `FsDirectory` reading an unordered
/// `read_dir` and the seeded [`InMemoryDirectory`] cannot disagree).
pub fn sort_entries(entries: &mut [DirEntry]) {
    entries.sort_by(|a, b| {
        (!a.is_dir, a.name.to_lowercase()).cmp(&(!b.is_dir, b.name.to_lowercase()))
    });
}

/// R787 §3 §5.15 — read-only directory enumeration surface (the §3
/// External(opaque) escape hatch's filesystem-browse capability). One
/// method: list the children of a path. `&self` (not `&mut self`) so a
/// `Rc<dyn Directory>` handle is shared across the widget state +
/// `External` bridge through immutable composition (same shape as
/// [`Storage`](crate::Storage)).
pub trait Directory {
    /// List the entries of the directory at `path`, in
    /// [`sort_entries`] order. Returns `None` when the path does not
    /// exist, is not a directory, or the listing errored (the total
    /// surface — the browser renders an empty / unreachable folder
    /// rather than surfacing an error type).
    fn read_dir(&self, path: &str) -> Option<Vec<DirEntry>>;
}

/// R787 §3 §5.15 — pure-Rust in-memory [`Directory`]: a synthetic tree
/// keyed by verbatim path string. The canonical test fixture and the
/// deterministic example backing (a file browser over a seeded sample
/// project), the headless analogue of
/// [`InMemoryStorage`](crate::storage::InMemoryStorage). The real-
/// filesystem `FsDirectory` (which calls `std::fs::read_dir`) lives in
/// `pinion-platform-storage`.
///
/// Each [`insert`](Self::insert)ed `(path, entries)` pair is one
/// directory's listing; [`read_dir`](Directory::read_dir) returns the
/// stored vector (re-sorted into canonical order) or `None` for an
/// unseeded path (a "directory does not exist" — same shape as a real
/// `read_dir` on a missing path).
#[derive(Debug, Default)]
pub struct InMemoryDirectory {
    tree: RefCell<BTreeMap<String, Vec<DirEntry>>>,
}

impl InMemoryDirectory {
    /// A fresh, empty synthetic tree (`read_dir` is `None` for every
    /// path until the first [`insert`](Self::insert)).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed the listing for one directory `path`. Entries are stored as
    /// given; [`read_dir`](Directory::read_dir) re-sorts into canonical
    /// order, so the caller need not pre-sort. Replaces any prior
    /// listing for `path` (builder-friendly: chainable seeding).
    pub fn insert(&self, path: impl Into<String>, entries: Vec<DirEntry>) {
        self.tree.borrow_mut().insert(path.into(), entries);
    }
}

impl Directory for InMemoryDirectory {
    fn read_dir(&self, path: &str) -> Option<Vec<DirEntry>> {
        let mut entries = self.tree.borrow().get(path).cloned()?;
        sort_entries(&mut entries);
        Some(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::{DirEntry, Directory, InMemoryDirectory, sort_entries};

    #[test]
    fn r787_dir_entry_constructors() {
        assert_eq!(DirEntry::dir("src"), DirEntry { name: "src".into(), is_dir: true });
        assert_eq!(DirEntry::file("main.rs"), DirEntry { name: "main.rs".into(), is_dir: false });
    }

    #[test]
    fn r787_sort_dirs_first_then_alpha_ci() {
        let mut e = vec![
            DirEntry::file("README.md"),
            DirEntry::dir("tests"),
            DirEntry::file("Cargo.toml"),
            DirEntry::dir("src"),
        ];
        sort_entries(&mut e);
        let names: Vec<&str> = e.iter().map(|x| x.name.as_str()).collect();
        // Dirs first (src, tests), then files (Cargo.toml, README.md);
        // each group case-insensitive alpha.
        assert_eq!(names, ["src", "tests", "Cargo.toml", "README.md"]);
    }

    #[test]
    fn r787_in_memory_read_dir_returns_sorted_listing() {
        let d = InMemoryDirectory::new();
        d.insert("/p", vec![DirEntry::file("b.txt"), DirEntry::dir("z"), DirEntry::dir("a")]);
        let listing = d.read_dir("/p").expect("seeded path lists");
        let names: Vec<&str> = listing.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["a", "z", "b.txt"], "dirs-first then alpha, re-sorted on read");
    }

    #[test]
    fn r787_in_memory_unseeded_path_is_none() {
        let d = InMemoryDirectory::new();
        assert_eq!(d.read_dir("/missing"), None, "unseeded path = directory does not exist");
    }

    #[test]
    fn r787_dyn_directory_polymorphism() {
        // Seed the concrete handle, then dispatch through `&dyn Directory`
        // (the shape the widget state holds: `Rc<dyn Directory>`).
        let concrete = std::rc::Rc::new(InMemoryDirectory::new());
        concrete.insert("/", vec![DirEntry::dir("x")]);
        let d: std::rc::Rc<dyn Directory> = concrete;
        assert_eq!(d.read_dir("/").map(|e| e.len()), Some(1));
        assert_eq!(d.read_dir("/missing"), None);
    }
}
