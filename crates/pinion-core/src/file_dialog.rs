//! R761 §3 §5.15 — `FileDialog` substrate for the `External(opaque)`
//! escape hatch: native **open** / **save** / **pick-folder** dialogs.
//!
//! Mirrors [`crate::Storage`] (R665) and [`crate::Clipboard`] (R56):
//! a pure-Rust trait plus a deterministic in-process implementation
//! live here in `pinion-core`; the native OS bridge (`rfd` →
//! GTK / `xdg-desktop-portal` on Linux, `NSOpenPanel` on macOS,
//! `IFileDialog` on Windows) lives in the `pinion-platform-file-dialog`
//! crate. Bindings construct the native impl when an OS dialog is
//! reachable and fall back to [`ScriptedFileDialog`] for headless /
//! deterministic contexts — the same first-impl / second-impl split
//! Storage uses with `InMemoryStorage` / `FileStorage`.
//!
//! ## Why async (unlike Storage / Clipboard)
//!
//! A file dialog is *modal user interaction*, not an instantaneous
//! key/value or string read. Every method returns a
//! [`FileDialogFuture`] that resolves when the user picks a path
//! ([`Some`]) or dismisses the dialog ([`None`]). The canonical
//! reactive carrier is [`Resource`](crate::Resource) (§5.22 R26): a
//! binding drives
//!
//! ```ignore
//! resource.fetch_with(spawner, async move { Ok(dialog.open_file(&req).await) });
//! ```
//!
//! and the view renders `Loading` (dialog open) → `Ready(Some(path))`
//! (chosen) / `Ready(None)` (cancelled). The §5.22 `Loading` state is
//! exactly the "a dialog is currently open" signal AI introspection
//! reads off `scene/snapshot` — the dialog's lifetime is observable
//! without any pixels (§2 #7 scene-as-data).
//!
//! ## Why `!Send` (UI-thread pump)
//!
//! Native file dialogs are modal **on the UI thread**: macOS requires
//! `NSOpenPanel` to run on the main thread, Windows `IFileDialog` is
//! single-threaded-apartment, and the GTK backend drives the GTK main
//! loop. [`FileDialogFuture`] is therefore `!Send` — pumped on the UI
//! thread through a [`LocalSpawner`](crate::LocalSpawner), never spawned
//! to a worker. This is the deliberate opposite of the §5.23
//! `Command` / `Executor` pipeline (whose `BoxFuture` is `Send`,
//! worker-spawned) precisely because that pipeline targets background
//! IO, not UI-thread modal interaction.
//!
//! ## Substrate placement
//!
//! Lives in `pinion-core` (not `pinion-core::widgets`) because the
//! trait + scripted impl serve the whole application's "open a
//! project" / "save as" / "import asset" flows rather than a single
//! widget — the same placement rationale as [`Storage`](crate::Storage)
//! and [`Clipboard`](crate::Clipboard).

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::task::{Context, Poll};

/// R761 §5.15 — boxed UI-thread future returned by every [`FileDialog`]
/// method.
///
/// `Output = Option<PathBuf>`: [`Some`] is the path the user chose,
/// [`None`] is dismissal (Cancel / Esc / window close). This mirrors
/// the total, optional surface of [`Storage::load`](crate::Storage::load)
/// — a cancelled dialog is a normal outcome, not an error.
///
/// `!Send` by design (no `Send` bound): see the module docs — native
/// dialogs are UI-thread modal. The future is driven on the same
/// thread that owns the reactive scope it ultimately mutates, through
/// a [`LocalSpawner`](crate::LocalSpawner).
pub type FileDialogFuture = Pin<Box<dyn Future<Output = Option<PathBuf>> + 'static>>;

