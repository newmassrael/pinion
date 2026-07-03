//! R665 §3 §5.15 — platform filesystem bridge for the `pinion_core`
//! [`Storage`] trait. `FileStorage` routes the bytes-only key/value
//! surface to a per-application directory under the OS conventional
//! data root (`$XDG_DATA_HOME` / `~/.local/share` on Linux,
//! `~/Library/Application Support` on macOS, `%APPDATA%` on Windows
//! — resolved via the `dirs` crate's `data_dir()`).
//!
//! Designed as the second-consumer impl of [`Storage`] alongside the
//! pinion-core in-memory first impl: bindings construct a
//! `FileStorage` if the OS data dir is reachable, fall back to
//! `InMemoryStorage` on error so headless CI / sandboxed containers
//! still wire up (persistence becomes session-local, all other state
//! flows are unchanged).
//!
//! ## Atomic write contract
//!
//! Every [`Storage::save`] call writes to a sibling `*.tmp` file
//! first, then `rename(2)`s on top of the final key file. POSIX
//! `rename` is atomic within the same filesystem; Windows `MoveFileEx`
//! with `MOVEFILE_REPLACE_EXISTING` is the equivalent guarantee. A
//! crash mid-`save` therefore leaves either the prior payload intact
//! or the new payload completely written — never a torn half-write.
//! Required for the textbook "persistence survives process kill"
//! contract that the [R665 §5.16] todomvc binding relies on.
//!
//! ## Key sanitization
//!
//! The trait surface accepts any `&str` key, but the file impl maps
//! keys to filenames inside its root directory. To stay portable +
//! safe across OSes, [`FileStorage`] rejects every key that contains
//! a character outside `[A-Za-z0-9._-]`, plus the explicit `..` /
//! `.` parent / current-directory traversal markers, and keys
//! longer than 200 bytes. Invalid keys observe the trait's silent-
//! no-op contract (load returns `None`, save / remove become no-ops).
//!
//! Application code that sticks to the canonical `"package.subkey"`
//! shape (`"todomvc.state"`, `"settings.theme"`) passes through the
//! sanitizer untouched.

use std::cell::RefCell;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pinion_core::reactive::Owner;
use pinion_core::storage::InMemoryStorage;
use pinion_core::{DirEntry, Directory, Storage};

/// R665 §3 §5.15 — filesystem-backed [`Storage`] impl. Owns a root
/// directory under which each key maps to a single file. Operations
/// re-open the file per call (no held handle), so the same root may
/// safely back multiple `FileStorage` instances from different
/// threads / processes (POSIX file locking is OS-level; the crate
/// stays lock-free at the Rust layer to keep the API total).
///
/// The instance carries no in-memory state besides the root path, so
/// the `RefCell` on the path is purely to allow a future "switch
/// root" reconfiguration without `&mut self` plumbing (YAGNI as of
/// R665; the field is `RefCell<PathBuf>` to leave the door open
/// without a `SemVer` break, mirroring [[arboard-canonical-platform-clipboard]]
/// memory's lazy interior-mutability pattern).
pub struct FileStorage {
    root: RefCell<PathBuf>,
}

/// R665 §3 §5.15 — per-application data directory under the OS
/// conventional data root. The argument is the application's reverse-
/// DNS or short name segment (e.g. `"pinion-todomvc"`). On Linux this
/// resolves to `$XDG_DATA_HOME/<app>` (typically `~/.local/share/<app>`),
/// on macOS to `~/Library/Application Support/<app>`, on Windows to
/// `%APPDATA%/<app>`.
///
/// # Errors
///
/// Surfaces:
/// - `io::ErrorKind::NotFound` when the OS data directory can't be
///   resolved (headless containers without a real `$HOME`);
/// - any `create_dir_all` failure (permission denied, read-only
///   filesystem) — propagated unchanged so callers see a precise
///   `std::io::Error` to log before falling back to
///   [`pinion_core::InMemoryStorage`].
pub fn default_app_dir(app: &str) -> io::Result<PathBuf> {
    let base = dirs::data_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "platform data directory unavailable (`dirs::data_dir()` returned None)",
        )
    })?;
    let path = base.join(app);
    fs::create_dir_all(&path)?;
    Ok(path)
}

