//! R56.2.b §5.22 — platform clipboard bridge for the R56.1.e
//! [`pinion_core::Clipboard`] trait.
//!
//! ## Why a separate crate (not in `pinion-core`)
//!
//! The R56.1.e Clipboard substrate (`pinion_core::Clipboard` trait +
//! [`pinion_core::InMemoryClipboard`]) lives in `pinion-core`
//! because the *trait* is widget-substrate (the `TextField`
//! `attach_clipboard` builder + `Ctrl/Cmd+C/X/V` dispatch in
//! [`pinion_core::widgets::text_field`] consumes the trait through
//! an `Rc<dyn Clipboard>` handle). The *platform impl* needs the
//! `arboard` crate which (a) pulls in OS-level bindings
//! (`wl-clipboard-rs`, `x11rb`, `objc2`, `windows-targets`),
//! (b) only compiles on a desktop target (Wayland / X11 / macOS /
//! Windows / WSL), (c) requires a display server at runtime
//! (headless CI fails `Clipboard::new()`). Keeping it out of
//! `pinion-core` preserves the substrate's "compiles on every
//! reasonable target + headless test always works" baseline.
//!
//! Same crate-split rationale as `pinion-tui` (TUI surface lives
//! outside `pinion-core` so headless / Vello-only consumers do not
//! pay the crossterm + ratatui compile cost).
//!
//! ## Textbook decision: `arboard` over hand-written OS bindings
//!
//! [[abstraction-needs-second-consumer]] mandates: a trait extracted
//! for an abstraction must have at least a second consumer in
//! flight; R56.1.e established the first consumer (`InMemoryClipboard`)
//! at substrate-land time. This crate's [`ArboardClipboard`] is the
//! second consumer that justifies the abstraction; without it the
//! `Clipboard` trait would be a 1-impl premature abstraction.
//!
//! `arboard` is the canonical Rust ecosystem clipboard crate
//! (egui, iced, Bevy, `cli-clipboard`, `enigo`, etc. all consume
//! it). It funnels Wayland `wl_data_device` + X11
//! PRIMARY/CLIPBOARD + macOS `NSPasteboard` + Windows
//! `OpenClipboard`/`SetClipboardData` under one trait-shape
//! surface, so this crate's platform-side closure is a single
//! ~30-LOC wrapper rather than four hand-written OS bindings.
//! Mirror of the R56.2.a [[winit-ime-canonical-platform-bridge]]
//! decision (winit over raw `zwp_text_input_v3` for IME).
//!
//! ## Usage
//!
//! ```ignore
//! use std::rc::Rc;
//! use pinion_core::Clipboard;
//! use pinion_core::widgets::text_field::TextFieldExternal;
//! use pinion_platform_clipboard::ArboardClipboard;
//!
//! // Try to acquire the platform clipboard; fall back to the
//! // in-memory impl if the display server is unavailable
//! // (headless CI, broken Wayland socket, etc.).
//! let clipboard: Rc<dyn Clipboard> = ArboardClipboard::try_new()
//!     .map(|cb| Rc::new(cb) as Rc<dyn Clipboard>)
//!     .unwrap_or_else(|_| Rc::new(pinion_core::InMemoryClipboard::new()));
//!
//! let external = TextFieldExternal::new()
//!     .attach_clipboard(clipboard);
//! ```

use std::cell::RefCell;

use pinion_core::Clipboard;

/// R56.2.b §5.22 — platform-backed [`Clipboard`] impl wrapping
/// `arboard::Clipboard`. Construct via [`Self::try_new`] (returns an
/// `arboard::Error` on platforms where the clipboard daemon is not
/// reachable — headless CI / broken Wayland socket / locked-down
/// sandbox); fall back to [`pinion_core::InMemoryClipboard`] when
/// the platform clipboard is unavailable.
///
/// ## Interior mutability
///
/// The [`Clipboard`] trait surface is `&self` (the substrate
/// requires shared-mutability so an `Rc<dyn Clipboard>` handle
/// can be shared across the `TextField` builder + the focus-
/// lifecycle commit path + the keyboard-shortcut dispatch path).
/// `arboard::Clipboard` requires `&mut self` for `set_text` /
/// `get_text`, so this wrapper carries a `RefCell` to bridge the
/// two ownership models. The `&self` API surface is single-
/// threaded (UI-thread access only) and the borrow window is the
/// duration of the synchronous `arboard` call, so the `RefCell`
/// borrow conflict has zero observable race — same shape as the
/// R56.1.e [`InMemoryClipboard`] which carries the same
/// `RefCell<Option<String>>` interior.
///
/// ## Cross-platform behaviour table
///
/// | Platform                  | Backend                                  |
/// |---------------------------|------------------------------------------|
/// | Linux Wayland             | `wl_data_device` via `wl-clipboard-rs`   |
/// | Linux X11                 | XCB `CLIPBOARD` (not PRIMARY)            |
/// | macOS                     | `NSPasteboard.general`                   |
/// | Windows                   | `OpenClipboard` / `SetClipboardData`     |
/// | iOS / Android / WASM      | `arboard::Clipboard::new()` returns Err  |
///
/// Both `copy` and `paste` swallow `arboard` errors (logging is the
/// application's choice) so the `Clipboard` trait surface stays
/// total. A `copy` that the OS rejects is observed as a paste
/// returning the *previous* clipboard value; a `paste` from an
/// empty / inaccessible clipboard returns `None` (same shape as
/// `InMemoryClipboard::paste` on a fresh handle).
pub struct ArboardClipboard {
    inner: RefCell<arboard::Clipboard>,
}

