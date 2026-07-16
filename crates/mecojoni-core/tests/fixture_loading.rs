use std::{fs, path::PathBuf};

use mecojoni_core::{
    ArgumentSyntax, BlockChomp, BodyPartSyntax, BodySyntax, ClauseSyntax, CompositionProfile,
    DataBinding, GenerationLimits, GenerationRequest, LocationProfile, PackageInput, PackageSource,
    Rational, ResolvedImport, ResourceProfile, SourceFile, SourceId, SplitMix64, Value,
    ValueSyntax, WeightSyntax, audit_composition, compile_package, diversity_factor_16_16,
    location_cooldown_multiplier, parse_front_matter, parse_module, validate_package_input,
};
use std::str::FromStr;

fn fixture_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative)
}

fn milestone5_package() -> PackageInput {
    let directory = fixture_path("packages/milestone5");
    PackageInput {
        root_id: "root".to_string(),
        modules: vec![
            PackageSource {
                canonical_id: "root".to_string(),
                source: SourceFile::new(
                    SourceId::new(0),
                    "root.meco.md",
                    fs::read_to_string(directory.join("root.meco.md"))
                        .expect("read Milestone 5 root"),
                ),
                resolved_imports: vec![ResolvedImport {
                    authored_path: "./common.meco.md".to_string(),
                    target_id: "common".to_string(),
                }],
            },
            PackageSource {
                canonical_id: "common".to_string(),
                source: SourceFile::new(
                    SourceId::new(1),
                    "common.meco.md",
                    fs::read_to_string(directory.join("common.meco.md"))
                        .expect("read Milestone 5 common module"),
                ),
                resolved_imports: vec![],
            },
        ],
    }
}

fn request_data(mood: &str, urgency: Rational, enabled: bool) -> Vec<DataBinding> {
    vec![
        DataBinding::new("playerName".to_string(), Value::Text("Rin".to_string())),
        DataBinding::new("mood".to_string(), Value::Enum(mood.to_string())),
        DataBinding::new("urgency".to_string(), Value::Number(urgency)),
        DataBinding::new("enabled".to_string(), Value::Boolean(enabled)),
    ]
}