/// R761 §5.15 — a named extension filter offered in the dialog's
/// file-type dropdown. Mirrors the `rfd::FileDialog::add_filter(name,
/// &extensions)` shape and the HTML `<input accept>` / GTK
/// `FileFilter` model.
///
/// `extensions` are bare lowercase suffixes **without** the leading
/// dot (`"png"`, not `".png"` or `"*.png"`) — the native bridge adds
/// whatever glob/MIME decoration each platform expects.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileFilter {
    /// Human-readable group label shown in the dialog (e.g. `"Images"`,
    /// `"pinion scene (*.pinion.xml)"`).
    pub name: String,
    /// Accepted extensions without the leading dot (`["png", "jpg"]`).
    pub extensions: Vec<String>,
}

impl FileFilter {
    /// Construct a filter from a label and an iterator of extensions.
    /// Extensions are stored verbatim (lowercase, dot-less is the
    /// caller's convention — see the type docs).
    #[must_use]
    pub fn new<N, E, S>(name: N, extensions: E) -> Self
    where
        N: Into<String>,
        E: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            name: name.into(),
            extensions: extensions.into_iter().map(Into::into).collect(),
        }
    }
}

/// R761 §5.15 — which of the three dialog flavours a call requested.
/// Recorded by [`ScriptedFileDialog`] so tests / AI introspection can
/// assert *what kind* of dialog a binding opened, and consumed by the
/// native bridge to pick the matching `rfd` entry point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    /// Pick one existing file to read ([`FileDialog::open_file`]).
    OpenFile,
    /// Choose a destination path to write ([`FileDialog::save_file`]).
    SaveFile,
    /// Pick a directory ([`FileDialog::pick_folder`]).
    PickFolder,
}

impl DialogKind {
    /// Stable lowercase wire name (`"open_file"` / `"save_file"` /
    /// `"pick_folder"`) for RPC introspection + log assertions.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            DialogKind::OpenFile => "open_file",
            DialogKind::SaveFile => "save_file",
            DialogKind::PickFolder => "pick_folder",
        }
    }
}

/// R761 §5.15 — parameters for a dialog invocation. Every field is
/// optional / empty by default so a bare `FileDialogRequest::default()`
/// opens an unfiltered dialog at the platform's default location.
///
/// Built fluently:
///
/// ```
/// use pinion_core::file_dialog::{FileDialogRequest, FileFilter};
/// let req = FileDialogRequest::new()
///     .with_title("Open project")
///     .with_filter(FileFilter::new("pinion scene", ["pinion.xml"]))
///     .with_suggested_name("untitled.pinion.xml");
/// assert_eq!(req.title.as_deref(), Some("Open project"));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileDialogRequest {
    /// Window title for the dialog (`None` → platform default).
    pub title: Option<String>,
    /// File-type filters offered in the dropdown (empty → all files).
    pub filters: Vec<FileFilter>,
    /// Pre-filled file name for a **save** dialog (`None` → empty /
    /// platform default). Ignored by open / pick-folder.
    pub suggested_name: Option<String>,
    /// Directory the dialog opens in (`None` → platform default, which
    /// is usually the last-used directory).
    pub start_dir: Option<PathBuf>,
}

impl FileDialogRequest {
    /// Construct an empty request (unfiltered, default location).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the dialog window title.
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Append one file-type filter.
    #[must_use]
    pub fn with_filter(mut self, filter: FileFilter) -> Self {
        self.filters.push(filter);
        self
    }

    /// Set the suggested file name for a **save** dialog.
    #[must_use]
    pub fn with_suggested_name(mut self, name: impl Into<String>) -> Self {
        self.suggested_name = Some(name.into());
        self
    }

    /// Set the directory the dialog opens in.
    #[must_use]
    pub fn with_start_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.start_dir = Some(dir.into());
        self
    }
}

