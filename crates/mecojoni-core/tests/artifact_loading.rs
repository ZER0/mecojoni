use std::{fs, path::PathBuf};

use mecojoni_core::{
    ArtifactDebugProfile, ArtifactLimits, ArtifactOptions, DiagnosticCode, GenerationRequest,
    PackageInput, PackageSource, ResolvedImport, SourceFile, SourceId, compile_package,
    decode_artifact, disassemble_artifact, encode_artifact, inspect_artifact,
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
                .starts_with("bytecode/1")
        );
    }
}

#[test]
fn canonical_artifacts_do_not_depend_on_host_source_ids() {
    let canonical = compile_package(&weighted_package(false)).expect("compile canonical package");
    let mut reassigned = weighted_package(true);
    reassigned.modules[0].source = SourceFile::new(
        SourceId::new(91),
        reassigned.modules[0].source.name(),
        reassigned.modules[0].source.text(),
    );
    reassigned.modules[1].source = SourceFile::new(
        SourceId::new(37),
        reassigned.modules[1].source.name(),
        reassigned.modules[1].source.text(),
    );
    let reassigned = compile_package(&reassigned).expect("compile reassigned package");

    assert_eq!(
        encode_artifact(&canonical, ArtifactOptions::default()).expect("encode canonical"),
        encode_artifact(&reassigned, ArtifactOptions::default()).expect("encode reassigned")
    );
}

#[test]
fn debug_profiles_change_only_header_declaration_and_content_hash() {
    let grammar = compile_package(&weighted_package(false)).expect("compile weighted package");
    let encode = |debug_profile| {
        encode_artifact(&grammar, ArtifactOptions { debug_profile }).expect("encode profile")
    };
    let full = encode(ArtifactDebugProfile::Full);
    let mapped = encode(ArtifactDebugProfile::Mapped);
    let stripped = encode(ArtifactDebugProfile::Stripped);

    assert_eq!(&full[104..], &mapped[104..]);
    assert_eq!(&full[104..], &stripped[104..]);
    assert_ne!(&full[12..16], &mapped[12..16]);
    assert_ne!(&mapped[12..16], &stripped[12..16]);
    assert_ne!(&full[48..56], &mapped[48..56]);
    assert_ne!(&mapped[48..56], &stripped[48..56]);
}

#[test]
fn canonicalization_still_rejects_duplicate_module_ids() {
    let mut duplicate = weighted_package(true);
    duplicate.modules[0].canonical_id = duplicate.modules[1].canonical_id.clone();
    duplicate.modules[0].source = SourceFile::new(
        SourceId::new(91),
        duplicate.modules[0].source.name(),
        duplicate.modules[0].source.text(),
    );

    let error = compile_package(&duplicate).expect_err("duplicate module IDs must fail");
    assert_eq!(
        error.diagnostics()[0].code(),
        DiagnosticCode::PACKAGE_DUPLICATE_MODULE
    );
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
