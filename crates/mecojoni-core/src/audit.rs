use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};

use crate::{
    BodyPartSyntax, BodySyntax, COMPOSITION_PROFILE_VERSION, CompositionProfile, GenerationResult,
    ModuleSyntax, OutputRange, ProvenanceKind, Span,
};

/// Stable tokenizer used by `composition/1` and initial structural fragments.
pub const WORD_TOKENIZER_VERSION: &str = "scalar-word/1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositionFinding {
    pub rule: String,
    pub production_index: u32,
    pub production_id: String,
    pub span: Span,
    pub direct_references: u32,
    pub longest_literal_run: u32,
    pub insufficient_references: bool,
    pub excessive_literal_run: bool,
}

/// One stable selection repeated across a traced generation corpus.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructuralRepetitionFinding {
    pub rule: String,
    pub production_id: String,
    pub selections: u32,
}

/// Attribution role proven from output-range overlap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttributionRole {
    DirectEmitter,
    ComposingAncestor,
}

/// One overlapping trace node responsible for a repeated rendered range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepetitionAttribution {
    pub result_index: u32,
    pub node_id: u32,
    pub role: AttributionRole,
    pub rule: String,
    pub production_id: String,
    pub source_span: Span,
    pub output: OutputRange,
}

/// One normalized repeated visible fragment and only its overlapping emitters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedRepetitionFinding {
    pub fragment: String,
    pub occurrences: u32,
    pub attributions: Vec<RepetitionAttribution>,
}

/// Counts repeated stable structural selections in already traced results.
#[must_use]
pub fn audit_structural_repetition(
    results: &[GenerationResult],
) -> Vec<StructuralRepetitionFinding> {
    let mut counts = BTreeMap::<(String, String), u32>::new();
    for selection in results.iter().flat_map(GenerationResult::selections) {
        let key = (
            selection.rule().to_string(),
            selection.selected_production_id().to_string(),
        );
        let count = counts.entry(key).or_default();
        *count = count.saturating_add(1);
    }
    counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(
            |((rule, production_id), selections)| StructuralRepetitionFinding {
                rule,
                production_id,
                selections,
            },
        )
        .collect()
}

/// Finds repeated normalized word fragments and attributes each occurrence only
/// to provenance nodes with overlapping scalar output ranges.
#[must_use]
pub fn audit_rendered_repetition(
    results: &[GenerationResult],
    fragment_words: u32,
) -> Vec<RenderedRepetitionFinding> {
    let length = usize::try_from(fragment_words.max(1)).unwrap_or(usize::MAX);
    let mut occurrences = BTreeMap::<String, Vec<(usize, OutputRange)>>::new();
    for (result_index, result) in results.iter().enumerate() {
        let words = word_ranges(result.text());
        for window in words.windows(length) {
            let fragment = window
                .iter()
                .map(|word| word.normalized.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let Some((first, tail)) = window.split_first() else {
                continue;
            };
            let last = tail.last().unwrap_or(first);
            occurrences.entry(fragment).or_default().push((
                result_index,
                OutputRange::new(
                    first.start_byte,
                    last.end_byte,
                    first.start_scalar,
                    last.end_scalar,
                ),
            ));
        }
    }
    occurrences
        .into_iter()
        .filter(|(_, ranges)| ranges.len() > 1)
        .map(|(fragment, ranges)| {
            let mut attributions = Vec::new();
            for (result_index, repeated) in &ranges {
                for node in results[*result_index].provenance() {
                    let Some(output) = node.output() else {
                        continue;
                    };
                    if !output.overlaps(*repeated) {
                        continue;
                    }
                    let role = match node.kind() {
                        ProvenanceKind::Production => AttributionRole::ComposingAncestor,
                        ProvenanceKind::AuthoredText
                        | ProvenanceKind::HostValue
                        | ProvenanceKind::BoundValue
                        | ProvenanceKind::EmittingCapture
                        | ProvenanceKind::Message => AttributionRole::DirectEmitter,
                        ProvenanceKind::Binding => continue,
                    };
                    attributions.push(RepetitionAttribution {
                        result_index: u32::try_from(*result_index).unwrap_or(u32::MAX),
                        node_id: node.id(),
                        role,
                        rule: node.rule().to_string(),
                        production_id: node.production_id().to_string(),
                        source_span: node.source_span(),
                        output,
                    });
                }
            }
            RenderedRepetitionFinding {
                fragment,
                occurrences: u32::try_from(ranges.len()).unwrap_or(u32::MAX),
                attributions,
            }
        })
        .collect()
}

struct WordRange {
    normalized: String,
    start_byte: u64,
    end_byte: u64,
    start_scalar: u64,
    end_scalar: u64,
}

fn word_ranges(text: &str) -> Vec<WordRange> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut start_byte = 0_usize;
    let mut start_scalar = 0_usize;
    let mut scalar = 0_usize;
    for (byte, character) in text
        .char_indices()
        .chain(core::iter::once((text.len(), '\0')))
    {
        if character != '\0' && is_word_scalar(character) {
            if current.is_empty() {
                start_byte = byte;
                start_scalar = scalar;
            }
            current.push(if character.is_ascii_uppercase() {
                character.to_ascii_lowercase()
            } else {
                character
            });
        } else if !current.is_empty() {
            words.push(WordRange {
                normalized: core::mem::take(&mut current),
                start_byte: u64::try_from(start_byte).unwrap_or(u64::MAX),
                end_byte: u64::try_from(byte).unwrap_or(u64::MAX),
                start_scalar: u64::try_from(start_scalar).unwrap_or(u64::MAX),
                end_scalar: u64::try_from(scalar).unwrap_or(u64::MAX),
            });
        }
        if character != '\0' {
            scalar = scalar.saturating_add(1);
        }
    }
    words
}

