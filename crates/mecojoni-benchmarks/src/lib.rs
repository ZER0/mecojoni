#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

use std::fmt::Write as _;

use mecojoni_core::{
    GenerationLimits, GenerationRequest, PackageInput, PackageSource, SourceFile, SourceId,
    compile_package,
};

pub const WORKLOAD_VERSION: &str = "workloads/1";

/// Explicit adversarial-suite limits; production defaults remain unchanged.
#[must_use]
pub const fn workload_limits() -> GenerationLimits {
    GenerationLimits {
        max_depth: 2_048,
        max_expansions: 100_000,
        max_output_scalars: 1_000_000,
        max_output_bytes: 4_000_000,
        max_sampler_words: 200_000,
    }
}

/// One deterministic committed benchmark topology.
pub struct Workload {
    pub name: &'static str,
    pub class: &'static str,
    pub source: String,
    pub generations: u32,
}

/// Cross-target deterministic seed-zero contract for one workload.
pub struct OperationContract {
    pub source_bytes: usize,
    pub rules: usize,
    pub productions: usize,
    pub artifact_hash: u64,
    pub expansions: u32,
    pub sampler_words: u32,
    pub text: String,
}

impl Workload {
    #[must_use]
    pub fn package(&self) -> PackageInput {
        PackageInput {
            root_id: self.name.to_string(),
            modules: vec![PackageSource {
                canonical_id: self.name.to_string(),
                source: SourceFile::new(
                    SourceId::new(0),
                    format!("{}.meco.md", self.name),
                    &self.source,
                ),
                resolved_imports: vec![],
            }],
        }
    }
}

/// Compiles and generates the cross-platform seed-zero operation contract.
///
/// # Panics
///
/// Panics only when a built-in committed workload violates its own specification.
#[must_use]
pub fn operation_contract(workload: &Workload) -> OperationContract {
    let grammar = compile_package(&workload.package()).expect("committed workload compiles");
    let result = grammar
        .generate_weighted(&GenerationRequest {
            entry: None,
            seed: 0,
            limits: workload_limits(),
            data: &[],
            trace_bindings: false,
            trace_selections: false,
            trace_provenance: false,
        })
        .expect("committed workload generates");
    OperationContract {
        source_bytes: workload.source.len(),
        rules: grammar.rule_count(),
        productions: grammar.production_count(),
        artifact_hash: grammar.artifact_hash(),
        expansions: result.expansions(),
        sampler_words: result.sampler_words(),
        text: result.text().to_string(),
    }
}

/// Returns the stable flat/tree/chain/dense/recursive/fan-out workload suite.
#[must_use]
pub fn workloads() -> Vec<Workload> {
    vec![
        Workload {
            name: "flat-64",
            class: "realistic",
            source: flat(64),
            generations: 1_000,
        },
        Workload {
            name: "tree-dialogue",
            class: "realistic",
            source: tree(),
            generations: 1_000,
        },
        Workload {
            name: "chain-512",
            class: "adversarial",
            source: chain(512),
            generations: 100,
        },
        Workload {
            name: "dense-dag-96x8",
            class: "adversarial",
            source: dense(96, 8),
            generations: 100,
        },
        Workload {
            name: "recursive-balanced",
            class: "realistic",
            source: recursive(),
            generations: 1_000,
        },
        Workload {
            name: "fanout-10000",
            class: "adversarial",
            source: flat(10_000),
            generations: 100,
        },
    ]
}

fn header(module: &str) -> String {
    format!(
        "---\nmeco: 2\nmodule: {module}\nentry: root\nsampler: weighted/1\nexports: [root]\n---\n\n"
    )
}

fn flat(alternatives: u32) -> String {
    let module = if alternatives == 10_000 {
        "fanout-10000"
    } else {
        "flat-64"
    };
    let mut source = header(module);
    source.push_str("# root\n");
    for index in 0..alternatives {
        let _ = writeln!(source, "- alternative-{index}");
    }
    source
}

fn tree() -> String {
    let mut source = header("tree-dialogue");
    source.push_str(
        "# root\n- @speaker @action @object @place.\n\n# speaker\n- The pilot\n- A mechanic\n- The courier\n- Our neighbour\n\n# action\n- inspected\n- repaired\n- carried\n- catalogued\n\n# object\n- the old radio\n- a toolkit\n- the package\n- a navigation chart\n\n# place\n- near the workshop\n- beside the market\n- outside the library\n- under the bridge\n",
    );
    source
}

fn chain(rules: u32) -> String {
    let mut source = header("chain-512");
    for index in 0..rules {
        let _ = writeln!(source, "# r{index}");
        if index + 1 == rules {
            source.push_str("- terminal\n\n");
        } else {
            let _ = writeln!(source, "- @r{}\n", index + 1);
        }
    }
    source.push_str("# root\n- @r0\n");
    source
}

fn dense(rules: u32, width: u32) -> String {
    let mut source = header("dense-dag-96x8");
    source.push_str("# root\n- @n0\n\n");
    for index in 0..rules {
        let _ = writeln!(source, "# n{index}");
        let end = index.saturating_add(width).min(rules - 1);
        if index == rules - 1 {
            source.push_str("- terminal\n\n");
        } else {
            for target in index + 1..=end {
                let _ = writeln!(source, "- @n{target}");
            }
            source.push('\n');
        }
    }
    source
}

fn recursive() -> String {
    let mut source = header("recursive-balanced");
    source.push_str("# root\n- [8] ()\n- [1] (@root)\n- [1] @root@root\n");
    source
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use mecojoni_core::compile_package;

    use super::{WORKLOAD_VERSION, workloads};

    #[test]
    fn workload_names_are_unique_and_every_source_compiles() {
        assert_eq!(WORKLOAD_VERSION, "workloads/1");
        let workloads = workloads();
        let names = workloads
            .iter()
            .map(|workload| workload.name)
            .collect::<BTreeSet<_>>();
        assert_eq!(names.len(), workloads.len());
        for workload in workloads {
            compile_package(&workload.package())
                .unwrap_or_else(|error| panic!("{} failed to compile: {error:?}", workload.name));
        }
    }
}
