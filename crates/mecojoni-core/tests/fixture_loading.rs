use std::{fs, path::PathBuf};

use mecojoni_core::{
    ArgumentSyntax, BlockChomp, BodyPartSyntax, BodySyntax, ClauseSyntax, CompiledGrammar,
    CompositionProfile, DataBinding, Diagnostic, DiagnosticCode, DiverseGenerationRequest,
    Formatter, FormatterRequest, FormatterResult, GenerationLimits, GenerationRequest,
    LocaleRequest, LocationProfile, MecoError, MecoResult, MessageArgument, MessageDefinition,
    MessageManifest, PackageInput, PackageSource, Rational, RepetitionStore, ResolvedImport,
    ResourceProfile, SamplerSession, SchemaType, Severity, SourceFile, SourceId, SplitMix64, Value,
    ValueSyntax, WeightSyntax, audit_composition, audit_rendered_repetition,
    audit_structural_repetition, compile_package, compile_package_with_manifest,
    diversity_factor_16_16, location_cooldown_multiplier, parse_front_matter, parse_module,
    validate_package_input,
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
                    "root.meco",
                    fs::read_to_string(directory.join("root.meco")).expect("read Milestone 5 root"),
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
                    fs::read_to_string(directory.join("common.meco"))
                        .expect("read Milestone 5 common module"),
                ),
                resolved_imports: vec![],
            },
        ],
    }
}

#[test]
fn filesystem_metadata_forms_evaluate_host_inputs_and_rule_parameters() {
    let path = fixture_path("packages/metadata/root.meco");
    let package = PackageInput {
        root_id: "metadata".to_string(),
        modules: vec![PackageSource {
            canonical_id: "metadata".to_string(),
            source: SourceFile::new(
                SourceId::new(0),
                "metadata/root.meco",
                fs::read_to_string(path).expect("read metadata fixture"),
            ),
            resolved_imports: vec![],
        }],
    };
    let grammar = compile_package(&package).expect("metadata fixture compiles");
    let data = [DataBinding::new(
        "inputValue".to_string(),
        Value::Number(Rational::new(2, 1).expect("number")),
    )];
    let result = grammar
        .generate_weighted(&GenerationRequest {
            data: &data,
            trace_selections: true,
            ..GenerationRequest::with_seed(7)
        })
        .expect("metadata fixture generates");
    let choice = result
        .selections()
        .iter()
        .find(|selection| selection.rule() == "metadata.choice")
        .expect("parameterized selection is traced");
    let weights = choice
        .eligible()
        .iter()
        .map(|candidate| (candidate.production_id(), candidate.base_weight()))
        .collect::<Vec<_>>();
    assert_eq!(weights[0], ("explicit", Rational::new(42, 1).expect("number")));
    assert_eq!(weights[1].1, Rational::new(42, 1).expect("number"));
    assert_eq!(weights[2], ("stable-default", Rational::ONE));
    assert_eq!(weights[3], ("static-three", Rational::new(3, 1).expect("number")));
}

fn request_data(mood: &str, urgency: Rational, enabled: bool) -> Vec<DataBinding> {
    vec![
        DataBinding::new("playerName".to_string(), Value::Text("Rin".to_string())),
        DataBinding::new("mood".to_string(), Value::Enum(mood.to_string())),
        DataBinding::new("urgency".to_string(), Value::Number(urgency)),
        DataBinding::new("enabled".to_string(), Value::Boolean(enabled)),
    ]
}

fn milestone6_manifest(directory: &std::path::Path) -> MessageManifest {
    let source = fs::read_to_string(directory.join("messages.manifest"))
        .expect("read Milestone 6 message manifest");
    let messages = source
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (id, raw_arguments) = line.split_once('|').expect("manifest message separator");
            let arguments = raw_arguments
                .split(',')
                .map(|argument| {
                    let (name, type_name) =
                        argument.split_once(':').expect("manifest type separator");
                    let type_ = match type_name {
                        "text" => SchemaType::Text,
                        "number" => SchemaType::Number,
                        "boolean" => SchemaType::Boolean,
                        other => SchemaType::Enum(other.to_string()),
                    };
                    MessageArgument {
                        name: name.to_string(),
                        type_,
                    }
                })
                .collect();
            MessageDefinition {
                id: id.to_string(),
                arguments,
            }
        })
        .collect();
    MessageManifest { messages }
}