/// R761 §3 §5.15 — native file-dialog capability behind the
/// `External(opaque)` escape hatch.
///
/// Every method takes `&self` (not `&mut self`) so an
/// `Rc<dyn FileDialog>` handle can be shared across hooks / callbacks /
/// `External` bridges through immutable composition — the same sharing
/// model as [`Storage`](crate::Storage) and [`Clipboard`](crate::Clipboard).
/// Implementations pick their own interior-mutability model
/// ([`ScriptedFileDialog`] uses `RefCell`; the native `rfd` bridge is
/// stateless).
///
/// The returned [`FileDialogFuture`] resolves to [`Some`] (chosen path)
/// or [`None`] (dismissed). There is no `Result`: a backend that cannot
/// present a dialog at all (no display server, no portal) resolves
/// [`None`], keeping the surface total. A caller that must distinguish
/// "user cancelled" from "no backend" wires the concrete impl's own
/// diagnostics (the native bridge logs the underlying error).
pub trait FileDialog {
    /// Open a dialog to pick **one existing file** to read.
    fn open_file(&self, request: &FileDialogRequest) -> FileDialogFuture;

    /// Open a dialog to choose a **destination path** to write.
    /// `request.suggested_name` pre-fills the file-name field.
    fn save_file(&self, request: &FileDialogRequest) -> FileDialogFuture;

    /// Open a dialog to pick **a directory**.
    fn pick_folder(&self, request: &FileDialogRequest) -> FileDialogFuture;
}

/// R761 §5.15 — record of one [`FileDialog`] call captured by
/// [`ScriptedFileDialog`]. Lets tests + AI introspection assert which
/// dialog kind a binding opened and with what request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptedCall {
    /// Which method was invoked.
    pub kind: DialogKind,
    /// The request the binding passed.
    pub request: FileDialogRequest,
}

/// R761 §3 §5.15 — deterministic, in-process [`FileDialog`] impl. The
/// counterpart of [`InMemoryStorage`](crate::InMemoryStorage): instead
/// of presenting a real OS dialog, it pops the next pre-scripted
/// outcome from a FIFO queue and resolves immediately.
///
/// "Scripted" = the caller scripts what the user *would* choose:
///
/// ```
/// use pinion_core::file_dialog::{FileDialog, FileDialogRequest, ScriptedFileDialog};
///
/// let dialog = ScriptedFileDialog::new();
/// dialog.queue_selection("/projects/demo.pinion.xml");
/// // The returned future is immediately ready; a binding drives it
/// // through a `LocalSpawner` (see `Resource::fetch_with`). Here we
/// // just observe the scripting surface: the call is logged and the
/// // queued outcome is consumed.
/// drop(dialog.open_file(&FileDialogRequest::new()));
/// assert_eq!(dialog.call_count(), 1);
/// assert_eq!(dialog.queued_len(), 0);
/// ```
///
/// This is what demos + the RPC sweep drive (a native OS dialog cannot
/// run headless), and the canonical test fixture. The trait-split that
/// makes it possible *is* the verification discipline + AI-first
/// channel: every dialog interaction a binding performs is reproducible
/// from a script and observable through the [`calls`](Self::calls) log.
///
/// Single-thread only (`RefCell`, no `Send` / `Sync`): the canonical
/// pinion view-fn / callback invocation is UI-thread synchronous (§6.3),
/// matching [`InMemoryStorage`](crate::InMemoryStorage).
#[derive(Debug, Default)]
pub struct ScriptedFileDialog {
    /// FIFO of scripted responses the next calls will return. An empty
    /// queue resolves [`None`] (modelling a user who cancels).
    queued: RefCell<VecDeque<ScriptedResponse>>,
    /// Append-only log of every call, for introspection / assertions.
    calls: RefCell<Vec<ScriptedCall>>,
}

/// R761.1 §5.15 — one queued scripted response: the outcome plus how
/// many `Pending` polls precede it. `pending_polls == 0` resolves
/// immediately (the common path); `> 0` models a *deferred* native
/// dialog future (an `rfd::AsyncFileDialog` awaiting an xdg-portal
/// reply), exercising the UI-thread [`LocalTaskPump`](crate::LocalTaskPump)
/// keep-alive that re-polls it each frame until it resolves.
#[derive(Debug)]
struct ScriptedResponse {
    outcome: Option<PathBuf>,
    pending_polls: u32,
}