impl FileStorage {
    /// R665 §3 §5.15 — construct a `FileStorage` rooted at `root`.
    /// The directory is created with `create_dir_all` so the call
    /// is idempotent across launches; permission denial / read-only
    /// volumes surface as `Err` so the caller can decide to fall
    /// back to [`pinion_core::InMemoryStorage`].
    ///
    /// # Errors
    ///
    /// Surfaces the underlying `std::io::Error` from
    /// `fs::create_dir_all`. The `PathBuf` is taken by value so the
    /// caller can construct it freely (e.g. `default_app_dir(name)`
    /// + custom subdir) without cloning.
    pub fn try_new(root: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&root)?;
        Ok(Self {
            root: RefCell::new(root),
        })
    }

    /// Resolve `key` to its on-disk filename. Returns `None` when
    /// the key fails sanitization (length / charset / traversal
    /// markers). The trait surface treats `None` here as a silent
    /// no-op on `save` / `remove` and as `load -> None`.
    fn path_for(&self, key: &str) -> Option<PathBuf> {
        if !is_valid_key(key) {
            return None;
        }
        Some(self.root.borrow().join(key))
    }

    /// Sibling temp-path for atomic write. The temp file lands in
    /// the same directory as the final key file so the closing
    /// `rename` stays within one filesystem (cross-filesystem rename
    /// fails with `EXDEV` on Linux). We append `.tmp` after the key —
    /// since keys are themselves restricted to `[A-Za-z0-9._-]`, the
    /// resulting temp name is always unambiguous.
    fn temp_path_for(&self, key: &str) -> PathBuf {
        let mut name: OsString = key.into();
        name.push(".tmp");
        self.root.borrow().join(name)
    }
}

impl core::fmt::Debug for FileStorage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FileStorage")
            .field("root", &*self.root.borrow())
            .finish()
    }
}

impl Storage for FileStorage {
    /// R665 §3 §5.15 — `fs::read` the file matching `key`. `None` on
    /// invalid key, missing file, or any IO error (per the trait's
    /// total-surface contract).
    fn load(&self, key: &str) -> Option<Vec<u8>> {
        let path = self.path_for(key)?;
        fs::read(&path).ok()
    }

    /// R665 §3 §5.15 — atomic write via tempfile + rename. The
    /// sequence is:
    ///
    /// 1. open the sibling `*.tmp` file with `create + truncate`;
    /// 2. write `bytes` in one shot, then `flush` + `sync_all` so the
    ///    OS page cache is reconciled before the rename closes the
    ///    transaction;
    /// 3. `rename` the tempfile on top of the key file. POSIX rename
    ///    is atomic within one filesystem; a crash before step 3
    ///    leaves the prior payload intact, a crash after step 3
    ///    leaves the new payload intact — never a torn half-write.
    ///
    /// All errors are swallowed at the boundary (trait surface is
    /// total). The tempfile is left in place on failure so a future
    /// `save` call truncates + overwrites it cleanly; we do not run
    /// a cleanup pass because (a) the directory is private per-app
    /// and (b) a future `save` for the same key reuses the same
    /// tempfile name (idempotent).
    fn save(&self, key: &str, bytes: &[u8]) {
        let Some(path) = self.path_for(key) else {
            return;
        };
        let tmp = self.temp_path_for(key);

        // Best-effort atomic write. We swallow each step's error so
        // the trait stays total — the caller observes the failure on
        // the next `load` (which still returns the prior payload, or
        // `None` if there was never one).
        if let Ok(mut file) = fs::File::create(&tmp) {
            if file.write_all(bytes).is_ok() && file.flush().is_ok() && file.sync_all().is_ok() {
                let _ = fs::rename(&tmp, &path);
            }
        }
    }

    /// R665 §3 §5.15 — best-effort `fs::remove_file`. Idempotent: a
    /// missing file is a silent no-op (matches the trait contract +
    /// the W3C `localStorage.removeItem` semantics).
    fn remove(&self, key: &str) {
        if let Some(path) = self.path_for(key) {
            let _ = fs::remove_file(&path);
        }
    }
}

