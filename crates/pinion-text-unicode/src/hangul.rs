//! UAX #15 §16 — Hangul algorithmic decomposition and composition.
//!
//! Hangul precomposed syllables (`S = U+AC00..U+D7A3`, exactly 11172
//! syllables) are deliberately absent from the UCD decomposition
//! tables. The Unicode Standard §3.12 defines a closed-form
//! algorithm that derives the L (leading consonant), V (vowel), and
//! optional T (trailing consonant) jamo for any syllable from a few
//! base constants. The inverse algorithm (used by NFC composition
//! in R50.2.4+) is also defined here.

/// Hangul syllable base (`S_BASE`), `U+AC00` "가".
pub(crate) const S_BASE: u32 = 0xAC00;
/// Hangul leading-consonant (L) base, `U+1100` "ᄀ".
pub(crate) const L_BASE: u32 = 0x1100;
/// Hangul vowel (V) base, `U+1161` "ᅡ".
pub(crate) const V_BASE: u32 = 0x1161;
/// Hangul trailing-consonant (T) offset base, `U+11A7` (T-offset 0
/// reserved for "no trailing consonant").
pub(crate) const T_BASE: u32 = 0x11A7;

pub(crate) const L_COUNT: u32 = 19;
pub(crate) const V_COUNT: u32 = 21;
pub(crate) const T_COUNT: u32 = 28;
/// Number of distinct (V, T) combinations per L.
pub(crate) const N_COUNT: u32 = V_COUNT * T_COUNT;
/// Total Hangul precomposed syllable count.
pub(crate) const S_COUNT: u32 = L_COUNT * N_COUNT;

/// `true` iff `c` is a precomposed Hangul syllable.
pub(crate) fn is_hangul_syllable(c: u32) -> bool {
    (S_BASE..S_BASE + S_COUNT).contains(&c)
}

/// Decompose a precomposed Hangul syllable into its L, V, and
/// optional T jamo (UAX #15 §16). Returns `true` iff `c` was a
/// Hangul syllable and decomposition was emitted; `false` leaves
/// `out` unchanged.
pub(crate) fn decompose_hangul_syllable(c: u32, out: &mut Vec<u32>) -> bool {
    if !is_hangul_syllable(c) {
        return false;
    }
    let s_index = c - S_BASE;
    let l = L_BASE + s_index / N_COUNT;
    let v = V_BASE + (s_index % N_COUNT) / T_COUNT;
    let t_offset = s_index % T_COUNT;
    out.push(l);
    out.push(v);
    if t_offset != 0 {
        out.push(T_BASE + t_offset);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{
        decompose_hangul_syllable, is_hangul_syllable, L_BASE, S_BASE,
        S_COUNT, T_BASE, V_BASE,
    };

    #[test]
    fn syllable_range_boundaries() {
        assert!(!is_hangul_syllable(S_BASE - 1));
        assert!(is_hangul_syllable(S_BASE));
        assert!(is_hangul_syllable(S_BASE + S_COUNT - 1));
        assert!(!is_hangul_syllable(S_BASE + S_COUNT));
    }

    #[test]
    fn ga_decomposes_to_l_v_no_trailing() {
        // 가 (U+AC00) = L base + V base (T-offset 0, no trailing).
        let mut out = Vec::new();
        assert!(decompose_hangul_syllable(0xAC00, &mut out));
        assert_eq!(out, vec![L_BASE, V_BASE]);
    }

    #[test]
    fn han_decomposes_to_l_v_t() {
        // 한 (U+D55C) = ᄒ (U+1112) + ᅡ (U+1161) + ᆫ (U+11AB).
        let mut out = Vec::new();
        assert!(decompose_hangul_syllable(0xD55C, &mut out));
        assert_eq!(out, vec![0x1112, 0x1161, 0x11AB]);
    }

    #[test]
    fn hih_decomposes_with_trailing() {
        // 힣 (U+D7A3) = last syllable = ᄒ ᅵ ᇂ.
        let mut out = Vec::new();
        assert!(decompose_hangul_syllable(0xD7A3, &mut out));
        assert_eq!(out, vec![0x1112, 0x1175, T_BASE + 27]);
    }

    #[test]
    fn non_hangul_leaves_buffer_untouched() {
        let mut out = vec![0xFFFF];
        assert!(!decompose_hangul_syllable(0x0041, &mut out));
        assert_eq!(out, vec![0xFFFF]);
    }
}
