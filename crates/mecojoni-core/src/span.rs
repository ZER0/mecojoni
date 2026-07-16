use core::fmt;

/// Stable identifier for one source module within a compilation package.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceId(u32);

impl SourceId {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Position within a source module in both required coordinate systems.
///
/// Byte offsets address UTF-8 source buffers. Scalar offsets count Unicode
/// scalar values and remain independent of UTF-16 editor coordinates.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourcePosition {
    byte: u64,
    scalar: u64,
}

impl SourcePosition {
    #[must_use]
    pub const fn new(byte: u64, scalar: u64) -> Self {
        Self { byte, scalar }
    }

    #[must_use]
    pub const fn byte(self) -> u64 {
        self.byte
    }

    #[must_use]
    pub const fn scalar(self) -> u64 {
        self.scalar
    }
}

/// Half-open UTF-8 byte range in one source module.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Span {
    source: SourceId,
    start: SourcePosition,
    end: SourcePosition,
}

impl Span {
    /// Creates a span after validating that `start <= end`.
    ///
    /// # Errors
    ///
    /// Returns [`SpanError`] when `start` is after `end`.
    pub const fn new(
        source: SourceId,
        start: SourcePosition,
        end: SourcePosition,
    ) -> Result<Self, SpanError> {
        let byte_ordered = start.byte() <= end.byte();
        let scalar_ordered = start.scalar() <= end.scalar();
        if byte_ordered && scalar_ordered {
            Ok(Self { source, start, end })
        } else {
            Err(SpanError { start, end })
        }
    }

    #[must_use]
    pub const fn empty(source: SourceId, at: SourcePosition) -> Self {
        Self {
            source,
            start: at,
            end: at,
        }
    }

    #[must_use]
    pub const fn source(self) -> SourceId {
        self.source
    }

    #[must_use]
    pub const fn start(self) -> SourcePosition {
        self.start
    }

    #[must_use]
    pub const fn end(self) -> SourcePosition {
        self.end
    }

    #[must_use]
    pub const fn byte_len(self) -> u64 {
        self.end.byte() - self.start.byte()
    }

    #[must_use]
    pub const fn scalar_len(self) -> u64 {
        self.end.scalar() - self.start.scalar()
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start.byte() == self.end.byte() && self.start.scalar() == self.end.scalar()
    }

    #[must_use]
    pub const fn contains(self, position: SourcePosition) -> bool {
        let contains_byte =
            self.start.byte() <= position.byte() && position.byte() < self.end.byte();
        let contains_scalar =
            self.start.scalar() <= position.scalar() && position.scalar() < self.end.scalar();
        contains_byte && contains_scalar
    }
}

/// Returned when a span end precedes its start.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpanError {
    start: SourcePosition,
    end: SourcePosition,
}

impl SpanError {
    #[must_use]
    pub const fn start(self) -> SourcePosition {
        self.start
    }

    #[must_use]
    pub const fn end(self) -> SourcePosition {
        self.end
    }
}

impl fmt::Display for SpanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "span start (byte {}, scalar {}) exceeds end (byte {}, scalar {})",
            self.start.byte(),
            self.start.scalar(),
            self.end.byte(),
            self.end.scalar()
        )
    }
}

/// A parsed value paired with the exact source range that produced it.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Spanned<T> {
    value: T,
    span: Span,
}

impl<T> Spanned<T> {
    #[must_use]
    pub const fn new(value: T, span: Span) -> Self {
        Self { value, span }
    }

    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }

    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }

    #[must_use]
    pub fn into_value(self) -> T {
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::{SourceId, SourcePosition, Span, Spanned};

    #[test]
    fn validates_span_order() {
        let error = Span::new(
            SourceId::new(2),
            SourcePosition::new(9, 7),
            SourcePosition::new(4, 4),
        )
        .expect_err("reversed span must fail");

        assert_eq!(error.start(), SourcePosition::new(9, 7));
        assert_eq!(error.end(), SourcePosition::new(4, 4));
    }

    #[test]
    fn uses_half_open_ranges() {
        let span = Span::new(
            SourceId::new(1),
            SourcePosition::new(3, 2),
            SourcePosition::new(8, 6),
        )
        .expect("ordered span");

        assert_eq!(span.byte_len(), 5);
        assert_eq!(span.scalar_len(), 4);
        assert!(span.contains(SourcePosition::new(3, 2)));
        assert!(span.contains(SourcePosition::new(7, 5)));
        assert!(!span.contains(SourcePosition::new(8, 6)));
    }

    #[test]
    fn spanned_values_retain_their_source_range() {
        let span = Span::empty(SourceId::new(4), SourcePosition::new(12, 9));
        let value = Spanned::new("module", span);

        assert_eq!(value.value(), &"module");
        assert_eq!(value.span(), span);
    }
}