/// R665 §3 §5.15 — key validator. Accepts non-empty keys of length
/// 1..=200 whose every byte falls in `[A-Za-z0-9._-]` and which are
/// not the parent / current-directory traversal markers `..` / `.`.
///
/// The restricted alphabet is intentionally narrow so the on-disk
/// filename is a verbatim copy of the key (no escape encoding) and
/// the same key shape works on every OS (Windows reserves `< > : "
/// / \ | ? *` plus several device names; restricting to `[A-Za-z0-9._-]`
/// dodges every reserved character).
fn is_valid_key(key: &str) -> bool {
    if key.is_empty() || key.len() > 200 {
        return false;
    }
    if key == "." || key == ".." {
        return false;
    }
    key.bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// R665 §3 §5.15 — `data_dir`-rooted [`FileStorage`] constructor.
/// Convenience wrapper that combines [`default_app_dir`] +
/// [`FileStorage::try_new`] into the canonical "per-app persistence
/// directory" entry point applications consume. Mirrors how
/// `ArboardClipboard::try_new` wraps `arboard::Clipboard::new` for
/// the clipboard substrate.
///
/// # Errors
///
/// Propagates either the data-dir resolution failure or the
/// directory creation failure from [`default_app_dir`] /
/// [`FileStorage::try_new`].
pub fn open_app_storage(app: &str) -> io::Result<FileStorage> {
    let root = default_app_dir(app)?;
    FileStorage::try_new(root)
}

/// R665 §3 §5.15 — accessor for the active root directory.
/// Useful for debug logs / RPC introspection (the demo script
/// surfaces this through the `PINION_STORAGE_DIR` env override path).
#[must_use]
pub fn storage_root(storage: &FileStorage) -> PathBuf {
    storage.root.borrow().clone()
}

/// R665 §3 §5.15 — visible API on the concrete impl: list the on-
/// disk filenames currently present in the root. Test-only helper
/// (not on the trait — see the [`Storage`] doc comment for the
/// rationale). Mirrors `std::fs::read_dir` filtering out non-files
/// and the `*.tmp` sibling write-buffer files.
///
/// # Errors
///
/// Surfaces any `std::io::Error` from `read_dir` / per-entry
/// `file_type` (permission denied, IO failure). The trait's silent-
/// no-op contract does not apply here because tests want explicit
/// failure surfacing.
pub fn list_keys_for_test(storage: &FileStorage) -> io::Result<Vec<String>> {
    let mut keys = Vec::new();
    for entry in fs::read_dir(storage.root.borrow().as_path())? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if Path::new(name_str)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("tmp"))
        {
            continue;
        }
        keys.push(name_str.to_owned());
    }
    keys.sort();
    Ok(keys)
}

/// R665 §3 §5.15 — `Path` accessor for tests that need to assert on
/// the exact on-disk layout (tempfile cleanup, atomic-rename
/// semantics). The trait surface stays bytes-only; this helper lives
/// on the concrete impl.
#[must_use]
pub fn key_path(storage: &FileStorage, key: &str) -> Option<PathBuf> {
    storage.path_for(key)
}

/// R665 §3 §5.15 — `Path` accessor for the sibling tempfile used by
/// the atomic-write sequence. Tests assert the temp path stays
/// distinct from the final path and stays inside the root.
#[must_use]
pub fn temp_key_path(storage: &FileStorage, key: &str) -> Option<PathBuf> {
    // Mirror `key_path`'s `Option` return so callers reading both
    // sides treat invalid keys uniformly — the temp path is only
    // meaningful when the key itself sanitizes through.
    if !is_valid_key(key) {
        return None;
    }
    Some(storage.temp_path_for(key))
}

/// R665 §3 §5.15 — predicate counterpart of `is_valid_key` for
/// callers that want to validate ahead of a `save` (e.g. to surface a
/// "rename the key" message in a settings panel). Mirrors the
/// trait's silent-no-op contract but exposes the decision.
#[must_use]
pub fn key_is_valid(key: &str) -> bool {
    is_valid_key(key)
}

/// R665 §3 §5.15 — predicate that the `path` resolves *inside* the
/// storage root. Defence-in-depth for callers integrating
/// [`FileStorage`] alongside other on-disk reads (e.g. config files
/// in a sibling directory).
#[must_use]
pub fn path_inside_root(storage: &FileStorage, path: &Path) -> bool {
    let root = storage.root.borrow();
    path.starts_with(root.as_path())
}

