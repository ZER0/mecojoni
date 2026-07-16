use std::{fs, path::PathBuf};

#[test]
fn recorded_cross_runtime_result_covers_every_workload_and_harbor() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../benchmarks/results/2026-07-16-darwin-arm64.json");
    let result = fs::read_to_string(path).expect("recorded benchmark result exists");
    assert!(result.contains("\"schema\": \"mecojoni-benchmark-result/1\""));
    for scenario in [
        "flat-64",
        "tree-dialogue",
        "chain-512",
        "dense-dag-96x8",
        "recursive-balanced",
        "fanout-10000",
        "harbor-dialogue",
    ] {
        assert!(
            result.contains(&format!("\"{scenario}\"")),
            "missing {scenario}"
        );
    }
    assert_eq!(result.matches("\"v1Js\"").count(), 6);
    assert_eq!(result.matches("\"v2Rust\"").count(), 7);
    assert_eq!(result.matches("\"v2Wasm\"").count(), 7);
}

#[test]
fn recorded_bytecode_result_contains_native_and_wasm_source_comparisons() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../benchmarks/results/2026-07-16-bytecode0-darwin-arm64.json");
    let result = fs::read_to_string(path).expect("recorded bytecode result exists");
    for field in [
        "mecojoni-artifact-benchmark-result/1",
        "sourceCompileNsMedian",
        "artifactLoadNsMedian",
        "sourceCompileMsMedian",
        "artifactLoadMsMedian",
        "liveHandlesAfterDispose",
    ] {
        assert!(result.contains(field), "missing {field}");
    }
}

#[test]
fn frozen_bytecode_result_covers_every_workload_and_delivery_gate() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../benchmarks/results/2026-07-16-bytecode1-workloads-darwin-arm64.json");
    let result = fs::read_to_string(path).expect("recorded bytecode/1 result exists");
    for scenario in [
        "flat-64",
        "tree-dialogue",
        "chain-512",
        "dense-dag-96x8",
        "recursive-balanced",
        "fanout-10000",
        "harbor-dialogue",
        "freeze-bytecode/1",
        "browserMecoOrMecobRequests",
    ] {
        assert!(result.contains(scenario), "missing {scenario}");
    }
}
