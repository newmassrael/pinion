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
//! ## Why total (no `Result`)
//!
//! Every method has a total surface: [`read_dir`](Directory::read_dir)
//! returns `None` on any failure (missing dir, permission denied,
//! not-a-directory) — the same shape as `Storage::load`, so the widget
//! treats "cannot list" as "empty / unreachable" without a fail-loud
//! path. The R789 mutation methods ([`create_dir`](Directory::create_dir)
//! / [`create_file`](Directory::create_file) /
//! [`remove`](Directory::remove)) return `bool` success rather than a
//! `Result` for the same reason — a file manager re-reads the listing to
//! observe the outcome, it does not branch on an error kind. A caller
//! that needs the underlying `std::io::Error` instantiates the concrete
//! `FsDirectory` and consults it directly.
//!
//! ## Why write is default-no-op on the same trait (not a `DirectoryWrite`)
//!
//! The mutation methods carry a default `false` (unsupported) body, so a
//! read-only [`Directory`] (a bundled resource tree, a remote listing)
//! inherits a safe no-op and only a writable backing
//! ([`InMemoryDirectory`], `FsDirectory`) overrides them. This mirrors
//! [`Storage`](crate::Storage) bundling `load` + `save` + `remove` on one
//! trait (a file picker holds a single `Rc<dyn Directory>` for both
//! browse and manage), and the default-no-op extension idiom the rest of
//! the codebase uses to grow an object-safe trait without splitting the
//! handle.
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

    /// R789 — create a new directory at the absolute `path` (its parent
    /// must already exist). Returns `true` on success, `false` when the
    /// parent does not exist, an entry of that name already exists, or
    /// the backing is read-only (the default). A successful call makes
    /// the new directory appear in its parent's [`read_dir`].
    fn create_dir(&self, path: &str) -> bool {
        let _ = path;
        false
    }

    /// R789 — create a new empty file at the absolute `path` (its parent
    /// must already exist). Returns `true` on success, `false` when the
    /// parent does not exist, an entry of that name already exists, or
    /// the backing is read-only (the default).
    fn create_file(&self, path: &str) -> bool {
        let _ = path;
        false
    }

    /// R789 — remove the entry at the absolute `path` (a file, or a
    /// directory and its subtree). Returns `true` when an entry was
    /// removed, `false` when nothing was there or the backing is
    /// read-only (the default). Idempotent — removing a missing path is a
    /// `false` no-op, never a panic.
    fn remove(&self, path: &str) -> bool {
        let _ = path;
        false
    }

    /// R791 — rename / move the entry at absolute `from` to absolute `to`
    /// (both parents must already exist). Returns `true` on success,
    /// `false` when `from` does not exist, `to`'s name is already taken,
    /// `to`'s parent does not exist, or the backing is read-only (the
    /// default). A successful directory rename carries its whole subtree.
    /// The file-manager "Rename" affordance; the same-parent case (only
    /// the leaf name changes) is the common one.
    fn rename(&self, from: &str, to: &str) -> bool {
        let _ = (from, to);
        false
    }
}

