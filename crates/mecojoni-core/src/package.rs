use alloc::{format, string::String, vec::Vec};

use crate::{
    Diagnostic, DiagnosticCode, MecoError, MecoResult, Severity, SourceFile, parse_front_matter,
};

/// Host-resolved edge for one authored import path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedImport {
    pub authored_path: String,
    pub target_id: String,
}

/// One canonical source module supplied by the host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageSource {
    pub canonical_id: String,
    pub source: SourceFile,
    pub resolved_imports: Vec<ResolvedImport>,
}

/// Complete, I/O-free input to package compilation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageInput {
    pub root_id: String,
    pub modules: Vec<PackageSource>,
}

/// Validates host package identity and import-edge completeness.
///
/// # Errors
///
/// Returns a structured error when the root or an import target is absent,
/// canonical IDs repeat, or authored import paths and host resolutions differ.
pub fn validate_package_input(package: &PackageInput) -> MecoResult<()> {
    if !package
        .modules
        .iter()
        .any(|module| module.canonical_id == package.root_id)
    {
        return Err(error(
            DiagnosticCode::PACKAGE_ROOT,
            format!("package root `{}` was not supplied", package.root_id),
        ));
    }

    for (index, module) in package.modules.iter().enumerate() {
        if module.canonical_id.is_empty() {
            return Err(error(
                DiagnosticCode::PACKAGE_DUPLICATE_MODULE,
                "canonical module IDs cannot be empty",
            ));
        }
        if package.modules[..index]
            .iter()
            .any(|previous| previous.canonical_id == module.canonical_id)
        {
            return Err(error(
                DiagnosticCode::PACKAGE_DUPLICATE_MODULE,
                format!("duplicate canonical module ID `{}`", module.canonical_id),
            ));
        }

        let header = parse_front_matter(&module.source)?;
        for import in header.imports() {
            let authored_path = import.path().value();
            let matches = module
                .resolved_imports
                .iter()
                .filter(|resolution| resolution.authored_path == *authored_path)
                .count();
            if matches != 1 {
                return Err(error(
                    DiagnosticCode::IMPORT_RESOLUTION,
                    format!(
                        "module `{}` requires exactly one resolution for `{authored_path}`",
                        module.canonical_id
                    ),
                ));
            }
        }
        for resolution in &module.resolved_imports {
            if !header
                .imports()
                .iter()
                .any(|import| import.path().value() == &resolution.authored_path)
            {
                return Err(error(
                    DiagnosticCode::IMPORT_RESOLUTION,
                    format!(
                        "module `{}` supplied an undeclared resolution for `{}`",
                        module.canonical_id, resolution.authored_path
                    ),
                ));
            }
            if !package
                .modules
                .iter()
                .any(|candidate| candidate.canonical_id == resolution.target_id)
            {
                return Err(error(
                    DiagnosticCode::IMPORT_RESOLUTION,
                    format!(
                        "import `{}` in `{}` targets missing module `{}`",
                        resolution.authored_path, module.canonical_id, resolution.target_id
                    ),
                ));
            }
        }
    }

    Ok(())
}

fn error(code: DiagnosticCode, message: impl Into<String>) -> MecoError {
    MecoError::new(Diagnostic::new(code, Severity::Error, None, message))
}

#[cfg(test)]
mod tests {
    use alloc::{string::ToString, vec};

    use super::{PackageInput, PackageSource, ResolvedImport, validate_package_input};
    use crate::{DiagnosticCode, SourceFile, SourceId};

    fn source(id: u32, name: &str, header: &str) -> SourceFile {
        SourceFile::new(SourceId::new(id), name, header)
    }

    #[test]
    fn requires_an_explicit_supplied_root() {
        let package = PackageInput {
            root_id: "missing".to_string(),
            modules: vec![],
        };

        let error = validate_package_input(&package).expect_err("root must exist");
        assert_eq!(error.diagnostics()[0].code(), DiagnosticCode::PACKAGE_ROOT);
    }

    #[test]
    fn validates_host_resolved_edges_without_io() {
        let package = PackageInput {
            root_id: "root".to_string(),
            modules: vec![
                PackageSource {
                    canonical_id: "root".to_string(),
                    source: source(
                        0,
                        "root.meco.md",
                        "---\nmeco: 2\nmodule: root\nimports:\n  common: \"./common.meco.md\"\n---\n",
                    ),
                    resolved_imports: vec![ResolvedImport {
                        authored_path: "./common.meco.md".to_string(),
                        target_id: "common".to_string(),
                    }],
                },
                PackageSource {
                    canonical_id: "common".to_string(),
                    source: source(1, "common.meco.md", "---\nmeco: 2\nmodule: common\n---\n"),
                    resolved_imports: vec![],
                },
            ],
        };

        validate_package_input(&package).expect("package edges validate");
    }
}
