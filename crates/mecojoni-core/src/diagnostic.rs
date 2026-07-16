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
    pub const COMMENT_SYNTAX: Self = Self("E_COMMENT_SYNTAX");
    pub const RULE_SYNTAX: Self = Self("E_RULE_SYNTAX");
    pub const DUPLICATE_RULE: Self = Self("E_DUPLICATE_RULE");
    pub const PRODUCTION_SYNTAX: Self = Self("E_PRODUCTION_SYNTAX");
    pub const PRODUCTION_ID: Self = Self("E_PRODUCTION_ID");
    pub const WEIGHT_SYNTAX: Self = Self("E_WEIGHT_SYNTAX");
    pub const CLAUSE_ORDER: Self = Self("E_CLAUSE_ORDER");
    pub const GUARD_SYNTAX: Self = Self("E_GUARD_SYNTAX");
    pub const BINDING_SYNTAX: Self = Self("E_BINDING_SYNTAX");
    pub const BODY_SYNTAX: Self = Self("E_BODY_SYNTAX");
    pub const STRING_SYNTAX: Self = Self("E_STRING_SYNTAX");
    pub const ESCAPE_SYNTAX: Self = Self("E_ESCAPE_SYNTAX");
    pub const CALL_SYNTAX: Self = Self("E_CALL_SYNTAX");
    pub const ARGUMENT_SYNTAX: Self = Self("E_ARGUMENT_SYNTAX");
    pub const BLOCK_SYNTAX: Self = Self("E_BLOCK_SYNTAX");
    pub const COMPOSITION_SHELL: Self = Self("W_COMPOSITION_SHELL");
    pub const PACKAGE_ROOT: Self = Self("E_PACKAGE_ROOT");
    pub const PACKAGE_DUPLICATE_MODULE: Self = Self("E_PACKAGE_DUPLICATE_MODULE");
    pub const IMPORT_RESOLUTION: Self = Self("E_IMPORT_RESOLUTION");
    pub const MODULE_IDENTITY: Self = Self("E_MODULE_IDENTITY");
    pub const IMPORT_CYCLE: Self = Self("E_IMPORT_CYCLE");
    pub const EXPORT: Self = Self("E_EXPORT");
    pub const ENTRY: Self = Self("E_ENTRY");
    pub const UNDEFINED_RULE: Self = Self("E_UNDEFINED_RULE");
    pub const RULE_VISIBILITY: Self = Self("E_RULE_VISIBILITY");
    pub const RULE_ARITY: Self = Self("E_RULE_ARITY");
    pub const TYPE: Self = Self("E_TYPE");
    pub const TYPE_MISMATCH: Self = Self("E_TYPE_MISMATCH");
    pub const VALUE_NAME: Self = Self("E_VALUE_NAME");
    pub const BINDING_NAME: Self = Self("E_BINDING_NAME");
    pub const REQUEST_DATA: Self = Self("E_REQUEST_DATA");
    pub const MESSAGE_MANIFEST: Self = Self("E_MESSAGE_MANIFEST");
    pub const MESSAGE_MISSING: Self = Self("E_MESSAGE_MISSING");
    pub const MESSAGE_ARGUMENT: Self = Self("E_MESSAGE_ARGUMENT");
    pub const MESSAGE_EFFECT: Self = Self("E_MESSAGE_EFFECT");
    pub const FORMATTER_REQUIRED: Self = Self("E_FORMATTER_REQUIRED");
    pub const FORMATTER: Self = Self("E_FORMATTER");
    pub const LOCALE: Self = Self("E_LOCALE");
    pub const FORMATTER_LIMIT: Self = Self("E_FORMATTER_LIMIT");
    pub const STATE_BUSY: Self = Self("E_STATE_BUSY");
    pub const CANCELLED: Self = Self("E_CANCELLED");
    pub const SNAPSHOT: Self = Self("E_SNAPSHOT");
    pub const SNAPSHOT_LIMIT: Self = Self("E_SNAPSHOT_LIMIT");
    pub const SNAPSHOT_EXPIRED: Self = Self("E_SNAPSHOT_EXPIRED");
    pub const NO_ELIGIBLE_PRODUCTION: Self = Self("E_NO_ELIGIBLE_PRODUCTION");
    pub const WEIGHT_VALUE: Self = Self("E_WEIGHT_VALUE");
    pub const UNSUPPORTED_FEATURE: Self = Self("E_UNSUPPORTED_FEATURE");
    pub const WEIGHT_OVERFLOW: Self = Self("E_WEIGHT_OVERFLOW");
    pub const UNPRODUCTIVE_RULE: Self = Self("E_UNPRODUCTIVE_RULE");
    pub const NO_ENTRY: Self = Self("E_NO_ENTRY");
    pub const LIMIT_DEPTH: Self = Self("E_LIMIT_DEPTH");
    pub const LIMIT_EXPANSIONS: Self = Self("E_LIMIT_EXPANSIONS");
    pub const LIMIT_OUTPUT: Self = Self("E_LIMIT_OUTPUT");
    pub const SAMPLER_BUDGET: Self = Self("E_SAMPLER_BUDGET");
    pub const RECURSION_RISK: Self = Self("W_RECURSION_RISK");

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
