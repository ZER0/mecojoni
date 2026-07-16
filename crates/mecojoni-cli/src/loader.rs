use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use mecojoni_core::{
    MecoError, PackageInput, PackageSource, ResolvedImport, SourceFile, SourceId,
    parse_front_matter,
};

use crate::{CliError, CliResult};

/// Loaded package plus canonical paths used to render source diagnostics.
pub(crate) struct LoadedPackage {
    pub input: PackageInput,
    pub paths: BTreeMap<u32, PathBuf>,
}

struct RawModule {
    path: PathBuf,
    module_id: String,
    source: SourceFile,
    imports: Vec<(String, PathBuf)>,
}

/// Resolves a v2 package from one explicit root. All I/O stays in this `std` crate.
pub(crate) fn load_package(root: &Path) -> CliResult<LoadedPackage> {
    let root = canonical_file(root)?;
    let mut pending = vec![root.clone()];
    let mut seen = BTreeSet::new();
    let mut raw_modules = Vec::new();
    let mut paths = BTreeMap::new();

    while let Some(path) = pending.pop() {
        if !seen.insert(path.clone()) {
            continue;
        }
        let bytes = fs::read(&path).map_err(|error| CliError::io(&path, &error))?;
        let id = u32::try_from(raw_modules.len())
            .map_err(|_| CliError::internal("package contains more than u32::MAX modules"))?;
        let source = SourceFile::from_utf8(SourceId::new(id), path.display().to_string(), &bytes)
            .map_err(|error| CliError::usage(format!("{}: {error}", path.display())))?;
        let header = parse_front_matter(&source).map_err(CliError::domain)?;
        let module_id = header.module().value().clone();
        let mut imports = Vec::new();
        for import in header.imports() {
            let authored = import.path().value().clone();
            let target = path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(&authored);
            let target = canonical_file(&target)?;
            imports.push((authored, target.clone()));
            pending.push(target);
        }
        paths.insert(id, path.clone());
        raw_modules.push(RawModule {
            path,
            module_id,
            source,
            imports,
        });
    }

    let ids_by_path = raw_modules
        .iter()
        .map(|module| (module.path.clone(), module.module_id.clone()))
        .collect::<BTreeMap<_, _>>();
    let root_id = ids_by_path
        .get(&root)
        .cloned()
        .ok_or_else(|| CliError::internal("loaded package lost its root module"))?;
    let modules = raw_modules
        .into_iter()
        .map(|module| {
            let resolved_imports = module
                .imports
                .into_iter()
                .map(|(authored_path, target)| {
                    let target_id = ids_by_path.get(&target).cloned().ok_or_else(|| {
                        CliError::internal("loaded import target has no stable module ID")
                    })?;
                    Ok(ResolvedImport {
                        authored_path,
                        target_id,
                    })
                })
                .collect::<CliResult<Vec<_>>>()?;
            Ok(PackageSource {
                canonical_id: module.module_id,
                source: module.source,
                resolved_imports,
            })
        })
        .collect::<CliResult<Vec<_>>>()?;

    Ok(LoadedPackage {
        input: PackageInput { root_id, modules },
        paths,
    })
}

fn canonical_file(path: &Path) -> CliResult<PathBuf> {
    let canonical = fs::canonicalize(path).map_err(|error| CliError::io(path, &error))?;
    if !canonical.is_file() {
        return Err(CliError::usage(format!(
            "{} is not a regular file",
            canonical.display()
        )));
    }
    Ok(canonical)
}

pub(crate) fn format_meco_error(error: &MecoError, package: Option<&LoadedPackage>) -> String {
    let mut lines = Vec::new();
    for diagnostic in error.diagnostics() {
        let location = diagnostic.span().map_or_else(String::new, |span| {
            let path = package
                .and_then(|package| package.paths.get(&span.source().get()))
                .map_or_else(
                    || format!("source {}", span.source().get()),
                    |path| path.display().to_string(),
                );
            format!("{path}:{}-{}: ", span.start().byte(), span.end().byte())
        });
        lines.push(format!(
            "{location}{}: {}",
            diagnostic.code().as_str(),
            diagnostic.message()
        ));
    }
    if lines.is_empty() {
        "Mecojoni operation failed without diagnostics".to_string()
    } else {
        lines.join("\n")
    }
}
