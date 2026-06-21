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

/// Algorithmic composition of two adjacent jamo (UAX #15 §16). Two
/// shapes succeed:
///
/// * `(L, V) → LV` — leading consonant `L_BASE..L_COUNT` combined with
///   vowel `V_BASE..V_COUNT` produces the syllable with T-offset 0.
/// * `(LV, T) → LVT` — existing LV syllable (T-offset 0) combined
///   with trailing consonant `T_BASE+1..T_BASE+T_COUNT` adds the
///   trailing offset (real T jamo start at `T_BASE + 1`; the
///   `T_BASE` codepoint itself is the "no trailing" sentinel).
///
/// Returns `None` for any other input pair.
pub(crate) fn compose_hangul(a: u32, b: u32) -> Option<u32> {
    if (L_BASE..L_BASE + L_COUNT).contains(&a) && (V_BASE..V_BASE + V_COUNT).contains(&b) {
        let l_index = a - L_BASE;
        let v_index = b - V_BASE;
        return Some(S_BASE + (l_index * N_COUNT + v_index * T_COUNT));
    }
    if (S_BASE..S_BASE + S_COUNT).contains(&a) {
        let s_index = a - S_BASE;
        let is_lv = s_index % T_COUNT == 0;
        if is_lv && (T_BASE + 1..T_BASE + T_COUNT).contains(&b) {
            return Some(a + (b - T_BASE));
        }
    }
    None
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
        L_BASE, S_BASE, S_COUNT, T_BASE, V_BASE, compose_hangul, decompose_hangul_syllable,
        is_hangul_syllable,
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

    #[test]
    fn compose_l_v_produces_ga() {
        // ᄀ (U+1100) + ᅡ (U+1161) → 가 (U+AC00).
        assert_eq!(compose_hangul(L_BASE, V_BASE), Some(0xAC00));
    }

    #[test]
    fn compose_lv_t_produces_han() {
        // 하 (U+D558, an LV syllable) + ᆫ (U+11AB) → 한 (U+D55C).
        // 하 has L=ᄒ V=ᅡ, so it's at S_BASE + (17 * N_COUNT + 0 * T_COUNT).
        let ha = 0xD558_u32;
        assert_eq!(compose_hangul(ha, 0x11AB), Some(0xD55C));
    }

    #[test]
    fn compose_lvt_rejects_further_t() {
        // 한 (U+D55C) is LVT (T-offset != 0); cannot accept another T.
        assert!(compose_hangul(0xD55C, 0x11AB).is_none());
    }

    #[test]
    fn compose_rejects_t_base_sentinel() {
        // T_BASE itself (U+11A7) is the "no trailing" sentinel, not a
        // real T jamo. A V-only LV syllable must NOT compose with it.
        assert!(compose_hangul(0xAC00, T_BASE).is_none());
    }

    #[test]
    fn compose_rejects_non_jamo_pair() {
        assert!(compose_hangul(0x0041, 0x0300).is_none());
    }
}
