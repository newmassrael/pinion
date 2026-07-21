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
//! ## R56.2.e §5.22 — Linux PRIMARY selection cascade
//!
//! On Linux (X11 / Wayland) the `Clipboard` trait's selection-aware
//! [`copy_to`](pinion_core::Clipboard::copy_to) /
//! [`paste_from`](pinion_core::Clipboard::paste_from) methods are
//! overridden to thread `Primary` through `arboard`'s
//! [`SetExtLinux::clipboard`](arboard::SetExtLinux::clipboard) / [`GetExtLinux::clipboard`](arboard::GetExtLinux::clipboard) trait
//! extensions, addressing the X11 PRIMARY selection atom / Wayland
//! `zwlr_data_control_v1` PRIMARY channel. The `TextField` widget
//! publishes selections to PRIMARY automatically (R56.2.e.2) and the
//! shell's middle-click handler reads from PRIMARY into the focused
//! field (R56.2.e.3), so cross-process X11/Wayland middle-click
//! paste interop comes online without further opt-in.
//!
//! On macOS / Windows / browser targets the cfg-gated override is
//! absent and the trait default takes over (`Primary` becomes a
//! no-op write / `None` read), matching the OS convention where
//! no parallel selection clipboard exists.
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
use std::rc::Rc;

use pinion_core::reactive::Owner;
use pinion_core::{Clipboard, ClipboardSelection, InMemoryClipboard};

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
        f.debug_struct("ArboardClipboard").finish_non_exhaustive()
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

    /// R56.2.e §5.22 — selection-aware write. On Linux (X11 / Wayland)
    /// the `Primary` arm routes through `arboard`'s
    /// [`SetExtLinux::clipboard`](arboard::SetExtLinux::clipboard) trait extension to address the X11
    /// PRIMARY selection / Wayland `zwlr_data_control_v1` PRIMARY
    /// channel. On other platforms the override is absent (cfg
    /// disabled) so the trait default takes over — `Primary` becomes
    /// a no-op write, matching the macOS / Windows / browser
    /// convention where no parallel "selection clipboard" exists.
    ///
    /// The Linux override does *not* set a wait window
    /// (`WaitConfig::default() == None`); X clients that read PRIMARY
    /// while pinion is still the selection owner observe the latest
    /// write immediately. If pinion exits before another app reads
    /// the selection the X clipboard manager (`clipmanager`, etc.)
    /// is expected to take ownership for CLIPBOARD; PRIMARY is
    /// session-scoped by X11 convention and is allowed to be lost.
    #[cfg(all(
        unix,
        not(any(
            target_os = "macos",
            target_os = "android",
            target_os = "ios",
            target_os = "emscripten",
        )),
    ))]
    fn copy_to(&self, selection: ClipboardSelection, text: String) {
        use arboard::{LinuxClipboardKind, SetExtLinux};
        let Ok(mut clipboard) = self.inner.try_borrow_mut() else {
            return;
        };
        match selection {
            ClipboardSelection::Clipboard => {
                let _ = clipboard.set_text(text);
            }
            ClipboardSelection::Primary => {
                let _ = clipboard
                    .set()
                    .clipboard(LinuxClipboardKind::Primary)
                    .text(text);
            }
            // R56.2.e §5.22 — `ClipboardSelection` is `#[non_exhaustive]`
            // for forward-compat with future variants (X11 SECONDARY,
            // GTK find-buffer, etc.). A wildcard arm is required by
            // the compiler from this crate's perspective; we map
            // unknown variants to no-op so a future-variant payload
            // does not silently corrupt the CLIPBOARD selection.
            _ => {}
        }
    }

    /// R56.2.e §5.22 — selection-aware read. On Linux the `Primary`
    /// arm routes through `arboard`'s [`GetExtLinux::clipboard`](arboard::GetExtLinux::clipboard)
    /// trait extension to read the X11 PRIMARY selection / Wayland
    /// PRIMARY channel. On other platforms the override is absent
    /// (cfg disabled) so the trait default returns `None` for
    /// `Primary` (macOS / Windows / browser have no equivalent
    /// surface).
    ///
    /// Returns `None` when the selection is empty, holds non-text
    /// content, or the OS clipboard daemon errors — the substrate's
    /// `paste`-style total trait contract is preserved per-selection.
    #[cfg(all(
        unix,
        not(any(
            target_os = "macos",
            target_os = "android",
            target_os = "ios",
            target_os = "emscripten",
        )),
    ))]
    fn paste_from(&self, selection: ClipboardSelection) -> Option<String> {
        use arboard::{GetExtLinux, LinuxClipboardKind};
        let mut clipboard = self.inner.try_borrow_mut().ok()?;
        match selection {
            ClipboardSelection::Clipboard => clipboard.get_text().ok(),
            ClipboardSelection::Primary => clipboard
                .get()
                .clipboard(LinuxClipboardKind::Primary)
                .text()
                .ok(),
            // R56.2.e §5.22 — see [`Self::copy_to`] for the
            // `non_exhaustive` rationale; unknown future variants
            // return `None` to keep the contract total.
            _ => None,
        }
    }
}

