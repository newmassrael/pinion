//! R56.1.e §5.22 §5.38 — Clipboard substrate for the R56
//! `TextField` axis. Pure-Rust trait + in-memory implementation;
//! platform integration (X11 PRIMARY/CLIPBOARD selection, Wayland
//! `wl_data_device`, macOS `NSPasteboard`, Windows
//! `OpenClipboard`/`SetClipboardData`) lives in the
//! `pinion-platform-clipboard` crate (R56.2.b).
//!
//! The trait is intentionally minimal: `copy(text)` writes a string
//! to the clipboard, `paste()` reads the current string (or `None`
//! when empty / unavailable). Multi-MIME payloads (text/html /
//! image/png / files) and clipboard history are R56 follow-ups —
//! the W3C Clipboard API surface starts with the same string-only
//! shape (`navigator.clipboard.writeText` / `readText`) before
//! `ClipboardItem` lands.
//!
//! R56.2.e §5.22 — selection-aware extension: [`ClipboardSelection`]
//! enum + [`Clipboard::copy_to`] / [`Clipboard::paste_from`] default
//! methods. Mirrors the Linux desktop convention where the user has
//! two parallel clipboards: explicit cut/copy/paste (CLIPBOARD) and
//! implicit text-selection / middle-click paste (PRIMARY).
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

/// R56.2.e §5.22 — clipboard selection enumeration. Mirrors the
/// Linux desktop convention (X11 X Selection mechanism / Wayland
/// `zwlr_data_control` protocol) where two parallel clipboards
/// coexist:
///
/// - [`ClipboardSelection::Clipboard`] — the W3C clipboard, written
///   by explicit Ctrl/Cmd+C / Ctrl/Cmd+X actions and read by
///   Ctrl/Cmd+V. The only clipboard present on macOS / Windows /
///   browsers.
/// - [`ClipboardSelection::Primary`] — the X11 PRIMARY selection,
///   written *implicitly* every time the user selects text (no
///   explicit copy keystroke) and pasted by middle-mouse click.
///   Linux desktop convention since X10R3 (1986); also exposed on
///   Wayland via `wl_data_device_manager` v2+ / `zwlr_data_control_v1`.
///   macOS / Windows do not implement this — `Clipboard` impls for
///   those platforms fall back to no-op on `Primary` (the trait's
///   default impl handles this gracefully).
///
/// Marked `#[non_exhaustive]` so a future variant
/// (X11 `SECONDARY`, GTK's `find buffer`, etc.) can land without
/// SemVer-break. Downstream `match` arms must include a default arm
/// (the lint `non_exhaustive_omitted_patterns` will catch missing
/// ones in the same crate).
///
/// `Default = Clipboard` matches the W3C surface assumption: when no
/// selection is specified, callers mean the user-visible clipboard.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
#[non_exhaustive]
pub enum ClipboardSelection {
    /// W3C `navigator.clipboard` target — written by Ctrl/Cmd+C /
    /// Ctrl/Cmd+X and read by Ctrl/Cmd+V. Default.
    #[default]
    Clipboard,

    /// X11 PRIMARY selection / Wayland implicit selection — written
    /// implicitly by text selection (no keystroke) and read by
    /// middle-mouse click. Linux desktop convention; macOS / Windows
    /// implementations fall back to no-op via the [`Clipboard`]
    /// default methods.
    Primary,
}

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
///
/// R56.2.e §5.22 — selection-aware methods [`copy_to`](Self::copy_to)
/// / [`paste_from`](Self::paste_from) extend the surface with the
/// [`ClipboardSelection`] enum. Default impls route `Clipboard`
/// selection to the original `copy` / `paste` methods (preserving
/// W3C semantics + backward compatibility) and treat `Primary` as
/// no-op write / `None` read. Linux platform impls override the
/// defaults to thread `Primary` through to the X11 / Wayland
/// PRIMARY selection.
pub trait Clipboard {
    /// R56.1.e §5.22 — write `text` to the clipboard, replacing any
    /// prior payload. Mirror of W3C `writeText`. The implementation
    /// chooses whether to record an empty string as "clipboard
    /// holds empty string" or "clipboard cleared"; the in-memory
    /// impl records the empty string verbatim so a round-trip
    /// `copy("") → paste() == Some("")` holds.
    ///
    /// Always writes to [`ClipboardSelection::Clipboard`]. For
    /// selection-aware writes (PRIMARY publish on text selection),
    /// use [`copy_to`](Self::copy_to).
    fn copy(&self, text: String);

    /// R56.1.e §5.22 — read the current clipboard string. `None`
    /// when no payload has been written yet (the in-memory impl's
    /// fresh-default state) or when the OS clipboard holds a
    /// non-text payload the implementation does not decode.
    ///
    /// Always reads from [`ClipboardSelection::Clipboard`]. For
    /// selection-aware reads (PRIMARY paste on middle-click), use
    /// [`paste_from`](Self::paste_from).
    fn paste(&self) -> Option<String>;

