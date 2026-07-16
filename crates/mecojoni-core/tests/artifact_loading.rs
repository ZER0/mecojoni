use std::{fs, path::PathBuf};

use mecojoni_core::{
    ArtifactDebugProfile, ArtifactLimits, ArtifactOptions, GenerationRequest, PackageInput,
    PackageSource, ResolvedImport, SourceFile, SourceId, compile_package, decode_artifact,
    disassemble_artifact, encode_artifact, inspect_artifact,
};

fn weighted_package(reverse_modules: bool) -> PackageInput {
    let directory =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/weighted");
    let root = PackageSource {
        canonical_id: "root".to_string(),
        source: SourceFile::new(
            SourceId::new(0),
            "root.meco",
            fs::read_to_string(directory.join("root.meco")).expect("read weighted root"),
        ),
        resolved_imports: vec![ResolvedImport {
            authored_path: "./common.meco".to_string(),
            target_id: "common".to_string(),
        }],
    };
    let common = PackageSource {
        canonical_id: "common".to_string(),
        source: SourceFile::new(
            SourceId::new(1),
            "common.meco",
            fs::read_to_string(directory.join("common.meco")).expect("read weighted common"),
        ),
        resolved_imports: vec![],
    };
    PackageInput {
        root_id: "root".to_string(),
        modules: if reverse_modules {
            vec![common, root]
        } else {
            vec![root, common]
        },
    }
}

#[test]
fn filesystem_weighted_package_is_canonical_and_artifact_equivalent() {
    let source = compile_package(&weighted_package(false)).expect("compile weighted package");
    let reordered = compile_package(&weighted_package(true)).expect("compile reordered package");
    for profile in [
        ArtifactDebugProfile::Full,
        ArtifactDebugProfile::Mapped,
        ArtifactDebugProfile::Stripped,
    ] {
        let bytes = encode_artifact(
            &source,
            ArtifactOptions {
                debug_profile: profile,
            },
        )
        .expect("encode weighted artifact");
        let reordered_bytes = encode_artifact(
            &reordered,
            ArtifactOptions {
                debug_profile: profile,
            },
        )
        .expect("encode reordered weighted artifact");
        assert_eq!(bytes, reordered_bytes);
        let decoded = decode_artifact(&bytes, ArtifactLimits::default()).expect("decode artifact");
        assert_eq!(source.artifact_hash(), decoded.artifact_hash());
        for seed in 0..128 {
            let request = GenerationRequest::with_seed(seed);
            assert_eq!(
                source.generate_weighted(&request).expect("source output"),
                decoded
                    .generate_weighted(&request)
                    .expect("artifact output")
            );
        }
        let metadata = inspect_artifact(&bytes, ArtifactLimits::default()).expect("inspect");
        assert_eq!(metadata.debug_profile, profile);
        assert!(
            disassemble_artifact(&bytes, ArtifactLimits::default())
                .expect("disassemble")
                .starts_with("bytecode/0")
        );
    }
}

#[test]
fn every_truncation_and_deterministic_mutation_fails_without_panicking() {
    let grammar = compile_package(&weighted_package(false)).expect("compile weighted package");
    let bytes = encode_artifact(&grammar, ArtifactOptions::default()).expect("encode artifact");
    for length in 0..bytes.len() {
        assert!(decode_artifact(&bytes[..length], ArtifactLimits::default()).is_err());
    }
    for index in 0..bytes.len() {
        let mut mutation = bytes.clone();
        mutation[index] ^= 0x5a;
        assert!(decode_artifact(&mutation, ArtifactLimits::default()).is_err());
    }
}