/// R787 §3 §5.15 — real-filesystem [`Directory`] impl: the read side of
/// the filesystem an own-rendered file browser walks, the second-consumer
/// platform bridge alongside [`FileStorage`]. Stateless (re-reads per
/// call, no held handle), so a single instance backs any number of
/// browsers. Maps the pure `pinion_core` [`Directory`] trait onto
/// `std::fs::read_dir`, keeping that `std::fs` dependency out of the pure
/// core (the same split [`FileStorage`] keeps for `Storage`).
///
/// IO failures (missing path, not-a-directory, permission denied) surface
/// as `None` per the trait's total contract — the browser renders an
/// empty / unreachable folder rather than an error type.
#[derive(Debug, Default, Clone, Copy)]
pub struct FsDirectory;

impl FsDirectory {
    /// A fresh stateless `FsDirectory`.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Directory for FsDirectory {
    fn read_dir(&self, path: &str) -> Option<Vec<DirEntry>> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(path).ok()? {
            let Ok(entry) = entry else { continue };
            let name = entry.file_name().to_string_lossy().into_owned();
            // `file_type()` avoids a follow-symlink `metadata()` stat; a
            // symlink whose type cannot be read defaults to a (selectable)
            // file rather than a navigable dir — the conservative choice.
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            entries.push(DirEntry { name, is_dir });
        }
        // Canonical order is the pure-core SSOT so the seeded in-memory
        // fixture and this real listing cannot disagree.
        pinion_core::directory::sort_entries(&mut entries);
        Some(entries)
    }

    /// R789 — `std::fs::create_dir` at `path` (non-recursive: the parent
    /// must exist, matching the in-memory backing). Fails (`false`) if the
    /// parent is missing or `path` already exists.
    fn create_dir(&self, path: &str) -> bool {
        fs::create_dir(path).is_ok()
    }

    /// R789 — create a new empty file at `path`. `create_new` fails
    /// (`false`) when the file already exists, mirroring the in-memory
    /// backing's duplicate rejection.
    fn create_file(&self, path: &str) -> bool {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .is_ok()
    }

    /// R789 — remove the entry at `path`: a directory (and its subtree)
    /// via `remove_dir_all`, a file via `remove_file`. `symlink_metadata`
    /// does not follow a symlink (a symlinked dir is unlinked, not its
    /// target's contents). `false` when nothing is there.
    fn remove(&self, path: &str) -> bool {
        match fs::symlink_metadata(path) {
            Ok(meta) if meta.is_dir() => fs::remove_dir_all(path).is_ok(),
            Ok(_) => fs::remove_file(path).is_ok(),
            Err(_) => false,
        }
    }

    /// R791 — rename `from` to `to` via `std::fs::rename` (a single atomic
    /// call carries a directory's whole subtree). Guarded to **not
    /// overwrite**: a pre-existing `to` makes this a `false` no-op,
    /// matching the in-memory backing's duplicate rejection (POSIX
    /// `rename(2)` would silently clobber an existing file otherwise).
    fn rename(&self, from: &str, to: &str) -> bool {
        if fs::symlink_metadata(to).is_ok() {
            return false; // destination exists — never overwrite
        }
        fs::rename(from, to).is_ok()
    }
}

// ─── application storage hook (R857 SSOT lift) ─────────────────────

/// R857 §3 §5.15 — env override pointing at a custom storage root. Set by the
/// RPC-verify demos (a per-test tempdir, via `rpc_verify.isolated_storage_dir`)
/// so example launches never contaminate the developer's real per-OS data dir.
pub const STORAGE_DIR_ENV: &str = "PINION_STORAGE_DIR";

/// R857 §3 §5.15 — sized newtype around `Box<dyn Storage>` so an `Owner::cache`
/// slot can park either a [`FileStorage`] (the common case) or an
/// [`InMemoryStorage`] (the headless / sandboxed fallback) under one concrete
/// type, forwarding the whole [`Storage`] surface. The boxed-dyn wrapper peer of
/// `pinion_platform_clipboard::AppClipboard` ([[owner-cache-typed-key]]).
pub struct AppStorage(Box<dyn Storage>);

