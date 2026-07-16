use std::{fs, path::PathBuf};

use mecojoni_core::{
    GenerationRequest, LOWERED_IR_CONTRACT, PackageInput, PackageSource, ResolvedImport,
    SourceFile, SourceId, compile_package,
};

#[test]
fn filesystem_package_pins_the_lowered_runtime_boundary() {
    let directory =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/minimal");
    let package = PackageInput {
        root_id: "root".to_string(),
        modules: vec![
            PackageSource {
                canonical_id: "root".to_string(),
                source: SourceFile::new(
                    SourceId::new(0),
                    "root.meco",
                    fs::read_to_string(directory.join("root.meco")).expect("read root fixture"),
                ),
                resolved_imports: vec![ResolvedImport {
                    authored_path: "./common.meco".to_string(),
                    target_id: "common".to_string(),
                }],
            },
            PackageSource {
                canonical_id: "common".to_string(),
                source: SourceFile::new(
                    SourceId::new(1),
                    "common.meco",
                    fs::read_to_string(directory.join("common.meco")).expect("read common fixture"),
                ),
                resolved_imports: vec![],
            },
        ],
    };
    let grammar = compile_package(&package).expect("minimal package compiles");
    let mut request = GenerationRequest::with_seed(0);
    request.entry = Some("fixture.greeting");
    let generated = grammar
        .generate_weighted(&request)
        .expect("minimal package generates");
    let contract = format!(
        "{}|{:016x}|{}|{}|{}|{}|{}\n",
        LOWERED_IR_CONTRACT,
        grammar.artifact_hash(),
        grammar.rule_count(),
        grammar.production_count(),
        grammar.entries().collect::<Vec<_>>().join(","),
        generated.expansions(),
        generated.text(),
    );
    let expected = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/expected/lowered-ir-v1.contract"),
    )
    .expect("read lowered contract");
    assert_eq!(contract, expected);
}
