//! `text/normalize` RPC method (§5.37.2, R50.2.X).
//!
//! Stateless typed wrapper over [`pinion_text_unicode::normalize`].
//! No registry needed — Unicode normalization is a pure function of
//! `(text, form)` over a build-time-pinned UCD 16.0.0 table set.
//!
//! Wire shape:
//! ```text
//! Request : { "method": "text/normalize",
//!             "params": { "text": "<string>", "form": "NFC|NFD|NFKC|NFKD" } }
//! Response: { "result": { "text": "<normalized>" } }
//! ```
//!
//! The form selector uses the canonical Unicode upper-case
//! abbreviation on the wire (`NFC`/`NFD`/`NFKC`/`NFKD`) while the
//! Rust enum sticks to idiomatic Rust casing — [`serde`] bridges via
//! per-variant `rename`.

use pinion_text_unicode::{NormForm, normalize};
use serde::{Deserialize, Serialize};

/// Wire-shape selector for the requested normalization form.
///
/// Serialized as `"NFC"` / `"NFD"` / `"NFKC"` / `"NFKD"` to match
/// the canonical Unicode abbreviations expected by AI agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum NormalizeForm {
    /// Canonical Composition.
    #[serde(rename = "NFC")]
    Nfc,
    /// Canonical Decomposition.
    #[serde(rename = "NFD")]
    Nfd,
    /// Compatibility Composition.
    #[serde(rename = "NFKC")]
    Nfkc,
    /// Compatibility Decomposition.
    #[serde(rename = "NFKD")]
    Nfkd,
}

impl From<NormalizeForm> for NormForm {
    fn from(f: NormalizeForm) -> Self {
        match f {
            NormalizeForm::Nfc => NormForm::Nfc,
            NormalizeForm::Nfd => NormForm::Nfd,
            NormalizeForm::Nfkc => NormForm::Nfkc,
            NormalizeForm::Nfkd => NormForm::Nfkd,
        }
    }
}

/// Outcome of [`text_normalize`]: the normalized text. The
/// underlying `Cow` borrow is collapsed to `String` here because the
/// JSON wire envelope owns the value anyway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NormalizeOutcome {
    pub text: String,
}

/// Normalize `text` to the requested Unicode form. Stateless; UAX
/// #15 conformant against UCD 16.0.0. See
/// [`pinion_text_unicode::normalize`] for algorithm details and
/// performance characteristics (Quick-check fast path, `O(n)`
/// compose-write).
#[must_use]
pub fn text_normalize(text: &str, form: NormalizeForm) -> NormalizeOutcome {
    NormalizeOutcome {
        text: normalize(text, form.into()).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{NormalizeForm, text_normalize};

    #[test]
    fn ascii_round_trip_all_forms() {
        let s = "Hello, world!";
        for form in [
            NormalizeForm::Nfc,
            NormalizeForm::Nfd,
            NormalizeForm::Nfkc,
            NormalizeForm::Nfkd,
        ] {
            assert_eq!(text_normalize(s, form).text, s);
        }
    }

    #[test]
    fn nfc_recomposes_decomposed() {
        // A + combining grave → À.
        let out = text_normalize("A\u{0300}", NormalizeForm::Nfc);
        assert_eq!(out.text, "\u{00C0}");
    }

    #[test]
    fn nfd_decomposes_precomposed() {
        // À → A + combining grave.
        let out = text_normalize("\u{00C0}", NormalizeForm::Nfd);
        assert_eq!(out.text, "A\u{0300}");
    }

    #[test]
    fn nfkd_strips_compatibility_decomposition() {
        // ﬁ ligature → "fi".
        let out = text_normalize("\u{FB01}", NormalizeForm::Nfkd);
        assert_eq!(out.text, "fi");
    }

    #[test]
    fn nfkc_strips_compat_then_no_recompose() {
        // ² superscript → "2"; no primary composite, stays "2".
        let out = text_normalize("\u{00B2}", NormalizeForm::Nfkc);
        assert_eq!(out.text, "2");
    }

    #[test]
    fn hangul_jamo_compose_to_syllable() {
        // ᄒ ᅡ ᆫ → 한 via algorithmic Hangul composition.
        let out = text_normalize("\u{1112}\u{1161}\u{11AB}", NormalizeForm::Nfc);
        assert_eq!(out.text, "\u{D55C}");
    }

    #[test]
    fn hangul_syllable_decomposes_algorithmically() {
        // 한 → ᄒ ᅡ ᆫ via UAX #15 §16.
        let out = text_normalize("\u{D55C}", NormalizeForm::Nfd);
        assert_eq!(out.text, "\u{1112}\u{1161}\u{11AB}");
    }

    #[test]
    fn idempotent_per_form() {
        let s = "caf\u{00E9} \u{1E0A} \u{D55C}";
        for form in [
            NormalizeForm::Nfc,
            NormalizeForm::Nfd,
            NormalizeForm::Nfkc,
            NormalizeForm::Nfkd,
        ] {
            let once = text_normalize(s, form).text;
            let twice = text_normalize(&once, form).text;
            assert_eq!(once, twice);
        }
    }

    #[test]
    fn form_serde_uppercase_wire() {
        // The enum serialises as the canonical Unicode upper-case
        // abbreviation (wire format expected by AI agents).
        let nfc = serde_json::to_string(&NormalizeForm::Nfc).unwrap();
        assert_eq!(nfc, "\"NFC\"");
        let nfkd: NormalizeForm = serde_json::from_str("\"NFKD\"").unwrap();
        assert_eq!(nfkd, NormalizeForm::Nfkd);
    }
}