    /// R56.2.e §5.22 — write `text` to a specific clipboard
    /// selection. Default impl routes `Clipboard` selection to
    /// [`copy`](Self::copy) and treats `Primary` as a silent no-op
    /// (matches macOS / Windows / browser behaviour where the PRIMARY
    /// selection does not exist at the OS level).
    ///
    /// Linux platform impls (X11 / Wayland) override this to write
    /// to the corresponding X selection / `wl_data_device` PRIMARY
    /// channel, so middle-click paste in other apps observes the
    /// text immediately after a pinion widget publishes a selection.
    fn copy_to(&self, selection: ClipboardSelection, text: String) {
        // R56.2.e §5.22 — explicit `match` so an added
        // [`ClipboardSelection`] variant triggers a compile error
        // in this crate (the `#[non_exhaustive]` attribute only
        // forces external crates to add a wildcard arm; in-crate
        // matches stay exhaustive).
        match selection {
            ClipboardSelection::Clipboard => self.copy(text),
            ClipboardSelection::Primary => {
                // No-op default: macOS / Windows / browsers. The
                // Linux platform impl overrides this method to
                // route through `arboard::SetExtLinux::clipboard`.
                let _ = text;
            }
        }
    }

    /// R56.2.e §5.22 — read a specific clipboard selection. Default
    /// impl routes `Clipboard` selection to [`paste`](Self::paste)
    /// and returns `None` for `Primary` (macOS / Windows do not
    /// implement the PRIMARY selection at the OS level).
    ///
    /// Linux platform impls override this to read from the X11
    /// PRIMARY atom / Wayland `wl_data_device` PRIMARY channel, so
    /// middle-click paste reads selections published by other apps
    /// (xterm, Firefox, GTK / Qt apps).
    fn paste_from(&self, selection: ClipboardSelection) -> Option<String> {
        match selection {
            ClipboardSelection::Clipboard => self.paste(),
            ClipboardSelection::Primary => None,
        }
    }
}

/// R56.1.e §5.22 — pure-Rust in-memory `Clipboard` impl. Stores the
/// last `copy`-d string per selection in a `RefCell<Option<String>>`
/// pair. Used as the canonical test fixture and as the default
/// attached clipboard for example bindings that do not need OS
/// integration (the `hello-textfield*` gallery uses this so the demo
/// runs without platform-specific clipboard plumbing).
///
/// R56.2.e §5.22 — carries an independent buffer per
/// [`ClipboardSelection`] variant so tests can verify the
/// `Clipboard` / `Primary` isolation contract end-to-end without a
/// display server. This matches the Linux platform behaviour at the
/// substrate level (X11 PRIMARY and CLIPBOARD selections are
/// independent atoms — writing to one never touches the other).
///
/// Single-thread only (no `Send` / `Sync`): the canonical pinion
/// view-fn invocation is UI-thread synchronous (§6.3). Multi-thread
/// implementers (a future `PlatformClipboard` reading from an OS
/// callback thread) wrap their payload in `Mutex<_>` / `Arc<_>`.
#[derive(Debug, Default)]
pub struct InMemoryClipboard {
    clipboard: RefCell<Option<String>>,
    primary: RefCell<Option<String>>,
}

impl InMemoryClipboard {
    /// Construct a fresh `InMemoryClipboard` with no payload on
    /// either selection — `paste()` / `paste_from(_)` return `None`
    /// until the first `copy(...)` / `copy_to(...)` lands.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl Clipboard for InMemoryClipboard {
    fn copy(&self, text: String) {
        *self.clipboard.borrow_mut() = Some(text);
    }

    fn paste(&self) -> Option<String> {
        self.clipboard.borrow().clone()
    }

    /// R56.2.e §5.22 — selection-aware write. Stores `text` in the
    /// `RefCell` corresponding to `selection`. Overrides the trait
    /// default so `Primary` is preserved (instead of swallowed),
    /// matching the canonical Linux PRIMARY behaviour at the
    /// substrate level.
    fn copy_to(&self, selection: ClipboardSelection, text: String) {
        match selection {
            ClipboardSelection::Clipboard => *self.clipboard.borrow_mut() = Some(text),
            ClipboardSelection::Primary => *self.primary.borrow_mut() = Some(text),
        }
    }