/// R761.1 §5.15 — future that yields `Pending` `remaining` times then
/// `Ready(outcome)`. The deterministic stand-in for a deferred native
/// dialog reply; lets a headless RPC demo prove the shell pump drives a
/// multi-frame future to completion (the dialog reads `Loading` until
/// the count hits zero, then `Ready`).
struct DeferredOutcome {
    remaining: Cell<u32>,
    outcome: Option<PathBuf>,
}

impl Future for DeferredOutcome {
    type Output = Option<PathBuf>;
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<PathBuf>> {
        if self.remaining.get() == 0 {
            Poll::Ready(self.outcome.clone())
        } else {
            self.remaining.set(self.remaining.get() - 1);
            Poll::Pending
        }
    }
}

impl ScriptedFileDialog {
    /// Construct a scripted dialog with an empty outcome queue. Until
    /// an outcome is queued, every call resolves [`None`] (cancelled).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a raw outcome — [`Some`] (the user picks this path) or
    /// [`None`] (the user cancels). Outcomes are consumed in FIFO order
    /// by subsequent [`FileDialog`] calls.
    pub fn queue(&self, outcome: Option<PathBuf>) {
        self.queued.borrow_mut().push_back(ScriptedResponse {
            outcome,
            pending_polls: 0,
        });
    }

    /// Queue a successful selection (`Some(path)`).
    pub fn queue_selection(&self, path: impl Into<PathBuf>) {
        self.queue(Some(path.into()));
    }

    /// R761.1 — queue a selection that resolves only after
    /// `pending_polls` `Pending` polls: a deterministic stand-in for a
    /// *deferred* native dialog future (`rfd::AsyncFileDialog` awaiting
    /// an xdg-portal reply). The dialog reads `Loading` until the count
    /// reaches zero, then `Ready(path)` — exercising the UI-thread
    /// [`LocalTaskPump`](crate::LocalTaskPump) keep-alive that drives a
    /// multi-frame future to completion.
    pub fn queue_deferred_selection(&self, path: impl Into<PathBuf>, pending_polls: u32) {
        self.queued.borrow_mut().push_back(ScriptedResponse {
            outcome: Some(path.into()),
            pending_polls,
        });
    }

    /// Queue a cancellation (`None`). Equivalent to leaving the queue
    /// empty for one call, but explicit when interleaving with
    /// selections.
    pub fn queue_cancel(&self) {
        self.queue(None);
    }

    /// Number of outcomes still queued (not yet consumed by a call).
    #[must_use]
    pub fn queued_len(&self) -> usize {
        self.queued.borrow().len()
    }

    /// Snapshot the call log. Cloned out so the caller can inspect it
    /// without holding the internal borrow.
    #[must_use]
    pub fn calls(&self) -> Vec<ScriptedCall> {
        self.calls.borrow().clone()
    }

    /// Number of [`FileDialog`] calls recorded so far.
    #[must_use]
    pub fn call_count(&self) -> usize {
        self.calls.borrow().len()
    }

    /// The most recent recorded call, if any.
    #[must_use]
    pub fn last_call(&self) -> Option<ScriptedCall> {
        self.calls.borrow().last().cloned()
    }