/// R790 §5.22 — `Owner::cache`-keyed clipboard hook shared by every
/// `TextField`-bearing binding (`hello-textfield`, `todomvc`,
/// `hello-textarea`, `settings-panel`, the modal file-save dialog, …).
///
/// Prefers the platform-backed [`ArboardClipboard`] (Wayland
/// `wl_data_device` + X11 CLIPBOARD/PRIMARY + macOS `NSPasteboard` +
/// Windows `OpenClipboard`) and falls back to [`InMemoryClipboard`] on
/// init failure (headless CI, sandboxed display-less container, broken
/// Wayland socket). The fallback keeps the keyboard-shortcut UX
/// functional (Ctrl/Cmd+C → Ctrl/Cmd+V round-trip within the running
/// process) at the cost of cross-process clipboard sharing.
///
/// The handle is parked in the [`Owner::cache`] slot keyed by `key`, so
/// the External's `attach_clipboard` and any later view-fn read resolve
/// to the same `Rc<dyn Clipboard>` instance — the tag-keyed singleton
/// shape mirroring `use_text_edit_state` / `use_caret_blink`.
///
/// ## Why this lives here (R790 lift)
///
/// Before R790 each text-field binding carried a byte-identical copy of
/// this hook plus an `AppClipboard` `Sized` wrapper. Those copies
/// forwarded only `copy` / `paste`, so the selection-aware `copy_to` /
/// `paste_from` (X11 / Wayland PRIMARY publish + middle-click paste) hit
/// the [`Clipboard`] trait **no-op default** on the wrapper instead of
/// reaching the inner [`ArboardClipboard`]'s Linux PRIMARY override —
/// PRIMARY was silently swallowed for every consumer. The lifted
/// `AppClipboard` below forwards all four methods, so the lift both
/// removes the 3-copy duplication and fixes the PRIMARY regression in
/// one place.
///
/// # Panics
/// Panics when called outside an active [`Owner`] scope (the hook needs
/// a reactive cache to dedup; every pinion view-fn / `create_external`
/// runs inside `root_owner.run`).
#[must_use]
pub fn use_app_clipboard(key: &'static str) -> Rc<dyn Clipboard> {
    let cb: Rc<AppClipboard> = Owner::current()
        .expect("use_app_clipboard requires an active Owner scope")
        .cache(key, || {
            AppClipboard(match ArboardClipboard::try_new() {
                Ok(arboard) => Box::new(arboard) as Box<dyn Clipboard>,
                Err(e) => {
                    eprintln!(
                        "pinion: ArboardClipboard init failed ({e}); falling back \
                         to InMemoryClipboard (cross-process clipboard disabled)",
                    );
                    Box::new(InMemoryClipboard::new()) as Box<dyn Clipboard>
                }
            })
        });
    cb
}