    /// R56.2.e §5.22 — selection-aware read. Returns the
    /// `RefCell<Option<String>>` clone for the requested selection.
    /// Overrides the trait default so `Primary` returns the stored
    /// payload instead of `None`.
    fn paste_from(&self, selection: ClipboardSelection) -> Option<String> {
        match selection {
            ClipboardSelection::Clipboard => self.clipboard.borrow().clone(),
            ClipboardSelection::Primary => self.primary.borrow().clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    //! R56.1.e §5.22 — `Clipboard` + `InMemoryClipboard` regression
    //! battery. Covers the W3C `writeText` / `readText` surface
    //! contract, default `paste()` shape, repeated overwrite, empty-
    //! string round-trip, and `dyn Clipboard` polymorphism.
    //!
    //! R56.2.e §5.22 — selection-aware regression battery. Covers
    //! the dual-buffer isolation contract (Clipboard vs Primary
    //! independence), `Default` selection alias, and the trait
    //! default impl semantics (no-op `Primary` for non-Linux
    //! platforms).

    use super::{Clipboard, ClipboardSelection, InMemoryClipboard};

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

    // R56.2.e §5.22 — selection-aware battery

    #[test]
    fn r56_2_e_default_selection_is_clipboard() {
        // The W3C surface assumes Clipboard when no selection is
        // named; ClipboardSelection::default() must match that
        // assumption so callers using `..Default::default()` get the
        // expected channel.
        assert_eq!(ClipboardSelection::default(), ClipboardSelection::Clipboard);
    }

    #[test]
    fn r56_2_e_in_memory_dual_buffer_isolation() {
        // Writing to Clipboard must never bleed into Primary and
        // vice versa — independent buffers like the X11 PRIMARY /
        // CLIPBOARD atoms.
        let cb = InMemoryClipboard::new();
        cb.copy_to(ClipboardSelection::Clipboard, "clip".to_string());
        cb.copy_to(ClipboardSelection::Primary, "prim".to_string());
        assert_eq!(
            cb.paste_from(ClipboardSelection::Clipboard),
            Some("clip".to_string()),
        );
        assert_eq!(
            cb.paste_from(ClipboardSelection::Primary),
            Some("prim".to_string()),
        );
    }

    #[test]
    fn r56_2_e_in_memory_copy_to_clipboard_matches_legacy_copy() {
        // The selection-aware `copy_to(Clipboard, _)` and the legacy
        // `copy(_)` must touch the same buffer so a W3C-style
        // caller and a selection-aware caller observe the same
        // payload.
        let cb = InMemoryClipboard::new();
        cb.copy_to(ClipboardSelection::Clipboard, "legacy".to_string());
        assert_eq!(cb.paste(), Some("legacy".to_string()));

        let cb2 = InMemoryClipboard::new();
        cb2.copy("modern".to_string());
        assert_eq!(
            cb2.paste_from(ClipboardSelection::Clipboard),
            Some("modern".to_string()),
        );
    }

    #[test]
    fn r56_2_e_in_memory_primary_fresh_returns_none() {
        // A fresh InMemoryClipboard has neither Clipboard nor
        // Primary written; both reads return None until a copy_to
        // lands.
        let cb = InMemoryClipboard::new();
        assert_eq!(cb.paste_from(ClipboardSelection::Clipboard), None);
        assert_eq!(cb.paste_from(ClipboardSelection::Primary), None);
    }

    #[test]
    fn r56_2_e_trait_default_primary_is_no_op() {
        // A Clipboard impl that only overrides `copy` / `paste`
        // (the W3C subset) must get the default `Primary` no-op
        // behaviour: write is silently dropped, read returns None.
        // This mirrors the macOS / Windows / browser fallback.
        struct WriteOnlyCb {
            buf: RefCell<Option<String>>,
        }
        impl Clipboard for WriteOnlyCb {
            fn copy(&self, text: String) {
                *self.buf.borrow_mut() = Some(text);
            }
            fn paste(&self) -> Option<String> {
                self.buf.borrow().clone()
            }
        }

        use std::cell::RefCell;
        let cb = WriteOnlyCb {
            buf: RefCell::new(None),
        };

        // Primary write goes to /dev/null (default no-op).
        cb.copy_to(ClipboardSelection::Primary, "lost".to_string());
        assert_eq!(cb.paste_from(ClipboardSelection::Primary), None);
        // The W3C clipboard is untouched.
        assert_eq!(cb.paste(), None);

        // Clipboard via copy_to still threads through the override.
        cb.copy_to(ClipboardSelection::Clipboard, "kept".to_string());
        assert_eq!(cb.paste(), Some("kept".to_string()));
    }

    #[test]
    fn r56_2_e_primary_overwrite_clears_prior_primary() {
        // Repeated copy_to(Primary, _) overwrites — Linux PRIMARY
        // is single-slot at the X selection level.
        let cb = InMemoryClipboard::new();
        cb.copy_to(ClipboardSelection::Primary, "first".to_string());
        cb.copy_to(ClipboardSelection::Primary, "second".to_string());
        assert_eq!(
            cb.paste_from(ClipboardSelection::Primary),
            Some("second".to_string()),
        );
    }

    #[test]
    fn r56_2_e_selection_enum_is_copy_and_default() {
        // ClipboardSelection is small enough to be Copy + Default
        // so callers can pass by value freely and use
        // `..Default::default()` in struct literals.
        let s = ClipboardSelection::Primary;
        let s_copy = s; // Copy
        assert_eq!(s, s_copy);
        let _: ClipboardSelection = ClipboardSelection::default();
    }
}
