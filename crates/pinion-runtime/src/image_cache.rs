//! R740 §5.16 — vello-side decoded-image cache.
//!
//! The GPU-adjacent half of the first asset-pipeline slice. The
//! backend-agnostic decode (encoded bytes → RGBA8) lives in
//! [`pinion_asset`]; this module owns the *vello* concern: resolving an
//! [`ImageNode::source`](pinion_core::scene::ImageNode) string to a
//! ready-to-draw `peniko::ImageData`, decoded **once** per source and
//! cached for every subsequent frame.
//!
//! The cache is owned by the shell (alongside the text
//! [`LayoutCache`](pinion_text::LayoutCache)) and threaded into
//! [`to_vello`](crate::paint_adapter::to_vello) so the pure paint walker
//! never performs IO on a cache hit — exactly the "codec / decoded-buffer
//! cache is carry-forward and resolved by the consumer rasterizer"
//! contract the [`ImageNode`](pinion_core::scene::ImageNode) doc states.
//!
//! ## Source model
//!
//! A **filesystem** `source` (the documented `file://` locator form, minus
//! the scheme for now) is read + decoded on the first frame that paints it
//! and the result — including a *negative* result (missing file /
//! undecodable) — is cached, so a broken source costs one failed read, not
//! one per frame.
//!
//! A `memory://<key>` `source` (R1404) resolves instead through a
//! producer-registered [`MemoryImageStore`] — a runtime-decoded RGBA image
//! a producer registered under `<key>`, with no filesystem round-trip. The
//! shell seeds one store at root ([`IMAGE_STORE`]) and hands it to every
//! window's cache; the producer registers through the same handle. Unlike a
//! filesystem source, a `memory://` source is deliberately NOT cached in the
//! decode-once map: the store is mutable (a terminal image updates / is
//! deleted, per the sprag Kitty-graphics / sixel consumer), so the current
//! image is read fresh each frame. `https://` remains an additive axis for a
//! later round.
//!
//! The decoded `peniko::ImageData` wraps the `Arc`-shared RGBA8 buffer in
//! a `peniko::Blob`, so each frame's lookup clones only the `Arc` (and the
//! handful of scalar fields), never the pixel data.

use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use lru::LruCache;

use pinion_asset::DecodedImage;
use pinion_core::{Owner, ProviderSlot};
use vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

/// The producer-supplied in-memory image scheme (R1404). A `Scene::Image`
/// [`source`](pinion_core::scene::ImageNode::source) of the form
/// `memory://<key>` resolves to the decoded image a producer registered
/// under `<key>` in the process [`MemoryImageStore`], with no filesystem
/// round-trip — the additive scheme this module has forecast since R740.
pub const MEMORY_SCHEME: &str = "memory://";

/// R1404 §5.16 — a producer-registered, mutable, in-memory image store.
///
/// A producer decodes an image at runtime (a terminal's Kitty-graphics /
/// sixel raster, an app-generated bitmap) and registers the RGBA buffer
/// under a string key; a `Scene::Image { source: "memory://<key>" }` node
/// then paints it with no filesystem read. This store is the
/// **binding-registered** half the
/// [`ImageNode`](pinion_core::scene::ImageNode) doc forecast; the
/// `Scene::Image` scheme half is [`MEMORY_SCHEME`].
///
/// # Why an `Arc` handle, not a field owned by the cache
///
/// The painter rebuilds its [`ImageCache`] freely — fresh per call on the
/// headless path, per-window on the live path — so the store cannot LIVE in
/// the cache or a registration would not survive the next frame's cache. It
/// is an `Arc`-shared handle the shell seeds once at root ([`IMAGE_STORE`])
/// and hands to both sides: the producer registers through it (on any thread
/// — it is `Send + Sync`, the sprag poll-thread requirement) and every
/// [`ImageCache`] the painter builds resolves through the SAME handle. A
/// re-registered or removed key is therefore visible on the next paint,
/// which is what a mutable terminal image (Kitty animation / retransmit /
/// delete) needs.
///
/// The decode-once discipline is preserved: [`insert`](Self::insert)
/// converts the [`DecodedImage`] to a `peniko::ImageData` ONCE (wrapping the
/// `Arc`-shared RGBA8 buffer in a `peniko::Blob`, no pixel copy), and
/// [`ImageCache::resolve`] then clones only that `Arc`-backed handle each
/// frame.
#[derive(Clone, Debug, Default)]
pub struct MemoryImageStore {
    inner: Arc<RwLock<HashMap<String, ImageData>>>,
}

