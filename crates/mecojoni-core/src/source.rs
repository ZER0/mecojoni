use alloc::string::{String, ToString};
use core::{fmt, str};

use crate::{SourceId, SourcePosition};

/// Owned UTF-8 source module supplied by a host package loader.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceFile {
    id: SourceId,
    name: String,
    text: String,
}

impl SourceFile {
    #[must_use]
    pub fn new(id: SourceId, name: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            text: text.into(),
        }
    }

    /// Copies a byte source after strict UTF-8 validation.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError`] with the first invalid byte offset when `bytes`
    /// is not well-formed UTF-8.
    pub fn from_utf8(
        id: SourceId,
        name: impl Into<String>,
        bytes: &[u8],
    ) -> Result<Self, SourceError> {
        let text = str::from_utf8(bytes).map_err(|error| SourceError {
            valid_up_to: error.valid_up_to(),
            error_len: error.error_len(),
        })?;

        Ok(Self::new(id, name, text.to_string()))
    }

    #[must_use]
    pub const fn id(&self) -> SourceId {
        self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.text.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Converts a UTF-8 byte offset into the core's dual coordinate system.
    ///
    /// Returns `None` when `byte` lies outside the source or in the middle of a
    /// multibyte UTF-8 scalar value.
    #[must_use]
    pub fn position(&self, byte: usize) -> Option<SourcePosition> {
        if !self.text.is_char_boundary(byte) {
            return None;
        }

        let scalar = self.text.get(..byte)?.chars().count();
        Some(SourcePosition::new(byte as u64, scalar as u64))
    }
}

/// Strict UTF-8 decoding failure without retaining the source bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceError {
    valid_up_to: usize,
    error_len: Option<usize>,
}

impl SourceError {
    #[must_use]
    pub const fn valid_up_to(self) -> usize {
        self.valid_up_to
    }

    #[must_use]
    pub const fn error_len(self) -> Option<usize> {
        self.error_len
    }
}

impl fmt::Display for SourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid UTF-8 at byte {}", self.valid_up_to)
    }
}

#[cfg(test)]
mod tests {
    use super::SourceFile;
    use crate::SourceId;

    #[test]
    fn accepts_unicode_terminal_text() {
        let source = SourceFile::from_utf8(
            SourceId::new(0),
            "unicode.meco.md",
            "# greeting\n- こんにちは 🌍".as_bytes(),
        )
        .expect("valid UTF-8");

        assert!(source.text().contains("こんにちは 🌍"));
    }

    #[test]
    fn reports_invalid_utf8_offset() {
        let error = SourceFile::from_utf8(SourceId::new(0), "broken.meco.md", &[b'a', 0xff, b'b'])
            .expect_err("invalid UTF-8 must fail");

        assert_eq!(error.valid_up_to(), 1);
        assert_eq!(error.error_len(), Some(1));
    }

    #[test]
    fn maps_utf8_bytes_to_unicode_scalar_offsets() {
        let source = SourceFile::new(SourceId::new(0), "unicode.meco.md", "a🦀z");

        assert_eq!(source.position(1), Some(crate::SourcePosition::new(1, 1)));
        assert_eq!(source.position(5), Some(crate::SourcePosition::new(5, 2)));
        assert_eq!(source.position(2), None);
    }
}
