//! R787 §5.15 §5.16 — own-rendered file-browser state: the reactive
//! directory-navigation model + its AI-first `External` surface, built
//! over the [`Directory`](crate::directory::Directory) read substrate.
//!
//! This is the scene-graph-native peer of an OS file dialog. Where
//! `examples/hello-file-dialog` bridges to a native (`rfd`-class) dialog
//! — a window pinion does not own, so it is invisible to `scene/query`
//! and unverifiable on a headless Linux box ([[native-menu-macos-windows-only-verify]])
//! — this browses the filesystem *inside* pinion's own scene tree: every
//! entry is a paint node, the current directory + listing + selection are
//! reactive `Signal`s, and an AI agent drives the whole flow over
//! `scene/query` + `scene/invoke` (§2 #2 / #7). The `Directory` trait is
//! the only escape hatch; the navigation logic + reactive state are pure.
//!
//! ## Pieces
//!
//! - [`DirectoryState`] — the reactive holder: `cwd` / `entries` /
//!   `selected` `Signal`s over a shared [`Directory`], with the
//!   navigation transitions (`navigate` into a child dir, `up` to the
//!   parent, `select` a leaf, `open_dir` an absolute jump). Mirrors the
//!   [`ColumnWidths`](crate::widgets::column_widths::ColumnWidths) /
//!   [`ScrollState`](crate::widgets::scroll::ScrollState) reactive-holder
//!   pattern: the view reads it, the `External` mutates it, one `Rc` SSOT
//!   via [`use_directory_state`].
//! - [`DirectoryExternal`] — the `External` adapter exposing the model
//!   over RPC: query `cwd` / `count` / `entries` / `selected` /
//!   `name.<i>` / `is_dir.<i>`; invoke `navigate` / `up` / `select` /
//!   `open`.

use std::borrow::Cow;
use std::cell::Cell;
use std::rc::Rc;

use crate::composite_tag::split_subindex;
use crate::directory::{DirEntry, Directory};
use crate::external::{
    Backend, BackendFallback, BackendSupport, DragPayload, DropPoint, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, ThreadOwnership,
};
use crate::input::{PointerWireEvent, is_activation_event};
use crate::reactive::{Owner, Signal};
use crate::widgets::scroll::ScrollState;

/// Join `name` onto the directory path `base` over `'/'` separators. A
/// `base` of `"/"` yields `"/name"`; otherwise `"<base>/name"`. The leaf
/// `name` is assumed to carry no separator (it is one [`DirEntry::name`]).
#[must_use]
pub fn join_path(base: &str, name: &str) -> String {
    if base.ends_with('/') {
        format!("{base}{name}")
    } else {
        format!("{base}/{name}")
    }
}

/// The parent of a `'/'`-separated path: `"/a/b"` → `"/a"`, `"/a"` →
/// `"/"`, `"/"` → `"/"` (the root is its own parent — `up` at the root is
/// a no-op). A trailing slash is ignored (`"/a/b/"` → `"/a"`).
#[must_use]
pub fn parent_path(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rsplit_once('/') {
        // Split above the root: the parent is everything before the last
        // segment, or "/" when that prefix is empty (direct child of root).
        Some((prefix, _)) if !prefix.is_empty() => prefix.to_string(),
        Some((_, _)) => "/".to_string(),
        // No separator at all (a relative root like "foo") → itself.
        None => path.to_string(),
    }
}

/// R794 §5.51 — the [`DragPayload::kind`] a file-browser row drag carries
/// (the discriminator a future cross-widget drop target matches on before
/// reading the dragged leaf name). One SSOT the binding / demo reference.
pub const FILE_DRAG_KIND: &str = "file-row";

/// R794 §5.51 — the live drop target a file-row drag resolves under the
/// cursor: a **directory row** to move the dragged entry *into*, or the
/// `../` **breadcrumb** to move it *up* to the parent. A file row / the
/// dragged source itself / the background resolve to no target (`None`),
/// so a drop there is an inert release. The view reads this (via
/// [`DirectoryState::drop_target`]) to paint the "drop here" affordance,
/// and an AI agent reads it back through the `External`'s `drop_target`
/// query (§2 #7 — the in-flight drag is scene-as-data).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FileDropTarget {
    /// Move the dragged entry into the directory at this **visual row
    /// index** of the current listing.
    Row(usize),
    /// Move the dragged entry up to the parent directory (the `../`
    /// breadcrumb affordance).
    Up,
}

/// R787 §5.15 §5.16 — reactive directory-navigation model for one file
/// browser. Holds the shared [`Directory`] and the three reactive
/// `Signal`s the view subscribes to (`cwd` / `entries` / `selected`); a
/// navigation mutates them and every subscribed view repaints.
#[derive(Clone)]
pub struct DirectoryState {
    dir: Rc<dyn Directory>,
    cwd: Signal<String>,
    entries: Signal<Vec<DirEntry>>,
    selected: Signal<Option<String>>,
    /// R792 §5.15 §5.40 — the roving **keyboard cursor** (WAI-ARIA
    /// `aria-activedescendant`): the visual entry-row the arrow keys
    /// address, distinct from [`selected`](Self::selected) (the picked
    /// leaf's path). `None` means "unmoved" — the effective cursor then
    /// defaults to the first row (the W3C "focus lands on the first option"
    /// convention), surfaced through [`cursor`](Self::cursor). A directory
    /// change resets it to `None` (the new listing's first row becomes the
    /// cursor). This is the Files/Explorer focus model: arrows move a
    /// highlight over folders *and* files, `Enter` opens a folder or picks a
    /// file — orthogonal to which leaf is currently selected.
    cursor: Signal<Option<usize>>,
    /// R794 §5.51 — the **drag-arm**: the visual row a `PointerDown`
    /// pressed, which [`begin_file_drag`](Self::begin_file_drag) reads to
    /// start a drag session. A `Cell` (not a reactive `Signal`) because
    /// arming is transient pointer bookkeeping the view never paints — the
    /// reorder model's `pressed` precedent. Cleared on drop.
    pressed: Cell<Option<usize>>,
    /// R794 §5.51 — the live [`FileDropTarget`] under the cursor during a
    /// drag, or `None`. A reactive `Signal` (unlike `pressed`) because the
    /// view *does* paint the "drop here" highlight on it, so a `drag_to`
    /// update repaints the targeted row. Cleared on drop.
    drop_target: Signal<Option<FileDropTarget>>,
}

impl core::fmt::Debug for DirectoryState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DirectoryState")
            .field("cwd", &self.cwd.get())
            .field("entry_count", &self.entries.get().len())
            .field("selected", &self.selected.get())
            .field("cursor", &self.cursor.get())
            .field("drop_target", &self.drop_target.get())
            .finish_non_exhaustive()
    }
}

impl DirectoryState {
    /// Construct over a shared [`Directory`], starting at `initial`. Reads
    /// the initial listing eagerly (an unreadable initial path lists empty,
    /// the total-surface shape) and starts with no selection.
    #[must_use]
    pub fn new(dir: Rc<dyn Directory>, initial: impl Into<String>) -> Self {
        let initial = initial.into();
        let entries = dir.read_dir(&initial).unwrap_or_default();
        Self {
            dir,
            cwd: Signal::new(initial),
            entries: Signal::new(entries),
            selected: Signal::new(None),
            cursor: Signal::new(None),
            pressed: Cell::new(None),
            drop_target: Signal::new(None),
        }
    }

    /// Current directory path. Subscribes when read inside a view-fn.
    #[must_use]
    pub fn cwd(&self) -> String {
        self.cwd.get()
    }

    /// Listing of the current directory (canonical order). Subscribes.
    #[must_use]
    pub fn entries(&self) -> Vec<DirEntry> {
        self.entries.get()
    }

    /// Number of entries in the current directory. Subscribes.
    #[must_use]
    pub fn count(&self) -> usize {
        self.entries.get().len()
    }

    /// The selected leaf's full path, or `None`. Subscribes.
    #[must_use]
    pub fn selected(&self) -> Option<String> {
        self.selected.get()
    }

    /// R789.1 — the **visual row index** of the current selection in the
    /// current listing, or `None`. Maps the path-keyed
    /// [`selected`](Self::selected) back to a row position, so a painted
    /// Accent row and the a11y `aria-selected` agree. Subscribes (reads
    /// `selected` / `cwd` / `entries`). The path→row mapping is the
    /// `DirectoryState`'s own derivation — every file UI reads it rather
    /// than re-deriving it (the SSOT the R789 examples duplicated).
    #[must_use]
    pub fn selected_index(&self) -> Option<usize> {
        let selected = self.selected()?;
        let cwd = self.cwd();
        self.entries()
            .iter()
            .position(|e| join_path(&cwd, &e.name) == selected)
    }

    /// R791.1 — the **leaf name** of the current selection (`"/proj/a.txt"`
    /// → `"a.txt"`), or `None`. The selection is stored as a full path; the
    /// path→name derivation is the `DirectoryState`'s own responsibility (the
    /// sibling of [`selected_index`](Self::selected_index)'s path→row map), so
    /// a save dialog's "click-to-overwrite" prefill and the file manager's
    /// inline-rename prefill read one SSOT rather than each re-deriving the
    /// basename. Subscribes (reads `selected`).
    #[must_use]
    pub fn selected_name(&self) -> Option<String> {
        self.selected()
            .map(|p| p.rsplit('/').next().unwrap_or(&p).to_owned())
    }

