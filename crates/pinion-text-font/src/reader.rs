//! R50.1.2 §5.37.1 — internal big-endian byte reader (fail-clean).
//!
//! 외부 byteorder crate 의존 없는 self-contained reader. 모든 read 가
//! bounds-checked + tag-aware `ParseError` 반환. Caller 는 `?` 로
//! propagate 만 하면 됨 — out-of-bounds panic 없음 (textbook fail-clean).
//!
//! Tag 는 reader 생성 시 박힘 — 각 read 에서 `ParseError::TableTooShort`
//! 의 `tag` 필드를 일관 채움.

use crate::error::ParseError;

pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    tag: [u8; 4],
}

// `position` / `remaining` / `skip` / `read_u8` 는 R50.1.3+ cmap/glyf/name 등
// variable-format table parser 에서 사용 예정 — 현재 R50.1.2 에서는 호출처
// 0이므로 dead_code allow.
#[allow(dead_code)]
impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8], tag: [u8; 4]) -> Self {
        Self { bytes, pos: 0, tag }
    }

    pub(crate) fn position(&self) -> usize {
        self.pos
    }

    pub(crate) fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.pos)
    }

    /// Advance the cursor by `n`. Returns `TableTooShort` if would exceed length.
    ///
    /// # Errors
    ///
    /// [`ParseError::TableTooShort`] when `position() + n > bytes.len()`.
    pub(crate) fn skip(&mut self, n: usize) -> Result<(), ParseError> {
        let end = self.pos.checked_add(n).ok_or(ParseError::TableTooShort {
            tag: self.tag,
            needed: usize::MAX,
            available: self.bytes.len(),
        })?;
        if end > self.bytes.len() {
            return Err(ParseError::TableTooShort {
                tag: self.tag,
                needed: end,
                available: self.bytes.len(),
            });
        }
        self.pos = end;
        Ok(())
    }

    fn advance<const N: usize>(&mut self) -> Result<[u8; N], ParseError> {
        let end = self.pos.checked_add(N).ok_or(ParseError::TableTooShort {
            tag: self.tag,
            needed: usize::MAX,
            available: self.bytes.len(),
        })?;
        if end > self.bytes.len() {
            return Err(ParseError::TableTooShort {
                tag: self.tag,
                needed: end,
                available: self.bytes.len(),
            });
        }
        let mut out = [0u8; N];
        out.copy_from_slice(&self.bytes[self.pos..end]);
        self.pos = end;
        Ok(out)
    }

    pub(crate) fn read_u8(&mut self) -> Result<u8, ParseError> {
        Ok(self.advance::<1>()?[0])
    }

    pub(crate) fn read_bytes<const N: usize>(&mut self) -> Result<[u8; N], ParseError> {
        self.advance::<N>()
    }

    pub(crate) fn read_u16(&mut self) -> Result<u16, ParseError> {
        Ok(u16::from_be_bytes(self.advance::<2>()?))
    }

    pub(crate) fn read_i16(&mut self) -> Result<i16, ParseError> {
        Ok(i16::from_be_bytes(self.advance::<2>()?))
    }

    pub(crate) fn read_u32(&mut self) -> Result<u32, ParseError> {
        Ok(u32::from_be_bytes(self.advance::<4>()?))
    }

    pub(crate) fn read_i32(&mut self) -> Result<i32, ParseError> {
        Ok(i32::from_be_bytes(self.advance::<4>()?))
    }

    pub(crate) fn read_i64(&mut self) -> Result<i64, ParseError> {
        Ok(i64::from_be_bytes(self.advance::<8>()?))
    }
}
