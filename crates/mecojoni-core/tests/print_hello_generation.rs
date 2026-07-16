//! A human-readable walkthrough of the README corpus and branching-memory model.
//!
//! Run it with output enabled:
//! `cargo +1.85.0 test -p mecojoni-core --test print_hello_generation -- --nocapture`

use std::{fs, path::PathBuf};

use mecojoni_core::{
    compile_package, parse_module, DataBinding, GenerationRequest, PackageInput, PackageSource,
    SourceFile, SourceId, Value,
};

fn readme_corpus() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../README.md");
    let readme = fs::read_to_string(path).expect("read root README");
    readme
        .split_once("## Complete example corpus")
        .expect("canonical corpus section")
        .1
        .split_once("```meco\n")
        .expect("canonical corpus fence")
        .1
        .split_once("\n```")
        .expect("canonical corpus closing fence")
        .0
        .to_string()
}

fn branching_memory_package() -> PackageInput {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/packages/branching-memory/root.meco");
    PackageInput {
        root_id: "root".to_string(),
        modules: vec![PackageSource {
            canonical_id: "root".to_string(),
            source: SourceFile::new(
                SourceId::new(0),
                "branching-memory/root.meco",
                fs::read_to_string(path).expect("read branching-memory example"),
            ),
            resolved_imports: vec![],
        }],
    }
}

#[test]
fn prints_readme_corpus_and_all_branching_memory_results() {
    let corpus = readme_corpus();
    let corpus_source = SourceFile::new(SourceId::new(0), "README.md#canonical-corpus", corpus);
    let module = parse_module(&corpus_source).expect("README corpus parses");

    println!("README canonical corpus:");
    println!("  module: {}", module.front_matter.module().value());
    println!("  parsed rules: {}", module.rules.len());
    for (index, rule) in module.rules.iter().enumerate() {
        println!("  {:>2}. {}", index + 1, rule.name.value());
    }
    assert!(
        module.rules.len() >= 30,
        "the complete README corpus is present"
    );

    // The README corpus intentionally contains independent syntax examples whose
    // localized-message signatures conflict when combined into one executable
    // package. The checked-in branching-memory package is its runnable host-state
    // example: if the host changes npcPath, the next generation follows that branch.
    let grammar =
        compile_package(&branching_memory_package()).expect("branching-memory example compiles");
    println!("\nBranching and host-persisted NPC memory:");
    for (npc_path, expected) in [
        ("unmet", "I do not know you yet."),
        (
            "cautious",
            "Keep your voice down. We should wait for daylight.",
        ),
        (
            "trusted",
            "Good to see you again. Let us start with the old signal tower.",
        ),
    ] {
        let data = [DataBinding::new(
            "npcPath".to_string(),
            Value::Enum(npc_path.to_string()),
        )];
        let result = grammar
            .generate_weighted(&GenerationRequest {
                trace_selections: true,
                data: &data,
                ..GenerationRequest::with_seed(0)
            })
            .expect("branching-memory example generates");

        println!(
            "\n  npcPath = {npc_path:?}\n    output: {:?}\n    entry: {}\n    work: {} expansions, {} sampler words",
            result.text(),
            result.entry(),
            result.expansions(),
            result.sampler_words(),
        );
        for selection in result.selections() {
            println!(
                "    {} -> production {} ({})",
                selection.rule(),
                selection.selected_production(),
                selection.selected_production_id(),
            );
        }
        assert_eq!(result.text(), expected);
    }
}
