use alloc::{string::String, vec::Vec};

use crate::{
    BodyPartSyntax, BodySyntax, COMPOSITION_PROFILE_VERSION, CompositionProfile, ModuleSyntax, Span,
};

/// Stable tokenizer used by `composition/1` and initial structural fragments.
pub const WORD_TOKENIZER_VERSION: &str = "scalar-word/1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositionFinding {
    pub rule: String,
    pub production_index: u32,
    pub span: Span,
    pub direct_references: u32,
    pub longest_literal_run: u32,
    pub insufficient_references: bool,
    pub excessive_literal_run: bool,
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

fn count_words(text: &str) -> u32 {
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

fn is_word_scalar(character: char) -> bool {
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
    use super::{WORD_TOKENIZER_VERSION, audit_composition, count_words};
    use crate::{SourceFile, SourceId, parse_module};

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
}