    /// R792 §5.15 §5.40 — the **effective keyboard cursor**: the visual row
    /// the arrow keys / focus ring address. `None` only when the listing is
    /// empty; otherwise the explicit cursor, or the **first row** when the
    /// cursor is unmoved (the W3C "first key focuses the first option"
    /// convention). This is the SSOT every consumer reads — the focus-ring
    /// active descendant, the `Enter`-activation target, and the
    /// `aria-activedescendant` a11y node — so the painted ring and the
    /// activated row never disagree. Subscribes (reads `cursor` + `entries`).
    #[must_use]
    pub fn cursor(&self) -> Option<usize> {
        if self.entries.get().is_empty() {
            None
        } else {
            Some(self.cursor.get().unwrap_or(0))
        }
    }

    /// R792 — move the keyboard cursor per `key` (`ArrowUp` / `ArrowDown` /
    /// `Home` / `End` / `PageUp` / `PageDown`) over the current listing,
    /// `page` rows per viewport-ful. Linear-clamp (no wrap — a directory
    /// listing has ends), the shared [`clamp_nav`](crate::widgets::virtual_select::clamp_nav)
    /// policy the virtualized list / grid navigation reuses. Returns whether
    /// the key was a handled navigation key (so the binding's `apply_key`
    /// returns the right bool). A no-op `false` for an empty listing or an
    /// unhandled key.
    #[allow(
        clippy::must_use_candidate,
        reason = "the handled-bool drives the controller's apply_key result, but a \
                  caller that just wants to move the cursor legitimately ignores it \
                  (the navigate / select fire-and-forget mutator precedent)"
    )]
    pub fn move_cursor(&self, key: &str, page: usize) -> bool {
        let Some(target) = crate::widgets::virtual_select::clamp_nav(
            self.cursor(),
            key,
            self.entries.get().len(),
            page,
        ) else {
            return false;
        };
        self.cursor.set(Some(target));
        true
    }

    /// R792 — set the keyboard cursor directly (the admin / restore channel,
    /// the `aria-activedescendant` intervene path). An out-of-range index is
    /// clamped to the last row; `None` resets to "unmoved" (the effective
    /// cursor falls back to the first row). Not an interaction.
    pub fn set_cursor(&self, index: Option<usize>) {
        let clamped = index.map(|i| {
            let count = self.entries.get().len();
            i.min(count.saturating_sub(1))
        });
        self.cursor.set(clamped);
    }

    /// R792 — activate the entry under the keyboard cursor (the `Enter`
    /// gesture): navigate into it if it is a directory, else select it (the
    /// shared [`activate_index`](Self::activate_index) row gesture). A no-op
    /// on an empty listing.
    pub fn activate_cursor(&self) {
        if let Some(idx) = self.cursor() {
            self.activate_index(idx);
        }
    }

    /// R802 §5.15 §5.40 — **selection-follows-focus** cursor move for a
    /// single-select picker (the file-open dialog). Moves the keyboard cursor
    /// like [`move_cursor`](Self::move_cursor), then syncs the file selection
    /// to the row the cursor lands on (a file row becomes the selected leaf, a
    /// directory row clears the selection — directories are navigation targets,
    /// not picks, matching GTK / macOS open panels). [`batch`](crate::reactive::batch)ed
    /// so the cursor + selection writes re-render subscribers once. Returns the
    /// handled bool like `move_cursor`.
    ///
    /// This is the opt-in picker policy: the Files / Explorer model keeps using
    /// `move_cursor` (cursor orthogonal to the picked leaf, R792). Which policy
    /// applies is the consumer's choice of [`dir_nav_key`] vs
    /// [`dir_nav_key_selecting`], not a mode flag on the controller.
    #[allow(
        clippy::must_use_candidate,
        reason = "the handled-bool drives apply_key's result; a caller that just \
                  wants to move + select legitimately ignores it (the move_cursor precedent)"
    )]
    pub fn move_cursor_selecting(&self, key: &str, page: usize) -> bool {
        crate::reactive::batch(|| {
            if !self.move_cursor(key, page) {
                return false;
            }
            self.sync_selection_to_cursor();
            true
        })
    }

    /// R802 — set the file selection to the row under the effective keyboard
    /// [`cursor`](Self::cursor): a file row becomes the selected leaf (its full
    /// path), a directory row (or an empty listing) clears the selection. The
    /// mechanism behind [`move_cursor_selecting`](Self::move_cursor_selecting)'s
    /// selection-follows-focus policy.
    fn sync_selection_to_cursor(&self) {
        let entries = self.entries.get();
        match self.cursor().and_then(|i| entries.get(i)) {
            // A file row picks through the [`select`](Self::select) SSOT (it
            // owns the cwd-joined path build, shared with `activate_index`); a
            // directory row (or empty listing) clears — dirs are nav targets.
            Some(entry) if !entry.is_dir => {
                self.select(&entry.name);
            }
            _ => self.selected.set(None),
        }
    }

    /// R792.1 — enter a new directory context `new_cwd`: set the cwd, clear
    /// the selection + keyboard cursor (a new listing has neither), and
    /// refresh the listing. The single SSOT for the
    /// [`navigate`](Self::navigate) / [`up`](Self::up) /
    /// [`open_dir`](Self::open_dir) directory change (each previously inlined
    /// the same four-signal reset). [`batch`](crate::reactive::batch)ed so
    /// the change re-renders subscribers once — the
    /// [`delete_selected`](Self::delete_selected) /
    /// [`rename_selected`](Self::rename_selected) precedent. The effective
    /// [`cursor`](Self::cursor) then defaults to the new listing's first row.
    fn enter_directory(&self, new_cwd: String) {
        crate::reactive::batch(|| {
            self.cwd.set(new_cwd);
            self.selected.set(None);
            self.cursor.set(None);
            self.refresh();
        });
    }

    /// Re-read the current directory's listing into the `entries` Signal
    /// (after a `cwd` change). An unreadable directory lists empty.
    fn refresh(&self) {
        let listing = self.dir.read_dir(&self.cwd.get()).unwrap_or_default();
        self.entries.set(listing);
    }

    /// Navigate into the child directory named `name`. No-op (returns the
    /// unchanged `cwd`) when `name` is not a directory entry of the current
    /// listing — selecting a file is [`select`](Self::select)'s job. On a
    /// real navigation the listing refreshes and the selection clears.
    /// Returns the resulting `cwd` (the AI-first setter-returns-outcome
    /// contract).
    #[allow(
        clippy::must_use_candidate,
        reason = "the returned cwd is the read-outcome the AI-first invoke path \
                  reports in one round-trip; the fire-and-forget row-click `send` \
                  handler legitimately ignores it (the ColumnWidths::set_width \
                  precedent)"
    )]
    pub fn navigate(&self, name: &str) -> String {
        let is_child_dir = self
            .entries
            .get()
            .iter()
            .any(|e| e.is_dir && e.name == name);
        if is_child_dir {
            self.enter_directory(join_path(&self.cwd.get(), name));
        }
        self.cwd.get()
    }

    /// Navigate to the parent directory (a no-op at the root). Refreshes
    /// the listing and clears the selection. Returns the resulting `cwd`.
    #[allow(
        clippy::must_use_candidate,
        reason = "setter-returns-outcome; the row-click `send` handler ignores \
                  the returned cwd (see `navigate`)"
    )]
    pub fn up(&self) -> String {
        let parent = parent_path(&self.cwd.get());
        if parent != self.cwd.get() {
            self.enter_directory(parent);
        }
        self.cwd.get()
    }

    /// Jump to an absolute `path` (admin / breadcrumb navigation), refresh
    /// the listing, clear the selection. Returns the resulting `cwd`.
    #[allow(
        clippy::must_use_candidate,
        reason = "setter-returns-outcome; callers (intervene `cwd`) ignore the \
                  returned cwd (see `navigate`)"
    )]
    pub fn open_dir(&self, path: impl Into<String>) -> String {
        self.enter_directory(path.into());
        self.cwd.get()
    }

    /// Select the entry named `name` (its full path becomes
    /// [`selected`](Self::selected)). No-op when `name` is not in the
    /// current listing. Returns the resulting selection. A file picker's
    /// binding calls this on a single click; a double-click on a directory
    /// routes to [`navigate`](Self::navigate) instead.
    #[allow(
        clippy::must_use_candidate,
        reason = "setter-returns-outcome; the row-click `send` handler ignores \
                  the returned selection (see `navigate`)"
    )]
    pub fn select(&self, name: &str) -> Option<String> {
        if self.entries.get().iter().any(|e| e.name == name) {
            self.selected.set(Some(join_path(&self.cwd.get(), name)));
        }
        self.selected.get()
    }

    /// R789 — create a directory named `name` inside the current
    /// directory, refreshing the listing on success. Returns whether it
    /// was created (`false` when the name is taken or the backing is
    /// read-only). The file-manager "New folder" affordance.
    #[allow(
        clippy::must_use_candidate,
        reason = "the returned success bool is the AI-first invoke outcome (mkdir); \
                  the fire-and-forget toolbar-click reducer legitimately ignores it \
                  and re-reads the listing (the navigate / set_width precedent)"
    )]
    pub fn create_dir(&self, name: &str) -> bool {
        let ok = self.dir.create_dir(&join_path(&self.cwd.get(), name));
        if ok {
            self.refresh();
        }
        ok
    }

    /// R789 — create an empty file named `name` inside the current
    /// directory, refreshing the listing on success. Returns whether it
    /// was created. The "New file" affordance.
    #[allow(
        clippy::must_use_candidate,
        reason = "setter-returns-outcome; the toolbar-click reducer ignores the \
                  returned bool (see `create_dir`)"
    )]
    pub fn create_file(&self, name: &str) -> bool {
        let ok = self.dir.create_file(&join_path(&self.cwd.get(), name));
        if ok {
            self.refresh();
        }
        ok
    }

    /// R789 — remove the currently [`selected`](Self::selected) entry (a
    /// file or a directory + its subtree), clearing the selection and
    /// refreshing the listing. A no-op (`false`) when nothing is selected
    /// or the remove failed. The "Delete" affordance.
    #[allow(
        clippy::must_use_candidate,
        reason = "setter-returns-outcome; the toolbar-click reducer ignores the \
                  returned bool (see `create_dir`)"
    )]
    pub fn delete_selected(&self) -> bool {
        let Some(path) = self.selected.get() else {
            return false;
        };
        let ok = self.dir.remove(&path);
        if ok {
            crate::reactive::batch(|| {
                self.selected.set(None);
                self.refresh();
            });
        }
        ok
    }

    /// R791 — rename the currently [`selected`](Self::selected) entry to
    /// `new_name` within the current directory, moving the selection to the
    /// new path and refreshing the listing. A no-op (`false`) when nothing
    /// is selected, `new_name` is blank, or the rename failed (name taken /
    /// read-only). The file-manager "Rename" affordance.
    #[allow(
        clippy::must_use_candidate,
        reason = "setter-returns-outcome; the reducer ignores the bool (see `create_dir`)"
    )]
    pub fn rename_selected(&self, new_name: &str) -> bool {
        let new_name = new_name.trim();
        if new_name.is_empty() {
            return false;
        }
        let Some(from) = self.selected.get() else {
            return false;
        };
        let to = join_path(&self.cwd.get(), new_name);
        let ok = self.dir.rename(&from, &to);
        if ok {
            crate::reactive::batch(|| {
                self.selected.set(Some(to));
                self.refresh();
            });
        }
        ok
    }

    /// Activate the entry at **visual index** `idx` (a row click /
    /// keyboard activation): navigate into it if it is a directory, else
    /// select it. The browser's canonical single-affordance row gesture
    /// (Files/Explorer "click a folder to open, click a file to pick").
    /// Out-of-range `idx` is a silent no-op.
    pub fn activate_index(&self, idx: usize) {
        let Some(entry) = self.entries.get().get(idx).cloned() else {
            return;
        };
        if entry.is_dir {
            self.navigate(&entry.name);
        } else {
            self.select(&entry.name);
        }
    }

    // ----- R794 drag-to-move -------------------------------------------

    /// R794 — the drag-armed visual row (the most recent `PointerDown`), or
    /// `None`. The drag source [`begin_file_drag`](Self::begin_file_drag)
    /// reads; exposed for the `External`'s `dragging` introspection slot.
    #[must_use]
    pub fn pressed(&self) -> Option<usize> {
        self.pressed.get()
    }

    /// R794 — arm a drag from the visual row `index` (the binding's row
    /// `PointerDown` send). An out-of-range index clears the arm (a press
    /// on no real row starts no drag). Not an interaction — pure pointer
    /// bookkeeping, the reorder-model `pressed` precedent.
    pub fn press(&self, index: usize) {
        self.pressed
            .set((index < self.entries.get().len()).then_some(index));
    }

    /// R794 §5.51 — arm a [`DragPayload`] from the [`pressed`](Self::pressed)
    /// row: the [`External::begin_drag`](crate::external::External::begin_drag)
    /// hook the router calls on `PointerDown`. The payload carries the dragged
    /// entry's **leaf name** under [`FILE_DRAG_KIND`] (so the in-flight drag is
    /// introspectable as scene-as-data and a future cross-widget target can
    /// match on `kind`); the destination is resolved at drop. `None` when no
    /// row is armed — the press was on the background / the `../` breadcrumb
    /// (which is not a draggable source), so no session starts.
    #[must_use]
    pub fn begin_file_drag(&self) -> Option<DragPayload> {
        let idx = self.pressed.get()?;
        let name = self.entries.get().get(idx)?.name.clone();
        Some(DragPayload {
            kind: Cow::Borrowed(FILE_DRAG_KIND),
            value: IntrospectValue::Text(name),
        })
    }

    /// R794 — the live [`FileDropTarget`] under the cursor during a drag, or
    /// `None`. The view reads this to paint the "drop here" highlight (it
    /// subscribes), and the `External` surfaces it on the `drop_target`
    /// query.
    #[must_use]
    pub fn drop_target(&self) -> Option<FileDropTarget> {
        self.drop_target.get()
    }

    /// R794 §5.51 — live drag update
    /// ([`External::drag_to`](crate::external::External::drag_to)): resolve and
    /// store the [`FileDropTarget`] under the cursor so the view repaints the
    /// highlight. `over` is the router's hit-test of the tag under the absolute
    /// cursor.
    pub fn drag_over(&self, over: Option<&DropPoint>) {
        self.drop_target.set(self.resolve_drop(over));
    }

    /// R794 §5.51 — drop commit
    /// ([`External::drag_release`](crate::external::External::drag_release)):
    /// move the dragged entry into the resolved [`FileDropTarget`] (a folder,
    /// or up to the parent), then clear the transient drag state. Returns
    /// whether a move took effect (`false` for a drop over no valid target,
    /// onto the source itself, or a rejected rename). The AI re-reads
    /// `entries` to observe the new listing.
    #[allow(
        clippy::must_use_candidate,
        reason = "the moved bool is the AI-first drop outcome; the router's \
                  fire-and-forget drag_release ignores it (the navigate / \
                  set_width setter-returns-outcome precedent)"
    )]
    pub fn drag_drop(&self, over: Option<&DropPoint>) -> bool {
        let moved = self.commit_drop(over);
        self.pressed.set(None);
        // Clearing the (reactive) drop highlight repaints the target row back
        // to its resting ink whether or not the move took effect.
        self.drop_target.set(None);
        moved
    }

    /// Resolve the tag under the cursor into a valid [`FileDropTarget`]: a
    /// **directory** row (move into it) or the `../` breadcrumb (move up). A
    /// file row, the dragged source row itself, or the background resolve to
    /// `None` (no valid target). The drop-classification SSOT both
    /// [`drag_over`](Self::drag_over) (preview) and [`commit_drop`](Self::commit_drop)
    /// (apply) read, so the highlighted target and the committed move never
    /// disagree.
    fn resolve_drop(&self, over: Option<&DropPoint>) -> Option<FileDropTarget> {
        let (_, sub) = split_subindex(&over?.tag);
        let sub = sub?;
        if sub == "up" {
            return Some(FileDropTarget::Up);
        }
        let idx: usize = sub.parse().ok()?;
        // Never the dragged source row (dropping onto yourself is inert) and
        // only a directory is a move-into target (a file has no inside).
        if self.pressed.get() == Some(idx) {
            return None;
        }
        self.entries
            .get()
            .get(idx)
            .filter(|e| e.is_dir)
            .map(|_| FileDropTarget::Row(idx))
    }

    /// Apply the move the released drag resolves: the [`pressed`](Self::pressed)
    /// source into the [`resolve_drop`](Self::resolve_drop) destination. `false`
    /// when there is no armed source, no valid target, or the move is rejected.
    fn commit_drop(&self, over: Option<&DropPoint>) -> bool {
        let Some(target) = self.resolve_drop(over) else {
            return false;
        };
        let Some(src_idx) = self.pressed.get() else {
            return false;
        };
        let entries = self.entries.get();
        let Some(source) = entries.get(src_idx) else {
            return false;
        };
        let source_name = source.name.clone();
        let dest_dir = match target {
            FileDropTarget::Row(i) => match entries.get(i) {
                Some(dir_entry) => join_path(&self.cwd.get(), &dir_entry.name),
                None => return false,
            },
            FileDropTarget::Up => parent_path(&self.cwd.get()),
        };
        self.move_into(&source_name, &dest_dir)
    }

    /// Move the entry `source_name` of the current directory into `dest_dir`
    /// (keeping its leaf name), refreshing the listing on success. The widget
    /// layer owns the path arithmetic (the `Directory` trait stays a pure
    /// `rename(from, to)`); this enforces the two file-manager invariants the
    /// backing's blind subtree re-key would otherwise violate: a move into the
    /// current directory is a no-op (the entry is already there), and a
    /// directory is never moved into itself or its own subtree. The selection
    /// follows the entry out of the listing (it cleared when it was the moved
    /// item). Returns whether the move took effect.
    fn move_into(&self, source_name: &str, dest_dir: &str) -> bool {
        let cwd = self.cwd.get();
        if dest_dir == cwd {
            return false;
        }
        let from = join_path(&cwd, source_name);
        // Refuse moving a directory into itself or any descendant of it.
        if dest_dir == from || dest_dir.starts_with(&format!("{from}/")) {
            return false;
        }
        let to = join_path(dest_dir, source_name);
        let ok = self.dir.rename(&from, &to);
        if ok {
            crate::reactive::batch(|| {
                if self.selected.get().as_deref() == Some(from.as_str()) {
                    self.selected.set(None);
                }
                self.refresh();
            });
        }
        ok
    }
}