impl MemoryImageStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) the decoded RGBA `image` under `key`. Converts
    /// to the drawable `peniko::ImageData` once here; a subsequent paint of a
    /// `memory://<key>` node draws it. Replacing an existing key is how a
    /// mutable terminal image updates (Kitty animation / retransmit) — the
    /// next paint shows the new pixels.
    pub fn insert(&self, key: impl Into<String>, image: &DecodedImage) {
        self.write().insert(key.into(), to_image_data(image));
    }

    /// Remove `key`, returning whether it was present. A subsequent paint of
    /// a `memory://<key>` node then draws nothing (the graceful
    /// missing-source skip) — the Kitty-graphics delete / sixel clear.
    ///
    /// Not `#[must_use]`: the bool is informational, and a fire-and-forget
    /// delete (`store.remove(key);`) is a valid use — the `HashMap::remove`
    /// shape, which is likewise not must-use.
    #[allow(
        clippy::must_use_candidate,
        reason = "informational bool; fire-and-forget delete is valid"
    )]
    pub fn remove(&self, key: &str) -> bool {
        self.write().remove(key).is_some()
    }

    /// Whether `key` is currently registered.
    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.read().contains_key(key)
    }

    /// The number of registered keys.
    #[must_use]
    pub fn len(&self) -> usize {
        self.read().len()
    }

    /// Whether no key is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.read().is_empty()
    }

    /// The current drawable image for `key`, or `None` when nothing is
    /// registered there. A cheap clone (the pixel buffer is `Arc`-shared via
    /// `peniko::Blob`). Read by [`ImageCache::resolve`] once per frame, so a
    /// mutation is visible on the next paint.
    pub(crate) fn get(&self, key: &str) -> Option<ImageData> {
        self.read().get(key).cloned()
    }

    /// Read guard, recovering a poisoned lock rather than panicking in the
    /// paint path (a producer thread that panicked mid-write must not take
    /// the renderer down — the last consistent map is still drawable).
    fn read(&self) -> RwLockReadGuard<'_, HashMap<String, ImageData>> {
        self.inner.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// Write guard, recovering a poisoned lock (see [`read`](Self::read)).
    fn write(&self) -> RwLockWriteGuard<'_, HashMap<String, ImageData>> {
        self.inner.write().unwrap_or_else(PoisonError::into_inner)
    }
}

/// R1404 §5.16 §5.22 — the process-wide producer image store slot.
///
/// [`ProviderSlot`] `inherited`, declared in `pinion-runtime` because the
/// store (and the [`ImageCache`] that resolves it) live here — the same
/// reason [`FRAME_TIMINGS`](crate::frame_timing::FRAME_TIMINGS) does. The
/// shell seeds it on the root owner at boot
/// ([`seed_root`](ProviderSlot::seed_root)) and hands the same handle to
/// every window's [`ImageCache`]; a producer resolves it through
/// [`use_image_store`] (or [`resolve_image_store`] off a known owner) to
/// register images. It inherits so a child window scope — the deferred R680
/// `window_owner(id).run(..)` wrap — lands on the ONE root store the painter
/// reads, not a private empty map (the desync R1364/R1365 closed for the
/// lifecycle sinks).
///
/// No `provide`: the store is created empty by the default and MUTATED in
/// place, so there is no shell "real value" to seed and no late-seed panic —
/// `seed_root` at boot only ensures the root owns it before any child
/// resolves.
pub static IMAGE_STORE: ProviderSlot<MemoryImageStore> =
    ProviderSlot::inherited("__pinion.runtime.image_store", MemoryImageStore::new);

/// Resolve the shared [`MemoryImageStore`] off `owner`. The outer `Rc` from
/// the slot stays on the UI thread; the returned [`MemoryImageStore`] is the
/// `Send + Sync` handle a producer thread moves into its reader closure
/// (resolve at wiring time, register from the thread).
#[must_use]
pub fn resolve_image_store(owner: &Owner) -> MemoryImageStore {
    (*IMAGE_STORE.resolve(owner)).clone()
}