/// Split an absolute `'/'`-path into `(parent, leaf)` for the in-memory
/// mutation methods: `"/proj/x"` → `("/proj", "x")`, `"/x"` →
/// `("/", "x")`. A trailing slash is ignored. A path with no separator
/// (or the bare root `"/"`) yields an empty leaf, which the callers treat
/// as "no creatable target" (a `false` no-op). The widget layer owns the
/// navigation join/parent arithmetic ([`crate::widgets::file_browser`]);
/// this is the backing's own internal split for listing maintenance.
fn split_parent_leaf(path: &str) -> (String, String) {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rsplit_once('/') {
        Some((parent, leaf)) => {
            let parent = if parent.is_empty() { "/".to_string() } else { parent.to_string() };
            (parent, leaf.to_string())
        }
        None => (String::new(), trimmed.to_string()),
    }
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

    fn create_dir(&self, path: &str) -> bool {
        let (parent, leaf) = split_parent_leaf(path);
        if leaf.is_empty() {
            return false;
        }
        let mut tree = self.tree.borrow_mut();
        match tree.get_mut(&parent) {
            Some(listing) if !listing.iter().any(|e| e.name == leaf) => {
                listing.push(DirEntry::dir(leaf));
            }
            // Parent unseeded (does not exist) or the name is taken.
            _ => return false,
        }
        // The new directory lists empty until something is created inside.
        tree.insert(path.to_string(), Vec::new());
        true
    }

    fn create_file(&self, path: &str) -> bool {
        let (parent, leaf) = split_parent_leaf(path);
        if leaf.is_empty() {
            return false;
        }
        let mut tree = self.tree.borrow_mut();
        match tree.get_mut(&parent) {
            Some(listing) if !listing.iter().any(|e| e.name == leaf) => {
                listing.push(DirEntry::file(leaf));
                true
            }
            _ => false,
        }
    }

    fn remove(&self, path: &str) -> bool {
        let (parent, leaf) = split_parent_leaf(path);
        if leaf.is_empty() {
            return false;
        }
        let mut tree = self.tree.borrow_mut();
        let removed = match tree.get_mut(&parent) {
            Some(listing) => {
                let before = listing.len();
                listing.retain(|e| e.name != leaf);
                listing.len() != before
            }
            None => false,
        };
        if removed {
            // Drop the removed entry's own listing (if it was a directory);
            // a file has no subtree key, so this is a harmless no-op.
            tree.remove(path);
        }
        removed
    }

    fn rename(&self, from: &str, to: &str) -> bool {
        let (from_parent, from_leaf) = split_parent_leaf(from);
        let (to_parent, to_leaf) = split_parent_leaf(to);
        if from_leaf.is_empty() || to_leaf.is_empty() {
            return false;
        }
        let mut tree = self.tree.borrow_mut();
        // Source must exist in its parent listing; capture its `is_dir`.
        let Some(entry) =
            tree.get(&from_parent).and_then(|l| l.iter().find(|e| e.name == from_leaf).cloned())
        else {
            return false;
        };
        // Destination parent must exist and the destination name must be
        // free (a rename never creates a parent or overwrites). Renaming
        // to the same path lands here as "name taken" → a `false` no-op.
        if !tree.contains_key(&to_parent)
            || tree.get(&to_parent).is_some_and(|l| l.iter().any(|e| e.name == to_leaf))
        {
            return false;
        }
        // Move the entry between listings (same listing in the common
        // same-parent case — sequential borrows, never aliased).
        if let Some(l) = tree.get_mut(&from_parent) {
            l.retain(|e| e.name != from_leaf);
        }
        let is_dir = entry.is_dir;
        tree.get_mut(&to_parent)
            .expect("destination parent existence checked above")
            .push(DirEntry { name: to_leaf, is_dir });
        // A directory carries its own listing key plus every descendant
        // key; re-key the whole subtree by swapping the `from` prefix.
        if is_dir {
            let subtree_prefix = format!("{from}/");
            let keys: Vec<String> = tree
                .keys()
                .filter(|k| k.as_str() == from || k.starts_with(&subtree_prefix))
                .cloned()
                .collect();
            for k in keys {
                let new_key = format!("{to}{}", &k[from.len()..]);
                let listing = tree.remove(&k).expect("key was just enumerated");
                tree.insert(new_key, listing);
            }
        }
        true
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

    // ----- R789 write surface -----

    #[test]
    fn r789_split_parent_leaf() {
        use super::split_parent_leaf;
        assert_eq!(split_parent_leaf("/proj/x"), ("/proj".into(), "x".into()));
        assert_eq!(split_parent_leaf("/x"), ("/".into(), "x".into()));
        assert_eq!(split_parent_leaf("/proj/x/"), ("/proj".into(), "x".into()), "trailing slash ignored");
        assert_eq!(split_parent_leaf("/"), (String::new(), String::new()), "root has no leaf");
    }

    #[test]
    fn r789_create_dir_appears_in_parent_and_lists_empty() {
        let d = InMemoryDirectory::new();
        d.insert("/proj", vec![DirEntry::file("Cargo.toml")]);
        assert!(d.create_dir("/proj/src"), "create succeeds under an existing parent");
        let names: Vec<String> =
            d.read_dir("/proj").unwrap().iter().map(|e| e.name.clone()).collect();
        // Dirs sort first: src before Cargo.toml.
        assert_eq!(names, ["src", "Cargo.toml"], "new dir appears in the parent listing");
        assert_eq!(d.read_dir("/proj/src"), Some(vec![]), "the new dir lists empty");
    }

    #[test]
    fn r789_create_file_appears_in_parent() {
        let d = InMemoryDirectory::new();
        d.insert("/proj", vec![DirEntry::dir("src")]);
        assert!(d.create_file("/proj/README.md"));
        let names: Vec<String> =
            d.read_dir("/proj").unwrap().iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, ["src", "README.md"], "new file appears (after the dir)");
    }

    #[test]
    fn r789_create_rejects_missing_parent_and_duplicates() {
        let d = InMemoryDirectory::new();
        d.insert("/proj", vec![DirEntry::dir("src")]);
        assert!(!d.create_dir("/nope/x"), "missing parent rejected");
        assert!(!d.create_dir("/proj/src"), "duplicate name rejected");
        assert!(!d.create_file("/proj/src"), "duplicate (as file) rejected");
    }

    #[test]
    fn r789_remove_drops_entry_and_subtree() {
        let d = InMemoryDirectory::new();
        d.insert("/proj", vec![DirEntry::dir("src"), DirEntry::file("README.md")]);
        d.insert("/proj/src", vec![DirEntry::file("main.rs")]);
        assert!(d.remove("/proj/src"), "removing an existing dir succeeds");
        let names: Vec<String> =
            d.read_dir("/proj").unwrap().iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, ["README.md"], "removed dir gone from the parent listing");
        assert_eq!(d.read_dir("/proj/src"), None, "removed dir's subtree dropped");
        assert!(!d.remove("/proj/ghost"), "removing a missing entry is a false no-op");
    }

    // ----- R791 rename surface -----

    #[test]
    fn r791_rename_file_in_place_changes_leaf() {
        let d = InMemoryDirectory::new();
        d.insert("/proj", vec![DirEntry::dir("src"), DirEntry::file("old.txt")]);
        assert!(d.rename("/proj/old.txt", "/proj/new.txt"), "same-parent file rename succeeds");
        let names: Vec<String> =
            d.read_dir("/proj").unwrap().iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, ["src", "new.txt"], "the leaf name changed in place");
    }

    #[test]
    fn r791_rename_directory_carries_its_subtree() {
        let d = InMemoryDirectory::new();
        d.insert("/proj", vec![DirEntry::dir("old")]);
        d.insert("/proj/old", vec![DirEntry::file("a.rs"), DirEntry::dir("deep")]);
        d.insert("/proj/old/deep", vec![DirEntry::file("b.rs")]);
        assert!(d.rename("/proj/old", "/proj/new"), "directory rename succeeds");
        // The parent listing shows the new name; the whole subtree re-keys.
        let names: Vec<String> =
            d.read_dir("/proj").unwrap().iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, ["new"], "parent lists the renamed directory");
        assert_eq!(d.read_dir("/proj/old"), None, "old subtree key gone");
        let inner: Vec<String> =
            d.read_dir("/proj/new").unwrap().iter().map(|e| e.name.clone()).collect();
        assert_eq!(inner, ["deep", "a.rs"], "the renamed dir keeps its listing");
        assert_eq!(
            d.read_dir("/proj/new/deep").unwrap().iter().map(|e| e.name.clone()).collect::<Vec<_>>(),
            ["b.rs"],
            "the deep descendant re-keyed under the new path",
        );
    }

    #[test]
    fn r791_rename_rejects_taken_name_missing_source_and_self() {
        let d = InMemoryDirectory::new();
        d.insert("/proj", vec![DirEntry::file("a.txt"), DirEntry::file("b.txt")]);
        assert!(!d.rename("/proj/a.txt", "/proj/b.txt"), "destination name already taken");
        assert!(!d.rename("/proj/ghost.txt", "/proj/c.txt"), "missing source rejected");
        assert!(!d.rename("/proj/a.txt", "/proj/a.txt"), "rename to the same name is a no-op");
        assert!(!d.rename("/proj/a.txt", "/nope/a.txt"), "missing destination parent rejected");
        // The listing is untouched by every rejected rename.
        let names: Vec<String> =
            d.read_dir("/proj").unwrap().iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, ["a.txt", "b.txt"], "rejected renames leave the listing intact");
    }

    #[test]
    fn r789_read_only_default_is_noop() {
        // A bare trait impl with only `read_dir` inherits the default
        // mutation no-ops (read-only backing).
        struct ReadOnly;
        impl Directory for ReadOnly {
            fn read_dir(&self, _path: &str) -> Option<Vec<DirEntry>> {
                Some(vec![])
            }
        }
        let d = ReadOnly;
        assert!(!d.create_dir("/x"), "read-only create_dir = false");
        assert!(!d.create_file("/x"), "read-only create_file = false");
        assert!(!d.remove("/x"), "read-only remove = false");
        assert!(!d.rename("/x", "/y"), "read-only rename = false");
    }
}