/// R787 — resolve the shared [`DirectoryState`] for `key`, building it
/// once via the `dir` + `initial` factories. Mirrors
/// [`use_column_widths`](crate::widgets::column_widths::use_column_widths):
/// the `External` and the view both call this with the same `key` and
/// receive the same `Rc`, so the navigation state is one source of truth.
///
/// # Panics
///
/// Panics if no current [`Owner`] is set — call from within a `view` /
/// `create_extra_externals` hook (both run inside a `root_owner.run`).
#[must_use]
pub fn use_directory_state(
    key: &'static str,
    dir: impl FnOnce() -> Rc<dyn Directory>,
    initial: impl FnOnce() -> String,
) -> Rc<DirectoryState> {
    Owner::current()
        .expect("use_directory_state requires an active Owner scope")
        .cache(key, || DirectoryState::new(dir(), initial()))
}

/// R792 §5.15 §5.40 — drive **keyboard navigation over a file list** backed
/// by a [`DirectoryState`] at `tag` and a flex-viewport [`ScrollState`].
///
/// The `WidgetCore::apply_key` body every own-rendered file UI shares to make
/// its entry list keyboard-operable: `ArrowUp` / `ArrowDown`, `Home` / `End`,
/// `PageUp` / `PageDown` move the roving cursor (the shell rings the active
/// row via `aria-activedescendant`), scrolling a never-materialized row into
/// view; `Enter` activates the cursor (navigate into a folder / pick a file).
/// Keys only route when `focused == Some(tag)` (single tab stop, no sibling
/// aliasing).
///
/// This is the `DirectoryState` peer of
/// [`nav_select_key`](crate::widgets::virtual_select::nav_select_key): both
/// reuse the shared [`clamp_nav`](crate::widgets::virtual_select::clamp_nav)
/// key→index policy and the [`page_rows`](crate::widgets::virtual_list::page_rows)
/// / [`reveal_row`](crate::widgets::virtual_list::reveal_row) viewport
/// helpers, but the state owner and
/// activation semantics differ — a `VirtualSelectExternal` *selects* the
/// navigated index (selection-follows-focus), whereas a file list's `Enter`
/// *navigates or picks* and the cursor is orthogonal to the picked leaf. So
/// it is a peer, not a fold (the R778 algorithm-peer rule): the byte-shared
/// part is the policy fn, not the whole controller.
///
/// Returns `true` when the key was handled (the list was focused and the key
/// is a navigation / activation key), `false` otherwise — the exact bool
/// `apply_key` must return.
#[must_use]
pub fn dir_nav_key(
    dir: &DirectoryState,
    scroll: &ScrollState,
    focused: Option<&str>,
    tag: &str,
    key: &str,
    row_pitch: u32,
) -> bool {
    dir_nav_key_inner(dir, scroll, focused, tag, key, row_pitch, false)
}