/// Binding-facing hook: the shared producer [`MemoryImageStore`]. Resolve it
/// once at wiring time — a `create_external` / `create_extra_externals`
/// hook, both of which run inside an `Owner::run` — and register / update /
/// remove images through the returned handle.
///
/// # Panics
///
/// Panics when called with no active [`Owner`] scope, the strict shape every
/// binding-facing `use_*` hook shares (call it from a hook that runs inside
/// an `Owner::run`).
#[must_use]
pub fn use_image_store() -> MemoryImageStore {
    let owner = Owner::current().expect("use_image_store requires an active Owner scope");
    resolve_image_store(&owner)
}

/// Per-shell cache mapping an image source string to its decoded
/// `peniko::ImageData` (or `None` when the source could not be loaded /
/// decoded — cached so the failure is not retried every frame).
pub struct ImageCache {
    /// Decoded images by source, most-recently-resolved first.
    ///
    /// R1550 — an `LruCache` rather than the `HashMap` this held until then, because the
    /// map had **no bound of any kind**: every image a session ever painted
    /// stayed decoded for the life of the window, and one 4K frame is 33 MB of
    /// RGBA8. The toolkit bounds the same arena — `setCacheLimit` — so an unbounded one
    /// sits below the floor rather than beside it.
    ///
    /// The recency order is what makes the bound safe to apply: eviction takes
    /// the image painted longest ago, and a re-resolve re-decodes it.
    entries: LruCache<String, Option<ImageData>>,
    /// R1550 — the byte budget [`Self::evict_to_budget`] holds `entries` under.
    ///
    /// See [`Self::DEFAULT_BUDGET_BYTES`]. One entry may exceed it — the entry
    /// just resolved is never evicted, which is a deliberate deviation from
    /// `insert`, whose refusal to cache an over-limit pixmap
    /// would put a re-decode of that image on every frame that paints it.
    budget_bytes: u64,
    /// The producer [`MemoryImageStore`] (R1404) this cache resolves
    /// `memory://<key>` sources through, or `None` for a bare cache (a
    /// `memory://` source then paints nothing). The shell builds each
    /// window's cache with the seeded [`IMAGE_STORE`] handle.
    store: Option<MemoryImageStore>,
}

