//! R665 §3 §5.15 — Storage substrate for the External(opaque) escape
//! hatch. Pure-Rust trait + in-memory implementation; platform
//! integration (XDG `$XDG_DATA_HOME` / `~/.local/share` on Linux,
//! `~/Library/Application Support` on macOS, `%APPDATA%` on Windows,
//! `localStorage` on web targets) lives in the `pinion-platform-storage`
//! crate.
//!
//! The trait is intentionally minimal — bytes-only (`load` returns
//! `Option<Vec<u8>>`, `save` takes `&[u8]`). Application code chooses
//! its own encoding (JSON, `MessagePack`, bincode, raw text). This
//! parallels how W3C `localStorage` exposes a bytes-as-string surface
//! and applications stringify their own object models on top.
//!
//! ## Why bytes-only (not generic over a serde type)
//!
//! Mirrors [`crate::Clipboard`]: the platform substrate is a
//! capability boundary (§3 External(opaque)) where the framework
//! exposes only the OS primitive (a key-value byte store) and lets
//! the application choose its own dehydrate/rehydrate pipeline. A
//! `Storage<T: Serialize>` generic would bake serde into the trait
//! object, breaking `Rc<dyn Storage>` dispatch — every consumer crate
//! would have to know the concrete `T`.
//!
//! ## Why total (no `Result`)
//!
//! Storage failures (disk full, permission denied, IO error, missing
//! file) are surfaced as `None` on `load` and silent no-op on `save`
//! / `remove`. The trait surface stays total so application code can
//! treat persistence as a best-effort capability (the textbook
//! `localStorage` shape — every browser implements it this way). A
//! [`Storage`] caller that genuinely needs to distinguish "missing
//! key" from "IO error" instantiates the concrete impl directly and
//! consults its underlying surface (the file impl exposes the actual
//! `std::io::Error` through its `Debug` repr).
//!
//! ## Substrate placement
//!
//! Lives in `pinion-core` (not `pinion-core::widgets`) because the
//! trait + in-memory impl are useful outside any single widget — the
//! whole application's persistence-rehydrate boot pass plugs in
//! through this abstraction. Same placement rationale as
//! [`crate::Clipboard`] (R56.1.e §5.22): substrate-level primitives
//! that more than one consumer touches belong above the widgets
//! module.

use std::cell::RefCell;
use std::collections::BTreeMap;

/// R665 §3 §5.15 — bytes-only persistence surface. Mirror of the W3C
/// `localStorage` API ("getItem" / "setItem" / "removeItem") except
/// the value type is `Vec<u8>` instead of `String` so application
/// code can persist non-text payloads (binary formats, compressed
/// JSON, `MessagePack`, bincode) without an intermediate base64 hop.
///
/// Every method takes `&self` (not `&mut self`) so a `Rc<dyn Storage>`
/// handle can be shared across hooks / Effects / `External` bridges
/// through immutable composition. Implementations pick their own
/// interior-mutability model (the in-memory impl uses
/// `RefCell<BTreeMap<...>>`; the file impl below is stateless and
/// re-opens the underlying file per call).
///
/// ## Key semantics
///
/// Keys are opaque `&str` slices the implementation may sanitize
/// (the file impl rejects path separators / `..` / `\0` / non-
/// ASCII to keep filesystem mapping deterministic). Callers should
/// stick to `[a-zA-Z0-9._\-]+` of length 1..=200 (the canonical
/// "package.subkey" / "todomvc.state" shape) so every implementation
/// accepts the key verbatim. Invalid keys observe a silent no-op on
/// `save` / `remove` and `None` on `load`, same total-surface shape
/// as the rest of the trait.
///
/// ## Why no `keys()` enumeration
///
/// YAGNI. Application code knows its own key list (constants
/// declared at the binding level). A future `keys()` axis can land
/// as a sub-trait the same way `Clipboard::copy_to` extends the
/// W3C surface — backward-compatible because every existing impl
/// already covers the bytes-only base.
pub trait Storage {
    /// Read the bytes for `key`. Returns `None` when:
    ///
    /// - the key was never written (fresh-default state — the
    ///   canonical "first launch" path);
    /// - the key was previously removed via [`remove`](Self::remove);
    /// - the underlying storage errored (disk read failure, permission
    ///   denied, malformed sanitization). The implementation logs /
    ///   debugs the underlying error through its own `Debug` repr;
    ///   the trait surface stays total.
    fn load(&self, key: &str) -> Option<Vec<u8>>;

    /// Write `bytes` to `key`, replacing any prior payload. Total
    /// surface — IO failures (disk full, permission denied) are
    /// silently swallowed at the boundary. Callers that need
    /// fail-loud semantics instantiate the concrete impl directly
    /// and consult its underlying `try_*` surface.
    ///
    /// Implementations may choose to write atomically (tempfile +
    /// rename on the file impl, single-pointer swap on the in-memory
    /// impl) so partial writes never surface to a subsequent `load`.
    fn save(&self, key: &str, bytes: &[u8]);

    /// Remove the payload for `key`. Subsequent `load(key)` returns
    /// `None`. Idempotent — removing a missing key is a silent no-op.
    fn remove(&self, key: &str);
}