impl AppStorage {
    /// Wrap an explicit backend. Production uses [`build_app_storage`]; a test
    /// injects an [`InMemoryStorage`] to stay off the filesystem.
    #[must_use]
    pub fn new(inner: Box<dyn Storage>) -> Self {
        Self(inner)
    }
}

impl Storage for AppStorage {
    fn load(&self, key: &str) -> Option<Vec<u8>> {
        self.0.load(key)
    }
    fn save(&self, key: &str, bytes: &[u8]) {
        self.0.save(key, bytes);
    }
    fn remove(&self, key: &str) {
        self.0.remove(key);
    }
}

/// R857 §3 §5.15 — build the platform storage backend for `app` once. Priority:
/// the [`STORAGE_DIR_ENV`] override (the demos' tempdir) → the per-OS data dir
/// ([`open_app_storage`]) → an [`InMemoryStorage`] fallback (headless / sandboxed;
/// persistence becomes session-local, every other path unchanged). The runtime
/// selection is what makes a binding genuinely reach the file backend rather
/// than a hardcoded in-memory stub (the R834 "no hollow feature" lesson).
#[must_use]
pub fn build_app_storage(app: &str) -> Box<dyn Storage> {
    if let Ok(custom_dir) = std::env::var(STORAGE_DIR_ENV) {
        match FileStorage::try_new(PathBuf::from(&custom_dir)) {
            Ok(file) => return Box::new(file) as Box<dyn Storage>,
            Err(e) => eprintln!(
                "{app}: FileStorage at {custom_dir:?} via {STORAGE_DIR_ENV} failed ({e}); \
                 falling back to the default data dir",
            ),
        }
    }
    match open_app_storage(app) {
        Ok(file) => Box::new(file) as Box<dyn Storage>,
        Err(e) => {
            eprintln!(
                "{app}: open_app_storage({app:?}) failed ({e}); persistence will be \
                 in-memory only (lost on exit)",
            );
            Box::new(InMemoryStorage::new()) as Box<dyn Storage>
        }
    }
}

/// R857 §3 §5.15 — the `Owner::cache`-keyed storage hook: one [`AppStorage`] per
/// process, shared by every consumer resolving `cache_key`. The SSOT lift of the
/// byte-near-identical `AppStorage` + `build_*_storage` + `use_*storage` cluster
/// that todomvc, settings-panel, and the node editor each hand-rolled — the
/// `pinion_platform_clipboard::use_app_clipboard` (R790) precedent applied to
/// storage now that the pattern is a verified 3-copy.
///
/// # Panics
/// Panics outside an active [`Owner`] scope (call from a view / reducer /
/// `create_external` hook — all run inside `root_owner.run`).
#[must_use]
pub fn use_app_storage(cache_key: &'static str, app: &'static str) -> Rc<AppStorage> {
    Owner::current()
        .expect("use_app_storage requires an active Owner scope")
        .cache(cache_key, || AppStorage::new(build_app_storage(app)))
}

#[cfg(test)]
mod tests {
    //! R665 §3 §5.15 — `FileStorage` regression battery covering
    //! atomic write, key sanitization, round-trip across instances
    //! (same root reopened), tempfile cleanup, and the trait's
    //! silent-no-op contract on invalid keys.

    use super::{
        AppStorage, FileStorage, InMemoryStorage, Storage, default_app_dir, is_valid_key, key_path,
        list_keys_for_test, open_app_storage, storage_root, temp_key_path,
    };
    use std::fs;
    use std::path::PathBuf;

    /// Thin tempdir helper — `tempfile` is not in the workspace dep
    /// graph (would be a transitive add just for one test crate), so
    /// the tests roll their own under the OS temp root.
    struct TmpDir(PathBuf);

    impl TmpDir {
        fn new(prefix: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let path =
                std::env::temp_dir().join(format!("pinion-platform-storage-{prefix}-{nanos}"));
            fs::create_dir_all(&path).expect("create test tempdir");
            Self(path)
        }