impl ImageCache {
    /// An empty cache with no producer store — resolves filesystem sources
    /// only (a `memory://` source paints nothing).
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Self::empty_entries(),
            budget_bytes: Self::DEFAULT_BUDGET_BYTES,
            store: None,
        }
    }

    /// R1550 — the default byte budget: 10 MiB of decoded pixels, which is
    /// pixmap cache's own default (`cacheLimit()` reports 10240 KB) and about
    /// three 1080p frames.
    ///
    /// A budget rather than an entry count for the reason R1550 exists: entries
    /// are not the resource. A hundred 16x16 icons cost 100 kB and one 4K
    /// screenshot costs 33 MB, and an entry-count bound prices them the same.
    pub const DEFAULT_BUDGET_BYTES: u64 = 10 * 1024 * 1024;

    /// An entry map with no count bound — the bound this cache enforces is
    /// [`Self::budget_bytes`], and stating a second one in entries would be a
    /// second answer to "how much may this hold".
    ///
    /// `unbounded` rather than `new(NonZeroUsize::MAX)`: `lru` allocates its
    /// index map at the stated capacity, so naming a huge one aborts on the
    /// spot with a hash-table capacity overflow.
    fn empty_entries() -> LruCache<String, Option<ImageData>> {
        LruCache::unbounded()
    }

    /// R1550 — set the byte budget, evicting immediately if the new one is
    /// smaller than what is held. The toolkit's `setCacheLimit`.
    pub fn set_budget_bytes(&mut self, budget_bytes: u64) {
        self.budget_bytes = budget_bytes;
        self.evict_to_budget(None);
    }

    /// R1550 — the byte budget this cache evicts toward.
    #[must_use]
    pub fn budget_bytes(&self) -> u64 {
        self.budget_bytes
    }

    /// R1550 — drop least-recently-resolved entries until the arena is within
    /// [`Self::budget_bytes`], never dropping `keep` (the entry the caller just
    /// resolved, which a paint is about to use).
    fn evict_to_budget(&mut self, keep: Option<&str>) {
        while self.entry_bytes() > self.budget_bytes {
            let Some((lru_key, _)) = self.entries.peek_lru() else {
                return;
            };
            if Some(lru_key.as_str()) == keep {
                return;
            }
            if self.entries.pop_lru().is_none() {
                return;
            }
        }
    }

    /// R1550 — decoded bytes held by [`Self::entries`], pixels only.
    fn entry_bytes(&self) -> u64 {
        self.entries
            .iter()
            .filter_map(|(_, slot)| slot.as_ref())
            .map(|image| image.data.len() as u64)
            .sum()
    }

    /// A cache wired to a producer [`MemoryImageStore`], so `memory://<key>`
    /// sources resolve to the producer's registered images. The shell builds
    /// each window's cache this way off the seeded [`IMAGE_STORE`].
    #[must_use]
    pub fn with_store(store: MemoryImageStore) -> Self {
        Self {
            entries: Self::empty_entries(),
            budget_bytes: Self::DEFAULT_BUDGET_BYTES,
            store: Some(store),
        }
    }

    /// Resolve `source` to a drawable `peniko::ImageData`. A `memory://<key>`
    /// source (R1404) resolves through the producer [`MemoryImageStore`]
    /// (`None` when the store is absent or nothing is registered under
    /// `<key>`); any other source is treated as a filesystem path, decoded +
    /// cached on the first call (the miss is cached too, so a broken source
    /// costs one failed read, not one per frame). The returned value is a
    /// cheap clone (the pixel buffer is `Arc`-shared via `peniko::Blob`).
    pub fn resolve(&mut self, source: &str) -> Option<ImageData> {
        // A `memory://<key>` source resolves through the producer store, NOT
        // the decode-once `entries` map: the store is mutable (a terminal
        // image updates / is deleted), so caching the resolved image would
        // pin a stale frame. The store's own get is a cheap Arc clone.
        if let Some(key) = source.strip_prefix(MEMORY_SCHEME) {
            return self.store.as_ref()?.get(key);
        }
        // `LruCache::get` is `&mut self` by construction: reading an entry is
        // what makes it recent, and recency is what the budget evicts by.
        if let Some(slot) = self.entries.get(source) {
            return slot.clone();
        }
        let loaded = load_source(source).map(|d| to_image_data(&d));
        self.entries.put(source.to_owned(), loaded.clone());
        self.evict_to_budget(Some(source));
        loaded
    }

    /// Number of distinct sources resolved so far (hits + misses).
    /// Diagnostic / test surface.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no source has been resolved yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Insert a pre-decoded image directly under `source`, bypassing the
    /// filesystem. Lets a test seed the decode-once cache deterministically
    /// without a fixture file on disk. (The producer `memory://` path is now
    /// the [`MemoryImageStore`]; this remains the direct seed for
    /// filesystem-path sources — do not pass a `memory://` source here, it
    /// would land in the decode-once map that [`resolve`](Self::resolve)
    /// bypasses for that scheme.)
    pub fn insert_decoded(&mut self, source: impl Into<String>, image: &DecodedImage) {
        let source = source.into();
        self.entries.put(source.clone(), Some(to_image_data(image)));
        self.evict_to_budget(Some(&source));
    }
}

impl Default for ImageCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Read + decode a filesystem `source` into a backend-agnostic
/// [`DecodedImage`]. `None` on any read or decode failure (the cache
/// records the miss).
fn load_source(source: &str) -> Option<DecodedImage> {
    let bytes = std::fs::read(source).ok()?;
    pinion_asset::decode_image(&bytes).ok()
}