/// R665 §3 §5.15 — pure-Rust in-memory `Storage` impl. Stores
/// payloads in a `RefCell<BTreeMap<String, Vec<u8>>>` keyed by the
/// caller's `&str` key. Used as the canonical test fixture and as
/// the headless / CI fallback when a concrete platform impl
/// (`pinion-platform-storage::FileStorage`) fails to initialize.
///
/// `BTreeMap` (not `HashMap`) so iteration order — should a future
/// `keys()` axis land — is deterministic. The application surface
/// today exercises only point reads / writes / removes; the choice
/// is forward-compatible.
///
/// Single-thread only (no `Send` / `Sync`): the canonical pinion
/// view-fn / hook invocation is UI-thread synchronous (§6.3). Multi-
/// thread implementations (a future browser `localStorage` adapter
/// that bridges through `Worker.postMessage`) wrap their payload in
/// `Mutex<_>` / `Arc<_>`.
#[derive(Debug, Default)]
pub struct InMemoryStorage {
    entries: RefCell<BTreeMap<String, Vec<u8>>>,
}

impl InMemoryStorage {
    /// Construct a fresh `InMemoryStorage` with no payloads.
    /// `load(_)` returns `None` for every key until the first
    /// `save(...)` lands.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot the number of stored keys. Test-only helper — the
    /// trait surface does not expose enumeration. Used in unit tests
    /// to verify `save` / `remove` symmetry without iterating.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.borrow().len()
    }

    /// `true` when no payloads have been written. Test-only helper.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.borrow().is_empty()
    }
}

impl Storage for InMemoryStorage {
    fn load(&self, key: &str) -> Option<Vec<u8>> {
        self.entries.borrow().get(key).cloned()
    }

    fn save(&self, key: &str, bytes: &[u8]) {
        self.entries
            .borrow_mut()
            .insert(key.to_owned(), bytes.to_vec());
    }

    fn remove(&self, key: &str) {
        self.entries.borrow_mut().remove(key);
    }
}

#[cfg(test)]
mod tests {
    //! R665 §3 §5.15 — `Storage` + `InMemoryStorage` regression
    //! battery. Covers the load/save/remove surface, fresh-default
    //! shape, repeated overwrite, empty-bytes round-trip, multi-key
    //! isolation, and `Rc<dyn Storage>` polymorphism.

    use super::{InMemoryStorage, Storage};

    #[test]
    fn r665_fresh_storage_load_returns_none() {
        let s = InMemoryStorage::new();
        assert_eq!(s.load("any.key"), None);
        assert!(s.is_empty());
    }

    #[test]
    fn r665_save_then_load_round_trips() {
        let s = InMemoryStorage::new();
        s.save("todomvc.state", b"hello");
        assert_eq!(s.load("todomvc.state"), Some(b"hello".to_vec()));
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn r665_repeated_save_overwrites_prior_payload() {
        let s = InMemoryStorage::new();
        s.save("k", b"first");
        s.save("k", b"second");
        assert_eq!(s.load("k"), Some(b"second".to_vec()));
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn r665_empty_bytes_round_trip_as_some() {
        // Per the W3C surface, saving empty bytes is a write
        // (storage now holds the empty payload), distinct from
        // never-written (load -> None).
        let s = InMemoryStorage::new();
        s.save("empty", b"");
        assert_eq!(s.load("empty"), Some(Vec::new()));
    }

    #[test]
    fn r665_remove_clears_key() {
        let s = InMemoryStorage::new();
        s.save("k", b"value");
        s.remove("k");
        assert_eq!(s.load("k"), None);
        assert!(s.is_empty());
    }

    #[test]
    fn r665_remove_missing_key_is_no_op() {
        let s = InMemoryStorage::new();
        s.remove("never.written");
        assert!(s.is_empty());
    }

    #[test]
    fn r665_multi_key_isolation() {
        let s = InMemoryStorage::new();
        s.save("a", b"alpha");
        s.save("b", b"beta");
        assert_eq!(s.load("a"), Some(b"alpha".to_vec()));
        assert_eq!(s.load("b"), Some(b"beta".to_vec()));
        s.remove("a");
        assert_eq!(s.load("a"), None);
        assert_eq!(s.load("b"), Some(b"beta".to_vec()));
    }

    #[test]
    fn r665_dyn_storage_polymorphism_works() {
        let s: std::rc::Rc<dyn Storage> = std::rc::Rc::new(InMemoryStorage::new());
        s.save("via.dyn", b"payload");
        assert_eq!(s.load("via.dyn"), Some(b"payload".to_vec()));
    }

    #[test]
    fn r665_binary_payload_round_trip() {
        // Storage is bytes-only, so non-UTF-8 binary payloads round-
        // trip without alteration (mirrors localStorage semantics
        // for ArrayBuffer-encoded values, except no base64 hop).
        let s = InMemoryStorage::new();
        let bytes: Vec<u8> = (0u8..=255).collect();
        s.save("binary", &bytes);
        assert_eq!(s.load("binary"), Some(bytes));
    }
}
