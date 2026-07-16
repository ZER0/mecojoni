use alloc::vec::Vec;
use core::fmt;

use crate::Diagnostic;

/// Failure returned by public core operations.
///
/// The collection is non-empty by construction so a failed operation always
/// explains itself with at least one stable diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MecoError {
    diagnostics: Vec<Diagnostic>,
}

impl MecoError {
    #[must_use]
    pub fn new(diagnostic: Diagnostic) -> Self {
        Self {
            diagnostics: alloc::vec![diagnostic],
        }
    }

    #[must_use]
    pub fn with_related(
        diagnostic: Diagnostic,
        related: impl IntoIterator<Item = Diagnostic>,
    ) -> Self {
        let mut diagnostics = alloc::vec![diagnostic];
        diagnostics.extend(related);
        Self { diagnostics }
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    #[must_use]
    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }
}

impl fmt::Display for MecoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let first = &self.diagnostics[0];
        write!(formatter, "{}: {}", first.code().as_str(), first.message())?;
        if self.diagnostics.len() > 1 {
            write!(
                formatter,
                " ({} related diagnostics)",
                self.diagnostics.len() - 1
            )?;
        }
        Ok(())
    }
}

/// Result type shared by the compiler and runtime APIs.
pub type MecoResult<T> = core::result::Result<T, MecoError>;

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::MecoError;
    use crate::{Diagnostic, DiagnosticCode, Severity};

    #[test]
    fn errors_always_contain_a_primary_diagnostic() {
        let error = MecoError::new(Diagnostic::new(
            DiagnosticCode::INVALID_UTF8,
            Severity::Error,
            None,
            "invalid source",
        ));

        assert_eq!(error.diagnostics().len(), 1);
        assert_eq!(error.to_string(), "E_INVALID_UTF8: invalid source");
    }
}