        fn path(&self) -> &PathBuf {
            &self.0
        }
    }

    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn r857_app_storage_forwards_to_its_backend() {
        // The Owner::cache wrapper forwards the whole Storage surface to its
        // boxed backend (the file-/in-memory-selection runtime path is exercised
        // cross-process by the example demos via PINION_STORAGE_DIR).
        let app = AppStorage::new(Box::new(InMemoryStorage::new()));
        assert_eq!(app.load("k"), None, "empty backend loads None");
        app.save("k", b"v");
        assert_eq!(
            app.load("k"),
            Some(b"v".to_vec()),
            "save then load forwards"
        );
        app.remove("k");
        assert_eq!(app.load("k"), None, "remove forwards");
    }

    #[test]
    fn r665_file_storage_save_then_load_round_trips() {
        let tmp = TmpDir::new("round-trip");
        let storage = FileStorage::try_new(tmp.path().clone()).expect("open root");
        storage.save("todomvc.state", b"hello world");
        assert_eq!(storage.load("todomvc.state"), Some(b"hello world".to_vec()));
    }

    #[test]
    fn r665_file_storage_load_missing_returns_none() {
        let tmp = TmpDir::new("missing");
        let storage = FileStorage::try_new(tmp.path().clone()).expect("open root");
        assert_eq!(storage.load("never.written"), None);
    }

    #[test]
    fn r665_file_storage_repeated_save_overwrites() {
        let tmp = TmpDir::new("overwrite");
        let storage = FileStorage::try_new(tmp.path().clone()).expect("open root");
        storage.save("k", b"first");
        storage.save("k", b"second");
        assert_eq!(storage.load("k"), Some(b"second".to_vec()));
    }

    #[test]
    fn r665_file_storage_remove_clears_key() {
        let tmp = TmpDir::new("remove");
        let storage = FileStorage::try_new(tmp.path().clone()).expect("open root");
        storage.save("k", b"value");
        storage.remove("k");
        assert_eq!(storage.load("k"), None);
    }

    #[test]
    fn r665_file_storage_reopen_root_preserves_payload() {
        // The canonical persistence guarantee: a process exit
        // (here modelled as a dropped FileStorage) must not lose
        // previously-saved bytes when the next process opens the
        // same root.
        let tmp = TmpDir::new("reopen");
        {
            let storage = FileStorage::try_new(tmp.path().clone()).expect("open root");
            storage.save("k", b"persistent");
        }
        let storage2 = FileStorage::try_new(tmp.path().clone()).expect("reopen root");
        assert_eq!(storage2.load("k"), Some(b"persistent".to_vec()));
    }

    #[test]
    fn r665_file_storage_atomic_write_no_tempfile_left_behind() {
        let tmp = TmpDir::new("atomic");
        let storage = FileStorage::try_new(tmp.path().clone()).expect("open root");
        storage.save("k", b"payload");
        let entries: Vec<_> = fs::read_dir(tmp.path())
            .expect("read tempdir")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        // Only the final key file should remain — the .tmp sibling
        // must have been renamed away.
        assert!(entries.iter().any(|n| n == "k"));
        assert!(
            !entries.iter().any(|n| n == "k.tmp"),
            "tempfile leaked: {entries:?}",
        );
    }

