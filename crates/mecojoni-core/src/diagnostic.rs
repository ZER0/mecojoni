use alloc::string::String;

use crate::Span;

/// Stable diagnostic identifier exposed by every public API.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DiagnosticCode(&'static str);

impl DiagnosticCode {
    pub const INVALID_UTF8: Self = Self("E_INVALID_UTF8");
    pub const INVALID_SPAN: Self = Self("E_INVALID_SPAN");
    pub const HEADER_MISSING: Self = Self("E_HEADER_MISSING");
    pub const HEADER_UNTERMINATED: Self = Self("E_HEADER_UNTERMINATED");
    pub const HEADER_SYNTAX: Self = Self("E_HEADER_SYNTAX");
    pub const HEADER_INDENT: Self = Self("E_HEADER_INDENT");
    pub const HEADER_UNKNOWN_FIELD: Self = Self("E_HEADER_UNKNOWN_FIELD");
    pub const HEADER_DUPLICATE_FIELD: Self = Self("E_HEADER_DUPLICATE_FIELD");
    pub const HEADER_REQUIRED_FIELD: Self = Self("E_HEADER_REQUIRED_FIELD");
    pub const HEADER_VALUE: Self = Self("E_HEADER_VALUE");
    pub const UNSUPPORTED_VERSION: Self = Self("E_UNSUPPORTED_VERSION");
    pub const INVALID_IDENTIFIER: Self = Self("E_INVALID_IDENTIFIER");

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Severity {
    Error,
    Warning,
}

/// Structured diagnostic with an optional exact source range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    code: DiagnosticCode,
    severity: Severity,
    span: Option<Span>,
    message: String,
}

impl Diagnostic {
    #[must_use]
    pub fn new(
        code: DiagnosticCode,
        severity: Severity,
        span: Option<Span>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            severity,
            span,
            message: message.into(),
        }
    }

    #[must_use]
    pub const fn code(&self) -> DiagnosticCode {
        self.code
    }

    #[must_use]
    pub const fn severity(&self) -> Severity {
        self.severity
    }

    #[must_use]
    pub const fn span(&self) -> Option<Span> {
        self.span
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::String;

    use super::{Diagnostic, DiagnosticCode, Severity};

    #[test]
    fn exposes_stable_code_and_owned_message() {
        let diagnostic = Diagnostic::new(
            DiagnosticCode::INVALID_UTF8,
            Severity::Error,
            None,
            String::from("source is not UTF-8"),
        );

        assert_eq!(diagnostic.code().as_str(), "E_INVALID_UTF8");
        assert_eq!(diagnostic.message(), "source is not UTF-8");
    }
}
