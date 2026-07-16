use std::{fs, path::PathBuf};

use mecojoni_core::{
    ArgumentSyntax, BlockChomp, BodyPartSyntax, BodySyntax, ClauseSyntax, CompositionProfile,
    LocationProfile, PackageInput, PackageSource, Rational, ResolvedImport, ResourceProfile,
    SourceFile, SourceId, SplitMix64, ValueSyntax, WeightSyntax, audit_composition,
    diversity_factor_16_16, location_cooldown_multiplier, parse_front_matter, parse_module,
    validate_package_input,
};
use std::str::FromStr;

fn fixture_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative)
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