    #[test]
    fn r665_file_storage_list_keys_excludes_tempfiles() {
        let tmp = TmpDir::new("list");
        let storage = FileStorage::try_new(tmp.path().clone()).expect("open root");
        storage.save("a", b"alpha");
        storage.save("b", b"beta");
        // Plant a stray .tmp by hand (simulates a crashed previous
        // save). list_keys_for_test should ignore it.
        fs::write(tmp.path().join("c.tmp"), b"orphan").expect("write tmp");
        let keys = list_keys_for_test(&storage).expect("list keys");
        assert_eq!(keys, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn r665_invalid_keys_are_rejected_silently() {
        let tmp = TmpDir::new("invalid");
        let storage = FileStorage::try_new(tmp.path().clone()).expect("open root");

        // Traversal markers
        storage.save("..", b"escape");
        assert_eq!(storage.load(".."), None);
        storage.save(".", b"escape");
        assert_eq!(storage.load("."), None);

        // Path separators
        storage.save("a/b", b"escape");
        assert_eq!(storage.load("a/b"), None);
        storage.save("a\\b", b"escape");
        assert_eq!(storage.load("a\\b"), None);

        // Null byte
        storage.save("a\0b", b"escape");
        assert_eq!(storage.load("a\0b"), None);

        // Empty key
        storage.save("", b"escape");
        assert_eq!(storage.load(""), None);

        // Non-ASCII (Korean) — rejected to keep filename mapping
        // portable across OSes.
        storage.save("한글", b"escape");
        assert_eq!(storage.load("한글"), None);

        // Whitespace
        storage.save("with space", b"escape");
        assert_eq!(storage.load("with space"), None);

        // The root must be empty — no file ever made it to disk.
        let keys = list_keys_for_test(&storage).expect("list keys");
        assert!(keys.is_empty(), "leaked invalid-key files: {keys:?}");
    }

    #[test]
    fn r665_key_validator_accepts_canonical_shape() {
        assert!(is_valid_key("todomvc.state"));
        assert!(is_valid_key("settings.theme"));
        assert!(is_valid_key("a"));
        assert!(is_valid_key("ABC_123.-foo"));
    }

    #[test]
    fn r665_key_validator_rejects_oversize() {
        // 200 bytes accepted, 201 rejected (boundary).
        let ok = "a".repeat(200);
        assert!(is_valid_key(&ok));
        let bad = "a".repeat(201);
        assert!(!is_valid_key(&bad));
    }

    #[test]
    fn r665_storage_root_exposes_path() {
        let tmp = TmpDir::new("root");
        let storage = FileStorage::try_new(tmp.path().clone()).expect("open root");
        assert_eq!(&storage_root(&storage), tmp.path());
    }

    #[test]
    fn r665_key_path_helper_resolves_inside_root() {
        let tmp = TmpDir::new("key-path");
        let storage = FileStorage::try_new(tmp.path().clone()).expect("open root");
        let path = key_path(&storage, "todomvc.state").expect("valid key");
        assert_eq!(path.parent(), Some(tmp.path().as_path()));
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("todomvc.state")
        );

        let tmp_path = temp_key_path(&storage, "todomvc.state").expect("valid key");
        assert_eq!(
            tmp_path.file_name().and_then(|n| n.to_str()),
            Some("todomvc.state.tmp"),
        );
    }

    #[test]
    fn r665_dyn_storage_polymorphism_works_for_file_storage() {
        let tmp = TmpDir::new("dyn-file");
        let storage: std::rc::Rc<dyn Storage> =
            std::rc::Rc::new(FileStorage::try_new(tmp.path().clone()).expect("open root"));
        storage.save("via.dyn", b"file-payload");
        assert_eq!(storage.load("via.dyn"), Some(b"file-payload".to_vec()));
    }

    #[test]
    fn r665_default_app_dir_creates_subdir_under_data_root() {
        // The dirs::data_dir() return may differ per test env, but
        // the subdir creation contract is universal — call it and
        // check the path exists. We use a uniqued suffix so parallel
        // test runs do not collide.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let app = format!("pinion-platform-storage-test-{nanos}");
        let path = default_app_dir(&app).expect("create app dir");
        assert!(path.exists(), "app dir not created: {path:?}");
        // Clean up — best-effort, ignore errors.
        let _ = std::fs::remove_dir(&path);
    }

    #[test]
    fn r665_open_app_storage_constructs_usable_handle() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let app = format!("pinion-platform-storage-open-{nanos}");
        let storage = open_app_storage(&app).expect("open app storage");
        storage.save("k", b"app-storage-payload");
        assert_eq!(storage.load("k"), Some(b"app-storage-payload".to_vec()));
        // Clean up.
        storage.remove("k");
        let _ = std::fs::remove_dir(default_app_dir(&app).expect("recover path"));
    }