/// R802 §5.15 §5.40 — **selection-follows-focus** variant of
/// [`dir_nav_key`] for a single-select picker (the file-open dialog). Same
/// keyboard model (`ArrowUp` / `ArrowDown` / `Home` / `End` / `PageUp` /
/// `PageDown` move the roving cursor, `Enter` activates), but each cursor
/// move also syncs the file selection to the focused row
/// ([`move_cursor_selecting`](DirectoryState::move_cursor_selecting)): a file
/// row becomes the selected leaf (so an OK gate enables as the user arrows),
/// a directory row clears the selection. The shared nav body is identical —
/// this is the W3C "selection follows focus" listbox policy the picker opts
/// into, where [`dir_nav_key`] is the Files / Explorer model (cursor
/// orthogonal to the picked leaf).
///
/// Returns `true` when the key was handled, `false` otherwise.
#[must_use]
pub fn dir_nav_key_selecting(
    dir: &DirectoryState,
    scroll: &ScrollState,
    focused: Option<&str>,
    tag: &str,
    key: &str,
    row_pitch: u32,
) -> bool {
    dir_nav_key_inner(dir, scroll, focused, tag, key, row_pitch, true)
}

/// Shared body for [`dir_nav_key`] (Files model) and
/// [`dir_nav_key_selecting`] (picker model). The single divergence is whether
/// a cursor move also syncs the file selection (`selection_follows_focus`).
fn dir_nav_key_inner(
    dir: &DirectoryState,
    scroll: &ScrollState,
    focused: Option<&str>,
    tag: &str,
    key: &str,
    row_pitch: u32,
    selection_follows_focus: bool,
) -> bool {
    if focused != Some(tag) {
        return false;
    }
    // `Enter` activates the cursor row (navigate a folder / pick a file).
    if key == "Enter" {
        if dir.cursor().is_none() {
            return false;
        }
        dir.activate_cursor();
        return true;
    }
    let page = crate::widgets::virtual_list::page_rows(scroll, row_pitch);
    let moved = if selection_follows_focus {
        dir.move_cursor_selecting(key, page)
    } else {
        dir.move_cursor(key, page)
    };
    if !moved {
        return false;
    }
    // Scroll the navigated row into view (a cursor on a never-materialized
    // row scrolls there — the same `reveal_row` glue `nav_select_key` uses).
    if let Some(target) = dir.cursor() {
        crate::widgets::virtual_list::reveal_row(scroll, target, row_pitch);
    }
    true
}