/// R790 §5.22 — `Sized` newtype around `Box<dyn Clipboard>` so the
/// [`Owner::cache`]`<V>` slot can park either an [`ArboardClipboard`]
/// (the common case) or an [`InMemoryClipboard`] (headless fallback)
/// inside one concrete `V`. The runtime impl choice hides inside the
/// box; downstream consumers receive the `Rc<dyn Clipboard>` shape.
///
/// All four [`Clipboard`] methods forward to the inner impl — crucially
/// the selection-aware `copy_to` / `paste_from`, so a wrapped
/// `ArboardClipboard`'s Linux PRIMARY override survives the wrapper
/// (the pre-R790 per-binding copies forwarded only `copy` / `paste`,
/// dropping PRIMARY to the trait no-op default).
struct AppClipboard(Box<dyn Clipboard>);

impl Clipboard for AppClipboard {
    fn copy(&self, text: String) {
        self.0.copy(text);
    }
    fn paste(&self) -> Option<String> {
        self.0.paste()
    }
    fn copy_to(&self, selection: ClipboardSelection, text: String) {
        self.0.copy_to(selection, text);
    }
    fn paste_from(&self, selection: ClipboardSelection) -> Option<String> {
        self.0.paste_from(selection)
    }
}

/// R1407 §5.22 — install `clipboard` as the [`use_app_clipboard`] handle for
/// `key`, returning it. **Seed-if-absent**: parks the impl in the very
/// [`Owner::cache`] slot `use_app_clipboard(key)` reads (same `AppClipboard`
/// type + `key`), so a later `use_app_clipboard(key)` in the SAME [`Owner`]
/// scope resolves to THIS instance instead of building an [`ArboardClipboard`].
/// If the slot is already populated it is a no-op that returns the existing
/// handle — so seed BEFORE the first `use_app_clipboard(key)` in the scope.
///
/// The dependency-inversion seam for the app clipboard. Its forcing consumer is
/// the keyboard-copy path: a `Ctrl`/`Cmd`+C handler writes through
/// `use_app_clipboard(TAG).copy(payload)`, whose OS write cannot be asserted in
/// a test without touching — and racing — the real clipboard
/// (the concurrent-clipboard-race hazard). A test seeds an
/// [`InMemoryClipboard`] here, drives a real
/// `Ctrl+C` through the binding's `apply_key`, and reads the copied bytes back
/// from the returned handle — the copy wiring verified end-to-end, no OS
/// clipboard touched. A headless / embedded shell can equally install a chosen
/// impl.
///
/// # Panics
/// Panics when called outside an active [`Owner`] scope (mirror of
/// [`use_app_clipboard`]).
#[must_use]
pub fn seed_app_clipboard(key: &'static str, clipboard: Box<dyn Clipboard>) -> Rc<dyn Clipboard> {
    let cb: Rc<AppClipboard> = Owner::current()
        .expect("seed_app_clipboard requires an active Owner scope")
        .cache(key, move || AppClipboard(clipboard));
    cb
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
    #[cfg(all(
        unix,
        not(any(
            target_os = "macos",
            target_os = "android",
            target_os = "ios",
            target_os = "emscripten",
        )),
    ))]
    use pinion_core::{Clipboard, ClipboardSelection};

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

    /// R56.2.e §5.22 — Linux PRIMARY round-trip smoke. Skipped on
    /// platforms where the cfg-gated override is absent (the trait
    /// default returns `None` for `Primary`, which is the correct
    /// macOS / Windows behaviour — not a regression). Also skipped
    /// when `arboard::Clipboard::new()` fails (headless CI, broken
    /// socket). When the display is reachable we write a unique
    /// token to PRIMARY and verify the read returns it; this
    /// confirms the `GetExtLinux` / `SetExtLinux` extension wires
    /// reach the same selection.
    ///
    /// `Primary` is X11/Wayland session-scoped — other X clients may
    /// race with this test, but the unique token avoids false
    /// positives (a foreign selection would not match the token).
    #[cfg(all(
        unix,
        not(any(
            target_os = "macos",
            target_os = "android",
            target_os = "ios",
            target_os = "emscripten",
        )),
    ))]
    #[test]
    fn r56_2_e_linux_primary_round_trip_when_display_present() {
        let Ok(cb) = ArboardClipboard::try_new() else {
            // Headless CI / broken display — the default impl path
            // is exercised by `pinion-core::clipboard::tests`. This
            // test is opportunistic: pass through on inaccessible
            // display.
            return;
        };
        // Unique token so a concurrent X client write does not
        // confuse the read-back check.
        let token = format!("pinion-r56_2_e-{}", std::process::id());
        cb.copy_to(ClipboardSelection::Primary, token.clone());
        let observed = cb.paste_from(ClipboardSelection::Primary);
        // Allow None when the compositor rejects PRIMARY (Wayland
        // protocol v1, sandbox restrictions); only assert mismatch
        // when a value came back.
        if let Some(text) = observed {
            assert_eq!(
                text, token,
                "PRIMARY round-trip returned a different payload",
            );
        }
    }

    /// R56.2.e §5.22 — verify the CLIPBOARD selection-aware path
    /// matches the legacy `copy` / `paste` path so callers can
    /// mix-and-match the two APIs against the same OS state without
    /// observing drift.
    #[cfg(all(
        unix,
        not(any(
            target_os = "macos",
            target_os = "android",
            target_os = "ios",
            target_os = "emscripten",
        )),
    ))]
    #[test]
    fn r56_2_e_linux_clipboard_selection_matches_legacy_when_display_present() {
        let Ok(cb) = ArboardClipboard::try_new() else {
            return;
        };
        let token = format!("pinion-r56_2_e-cb-{}", std::process::id());
        cb.copy_to(ClipboardSelection::Clipboard, token.clone());
        // The legacy `paste()` and the selection-aware
        // `paste_from(Clipboard)` must observe the same payload —
        // both route through `arboard::Clipboard::get_text` on
        // CLIPBOARD.
        let via_legacy = cb.paste();
        let via_selection = cb.paste_from(ClipboardSelection::Clipboard);
        match (via_legacy, via_selection) {
            (Some(a), Some(b)) => assert_eq!(a, b),
            (None, None) => {}
            (a, b) => {
                panic!("legacy vs selection paste shape mismatch: legacy={a:?} selection={b:?}",)
            }
        }
    }

    // ─────────────────────────────────────────────────────────────
    // R790 §5.22 — lifted `use_app_clipboard` hook + `AppClipboard`
    // wrapper (shared by the text-field bindings + the file-save dialog).
    // Nested module so its unconditional `Clipboard` / `ClipboardSelection`
    // imports do not clash with the cfg-gated PRIMARY-test imports above.
    // ─────────────────────────────────────────────────────────────
    mod r790_lift {
        use std::rc::Rc;

        use super::super::{AppClipboard, seed_app_clipboard, use_app_clipboard};
        use pinion_core::reactive::Owner;
        use pinion_core::{Clipboard, ClipboardSelection, InMemoryClipboard};

        /// R790 §5.22 — the wrapper must forward the *selection-aware*
        /// `copy_to` / `paste_from` to the inner impl. The pre-R790
        /// per-binding copies forwarded only `copy` / `paste`, so a wrapped
        /// `ArboardClipboard`'s Linux PRIMARY override fell through to the
        /// trait no-op default. Wrap an [`InMemoryClipboard`] (which keeps an
        /// independent PRIMARY buffer) and verify PRIMARY survives the
        /// wrapper, independent of the CLIPBOARD selection.
        #[test]
        fn r790_app_clipboard_forwards_selection_aware_methods() {
            let cb = AppClipboard(Box::new(InMemoryClipboard::new()));
            cb.copy_to(ClipboardSelection::Primary, "primary-token".to_owned());
            assert_eq!(
                cb.paste_from(ClipboardSelection::Primary),
                Some("primary-token".to_owned()),
                "PRIMARY write/read must reach the inner impl (not the no-op default)",
            );
            // CLIPBOARD selection stays independent of PRIMARY.
            cb.copy("clip-token".to_owned());
            assert_eq!(cb.paste(), Some("clip-token".to_owned()));
            assert_eq!(
                cb.paste_from(ClipboardSelection::Primary),
                Some("primary-token".to_owned()),
                "a CLIPBOARD write must not clobber the PRIMARY buffer",
            );
        }

        /// R790 §5.22 — the hook dedups per `Owner::cache` key: two calls
        /// with the same key resolve to one shared `Rc<dyn Clipboard>` so the
        /// External's `attach_clipboard` and a later view-fn read see the
        /// same instance. (Backend-agnostic: holds whether the real arboard
        /// handle or the `InMemory` fallback won.)
        #[test]
        fn r790_use_app_clipboard_dedups_per_key() {
            Owner::new().run(|| {
                let a = use_app_clipboard("test_field");
                let b = use_app_clipboard("test_field");
                assert!(Rc::ptr_eq(&a, &b), "same key resolves to one shared handle");
                let c = use_app_clipboard("other_field");
                assert!(!Rc::ptr_eq(&a, &c), "a distinct key is a distinct handle");
                // NB: deliberately no `copy`/`paste` here. When a real
                // arboard handle wins, a `copy` would write the *shared
                // system* clipboard and race the `r56_2_e_*` round-trip tests
                // running concurrently in this same binary. The dedup
                // contract (`Rc::ptr_eq`) is the pin; the copy/paste surface
                // is covered by `r790_app_clipboard_forwards_selection_aware_methods`
                // over a hermetic `InMemoryClipboard`.
            });
        }

        /// R1407 §5.22 — `seed_app_clipboard` installs a chosen impl into the
        /// slot `use_app_clipboard` reads, so a later `use_app_clipboard(key)`
        /// resolves the SAME instance (never building an arboard handle). This
        /// is the seam a keyboard-copy test uses to read back the bytes a
        /// `Ctrl+C` wrote WITHOUT racing the OS clipboard.
        #[test]
        fn r1407_seed_app_clipboard_is_returned_by_use_app_clipboard() {
            Owner::new().run(|| {
                let seeded = seed_app_clipboard("seed_key", Box::new(InMemoryClipboard::new()));
                seeded.copy("copied-via-Ctrl+C".to_owned());
                // The binding's own `use_app_clipboard(key)` resolves the
                // seeded instance (a hermetic InMemoryClipboard), so it never
                // touches — nor races — the real OS clipboard.
                let resolved = use_app_clipboard("seed_key");
                assert!(
                    Rc::ptr_eq(&seeded, &resolved),
                    "use_app_clipboard resolves the seeded handle",
                );
                assert_eq!(
                    resolved.paste(),
                    Some("copied-via-Ctrl+C".to_owned()),
                    "the copied bytes are readable back from the seeded clipboard",
                );
            });
        }

        /// R1407 §5.22 — seed is seed-IF-ABSENT: once a key is populated (here
        /// by `use_app_clipboard` first) a later `seed_app_clipboard` is a no-op
        /// returning the existing handle. So a test must seed BEFORE the first
        /// `use_app_clipboard(key)`.
        #[test]
        fn r1407_seed_after_use_is_a_no_op() {
            Owner::new().run(|| {
                let first = use_app_clipboard("late_seed");
                let seeded = seed_app_clipboard("late_seed", Box::new(InMemoryClipboard::new()));
                assert!(
                    Rc::ptr_eq(&first, &seeded),
                    "seeding an already-populated key returns the existing handle",
                );
            });
        }
    }
}