/// Runs the versioned composition heuristic over every locally composed,
/// sentence-ending production in a parsed module.
#[must_use]
pub fn audit_composition(module: &ModuleSyntax) -> Vec<CompositionFinding> {
    let profile = CompositionProfile::V1;
    let mut findings = Vec::new();
    for rule in &module.rules {
        for (index, production) in rule.productions.iter().enumerate() {
            let BodySyntax::Parts(parts) = &production.body else {
                continue;
            };
            if profile.complete_messages_are_exempt
                && matches!(parts.as_slice(), [BodyPartSyntax::MessageCall(_)])
            {
                continue;
            }
            if !ends_sentence(parts) {
                continue;
            }

            let direct_references = u32::try_from(
                parts
                    .iter()
                    .filter(|part| {
                        matches!(
                            part,
                            BodyPartSyntax::RuleReference(_)
                                | BodyPartSyntax::RuleCall(_)
                                | BodyPartSyntax::EmittingCapture { .. }
                        )
                    })
                    .count(),
            )
            .unwrap_or(u32::MAX);
            let longest_literal_run = longest_literal_run(parts);
            let insufficient_references = direct_references < profile.minimum_direct_references;
            let excessive_literal_run = longest_literal_run > profile.maximum_literal_run_words;
            if insufficient_references || excessive_literal_run {
                findings.push(CompositionFinding {
                    rule: rule.name.value().clone(),
                    production_index: u32::try_from(index).unwrap_or(u32::MAX),
                    production_id: crate::compiler::stable_production_id(
                        &alloc::format!(
                            "{}.{}",
                            module.front_matter.module().value(),
                            rule.name.value()
                        ),
                        production,
                    ),
                    span: production.span,
                    direct_references,
                    longest_literal_run,
                    insufficient_references,
                    excessive_literal_run,
                });
            }
        }
    }
    findings
}

#[must_use]
pub const fn composition_profile_version() -> &'static str {
    COMPOSITION_PROFILE_VERSION
}

fn ends_sentence(parts: &[BodyPartSyntax]) -> bool {
    parts
        .iter()
        .rev()
        .find_map(|part| match part {
            BodyPartSyntax::Literal(literal) => literal.value().chars().next_back(),
            _ => None,
        })
        .is_some_and(|character| matches!(character, '.' | '!' | '?'))
}

