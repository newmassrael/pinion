//! R761 §3 §5.15 — platform file-dialog bridge for the `pinion_core`
//! [`FileDialog`] trait. [`RfdFileDialog`] routes the
//! open / save / pick-folder surface to the OS-native modal through the
//! cross-platform [`rfd`](https://docs.rs/rfd) crate (xdg-desktop-portal
//! / GTK on Linux, `NSOpenPanel` on macOS, `IFileDialog` on Windows).
//!
//! Designed as the second-consumer impl of [`FileDialog`] alongside the
//! pinion-core [`ScriptedFileDialog`](pinion_core::ScriptedFileDialog)
//! first impl: real desktop bindings construct an [`RfdFileDialog`],
//! while headless / deterministic contexts (the RPC demo sweep, unit
//! tests) keep the scripted impl. A native OS dialog cannot be driven
//! headless, which is exactly why the trait is split — the scripted
//! impl is the AI-first verification channel; this crate is the
//! production UX.
//!
//! ## Driving the future
//!
//! Every method returns a [`FileDialogFuture`] wrapping the matching
//! `rfd::AsyncFileDialog` future. With the `xdg-portal` backend the
//! dialog is presented asynchronously over D-Bus, so the future yields
//! `Pending` until the user responds — it must be driven by the host's
//! async runtime (the tokio runtime pinion-shell owns at the §6.3
//! IO-async boundary) or pumped each frame through a
//! [`LocalSpawner`](pinion_core::LocalSpawner). The scripted impl, by
//! contrast, resolves immediately; bindings that want a uniform
//! immediate-or-deferred surface go through
//! [`Resource::fetch_with`](pinion_core::Resource::fetch_with).
//!
//! ## Why the `xdg-portal` backend (Linux)
//!
//! `rfd`'s default Linux backend is GTK, which needs the system
//! `libgtk-3-dev` headers at build time. This crate selects
//! `xdg-portal` instead so it compiles on a headless box without GTK
//! and integrates with the desktop's sandbox-friendly file chooser
//! (the same portal Flatpak / Snap apps use). See `Cargo.toml`.

use pinion_core::file_dialog::{FileDialog, FileDialogFuture, FileDialogRequest};

/// R761 §3 §5.15 — native [`FileDialog`] impl backed by `rfd`. Stateless
/// (every call builds a fresh `rfd::AsyncFileDialog` from the request),
/// so a single instance is freely shared as `Rc<dyn FileDialog>` across
/// the application.
#[derive(Debug, Default, Clone, Copy)]
pub struct RfdFileDialog;

impl RfdFileDialog {
    /// Construct the native file-dialog bridge. Infallible — `rfd`
    /// resolves its backend lazily when a dialog is actually presented,
    /// so construction never touches the display server / portal.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Translate a [`FileDialogRequest`] into a configured
    /// `rfd::AsyncFileDialog`. Title, filters, and start directory apply
    /// to every dialog kind; `suggested_name` is applied separately by
    /// [`Self::save_file`] (it is meaningless for open / pick-folder).
    fn build(request: &FileDialogRequest) -> rfd::AsyncFileDialog {
        let mut dialog = rfd::AsyncFileDialog::new();
        if let Some(title) = &request.title {
            dialog = dialog.set_title(title);
        }
        for filter in &request.filters {
            if !filter.extensions.is_empty() {
                dialog = dialog.add_filter(&filter.name, filter.extensions.as_slice());
            }
        }
        if let Some(dir) = &request.start_dir {
            dialog = dialog.set_directory(dir);
        }
        dialog
    }
}

impl FileDialog for RfdFileDialog {
    fn open_file(&self, request: &FileDialogRequest) -> FileDialogFuture {
        let dialog = Self::build(request);
        // On native targets `rfd::FileHandle::path` is the real
        // filesystem path; `.to_path_buf()` owns it so the resolved
        // `PathBuf` outlives the dialog future.
        Box::pin(async move { dialog.pick_file().await.map(|h| h.path().to_path_buf()) })
    }

    fn save_file(&self, request: &FileDialogRequest) -> FileDialogFuture {
        let mut dialog = Self::build(request);
        if let Some(name) = &request.suggested_name {
            dialog = dialog.set_file_name(name);
        }
        Box::pin(async move { dialog.save_file().await.map(|h| h.path().to_path_buf()) })
    }

    fn pick_folder(&self, request: &FileDialogRequest) -> FileDialogFuture {
        let dialog = Self::build(request);
        Box::pin(async move { dialog.pick_folder().await.map(|h| h.path().to_path_buf()) })
    }
}

#[cfg(test)]
mod tests {
    //! R761 §5.15 — construction + trait-object dispatch. The dialogs
    //! themselves cannot be presented headlessly (no display server /
    //! portal in CI), so these tests stay at the type / wiring level;
    //! the behavioural surface is exercised through
    //! [`ScriptedFileDialog`](pinion_core::ScriptedFileDialog) in
    //! pinion-core + the `hello-file-dialog` RPC demo.

    use super::RfdFileDialog;
    use pinion_core::file_dialog::{FileDialog, FileDialogRequest, FileFilter};
    use std::rc::Rc;

    #[test]
    fn r761_constructs_and_coerces_to_dyn_file_dialog() {
        let dialog = RfdFileDialog::new();
        let _handle: Rc<dyn FileDialog> = Rc::new(dialog);
    }

    #[test]
    fn r761_build_accepts_full_request_without_panicking() {
        // Building the dialog config must not touch the display server;
        // only presenting it (await) would. Construct futures and drop
        // them undriven to assert the builder path is panic-free.
        let dialog = RfdFileDialog::new();
        let request = FileDialogRequest::new()
            .with_title("Open project")
            .with_filter(FileFilter::new("pinion scene", ["pinion.xml"]))
            .with_suggested_name("untitled.pinion.xml")
            .with_start_dir("/tmp");
        // Construct then drop the futures undriven — `let _ =` on a
        // future trips `clippy::let_underscore_future`, so drop explicitly.
        drop(dialog.open_file(&request));
        drop(dialog.save_file(&request));
        drop(dialog.pick_folder(&request));
    }
}