    #[test]
    fn r787_fs_directory_lists_real_entries_sorted() {
        use super::FsDirectory;
        use pinion_core::Directory;

        let tmp = TmpDir::new("fsdir");
        // Seed a real on-disk tree: a subdir + two files (out of alpha order).
        fs::create_dir(tmp.path().join("zsub")).expect("mkdir");
        fs::write(tmp.path().join("b.txt"), b"b").expect("write");
        fs::write(tmp.path().join("a.txt"), b"a").expect("write");

        let fsd = FsDirectory::new();
        let listing = fsd
            .read_dir(tmp.path().to_str().expect("utf8 tmp path"))
            .expect("real dir lists");
        let names: Vec<&str> = listing.iter().map(|e| e.name.as_str()).collect();
        // Dirs first (zsub), then files alpha (a.txt, b.txt) — canonical order.
        assert_eq!(names, ["zsub", "a.txt", "b.txt"]);
        assert!(listing[0].is_dir, "zsub is a directory");
        assert!(!listing[1].is_dir, "a.txt is a file");

        // A missing path lists None (the total-surface contract).
        assert_eq!(
            fsd.read_dir(&format!("{}/nope", tmp.path().display())),
            None
        );
        // TmpDir's Drop removes the seeded tree.
    }

    #[test]
    fn r789_fs_directory_create_and_remove_round_trip() {
        use super::FsDirectory;
        use pinion_core::Directory;

        let tmp = TmpDir::new("fsdir-write");
        let root = tmp.path().to_str().expect("utf8 tmp path").to_string();
        let fsd = FsDirectory::new();

        // create_dir + create_file appear in the real listing.
        assert!(
            fsd.create_dir(&format!("{root}/src")),
            "create_dir succeeds"
        );
        assert!(
            fsd.create_file(&format!("{root}/README.md")),
            "create_file succeeds"
        );
        let names: Vec<String> = fsd
            .read_dir(&root)
            .expect("lists")
            .iter()
            .map(|e| e.name.clone())
            .collect();
        assert_eq!(
            names,
            ["src", "README.md"],
            "created entries appear (dir first)"
        );

        // create_new semantics: a duplicate file is rejected.
        assert!(
            !fsd.create_file(&format!("{root}/README.md")),
            "duplicate file rejected"
        );
        // create_dir under a missing parent is rejected (non-recursive).
        assert!(
            !fsd.create_dir(&format!("{root}/nope/deep")),
            "missing parent rejected"
        );

        // remove drops a file and (recursively) a directory.
        assert!(
            fsd.remove(&format!("{root}/README.md")),
            "remove file succeeds"
        );
        fs::write(format!("{root}/src/main.rs"), b"fn main() {{}}").expect("seed subfile");
        assert!(
            fsd.remove(&format!("{root}/src")),
            "remove dir (recursive) succeeds"
        );
        assert_eq!(fsd.read_dir(&root).expect("lists").len(), 0, "both removed");
        assert!(
            !fsd.remove(&format!("{root}/ghost")),
            "removing a missing path = false"
        );
        // TmpDir's Drop removes the root.
    }

    #[test]
    fn r791_fs_directory_rename_round_trip() {
        use super::FsDirectory;
        use pinion_core::Directory;

        let tmp = TmpDir::new("fsdir-rename");
        let root = tmp.path().to_str().expect("utf8 tmp path").to_string();
        let fsd = FsDirectory::new();
        fs::write(format!("{root}/old.txt"), b"content").expect("seed file");
        fs::create_dir(format!("{root}/keep")).expect("seed dir");

        // Rename the file: old gone, new present, content preserved.
        assert!(
            fsd.rename(&format!("{root}/old.txt"), &format!("{root}/new.txt")),
            "rename ok"
        );
        let names: Vec<String> = fsd
            .read_dir(&root)
            .expect("lists")
            .iter()
            .map(|e| e.name.clone())
            .collect();
        assert_eq!(
            names,
            ["keep", "new.txt"],
            "renamed entry present, old gone"
        );
        assert_eq!(
            fs::read(format!("{root}/new.txt")).expect("read"),
            b"content",
            "content kept"
        );

        // Never overwrite: renaming onto an existing name is a false no-op.
        fs::write(format!("{root}/taken.txt"), b"x").expect("seed");
        assert!(
            !fsd.rename(&format!("{root}/new.txt"), &format!("{root}/taken.txt")),
            "no overwrite"
        );
        assert!(
            fsd.read_dir(&root)
                .expect("lists")
                .iter()
                .any(|e| e.name == "new.txt"),
            "source kept"
        );
        // A missing source is a false no-op.
        assert!(
            !fsd.rename(&format!("{root}/ghost"), &format!("{root}/z.txt")),
            "missing source"
        );
        // TmpDir's Drop removes the root.
    }
}