/// Wire form of one listing: newline-joined entry names, directories
/// suffixed `'/'` (the `ls -p` convention) so the AI-readable `entries`
/// query distinguishes navigable children from selectable leaves in one
/// glance.
fn entries_wire(entries: &[DirEntry]) -> String {
    entries
        .iter()
        .map(|e| {
            if e.is_dir {
                format!("{}/", e.name)
            } else {
                e.name.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// R787 §5.15 §5.16 — `External` adapter over a shared [`DirectoryState`],
/// the AI-first browse + select surface for the file browser. A config
/// holder (no §5.20 intent): a navigation `Signal` write already repaints
/// every subscribed view, mirroring
/// [`ColumnWidthExternal`](crate::widgets::column_widths::ColumnWidthExternal).
#[derive(Debug)]
pub struct DirectoryExternal {
    state: Rc<DirectoryState>,
}

impl DirectoryExternal {
    /// Wrap the shared [`DirectoryState`] (from [`use_directory_state`]).
    #[must_use]
    pub fn new(state: Rc<DirectoryState>) -> Self {
        Self { state }
    }

    /// The shared state handle (the view reaches the same `Rc`).
    #[must_use]
    pub fn state(&self) -> &Rc<DirectoryState> {
        &self.state
    }
}

impl External for DirectoryExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }

    // R794 §5.51 — drag-to-move. The router calls these on the source
    // coordinator (this `External`, resolved from the pressed row's primary
    // tag): `begin_drag` arms from the `PointerDown`-pressed row, `drag_to`
    // tracks the folder under the cursor, `drag_release` commits the move.
    // Every drop candidate (every row + the `../` breadcrumb) belongs to this
    // one coordinator, so the source is the resolver — the R742 contract.
    fn begin_drag(&self) -> Option<DragPayload> {
        self.state.begin_file_drag()
    }

    fn drag_to(&mut self, _payload: &DragPayload, over: Option<DropPoint>) {
        self.state.drag_over(over.as_ref());
    }

    fn drag_release(&mut self, _payload: &DragPayload, over: Option<DropPoint>) {
        self.state.drag_drop(over.as_ref());
    }
}

impl ExternalIntrospect for DirectoryExternal {
    fn schema(&self) -> IntrospectSchema {
        // cwd        — current directory path (query; intervene = open_dir).
        // count      — entry count (query).
        // entries    — newline-joined names, dirs suffixed '/' (query).
        // selected   — selected leaf's full path, or Null (query).
        // name.<i>   — entry i's leaf name (query).
        // is_dir.<i> — whether entry i is a directory (query).
        // navigate   — "<name>" invoke: into a child dir; returns the cwd.
        // up         — invoke: to the parent; returns the cwd.
        // select     — "<name>" invoke: select a leaf; returns the selection.
        // open       — "<path>" invoke: absolute jump; returns the cwd.
        IntrospectSchema::new(&[
            ("cwd", "string"),
            ("count", "int"),
            ("entries", "string"),
            ("selected", "string"),
            // R792 — the roving keyboard cursor's row (query; intervene =
            // admin set, the aria-activedescendant write peer). The effective
            // active row: the first row when unmoved, Null only when empty.
            ("cursor", "int"),
            ("name", "string"),
            ("is_dir", "bool"),
            ("navigate", "string"),
            ("up", "string"),
            ("select", "string"),
            ("open", "string"),
            ("send", "string"),
            // R789 write surface (file-manager mutations).
            ("mkdir", "bool"),
            ("touch", "bool"),
            ("delete", "bool"),
            // R791 — rename the selection to the string arg.
            ("rename", "bool"),
            // R794 — the in-flight drag-to-move state (§2 #7 — the live
            // drag is scene-as-data). `dragging` (bool) is whether a row is
            // armed; `drop_target` (query) is the resolved target under the
            // cursor: Text "up" (the parent breadcrumb), Text "row:<i>" (a
            // folder row), or Null (no valid target).
            ("dragging", "bool"),
            ("drop_target", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        // Indexed entry fields: `name.<i>` / `is_dir.<i>`.
        if let Some(rest) = path.strip_prefix("name.") {
            let i: usize = rest.parse().ok()?;
            return self
                .state
                .entries()
                .get(i)
                .map(|e| IntrospectValue::Text(e.name.clone()));
        }
        if let Some(rest) = path.strip_prefix("is_dir.") {
            let i: usize = rest.parse().ok()?;
            return self
                .state
                .entries()
                .get(i)
                .map(|e| IntrospectValue::Bool(e.is_dir));
        }
        match path {
            "cwd" => Some(IntrospectValue::Text(self.state.cwd())),
            "count" => Some(IntrospectValue::Int(
                i64::try_from(self.state.count()).unwrap_or(i64::MAX),
            )),
            "entries" => Some(IntrospectValue::Text(entries_wire(&self.state.entries()))),
            "selected" => Some(match self.state.selected() {
                Some(p) => IntrospectValue::Text(p),
                None => IntrospectValue::Null,
            }),
            // R792 — the effective keyboard-cursor row (Null only when empty).
            "cursor" => Some(
                self.state
                    .cursor()
                    .and_then(|i| i64::try_from(i).ok())
                    .map_or(IntrospectValue::Null, IntrospectValue::Int),
            ),
            // R794 — the in-flight drag-to-move state.
            "dragging" => Some(IntrospectValue::Bool(self.state.pressed().is_some())),
            "drop_target" => Some(match self.state.drop_target() {
                Some(FileDropTarget::Row(i)) => IntrospectValue::Text(format!("row:{i}")),
                Some(FileDropTarget::Up) => IntrospectValue::Text("up".into()),
                None => IntrospectValue::Null,
            }),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // Admin absolute navigation (breadcrumb / restore): set cwd.
            "cwd" => match value {
                IntrospectValue::Text(p) => {
                    self.state.open_dir(p);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            // R792 — admin set the keyboard cursor (restore / AT
            // active-descendant write). `Int` sets the row (out-of-range
            // clamps to the last); `Null` resets to the first row.
            "cursor" => match value {
                IntrospectValue::Int(i) => {
                    self.state.set_cursor(usize::try_from(i).ok());
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.state.set_cursor(None);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "count" | "entries" | "selected" | "name" | "is_dir" | "dragging" | "drop_target" => {
                Err(InterveneError::ReadOnly)
            }
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // R787 — pointer / keyboard activation of a painted row, the
            // composite-tag convention the InputRouter routes here: a row
            // tagged "<dir_tag>#<idx>" sends "<idx>:PointerUp", the "up"
            // affordance ("<dir_tag>#up") sends "up:PointerUp". Acts only on
            // the activation edge (PointerUp / KeyboardActivate, the R778
            // `is_activation_event` SSOT), so hover / press / leave are inert.
            "send" => {
                let raw = args.as_str().ok_or(InvokeError::TypeMismatch)?;
                // R880.1 — the `split_send_payload` `:` grammar SSOT strips
                // a held-modifier third segment ("3:PointerUp:c" activated
                // nothing under the hand-rolled split_once). A bare payload
                // (no `:`) keeps the pre-R880.1 inert shape.
                let (sub, event, _mods) = crate::composite_tag::split_send_payload(raw)
                    .unwrap_or((raw, "", crate::input::Modifiers::empty()));
                // R794 — a `PointerDown` on a row arms a drag from it (the
                // source the router's `begin_drag` reads). Only numeric row
                // subs arm; the "up" breadcrumb is not a draggable source.
                if event == PointerWireEvent::Down.as_wire_name() {
                    if let Ok(idx) = sub.parse::<usize>() {
                        self.state.press(idx);
                    }
                    return Ok(IntrospectValue::Null);
                }
                if !is_activation_event(event) {
                    return Ok(IntrospectValue::Null);
                }
                // R794 — click vs drag is the router's job: after a *moved*
                // drag it does not dispatch this trailing `PointerUp` at all
                // (DRAG_CLICK_THRESHOLD_PX), so a row activation here is always
                // a genuine click. A press-release in place still arrives and
                // navigates / selects.
                if sub == "up" {
                    self.state.up();
                } else if let Ok(idx) = sub.parse::<usize>() {
                    self.state.activate_index(idx);
                }
                Ok(IntrospectValue::Null)
            }
            "navigate" => {
                let name = args.as_str().ok_or(InvokeError::TypeMismatch)?;
                Ok(IntrospectValue::Text(self.state.navigate(name)))
            }
            "up" => Ok(IntrospectValue::Text(self.state.up())),
            "select" => {
                let name = args.as_str().ok_or(InvokeError::TypeMismatch)?;
                Ok(match self.state.select(name) {
                    Some(p) => IntrospectValue::Text(p),
                    None => IntrospectValue::Null,
                })
            }
            "open" => {
                let path = args.as_str().ok_or(InvokeError::TypeMismatch)?;
                Ok(IntrospectValue::Text(self.state.open_dir(path)))
            }
            // R789 write surface — create/delete in the current directory.
            // Each returns whether the mutation took effect (the AI re-reads
            // `entries` / `count` to observe the new listing).
            "mkdir" => {
                let name = args.as_str().ok_or(InvokeError::TypeMismatch)?;
                Ok(IntrospectValue::Bool(self.state.create_dir(name)))
            }
            "touch" => {
                let name = args.as_str().ok_or(InvokeError::TypeMismatch)?;
                Ok(IntrospectValue::Bool(self.state.create_file(name)))
            }
            "delete" => Ok(IntrospectValue::Bool(self.state.delete_selected())),
            // R791 — rename the selection to the string arg (the selection
            // follows to the new path on success).
            "rename" => {
                let name = args.as_str().ok_or(InvokeError::TypeMismatch)?;
                Ok(IntrospectValue::Bool(self.state.rename_selected(name)))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directory::InMemoryDirectory;

    fn sample() -> Rc<dyn Directory> {
        let d = InMemoryDirectory::new();
        d.insert(
            "/proj",
            vec![
                DirEntry::dir("src"),
                DirEntry::file("Cargo.toml"),
                DirEntry::file("README.md"),
            ],
        );
        d.insert(
            "/proj/src",
            vec![DirEntry::file("main.rs"), DirEntry::file("lib.rs")],
        );
        Rc::new(d)
    }

    fn state() -> DirectoryState {
        DirectoryState::new(sample(), "/proj")
    }

    #[test]
    fn r787_join_and_parent_path() {
        assert_eq!(join_path("/", "a"), "/a");
        assert_eq!(join_path("/proj", "src"), "/proj/src");
        assert_eq!(parent_path("/proj/src"), "/proj");
        assert_eq!(parent_path("/proj"), "/");
        assert_eq!(parent_path("/"), "/", "root is its own parent");
        assert_eq!(parent_path("/proj/src/"), "/proj", "trailing slash ignored");
    }

    #[test]
    fn r787_initial_listing_sorted_dirs_first() {
        let s = state();
        assert_eq!(s.cwd(), "/proj");
        let names: Vec<String> = s.entries().iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, ["src", "Cargo.toml", "README.md"]);
        assert_eq!(s.selected(), None);
    }

    #[test]
    fn r787_navigate_into_dir_then_up() {
        let s = state();
        assert_eq!(s.navigate("src"), "/proj/src", "navigate into a child dir");
        let names: Vec<String> = s.entries().iter().map(|e| e.name.clone()).collect();
        assert_eq!(
            names,
            ["lib.rs", "main.rs"],
            "listing refreshed to child dir"
        );
        assert_eq!(s.up(), "/proj", "up returns to the parent");
        assert_eq!(s.entries().len(), 3, "parent listing restored");
    }

    #[test]
    fn r787_navigate_to_file_is_noop() {
        let s = state();
        assert_eq!(
            s.navigate("README.md"),
            "/proj",
            "navigate to a file is a no-op"
        );
        assert_eq!(
            s.navigate("nope"),
            "/proj",
            "navigate to a missing entry is a no-op"
        );
    }

    #[test]
    fn r787_select_sets_full_path_and_clears_on_navigate() {
        let s = state();
        assert_eq!(s.select("README.md"), Some("/proj/README.md".to_string()));
        assert_eq!(s.selected(), Some("/proj/README.md".to_string()));
        // Navigating clears the selection (a new directory context).
        s.navigate("src");
        assert_eq!(s.selected(), None, "navigation clears the selection");
        // Selecting a missing entry is a no-op.
        assert_eq!(s.select("ghost"), None);
    }

    #[test]
    fn r787_up_at_root_is_noop() {
        let d = InMemoryDirectory::new();
        d.insert("/", vec![DirEntry::dir("proj")]);
        let s = DirectoryState::new(Rc::new(d), "/");
        assert_eq!(s.up(), "/", "up at the root stays at the root");
    }

    #[test]
    fn r787_external_query_surface() {
        let st = Rc::new(state());
        let ext = DirectoryExternal::new(Rc::clone(&st));
        assert_eq!(
            ext.query("cwd"),
            Some(IntrospectValue::Text("/proj".into()))
        );
        assert_eq!(ext.query("count"), Some(IntrospectValue::Int(3)));
        assert_eq!(
            ext.query("entries"),
            Some(IntrospectValue::Text("src/\nCargo.toml\nREADME.md".into())),
            "dirs suffixed '/' in the wire listing",
        );
        assert_eq!(
            ext.query("name.0"),
            Some(IntrospectValue::Text("src".into()))
        );
        assert_eq!(ext.query("is_dir.0"), Some(IntrospectValue::Bool(true)));
        assert_eq!(ext.query("is_dir.1"), Some(IntrospectValue::Bool(false)));
        assert_eq!(ext.query("selected"), Some(IntrospectValue::Null));
        assert_eq!(ext.query("name.9"), None, "out-of-range index = None");
    }

    #[test]
    fn r787_external_invoke_navigate_select_up_open() {
        let st = Rc::new(state());
        let mut ext = DirectoryExternal::new(Rc::clone(&st));
        assert_eq!(
            ext.invoke("navigate", IntrospectValue::Text("src".into()))
                .unwrap(),
            IntrospectValue::Text("/proj/src".into()),
        );
        assert_eq!(st.cwd(), "/proj/src");
        assert_eq!(
            ext.invoke("select", IntrospectValue::Text("main.rs".into()))
                .unwrap(),
            IntrospectValue::Text("/proj/src/main.rs".into()),
        );
        assert_eq!(
            ext.invoke("up", IntrospectValue::Null).unwrap(),
            IntrospectValue::Text("/proj".into())
        );
        assert_eq!(st.selected(), None, "up cleared the selection");
        assert_eq!(
            ext.invoke("open", IntrospectValue::Text("/proj/src".into()))
                .unwrap(),
            IntrospectValue::Text("/proj/src".into()),
        );
        assert_eq!(
            ext.invoke("nope", IntrospectValue::Null),
            Err(InvokeError::UnknownPath),
        );
    }

    #[test]
    fn r787_activate_index_navigates_dirs_selects_files() {
        let s = state(); // entries: [src(dir), Cargo.toml, README.md]
        s.activate_index(0); // src → navigate
        assert_eq!(s.cwd(), "/proj/src");
        s.up();
        s.activate_index(1); // Cargo.toml → select
        assert_eq!(s.selected(), Some("/proj/Cargo.toml".to_string()));
        s.activate_index(99); // out of range → no-op
        assert_eq!(s.selected(), Some("/proj/Cargo.toml".to_string()));
    }

    #[test]
    fn r787_external_send_row_click_and_up() {
        let st = Rc::new(state());
        let mut ext = DirectoryExternal::new(Rc::clone(&st));
        // Row 0 (src) PointerUp → navigate.
        ext.invoke("send", IntrospectValue::Text("0:PointerUp".into()))
            .unwrap();
        assert_eq!(st.cwd(), "/proj/src");
        // "up" affordance → parent.
        ext.invoke("send", IntrospectValue::Text("up:PointerUp".into()))
            .unwrap();
        assert_eq!(st.cwd(), "/proj");
        // Row 2 (README.md) PointerUp → select.
        ext.invoke("send", IntrospectValue::Text("2:PointerUp".into()))
            .unwrap();
        assert_eq!(st.selected(), Some("/proj/README.md".to_string()));
        // Non-activation edge (hover) is inert.
        let before = st.cwd();
        ext.invoke("send", IntrospectValue::Text("0:PointerEnter".into()))
            .unwrap();
        assert_eq!(st.cwd(), before, "hover does not navigate");
    }

    #[test]
    fn r880_1_row_click_with_modifier_segment_still_activates() {
        // "0:PointerUp:c" (the R781 modifier segment) must still navigate —
        // the pre-R880.1 hand-rolled split read "PointerUp:c" as the event
        // name and the activation test silently failed.
        let st = Rc::new(state());
        let mut ext = DirectoryExternal::new(Rc::clone(&st));
        ext.invoke("send", IntrospectValue::Text("0:PointerUp:c".into()))
            .unwrap();
        assert_eq!(st.cwd(), "/proj/src", "Ctrl+click still navigates");
    }

    #[test]
    fn r789_1_selected_index_maps_path_to_row() {
        let s = state(); // /proj: [src(dir), Cargo.toml, README.md]
        assert_eq!(s.selected_index(), None, "no selection = None");
        s.select("README.md");
        assert_eq!(s.selected_index(), Some(2), "README.md is row 2");
        s.navigate("src");
        assert_eq!(s.selected_index(), None, "navigation cleared the selection");
    }

    #[test]
    fn r791_1_selected_name_is_the_leaf_of_the_selection() {
        let s = state(); // /proj: [src(dir), Cargo.toml, README.md]
        assert_eq!(s.selected_name(), None, "no selection = None");
        s.select("README.md");
        assert_eq!(
            s.selected_name(),
            Some("README.md".to_string()),
            "leaf of /proj/README.md"
        );
        s.navigate("src");
        s.select("main.rs");
        assert_eq!(
            s.selected_name(),
            Some("main.rs".to_string()),
            "leaf in a nested cwd"
        );
    }

    #[test]
    fn r789_state_create_dir_file_and_delete_selected() {
        let s = state(); // /proj: [src(dir), Cargo.toml, README.md]
        assert!(s.create_dir("assets"), "new dir created in cwd");
        assert!(
            s.entries().iter().any(|e| e.name == "assets" && e.is_dir),
            "assets listed"
        );
        assert!(s.create_file("notes.txt"), "new file created in cwd");
        assert!(
            s.entries()
                .iter()
                .any(|e| e.name == "notes.txt" && !e.is_dir)
        );
        assert!(!s.create_dir("src"), "duplicate name rejected");
        // delete_selected removes the picked entry + clears the selection.
        s.select("Cargo.toml");
        assert!(s.delete_selected(), "selected entry removed");
        assert_eq!(s.selected(), None, "selection cleared after delete");
        assert!(
            !s.entries().iter().any(|e| e.name == "Cargo.toml"),
            "Cargo.toml gone"
        );
        assert!(
            !s.delete_selected(),
            "delete with no selection is a false no-op"
        );
    }

    #[test]
    fn r789_external_write_invoke_surface() {
        let st = Rc::new(state());
        let mut ext = DirectoryExternal::new(Rc::clone(&st));
        assert_eq!(
            ext.invoke("mkdir", IntrospectValue::Text("assets".into()))
                .unwrap(),
            IntrospectValue::Bool(true),
        );
        assert_eq!(st.count(), 4, "mkdir grew the listing");
        assert_eq!(
            ext.invoke("touch", IntrospectValue::Text("LICENSE".into()))
                .unwrap(),
            IntrospectValue::Bool(true),
        );
        // delete operates on the selection.
        ext.invoke("select", IntrospectValue::Text("README.md".into()))
            .unwrap();
        assert_eq!(
            ext.invoke("delete", IntrospectValue::Null).unwrap(),
            IntrospectValue::Bool(true)
        );
        assert_eq!(st.selected(), None, "delete cleared the selection");
        assert!(!st.entries().iter().any(|e| e.name == "README.md"));
        // duplicate mkdir reports false (not an error).
        assert_eq!(
            ext.invoke("mkdir", IntrospectValue::Text("src".into()))
                .unwrap(),
            IntrospectValue::Bool(false),
        );
    }

    #[test]
    fn r791_state_rename_selected_follows_selection() {
        let s = state(); // /proj: [src(dir), Cargo.toml, README.md]
        // No selection → no-op.
        assert!(
            !s.rename_selected("x"),
            "rename with no selection is a false no-op"
        );
        s.select("README.md");
        assert!(s.rename_selected("NOTES.md"), "selected file renamed");
        assert!(
            s.entries().iter().any(|e| e.name == "NOTES.md"),
            "new name listed"
        );
        assert!(
            !s.entries().iter().any(|e| e.name == "README.md"),
            "old name gone"
        );
        assert_eq!(
            s.selected(),
            Some("/proj/NOTES.md".to_string()),
            "selection follows the rename"
        );
        // Blank name + taken name are rejected (selection unchanged).
        assert!(!s.rename_selected("   "), "blank name rejected");
        assert!(!s.rename_selected("Cargo.toml"), "taken name rejected");
        assert_eq!(
            s.selected(),
            Some("/proj/NOTES.md".to_string()),
            "rejected rename left selection"
        );
    }

    #[test]
    fn r791_external_rename_invoke() {
        let st = Rc::new(state());
        let mut ext = DirectoryExternal::new(Rc::clone(&st));
        ext.invoke("select", IntrospectValue::Text("Cargo.toml".into()))
            .unwrap();
        assert_eq!(
            ext.invoke("rename", IntrospectValue::Text("Cargo.lock".into()))
                .unwrap(),
            IntrospectValue::Bool(true),
        );
        assert!(
            st.entries().iter().any(|e| e.name == "Cargo.lock"),
            "renamed via invoke"
        );
        assert_eq!(
            st.selected(),
            Some("/proj/Cargo.lock".to_string()),
            "selection followed"
        );
        // rename with no selection cleared → false; a non-string arg errors.
        ext.invoke("open", IntrospectValue::Text("/proj".into()))
            .unwrap(); // clears selection
        assert_eq!(
            ext.invoke("rename", IntrospectValue::Text("x".into()))
                .unwrap(),
            IntrospectValue::Bool(false),
            "rename with no selection reports false",
        );
        assert_eq!(
            ext.invoke("rename", IntrospectValue::Null),
            Err(InvokeError::TypeMismatch)
        );
    }

    // ── R792 keyboard cursor ────────────────────────────────────────

    #[test]
    fn r792_cursor_defaults_to_first_row_and_empty_is_none() {
        let s = state(); // /proj: [src, Cargo.toml, README.md]
        // Unmoved → the effective cursor is the first row.
        assert_eq!(
            s.cursor(),
            Some(0),
            "unmoved cursor defaults to the first row"
        );
        // An empty listing has no cursor.
        let d = InMemoryDirectory::new();
        d.insert("/empty", vec![]);
        let empty = DirectoryState::new(Rc::new(d), "/empty");
        assert_eq!(empty.cursor(), None, "empty listing has no cursor");
    }

    #[test]
    fn r792_move_cursor_clamps_and_handles_keys() {
        let s = state(); // 3 entries
        assert!(s.move_cursor("ArrowDown", 5), "ArrowDown handled");
        assert_eq!(s.cursor(), Some(1), "ArrowDown from row 0 → row 1");
        assert!(s.move_cursor("ArrowDown", 5));
        assert_eq!(s.cursor(), Some(2));
        assert!(s.move_cursor("ArrowDown", 5));
        assert_eq!(
            s.cursor(),
            Some(2),
            "ArrowDown at the last row clamps (no wrap)"
        );
        assert!(s.move_cursor("Home", 5));
        assert_eq!(s.cursor(), Some(0), "Home jumps to the first row");
        assert!(s.move_cursor("End", 5));
        assert_eq!(s.cursor(), Some(2), "End jumps to the last row");
        assert!(
            !s.move_cursor("Tab", 5),
            "an unhandled key is a false no-op"
        );
        assert_eq!(
            s.cursor(),
            Some(2),
            "an unhandled key leaves the cursor put"
        );
    }

    #[test]
    fn r792_cursor_resets_on_directory_change() {
        let s = state();
        s.move_cursor("End", 5);
        assert_eq!(s.cursor(), Some(2), "cursor moved to the last row");
        s.navigate("src"); // /proj/src: [lib.rs, main.rs]
        assert_eq!(
            s.cursor(),
            Some(0),
            "navigation resets the cursor to the new dir's first row"
        );
        s.move_cursor("ArrowDown", 5);
        assert_eq!(s.cursor(), Some(1));
        s.up();
        assert_eq!(s.cursor(), Some(0), "up resets the cursor too");
    }

    #[test]
    fn r792_activate_cursor_navigates_dirs_selects_files() {
        let s = state(); // [src(dir), Cargo.toml, README.md]
        // Cursor on row 0 (src) → Enter navigates.
        s.activate_cursor();
        assert_eq!(s.cwd(), "/proj/src", "activate on a dir navigates into it");
        s.up();
        // Cursor on row 2 (README.md) → Enter selects.
        s.move_cursor("End", 5);
        s.activate_cursor();
        assert_eq!(
            s.selected(),
            Some("/proj/README.md".to_string()),
            "activate on a file selects it"
        );
    }

    #[test]
    fn r792_set_cursor_admin_clamps_and_clears() {
        let s = state(); // 3 entries
        s.set_cursor(Some(1));
        assert_eq!(s.cursor(), Some(1));
        s.set_cursor(Some(99));
        assert_eq!(s.cursor(), Some(2), "out-of-range clamps to the last row");
        s.set_cursor(None);
        assert_eq!(
            s.cursor(),
            Some(0),
            "None resets to the effective first row"
        );
    }

    #[test]
    fn r792_dir_nav_key_gated_on_focus_and_drives_cursor() {
        use crate::widgets::scroll::ScrollState;
        let s = state();
        let scroll = ScrollState::new();
        scroll.set_measured_viewport(300, 160);
        // Unfocused → ignored.
        assert!(!dir_nav_key(
            &s,
            &scroll,
            Some("other"),
            "fb",
            "ArrowDown",
            34
        ));
        assert_eq!(s.cursor(), Some(0), "unfocused key did not move the cursor");
        // Focused → ArrowDown advances.
        assert!(dir_nav_key(&s, &scroll, Some("fb"), "fb", "ArrowDown", 34));
        assert_eq!(s.cursor(), Some(1));
        // Enter activates the cursor row (row 1 = Cargo.toml → select).
        assert!(dir_nav_key(&s, &scroll, Some("fb"), "fb", "Enter", 34));
        assert_eq!(
            s.selected(),
            Some("/proj/Cargo.toml".to_string()),
            "Enter picked the cursor file"
        );
        // A non-nav key is a false no-op.
        assert!(!dir_nav_key(&s, &scroll, Some("fb"), "fb", "Tab", 34));
    }

    #[test]
    fn r792_dir_nav_key_scrolls_a_deep_cursor_into_view() {
        use crate::widgets::scroll::ScrollState;
        // A listing taller than the viewport so End scrolls.
        let d = InMemoryDirectory::new();
        let files: Vec<DirEntry> = (0..40)
            .map(|i| DirEntry::file(format!("f{i}.txt")))
            .collect();
        d.insert("/big", files);
        let s = DirectoryState::new(Rc::new(d), "/big");
        let scroll = ScrollState::new();
        scroll.set_max(0, 40 * 34);
        scroll.set_measured_viewport(300, 160);
        assert!(dir_nav_key(&s, &scroll, Some("fb"), "fb", "End", 34));
        assert_eq!(s.cursor(), Some(39), "End moves to the last row");
        assert!(
            scroll.offset_y() > 0,
            "the deep cursor scrolled into view, offset {}",
            scroll.offset_y()
        );
        assert!(dir_nav_key(&s, &scroll, Some("fb"), "fb", "Home", 34));
        assert_eq!(s.cursor(), Some(0));
        assert_eq!(scroll.offset_y(), 0, "Home scrolled back to the top");
    }

    // ── R802 selection-follows-focus (single-select picker) ─────────

    #[test]
    fn r802_move_cursor_selecting_follows_focus_over_files_and_dirs() {
        let s = state(); // /proj: row0=src(dir), row1=Cargo.toml, row2=README.md
        assert_eq!(s.selected(), None, "no selection on construction");
        // ArrowDown lands on Cargo.toml (a file) → it becomes selected.
        assert!(s.move_cursor_selecting("ArrowDown", 5), "ArrowDown handled");
        assert_eq!(s.cursor(), Some(1));
        assert_eq!(
            s.selected(),
            Some("/proj/Cargo.toml".to_string()),
            "file row selection follows focus"
        );
        // ArrowDown to README.md (a file) → selection follows.
        assert!(s.move_cursor_selecting("ArrowDown", 5));
        assert_eq!(
            s.selected(),
            Some("/proj/README.md".to_string()),
            "selection tracks the focused file"
        );
        // Home lands on src (a directory) → selection clears (dirs are not picks).
        assert!(s.move_cursor_selecting("Home", 5));
        assert_eq!(s.cursor(), Some(0));
        assert_eq!(
            s.selected(),
            None,
            "a directory row under the cursor clears the selection"
        );
        // Plain move_cursor (the Files model) leaves the selection orthogonal.
        s.move_cursor_selecting("End", 5); // select README.md again
        assert_eq!(s.selected(), Some("/proj/README.md".to_string()));
        assert!(
            s.move_cursor("Home", 5),
            "plain move_cursor moves the cursor"
        );
        assert_eq!(s.cursor(), Some(0), "cursor moved to the dir row");
        assert_eq!(
            s.selected(),
            Some("/proj/README.md".to_string()),
            "plain move_cursor does not touch selection"
        );
    }

    #[test]
    fn r802_dir_nav_key_selecting_drives_the_picker_selection() {
        use crate::widgets::scroll::ScrollState;
        let s = state();
        let scroll = ScrollState::new();
        scroll.set_measured_viewport(300, 160);
        // Unfocused → ignored, selection untouched.
        assert!(!dir_nav_key_selecting(
            &s,
            &scroll,
            Some("other"),
            "fb",
            "ArrowDown",
            34
        ));
        assert_eq!(s.selected(), None, "unfocused key selects nothing");
        // Focused ArrowDown → row 1 (Cargo.toml) selected (the OK gate would enable).
        assert!(dir_nav_key_selecting(
            &s,
            &scroll,
            Some("fb"),
            "fb",
            "ArrowDown",
            34
        ));
        assert_eq!(
            s.selected(),
            Some("/proj/Cargo.toml".to_string()),
            "selection follows the focused file"
        );
        // Home → row 0 (src, a dir) → selection clears (the OK gate would disable).
        assert!(dir_nav_key_selecting(
            &s,
            &scroll,
            Some("fb"),
            "fb",
            "Home",
            34
        ));
        assert_eq!(
            s.selected(),
            None,
            "focusing a directory row clears the picker selection"
        );
    }

    #[test]
    fn r792_external_cursor_query_and_intervene() {
        let st = Rc::new(state());
        let mut ext = DirectoryExternal::new(Rc::clone(&st));
        // Effective cursor defaults to the first row.
        assert_eq!(ext.query("cursor"), Some(IntrospectValue::Int(0)));
        st.move_cursor("End", 5);
        assert_eq!(
            ext.query("cursor"),
            Some(IntrospectValue::Int(2)),
            "query tracks the moved cursor"
        );
        // Admin set via intervene.
        ext.intervene("cursor", IntrospectValue::Int(1)).unwrap();
        assert_eq!(st.cursor(), Some(1));
        ext.intervene("cursor", IntrospectValue::Null).unwrap();
        assert_eq!(st.cursor(), Some(0), "Null resets to the first row");
        assert_eq!(
            ext.intervene("cursor", IntrospectValue::Bool(true)),
            Err(InterveneError::TypeMismatch),
        );
    }

    #[test]
    fn r787_external_intervene_cwd_is_admin_navigation() {
        let st = Rc::new(state());
        let mut ext = DirectoryExternal::new(Rc::clone(&st));
        ext.intervene("cwd", IntrospectValue::Text("/proj/src".into()))
            .unwrap();
        assert_eq!(st.cwd(), "/proj/src", "intervene cwd jumps absolutely");
        assert_eq!(
            ext.intervene("count", IntrospectValue::Int(0)),
            Err(InterveneError::ReadOnly)
        );
        assert_eq!(
            ext.intervene("nope", IntrospectValue::Null),
            Err(InterveneError::UnknownPath)
        );
    }

    // ── R794 drag-to-move ───────────────────────────────────────────

    fn drop_at(tag: &str) -> DropPoint {
        DropPoint {
            tag: tag.to_string(),
            x_rel: 0.5,
            y_rel: 0.5,
        }
    }

    #[test]
    fn r794_press_arms_drag_and_builds_payload() {
        let s = state(); // [src(dir), Cargo.toml, README.md]
        assert_eq!(s.begin_file_drag(), None, "nothing armed → no drag starts");
        s.press(2); // README.md
        assert_eq!(s.pressed(), Some(2));
        let payload = s.begin_file_drag().expect("an armed row drags");
        assert_eq!(
            payload.kind, FILE_DRAG_KIND,
            "payload carries the file-row kind"
        );
        assert_eq!(
            payload.value,
            IntrospectValue::Text("README.md".into()),
            "and the leaf name"
        );
        // An out-of-range press clears the arm (a press on no real row).
        s.press(99);
        assert_eq!(s.pressed(), None, "out-of-range press disarms");
        assert_eq!(s.begin_file_drag(), None);
    }

    #[test]
    fn r794_drag_over_resolves_only_folders_up_and_not_self() {
        let s = state(); // [src(dir)@0, Cargo.toml@1, README.md@2]
        s.press(2); // dragging README.md
        // A directory row is a move-into target.
        s.drag_over(Some(&drop_at("fb#0")));
        assert_eq!(
            s.drop_target(),
            Some(FileDropTarget::Row(0)),
            "folder row = drop into it"
        );
        // A file row is not a move-into target.
        s.drag_over(Some(&drop_at("fb#1")));
        assert_eq!(s.drop_target(), None, "a file has no inside");
        // The dragged source row itself is inert.
        s.press(0);
        s.drag_over(Some(&drop_at("fb#0")));
        assert_eq!(s.drop_target(), None, "dropping onto yourself is no target");
        // The `../` breadcrumb resolves to Up.
        s.drag_over(Some(&drop_at("fb#up")));
        assert_eq!(
            s.drop_target(),
            Some(FileDropTarget::Up),
            "breadcrumb = move up"
        );
        // The background (no tag) clears the target.
        s.drag_over(None);
        assert_eq!(s.drop_target(), None, "over no region = no target");
    }

    #[test]
    fn r794_drag_drop_moves_file_into_folder() {
        let s = state(); // [src(dir)@0, Cargo.toml@1, README.md@2]
        s.select("README.md"); // selection follows the move out of the listing
        s.press(2); // drag README.md
        s.drag_over(Some(&drop_at("fb#0"))); // over src/
        assert!(s.drag_drop(Some(&drop_at("fb#0"))), "the move took effect");
        assert!(
            !s.entries().iter().any(|e| e.name == "README.md"),
            "file left the cwd"
        );
        assert_eq!(s.selected(), None, "the moved selection cleared");
        assert_eq!(s.pressed(), None, "drag arm cleared on drop");
        assert_eq!(s.drop_target(), None, "drop highlight cleared on drop");
        // It landed inside src/.
        s.navigate("src");
        assert!(
            s.entries().iter().any(|e| e.name == "README.md"),
            "file is now inside src/"
        );
    }

    #[test]
    fn r794_drag_drop_into_breadcrumb_moves_to_parent() {
        let s = state();
        s.navigate("src"); // /proj/src: [lib.rs@0, main.rs@1]
        s.press(0); // drag lib.rs
        assert!(
            s.drag_drop(Some(&drop_at("fb#up"))),
            "drop on `../` moves to the parent"
        );
        assert!(
            !s.entries().iter().any(|e| e.name == "lib.rs"),
            "lib.rs left /proj/src"
        );
        s.up();
        assert!(
            s.entries().iter().any(|e| e.name == "lib.rs"),
            "lib.rs landed in /proj"
        );
    }

    #[test]
    fn r794_drag_drop_onto_file_or_self_is_inert() {
        let s = state();
        s.press(2); // README.md
        // Drop onto a file row (Cargo.toml@1) → no valid target.
        assert!(
            !s.drag_drop(Some(&drop_at("fb#1"))),
            "dropping onto a file is inert"
        );
        assert!(
            s.entries().iter().any(|e| e.name == "README.md"),
            "nothing moved"
        );
        // Drop onto the source row itself → inert.
        s.press(2);
        assert!(
            !s.drag_drop(Some(&drop_at("fb#2"))),
            "dropping onto yourself is inert"
        );
        // Drop with no armed source → inert.
        assert!(!s.drag_drop(Some(&drop_at("fb#0"))));
    }

    #[test]
    fn r794_move_dir_into_dir_carries_subtree() {
        let d = InMemoryDirectory::new();
        d.insert("/p", vec![DirEntry::dir("from"), DirEntry::dir("dest")]);
        d.insert("/p/from", vec![DirEntry::file("a.rs")]);
        d.insert("/p/dest", vec![]);
        let s = DirectoryState::new(Rc::new(d), "/p"); // [dest@0, from@1]
        s.press(1); // drag the `from` directory
        assert!(
            s.drag_drop(Some(&drop_at("fb#0"))),
            "folder moved into dest/"
        );
        assert!(
            !s.entries().iter().any(|e| e.name == "from"),
            "from/ left /p"
        );
        s.navigate("dest");
        assert!(
            s.entries().iter().any(|e| e.name == "from"),
            "from/ now under dest/"
        );
        s.navigate("from");
        assert!(
            s.entries().iter().any(|e| e.name == "a.rs"),
            "the subtree came along"
        );
    }

    #[test]
    fn r794_move_into_self_descendant_or_same_dir_rejected() {
        let d = InMemoryDirectory::new();
        d.insert("/a", vec![DirEntry::dir("dir1")]);
        d.insert("/a/dir1", vec![DirEntry::dir("sub")]);
        d.insert("/a/dir1/sub", vec![]);
        let s = DirectoryState::new(Rc::new(d), "/a");
        // Into the current directory (already there) → rejected.
        assert!(
            !s.move_into("dir1", "/a"),
            "moving into the current dir is a no-op"
        );
        // Into itself → rejected.
        assert!(
            !s.move_into("dir1", "/a/dir1"),
            "moving a dir into itself is refused"
        );
        // Into its own descendant → rejected (would corrupt the subtree re-key).
        assert!(
            !s.move_into("dir1", "/a/dir1/sub"),
            "moving a dir into its subtree is refused"
        );
        // The listing is untouched by every rejected move.
        assert!(
            s.entries().iter().any(|e| e.name == "dir1"),
            "dir1 still in /a"
        );
    }

    #[test]
    fn r794_external_drag_hooks_and_introspection() {
        let st = Rc::new(state()); // [src(dir)@0, Cargo.toml@1, README.md@2]
        let mut ext = DirectoryExternal::new(Rc::clone(&st));
        assert_eq!(ext.query("dragging"), Some(IntrospectValue::Bool(false)));
        assert_eq!(ext.query("drop_target"), Some(IntrospectValue::Null));
        // A row PointerDown arms the drag (the router's begin_drag source).
        ext.invoke("send", IntrospectValue::Text("2:PointerDown".into()))
            .unwrap();
        assert_eq!(
            ext.query("dragging"),
            Some(IntrospectValue::Bool(true)),
            "armed after PointerDown"
        );
        let payload = ext
            .begin_drag()
            .expect("begin_drag arms from the pressed row");
        // drag_to over a folder row surfaces the introspectable target.
        ext.drag_to(&payload, Some(drop_at("fb#0")));
        assert_eq!(
            ext.query("drop_target"),
            Some(IntrospectValue::Text("row:0".into())),
            "the in-flight target is scene-as-data",
        );
        // release commits the move and clears the transient state.
        ext.drag_release(&payload, Some(drop_at("fb#0")));
        assert!(
            !st.entries().iter().any(|e| e.name == "README.md"),
            "README.md moved out of /proj"
        );
        assert_eq!(
            ext.query("dragging"),
            Some(IntrospectValue::Bool(false)),
            "disarmed after drop"
        );
        assert_eq!(
            ext.query("drop_target"),
            Some(IntrospectValue::Null),
            "highlight cleared after drop"
        );
        // The drag introspection slots are read-only.
        assert_eq!(
            ext.intervene("dragging", IntrospectValue::Bool(true)),
            Err(InterveneError::ReadOnly)
        );
        assert_eq!(
            ext.intervene("drop_target", IntrospectValue::Text("up".into())),
            Err(InterveneError::ReadOnly),
        );
    }

    #[test]
    fn r794_external_drop_target_reports_up() {
        let st = Rc::new(state());
        st.navigate("src");
        let mut ext = DirectoryExternal::new(Rc::clone(&st));
        ext.invoke("send", IntrospectValue::Text("0:PointerDown".into()))
            .unwrap();
        let payload = ext.begin_drag().expect("armed");
        ext.drag_to(&payload, Some(drop_at("fb#up")));
        assert_eq!(
            ext.query("drop_target"),
            Some(IntrospectValue::Text("up".into())),
            "Up reports as text"
        );
    }
}
