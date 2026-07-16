use std::{fs, path::PathBuf};

use mecojoni_benchmarks::{harbor_startup_package, workloads};
use mecojoni_core::{
    ArtifactLimits, ArtifactOptions, PackageInput, PackageSource, SourceFile, SourceId,
    compile_package, compile_package_with_manifest, encode_artifact, inspect_artifact,
};

#[test]
fn checked_in_bytecode_one_artifacts_are_canonical_for_filesystem_sources() {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../benchmarks/artifacts");
    for workload in workloads() {
        let grammar = compile_package(&workload.package()).expect("compile workload");
        let encoded =
            encode_artifact(&grammar, ArtifactOptions::default()).expect("encode workload");
        let checked_in = fs::read(
            directory
                .join("workloads")
                .join(format!("{}.mecob", workload.name)),
        )
        .expect("read checked-in workload artifact");
        assert_eq!(encoded, checked_in, "{} artifact drifted", workload.name);
        assert_eq!(
            inspect_artifact(&checked_in, ArtifactLimits::default())
                .expect("inspect workload artifact")
                .version,
            "bytecode/1"
        );
    }

    let harbor = harbor_startup_package().expect("load Harbor source package");
    let grammar = compile_package_with_manifest(&harbor.input, &harbor.manifest)
        .expect("compile Harbor package");
    let encoded = encode_artifact(&grammar, ArtifactOptions::default()).expect("encode Harbor");
    assert_eq!(
        encoded,
        fs::read(directory.join("harbor.mecob")).expect("read checked-in Harbor artifact")
    );
}

#[test]
fn checked_in_javascript_artifact_is_canonical_for_hello_source() {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source = fs::read_to_string(workspace.join("examples/hello.meco"))
        .expect("read hello source fixture");
    let grammar = compile_package(&PackageInput {
        root_id: "hello".to_string(),
        modules: vec![PackageSource {
            canonical_id: "hello".to_string(),
            source: SourceFile::new(SourceId::new(0), "hello.meco", source),
            resolved_imports: vec![],
        }],
    })
    .expect("compile hello fixture");
    let encoded = encode_artifact(&grammar, ArtifactOptions::default()).expect("encode hello");

    assert_eq!(
        encoded,
        fs::read(workspace.join("js/fixtures/hello.mecob"))
            .expect("read checked-in JavaScript artifact")
    );
}
