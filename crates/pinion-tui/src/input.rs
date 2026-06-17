//! R51.111 / R51.112 §5.41 — crossterm → pinion input conversion.
//!
//! Bridges the crossterm `KeyEvent` / `MouseEvent` vocabulary to the
//! substrate-internal shape (`pinion_runtime::Modifiers` plus the W3C
//! `KeyboardEvent.key` strings the `WidgetView::apply_key`-style hooks
//! match against). Mirrors `pinion-shell::app::winit_modifiers_to_pinion`
//! / `pinion-shell::app::named_key_str` — the substrate sees only the
//! abstract vocabulary so a future second TUI binding can swap the
//! input source without touching the dispatch substrate.
//!
//! ## Why the W3C name strings
//!
//! Every existing widget binding (Vello-side `WidgetView::apply_key`)
//! already matches against `"ArrowLeft"` / `"Home"` / `"Space"` /
//! `"Enter"`, etc. — the strings W3C `KeyboardEvent.key` defines and
//! WAI-ARIA Authoring Practices specify for keyboard affordances. The
//! TUI side reuses the same vocabulary so a single `apply_key` impl on
//! a widget binding handles both backends; the only divergence is
//! where the string comes from (`crossterm::event::KeyCode` here,
//! `winit::keyboard::Key` on the Vello side).
//!
//! ## Reserved keys
//!
//! `KeyCode::Esc` returns `None` — the TUI shell handles it directly
//! as the exit signal (mirrors the Vello shell's `Escape → window
//! close` convention). `KeyCode::Tab` and `KeyCode::BackTab` also
//! return `None` for the same reason: focus traversal is shell-side
//! per §5.39 R51.53. Future R51.112+ wires Tab into a TUI
//! `FocusManager` once a second focusable TUI binding lands.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use pinion_runtime::Modifiers;

use pinion_core::CellMetric;

/// R51.111 §5.41 — convert a crossterm `KeyEvent` into the W3C
/// `KeyboardEvent.key` string the `WidgetViewTui::apply_key` /
/// `keybinding` contracts speak.
///
/// Returns `None` for keys the substrate intentionally absorbs
/// (`Esc`, `Tab`, `BackTab`) or for keys without a stable cross-platform
/// widget meaning (`Null`, `CapsLock`, `ScrollLock`, `NumLock`,
/// `PrintScreen`, `Pause`, `Menu`, `KeypadBegin`, `Media`, `Modifier`).
///
/// Character keys round-trip through their `char` representation
/// except for `' '`, which becomes `"Space"` to match the W3C
/// vocabulary every existing `WidgetView::apply_key` impl matches
/// against (the `winit::keyboard::NamedKey::Space → "Space"` path on
/// the Vello side normalises the same way).
#[must_use]
pub fn key_str_from_event(event: &KeyEvent) -> Option<String> {
    match event.code {
        KeyCode::Char(' ') => Some("Space".to_string()),
        KeyCode::Char(c) => Some(c.to_string()),
        KeyCode::Enter => Some("Enter".to_string()),
        KeyCode::Left => Some("ArrowLeft".to_string()),
        KeyCode::Right => Some("ArrowRight".to_string()),
        KeyCode::Up => Some("ArrowUp".to_string()),
        KeyCode::Down => Some("ArrowDown".to_string()),
        KeyCode::Home => Some("Home".to_string()),
        KeyCode::End => Some("End".to_string()),
        KeyCode::PageUp => Some("PageUp".to_string()),
        KeyCode::PageDown => Some("PageDown".to_string()),
        KeyCode::Backspace => Some("Backspace".to_string()),
        KeyCode::Delete => Some("Delete".to_string()),
        KeyCode::Insert => Some("Insert".to_string()),
        KeyCode::F(n) => Some(format!("F{n}")),
        // Shell-reserved or no stable widget meaning.
        KeyCode::Esc
        | KeyCode::Tab
        | KeyCode::BackTab
        | KeyCode::Null
        | KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Media(_)
        | KeyCode::Modifier(_) => None,
    }
}