fn single_file_package(relative: &str) -> PackageInput {
    let path = fixture_path(relative);
    PackageInput {
        root_id: "root".to_string(),
        modules: vec![PackageSource {
            canonical_id: "root".to_string(),
            source: SourceFile::new(
                SourceId::new(0),
                path.display().to_string(),
                fs::read_to_string(path).expect("read single-file package"),
            ),
            resolved_imports: vec![],
        }],
    }
}

#[test]
fn filesystem_fixture_models_branching_with_host_persisted_npc_memory() {
    let package = single_file_package("packages/branching-memory/root.meco");
    let grammar = compile_package(&package).expect("branching-memory fixture compiles");

    // This value represents the host application's saved NPC record. Mecojoni
    // reads it as an input for each independent generation; it never mutates it.
    let mut saved_npc_path = "unmet";
    let generate = |npc_path: &str| {
        let data = [DataBinding::new(
            "npcPath".to_string(),
            Value::Enum(npc_path.to_string()),
        )];
        grammar
            .generate_weighted(&GenerationRequest {
                data: &data,
                ..GenerationRequest::with_seed(0)
            })
            .expect("branching-memory fixture generates")
    };

    assert_eq!(generate(saved_npc_path).text(), "I do not know you yet.");

    // A player action changes the host-owned record. The next request enters
    // the corresponding rule-call branch and follows it to its decision.
    saved_npc_path = "cautious";
    assert_eq!(
        generate(saved_npc_path).text(),
        "Keep your voice down. We should wait for daylight."
    );

    saved_npc_path = "trusted";
    assert_eq!(
        generate(saved_npc_path).text(),
        "Good to see you again. Let us start with the old signal tower."
    );
}

struct FixtureFormatter {
    catalogs: Vec<(String, Vec<(String, String)>)>,
}

impl FixtureFormatter {
    fn load(directory: &std::path::Path, locales: &[&str]) -> Self {
        let catalogs = locales
            .iter()
            .map(|locale| {
                let catalog = fs::read_to_string(directory.join(format!("{locale}.catalog")))
                    .expect("read locale fixture")
                    .lines()
                    .map(|line| {
                        let (category, pattern) =
                            line.split_once('=').expect("catalog category separator");
                        (category.to_string(), pattern.to_string())
                    })
                    .collect();
                ((*locale).to_string(), catalog)
            })
            .collect();
        Self { catalogs }
    }
}

impl Formatter for FixtureFormatter {
    fn format(&mut self, request: &FormatterRequest) -> MecoResult<FormatterResult> {
        let actual_locale = core::iter::once(request.requested_locale())
            .chain(request.fallback_locales().iter().map(String::as_str))
            .find(|locale| self.catalogs.iter().any(|(known, _)| known == locale))
            .ok_or_else(|| {
                MecoError::new(Diagnostic::new(
                    DiagnosticCode::FORMATTER,
                    Severity::Error,
                    None,
                    "no requested or fallback catalog is loaded",
                ))
            })?
            .to_string();
        let catalog = self
            .catalogs
            .iter()
            .find(|(locale, _)| locale == &actual_locale)
            .expect("selected catalog exists");
        let Value::Text(hero) = &request.arguments()[0].1 else {
            panic!("fixture hero is text")
        };
        let Value::Number(count) = request.arguments()[1].1 else {
            panic!("fixture count is numeric")
        };
        assert_eq!(count.denominator(), 1);
        let number = count.numerator();
        let category = if actual_locale == "pl" {
            let mod10 = number.rem_euclid(10);
            let mod100 = number.rem_euclid(100);
            if mod10 == 1 && mod100 != 11 {
                "one"
            } else if (2..=4).contains(&mod10) && !(12..=14).contains(&mod100) {
                "few"
            } else {
                "many"
            }
        } else if number == 1 {
            "one"
        } else {
            "other"
        };
        let pattern = catalog
            .1
            .iter()
            .find(|(candidate, _)| candidate == category)
            .expect("plural category exists")
            .1
            .replace("{hero}", hero)
            .replace("{count}", &count.to_string());
        Ok(FormatterResult {
            text: pattern,
            actual_locale: actual_locale.clone(),
            environment_hash: format!("fixture/{actual_locale}/current"),
            diagnostics: vec![],
            work_units: 1,
            replayable: true,
        })
    }
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
        fs::read_to_string(fixture_path("expected/milestone5-seeds.outputs"))
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
        ("type-mismatch.meco", "E_TYPE_MISMATCH"),
        ("guard-binding.meco", "E_VALUE_NAME"),
        ("weight-binding.meco", "E_VALUE_NAME"),
        ("forward-capture.meco", "E_VALUE_NAME"),
        ("shadow-input.meco", "E_BINDING_NAME"),
        ("unused-binding.meco", "E_BINDING_NAME"),
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
    let path = fixture_path("valid/minimal.meco");
    let bytes = fs::read(&path).expect("read fixture");
    let source = SourceFile::from_utf8(SourceId::new(0), path.display().to_string(), &bytes)
        .expect("fixture is valid UTF-8");