#[test]
fn milestone5_multimodule_fixture_generates_bindings_guards_and_frames() {
    let grammar = compile_package(&milestone5_package()).expect("Milestone 5 package compiles");
    let tense_data = request_data("tense", Rational::new(2, 1).expect("number"), true);
    let first = grammar
        .generate_weighted(&GenerationRequest {
            data: &tense_data,
            trace_bindings: true,
            ..GenerationRequest::with_seed(7)
        })
        .expect("tense fixture generates");
    let replay = grammar
        .generate_weighted(&GenerationRequest {
            data: &tense_data,
            trace_bindings: true,
            ..GenerationRequest::with_seed(7)
        })
        .expect("tense fixture replays");

    assert_eq!(first, replay);
    assert_eq!(first.bindings().len(), 2);
    assert_eq!(first.bindings()[0].name(), "hero");
    assert_eq!(first.bindings()[1].name(), "companion");
    assert!(first.text().contains("Rin") || first.text().contains("watches"));
    let corpus = (0..8)
        .map(|seed| {
            let generated = grammar
                .generate_weighted(&GenerationRequest {
                    data: &tense_data,
                    ..GenerationRequest::with_seed(seed)
                })
                .expect("seed corpus generates");
            format!("{seed}|{}", generated.text())
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        corpus,
        fs::read_to_string(fixture_path("expected/milestone5-seeds-v1.outputs"))
            .expect("read Milestone 5 corpus")
            .trim_end()
    );

    let calm_data = request_data("calm", Rational::ZERO, true);
    let calm = grammar
        .generate_weighted(&GenerationRequest {
            data: &calm_data,
            trace_bindings: true,
            ..GenerationRequest::with_seed(11)
        })
        .expect("calm fixture generates");
    let Value::Text(hero) = calm.bindings()[0].value() else {
        panic!("hero binding is text");
    };
    let Value::Text(witness) = calm.bindings()[1].value() else {
        panic!("witness capture is text");
    };
    assert_eq!(
        calm.text(),
        format!("the crew welcomes {witness}; {witness} greets {hero}.")
    );
    let recursive = grammar
        .generate_weighted(&GenerationRequest {
            entry: Some("scene.recursion"),
            data: &calm_data,
            ..GenerationRequest::with_seed(0)
        })
        .expect("recursive parameter frames generate");
    assert_eq!(recursive.text(), "inner");
}

#[test]
fn milestone5_invalid_files_report_stable_semantic_codes() {
    let cases = [
        ("type-mismatch.meco.md", "E_TYPE_MISMATCH"),
        ("guard-binding.meco.md", "E_VALUE_NAME"),
        ("weight-binding.meco.md", "E_VALUE_NAME"),
        ("forward-capture.meco.md", "E_VALUE_NAME"),
        ("shadow-input.meco.md", "E_BINDING_NAME"),
        ("unused-binding.meco.md", "E_BINDING_NAME"),
    ];
    for (index, (name, expected)) in cases.iter().enumerate() {
        let path = fixture_path(&format!("packages/milestone5-invalid/{name}"));
        let package = PackageInput {
            root_id: "root".to_string(),
            modules: vec![PackageSource {
                canonical_id: "root".to_string(),
                source: SourceFile::new(
                    SourceId::new(u32::try_from(index).expect("fixture index")),
                    path.display().to_string(),
                    fs::read_to_string(path).expect("read invalid Milestone 5 fixture"),
                ),
                resolved_imports: vec![],
            }],
        };
        let error = compile_package(&package).expect_err("invalid semantic fixture must fail");
        assert_eq!(error.diagnostics()[0].code().as_str(), *expected, "{name}");
    }
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
    let modules = sources
        .into_iter()
        .map(|source| {
            let module_name = parse_front_matter(&source)
                .expect("package header parses")
                .module()
                .value()
                .clone();
            let resolved_imports = if module_name == "fixture" {
                vec![ResolvedImport {
                    authored_path: "./common.meco.md".to_string(),
                    target_id: "common".to_string(),
                }]
            } else {
                vec![]
            };
            PackageSource {
                canonical_id: module_name,
                source,
                resolved_imports,
            }
        })
        .collect();
    let package = PackageInput {
        root_id: "fixture".to_string(),
        modules,
    };
    validate_package_input(&package).expect("host-supplied package graph validates");
    let grammar = compile_package(&package).expect("filesystem package compiles");
    let result = grammar
        .generate_weighted(&GenerationRequest {
            entry: Some("fixture.greeting"),
            seed: 0,
            limits: GenerationLimits::default(),
            data: &[],
            trace_bindings: false,
            trace_selections: false,
        })
        .expect("filesystem package generates");
    assert_eq!(result.text(), "Hello, world!");
}

fn load_weighted_package() -> PackageInput {
    let directory = fixture_path("packages/weighted");
    let root_path = directory.join("root.meco.md");
    let common_path = directory.join("common.meco.md");
    PackageInput {
        root_id: "root".to_string(),
        modules: vec![
            PackageSource {
                canonical_id: "root".to_string(),
                source: SourceFile::new(
                    SourceId::new(0),
                    root_path.display().to_string(),
                    fs::read_to_string(root_path).expect("read weighted root"),
                ),
                resolved_imports: vec![ResolvedImport {
                    authored_path: "./common.meco.md".to_string(),
                    target_id: "common".to_string(),
                }],
            },
            PackageSource {
                canonical_id: "common".to_string(),
                source: SourceFile::new(
                    SourceId::new(1),
                    common_path.display().to_string(),
                    fs::read_to_string(common_path).expect("read weighted common"),
                ),
                resolved_imports: vec![],
            },
        ],
    }
}

#[test]
fn weighted_package_matches_the_seeded_filesystem_corpus() {
    let package = load_weighted_package();
    let grammar = compile_package(&package).expect("weighted package compiles");
    let actual = (0..16_u64)
        .map(|seed| {
            let result = grammar
                .generate_weighted(&GenerationRequest::with_seed(seed))
                .expect("seeded generation succeeds");
            format!(
                "{seed}|{}|{}|{}",
                result.text(),
                result.expansions(),
                result.sampler_words()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let expected = fs::read_to_string(fixture_path("expected/weighted-seeds-v1.outputs"))
        .expect("read weighted output corpus");

    assert_eq!(actual, expected.trim_end());
    let explicit = |entry, seed| {
        grammar
            .generate_weighted(&GenerationRequest {
                entry: Some(entry),
                seed,
                limits: GenerationLimits::default(),
                data: &[],
                trace_bindings: false,
                trace_selections: false,
            })
            .expect("explicit entry generates")
    };
    assert!((0..64).any(|seed| explicit("weighted.empty", seed).text().is_empty()));
    assert_eq!(explicit("weighted.literals", 0).text(), "quoted @raw");
    assert_eq!(explicit("weighted.raw-block", 0).text(), "@literal");
    assert!(!explicit("weighted.recursive", 0).text().is_empty());
}

#[test]
fn weighted_selection_matches_its_relative_probability_in_a_seed_corpus() {
    let package = load_weighted_package();
    let grammar = compile_package(&package).expect("weighted package compiles");
    let quiet = (0..4_096_u64)
        .filter(|seed| {
            grammar
                .generate_weighted(&GenerationRequest::with_seed(*seed))
                .expect("statistical corpus generation succeeds")
                .text()
                == "A quiet scene."
        })
        .count();

    assert!((850..=1_200).contains(&quiet), "quiet count was {quiet}");
}

#[test]
fn deep_filesystem_grammar_uses_the_heap_stack_and_exact_limits() {
    let path = fixture_path("packages/deep/root.meco.md");
    let package = PackageInput {
        root_id: "deep".to_string(),
        modules: vec![PackageSource {
            canonical_id: "deep".to_string(),
            source: SourceFile::new(
                SourceId::new(0),
                path.display().to_string(),
                fs::read_to_string(path).expect("read deep grammar"),
            ),
            resolved_imports: vec![],
        }],
    };
    let grammar = compile_package(&package).expect("deep grammar compiles iteratively");
    let limited = grammar
        .generate_weighted(&GenerationRequest::with_seed(0))
        .expect_err("interactive depth limit is exact");
    assert_eq!(limited.diagnostics()[0].code().as_str(), "E_LIMIT_DEPTH");

    let result = grammar
        .generate_weighted(&GenerationRequest {
            entry: None,
            seed: 0,
            limits: GenerationLimits {
                max_depth: 2_048,
                max_expansions: 2_048,
                ..GenerationLimits::default()
            },
            data: &[],
            trace_bindings: false,
            trace_selections: false,
        })
        .expect("looser named test limits permit the whole chain");
    assert_eq!(result.text(), "finished");
    assert_eq!(result.expansions(), 2_048);
    assert_eq!(result.sampler_words(), 2_048);
}

fn load_compiler_invalid_package(case: &str) -> PackageInput {
    let directory = fixture_path(&format!("packages/compiler-invalid/{case}"));
    let root_path = directory.join("root.meco.md");
    let mut modules = vec![PackageSource {
        canonical_id: "root".to_string(),
        source: SourceFile::new(
            SourceId::new(0),
            root_path.display().to_string(),
            fs::read_to_string(&root_path).expect("read invalid compiler root"),
        ),
        resolved_imports: vec![],
    }];
    if case != "undefined" {
        let common_path = directory.join("common.meco.md");
        modules.push(PackageSource {
            canonical_id: "common".to_string(),
            source: SourceFile::new(
                SourceId::new(1),
                common_path.display().to_string(),
                fs::read_to_string(common_path).expect("read invalid compiler dependency"),
            ),
            resolved_imports: vec![],
        });
        modules[0].resolved_imports.push(ResolvedImport {
            authored_path: "./common.meco.md".to_string(),
            target_id: "common".to_string(),
        });
        if case == "cycle" {
            modules[1].resolved_imports.push(ResolvedImport {
                authored_path: "./root.meco.md".to_string(),
                target_id: "root".to_string(),
            });
        }
    }
    PackageInput {
        root_id: "root".to_string(),
        modules,
    }
}

#[test]
fn compiler_failures_match_codes_and_spans_from_filesystem_packages() {
    let expected = fs::read_to_string(fixture_path("expected/compiler-invalid-v1.diags"))
        .expect("read compiler diagnostic contract");
    let mut actual = Vec::new();
    for case in ["undefined", "private", "cycle"] {
        let package = load_compiler_invalid_package(case);
        let error = compile_package(&package).expect_err("compiler fixture must fail");
        let diagnostic = &error.diagnostics()[0];
        let source_slice = diagnostic.span().map_or_else(
            || "<none>".to_string(),
            |diagnostic_span| {
                let source = &package.modules
                    [usize::try_from(diagnostic_span.source().get()).expect("source ID fits")]
                .source;
                let start = usize::try_from(diagnostic_span.start().byte()).expect("start fits");
                let end = usize::try_from(diagnostic_span.end().byte()).expect("end fits");
                source.text()[start..end].to_string()
            },
        );
        actual.push(format!(
            "{case}|{}|{source_slice}",
            diagnostic.code().as_str()
        ));
    }

    assert_eq!(actual.join("\n"), expected.trim_end());
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

#[test]
fn deterministic_prng_matches_the_checked_in_cross_runtime_vector() {
    let expected_path = fixture_path("expected/splitmix64-seed-0.words");
    let expected = fs::read_to_string(expected_path).expect("read PRNG vector");
    let mut random = SplitMix64::new(0);
    let actual = (0..4)
        .map(|_| format!("{:016x}", random.next_u64()))
        .collect::<Vec<_>>()
        .join("\n");

    assert_eq!(actual, expected.trim());
}

#[test]
fn exact_numbers_match_the_checked_in_cross_runtime_cases() {
    let cases_path = fixture_path("expected/rational-v1.cases");
    let cases = fs::read_to_string(cases_path).expect("read rational cases");

    for line in cases.lines() {
        let (source, expected) = line.split_once(" = ").expect("fixture case delimiter");
        let actual = Rational::from_str(source).expect("fixture number parses");
        assert_eq!(
            actual.to_string(),
            expected,
            "unexpected value for {source}"
        );
    }
}

#[test]
fn named_profiles_match_the_checked_in_contract() {
    let expected_path = fixture_path("expected/profiles-v1.contract");
    let expected = fs::read_to_string(expected_path).expect("read profile contract");
    let location = LocationProfile::V1;
    let weighted = ResourceProfile::WEIGHTED_INTERACTIVE_V1;
    let diverse = ResourceProfile::DIVERSE_INTERACTIVE_V1;
    let composition = CompositionProfile::V1;
    let actual = format!(
        concat!(
            "location/1 candidates={} gap={} horizon={} cooldown={}/{} edges={}..{} internal={} edge_window={} exact_window={}\n",
            "interactive/1 weighted candidates={} depth={} expansions={} scalars={} sampler={} aggregate_expansions={} aggregate_sampler={} rendered_scalars={} rendered_bytes={} formatter={}\n",
            "interactive/1 diverse candidates={} depth={} expansions={} scalars={} sampler={} aggregate_expansions={} aggregate_sampler={} rendered_scalars={} rendered_bytes={} formatter={}\n",
            "composition/1 references={} literal_words={} messages_exempt={}\n",
            "location/1 math factor(1)={} factor(2)={} factor(32768)={} cooldown(1)={} cooldown(2)={} cooldown(3)={} cooldown(4)={}"
        ),
        location.candidate_attempts,
        location.hard_minimum_gap,
        location.soft_cooldown_horizon,
        location.soft_cooldown_numerator,
        location.soft_cooldown_denominator,
        location.minimum_edge_words,
        location.maximum_edge_words,
        location.internal_boundary_words,
        location.edge_history_window,
        location.exact_history_window,
        weighted.candidate_attempts,
        weighted.maximum_depth_per_candidate,
        weighted.maximum_expansions_per_candidate,
        weighted.maximum_unformatted_scalars_per_candidate,
        weighted.maximum_sampler_steps_per_candidate,
        weighted.maximum_aggregate_expansions,
        weighted.maximum_aggregate_sampler_steps,
        weighted.maximum_rendered_scalars,
        weighted.maximum_rendered_utf8_bytes,
        weighted.maximum_formatter_work_units,
        diverse.candidate_attempts,
        diverse.maximum_depth_per_candidate,
        diverse.maximum_expansions_per_candidate,
        diverse.maximum_unformatted_scalars_per_candidate,
        diverse.maximum_sampler_steps_per_candidate,
        diverse.maximum_aggregate_expansions,
        diverse.maximum_aggregate_sampler_steps,
        diverse.maximum_rendered_scalars,
        diverse.maximum_rendered_utf8_bytes,
        diverse.maximum_formatter_work_units,
        composition.minimum_direct_references,
        composition.maximum_literal_run_words,
        composition.complete_messages_are_exempt,
        diversity_factor_16_16(1),
        diversity_factor_16_16(2),
        diversity_factor_16_16(32_768),
        location_cooldown_multiplier(1).expect("cooldown age 1"),
        location_cooldown_multiplier(2).expect("cooldown age 2"),
        location_cooldown_multiplier(3).expect("cooldown age 3"),
        location_cooldown_multiplier(4).expect("cooldown age 4"),
    );

    assert_eq!(actual, expected.trim());
}

#[test]
fn canonical_readme_corpus_parses_from_the_filesystem() {
    let readme_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../README.md");
    let readme = fs::read_to_string(readme_path).expect("read root README");
    let section = readme
        .split_once("## Complete v2 example corpus")
        .expect("canonical corpus section")
        .1;
    let source_text = section
        .split_once("```meco\n")
        .expect("canonical corpus fence")
        .1
        .split_once("\n```")
        .expect("canonical corpus closing fence")
        .0;
    let source = SourceFile::new(SourceId::new(0), "README.md#canonical-corpus", source_text);
    let module = parse_module(&source).expect("canonical README corpus parses");

    assert_eq!(module.front_matter.module().value(), "npc");
    assert!(module.rules.len() >= 30);
    assert!(
        module
            .rules
            .iter()
            .any(|rule| rule.name.value() == "raw-sigils")
    );
    assert!(
        module
            .rules
            .iter()
            .any(|rule| rule.name.value() == "inventory")
    );
}

fn argument_signature(argument: &ArgumentSyntax) -> String {
    let value = match &argument.value {
        ValueSyntax::Reference(value) => format!("${}", value.value()),
        ValueSyntax::Number(value) => value.value().to_string(),
        ValueSyntax::Text(value) => format!("{:?}", value.value()),
        ValueSyntax::Boolean(value) => value.value().to_string(),
    };
    format!(
        "{}={value}{}",
        argument.name.value(),
        if argument.punned { "~" } else { "" }
    )
}

fn body_signature(body: &BodySyntax) -> String {
    match body {
        BodySyntax::Empty(_) => "EMPTY".to_string(),
        BodySyntax::Block(block) => format!(
            "BLOCK:{}:{}:{:?}",
            if block.raw { "raw" } else { "cooked" },
            match block.chomp {
                BlockChomp::Clip => "clip",
                BlockChomp::Strip => "strip",
                BlockChomp::Keep => "keep",
            },
            block.text.value()
        ),
        BodySyntax::Parts(parts) => parts
            .iter()
            .map(|part| match part {
                BodyPartSyntax::Literal(value) => format!("L:{:?}", value.value()),
                BodyPartSyntax::RuleReference(value) => format!("R:{}", value.value()),
                BodyPartSyntax::EmittingCapture { rule, name, .. } => {
                    format!("C:{}>{}", rule.value(), name.value())
                }
                BodyPartSyntax::ValueReference(value) => format!("V:{}", value.value()),
                BodyPartSyntax::RuleCall(call) | BodyPartSyntax::MessageCall(call) => format!(
                    "{}:{}({})",
                    if matches!(part, BodyPartSyntax::RuleCall(_)) {
                        "RC"
                    } else {
                        "MC"
                    },
                    call.target.value(),
                    call.arguments
                        .iter()
                        .map(argument_signature)
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn module_ast_summary(module: &mecojoni_core::ModuleSyntax) -> String {
    let mut lines = Vec::new();
    for rule in &module.rules {
        let parameters = rule
            .parameters
            .iter()
            .map(|parameter| format!("{}:{}", parameter.name.value(), parameter.type_name.value()))
            .collect::<Vec<_>>()
            .join(",");
        lines.push(format!("rule {}({parameters})", rule.name.value()));
        for (index, production) in rule.productions.iter().enumerate() {
            let weight = match &production.weight {
                WeightSyntax::Default => "1".to_string(),
                WeightSyntax::Static(value) => value.value().to_string(),
                WeightSyntax::Dynamic(value) => format!("expr:{:?}", value.value()),
            };
            let clauses = production
                .clauses
                .iter()
                .map(|clause| match clause {
                    ClauseSyntax::Guard(guard) => format!("G:{:?}", guard.value()),
                    ClauseSyntax::Binding(binding) => {
                        format!("B:{}>{}", binding.rule.value(), binding.name.value())
                    }
                })
                .collect::<Vec<_>>()
                .join(",");
            lines.push(format!(
                "  p{index} w={weight} id={} c=[{clauses}] b={}",
                production.authored_id.as_ref().map_or("-", |id| id.value()),
                body_signature(&production.body)
            ));
        }
    }
    lines.join("\n")
}

#[test]
fn canonical_readme_corpus_matches_the_checked_in_ast_prediction() {
    let readme_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../README.md");
    let readme = fs::read_to_string(readme_path).expect("read root README");
    let source_text = readme
        .split_once("## Complete v2 example corpus")
        .expect("canonical corpus section")
        .1
        .split_once("```meco\n")
        .expect("canonical corpus fence")
        .1
        .split_once("\n```")
        .expect("canonical corpus closing fence")
        .0;
    let source = SourceFile::new(SourceId::new(0), "README.md#canonical-corpus", source_text);
    let module = parse_module(&source).expect("canonical corpus parses");
    let actual = module_ast_summary(&module);
    let expected = fs::read_to_string(fixture_path("expected/readme-corpus.ast"))
        .expect("read AST prediction");

    assert_eq!(actual, expected.trim_end());
}

#[test]
fn invalid_body_fixtures_match_codes_and_exact_source_spans() {
    let cases = [
        "body-guard-order",
        "body-unknown-escape",
        "body-zero-weight",
        "body-unicode-rule",
        "body-unterminated-comment",
    ];

    for (index, case) in cases.iter().enumerate() {
        let source_path = fixture_path(&format!("invalid/{case}.meco.md"));
        let expected_path = fixture_path(&format!("expected/{case}.diag"));
        let source_text = fs::read_to_string(&source_path).expect("read invalid body fixture");
        let expected = fs::read_to_string(expected_path).expect("read expected body diagnostic");
        let (expected_code, expected_slice) = expected
            .trim_end_matches('\n')
            .split_once('\n')
            .expect("diagnostic fixture has code and slice");
        let source = SourceFile::new(
            SourceId::new(u32::try_from(index).expect("fixture index fits")),
            source_path.display().to_string(),
            source_text,
        );
        let error = parse_module(&source).expect_err("invalid body fixture must fail");
        let diagnostic = &error.diagnostics()[0];
        let diagnostic_span = diagnostic.span().expect("body diagnostic has a span");
        let start = usize::try_from(diagnostic_span.start().byte()).expect("span start fits");
        let end = usize::try_from(diagnostic_span.end().byte()).expect("span end fits");

        assert_eq!(
            diagnostic.code().as_str(),
            expected_code,
            "wrong code for {case}"
        );
        assert_eq!(
            &source.text()[start..end],
            expected_slice,
            "wrong span for {case}"
        );
    }
}

#[test]
fn independent_syntax_errors_are_aggregated_from_a_real_fixture() {
    let source_path = fixture_path("invalid/independent-errors.meco.md");
    let expected_path = fixture_path("expected/independent-errors.diags");
    let source_text = fs::read_to_string(&source_path).expect("read recovery fixture");
    let expected = fs::read_to_string(expected_path).expect("read expected diagnostics");
    let source = SourceFile::new(
        SourceId::new(0),
        source_path.display().to_string(),
        source_text,
    );
    let error = parse_module(&source).expect_err("recovery fixture must fail");
    let actual = error
        .diagnostics()
        .iter()
        .map(|diagnostic| {
            let diagnostic_span = diagnostic.span().expect("parser diagnostic has a span");
            let start = usize::try_from(diagnostic_span.start().byte()).expect("start fits");
            let end = usize::try_from(diagnostic_span.end().byte()).expect("end fits");
            format!(
                "{}|{}",
                diagnostic.code().as_str(),
                &source.text()[start..end]
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert_eq!(actual, expected.trim_end());
}

#[test]
fn composition_audit_matches_the_checked_in_finding_contract() {
    let source_path = fixture_path("valid/composition-audit.meco.md");
    let expected_path = fixture_path("expected/composition-audit.findings");
    let source_text = fs::read_to_string(&source_path).expect("read audit source");
    let expected = fs::read_to_string(expected_path).expect("read audit findings");
    let source = SourceFile::new(
        SourceId::new(0),
        source_path.display().to_string(),
        source_text,
    );
    let module = parse_module(&source).expect("audit source parses");
    let actual = audit_composition(&module)
        .into_iter()
        .map(|finding| {
            format!(
                "{}|{}|{}|{}|{}|{}",
                finding.rule,
                finding.production_index,
                finding.direct_references,
                finding.longest_literal_run,
                finding.insufficient_references,
                finding.excessive_literal_run,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert_eq!(actual, expected.trim());
}

#[test]
fn cli_contract_fixture_covers_every_stream_and_status_class() {
    let contract_path = fixture_path("expected/cli-v1.contract");
    let contract = fs::read_to_string(contract_path).expect("read CLI contract");
    let rows = contract.lines().collect::<Vec<_>>();
    let interfaces_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../V2_INTERFACES.md");
    let interfaces = fs::read_to_string(interfaces_path).expect("read interface contract");

    assert_eq!(rows.len(), 7);
    assert!(rows.iter().any(|row| row.ends_with("|0")));
    assert!(rows.iter().any(|row| row.ends_with("|1")));
    assert!(rows.iter().any(|row| row.ends_with("|2")));
    assert!(rows.iter().any(|row| row.ends_with("|3")));
    assert!(interfaces.contains("CLI streams and statuses (`cli/1`)"));
    assert!(interfaces.contains("Deno is the"));
    assert!(interfaces.contains("normative JS integration host"));
}

#[test]
fn unicode_terminal_text_and_crlf_normalization_match_from_a_real_fixture() {
    let path = fixture_path("valid/unicode-terminal.meco.md");
    let lf = fs::read_to_string(&path).expect("read Unicode fixture");
    let crlf = lf.replace('\n', "\r\n");
    let lf_source = SourceFile::new(SourceId::new(0), path.display().to_string(), lf);
    let crlf_source = SourceFile::new(SourceId::new(1), "unicode-terminal-crlf.meco.md", crlf);
    let lf_module = parse_module(&lf_source).expect("LF Unicode fixture parses");
    let crlf_module = parse_module(&crlf_source).expect("CRLF Unicode fixture parses");

    assert_eq!(
        body_signature(&lf_module.rules[0].productions[0].body),
        "L:\"Héllo, 世界 🦀.\""
    );
    assert_eq!(
        body_signature(&lf_module.rules[1].productions[0].body),
        "BLOCK:cooked:strip:\"café\\n世界\""
    );
    assert_eq!(
        body_signature(&lf_module.rules[0].productions[0].body),
        body_signature(&crlf_module.rules[0].productions[0].body)
    );
    assert_eq!(
        body_signature(&lf_module.rules[1].productions[0].body),
        body_signature(&crlf_module.rules[1].productions[0].body)
    );
    let literal_span = match &crlf_module.rules[0].productions[0].body {
        BodySyntax::Parts(parts) => match &parts[0] {
            BodyPartSyntax::Literal(literal) => literal.span(),
            _ => panic!("expected literal"),
        },
        _ => panic!("expected inline body"),
    };
    assert!(literal_span.byte_len() > literal_span.scalar_len());
}

#[test]
fn cooked_block_interpolation_and_raw_blocks_are_distinct_in_a_real_fixture() {
    let path = fixture_path("valid/cooked-block.meco.md");
    let source_text = fs::read_to_string(&path).expect("read block fixture");
    let source = SourceFile::new(SourceId::new(0), path.display().to_string(), source_text);
    let module = parse_module(&source).expect("block fixture parses");
    let BodySyntax::Block(cooked) = &module.rules[0].productions[0].body else {
        panic!("expected cooked block");
    };
    let BodySyntax::Block(kept) = &module.rules[1].productions[0].body else {
        panic!("expected kept block");
    };
    let BodySyntax::Block(raw) = &module.rules[2].productions[0].body else {
        panic!("expected raw block");
    };

    let cooked_parts = cooked
        .parts
        .as_ref()
        .expect("cooked block has parsed parts");
    assert!(cooked_parts.iter().any(
        |part| matches!(part, BodyPartSyntax::RuleReference(name) if name.value() == "person")
    ));
    assert!(cooked_parts.iter().any(
        |part| matches!(part, BodyPartSyntax::ValueReference(name) if name.value() == "playerName")
    ));
    assert!(
        cooked_parts
            .iter()
            .any(|part| matches!(part, BodyPartSyntax::Literal(text) if text.value() == "\n"))
    );
    assert_eq!(
        cooked.text.value(),
        "Hello, @person.\nWelcome, $playerName!"
    );
    assert_eq!(kept.text.value(), "  @person\n");
    assert!(
        kept.parts
            .as_ref()
            .expect("cooked kept parts")
            .iter()
            .any(|part| matches!(part, BodyPartSyntax::Literal(text) if text.value() == "  "))
    );
    assert!(raw.raw);
    assert!(raw.parts.is_none());
    assert_eq!(raw.text.value(), "@person and $playerName stay literal.");
}
