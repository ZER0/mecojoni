use std::{fs, path::PathBuf};

use mecojoni_core::{SourceFile, SourceId, parse_front_matter};

fn fixture_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative)
}

#[test]
fn loads_a_real_meco_file_from_the_filesystem() {
    let path = fixture_path("valid/minimal.meco.md");
    let bytes = fs::read(&path).expect("read fixture");
    let source = SourceFile::from_utf8(SourceId::new(0), path.display().to_string(), &bytes)
        .expect("fixture is valid UTF-8");

    assert_eq!(source.id(), SourceId::new(0));
    assert!(source.name().ends_with("minimal.meco.md"));
    assert!(source.text().contains("meco: 2"));
    assert!(source.text().contains("# greeting"));
    let header = parse_front_matter(&source).expect("fixture header parses");
    assert_eq!(header.module().value(), "hello");
}

#[test]
fn loads_every_module_in_a_real_package_from_the_filesystem() {
    let package = fixture_path("packages/minimal");
    let mut paths = fs::read_dir(&package)
        .expect("read package directory")
        .map(|entry| entry.expect("read package entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .collect::<Vec<_>>();
    paths.sort();

    let sources = paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            let bytes = fs::read(path).expect("read package module");
            SourceFile::from_utf8(
                SourceId::new(u32::try_from(index).expect("fixture count fits in u32")),
                path.display().to_string(),
                &bytes,
            )
            .expect("package module is valid UTF-8")
        })
        .collect::<Vec<_>>();

    assert_eq!(sources.len(), 2);
    assert!(
        sources
            .iter()
            .any(|source| source.text().contains("@common.person"))
    );
    assert!(
        sources
            .iter()
            .any(|source| source.text().contains("# person"))
    );
    for source in &sources {
        parse_front_matter(source).expect("package module header parses");
    }
}

#[test]
fn invalid_headers_match_checked_in_diagnostic_codes() {
    let cases = [
        "unknown-field",
        "unsupported-version",
        "bad-indentation",
        "missing-module",
    ];

    for (index, case) in cases.iter().enumerate() {
        let source_path = fixture_path(&format!("invalid/{case}.meco.md"));
        let expected_path = fixture_path(&format!("expected/{case}.code"));
        let bytes = fs::read(&source_path).expect("read invalid fixture");
        let expected = fs::read_to_string(&expected_path).expect("read expected diagnostic");
        let source = SourceFile::from_utf8(
            SourceId::new(u32::try_from(index).expect("fixture count fits in u32")),
            source_path.display().to_string(),
            &bytes,
        )
        .expect("invalid syntax fixture is still valid UTF-8");

        let error = parse_front_matter(&source).expect_err("fixture must fail");
        assert_eq!(
            error.diagnostics()[0].code().as_str(),
            expected.trim(),
            "unexpected primary diagnostic for {case}"
        );
    }
}
