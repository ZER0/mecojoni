use std::{fmt::Write as _, fs, path::PathBuf};

use mecojoni_benchmarks::{WORKLOAD_VERSION, operation_contract, workloads};

#[test]
fn filesystem_operation_contract_matches_every_committed_workload() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("baselines/operations-v1.contract");
    let expected = fs::read_to_string(path).expect("read operation contract");
    let mut actual = String::from(
        "version|scenario|source_bytes|rules|productions|artifact_hash|expansions|sampler_words|text\n",
    );
    for workload in workloads() {
        let contract = operation_contract(&workload);
        writeln!(
            actual,
            "{WORKLOAD_VERSION}|{}|{}|{}|{}|{:016x}|{}|{}|{}",
            workload.name,
            contract.source_bytes,
            contract.rules,
            contract.productions,
            contract.artifact_hash,
            contract.expansions,
            contract.sampler_words,
            contract.text
        )
        .expect("write operation contract");
    }
    assert_eq!(actual, expected);
}