    /// Record the call and resolve the next queued outcome. Shared by
    /// all three trait methods — the only thing that differs between
    /// open / save / pick-folder for a scripted dialog is the recorded
    /// [`DialogKind`].
    fn respond(&self, kind: DialogKind, request: &FileDialogRequest) -> FileDialogFuture {
        self.calls.borrow_mut().push(ScriptedCall {
            kind,
            request: request.clone(),
        });
        match self.queued.borrow_mut().pop_front() {
            // Empty queue == the user cancelled.
            None => Box::pin(core::future::ready(None)),
            // Immediate response (the common scripted path).
            Some(ScriptedResponse {
                outcome,
                pending_polls: 0,
            }) => Box::pin(core::future::ready(outcome)),
            // Deferred response: Pending for `pending_polls` polls,
            // driven to completion by the shell's per-frame pump.
            Some(ScriptedResponse {
                outcome,
                pending_polls,
            }) => Box::pin(DeferredOutcome {
                remaining: Cell::new(pending_polls),
                outcome,
            }),
        }
    }
}

impl FileDialog for ScriptedFileDialog {
    fn open_file(&self, request: &FileDialogRequest) -> FileDialogFuture {
        self.respond(DialogKind::OpenFile, request)
    }

    fn save_file(&self, request: &FileDialogRequest) -> FileDialogFuture {
        self.respond(DialogKind::SaveFile, request)
    }

    fn pick_folder(&self, request: &FileDialogRequest) -> FileDialogFuture {
        self.respond(DialogKind::PickFolder, request)
    }
}

#[cfg(test)]
mod tests {
    //! R761 §3 §5.15 — `FileDialog` + `ScriptedFileDialog` regression
    //! battery: queue FIFO ordering, cancel-on-empty, the call log,
    //! request capture, and `Rc<dyn FileDialog>` polymorphism.