fn longest_literal_run(parts: &[BodyPartSyntax]) -> u32 {
    let mut longest = 0;
    let mut current = 0_u32;
    for part in parts {
        if let BodyPartSyntax::Literal(literal) = part {
            current = current.saturating_add(count_words(literal.value()));
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
}

pub(crate) fn count_words(text: &str) -> u32 {
    let mut count = 0_u32;
    let mut in_word = false;
    for character in text.chars() {
        let word = is_word_scalar(character);
        if word && !in_word {
            count = count.saturating_add(1);
        }
        in_word = word;
    }
    count
}

pub(crate) fn is_word_scalar(character: char) -> bool {
    if character.is_ascii() {
        return character.is_ascii_alphanumeric() || character == '_';
    }
    !matches!(
        character as u32,
        0x0085
            | 0x00a0
            | 0x1680
            | 0x3000
            | 0x2000..=0x206f
            | 0x2e00..=0x2e7f
            | 0x3001..=0x303f
            | 0xfe10..=0xfe1f
            | 0xfe30..=0xfe4f
            | 0xff01..=0xff0f
            | 0xff1a..=0xff20
            | 0xff3b..=0xff40
            | 0xff5b..=0xff65
    )
}

#[cfg(test)]
mod tests {
    use alloc::{string::ToString, vec, vec::Vec};

    use super::{
        WORD_TOKENIZER_VERSION, audit_composition, audit_rendered_repetition,
        audit_structural_repetition, count_words,
    };
    use crate::{
        GenerationRequest, PackageInput, PackageSource, SourceFile, SourceId, compile_package,
        parse_module,
    };

    #[test]
    fn tokenizer_is_scalar_stable_and_dependency_free() {
        assert_eq!(WORD_TOKENIZER_VERSION, "scalar-word/1");
        assert_eq!(count_words("one two-three"), 3);
        assert_eq!(count_words("你好，世界"), 2);
        assert_eq!(count_words("pilot_2"), 1);
    }

    #[test]
    fn messages_are_exempt_but_literal_shells_are_reported() {
        let source = SourceFile::new(
            SourceId::new(0),
            "audit.meco.md",
            concat!(
                "---\nmeco: 2\nmodule: audit\n---\n",
                "# shell\n- The old pilot waited quietly.\n",
                "# message\n- &localized <- name: $name\n",
            ),
        );
        let module = parse_module(&source).expect("audit fixture parses");
        let findings = audit_composition(&module);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, "shell");
        assert!(findings[0].insufficient_references);
        assert!(findings[0].excessive_literal_run);
    }

    #[test]
    fn rendered_audit_attributes_only_nodes_overlapping_the_repeated_fragment() {
        let source = SourceFile::new(
            SourceId::new(0),
            "repetition.meco.md",
            concat!(
                "---\nmeco: 2\nmodule: root\nentry: line\nexports: [line]\n---\n",
                "# line\n- [weight = 1, id = shell] @opening @suffix\n",
                "# opening\n- [weight = 1, id = opening] Fixed opening words\n",
                "# suffix\n- [weight = 1, id = alpha] alpha\n",
                "- [weight = 1, id = beta] beta\n",
            ),
        );
        let grammar = compile_package(&PackageInput {
            root_id: "root".to_string(),
            modules: vec![PackageSource {
                canonical_id: "root".to_string(),
                source,
                resolved_imports: vec![],
            }],
        })
        .expect("repetition fixture compiles");
        let mut results = Vec::new();
        for seed in 0..64 {
            let result = grammar
                .generate_weighted(&GenerationRequest {
                    trace_selections: true,
                    trace_provenance: true,
                    ..GenerationRequest::with_seed(seed)
                })
                .expect("traced corpus item");
            if results
                .iter()
                .all(|previous: &crate::GenerationResult| previous.text() != result.text())
            {
                results.push(result);
            }
            if results.len() == 2 {
                break;
            }
        }
        assert_eq!(results.len(), 2);

        let rendered = audit_rendered_repetition(&results, 3);
        let opening = rendered
            .iter()
            .find(|finding| finding.fragment == "fixed opening words")
            .expect("opening repetition found");
        assert_eq!(opening.occurrences, 2);
        assert!(!opening.attributions.is_empty());
        assert!(
            opening
                .attributions
                .iter()
                .all(|attribution| attribution.rule != "root.suffix")
        );
        let structural = audit_structural_repetition(&results);
        assert!(structural.iter().any(|finding| {
            finding.rule == "root.opening"
                && finding.production_id == "opening"
                && finding.selections == 2
        }));
    }
}