/// R51.111 §5.41 — convert crossterm `KeyModifiers` to the
/// substrate-local [`Modifiers`] at the crossterm boundary so the
/// shell stays input-source-agnostic.
///
/// The four W3C DOM Level 3 modifier bits map 1:1 (crossterm's `SUPER`
/// is the Meta / Cmd / Win key in the abstract vocabulary, matching
/// winit's `super_key`). `HYPER` is collapsed onto `meta` because the
/// abstract vocabulary has no separate slot (no widget binding in
/// pinion's catalogue branches on Hyper).
#[must_use]
pub fn modifiers_from_crossterm(modifiers: KeyModifiers) -> Modifiers {
    Modifiers {
        shift: modifiers.contains(KeyModifiers::SHIFT),
        ctrl: modifiers.contains(KeyModifiers::CONTROL),
        alt: modifiers.contains(KeyModifiers::ALT),
        meta: modifiers.contains(KeyModifiers::SUPER)
            || modifiers.contains(KeyModifiers::META)
            || modifiers.contains(KeyModifiers::HYPER),
    }
}

/// R51.112 §5.41 — convert a crossterm mouse cell coordinate
/// `(column, row)` into the abstract pixel `(x, y)` the substrate's
/// [`pinion_runtime::input::InputRouter`] expects.
///
/// The substrate is pixel-coord native (DPI-aware logical pixels,
/// same axis the Vello shell's winit `CursorMoved` reports). The TUI
/// path maps cell coords through [`CellMetric`] (R968 §5.41) — the
/// same metric [`crate::paint`] uses for the inverse direction — so
/// `Scene::Container.rect` hit-tests align with the visible cell
/// geometry. It renders against [`CellMetric::DEFAULT`] (8×16),
/// matching the pre-R968 `PIXEL_PER_CELL_*` behaviour. When a future
/// `Scene::TextGrid` makes the substrate generic over coord units,
/// this helper folds away.
#[must_use]
pub fn cell_to_pixel(column: u16, row: u16) -> (f64, f64) {
    CellMetric::DEFAULT.cell_to_px(column, row)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn ev(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn space_char_maps_to_w3c_space_string() {
        // R51.111 — `KeyCode::Char(' ')` must become `"Space"`, not
        // a single space character, so widget `apply_key` impls
        // matching against W3C `"Space"` work across both backends.
        assert_eq!(key_str_from_event(&ev(KeyCode::Char(' '))).as_deref(), Some("Space"));
    }

    #[test]
    fn enter_key_maps_to_w3c_enter() {
        assert_eq!(key_str_from_event(&ev(KeyCode::Enter)).as_deref(), Some("Enter"));
    }

    #[test]
    fn arrow_keys_map_to_w3c_strings() {
        // ARIA Authoring Practices Slider / Listbox / Tabs all match
        // against `"ArrowLeft"`-style names; the TUI path must agree.
        assert_eq!(key_str_from_event(&ev(KeyCode::Left)).as_deref(), Some("ArrowLeft"));
        assert_eq!(key_str_from_event(&ev(KeyCode::Right)).as_deref(), Some("ArrowRight"));
        assert_eq!(key_str_from_event(&ev(KeyCode::Up)).as_deref(), Some("ArrowUp"));
        assert_eq!(key_str_from_event(&ev(KeyCode::Down)).as_deref(), Some("ArrowDown"));
    }

    #[test]
    fn navigation_keys_map_to_w3c_strings() {
        assert_eq!(key_str_from_event(&ev(KeyCode::Home)).as_deref(), Some("Home"));
        assert_eq!(key_str_from_event(&ev(KeyCode::End)).as_deref(), Some("End"));
        assert_eq!(key_str_from_event(&ev(KeyCode::PageUp)).as_deref(), Some("PageUp"));
        assert_eq!(key_str_from_event(&ev(KeyCode::PageDown)).as_deref(), Some("PageDown"));
    }

    #[test]
    fn character_keys_round_trip_through_their_glyph() {
        // Letter keys (no modifier) — apply_key impls speak the literal
        // glyph, e.g. `"d"` for the Vello hello-button's Disable keybinding.
        assert_eq!(key_str_from_event(&ev(KeyCode::Char('d'))).as_deref(), Some("d"));
        assert_eq!(key_str_from_event(&ev(KeyCode::Char('A'))).as_deref(), Some("A"));
    }

    #[test]
    fn function_keys_map_to_fn_names() {
        assert_eq!(key_str_from_event(&ev(KeyCode::F(1))).as_deref(), Some("F1"));
        assert_eq!(key_str_from_event(&ev(KeyCode::F(12))).as_deref(), Some("F12"));
    }

    #[test]
    fn escape_returns_none_shell_reserved() {
        // `Esc` is shell-reserved (TUI exit signal); the substrate
        // never routes it to widgets.
        assert!(key_str_from_event(&ev(KeyCode::Esc)).is_none());
    }

    #[test]
    fn tab_returns_none_shell_reserved() {
        // `Tab` / `BackTab` are shell-reserved for the future
        // `FocusManager` carry (R51.112+).
        assert!(key_str_from_event(&ev(KeyCode::Tab)).is_none());
        assert!(key_str_from_event(&ev(KeyCode::BackTab)).is_none());
    }

    #[test]
    fn unmapped_keys_return_none() {
        // Keys without a stable cross-platform widget meaning fall
        // through to `None` so the shell stays silent on them.
        assert!(key_str_from_event(&ev(KeyCode::Null)).is_none());
        assert!(key_str_from_event(&ev(KeyCode::CapsLock)).is_none());
    }

    #[test]
    fn modifier_conversion_maps_all_four_bits() {
        let m = modifiers_from_crossterm(
            KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
        );
        assert!(m.shift && m.ctrl && m.alt && m.meta);
    }

    #[test]
    fn modifier_conversion_empty_is_no_modifiers() {
        let m = modifiers_from_crossterm(KeyModifiers::NONE);
        assert!(!m.shift && !m.ctrl && !m.alt && !m.meta);
    }

    #[test]
    fn modifier_conversion_hyper_collapses_to_meta() {
        // The abstract `Modifiers` vocabulary has no Hyper slot;
        // crossterm's HYPER bit folds into `meta` so the substrate
        // sees a single Meta / Cmd / Win signal.
        let m = modifiers_from_crossterm(KeyModifiers::HYPER);
        assert!(m.meta);
    }

    #[test]
    fn modifier_conversion_meta_bit_also_maps_to_meta() {
        // crossterm distinguishes SUPER + META + HYPER as separate
        // bits; all three collapse to abstract `meta` because no
        // widget in pinion's catalogue branches finer-grained.
        let m = modifiers_from_crossterm(KeyModifiers::META);
        assert!(m.meta);
    }

    #[test]
    fn cell_to_pixel_origin_is_zero() {
        // R51.112 — cell (0, 0) → pixel (0.0, 0.0). The substrate's
        // pixel coord space shares the top-left origin with the
        // terminal's cell coord space; no axis flip.
        assert_eq!(cell_to_pixel(0, 0), (0.0, 0.0));
    }

    #[test]
    fn cell_to_pixel_scales_by_pixel_per_cell_constants() {
        // R51.112 — cell (3, 2) → pixel (24, 32) under the 8×16
        // placeholder. Matches the inverse direction in
        // `paint::to_buffer` so a `Scene::Container.rect` of
        // pixel (24, 32) hit-tests the cell at (3, 2) exactly.
        assert_eq!(cell_to_pixel(3, 2), (24.0, 32.0));
    }

    #[test]
    fn cell_to_pixel_far_corner_does_not_overflow() {
        // R51.112 — u16::MAX cell coord widens into f64 without
        // truncation: 65535 * 16 = 1_048_560, well within f64 mantissa.
        let (x, y) = cell_to_pixel(u16::MAX, u16::MAX);
        assert!((x - 524_280.0).abs() < f64::EPSILON);
        assert!((y - 1_048_560.0).abs() < f64::EPSILON);
    }
}