    use super::{
        DialogKind, FileDialog, FileDialogRequest, FileFilter, ScriptedFileDialog,
    };
    use std::future::Future;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    /// Drive an immediately-ready future to its output with the stable
    /// `Waker::noop`. Every [`ScriptedFileDialog`] future is
    /// `core::future::ready`, so the first poll always yields `Ready`;
    /// a `Pending` would mean a real dialog backend leaked into a unit
    /// test, which we surface loudly. Mirrors the `BlockingSpawner`
    /// helper in `reactive::resource` tests (pinion-core keeps no
    /// `futures_executor` dependency).
    fn block<F: Future>(future: F) -> F::Output {
        let mut future = Box::pin(future);
        let waker: &Waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        match Pin::new(&mut future).poll(&mut cx) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("ScriptedFileDialog future must be immediately ready"),
        }
    }

    #[test]
    fn r761_empty_queue_resolves_cancelled() {
        let d = ScriptedFileDialog::new();
        assert_eq!(block(d.open_file(&FileDialogRequest::new())), None);
        assert_eq!(d.call_count(), 1);
    }

    #[test]
    fn r761_queued_selection_resolves_some() {
        let d = ScriptedFileDialog::new();
        d.queue_selection("/a/b.txt");
        assert_eq!(
            block(d.open_file(&FileDialogRequest::new())),
            Some(PathBuf::from("/a/b.txt")),
        );
        assert_eq!(d.queued_len(), 0);
    }

    #[test]
    fn r761_1_deferred_selection_pends_then_resolves() {
        // A deferred response yields Pending `pending_polls` times then
        // Ready — the deterministic stand-in for a native dialog future
        // the shell pump drives across frames.
        let d = ScriptedFileDialog::new();
        d.queue_deferred_selection("/slow.dat", 2);
        let mut fut = d.open_file(&FileDialogRequest::new());
        let waker: &Waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        assert!(Pin::new(&mut fut).poll(&mut cx).is_pending(), "poll 1 Pending");
        assert!(Pin::new(&mut fut).poll(&mut cx).is_pending(), "poll 2 Pending");
        match Pin::new(&mut fut).poll(&mut cx) {
            Poll::Ready(out) => assert_eq!(out, Some(PathBuf::from("/slow.dat"))),
            Poll::Pending => panic!("poll 3 must resolve the deferred outcome"),
        }
    }

    #[test]
    fn r761_queue_is_fifo_across_calls() {
        let d = ScriptedFileDialog::new();
        d.queue_selection("/first");
        d.queue_cancel();
        d.queue_selection("/third");
        assert_eq!(block(d.open_file(&FileDialogRequest::new())), Some("/first".into()));
        assert_eq!(block(d.save_file(&FileDialogRequest::new())), None);
        assert_eq!(block(d.pick_folder(&FileDialogRequest::new())), Some("/third".into()));
    }

    #[test]
    fn r761_call_log_records_kind_and_request() {
        let d = ScriptedFileDialog::new();
        let req = FileDialogRequest::new()
            .with_title("Open project")
            .with_filter(FileFilter::new("scene", ["pinion.xml"]));
        let _ = block(d.open_file(&req));
        let _ = block(d.save_file(&FileDialogRequest::new().with_suggested_name("out.txt")));
        let _ = block(d.pick_folder(&FileDialogRequest::new()));

        let calls = d.calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].kind, DialogKind::OpenFile);
        assert_eq!(calls[0].request.title.as_deref(), Some("Open project"));
        assert_eq!(calls[0].request.filters.len(), 1);
        assert_eq!(calls[1].kind, DialogKind::SaveFile);
        assert_eq!(calls[1].request.suggested_name.as_deref(), Some("out.txt"));
        assert_eq!(calls[2].kind, DialogKind::PickFolder);
    }

    #[test]
    fn r761_last_call_tracks_most_recent() {
        let d = ScriptedFileDialog::new();
        assert!(d.last_call().is_none());
        let _ = block(d.pick_folder(&FileDialogRequest::new()));
        assert_eq!(d.last_call().map(|c| c.kind), Some(DialogKind::PickFolder));
    }

    #[test]
    fn r761_dialog_kind_wire_names() {
        assert_eq!(DialogKind::OpenFile.as_str(), "open_file");
        assert_eq!(DialogKind::SaveFile.as_str(), "save_file");
        assert_eq!(DialogKind::PickFolder.as_str(), "pick_folder");
    }

    #[test]
    fn r761_filter_strips_nothing_stores_verbatim() {
        let f = FileFilter::new("Images", ["png", "jpg", "webp"]);
        assert_eq!(f.name, "Images");
        assert_eq!(f.extensions, vec!["png", "jpg", "webp"]);
    }

    #[test]
    fn r761_request_builder_sets_all_fields() {
        let req = FileDialogRequest::new()
            .with_title("t")
            .with_filter(FileFilter::new("a", ["x"]))
            .with_suggested_name("name")
            .with_start_dir("/start");
        assert_eq!(req.title.as_deref(), Some("t"));
        assert_eq!(req.suggested_name.as_deref(), Some("name"));
        assert_eq!(req.start_dir, Some(PathBuf::from("/start")));
        assert_eq!(req.filters.len(), 1);
    }

}

// Queueing is a concrete-impl affordance, not part of the capability
// surface, so `Rc<dyn FileDialog>` dispatch is exercised through the
// concrete handle that retains the queue — see below.
#[cfg(test)]
mod dyn_dispatch_tests {
    use super::{FileDialog, FileDialogRequest, ScriptedFileDialog};
    use std::future::Future;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::rc::Rc;
    use std::task::{Context, Poll, Waker};

    fn block<F: Future>(future: F) -> F::Output {
        let mut future = Box::pin(future);
        let mut cx = Context::from_waker(Waker::noop());
        match Pin::new(&mut future).poll(&mut cx) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("ScriptedFileDialog future must be immediately ready"),
        }
    }

    #[test]
    fn r761_rc_dyn_dispatch_resolves_scripted_outcome() {
        let concrete = Rc::new(ScriptedFileDialog::new());
        concrete.queue_selection("/via/dyn.txt");
        let handle: Rc<dyn FileDialog> = concrete.clone();
        let chosen = block(handle.open_file(&FileDialogRequest::new()));
        assert_eq!(chosen, Some(PathBuf::from("/via/dyn.txt")));
        assert_eq!(concrete.call_count(), 1);
    }
}