impl ArboardClipboard {
    /// R56.2.b §5.22 — try to acquire the platform clipboard. Fails
    /// when the OS clipboard daemon is unreachable (headless CI,
    /// broken Wayland socket, sandboxed display-less container).
    /// Callers should fall back to [`pinion_core::InMemoryClipboard`]
    /// on error so the `TextField` builder still wires up (Ctrl+C
    /// stays a no-op-to-the-OS but still cycles through the
    /// in-memory copy).
    ///
    /// # Errors
    /// Surfaces the underlying `arboard::Error` (`PlatformInit`,
    /// `Unknown`, etc.) so the caller can log a specific reason.
    pub fn try_new() -> Result<Self, arboard::Error> {
        Ok(Self {
            inner: RefCell::new(arboard::Clipboard::new()?),
        })
    }
}

impl core::fmt::Debug for ArboardClipboard {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ArboardClipboard")
            .finish_non_exhaustive()
    }
}

impl Clipboard for ArboardClipboard {
    /// R56.2.b §5.22 — write `text` to the OS clipboard, swallowing
    /// `arboard` errors (the substrate's `Clipboard` trait surface
    /// is total). A failed write is observed by callers as a `paste`
    /// returning the *previous* clipboard value.
    fn copy(&self, text: String) {
        if let Ok(mut clipboard) = self.inner.try_borrow_mut() {
            // `arboard::Clipboard::set_text` returns `Result`;
            // swallow to keep the trait surface total. Application
            // log-on-error policy lives in the caller (this crate
            // stays substrate-pure).
            let _ = clipboard.set_text(text);
        }
        // `try_borrow_mut` failing means a re-entrant Ctrl+C call
        // mid-paste (impossible under single-threaded UI), so the
        // no-op branch is unreachable in practice; we model it as
        // a silent skip rather than a panic so a future async-
        // clipboard refactor stays backward-compat.
    }

    /// R56.2.b §5.22 — read the current OS clipboard string.
    /// Returns `None` when the clipboard is empty, holds non-text
    /// content (image / files), or the OS daemon errors. Mirrors
    /// W3C `navigator.clipboard.readText` rejection-as-empty
    /// behaviour at the substrate boundary (the trait contract
    /// declares `None` for both "empty" and "unavailable").
    fn paste(&self) -> Option<String> {
        let mut clipboard = self.inner.try_borrow_mut().ok()?;
        clipboard.get_text().ok()
    }
}

#[cfg(test)]
mod tests {
    //! R56.2.b §5.22 — `ArboardClipboard` smoke tests.
    //!
    //! Real platform-clipboard round-trips are not exercised here:
    //! CI runners (GitHub Actions Linux containers, etc.) typically
    //! lack a display server, and Wayland / X11 socket access from
    //! `arboard::Clipboard::new()` would fail with `PlatformInit`.
    //! The substrate behaviour we *can* pin without a display:
    //!
    //! - `Debug` impl renders without panic (so application log
    //!   sites that print the clipboard handle compile + run).
    //! - `try_new` returns either `Ok` (display present) or `Err`
    //!   with a non-empty `Display` (so the fallback path can log
    //!   the reason).
    //!
    //! The round-trip behaviour (`copy("hi") → paste() == Some("hi")`)
    //! is already pinned by `pinion_core::clipboard::tests` for the
    //! in-memory impl; the `arboard` round-trip is `arboard`'s own
    //! test coverage (and is platform-specific behaviour we should
    //! not duplicate at the pinion layer).

    use super::ArboardClipboard;

    #[test]
    fn r56_2_b_debug_renders_without_panic() {
        // `Debug` derive on the wrapper goes through `RefCell`'s
        // `Debug` impl (which short-circuits when the cell is
        // borrowed); we don't need a constructed handle to verify
        // the derived shape compiles — but a constructed handle
        // is only available when the display server is present.
        // We skip-or-render: on success render the Debug, on
        // error verify the error has a non-empty Display.
        match ArboardClipboard::try_new() {
            Ok(cb) => {
                let s = format!("{cb:?}");
                assert!(
                    s.contains("ArboardClipboard"),
                    "Debug must include the struct name, got {s:?}",
                );
            }
            Err(e) => {
                let s = e.to_string();
                assert!(
                    !s.is_empty(),
                    "arboard::Error Display must produce a non-empty reason",
                );
            }
        }
    }
}