/// Convert a backend-agnostic [`DecodedImage`] into a `peniko::ImageData`
/// by wrapping its `Arc`-shared RGBA8 buffer in a `peniko::Blob` (no
/// pixel copy). Straight (un-premultiplied) alpha, matching
/// [`pinion_asset`]'s output format.
fn to_image_data(decoded: &DecodedImage) -> ImageData {
    let pixels: Arc<dyn AsRef<[u8]> + Send + Sync> = decoded.pixels().clone();
    ImageData {
        data: Blob::new(pixels),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width: decoded.width(),
        height: decoded.height(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded_2x2() -> DecodedImage {
        DecodedImage::from_rgba8(2, 2, vec![10; 2 * 2 * 4]).unwrap()
    }

    /// A `w`x`h` solid-`fill` decoded image (distinct dims prove which image
    /// a resolve returned).
    fn decoded(w: u32, h: u32, fill: u8) -> DecodedImage {
        DecodedImage::from_rgba8(w, h, vec![fill; (w * h * 4) as usize]).unwrap()
    }

    #[test]
    fn new_cache_is_empty() {
        let c = ImageCache::new();
        assert!(c.is_empty());
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn insert_decoded_then_resolve_hits() {
        let mut c = ImageCache::new();
        // A non-`memory://` key: `insert_decoded` seeds the decode-once
        // `entries` map, which `resolve` reads for any non-scheme source.
        c.insert_decoded("fixture:2x2", &decoded_2x2());
        let data = c.resolve("fixture:2x2").expect("seeded source resolves");
        assert_eq!(data.width, 2);
        assert_eq!(data.height, 2);
        assert_eq!(data.format, ImageFormat::Rgba8);
        assert_eq!(data.alpha_type, ImageAlphaType::Alpha);
        // Cloning the resolved data shares the same Blob backing.
        let again = c.resolve("fixture:2x2").unwrap();
        assert_eq!(again.data.as_ref().len(), 2 * 2 * 4);
    }

    #[test]
    fn missing_file_caches_the_miss() {
        let mut c = ImageCache::new();
        assert!(
            c.resolve("/no/such/file/exists.png").is_none(),
            "missing → None"
        );
        // The miss is cached (one entry), so a second resolve does not retry IO.
        assert_eq!(c.len(), 1);
        assert!(c.resolve("/no/such/file/exists.png").is_none());
        assert_eq!(c.len(), 1, "second resolve reuses the cached miss");
    }

    // --- R1404 producer memory:// store ------------------------------------

    #[test]
    fn memory_scheme_resolves_through_the_store() {
        let store = MemoryImageStore::new();
        store.insert("logo", &decoded(4, 3, 200));
        let mut c = ImageCache::with_store(store);
        let data = c
            .resolve("memory://logo")
            .expect("registered memory source");
        assert_eq!((data.width, data.height), (4, 3));
        // An unregistered key resolves to nothing (the graceful skip).
        assert!(c.resolve("memory://missing").is_none());
        // A `memory://` source is NOT entered into the decode-once map — it
        // must re-read the store each frame so a mutation is visible.
        assert!(c.is_empty(), "memory sources bypass the decode-once cache");
    }

    #[test]
    fn bare_cache_skips_the_memory_scheme() {
        // No store wired: a `memory://` source paints nothing rather than
        // being read as a filesystem path (which would be a spurious miss).
        let mut c = ImageCache::new();
        assert!(c.resolve("memory://logo").is_none());
        assert!(c.is_empty(), "no filesystem read is attempted");
    }

    #[test]
    fn store_mutation_and_removal_are_visible_on_the_next_resolve() {
        let store = MemoryImageStore::new();
        store.insert("img", &decoded(2, 2, 10));
        let mut c = ImageCache::with_store(store.clone());
        assert_eq!(c.resolve("memory://img").unwrap().width, 2);

        // Re-register the SAME key with different dims (a terminal image
        // update): the next resolve returns the new image, not the old.
        store.insert("img", &decoded(8, 5, 20));
        let updated = c.resolve("memory://img").expect("updated image");
        assert_eq!((updated.width, updated.height), (8, 5), "update is visible");

        // Removing the key makes the next resolve paint nothing (delete).
        assert!(store.remove("img"));
        assert!(c.resolve("memory://img").is_none(), "removal is visible");
        assert!(!store.remove("img"), "second remove reports absent");
    }

    #[test]
    fn store_insert_contains_remove_len() {
        let store = MemoryImageStore::new();
        assert!(store.is_empty());
        store.insert("a", &decoded(2, 2, 1));
        store.insert("b", &decoded(2, 2, 2));
        assert_eq!(store.len(), 2);
        assert!(store.contains("a"));
        assert!(!store.contains("c"));
        assert!(store.remove("a"));
        assert!(!store.contains("a"));
        assert_eq!(store.len(), 1);
        // Cloned handles share one map (the Arc), so a clone's mutation is
        // seen through the original — the shell/producer split.
        let clone = store.clone();
        clone.insert("c", &decoded(2, 2, 3));
        assert!(store.contains("c"), "a clone shares the one backing map");
    }

    #[test]
    fn resolve_image_store_shares_one_handle_off_an_owner() {
        // Two resolves off the same owner return the SAME store (inherited
        // slot), so the producer's registration is seen by the painter's.
        let root = Owner::new();
        let producer = resolve_image_store(&root);
        let painter = resolve_image_store(&root);
        producer.insert("x", &decoded(2, 2, 9));
        assert!(painter.contains("x"), "one root store, two handles");
        // A child scope inherits the root store, not a private empty one.
        let child = Owner::new_child(&root);
        assert!(
            resolve_image_store(&child).contains("x"),
            "a child window scope resolves the root store",
        );
    }

    // The Inherited verdict, EMITTED from the declaration (R1366 discipline).
    pinion_core::provider_slot_tests!(
        r1404_image_store_inherits,
        super::IMAGE_STORE,
        MemoryImageStore::new
    );
}

/// R1550 §5.16 §5.7 — what the decoded-image arenas are holding, in bytes.
///
/// Exact. A `peniko::ImageData` is a `Blob` over an `Arc<[u8]>` of pixels and
/// four plain fields, and the blob reports its own length — no interior is
/// hidden the way parley's `Layout` hides its glyph buffers.
///
/// # Two arenas, not one
///
/// A window's [`ImageCache`] holds what it decoded from the filesystem; the
/// shell-wide [`MemoryImageStore`] holds what a producer registered under
/// `memory://`. They are separately owned and separately sized — a terminal
/// emitting Kitty frames grows the store while every window's decode cache
/// stays empty — so they publish separate rows.
///
/// [`ImageCache::store`] is a handle to the second arena, and is deliberately
/// not in the first one's total: counting it in every window would report one
/// registered image once per window.
mod footprint {
    use super::{ImageCache, MemoryImageStore};
    use pinion_core::footprint::{Footprint, hash_table_bytes, lru_table_bytes};
    use pinion_core::memory_census::{Arena, ArenaFootprint, MeasuredArena};
    use vello::peniko::ImageData;

    /// The pixels an [`ImageData`] holds. A free function rather than a
    /// `Footprint` impl: both the type and the trait are foreign to this
    /// crate, which the orphan rule forbids.
    fn image_bytes(image: &ImageData) -> usize {
        let ImageData {
            data,
            format: _,
            alpha_type: _,
            width: _,
            height: _,
        } = image;
        data.len()
    }

    impl Footprint for ImageCache {
        fn footprint(&self) -> usize {
            let Self {
                entries,
                budget_bytes,
                // The producer store is a shared handle — see the module note.
                store: _,
            } = self;
            let _ = budget_bytes;
            lru_table_bytes::<String, Option<ImageData>>(entries.len(), entries.len())
                + entries
                    .iter()
                    .map(|(source, slot)| source.footprint() + slot.as_ref().map_or(0, image_bytes))
                    .sum::<usize>()
        }
    }

    impl MeasuredArena for ImageCache {
        fn arena_footprint(&self) -> ArenaFootprint {
            ArenaFootprint::exact(Arena::Images, self.footprint() as u64, self.len() as u64)
                .with_budget(self.budget_bytes())
        }
    }

    impl Footprint for MemoryImageStore {
        fn footprint(&self) -> usize {
            let map = self.read();
            hash_table_bytes::<(String, ImageData)>(map.capacity())
                + map
                    .iter()
                    .map(|(key, image)| key.footprint() + image_bytes(image))
                    .sum::<usize>()
        }
    }

    impl MeasuredArena for MemoryImageStore {
        fn arena_footprint(&self) -> ArenaFootprint {
            ArenaFootprint::exact(Arena::Images, self.footprint() as u64, self.len() as u64)
        }
    }
}

#[cfg(test)]
mod r1550_tests {
    use super::*;
    use pinion_core::footprint::Footprint;
    use pinion_core::memory_census::{FootprintBasis, MeasuredArena};

    /// A `w`x`h` solid image — `w * h * 4` bytes of RGBA8.
    fn decoded(w: u32, h: u32, fill: u8) -> DecodedImage {
        DecodedImage::from_rgba8(w, h, vec![fill; (w * h * 4) as usize]).unwrap()
    }

    /// The whole point of a byte budget over an entry count: the arena is
    /// priced by what it holds, not by how many things it holds.
    #[test]
    fn r1550_footprint_tracks_pixels_not_entries() {
        let mut small = ImageCache::new();
        let mut large = ImageCache::new();
        small.insert_decoded("a", &decoded(16, 16, 1));
        large.insert_decoded("a", &decoded(256, 256, 1));
        assert_eq!(small.len(), large.len(), "same entry count");
        assert!(
            large.footprint() > 200 * small.footprint(),
            "256x256 costs 256x the pixels of 16x16: {} vs {}",
            large.footprint(),
            small.footprint(),
        );
    }

    #[test]
    fn r1550_over_budget_evicts_the_least_recently_resolved() {
        let mut cache = ImageCache::new();
        // 32x32 RGBA8 = 4,096 bytes each; three of them exceed a 10,000-byte
        // budget by one.
        cache.set_budget_bytes(10_000);
        cache.insert_decoded("first", &decoded(32, 32, 1));
        cache.insert_decoded("second", &decoded(32, 32, 2));
        assert_eq!(cache.len(), 2, "two fit");
        cache.insert_decoded("third", &decoded(32, 32, 3));
        assert_eq!(cache.len(), 2, "the third pushed the first out");
        assert!(
            cache.resolve("first").is_none(),
            "and 'first' is the one gone — least recently resolved",
        );
    }

    /// Recency is what the bound evicts by, so a *read* must move an entry
    /// out of the firing line. This is the assertion that fails if `resolve`
    /// peeks instead of touching.
    #[test]
    fn r1550_resolving_an_entry_protects_it_from_the_next_eviction() {
        let mut cache = ImageCache::new();
        cache.set_budget_bytes(10_000);
        cache.insert_decoded("first", &decoded(32, 32, 1));
        cache.insert_decoded("second", &decoded(32, 32, 2));
        assert!(cache.resolve("first").is_some(), "touch 'first'");
        cache.insert_decoded("third", &decoded(32, 32, 3));
        assert!(
            cache.resolve("first").is_some(),
            "'first' was resolved most recently, so it stays",
        );
        assert!(cache.resolve("second").is_none(), "'second' went instead");
    }

    /// The toolkit's `insert` refuses a pixmap larger than the limit and
    /// returns false. Here the entry is kept, deliberately: `resolve` is on
    /// the paint path, so refusing to cache would re-read and re-decode that
    /// image on **every frame** that paints it.
    #[test]
    fn r1550_an_entry_larger_than_the_budget_is_kept() {
        let mut cache = ImageCache::new();
        cache.set_budget_bytes(1_000);
        cache.insert_decoded("huge", &decoded(64, 64, 1));
        assert_eq!(cache.len(), 1);
        assert!(
            cache.footprint() > 1_000,
            "the arena states that it is over budget rather than thrashing",
        );
        // And it is the only survivor: a second image evicts nothing else,
        // because there is nothing else to evict.
        cache.insert_decoded("also-huge", &decoded(64, 64, 2));
        assert_eq!(cache.len(), 1, "each oversized entry displaces the last");
    }

    #[test]
    fn r1550_shrinking_the_budget_evicts_immediately() {
        let mut cache = ImageCache::new();
        cache.insert_decoded("a", &decoded(32, 32, 1));
        cache.insert_decoded("b", &decoded(32, 32, 2));
        assert_eq!(cache.len(), 2);
        cache.set_budget_bytes(5_000);
        assert_eq!(
            cache.len(),
            1,
            "the toolkit's setCacheLimit, applied at once"
        );
    }

    #[test]
    fn r1550_arena_row_is_exact_and_states_its_budget() {
        let mut cache = ImageCache::new();
        cache.insert_decoded("a", &decoded(16, 16, 1));
        let row = cache.arena_footprint();
        assert_eq!(row.basis(), FootprintBasis::Exact, "pixels are measurable");
        assert_eq!(row.entries, 1);
        assert!(row.bytes >= 16 * 16 * 4);
        assert_eq!(row.budget_bytes, Some(ImageCache::DEFAULT_BUDGET_BYTES));
    }

    /// The producer store is a separate arena. A window's cache holding a
    /// handle to it must not report the store's images as its own, or a
    /// three-window shell would count one registered image three times.
    #[test]
    fn r1550_the_producer_store_is_not_counted_by_the_window_cache() {
        let store = MemoryImageStore::new();
        store.insert("frame", &decoded(64, 64, 7));
        let cache = ImageCache::with_store(store.clone());
        assert_eq!(cache.footprint(), 0, "the window cache decoded nothing");
        assert!(
            store.footprint() >= 64 * 64 * 4,
            "and the store owns the pixels",
        );
        assert_eq!(store.arena_footprint().entries, 1);
    }
}
