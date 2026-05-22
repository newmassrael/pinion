//! R56.1.e §5.22 §5.38 — Clipboard substrate for the R56
//! `TextField` axis. Pure-Rust trait + in-memory implementation;
//! platform integration (X11 PRIMARY/CLIPBOARD selection, Wayland
//! `wl_data_device`, macOS `NSPasteboard`, Windows
//! `OpenClipboard`/`SetClipboardData`) lives in a follow-up
//! `pinion-platform-clipboard` crate (carry).
//!
//! The trait is intentionally minimal: `copy(text)` writes a string
//! to the clipboard, `paste()` reads the current string (or `None`
//! when empty / unavailable). Multi-MIME payloads (text/html /
//! image/png / files), clipboard history, and selection-vs-clipboard
//! distinction (X11 PRIMARY vs CLIPBOARD) are R56.1.e follow-ups —
//! the W3C Clipboard API surface starts with the same string-only
//! shape (`navigator.clipboard.writeText` / `readText`) before
//! `ClipboardItem` lands.
//!
//! ## Substrate placement
//!
//! Lives in `pinion-core` (not `pinion-core::widgets`) because the
//! trait + in-memory impl are useful outside the `TextField` widget:
//! a future `RichTextEdit`, `TextArea`, or even non-text widgets
//! (the Slint `Image` widget's "copy URL") plug in through the same
//! abstraction. Same placement rationale as
//! `pinion_core::input::Modifiers` (R56.1.f.0): substrate-level
//! primitives that more than one widget consumes belong above the
//! widgets module.

use std::cell::RefCell;

/// R56.1.e §5.22 — string-only clipboard surface. Every method is
/// `&self` (not `&mut self`) so a `Rc<dyn Clipboard>` handle can
/// shared across widgets through immutable composition (the
/// canonical `Rc<RefCell<_>>` interior-mutability shape an
/// implementer picks per its own thread-safety / OS-binding model).
///
/// Mirror of the W3C `navigator.clipboard.writeText` /
/// `readText` async surface, except synchronous (UI-thread access to
/// the OS clipboard is synchronous on every desktop platform — the
/// W3C Promise wrapping exists for browser-sandbox permission gating,
/// not OS-level capability).
pub trait Clipboard {
    /// R56.1.e §5.22 — write `text` to the clipboard, replacing any
    /// prior payload. Mirror of W3C `writeText`. The implementation
    /// chooses whether to record an empty string as "clipboard
    /// holds empty string" or "clipboard cleared"; the in-memory
    /// impl records the empty string verbatim so a round-trip
    /// `copy("") → paste() == Some("")` holds.
    fn copy(&self, text: String);

    /// R56.1.e §5.22 — read the current clipboard string. `None`
    /// when no payload has been written yet (the in-memory impl's
    /// fresh-default state) or when the OS clipboard holds a
    /// non-text payload the implementation does not decode.
    fn paste(&self) -> Option<String>;
}

/// R56.1.e §5.22 — pure-Rust in-memory `Clipboard` impl. Stores the
/// last `copy`-d string in a `RefCell<Option<String>>`. Used as the
/// canonical test fixture and as the default attached clipboard for
/// example bindings that do not need OS integration (the
/// `hello-textfield*` gallery uses this so the demo runs without
/// platform-specific clipboard plumbing).
///
/// Single-thread only (no `Send` / `Sync`): the canonical pinion
/// view-fn invocation is UI-thread synchronous (§6.3). Multi-thread
/// implementers (a future `PlatformClipboard` reading from an OS
/// callback thread) wrap their payload in `Mutex<_>` / `Arc<_>`.
#[derive(Debug, Default)]
pub struct InMemoryClipboard {
    buf: RefCell<Option<String>>,
}

impl InMemoryClipboard {
    /// Construct a fresh `InMemoryClipboard` with no payload —
    /// `paste()` returns `None` until the first `copy(...)` lands.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl Clipboard for InMemoryClipboard {
    fn copy(&self, text: String) {
        *self.buf.borrow_mut() = Some(text);
    }

    fn paste(&self) -> Option<String> {
        self.buf.borrow().clone()
    }
}

#[cfg(test)]
mod tests {
    //! R56.1.e §5.22 — `Clipboard` + `InMemoryClipboard` regression
    //! battery. Covers the W3C `writeText` / `readText` surface
    //! contract, default `paste()` shape, repeated overwrite, empty-
    //! string round-trip, and `dyn Clipboard` polymorphism.

    use super::{Clipboard, InMemoryClipboard};

    #[test]
    fn r56_1_e_fresh_clipboard_paste_returns_none() {
        let cb = InMemoryClipboard::new();
        assert_eq!(cb.paste(), None);
    }

    #[test]
    fn r56_1_e_copy_then_paste_round_trips() {
        let cb = InMemoryClipboard::new();
        cb.copy("hello".to_string());
        assert_eq!(cb.paste(), Some("hello".to_string()));
    }

    #[test]
    fn r56_1_e_repeated_copy_overwrites_prior_payload() {
        let cb = InMemoryClipboard::new();
        cb.copy("first".to_string());
        cb.copy("second".to_string());
        assert_eq!(cb.paste(), Some("second".to_string()));
    }

    #[test]
    fn r56_1_e_empty_string_round_trips_as_some() {
        // Per the W3C surface, copying an empty string is a write
        // (clipboard now holds empty string), distinct from
        // never-written (paste -> None).
        let cb = InMemoryClipboard::new();
        cb.copy(String::new());
        assert_eq!(cb.paste(), Some(String::new()));
    }

    #[test]
    fn r56_1_e_dyn_clipboard_polymorphism_works() {
        // The canonical handle shape for widgets is `Rc<dyn
        // Clipboard>` — the dyn dispatch must not lose the round-
        // trip behaviour.
        let cb: std::rc::Rc<dyn Clipboard> = std::rc::Rc::new(InMemoryClipboard::new());
        cb.copy("via-dyn".to_string());
        assert_eq!(cb.paste(), Some("via-dyn".to_string()));
    }

    #[test]
    fn r56_1_e_multi_byte_utf8_round_trip() {
        // Korean syllables + emoji exercise the multi-byte path —
        // String is UTF-8 and the in-memory impl does no encoding.
        let cb = InMemoryClipboard::new();
        cb.copy("한글 🌍".to_string());
        assert_eq!(cb.paste(), Some("한글 🌍".to_string()));
    }
}