    assert_eq!(source.id(), SourceId::new(0));
    assert!(source.name().ends_with("minimal.meco"));
    assert!(source.text().contains("meco: 1.0"));
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
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "meco")
        })
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
                    authored_path: "./common.meco".to_string(),
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
            trace_provenance: false,
        })
        .expect("filesystem package generates");
    assert_eq!(result.text(), "Hello, world!");
}

fn load_weighted_package() -> PackageInput {
    let directory = fixture_path("packages/weighted");
    let root_path = directory.join("root.meco");
    let common_path = directory.join("common.meco");
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
                    authored_path: "./common.meco".to_string(),
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
    let expected = fs::read_to_string(fixture_path("expected/weighted-seeds.outputs"))
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
                trace_provenance: false,
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
    let path = fixture_path("packages/deep/root.meco");
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
            trace_provenance: false,
        })
        .expect("looser named test limits permit the whole chain");
    assert_eq!(result.text(), "finished");
    assert_eq!(result.expansions(), 2_048);
    assert_eq!(result.sampler_words(), 2_048);
}

fn load_compiler_invalid_package(case: &str) -> PackageInput {
    let directory = fixture_path(&format!("packages/compiler-invalid/{case}"));
    let root_path = directory.join("root.meco");
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
        let common_path = directory.join("common.meco");
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
            authored_path: "./common.meco".to_string(),
            target_id: "common".to_string(),
        });
        if case == "cycle" {
            modules[1].resolved_imports.push(ResolvedImport {
                authored_path: "./root.meco".to_string(),
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
    let expected = fs::read_to_string(fixture_path("expected/compiler-invalid.diags"))
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
        let source_path = fixture_path(&format!("invalid/{case}.meco"));
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
    let cases_path = fixture_path("expected/rational.cases");
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
    let expected_path = fixture_path("expected/profiles.contract");
    let expected = fs::read_to_string(expected_path).expect("read profile contract");
    let location = LocationProfile::DEFAULT;
    let weighted = ResourceProfile::WEIGHTED_INTERACTIVE;
    let diverse = ResourceProfile::DIVERSE_INTERACTIVE;
    let composition = CompositionProfile::DEFAULT;
    let actual = format!(
        concat!(
            "location/1 candidates={} gap={} horizon={} cooldown={}/{} edges={}..{} internal={} edge_window={} exact_window={} edge_bytes={} exact_bytes={}\n",
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
        location.edge_history_logical_bytes,
        location.exact_history_logical_bytes,
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
        .split_once("## Complete example corpus")
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
        .split_once("## Complete example corpus")
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
        let source_path = fixture_path(&format!("invalid/{case}.meco"));
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
    let source_path = fixture_path("invalid/independent-errors.meco");
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
    let source_path = fixture_path("valid/composition-audit.meco");
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
    let contract_path = fixture_path("expected/cli.contract");
    let contract = fs::read_to_string(contract_path).expect("read CLI contract");
    let rows = contract.lines().collect::<Vec<_>>();
    let interfaces_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../INTERFACES.md");
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
fn deterministic_filesystem_mutation_corpus_never_panics_or_accepts_invalid_utf8() {
    let path = fixture_path("packages/milestone5/root.meco");
    let original = fs::read(&path).expect("read canonical mutation seed");
    let replacements = [0_u8, b'@', b'#', b'"', 0x7f, 0xff];
    let step = (original.len() / 128).max(1);
    let mut case = 0_u32;
    for offset in (0..original.len()).step_by(step) {
        for replacement in replacements {
            let mut mutated = original.clone();
            mutated[offset] = replacement;
            if let Ok(source) = SourceFile::from_utf8(
                SourceId::new(case),
                format!("mutation-{case}.meco"),
                &mutated,
            ) {
                let _ = parse_module(&source);
            }
            case = case.saturating_add(1);
        }
    }
    for length in (0..original.len()).step_by(step) {
        if let Ok(source) = SourceFile::from_utf8(
            SourceId::new(case),
            format!("truncated-{length}.meco"),
            &original[..length],
        ) {
            let _ = parse_module(&source);
        }
        case = case.saturating_add(1);
    }
    assert!(case >= 800, "mutation corpus unexpectedly small");
}

#[test]
fn unicode_terminal_text_and_crlf_normalization_match_from_a_real_fixture() {
    let path = fixture_path("valid/unicode-terminal.meco");
    let lf = fs::read_to_string(&path).expect("read Unicode fixture");
    let crlf = lf.replace('\n', "\r\n");
    let lf_source = SourceFile::new(SourceId::new(0), path.display().to_string(), lf);
    let crlf_source = SourceFile::new(SourceId::new(1), "unicode-terminal-crlf.meco", crlf);
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
    let path = fixture_path("valid/cooked-block.meco");
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

#[test]
fn milestone6_files_format_english_polish_categories_and_explicit_fallback() {
    let directory = fixture_path("packages/milestone6");
    let root_path = directory.join("root.meco");
    let package = PackageInput {
        root_id: "root".to_string(),
        modules: vec![PackageSource {
            canonical_id: "root".to_string(),
            source: SourceFile::new(
                SourceId::new(0),
                root_path.display().to_string(),
                fs::read_to_string(root_path).expect("read Milestone 6 grammar"),
            ),
            resolved_imports: vec![],
        }],
    };
    let manifest = milestone6_manifest(&directory);
    let grammar = compile_package_with_manifest(&package, &manifest)
        .expect("Milestone 6 package and manifest compile");
    let mut formatter = FixtureFormatter::load(&directory, &["en", "pl"]);

    for (locale, count, ending) in [
        ("en", 1, "arrived with one item."),
        ("en", 2, "arrived with 2 items."),
        ("pl", 1, "przybył z jednym przedmiotem."),
        ("pl", 2, "przybył z 2 przedmiotami."),
        ("pl", 5, "przybył z 5 przedmiotów."),
    ] {
        let values = vec![DataBinding::new(
            "itemCount".to_string(),
            Value::Number(Rational::new(count, 1).expect("fixture count")),
        )];
        let generated = grammar
            .generate_weighted_with_formatter(
                &GenerationRequest {
                    data: &values,
                    ..GenerationRequest::with_seed(0)
                },
                LocaleRequest {
                    requested: locale,
                    fallbacks: &[],
                },
                &mut formatter,
            )
            .expect("localized fixture generates");
        assert!(generated.text().ends_with(ending), "{locale}/{count}");
        assert_eq!(
            generated.message().expect("message trace").actual_locale(),
            locale
        );
    }

    let values = vec![DataBinding::new(
        "itemCount".to_string(),
        Value::Number(Rational::ONE),
    )];
    let fallback = grammar
        .generate_weighted_with_formatter(
            &GenerationRequest {
                data: &values,
                ..GenerationRequest::with_seed(0)
            },
            LocaleRequest {
                requested: "fr",
                fallbacks: &["en"],
            },
            &mut formatter,
        )
        .expect("ordered fallback resolves");
    let trace = fallback.message().expect("fallback message trace");
    assert_eq!(trace.requested_locale(), "fr");
    assert_eq!(trace.actual_locale(), "en");
}

#[test]
fn milestone6_invalid_files_report_missing_messages_and_schema_drift() {
    let directory = fixture_path("packages/milestone6");
    let manifest = milestone6_manifest(&directory);
    for (index, (name, code)) in [
        ("missing-message.meco", DiagnosticCode::MESSAGE_MISSING),
        ("schema-drift.meco", DiagnosticCode::MESSAGE_ARGUMENT),
    ]
    .iter()
    .enumerate()
    {
        let path = fixture_path(&format!("packages/milestone6-invalid/{name}"));
        let package = PackageInput {
            root_id: "root".to_string(),
            modules: vec![PackageSource {
                canonical_id: "root".to_string(),
                source: SourceFile::new(
                    SourceId::new(u32::try_from(index).expect("fixture index")),
                    path.display().to_string(),
                    fs::read_to_string(path).expect("read invalid Milestone 6 fixture"),
                ),
                resolved_imports: vec![],
            }],
        };
        let error = compile_package_with_manifest(&package, &manifest)
            .expect_err("invalid message fixture fails");
        assert_eq!(error.diagnostics()[0].code(), *code, "{name}");
    }
}

#[test]
fn milestone7_diverse_session_is_deterministic_transactional_and_gap_safe() {
    let grammar = compile_package(&single_file_package("packages/milestone7/root.meco"))
        .expect("Milestone 7 package compiles");
    let mut session = SamplerSession::new(0);
    let mut store = RepetitionStore::new_location();
    assert_milestone7_sequence(&grammar, &mut session, &mut store);
    assert_milestone7_failures_roll_back(&grammar, &mut session, &mut store);
    assert_milestone7_exempt_rules_generate(&grammar);
}

fn assert_milestone7_sequence(
    grammar: &CompiledGrammar,
    session: &mut SamplerSession,
    store: &mut RepetitionStore,
) {
    let mut outputs = Vec::new();
    let mut previous = None;
    for call in 0..16_u32 {
        let result = session
            .generate(
                grammar,
                store,
                &DiverseGenerationRequest {
                    trace_selections: true,
                    ..DiverseGenerationRequest::default()
                },
            )
            .expect("diverse call succeeds");
        let text = result.generation().text();
        assert_ne!(
            previous.as_deref(),
            Some(text),
            "hard gap failed at call {call}"
        );
        previous = Some(text.to_string());
        assert_eq!(result.attempts(), 12);
        assert_eq!(result.committed_revision(), u64::from(call + 1));
        assert_eq!(session.random_words(), u64::from(call + 1) * 12);
        outputs.push(format!(
            "{call}|{text}|{}|{}|{}",
            result.winner_attempt(),
            result.exact_repetitions(),
            result.edge_repetitions()
        ));
    }
    let actual = outputs.join("\n");
    let expected = fs::read_to_string(fixture_path("expected/milestone7-sequence.outputs"))
        .expect("read Milestone 7 sequence");
    assert_eq!(actual, expected.trim_end());
}

fn assert_milestone7_failures_roll_back(
    grammar: &CompiledGrammar,
    session: &mut SamplerSession,
    store: &mut RepetitionStore,
) {
    let before_words = session.random_words();
    let before_revision = store.revision();
    let failure = session
        .generate(
            grammar,
            store,
            &DiverseGenerationRequest {
                entry: Some("not.public"),
                ..DiverseGenerationRequest::default()
            },
        )
        .expect_err("invalid request fails");
    assert_eq!(failure.diagnostics()[0].code(), DiagnosticCode::NO_ENTRY);
    assert_eq!(session.random_words(), before_words);
    assert_eq!(store.revision(), before_revision);
    let over_budget = session
        .generate(
            grammar,
            store,
            &DiverseGenerationRequest {
                limits: GenerationLimits {
                    max_output_scalars: 1,
                    ..GenerationLimits::default()
                },
                ..DiverseGenerationRequest::default()
            },
        )
        .expect_err("all over-budget candidates fail");
    assert_eq!(
        over_budget.diagnostics()[0].code(),
        DiagnosticCode::LIMIT_OUTPUT
    );
    assert_eq!(session.random_words(), before_words);
    assert_eq!(store.revision(), before_revision);
    let cancelled = session
        .generate(
            grammar,
            store,
            &DiverseGenerationRequest {
                cancelled: true,
                ..DiverseGenerationRequest::default()
            },
        )
        .expect_err("cancelled request fails");
    assert_eq!(cancelled.diagnostics()[0].code(), DiagnosticCode::CANCELLED);
    assert_eq!(session.random_words(), before_words);
    assert_eq!(store.revision(), before_revision);
}

fn assert_milestone7_exempt_rules_generate(grammar: &CompiledGrammar) {
    for entry in ["diverse.nullable", "diverse.recursive"] {
        let mut exempt_session = SamplerSession::new(7);
        let mut exempt_store = RepetitionStore::new_location();
        for _ in 0..4 {
            exempt_session
                .generate(
                    grammar,
                    &mut exempt_store,
                    &DiverseGenerationRequest {
                        entry: Some(entry),
                        ..DiverseGenerationRequest::default()
                    },
                )
                .expect("nullable/recursive exemption remains generatable");
        }
    }
}

#[test]
fn milestone8_provenance_audits_and_nonempty_replay_round_trip_from_filesystem() {
    let package = single_file_package("packages/milestone8/root.meco");
    let manifest = MessageManifest {
        messages: vec![MessageDefinition {
            id: "arrival".to_string(),
            arguments: vec![MessageArgument {
                name: "name".to_string(),
                type_: SchemaType::Text,
            }],
        }],
    };
    let grammar =
        compile_package_with_manifest(&package, &manifest).expect("Milestone 8 package compiles");
    let expected = fs::read_to_string(fixture_path("expected/milestone8-audit.findings"))
        .expect("read Milestone 8 audit findings");
    let actual = grammar
        .audit_composition()
        .into_iter()
        .map(|finding| {
            format!(
                "{}|{}|{}|{}|{}|{}",
                finding.rule,
                finding.production_id,
                finding.direct_references,
                finding.longest_literal_run,
                finding.insufficient_references,
                finding.excessive_literal_run
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(actual, expected.trim());

    let data = [DataBinding::new(
        "playerName".to_string(),
        Value::Text("Rin".to_string()),
    )];
    let request = DiverseGenerationRequest {
        data: &data,
        trace_selections: true,
        trace_provenance: true,
        ..DiverseGenerationRequest::default()
    };
    let mut session = SamplerSession::new(19);
    let mut store = RepetitionStore::new_location();
    let first = session
        .generate(&grammar, &mut store, &request)
        .expect("first traced generation");
    let session_snapshot = session.snapshot();
    let repetition_snapshot = store.snapshot().expect("nonempty history snapshot");
    let second = session
        .generate(&grammar, &mut store, &request)
        .expect("second traced generation");

    let corpus = vec![first.generation().clone(), second.generation().clone()];
    let repeated = audit_rendered_repetition(&corpus, 3);
    let opening = repeated
        .iter()
        .find(|finding| finding.fragment == "fixed opening words")
        .expect("repeated opening is audited");
    assert_eq!(opening.occurrences, 2);
    assert!(
        opening
            .attributions
            .iter()
            .all(|attribution| attribution.rule != "audit.suffix")
    );
    assert!(audit_structural_repetition(&corpus).iter().any(|finding| {
        finding.rule == "audit.opening" && finding.production_id == "fixed-opening"
    }));

    let mut restored_session =
        SamplerSession::restore(session_snapshot).expect("session snapshot restores");
    let mut restored_store =
        RepetitionStore::restore(&repetition_snapshot).expect("history snapshot restores");
    let replayed = restored_session
        .generate(&grammar, &mut restored_store, &request)
        .expect("restored next call succeeds");
    assert_eq!(replayed, second);
    assert_eq!(restored_session, session);
    assert_eq!(restored_store, store);
}
