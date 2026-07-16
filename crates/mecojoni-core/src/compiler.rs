use alloc::{
    boxed::Box,
    collections::{BTreeMap, VecDeque},
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use crate::diverse::DiverseCandidateState;
use crate::{
    ArgumentSyntax, BindingTrace, BodyPartSyntax, BodySyntax, ClauseSyntax, CompositionFinding,
    DataBinding, Diagnostic, DiagnosticCode, EligibleWeightTrace, Formatter, FormatterRequest,
    GuardExpression, GuardValue, InputDefinition, LocaleRequest, MecoError, MecoResult,
    MessageDefinition, MessageManifest, MessageTrace, ModuleSyntax, OutputRange, PackageInput,
    PackageManifest, ProductionSyntax, ProvenanceKind, ProvenanceNode, Rational, SchemaType,
    SelectionTrace, Severity, SourceFile, SourceId, Span, SplitMix64, Value, ValueSyntax,
    WeightExpression, WeightSyntax, diversity_factor_16_16, location_cooldown_multiplier,
    parse_module, validate_package_input,
};

/// Compatibility identifier for independent exact weighted selection.
pub const WEIGHTED_SAMPLER_VERSION: &str = "weighted/1";

/// Default deterministic work limits for one `weighted/1` request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GenerationLimits {
    pub max_depth: u32,
    pub max_expansions: u32,
    pub max_output_scalars: u32,
    pub max_output_bytes: u32,
    pub max_sampler_words: u32,
}

impl GenerationLimits {
    pub const INTERACTIVE_WEIGHTED_V1: Self = Self {
        max_depth: 80,
        max_expansions: 2_000,
        max_output_scalars: 16_384,
        max_output_bytes: 65_536,
        max_sampler_words: 8_192,
    };
}

impl Default for GenerationLimits {
    fn default() -> Self {
        Self::INTERACTIVE_WEIGHTED_V1
    }
}

/// One stateless deterministic generation request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GenerationRequest<'a> {
    /// Qualified public entry, or `None` to use the root module's default entry.
    pub entry: Option<&'a str>,
    pub seed: u64,
    pub limits: GenerationLimits,
    /// Immutable host values checked against the compiled input schema.
    pub data: &'a [DataBinding],
    /// Retain ordered candidate-local binding/capture values in the result.
    pub trace_bindings: bool,
    /// Retain exact eligible and normalized weights for every selection.
    pub trace_selections: bool,
    /// Retain source-to-output derivation nodes and exact output ranges.
    pub trace_provenance: bool,
}

impl GenerationRequest<'_> {
    #[must_use]
    pub const fn with_seed(seed: u64) -> Self {
        Self {
            entry: None,
            seed,
            limits: GenerationLimits::INTERACTIVE_WEIGHTED_V1,
            data: &[],
            trace_bindings: false,
            trace_selections: false,
            trace_provenance: false,
        }
    }
}

/// Returned text and deterministic work counters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenerationResult {
    text: String,
    entry: String,
    expansions: u32,
    sampler_words: u32,
    bindings: Vec<BindingTrace>,
    selections: Vec<SelectionTrace>,
    formatter_diagnostics: Vec<Diagnostic>,
    message: Option<MessageTrace>,
    provenance: Vec<ProvenanceNode>,
}

impl GenerationResult {
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub fn entry(&self) -> &str {
        &self.entry
    }

    #[must_use]
    pub const fn expansions(&self) -> u32 {
        self.expansions
    }

    #[must_use]
    pub const fn sampler_words(&self) -> u32 {
        self.sampler_words
    }

    #[must_use]
    pub fn bindings(&self) -> &[BindingTrace] {
        &self.bindings
    }

    #[must_use]
    pub fn selections(&self) -> &[SelectionTrace] {
        &self.selections
    }

    #[must_use]
    pub fn formatter_diagnostics(&self) -> &[Diagnostic] {
        &self.formatter_diagnostics
    }

    #[must_use]
    pub const fn message(&self) -> Option<&MessageTrace> {
        self.message.as_ref()
    }

    #[must_use]
    pub fn provenance(&self) -> &[ProvenanceNode] {
        &self.provenance
    }
}

/// Unformatted deterministic output produced before crossing the host boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GeneratedContent {
    Text(String),
    Message(FormatterRequest),
}

/// Structural generation result used by foreign wrappers and custom adapters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructuralGenerationResult {
    content: GeneratedContent,
    entry: String,
    expansions: u32,
    sampler_words: u32,
    bindings: Vec<BindingTrace>,
    selections: Vec<SelectionTrace>,
    provenance: Vec<ProvenanceNode>,
}

impl StructuralGenerationResult {
    #[must_use]
    pub const fn content(&self) -> &GeneratedContent {
        &self.content
    }

    #[must_use]
    pub fn entry(&self) -> &str {
        &self.entry
    }

    #[must_use]
    pub const fn expansions(&self) -> u32 {
        self.expansions
    }

    #[must_use]
    pub const fn sampler_words(&self) -> u32 {
        self.sampler_words
    }

    #[must_use]
    pub fn bindings(&self) -> &[BindingTrace] {
        &self.bindings
    }

    #[must_use]
    pub fn selections(&self) -> &[SelectionTrace] {
        &self.selections
    }

    #[must_use]
    pub fn provenance(&self) -> &[ProvenanceNode] {
        &self.provenance
    }
}

/// Read-only graph facts retained for tooling without exposing mutable IR.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct RuleAnalysis {
    pub reachable: bool,
    pub productive: bool,
    pub nullable: bool,
    pub recursive: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CompiledPart {
    Literal {
        text: String,
        span: Span,
    },
    RuleCall {
        rule: usize,
        arguments: Vec<CompiledValue>,
        span: Span,
    },
    Value {
        value: CompiledValue,
        span: Span,
    },
    Capture {
        rule: usize,
        slot: usize,
        name: String,
        span: Span,
    },
    MessageCall {
        id: String,
        arguments: Vec<(String, CompiledValue)>,
        span: Span,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CompiledValue {
    Input(usize),
    Local(usize),
    Constant(Value),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CompiledWeight {
    Static(Rational),
    Dynamic(CompiledWeightExpression),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CompiledWeightExpression {
    Literal(Rational),
    Value(CompiledValue),
    Add(Box<Self>, Box<Self>),
    Subtract(Box<Self>, Box<Self>),
    Multiply(Box<Self>, Box<Self>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CompiledGuardValue {
    Value(CompiledValue),
    Constant(Value),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CompiledGuard {
    Value(CompiledGuardValue),
    Is(CompiledGuardValue, CompiledGuardValue),
    IsNot(CompiledGuardValue, CompiledGuardValue),
    Less(CompiledGuardValue, CompiledGuardValue),
    LessOrEqual(CompiledGuardValue, CompiledGuardValue),
    Greater(CompiledGuardValue, CompiledGuardValue),
    GreaterOrEqual(CompiledGuardValue, CompiledGuardValue),
    Not(Box<Self>),
    And(Box<Self>, Box<Self>),
    Or(Box<Self>, Box<Self>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CompiledBinding {
    pub(crate) rule: usize,
    pub(crate) arguments: Vec<CompiledValue>,
    pub(crate) slot: usize,
    pub(crate) name: String,
    pub(crate) span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CompiledProduction {
    pub(crate) id: String,
    pub(crate) authored_id: bool,
    pub(crate) span: Span,
    pub(crate) weight: CompiledWeight,
    pub(crate) guard: Option<CompiledGuard>,
    pub(crate) bindings: Vec<CompiledBinding>,
    pub(crate) parts: Vec<CompiledPart>,
    pub(crate) diversity_factor_16_16: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ValueType {
    Text,
    Number,
    Boolean,
    Enum { name: String, variants: Vec<String> },
}

impl ValueType {
    fn display_name(&self) -> &str {
        match self {
            Self::Text => "text",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Enum { name, .. } => name,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CompiledInput {
    pub(crate) external_name: String,
    pub(crate) type_: ValueType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CompiledRule {
    pub(crate) name: String,
    pub(crate) parameters: Vec<(String, ValueType)>,
    pub(crate) span: Span,
    pub(crate) productions: Vec<CompiledProduction>,
    pub(crate) static_selection: Option<StaticSelection>,
    pub(crate) analysis: RuleAnalysis,
    pub(crate) message_effect: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StaticSelection {
    pub(crate) cumulative: Vec<u64>,
    pub(crate) total: u64,
}

impl StaticSelection {
    fn select(&self, choice: u64) -> usize {
        debug_assert!(choice < self.total);
        self.cumulative.partition_point(|upper| *upper <= choice)
    }
}

fn validate_value_operand(
    value: &CompiledValue,
    input_count: usize,
    local_count: usize,
) -> MecoResult<()> {
    let valid = match value {
        CompiledValue::Input(index) => *index < input_count,
        CompiledValue::Local(index) => *index < local_count,
        CompiledValue::Constant(_) => true,
    };
    if !valid {
        return Err(runtime_error(
            DiagnosticCode::BYTECODE_CORRUPT,
            "lowered value operand references a missing slot",
        ));
    }
    Ok(())
}

fn validate_weight_operands(
    weight: &CompiledWeight,
    input_count: usize,
    local_count: usize,
) -> MecoResult<()> {
    fn expression(
        value: &CompiledWeightExpression,
        input_count: usize,
        local_count: usize,
    ) -> MecoResult<()> {
        match value {
            CompiledWeightExpression::Literal(_) => Ok(()),
            CompiledWeightExpression::Value(value) => {
                validate_value_operand(value, input_count, local_count)
            }
            CompiledWeightExpression::Add(left, right)
            | CompiledWeightExpression::Subtract(left, right)
            | CompiledWeightExpression::Multiply(left, right) => {
                expression(left, input_count, local_count)?;
                expression(right, input_count, local_count)
            }
        }
    }
    match weight {
        CompiledWeight::Static(_) => Ok(()),
        CompiledWeight::Dynamic(value) => expression(value, input_count, local_count),
    }
}

fn validate_guard_operands(
    guard: &CompiledGuard,
    input_count: usize,
    local_count: usize,
) -> MecoResult<()> {
    fn value(value: &CompiledGuardValue, input_count: usize, local_count: usize) -> MecoResult<()> {
        match value {
            CompiledGuardValue::Value(value) => {
                validate_value_operand(value, input_count, local_count)
            }
            CompiledGuardValue::Constant(_) => Ok(()),
        }
    }
    match guard {
        CompiledGuard::Value(operand) => value(operand, input_count, local_count),
        CompiledGuard::Is(left, right)
        | CompiledGuard::IsNot(left, right)
        | CompiledGuard::Less(left, right)
        | CompiledGuard::LessOrEqual(left, right)
        | CompiledGuard::Greater(left, right)
        | CompiledGuard::GreaterOrEqual(left, right) => {
            value(left, input_count, local_count)?;
            value(right, input_count, local_count)
        }
        CompiledGuard::Not(value) => validate_guard_operands(value, input_count, local_count),
        CompiledGuard::And(left, right) | CompiledGuard::Or(left, right) => {
            validate_guard_operands(left, input_count, local_count)?;
            validate_guard_operands(right, input_count, local_count)
        }
    }
}

/// Immutable, indexed package artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledGrammar {
    pub(crate) artifact_hash: u64,
    pub(crate) rules: Vec<CompiledRule>,
    pub(crate) inputs: Vec<CompiledInput>,
    pub(crate) entries: Vec<(String, usize)>,
    pub(crate) default_entry: Option<usize>,
    pub(crate) warnings: Vec<Diagnostic>,
    pub(crate) message_manifest: MessageManifest,
}

impl CompiledGrammar {
    /// Verifies the immutable lowered representation shared by all loaders.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn validate_lowered_invariants(&self) -> MecoResult<()> {
        let rule_count = self.rules.len();
        if self.entries.iter().any(|(_, rule)| *rule >= rule_count)
            || self.default_entry.is_some_and(|rule| rule >= rule_count)
        {
            return Err(runtime_error(
                DiagnosticCode::BYTECODE_CORRUPT,
                "lowered entry references a missing rule",
            ));
        }
        if self.rules.iter().enumerate().any(|(index, rule)| {
            self.rules[..index]
                .iter()
                .any(|other| other.name == rule.name)
        }) || self
            .entries
            .iter()
            .enumerate()
            .any(|(index, (name, _))| self.entries[..index].iter().any(|(other, _)| other == name))
        {
            return Err(runtime_error(
                DiagnosticCode::BYTECODE_CORRUPT,
                "lowered rule or entry names are not unique",
            ));
        }
        if self
            .default_entry
            .is_some_and(|default| !self.entries.iter().any(|(_, rule)| *rule == default))
        {
            return Err(runtime_error(
                DiagnosticCode::BYTECODE_CORRUPT,
                "lowered default entry is not public",
            ));
        }
        for rule in &self.rules {
            if let Some(selection) = &rule.static_selection {
                if selection.cumulative.len() != rule.productions.len()
                    || selection.cumulative.last().copied() != Some(selection.total)
                    || selection.total == 0
                    || !selection
                        .cumulative
                        .windows(2)
                        .all(|pair| pair[0] < pair[1])
                {
                    return Err(runtime_error(
                        DiagnosticCode::BYTECODE_CORRUPT,
                        "lowered static-selection index is inconsistent",
                    ));
                }
            }
            for production in &rule.productions {
                if production
                    .bindings
                    .iter()
                    .any(|binding| binding.rule >= rule_count)
                    || production.parts.iter().any(|part| match part {
                        CompiledPart::RuleCall { rule, .. }
                        | CompiledPart::Capture { rule, .. } => *rule >= rule_count,
                        CompiledPart::Literal { .. }
                        | CompiledPart::Value { .. }
                        | CompiledPart::MessageCall { .. } => false,
                    })
                {
                    return Err(runtime_error(
                        DiagnosticCode::BYTECODE_CORRUPT,
                        "lowered production references a missing rule",
                    ));
                }
                let parameter_slots = rule.parameters.len();
                validate_weight_operands(&production.weight, self.inputs.len(), parameter_slots)?;
                if let Some(guard) = &production.guard {
                    validate_guard_operands(guard, self.inputs.len(), parameter_slots)?;
                }
                let mut local_slots = parameter_slots;
                for binding in &production.bindings {
                    if binding.slot != local_slots
                        || binding.arguments.len() != self.rules[binding.rule].parameters.len()
                    {
                        return Err(runtime_error(
                            DiagnosticCode::BYTECODE_CORRUPT,
                            "lowered binding slots or call arity are inconsistent",
                        ));
                    }
                    for value in &binding.arguments {
                        validate_value_operand(value, self.inputs.len(), local_slots)?;
                    }
                    local_slots += 1;
                }
                for part in &production.parts {
                    match part {
                        CompiledPart::Literal { .. } => {}
                        CompiledPart::RuleCall {
                            rule, arguments, ..
                        } => {
                            if arguments.len() != self.rules[*rule].parameters.len() {
                                return Err(runtime_error(
                                    DiagnosticCode::BYTECODE_CORRUPT,
                                    "lowered rule-call arity is inconsistent",
                                ));
                            }
                            for value in arguments {
                                validate_value_operand(value, self.inputs.len(), local_slots)?;
                            }
                        }
                        CompiledPart::Value { value, .. } => {
                            validate_value_operand(value, self.inputs.len(), local_slots)?;
                        }
                        CompiledPart::Capture { rule, slot, .. } => {
                            if *slot != local_slots || !self.rules[*rule].parameters.is_empty() {
                                return Err(runtime_error(
                                    DiagnosticCode::BYTECODE_CORRUPT,
                                    "lowered capture slot or arity is inconsistent",
                                ));
                            }
                            local_slots += 1;
                        }
                        CompiledPart::MessageCall { id, arguments, .. } => {
                            let Some(schema) = self
                                .message_manifest
                                .messages
                                .iter()
                                .find(|message| message.id == *id)
                            else {
                                return Err(runtime_error(
                                    DiagnosticCode::BYTECODE_CORRUPT,
                                    "lowered message call has no manifest schema",
                                ));
                            };
                            if arguments.len() != schema.arguments.len()
                                || arguments
                                    .iter()
                                    .zip(&schema.arguments)
                                    .any(|((name, _), schema)| name != &schema.name)
                            {
                                return Err(runtime_error(
                                    DiagnosticCode::BYTECODE_CORRUPT,
                                    "lowered message arguments do not match the manifest",
                                ));
                            }
                            for (_, value) in arguments {
                                validate_value_operand(value, self.inputs.len(), local_slots)?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Stable content hash of canonical package sources, resolutions, and manifest.
    #[must_use]
    pub const fn artifact_hash(&self) -> u64 {
        self.artifact_hash
    }

    #[must_use]
    pub fn entries(&self) -> impl ExactSizeIterator<Item = &str> {
        self.entries.iter().map(|(name, _)| name.as_str())
    }

    #[must_use]
    pub fn default_entry(&self) -> Option<&str> {
        self.default_entry
            .map(|rule| self.rules[rule].name.as_str())
    }

    #[must_use]
    pub fn warnings(&self) -> &[Diagnostic] {
        &self.warnings
    }

    /// Returns a deeply owned schema suitable for serialization by the host.
    #[must_use]
    pub fn manifest(&self) -> PackageManifest {
        PackageManifest {
            inputs: self
                .inputs
                .iter()
                .map(|input| InputDefinition {
                    name: input.external_name.clone(),
                    type_: schema_type_from_value_type(&input.type_),
                })
                .collect(),
            messages: self.message_manifest.clone(),
        }
    }

    #[must_use]
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Total number of immutable compiled alternatives in the package.
    #[must_use]
    pub fn production_count(&self) -> usize {
        self.rules.iter().map(|rule| rule.productions.len()).sum()
    }

    #[must_use]
    pub fn rule_analysis(&self, name: &str) -> Option<RuleAnalysis> {
        self.rules
            .iter()
            .find(|rule| rule.name == name)
            .map(|rule| rule.analysis)
    }

    /// Returns the stable authored or content-addressed ID for one production.
    #[must_use]
    pub fn production_id(&self, rule: &str, production: usize) -> Option<&str> {
        self.rules
            .iter()
            .find(|candidate| candidate.name == rule)
            .and_then(|candidate| candidate.productions.get(production))
            .map(|candidate| candidate.id.as_str())
    }

    /// Reports whether a production ID was explicitly authored.
    #[must_use]
    pub fn production_id_is_authored(&self, rule: &str, production: usize) -> Option<bool> {
        self.rules
            .iter()
            .find(|candidate| candidate.name == rule)
            .and_then(|candidate| candidate.productions.get(production))
            .map(|candidate| candidate.authored_id)
    }

    /// Runs `composition/1` over reachable locally composed productions using
    /// compiled stable identities and message-effect facts.
    #[must_use]
    pub fn audit_composition(&self) -> Vec<CompositionFinding> {
        let profile = crate::CompositionProfile::V1;
        let mut findings = Vec::new();
        for rule in self.rules.iter().filter(|rule| rule.analysis.reachable) {
            for (index, production) in rule.productions.iter().enumerate() {
                if profile.complete_messages_are_exempt
                    && matches!(
                        production.parts.as_slice(),
                        [CompiledPart::MessageCall { .. }]
                    )
                {
                    continue;
                }
                let sentence_ending = production
                    .parts
                    .iter()
                    .rev()
                    .find_map(|part| match part {
                        CompiledPart::Literal { text, .. } => text.chars().next_back(),
                        _ => None,
                    })
                    .is_some_and(|character| matches!(character, '.' | '!' | '?'));
                if !sentence_ending {
                    continue;
                }
                let direct_references = u32::try_from(
                    production
                        .parts
                        .iter()
                        .filter(|part| {
                            matches!(
                                part,
                                CompiledPart::RuleCall { .. } | CompiledPart::Capture { .. }
                            )
                        })
                        .count(),
                )
                .unwrap_or(u32::MAX);
                let mut longest_literal_run = 0_u32;
                let mut current_literal_run = 0_u32;
                for part in &production.parts {
                    if let CompiledPart::Literal { text, .. } = part {
                        current_literal_run =
                            current_literal_run.saturating_add(crate::audit::count_words(text));
                        longest_literal_run = longest_literal_run.max(current_literal_run);
                    } else {
                        current_literal_run = 0;
                    }
                }
                let insufficient_references = direct_references < profile.minimum_direct_references;
                let excessive_literal_run = longest_literal_run > profile.maximum_literal_run_words;
                if insufficient_references || excessive_literal_run {
                    findings.push(CompositionFinding {
                        rule: rule.name.clone(),
                        production_index: u32::try_from(index).unwrap_or(u32::MAX),
                        production_id: production.id.clone(),
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

    pub(crate) fn generate_diverse_candidate(
        &self,
        request: &GenerationRequest<'_>,
        state: &mut DiverseCandidateState<'_>,
    ) -> MecoResult<GenerationResult> {
        let structural = self.generate_weighted_structural_internal(request, None, Some(state))?;
        let GeneratedContent::Text(text) = structural.content else {
            return Err(runtime_error(
                DiagnosticCode::FORMATTER_REQUIRED,
                "diverse complete-message generation requires a formatter-aware session",
            ));
        };
        Ok(GenerationResult {
            text,
            entry: structural.entry,
            expansions: structural.expansions,
            sampler_words: structural.sampler_words,
            bindings: structural.bindings,
            selections: structural.selections,
            provenance: structural.provenance,
            formatter_diagnostics: Vec::new(),
            message: None,
        })
    }

    /// Generates one exact independent weighted derivation without recursion on
    /// the native call stack.
    ///
    /// # Errors
    ///
    /// Returns a stable diagnostic when the entry is absent or not public, a
    /// deterministic work limit is reached, or unbiased sampling exhausts its
    /// configured word budget.
    #[allow(clippy::too_many_lines)]
    pub fn generate_weighted(
        &self,
        request: &GenerationRequest<'_>,
    ) -> MecoResult<GenerationResult> {
        let (_, entry_rule) = self.resolve_entry(request.entry)?;
        if self.rules[entry_rule].message_effect {
            return Err(runtime_error(
                DiagnosticCode::FORMATTER_REQUIRED,
                "the selected entry produces a complete message and requires a formatter",
            ));
        }
        let structural = self.generate_weighted_structural(request, None)?;
        let GeneratedContent::Text(text) = structural.content else {
            return Err(runtime_error(
                DiagnosticCode::FORMATTER_REQUIRED,
                "the selected entry produces a complete message and requires a formatter",
            ));
        };
        Ok(GenerationResult {
            text,
            entry: structural.entry,
            expansions: structural.expansions,
            sampler_words: structural.sampler_words,
            bindings: structural.bindings,
            selections: structural.selections,
            provenance: structural.provenance,
            formatter_diagnostics: Vec::new(),
            message: None,
        })
    }

    /// Generates structure and returns a complete formatter request without
    /// invoking host code.
    ///
    /// # Errors
    ///
    /// Returns stable generation diagnostics, or `E_LOCALE` when a selected
    /// complete message has no explicit locale request.
    #[allow(clippy::too_many_lines)]
    pub fn generate_weighted_structural(
        &self,
        request: &GenerationRequest<'_>,
        locale: Option<LocaleRequest<'_>>,
    ) -> MecoResult<StructuralGenerationResult> {
        self.generate_weighted_structural_internal(request, locale, None)
    }

    #[allow(clippy::too_many_lines)]
    fn generate_weighted_structural_internal(
        &self,
        request: &GenerationRequest<'_>,
        locale: Option<LocaleRequest<'_>>,
        mut diversity: Option<&mut DiverseCandidateState<'_>>,
    ) -> MecoResult<StructuralGenerationResult> {
        let (entry_name, entry_rule) = self.resolve_entry(request.entry)?;
        let inputs = self.validate_request_data(request.data)?;
        let mut random = SplitMix64::new(request.seed);
        let mut buffers = vec![String::new()];
        let mut frames = Vec::<RuntimeFrame>::new();
        let mut binding_trace = Vec::new();
        let mut selection_trace = Vec::new();
        let mut provenance = Vec::new();
        let mut output_scalars = 0_u32;
        let mut expansions = 0_u32;
        let mut message = None;
        let mut stack = vec![Work::Expand {
            rule: entry_rule,
            arguments: Vec::new(),
            argument_origins: Vec::new(),
            sink: 0,
            depth: 1,
            parent_trace: None,
        }];

        while let Some(work) = stack.pop() {
            match work {
                Work::Continue {
                    rule,
                    production,
                    part,
                    frame,
                    sink,
                    depth,
                    trace_node,
                } => {
                    let parts = &self.rules[rule].productions[production].parts;
                    let Some(compiled_part) = parts.get(part) else {
                        continue;
                    };
                    stack.push(Work::Continue {
                        rule,
                        production,
                        part: part + 1,
                        frame,
                        sink,
                        depth,
                        trace_node,
                    });
                    match compiled_part {
                        CompiledPart::Literal { text, span } => {
                            let start = output_cursor(&buffers[sink]);
                            append_output(
                                &mut buffers[sink],
                                text,
                                &mut output_scalars,
                                request.limits,
                            )?;
                            record_emission(
                                &mut provenance,
                                request.trace_provenance,
                                trace_node,
                                ProvenanceKind::AuthoredText,
                                &self.rules[rule],
                                production,
                                *span,
                                sink,
                                start,
                                &buffers[sink],
                                depth,
                                None,
                            );
                        }
                        CompiledPart::Value { value, span } => {
                            let origin = compiled_value_origin(value, &frames[frame].origins);
                            let (provenance_kind, provenance_parent) = match origin {
                                ValueOrigin::Host => (ProvenanceKind::HostValue, trace_node),
                                ValueOrigin::Derived(origin) => {
                                    (ProvenanceKind::BoundValue, origin.or(trace_node))
                                }
                                ValueOrigin::Constant => (ProvenanceKind::BoundValue, trace_node),
                            };
                            let value = runtime_value(value, &inputs, &frames[frame].values)?;
                            let Value::Text(text) = value else {
                                return Err(runtime_error(
                                    DiagnosticCode::TYPE_MISMATCH,
                                    "only text values can be emitted directly",
                                ));
                            };
                            let start = output_cursor(&buffers[sink]);
                            append_output(
                                &mut buffers[sink],
                                &text,
                                &mut output_scalars,
                                request.limits,
                            )?;
                            record_emission(
                                &mut provenance,
                                request.trace_provenance,
                                provenance_parent,
                                provenance_kind,
                                &self.rules[rule],
                                production,
                                *span,
                                sink,
                                start,
                                &buffers[sink],
                                depth,
                                None,
                            );
                        }
                        CompiledPart::RuleCall {
                            rule, arguments, ..
                        } => {
                            let argument_origins =
                                evaluate_argument_origins(arguments, &frames[frame].origins);
                            let arguments =
                                evaluate_arguments(arguments, &inputs, &frames[frame].values)?;
                            stack.push(Work::Expand {
                                rule: *rule,
                                arguments,
                                argument_origins,
                                sink,
                                depth: depth.saturating_add(1),
                                parent_trace: trace_node,
                            });
                        }
                        CompiledPart::Capture {
                            rule: target_rule,
                            slot,
                            name,
                            span,
                        } => {
                            let start = buffers[sink].len();
                            let start_scalar = scalar_len(&buffers[sink]);
                            let capture_trace = push_provenance(
                                &mut provenance,
                                request.trace_provenance,
                                trace_node,
                                ProvenanceKind::EmittingCapture,
                                &self.rules[rule],
                                production,
                                *span,
                                depth,
                                Some(name.clone()),
                            );
                            stack.push(Work::FinishCapture {
                                frame,
                                slot: *slot,
                                name: name.clone(),
                                sink,
                                start,
                                start_scalar,
                                trace_node: capture_trace,
                            });
                            stack.push(Work::Expand {
                                rule: *target_rule,
                                arguments: Vec::new(),
                                argument_origins: Vec::new(),
                                sink,
                                depth: depth.saturating_add(1),
                                parent_trace: capture_trace,
                            });
                        }
                        CompiledPart::MessageCall {
                            id,
                            arguments,
                            span,
                        } => {
                            let locale = locale.ok_or_else(|| {
                                runtime_error(
                                    DiagnosticCode::LOCALE,
                                    "complete-message generation requires an explicit locale",
                                )
                            })?;
                            validate_locale_request(locale)?;
                            if message.is_some() {
                                return Err(runtime_error(
                                    DiagnosticCode::MESSAGE_EFFECT,
                                    "one derivation cannot produce multiple complete messages",
                                ));
                            }
                            let arguments = arguments
                                .iter()
                                .map(|(name, value)| {
                                    Ok((
                                        name.clone(),
                                        runtime_value(value, &inputs, &frames[frame].values)?,
                                    ))
                                })
                                .collect::<MecoResult<Vec<_>>>()?;
                            message = Some(FormatterRequest::new(
                                id.clone(),
                                arguments,
                                locale.requested.to_string(),
                                locale
                                    .fallbacks
                                    .iter()
                                    .map(|fallback| (*fallback).to_string())
                                    .collect(),
                            ));
                            let _ = push_provenance(
                                &mut provenance,
                                request.trace_provenance,
                                trace_node,
                                ProvenanceKind::Message,
                                &self.rules[rule],
                                production,
                                *span,
                                depth,
                                Some(id.clone()),
                            );
                        }
                    }
                }
                Work::PrepareBinding {
                    rule,
                    production,
                    binding,
                    frame,
                    sink,
                    depth,
                    trace_node,
                } => {
                    let bindings = &self.rules[rule].productions[production].bindings;
                    let Some(compiled_binding) = bindings.get(binding) else {
                        stack.push(Work::Continue {
                            rule,
                            production,
                            part: 0,
                            frame,
                            sink,
                            depth,
                            trace_node,
                        });
                        continue;
                    };
                    let temporary_sink = buffers.len();
                    buffers.push(String::new());
                    let binding_trace_node = push_provenance(
                        &mut provenance,
                        request.trace_provenance,
                        trace_node,
                        ProvenanceKind::Binding,
                        &self.rules[rule],
                        production,
                        compiled_binding.span,
                        depth,
                        Some(compiled_binding.name.clone()),
                    );
                    stack.push(Work::PrepareBinding {
                        rule,
                        production,
                        binding: binding + 1,
                        frame,
                        sink,
                        depth,
                        trace_node,
                    });
                    stack.push(Work::FinishBinding {
                        frame,
                        slot: compiled_binding.slot,
                        name: compiled_binding.name.clone(),
                        sink: temporary_sink,
                        trace_node: binding_trace_node,
                    });
                    stack.push(Work::Expand {
                        rule: compiled_binding.rule,
                        argument_origins: evaluate_argument_origins(
                            &compiled_binding.arguments,
                            &frames[frame].origins,
                        ),
                        arguments: evaluate_arguments(
                            &compiled_binding.arguments,
                            &inputs,
                            &frames[frame].values,
                        )?,
                        sink: temporary_sink,
                        depth: depth.saturating_add(1),
                        parent_trace: binding_trace_node,
                    });
                }
                Work::FinishBinding {
                    frame,
                    slot,
                    name,
                    sink,
                    trace_node,
                } => {
                    let text = core::mem::take(&mut buffers[sink]);
                    let value = Value::Text(text);
                    bind_runtime_value(
                        &mut frames[frame],
                        slot,
                        value.clone(),
                        ValueOrigin::Derived(trace_node),
                    )?;
                    if request.trace_bindings {
                        binding_trace.push(BindingTrace::new(name, value, false));
                    }
                }
                Work::FinishCapture {
                    frame,
                    slot,
                    name,
                    sink,
                    start,
                    start_scalar,
                    trace_node,
                } => {
                    let text = buffers[sink]
                        .get(start..)
                        .ok_or_else(|| {
                            runtime_error(
                                DiagnosticCode::INVALID_SPAN,
                                "capture output boundary is invalid",
                            )
                        })?
                        .to_string();
                    let value = Value::Text(text);
                    bind_runtime_value(
                        &mut frames[frame],
                        slot,
                        value.clone(),
                        ValueOrigin::Derived(trace_node),
                    )?;
                    if request.trace_bindings {
                        binding_trace.push(BindingTrace::new(name, value, true));
                    }
                    finish_provenance(
                        &mut provenance,
                        trace_node,
                        sink,
                        start,
                        start_scalar,
                        &buffers[sink],
                    );
                }
                Work::FinishProduction {
                    sink,
                    start,
                    start_scalar,
                    trace_node,
                } => finish_provenance(
                    &mut provenance,
                    trace_node,
                    sink,
                    start,
                    start_scalar,
                    &buffers[sink],
                ),
                Work::Expand {
                    rule,
                    arguments,
                    argument_origins,
                    sink,
                    depth,
                    parent_trace,
                } => {
                    if depth > request.limits.max_depth {
                        return Err(runtime_error(
                            DiagnosticCode::LIMIT_DEPTH,
                            format!("derivation depth exceeds {}", request.limits.max_depth),
                        ));
                    }
                    expansions = expansions.checked_add(1).ok_or_else(|| {
                        runtime_error(
                            DiagnosticCode::LIMIT_EXPANSIONS,
                            "rule expansion counter overflowed",
                        )
                    })?;
                    if expansions > request.limits.max_expansions {
                        return Err(runtime_error(
                            DiagnosticCode::LIMIT_EXPANSIONS,
                            format!("rule expansions exceed {}", request.limits.max_expansions),
                        ));
                    }
                    let compiled_rule = &self.rules[rule];
                    if arguments.len() != compiled_rule.parameters.len()
                        || argument_origins.len() != arguments.len()
                    {
                        return Err(runtime_error(
                            DiagnosticCode::RULE_ARITY,
                            format!(
                                "rule `{}` expected {} arguments but received {}",
                                compiled_rule.name,
                                compiled_rule.parameters.len(),
                                arguments.len()
                            ),
                        ));
                    }
                    let frame = frames.len();
                    frames.push(RuntimeFrame {
                        values: arguments,
                        origins: argument_origins,
                    });
                    let fast_static = diversity.is_none() && !request.trace_selections;
                    let mut weighted = None;
                    let total = if fast_static {
                        compiled_rule
                            .static_selection
                            .as_ref()
                            .map(|selection| selection.total)
                    } else {
                        None
                    };
                    let total = if let Some(total) = total {
                        total
                    } else {
                        let mut eligible =
                            eligible_weights(compiled_rule, &inputs, &frames[frame].values)?;
                        if let Some(state) = diversity.as_deref_mut() {
                            eligible = diverse_eligible_weights(compiled_rule, eligible, state)?;
                        }
                        let total = eligible
                            .iter()
                            .try_fold(0_u64, |sum, weight| sum.checked_add(weight.normalized))
                            .ok_or_else(|| {
                                weight_runtime_overflow("eligible weight total overflowed")
                            })?;
                        weighted = Some(eligible);
                        total
                    };
                    let used = u32::try_from(random.words()).unwrap_or(u32::MAX);
                    let remaining = request.limits.max_sampler_words.saturating_sub(used);
                    let choice = random
                        .uniform_below(total, u64::from(remaining))
                        .ok_or_else(|| {
                            runtime_error(
                                DiagnosticCode::SAMPLER_BUDGET,
                                format!(
                                    "weighted sampling exceeds {} PRNG words",
                                    request.limits.max_sampler_words
                                ),
                            )
                        })?;
                    let production = weighted.as_ref().map_or_else(
                        || {
                            compiled_rule
                                .static_selection
                                .as_ref()
                                .expect("fast static selection was prepared at compilation")
                                .select(choice)
                        },
                        |eligible| select_eligible_production(eligible, choice),
                    );
                    if let Some(state) = diversity.as_deref_mut() {
                        state.record(
                            &compiled_rule.name,
                            &compiled_rule.productions[production].id,
                        );
                    }
                    if request.trace_selections {
                        selection_trace.push(SelectionTrace::new(
                            compiled_rule.name.clone(),
                            u32::try_from(production).unwrap_or(u32::MAX),
                            compiled_rule.productions[production].id.clone(),
                            weighted
                                .as_ref()
                                .expect("selection tracing uses the full eligible set")
                                .iter()
                                .map(|weight| {
                                    EligibleWeightTrace::new(
                                        u32::try_from(weight.production).unwrap_or(u32::MAX),
                                        compiled_rule.productions[weight.production].id.clone(),
                                        weight.base,
                                        weight.normalized,
                                    )
                                })
                                .collect(),
                        ));
                    }
                    let start = buffers[sink].len();
                    let start_scalar = scalar_len(&buffers[sink]);
                    let production_trace = push_provenance(
                        &mut provenance,
                        request.trace_provenance,
                        parent_trace,
                        ProvenanceKind::Production,
                        compiled_rule,
                        production,
                        compiled_rule.productions[production].span,
                        depth,
                        None,
                    );
                    stack.push(Work::FinishProduction {
                        sink,
                        start,
                        start_scalar,
                        trace_node: production_trace,
                    });
                    stack.push(Work::PrepareBinding {
                        rule,
                        production,
                        binding: 0,
                        frame,
                        sink,
                        depth,
                        trace_node: production_trace,
                    });
                }
            }
        }

        let text = buffers.swap_remove(0);
        let content = if let Some(message) = message {
            if !text.is_empty() {
                return Err(runtime_error(
                    DiagnosticCode::MESSAGE_EFFECT,
                    "a complete message cannot be combined with generated text",
                ));
            }
            GeneratedContent::Message(message)
        } else {
            GeneratedContent::Text(text)
        };
        Ok(StructuralGenerationResult {
            content,
            entry: entry_name.to_string(),
            expansions,
            sampler_words: u32::try_from(random.words()).unwrap_or(u32::MAX),
            bindings: binding_trace,
            selections: selection_trace,
            provenance,
        })
    }

    /// Generates and synchronously formats one complete-message derivation.
    /// Ordinary text entries bypass the formatter.
    ///
    /// # Errors
    ///
    /// Returns generation, locale, formatter, or aggregate output-limit
    /// diagnostics without exposing partial output.
    pub fn generate_weighted_with_formatter(
        &self,
        request: &GenerationRequest<'_>,
        locale: LocaleRequest<'_>,
        formatter: &mut impl Formatter,
    ) -> MecoResult<GenerationResult> {
        let structural = self.generate_weighted_structural(request, Some(locale))?;
        let formatter_request = match structural.content {
            GeneratedContent::Message(formatter_request) => formatter_request,
            GeneratedContent::Text(text) => {
                return Ok(GenerationResult {
                    text,
                    entry: structural.entry,
                    expansions: structural.expansions,
                    sampler_words: structural.sampler_words,
                    bindings: structural.bindings,
                    selections: structural.selections,
                    provenance: structural.provenance,
                    formatter_diagnostics: Vec::new(),
                    message: None,
                });
            }
        };
        let response = formatter.format(&formatter_request)?;
        validate_formatter_result(&formatter_request, &response, request.limits)?;
        let message_trace = MessageTrace::new(
            formatter_request.message_id().to_string(),
            formatter_request.requested_locale().to_string(),
            response.actual_locale.clone(),
            response.environment_hash.clone(),
            response.work_units,
            response.replayable,
        );
        let mut provenance = structural.provenance;
        finalize_formatted_provenance(&mut provenance, &response.text);
        Ok(GenerationResult {
            text: response.text,
            entry: structural.entry,
            expansions: structural.expansions,
            sampler_words: structural.sampler_words,
            bindings: structural.bindings,
            selections: structural.selections,
            provenance,
            formatter_diagnostics: response.diagnostics,
            message: Some(message_trace),
        })
    }

    fn validate_request_data(&self, data: &[DataBinding]) -> MecoResult<Vec<Value>> {
        if let Some(duplicate) = data.iter().enumerate().find_map(|(index, binding)| {
            data[..index]
                .iter()
                .any(|previous| previous.name == binding.name)
                .then_some(binding.name.as_str())
        }) {
            return Err(runtime_error(
                DiagnosticCode::REQUEST_DATA,
                format!("request data contains duplicate `{duplicate}`"),
            ));
        }
        if let Some(unknown) = data.iter().find(|binding| {
            !self
                .inputs
                .iter()
                .any(|input| input.external_name == binding.name)
        }) {
            return Err(runtime_error(
                DiagnosticCode::REQUEST_DATA,
                format!("request data contains unknown input `{}`", unknown.name),
            ));
        }
        let mut values = Vec::with_capacity(self.inputs.len());
        for input in &self.inputs {
            let binding = data
                .iter()
                .find(|binding| binding.name == input.external_name)
                .ok_or_else(|| {
                    runtime_error(
                        DiagnosticCode::REQUEST_DATA,
                        format!("request data is missing `{}`", input.external_name),
                    )
                })?;
            validate_runtime_type(&binding.value, &input.type_).map_err(|message| {
                runtime_error(
                    DiagnosticCode::TYPE_MISMATCH,
                    format!("input `{}` {message}", input.external_name),
                )
            })?;
            values.push(binding.value.clone());
        }
        Ok(values)
    }

    fn resolve_entry(&self, requested: Option<&str>) -> MecoResult<(&str, usize)> {
        if let Some(requested) = requested {
            return self
                .entries
                .iter()
                .find(|(name, _)| name == requested)
                .map(|(name, rule)| (name.as_str(), *rule))
                .ok_or_else(|| {
                    runtime_error(
                        DiagnosticCode::NO_ENTRY,
                        format!("`{requested}` is not a public package entry"),
                    )
                });
        }
        let rule = self.default_entry.ok_or_else(|| {
            runtime_error(
                DiagnosticCode::NO_ENTRY,
                "generation requires an explicit public entry",
            )
        })?;
        Ok((self.rules[rule].name.as_str(), rule))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Work {
    Expand {
        rule: usize,
        arguments: Vec<Value>,
        argument_origins: Vec<ValueOrigin>,
        sink: usize,
        depth: u32,
        parent_trace: Option<u32>,
    },
    Continue {
        rule: usize,
        production: usize,
        part: usize,
        frame: usize,
        sink: usize,
        depth: u32,
        trace_node: Option<u32>,
    },
    PrepareBinding {
        rule: usize,
        production: usize,
        binding: usize,
        frame: usize,
        sink: usize,
        depth: u32,
        trace_node: Option<u32>,
    },
    FinishBinding {
        frame: usize,
        slot: usize,
        name: String,
        sink: usize,
        trace_node: Option<u32>,
    },
    FinishCapture {
        frame: usize,
        slot: usize,
        name: String,
        sink: usize,
        start: usize,
        start_scalar: u64,
        trace_node: Option<u32>,
    },
    FinishProduction {
        sink: usize,
        start: usize,
        start_scalar: u64,
        trace_node: Option<u32>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RuntimeFrame {
    values: Vec<Value>,
    origins: Vec<ValueOrigin>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ValueOrigin {
    Host,
    Derived(Option<u32>),
    Constant,
}

struct ModuleBuild<'a> {
    canonical_id: &'a str,
    syntax: ModuleSyntax,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ModuleSchema {
    types: Vec<(String, ValueType)>,
    inputs: Vec<(String, usize, ValueType)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CompileScope {
    module: usize,
    locals: Vec<(String, ValueType)>,
}

type EntryIndex = Vec<(String, usize)>;

struct CompiledEntries {
    entries: EntryIndex,
    default_entry: Option<usize>,
}

/// Compiles a host-supplied package into immutable indexed weighted IR.
///
/// # Errors
///
/// Returns structured diagnostics for package identity, imports, exports,
/// entries, visibility, references, arity, unsupported later-phase constructs,
/// exact static weight totals, or reachable unproductive rules.
#[allow(clippy::too_many_lines)]
pub fn compile_package(package: &PackageInput) -> MecoResult<CompiledGrammar> {
    compile_package_with_manifest(package, &MessageManifest::default())
}

/// Compiles a package against an exact external complete-message schema.
///
/// # Errors
///
/// Returns all ordinary compilation diagnostics plus stable manifest, missing
/// message, message-argument, and complete-message-effect diagnostics.
#[allow(clippy::too_many_lines)]
pub fn compile_package_with_manifest(
    package: &PackageInput,
    manifest: &MessageManifest,
) -> MecoResult<CompiledGrammar> {
    validate_message_manifest(manifest)?;
    let canonical_order = package.modules.windows(2).all(|pair| {
        let left = (
            pair[0].canonical_id != package.root_id,
            pair[0].canonical_id.as_str(),
        );
        let right = (
            pair[1].canonical_id != package.root_id,
            pair[1].canonical_id.as_str(),
        );
        left <= right
    });
    let canonical_source_ids = package.modules.iter().enumerate().all(|(index, module)| {
        module.source.id().get() == u32::try_from(index).unwrap_or(u32::MAX)
    });
    if !canonical_order || !canonical_source_ids {
        let mut canonical = package.clone();
        canonical.modules.sort_by(|left, right| {
            let left_key = (
                left.canonical_id != package.root_id,
                left.canonical_id.as_str(),
            );
            let right_key = (
                right.canonical_id != package.root_id,
                right.canonical_id.as_str(),
            );
            left_key.cmp(&right_key)
        });
        for (index, module) in canonical.modules.iter_mut().enumerate() {
            let source_id = u32::try_from(index).map_err(|_| {
                runtime_error(
                    DiagnosticCode::PACKAGE_DUPLICATE_MODULE,
                    "package contains more than u32::MAX modules",
                )
            })?;
            module.source = SourceFile::new(
                SourceId::new(source_id),
                module.source.name(),
                module.source.text(),
            );
        }
        return compile_package_with_manifest(&canonical, manifest);
    }
    validate_package_input(package)?;
    let mut modules = Vec::with_capacity(package.modules.len());
    for package_source in &package.modules {
        modules.push(ModuleBuild {
            canonical_id: &package_source.canonical_id,
            syntax: parse_module(&package_source.source)?,
        });
    }
    validate_module_contracts(package, &modules)?;
    validate_acyclic_imports(package, &modules)?;

    let root_module = modules
        .iter()
        .position(|module| module.canonical_id == package.root_id)
        .ok_or_else(|| {
            runtime_error(
                DiagnosticCode::PACKAGE_ROOT,
                "validated package root disappeared during compilation",
            )
        })?;
    let mut offsets = Vec::with_capacity(modules.len());
    let mut rule_count = 0_usize;
    for module in &modules {
        offsets.push(rule_count);
        rule_count += module.syntax.rules.len();
    }

    let (schemas, inputs) = compile_schemas(&modules, root_module)?;
    let mut rule_parameters = Vec::with_capacity(rule_count);
    for (module_index, module) in modules.iter().enumerate() {
        for rule in &module.syntax.rules {
            let mut parameters = Vec::with_capacity(rule.parameters.len());
            for parameter in &rule.parameters {
                if parameters
                    .iter()
                    .any(|(name, _): &(String, ValueType)| name == parameter.name.value())
                    || schemas[module_index]
                        .inputs
                        .iter()
                        .any(|(name, _, _)| name == parameter.name.value())
                {
                    return Err(source_error(
                        DiagnosticCode::BINDING_NAME,
                        parameter.name.span(),
                        format!(
                            "parameter `{}` shadows an existing value",
                            parameter.name.value()
                        ),
                    ));
                }
                parameters.push((
                    parameter.name.value().clone(),
                    resolve_type(&schemas[module_index], parameter.type_name.value()).ok_or_else(
                        || {
                            source_error(
                                DiagnosticCode::TYPE,
                                parameter.type_name.span(),
                                format!("unknown type `{}`", parameter.type_name.value()),
                            )
                        },
                    )?,
                ));
            }
            rule_parameters.push(parameters);
        }
    }

    let mut rules = Vec::with_capacity(rule_count);
    for (module_index, module) in modules.iter().enumerate() {
        for (local_rule, rule) in module.syntax.rules.iter().enumerate() {
            let global_rule = offsets[module_index] + local_rule;
            let qualified_rule = format!(
                "{}.{}",
                module.syntax.front_matter.module().value(),
                rule.name.value()
            );
            let mut productions = Vec::<CompiledProduction>::with_capacity(rule.productions.len());
            let mut production_ids = BTreeMap::<String, Span>::new();
            for production in &rule.productions {
                let mut scope = CompileScope {
                    module: module_index,
                    locals: rule_parameters[global_rule].clone(),
                };
                let guard = compile_guards(&production.clauses, &scope, &schemas)?;
                let weight = compile_weight(&production.weight, &scope, &schemas)?;
                let mut bindings = Vec::new();
                for clause in &production.clauses {
                    let ClauseSyntax::Binding(binding) = clause else {
                        continue;
                    };
                    ensure_new_local(&scope, &schemas, binding.name.value(), binding.name.span())?;
                    let target = resolve_rule(
                        package,
                        &modules,
                        &offsets,
                        module_index,
                        binding.rule.value(),
                        binding.rule.span(),
                    )?;
                    let call = crate::CallSyntax {
                        target: binding.rule.clone(),
                        arguments: binding.arguments.clone(),
                        span: binding.span,
                    };
                    let arguments =
                        compile_call_arguments(&call, &rule_parameters[target], &scope, &schemas)?;
                    let slot = scope.locals.len();
                    bindings.push(CompiledBinding {
                        rule: target,
                        arguments,
                        slot,
                        name: binding.name.value().clone(),
                        span: binding.span,
                    });
                    scope
                        .locals
                        .push((binding.name.value().clone(), ValueType::Text));
                }
                let parts = lower_body(
                    package,
                    &modules,
                    &offsets,
                    &schemas,
                    &rule_parameters,
                    &mut scope,
                    &production.body,
                    manifest,
                )?;
                validate_bound_value_usage(production.span, &bindings, &parts)?;
                let id = stable_production_id(&qualified_rule, production);
                if let Some(previous_span) = production_ids.insert(id.clone(), production.span) {
                    return Err(MecoError::with_related(
                        Diagnostic::new(
                            DiagnosticCode::PRODUCTION_ID,
                            Severity::Error,
                            Some(production.span),
                            format!("production ID `{id}` is not unique within `{qualified_rule}`"),
                        ),
                        [Diagnostic::new(
                            DiagnosticCode::PRODUCTION_ID,
                            Severity::Error,
                            Some(previous_span),
                            "the colliding production is here",
                        )],
                    ));
                }
                productions.push(CompiledProduction {
                    id,
                    authored_id: production.authored_id.is_some(),
                    span: production.span,
                    weight,
                    guard,
                    bindings,
                    parts,
                    diversity_factor_16_16: 1 << 16,
                });
            }
            let static_selection = prepare_static_selection(rule.span, &productions)?;
            rules.push(CompiledRule {
                name: qualified_rule,
                parameters: rule_parameters[global_rule].clone(),
                span: rule.span,
                productions,
                static_selection,
                analysis: RuleAnalysis {
                    reachable: false,
                    productive: false,
                    nullable: false,
                    recursive: false,
                },
                message_effect: false,
            });
        }
    }

    let compiled_entries = compile_entries(&modules, &offsets, root_module)?;
    let entries = compiled_entries.entries;
    let default_entry = compiled_entries.default_entry;
    if let Some((entry, rule)) = entries
        .iter()
        .find(|(_, rule)| !rules[*rule].parameters.is_empty())
    {
        return Err(source_error(
            DiagnosticCode::RULE_ARITY,
            rules[*rule].span,
            format!("public entry `{entry}` cannot require rule parameters"),
        ));
    }
    validate_message_effects(&mut rules)?;
    analyze_graph(&mut rules, &entries)?;
    prepare_diversity_metadata(&mut rules);
    let warnings = recursion_warnings(&rules);
    let grammar = CompiledGrammar {
        artifact_hash: package_artifact_hash(package, manifest),
        rules,
        inputs,
        entries,
        default_entry,
        warnings,
        message_manifest: manifest.clone(),
    };
    grammar.validate_lowered_invariants()?;
    Ok(grammar)
}

fn package_artifact_hash(package: &PackageInput, manifest: &MessageManifest) -> u64 {
    let mut hash = StableHasher::new();
    hash.string("mecojoni-artifact-fnv1a64/1");
    hash.string(&package.root_id);
    let mut modules = package.modules.iter().collect::<Vec<_>>();
    modules.sort_by(|left, right| left.canonical_id.cmp(&right.canonical_id));
    for module in modules {
        hash.string(&module.canonical_id);
        hash.string(module.source.text());
        let mut resolutions = module.resolved_imports.iter().collect::<Vec<_>>();
        resolutions.sort_by(|left, right| {
            (&left.authored_path, &left.target_id).cmp(&(&right.authored_path, &right.target_id))
        });
        for resolution in resolutions {
            hash.string(&resolution.authored_path);
            hash.string(&resolution.target_id);
        }
    }
    let mut messages = manifest.messages.iter().collect::<Vec<_>>();
    messages.sort_by(|left, right| left.id.cmp(&right.id));
    for message in messages {
        hash.string(&message.id);
        for argument in &message.arguments {
            hash.string(&argument.name);
            hash.string(schema_type_name(&argument.type_));
        }
    }
    hash.finish()
}

fn schema_type_from_value_type(type_: &ValueType) -> SchemaType {
    match type_ {
        ValueType::Text => SchemaType::Text,
        ValueType::Number => SchemaType::Number,
        ValueType::Boolean => SchemaType::Boolean,
        ValueType::Enum { name, .. } => SchemaType::Enum(name.clone()),
    }
}

fn validate_module_contracts(
    package: &PackageInput,
    modules: &[ModuleBuild<'_>],
) -> MecoResult<()> {
    for (index, module) in modules.iter().enumerate() {
        if modules[..index].iter().any(|previous| {
            previous.syntax.front_matter.module().value()
                == module.syntax.front_matter.module().value()
        }) {
            return Err(source_error(
                DiagnosticCode::MODULE_IDENTITY,
                module.syntax.front_matter.module().span(),
                format!(
                    "duplicate declared module `{}`",
                    module.syntax.front_matter.module().value()
                ),
            ));
        }
        let is_root = module.canonical_id == package.root_id;
        if !is_root {
            if let Some(entry) = module.syntax.front_matter.entry() {
                return Err(source_error(
                    DiagnosticCode::ENTRY,
                    entry.span(),
                    "only the package root may declare `entry`",
                ));
            }
            if let Some(sampler) = module.syntax.front_matter.sampler() {
                return Err(source_error(
                    DiagnosticCode::ENTRY,
                    sampler.span(),
                    "only the package root may recommend a sampler",
                ));
            }
        }
        for export in module.syntax.front_matter.exports() {
            if !module
                .syntax
                .rules
                .iter()
                .any(|rule| rule.name.value() == export.value())
            {
                return Err(source_error(
                    DiagnosticCode::EXPORT,
                    export.span(),
                    format!("exported rule `{}` is undefined", export.value()),
                ));
            }
        }
        if let Some(entry) = module.syntax.front_matter.entry() {
            if !module
                .syntax
                .front_matter
                .exports()
                .iter()
                .any(|export| export.value() == entry.value())
            {
                return Err(source_error(
                    DiagnosticCode::ENTRY,
                    entry.span(),
                    "the default entry must also appear in `exports`",
                ));
            }
        }
    }
    Ok(())
}

fn validate_acyclic_imports(package: &PackageInput, modules: &[ModuleBuild<'_>]) -> MecoResult<()> {
    let mut incoming = vec![0_usize; modules.len()];
    let mut edges = vec![Vec::new(); modules.len()];
    for (module_index, _module) in modules.iter().enumerate() {
        let package_source = &package.modules[module_index];
        for resolution in &package_source.resolved_imports {
            let target = modules
                .iter()
                .position(|candidate| candidate.canonical_id == resolution.target_id)
                .ok_or_else(|| {
                    runtime_error(
                        DiagnosticCode::IMPORT_RESOLUTION,
                        "validated import target disappeared during compilation",
                    )
                })?;
            edges[module_index].push(target);
            incoming[target] += 1;
        }
    }
    let mut queue = incoming
        .iter()
        .enumerate()
        .filter_map(|(index, degree)| (*degree == 0).then_some(index))
        .collect::<VecDeque<_>>();
    let mut visited = 0_usize;
    while let Some(module) = queue.pop_front() {
        visited += 1;
        for target in &edges[module] {
            incoming[*target] -= 1;
            if incoming[*target] == 0 {
                queue.push_back(*target);
            }
        }
    }
    if visited != modules.len() {
        return Err(runtime_error(
            DiagnosticCode::IMPORT_CYCLE,
            "format 2 packages cannot contain import cycles",
        ));
    }
    Ok(())
}

fn compile_entries(
    modules: &[ModuleBuild<'_>],
    offsets: &[usize],
    root: usize,
) -> MecoResult<CompiledEntries> {
    let module = &modules[root];
    let mut entries = Vec::new();
    for export in module.syntax.front_matter.exports() {
        let local = module
            .syntax
            .rules
            .iter()
            .position(|rule| rule.name.value() == export.value())
            .ok_or_else(|| {
                source_error(
                    DiagnosticCode::EXPORT,
                    export.span(),
                    "validated export disappeared during compilation",
                )
            })?;
        entries.push((
            format!(
                "{}.{}",
                module.syntax.front_matter.module().value(),
                export.value()
            ),
            offsets[root] + local,
        ));
    }
    let default_entry = if let Some(entry) = module.syntax.front_matter.entry() {
        let local = module
            .syntax
            .rules
            .iter()
            .position(|rule| rule.name.value() == entry.value())
            .ok_or_else(|| {
                source_error(
                    DiagnosticCode::ENTRY,
                    entry.span(),
                    "validated entry disappeared during compilation",
                )
            })?;
        Some(offsets[root] + local)
    } else {
        None
    };
    Ok(CompiledEntries {
        entries,
        default_entry,
    })
}

fn compile_schemas(
    modules: &[ModuleBuild<'_>],
    root: usize,
) -> MecoResult<(Vec<ModuleSchema>, Vec<CompiledInput>)> {
    let mut schemas = Vec::with_capacity(modules.len());
    for module in modules {
        let mut types = Vec::new();
        for declaration in module.syntax.front_matter.types() {
            if matches!(
                declaration.name().value().as_str(),
                "text" | "number" | "boolean"
            ) {
                return Err(source_error(
                    DiagnosticCode::TYPE,
                    declaration.name().span(),
                    format!("`{}` is a built-in type", declaration.name().value()),
                ));
            }
            types.push((
                declaration.name().value().clone(),
                ValueType::Enum {
                    name: format!(
                        "{}.{}",
                        module.syntax.front_matter.module().value(),
                        declaration.name().value()
                    ),
                    variants: declaration
                        .variants()
                        .iter()
                        .map(|variant| variant.value().clone())
                        .collect(),
                },
            ));
        }
        schemas.push(ModuleSchema {
            types,
            inputs: Vec::new(),
        });
    }
    let mut inputs = Vec::new();
    for (module_index, module) in modules.iter().enumerate() {
        for declaration in module.syntax.front_matter.inputs() {
            let type_ = resolve_type(&schemas[module_index], declaration.type_name().value())
                .ok_or_else(|| {
                    source_error(
                        DiagnosticCode::TYPE,
                        declaration.type_name().span(),
                        format!("unknown type `{}`", declaration.type_name().value()),
                    )
                })?;
            let external_name = if module_index == root {
                declaration.name().value().clone()
            } else {
                format!(
                    "{}.{}",
                    module.syntax.front_matter.module().value(),
                    declaration.name().value()
                )
            };
            let index = inputs.len();
            inputs.push(CompiledInput {
                external_name,
                type_: type_.clone(),
            });
            schemas[module_index]
                .inputs
                .push((declaration.name().value().clone(), index, type_));
        }
    }
    Ok((schemas, inputs))
}

fn resolve_type(schema: &ModuleSchema, name: &str) -> Option<ValueType> {
    match name {
        "text" => Some(ValueType::Text),
        "number" => Some(ValueType::Number),
        "boolean" => Some(ValueType::Boolean),
        _ => schema
            .types
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, type_)| type_.clone()),
    }
}

fn ensure_new_local(
    scope: &CompileScope,
    schemas: &[ModuleSchema],
    name: &str,
    span: Span,
) -> MecoResult<()> {
    if scope.locals.iter().any(|(candidate, _)| candidate == name)
        || schemas[scope.module]
            .inputs
            .iter()
            .any(|(candidate, _, _)| candidate == name)
    {
        return Err(source_error(
            DiagnosticCode::BINDING_NAME,
            span,
            format!("binding `{name}` shadows an existing value"),
        ));
    }
    Ok(())
}

fn lookup_value(
    scope: &CompileScope,
    schemas: &[ModuleSchema],
    name: &str,
) -> Option<(CompiledValue, ValueType)> {
    if let Some((index, (_, type_))) = scope
        .locals
        .iter()
        .enumerate()
        .find(|(_, (candidate, _))| candidate == name)
    {
        return Some((CompiledValue::Local(index), type_.clone()));
    }
    schemas[scope.module]
        .inputs
        .iter()
        .find(|(candidate, _, _)| candidate == name)
        .map(|(_, index, type_)| (CompiledValue::Input(*index), type_.clone()))
}

#[allow(clippy::too_many_arguments)]
fn lower_body(
    package: &PackageInput,
    modules: &[ModuleBuild<'_>],
    offsets: &[usize],
    schemas: &[ModuleSchema],
    rule_parameters: &[Vec<(String, ValueType)>],
    scope: &mut CompileScope,
    body: &BodySyntax,
    manifest: &MessageManifest,
) -> MecoResult<Vec<CompiledPart>> {
    match body {
        BodySyntax::Empty(_) => Ok(Vec::new()),
        BodySyntax::Block(block) if block.raw => Ok(if block.text.value().is_empty() {
            Vec::new()
        } else {
            vec![CompiledPart::Literal {
                text: block.text.value().clone(),
                span: block.text.span(),
            }]
        }),
        BodySyntax::Block(block) => lower_parts(
            package,
            modules,
            offsets,
            schemas,
            rule_parameters,
            scope,
            manifest,
            block
                .parts
                .as_ref()
                .expect("cooked blocks have parsed parts"),
        ),
        BodySyntax::Parts(parts) => lower_parts(
            package,
            modules,
            offsets,
            schemas,
            rule_parameters,
            scope,
            manifest,
            parts,
        ),
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn lower_parts(
    package: &PackageInput,
    modules: &[ModuleBuild<'_>],
    offsets: &[usize],
    schemas: &[ModuleSchema],
    rule_parameters: &[Vec<(String, ValueType)>],
    scope: &mut CompileScope,
    manifest: &MessageManifest,
    parts: &[BodyPartSyntax],
) -> MecoResult<Vec<CompiledPart>> {
    let mut lowered = Vec::new();
    for part in parts {
        match part {
            BodyPartSyntax::Literal(literal) => {
                if !literal.value().is_empty() {
                    lowered.push(CompiledPart::Literal {
                        text: literal.value().clone(),
                        span: literal.span(),
                    });
                }
            }
            BodyPartSyntax::RuleReference(reference) => {
                let target = resolve_rule(
                    package,
                    modules,
                    offsets,
                    scope.module,
                    reference.value(),
                    reference.span(),
                )?;
                ensure_zero_arity(target, rule_parameters, reference.value(), reference.span())?;
                lowered.push(CompiledPart::RuleCall {
                    rule: target,
                    arguments: Vec::new(),
                    span: reference.span(),
                });
            }
            BodyPartSyntax::RuleCall(call) => {
                let target = resolve_rule(
                    package,
                    modules,
                    offsets,
                    scope.module,
                    call.target.value(),
                    call.target.span(),
                )?;
                lowered.push(CompiledPart::RuleCall {
                    rule: target,
                    arguments: compile_call_arguments(
                        call,
                        &rule_parameters[target],
                        scope,
                        schemas,
                    )?,
                    span: call.span,
                });
            }
            BodyPartSyntax::EmittingCapture { rule, name, span } => {
                ensure_new_local(scope, schemas, name.value(), name.span())?;
                let target = resolve_rule(
                    package,
                    modules,
                    offsets,
                    scope.module,
                    rule.value(),
                    rule.span(),
                )?;
                ensure_zero_arity(target, rule_parameters, rule.value(), rule.span())?;
                let slot = scope.locals.len();
                lowered.push(CompiledPart::Capture {
                    rule: target,
                    slot,
                    name: name.value().clone(),
                    span: *span,
                });
                scope.locals.push((name.value().clone(), ValueType::Text));
            }
            BodyPartSyntax::ValueReference(reference) => {
                let (value, type_) =
                    lookup_value(scope, schemas, reference.value()).ok_or_else(|| {
                        source_error(
                            DiagnosticCode::VALUE_NAME,
                            reference.span(),
                            format!("undefined value `{}`", reference.value()),
                        )
                    })?;
                if type_ != ValueType::Text {
                    return Err(source_error(
                        DiagnosticCode::TYPE_MISMATCH,
                        reference.span(),
                        format!(
                            "`{}` has type `{}` and cannot be emitted as text",
                            reference.value(),
                            type_.display_name()
                        ),
                    ));
                }
                lowered.push(CompiledPart::Value {
                    value,
                    span: reference.span(),
                });
            }
            BodyPartSyntax::MessageCall(call) => {
                let definition = manifest
                    .messages
                    .iter()
                    .find(|message| message.id == *call.target.value())
                    .ok_or_else(|| {
                        source_error(
                            DiagnosticCode::MESSAGE_MISSING,
                            call.target.span(),
                            format!(
                                "message `{}` is absent from the formatter manifest",
                                call.target.value()
                            ),
                        )
                    })?;
                lowered.push(CompiledPart::MessageCall {
                    id: definition.id.clone(),
                    arguments: compile_message_arguments(call, definition, scope, schemas)?,
                    span: call.span,
                });
            }
        }
    }
    Ok(lowered)
}

fn ensure_zero_arity(
    target: usize,
    rule_parameters: &[Vec<(String, ValueType)>],
    authored: &str,
    span: Span,
) -> MecoResult<()> {
    if rule_parameters[target].is_empty() {
        Ok(())
    } else {
        Err(source_error(
            DiagnosticCode::RULE_ARITY,
            span,
            format!(
                "rule `{authored}` requires {} named arguments",
                rule_parameters[target].len()
            ),
        ))
    }
}

fn compile_call_arguments(
    call: &crate::CallSyntax,
    parameters: &[(String, ValueType)],
    scope: &CompileScope,
    schemas: &[ModuleSchema],
) -> MecoResult<Vec<CompiledValue>> {
    if parameters.len() != call.arguments.len()
        || parameters.iter().any(|(parameter, _)| {
            !call
                .arguments
                .iter()
                .any(|argument| argument.name.value() == parameter)
        })
    {
        return Err(source_error(
            DiagnosticCode::RULE_ARITY,
            call.span,
            format!(
                "call to `{}` does not supply its {} named parameters",
                call.target.value(),
                parameters.len()
            ),
        ));
    }
    let mut compiled = Vec::with_capacity(parameters.len());
    for (parameter, expected) in parameters {
        let argument = call
            .arguments
            .iter()
            .find(|argument| argument.name.value() == parameter)
            .expect("arity validation found every named argument");
        let (value, actual) = compile_argument_value(argument, scope, schemas)?;
        if &actual != expected {
            return Err(source_error(
                DiagnosticCode::TYPE_MISMATCH,
                argument.span,
                format!(
                    "argument `{parameter}` expects `{}` but received `{}`",
                    expected.display_name(),
                    actual.display_name()
                ),
            ));
        }
        compiled.push(value);
    }
    Ok(compiled)
}

fn compile_message_arguments(
    call: &crate::CallSyntax,
    definition: &MessageDefinition,
    scope: &CompileScope,
    schemas: &[ModuleSchema],
) -> MecoResult<Vec<(String, CompiledValue)>> {
    if definition.arguments.len() != call.arguments.len()
        || definition.arguments.iter().any(|expected| {
            !call
                .arguments
                .iter()
                .any(|argument| argument.name.value() == &expected.name)
        })
    {
        return Err(source_error(
            DiagnosticCode::MESSAGE_ARGUMENT,
            call.span,
            format!(
                "message `{}` does not supply its {} manifest arguments",
                definition.id,
                definition.arguments.len()
            ),
        ));
    }
    let mut compiled = Vec::with_capacity(definition.arguments.len());
    for expected in &definition.arguments {
        let argument = call
            .arguments
            .iter()
            .find(|argument| argument.name.value() == &expected.name)
            .expect("message arity validation found every named argument");
        let (value, actual) = compile_argument_value(argument, scope, schemas)?;
        if !schema_type_matches(&expected.type_, &actual) {
            return Err(source_error(
                DiagnosticCode::MESSAGE_ARGUMENT,
                argument.span,
                format!(
                    "message argument `{}` expects `{}` but received `{}`",
                    expected.name,
                    schema_type_name(&expected.type_),
                    actual.display_name()
                ),
            ));
        }
        compiled.push((expected.name.clone(), value));
    }
    Ok(compiled)
}

fn schema_type_matches(schema: &SchemaType, actual: &ValueType) -> bool {
    match (schema, actual) {
        (SchemaType::Text, ValueType::Text)
        | (SchemaType::Number, ValueType::Number)
        | (SchemaType::Boolean, ValueType::Boolean) => true,
        (SchemaType::Enum(expected), ValueType::Enum { name, .. }) => expected == name,
        _ => false,
    }
}

fn schema_type_name(schema: &SchemaType) -> &str {
    match schema {
        SchemaType::Text => "text",
        SchemaType::Number => "number",
        SchemaType::Boolean => "boolean",
        SchemaType::Enum(name) => name,
    }
}

fn validate_message_manifest(manifest: &MessageManifest) -> MecoResult<()> {
    for (message_index, message) in manifest.messages.iter().enumerate() {
        if !valid_message_id(&message.id) {
            return Err(runtime_error(
                DiagnosticCode::MESSAGE_MANIFEST,
                format!(
                    "message ID `{}` must use lowercase ASCII letters, digits, and hyphens and begin with a letter",
                    message.id
                ),
            ));
        }
        if manifest.messages[..message_index]
            .iter()
            .any(|previous| previous.id == message.id)
        {
            return Err(runtime_error(
                DiagnosticCode::MESSAGE_MANIFEST,
                format!(
                    "formatter manifest contains duplicate message `{}`",
                    message.id
                ),
            ));
        }
        for (argument_index, argument) in message.arguments.iter().enumerate() {
            if !valid_value_name(&argument.name) {
                return Err(runtime_error(
                    DiagnosticCode::MESSAGE_MANIFEST,
                    format!(
                        "message `{}` has invalid argument name `{}`",
                        message.id, argument.name
                    ),
                ));
            }
            if message.arguments[..argument_index]
                .iter()
                .any(|previous| previous.name == argument.name)
            {
                return Err(runtime_error(
                    DiagnosticCode::MESSAGE_MANIFEST,
                    format!(
                        "message `{}` contains duplicate argument `{}`",
                        message.id, argument.name
                    ),
                ));
            }
            if let SchemaType::Enum(name) = &argument.type_ {
                if name.is_empty() {
                    return Err(runtime_error(
                        DiagnosticCode::MESSAGE_MANIFEST,
                        format!(
                            "message `{}` argument `{}` has an empty enum type",
                            message.id, argument.name
                        ),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_message_effects(rules: &mut [CompiledRule]) -> MecoResult<()> {
    let mut effects = vec![false; rules.len()];
    loop {
        let mut changed = false;
        for (index, rule) in rules.iter().enumerate() {
            if !effects[index]
                && !rule.productions.is_empty()
                && rule
                    .productions
                    .iter()
                    .all(|production| complete_message_production(production, &effects))
            {
                effects[index] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    for (rule_index, rule) in rules.iter().enumerate() {
        for production in &rule.productions {
            for binding in &production.bindings {
                if effects[binding.rule] {
                    return Err(source_error(
                        DiagnosticCode::MESSAGE_EFFECT,
                        rule.span,
                        format!(
                            "rule `{}` cannot silently bind complete-message rule `{}`",
                            rule.name, rules[binding.rule].name
                        ),
                    ));
                }
            }
            for part in &production.parts {
                match part {
                    CompiledPart::Capture { rule: target, .. } if effects[*target] => {
                        return Err(source_error(
                            DiagnosticCode::MESSAGE_EFFECT,
                            rule.span,
                            format!(
                                "rule `{}` cannot capture complete-message rule `{}`",
                                rule.name, rules[*target].name
                            ),
                        ));
                    }
                    CompiledPart::MessageCall { .. } | CompiledPart::RuleCall { .. }
                        if part_is_complete_message(part, &effects) && !effects[rule_index] =>
                    {
                        return Err(source_error(
                            DiagnosticCode::MESSAGE_EFFECT,
                            rule.span,
                            format!(
                                "rule `{}` mixes a complete message with ordinary text or bindings",
                                rule.name
                            ),
                        ));
                    }
                    _ => {}
                }
            }
        }
    }
    for (index, rule) in rules.iter_mut().enumerate() {
        rule.message_effect = effects[index];
    }
    Ok(())
}

fn complete_message_production(production: &CompiledProduction, effects: &[bool]) -> bool {
    production.parts.len() == 1 && part_is_complete_message(&production.parts[0], effects)
}

fn part_is_complete_message(part: &CompiledPart, effects: &[bool]) -> bool {
    match part {
        CompiledPart::MessageCall { .. } => true,
        CompiledPart::RuleCall { rule, .. } => effects[*rule],
        CompiledPart::Literal { .. }
        | CompiledPart::Value { .. }
        | CompiledPart::Capture { .. } => false,
    }
}

const PRODUCTION_ID_HASH_VERSION: &str = "production-fnv1a64/1";

pub(crate) fn stable_production_id(rule: &str, production: &ProductionSyntax) -> String {
    if let Some(authored) = &production.authored_id {
        return authored.value().clone();
    }
    let mut hash = StableHasher::new();
    hash.string(PRODUCTION_ID_HASH_VERSION);
    hash.string(rule);
    hash.production(production);
    format!("derived-{:016x}", hash.finish())
}

struct StableHasher(u64);

impl StableHasher {
    const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    const fn finish(self) -> u64 {
        self.0
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.0 = bytes.iter().fold(self.0, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    }

    fn tag(&mut self, tag: u8) {
        self.bytes(&[tag]);
    }

    fn string(&mut self, value: &str) {
        self.bytes(&u64::try_from(value.len()).unwrap_or(u64::MAX).to_le_bytes());
        self.bytes(value.as_bytes());
    }

    fn rational(&mut self, value: Rational) {
        self.bytes(&value.numerator().to_le_bytes());
        self.bytes(&value.denominator().to_le_bytes());
    }

    fn production(&mut self, production: &ProductionSyntax) {
        for clause in &production.clauses {
            match clause {
                ClauseSyntax::Guard(guard) => {
                    self.tag(1);
                    self.guard(guard.value());
                }
                ClauseSyntax::Binding(binding) => {
                    self.tag(2);
                    self.string(binding.rule.value());
                    self.arguments(&binding.arguments);
                    self.string(binding.name.value());
                }
            }
        }
        self.body(&production.body);
    }

    fn body(&mut self, body: &BodySyntax) {
        match body {
            BodySyntax::Empty(_) => self.tag(3),
            BodySyntax::Block(block) => {
                self.tag(4);
                self.tag(u8::from(block.raw));
                self.tag(match block.chomp {
                    crate::BlockChomp::Clip => 0,
                    crate::BlockChomp::Strip => 1,
                    crate::BlockChomp::Keep => 2,
                });
                self.string(block.text.value());
            }
            BodySyntax::Parts(parts) => {
                self.tag(5);
                self.parts(parts);
            }
        }
    }

    fn parts(&mut self, parts: &[BodyPartSyntax]) {
        self.bytes(&u64::try_from(parts.len()).unwrap_or(u64::MAX).to_le_bytes());
        for part in parts {
            match part {
                BodyPartSyntax::Literal(value) => {
                    self.tag(6);
                    self.string(value.value());
                }
                BodyPartSyntax::RuleReference(value) => {
                    self.tag(7);
                    self.string(value.value());
                }
                BodyPartSyntax::EmittingCapture { rule, name, .. } => {
                    self.tag(8);
                    self.string(rule.value());
                    self.string(name.value());
                }
                BodyPartSyntax::ValueReference(value) => {
                    self.tag(9);
                    self.string(value.value());
                }
                BodyPartSyntax::RuleCall(call) => {
                    self.tag(10);
                    self.string(call.target.value());
                    self.arguments(&call.arguments);
                }
                BodyPartSyntax::MessageCall(call) => {
                    self.tag(11);
                    self.string(call.target.value());
                    self.arguments(&call.arguments);
                }
            }
        }
    }

    fn arguments(&mut self, arguments: &[ArgumentSyntax]) {
        self.bytes(
            &u64::try_from(arguments.len())
                .unwrap_or(u64::MAX)
                .to_le_bytes(),
        );
        for argument in arguments {
            self.string(argument.name.value());
            self.value(&argument.value);
        }
    }

    fn value(&mut self, value: &ValueSyntax) {
        match value {
            ValueSyntax::Reference(value) => {
                self.tag(12);
                self.string(value.value());
            }
            ValueSyntax::Number(value) => {
                self.tag(13);
                self.rational(*value.value());
            }
            ValueSyntax::Text(value) => {
                self.tag(14);
                self.string(value.value());
            }
            ValueSyntax::Boolean(value) => {
                self.tag(15);
                self.tag(u8::from(*value.value()));
            }
        }
    }

    fn guard(&mut self, guard: &GuardExpression) {
        match guard {
            GuardExpression::Value(value) => {
                self.tag(16);
                self.guard_value(value);
            }
            GuardExpression::Is(left, right) => self.guard_pair(17, left, right),
            GuardExpression::IsNot(left, right) => self.guard_pair(18, left, right),
            GuardExpression::Less(left, right) => self.guard_pair(19, left, right),
            GuardExpression::LessOrEqual(left, right) => self.guard_pair(20, left, right),
            GuardExpression::Greater(left, right) => self.guard_pair(21, left, right),
            GuardExpression::GreaterOrEqual(left, right) => self.guard_pair(22, left, right),
            GuardExpression::Not(value) => {
                self.tag(23);
                self.guard(value);
            }
            GuardExpression::And(left, right) => {
                self.tag(24);
                self.guard(left);
                self.guard(right);
            }
            GuardExpression::Or(left, right) => {
                self.tag(25);
                self.guard(left);
                self.guard(right);
            }
        }
    }

    fn guard_pair(&mut self, tag: u8, left: &GuardValue, right: &GuardValue) {
        self.tag(tag);
        self.guard_value(left);
        self.guard_value(right);
    }

    fn guard_value(&mut self, value: &GuardValue) {
        match value {
            GuardValue::Name(value) => {
                self.tag(26);
                self.string(value);
            }
            GuardValue::Number(value) => {
                self.tag(27);
                self.rational(*value);
            }
            GuardValue::Boolean(value) => {
                self.tag(28);
                self.tag(u8::from(*value));
            }
            GuardValue::Text(value) => {
                self.tag(29);
                self.string(value);
            }
        }
    }
}

fn valid_message_id(id: &str) -> bool {
    let mut bytes = id.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn valid_value_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn compile_argument_value(
    argument: &ArgumentSyntax,
    scope: &CompileScope,
    schemas: &[ModuleSchema],
) -> MecoResult<(CompiledValue, ValueType)> {
    match &argument.value {
        ValueSyntax::Reference(reference) => lookup_value(scope, schemas, reference.value())
            .ok_or_else(|| {
                source_error(
                    DiagnosticCode::VALUE_NAME,
                    reference.span(),
                    format!("undefined value `{}`", reference.value()),
                )
            }),
        ValueSyntax::Number(number) => Ok((
            CompiledValue::Constant(Value::Number(*number.value())),
            ValueType::Number,
        )),
        ValueSyntax::Text(text) => Ok((
            CompiledValue::Constant(Value::Text(text.value().clone())),
            ValueType::Text,
        )),
        ValueSyntax::Boolean(boolean) => Ok((
            CompiledValue::Constant(Value::Boolean(*boolean.value())),
            ValueType::Boolean,
        )),
    }
}

fn validate_bound_value_usage(
    span: Span,
    bindings: &[CompiledBinding],
    parts: &[CompiledPart],
) -> MecoResult<()> {
    let mut used = Vec::new();
    for binding in bindings {
        for argument in &binding.arguments {
            collect_local_value(argument, &mut used);
        }
    }
    for part in parts {
        match part {
            CompiledPart::Value { value, .. } => collect_local_value(value, &mut used),
            CompiledPart::RuleCall { arguments, .. } => {
                for argument in arguments {
                    collect_local_value(argument, &mut used);
                }
            }
            CompiledPart::MessageCall { arguments, .. } => {
                for (_, argument) in arguments {
                    collect_local_value(argument, &mut used);
                }
            }
            CompiledPart::Literal { .. } | CompiledPart::Capture { .. } => {}
        }
    }
    for (slot, name) in bindings
        .iter()
        .map(|binding| (binding.slot, binding.name.as_str()))
        .chain(parts.iter().filter_map(|part| match part {
            CompiledPart::Capture { slot, name, .. } => Some((*slot, name.as_str())),
            _ => None,
        }))
    {
        if !used.contains(&slot) {
            return Err(source_error(
                DiagnosticCode::BINDING_NAME,
                span,
                format!("bound value `{name}` is never used"),
            ));
        }
    }
    Ok(())
}

fn collect_local_value(value: &CompiledValue, used: &mut Vec<usize>) {
    if let CompiledValue::Local(slot) = value {
        used.push(*slot);
    }
}

fn compile_weight(
    syntax: &WeightSyntax,
    scope: &CompileScope,
    schemas: &[ModuleSchema],
) -> MecoResult<CompiledWeight> {
    match syntax {
        WeightSyntax::Default => Ok(CompiledWeight::Static(Rational::ONE)),
        WeightSyntax::Static(value) => Ok(CompiledWeight::Static(*value.value())),
        WeightSyntax::Dynamic(expression) => Ok(CompiledWeight::Dynamic(
            compile_weight_expression(expression.value(), scope, schemas, expression.span())?,
        )),
    }
}

fn compile_weight_expression(
    expression: &WeightExpression,
    scope: &CompileScope,
    schemas: &[ModuleSchema],
    span: Span,
) -> MecoResult<CompiledWeightExpression> {
    match expression {
        WeightExpression::Literal(value) => Ok(CompiledWeightExpression::Literal(*value)),
        WeightExpression::Name(name) => {
            let (value, type_) = lookup_value(scope, schemas, name).ok_or_else(|| {
                source_error(
                    DiagnosticCode::VALUE_NAME,
                    span,
                    format!("undefined dynamic-weight value `{name}`"),
                )
            })?;
            if type_ != ValueType::Number {
                return Err(source_error(
                    DiagnosticCode::TYPE_MISMATCH,
                    span,
                    format!(
                        "dynamic weight `{name}` must be `number`, not `{}`",
                        type_.display_name()
                    ),
                ));
            }
            Ok(CompiledWeightExpression::Value(value))
        }
        WeightExpression::Add(left, right) => Ok(CompiledWeightExpression::Add(
            Box::new(compile_weight_expression(left, scope, schemas, span)?),
            Box::new(compile_weight_expression(right, scope, schemas, span)?),
        )),
        WeightExpression::Subtract(left, right) => Ok(CompiledWeightExpression::Subtract(
            Box::new(compile_weight_expression(left, scope, schemas, span)?),
            Box::new(compile_weight_expression(right, scope, schemas, span)?),
        )),
        WeightExpression::Multiply(left, right) => Ok(CompiledWeightExpression::Multiply(
            Box::new(compile_weight_expression(left, scope, schemas, span)?),
            Box::new(compile_weight_expression(right, scope, schemas, span)?),
        )),
    }
}

fn compile_guards(
    clauses: &[ClauseSyntax],
    scope: &CompileScope,
    schemas: &[ModuleSchema],
) -> MecoResult<Option<CompiledGuard>> {
    let mut combined = None;
    for clause in clauses {
        let ClauseSyntax::Guard(guard) = clause else {
            continue;
        };
        let next = compile_guard(guard.value(), scope, schemas, guard.span())?;
        combined = Some(match combined {
            None => next,
            Some(previous) => CompiledGuard::And(Box::new(previous), Box::new(next)),
        });
    }
    Ok(combined)
}

fn compile_guard(
    expression: &GuardExpression,
    scope: &CompileScope,
    schemas: &[ModuleSchema],
    span: Span,
) -> MecoResult<CompiledGuard> {
    match expression {
        GuardExpression::Value(value) => {
            let (value, type_) = compile_known_guard_value(value, scope, schemas, span)?;
            if type_ != ValueType::Boolean {
                return Err(source_error(
                    DiagnosticCode::TYPE_MISMATCH,
                    span,
                    "a standalone guard value must be boolean",
                ));
            }
            Ok(CompiledGuard::Value(value))
        }
        GuardExpression::Is(left, right) => {
            let (left, right, _) = compile_comparison_values(left, right, scope, schemas, span)?;
            Ok(CompiledGuard::Is(left, right))
        }
        GuardExpression::IsNot(left, right) => {
            let (left, right, _) = compile_comparison_values(left, right, scope, schemas, span)?;
            Ok(CompiledGuard::IsNot(left, right))
        }
        GuardExpression::Less(left, right) => {
            let (left, right) = compile_ordered_values(left, right, scope, schemas, span)?;
            Ok(CompiledGuard::Less(left, right))
        }
        GuardExpression::LessOrEqual(left, right) => {
            let (left, right) = compile_ordered_values(left, right, scope, schemas, span)?;
            Ok(CompiledGuard::LessOrEqual(left, right))
        }
        GuardExpression::Greater(left, right) => {
            let (left, right) = compile_ordered_values(left, right, scope, schemas, span)?;
            Ok(CompiledGuard::Greater(left, right))
        }
        GuardExpression::GreaterOrEqual(left, right) => {
            let (left, right) = compile_ordered_values(left, right, scope, schemas, span)?;
            Ok(CompiledGuard::GreaterOrEqual(left, right))
        }
        GuardExpression::Not(value) => Ok(CompiledGuard::Not(Box::new(compile_guard(
            value, scope, schemas, span,
        )?))),
        GuardExpression::And(left, right) => Ok(CompiledGuard::And(
            Box::new(compile_guard(left, scope, schemas, span)?),
            Box::new(compile_guard(right, scope, schemas, span)?),
        )),
        GuardExpression::Or(left, right) => Ok(CompiledGuard::Or(
            Box::new(compile_guard(left, scope, schemas, span)?),
            Box::new(compile_guard(right, scope, schemas, span)?),
        )),
    }
}

fn compile_ordered_values(
    left: &GuardValue,
    right: &GuardValue,
    scope: &CompileScope,
    schemas: &[ModuleSchema],
    span: Span,
) -> MecoResult<(CompiledGuardValue, CompiledGuardValue)> {
    let (left, right, type_) = compile_comparison_values(left, right, scope, schemas, span)?;
    if type_ != ValueType::Number {
        return Err(source_error(
            DiagnosticCode::TYPE_MISMATCH,
            span,
            format!("ordering requires numbers, not `{}`", type_.display_name()),
        ));
    }
    Ok((left, right))
}

fn compile_comparison_values(
    left: &GuardValue,
    right: &GuardValue,
    scope: &CompileScope,
    schemas: &[ModuleSchema],
    span: Span,
) -> MecoResult<(CompiledGuardValue, CompiledGuardValue, ValueType)> {
    let left_known = try_compile_known_guard_value(left, scope, schemas);
    let right_known = try_compile_known_guard_value(right, scope, schemas);
    let (left_value, left_type, right_value, right_type) = match (left_known, right_known) {
        (Some((left_value, left_type)), Some((right_value, right_type))) => {
            (left_value, left_type, right_value, right_type)
        }
        (Some((left_value, left_type)), None) => {
            let (right_value, right_type) = compile_enum_variant(right, &left_type, span)?;
            (left_value, left_type, right_value, right_type)
        }
        (None, Some((right_value, right_type))) => {
            let (left_value, left_type) = compile_enum_variant(left, &right_type, span)?;
            (left_value, left_type, right_value, right_type)
        }
        (None, None) => {
            return Err(source_error(
                DiagnosticCode::VALUE_NAME,
                span,
                "guard comparison contains no declared value",
            ));
        }
    };
    if left_type != right_type {
        return Err(source_error(
            DiagnosticCode::TYPE_MISMATCH,
            span,
            format!(
                "guard compares `{}` with `{}`",
                left_type.display_name(),
                right_type.display_name()
            ),
        ));
    }
    Ok((left_value, right_value, left_type))
}

fn compile_known_guard_value(
    value: &GuardValue,
    scope: &CompileScope,
    schemas: &[ModuleSchema],
    span: Span,
) -> MecoResult<(CompiledGuardValue, ValueType)> {
    try_compile_known_guard_value(value, scope, schemas).ok_or_else(|| {
        let GuardValue::Name(name) = value else {
            unreachable!("all non-name guard values are known")
        };
        source_error(
            DiagnosticCode::VALUE_NAME,
            span,
            format!("undefined guard value `{name}`"),
        )
    })
}

fn try_compile_known_guard_value(
    value: &GuardValue,
    scope: &CompileScope,
    schemas: &[ModuleSchema],
) -> Option<(CompiledGuardValue, ValueType)> {
    match value {
        GuardValue::Name(name) => lookup_value(scope, schemas, name)
            .map(|(value, type_)| (CompiledGuardValue::Value(value), type_)),
        GuardValue::Number(value) => Some((
            CompiledGuardValue::Constant(Value::Number(*value)),
            ValueType::Number,
        )),
        GuardValue::Boolean(value) => Some((
            CompiledGuardValue::Constant(Value::Boolean(*value)),
            ValueType::Boolean,
        )),
        GuardValue::Text(value) => Some((
            CompiledGuardValue::Constant(Value::Text(value.clone())),
            ValueType::Text,
        )),
    }
}

fn compile_enum_variant(
    value: &GuardValue,
    expected: &ValueType,
    span: Span,
) -> MecoResult<(CompiledGuardValue, ValueType)> {
    let GuardValue::Name(variant) = value else {
        return Err(source_error(
            DiagnosticCode::TYPE_MISMATCH,
            span,
            "guard values have incompatible types",
        ));
    };
    let ValueType::Enum { variants, .. } = expected else {
        return Err(source_error(
            DiagnosticCode::VALUE_NAME,
            span,
            format!("undefined guard value `{variant}`"),
        ));
    };
    if !variants.iter().any(|candidate| candidate == variant) {
        return Err(source_error(
            DiagnosticCode::TYPE_MISMATCH,
            span,
            format!(
                "`{variant}` is not a member of `{}`",
                expected.display_name()
            ),
        ));
    }
    Ok((
        CompiledGuardValue::Constant(Value::Enum(variant.clone())),
        expected.clone(),
    ))
}

fn resolve_rule(
    package: &PackageInput,
    modules: &[ModuleBuild<'_>],
    offsets: &[usize],
    module: usize,
    authored: &str,
    authored_span: Span,
) -> MecoResult<usize> {
    let (target_module, rule_name, imported) = if let Some((alias, rule)) = authored.split_once('.')
    {
        let import = modules[module]
            .syntax
            .front_matter
            .imports()
            .iter()
            .find(|import| import.alias().value() == alias)
            .ok_or_else(|| {
                source_error(
                    DiagnosticCode::UNDEFINED_RULE,
                    authored_span,
                    format!("unknown import alias `{alias}`"),
                )
            })?;
        let resolution = package.modules[module]
            .resolved_imports
            .iter()
            .find(|resolution| resolution.authored_path == *import.path().value())
            .expect("package imports were validated");
        let target = modules
            .iter()
            .position(|candidate| candidate.canonical_id == resolution.target_id)
            .expect("package imports target supplied modules");
        (target, rule, true)
    } else {
        (module, authored, false)
    };
    let local = modules[target_module]
        .syntax
        .rules
        .iter()
        .position(|rule| rule.name.value() == rule_name)
        .ok_or_else(|| {
            source_error(
                DiagnosticCode::UNDEFINED_RULE,
                authored_span,
                format!("undefined rule `{authored}`"),
            )
        })?;
    if imported
        && !modules[target_module]
            .syntax
            .front_matter
            .exports()
            .iter()
            .any(|export| export.value() == rule_name)
    {
        return Err(source_error(
            DiagnosticCode::RULE_VISIBILITY,
            authored_span,
            format!("imported rule `{authored}` is private"),
        ));
    }
    Ok(offsets[target_module] + local)
}

fn prepare_static_selection(
    span: Span,
    productions: &[CompiledProduction],
) -> MecoResult<Option<StaticSelection>> {
    let Some(rationals) = productions
        .iter()
        .map(|production| match production.weight {
            CompiledWeight::Static(value) => Some(value),
            CompiledWeight::Dynamic(_) => None,
        })
        .collect::<Option<Vec<_>>>()
    else {
        return Ok(None);
    };
    let normalized = normalize_rationals(&rationals, Some(span))?;
    if productions
        .iter()
        .any(|production| production.guard.is_some())
    {
        return Ok(None);
    }
    let mut total = 0_u64;
    let mut cumulative = Vec::with_capacity(normalized.len());
    for weight in normalized {
        total = total
            .checked_add(weight)
            .ok_or_else(|| weight_overflow_at(Some(span)))?;
        cumulative.push(total);
    }
    Ok(Some(StaticSelection { cumulative, total }))
}

fn normalize_rationals(rationals: &[Rational], source_span: Option<Span>) -> MecoResult<Vec<u64>> {
    let mut common_denominator = 1_u128;
    for rational in rationals {
        let denominator = u128::from(rational.denominator());
        let divisor = gcd(common_denominator, denominator);
        common_denominator = common_denominator
            .checked_div(divisor)
            .and_then(|value| value.checked_mul(denominator))
            .ok_or_else(|| weight_overflow_at(source_span))?;
    }
    let mut scaled = Vec::with_capacity(rationals.len());
    for rational in rationals {
        let numerator =
            u128::try_from(rational.numerator()).expect("eligible weights are positive");
        scaled.push(
            numerator
                .checked_mul(common_denominator / u128::from(rational.denominator()))
                .ok_or_else(|| weight_overflow_at(source_span))?,
        );
    }
    let divisor = scaled.iter().copied().reduce(gcd).unwrap_or(1);
    let mut total = 0_u128;
    let mut normalized = Vec::with_capacity(scaled.len());
    for value in scaled {
        let value = value / divisor;
        total = total
            .checked_add(value)
            .ok_or_else(|| weight_overflow_at(source_span))?;
        normalized.push(value);
    }
    if total > i64::MAX as u128 {
        return Err(weight_overflow_at(source_span));
    }
    normalized
        .into_iter()
        .map(|value| u64::try_from(value).map_err(|_| weight_overflow_at(source_span)))
        .collect()
}

fn weight_overflow_at(span: Option<Span>) -> MecoError {
    let message = "a rule's scaled eligible weight total exceeds 2^63 - 1";
    span.map_or_else(
        || runtime_error(DiagnosticCode::WEIGHT_OVERFLOW, message),
        |span| source_error(DiagnosticCode::WEIGHT_OVERFLOW, span, message),
    )
}

fn weight_runtime_overflow(message: &str) -> MecoError {
    runtime_error(DiagnosticCode::WEIGHT_OVERFLOW, message)
}

const fn gcd(mut left: u128, mut right: u128) -> u128 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EligibleProduction {
    production: usize,
    base: Rational,
    normalized: u64,
}

fn select_eligible_production(productions: &[EligibleProduction], choice: u64) -> usize {
    let mut cursor = choice;
    for production in productions {
        if cursor < production.normalized {
            return production.production;
        }
        cursor -= production.normalized;
    }
    unreachable!("choice is strictly below the checked weight total")
}

fn eligible_weights(
    rule: &CompiledRule,
    inputs: &[Value],
    locals: &[Value],
) -> MecoResult<Vec<EligibleProduction>> {
    let mut indexes = Vec::new();
    let mut rationals = Vec::new();
    for (index, production) in rule.productions.iter().enumerate() {
        if let Some(guard) = &production.guard {
            if !evaluate_guard(guard, inputs, locals)? {
                continue;
            }
        }
        let weight = evaluate_weight(&production.weight, inputs, locals)?;
        if weight.numerator() < 0 {
            return Err(runtime_error(
                DiagnosticCode::WEIGHT_VALUE,
                format!("dynamic weight in `{}` evaluated to {weight}", rule.name),
            ));
        }
        if weight.is_zero() {
            continue;
        }
        indexes.push(index);
        rationals.push(weight);
    }
    if indexes.is_empty() {
        return Err(runtime_error(
            DiagnosticCode::NO_ELIGIBLE_PRODUCTION,
            format!(
                "rule `{}` has no eligible positive-weight production",
                rule.name
            ),
        ));
    }
    let weights = normalize_rationals(&rationals, None)?;
    Ok(indexes
        .into_iter()
        .zip(rationals)
        .zip(weights)
        .map(|((production, base), normalized)| EligibleProduction {
            production,
            base,
            normalized,
        })
        .collect())
}

fn diverse_eligible_weights(
    rule: &CompiledRule,
    mut eligible: Vec<EligibleProduction>,
    state: &DiverseCandidateState<'_>,
) -> MecoResult<Vec<EligibleProduction>> {
    if rule.analysis.nullable || rule.analysis.recursive {
        return Ok(eligible);
    }
    let profile = crate::LocationProfile::V1;
    let recent = state.recent(&rule.name);
    let gap = usize::try_from(profile.hard_minimum_gap).unwrap_or(usize::MAX);
    let cooled = recent.iter().rev().take(gap).copied().collect::<Vec<_>>();
    let available = eligible
        .iter()
        .filter(|candidate| !cooled.contains(&rule.productions[candidate.production].id.as_str()))
        .map(|candidate| candidate.production)
        .collect::<Vec<_>>();
    if available.is_empty() {
        let relaxed = eligible
            .iter()
            .max_by_key(|candidate| {
                (
                    state
                        .selection_age(&rule.name, &rule.productions[candidate.production].id)
                        .unwrap_or(u32::MAX),
                    core::cmp::Reverse(rule.productions[candidate.production].id.as_str()),
                )
            })
            .map(|candidate| candidate.production)
            .expect("eligible candidates are nonempty");
        eligible.retain(|candidate| candidate.production == relaxed);
    } else {
        eligible.retain(|candidate| available.contains(&candidate.production));
    }

    let mut effective = Vec::with_capacity(eligible.len());
    for candidate in &eligible {
        let production = &rule.productions[candidate.production];
        let diversity = Rational::new(i64::from(production.diversity_factor_16_16), 1 << 16)
            .map_err(|_| weight_runtime_overflow("diversity factor exceeds rational budget"))?;
        let cooldown = state
            .selection_age(&rule.name, &production.id)
            .map_or(Ok(Rational::ONE), location_cooldown_multiplier)
            .map_err(|_| weight_runtime_overflow("cooldown factor exceeds rational budget"))?;
        effective.push(
            candidate
                .base
                .checked_mul(diversity)
                .and_then(|value| value.checked_mul(cooldown))
                .map_err(|_| weight_runtime_overflow("diverse effective weight overflowed"))?,
        );
    }
    let normalized = normalize_rationals(&effective, None)?;
    for (candidate, weight) in eligible.iter_mut().zip(normalized) {
        candidate.normalized = weight;
    }
    Ok(eligible)
}

fn prepare_diversity_metadata(rules: &mut [CompiledRule]) {
    let production_counts = rules
        .iter()
        .map(|rule| u64::try_from(rule.productions.len()).unwrap_or(u64::MAX))
        .collect::<Vec<_>>();
    for rule in rules {
        for production in &mut rule.productions {
            let descendants = production
                .bindings
                .iter()
                .map(|binding| production_counts[binding.rule])
                .chain(
                    production
                        .parts
                        .iter()
                        .filter_map(part_rule)
                        .map(|child| production_counts[child]),
                )
                .fold(1_u64, u64::saturating_add);
            production.diversity_factor_16_16 = diversity_factor_16_16(descendants);
        }
    }
}

fn evaluate_weight(
    weight: &CompiledWeight,
    inputs: &[Value],
    locals: &[Value],
) -> MecoResult<Rational> {
    match weight {
        CompiledWeight::Static(value) => Ok(*value),
        CompiledWeight::Dynamic(expression) => {
            evaluate_weight_expression(expression, inputs, locals)
        }
    }
}

fn evaluate_weight_expression(
    expression: &CompiledWeightExpression,
    inputs: &[Value],
    locals: &[Value],
) -> MecoResult<Rational> {
    match expression {
        CompiledWeightExpression::Literal(value) => Ok(*value),
        CompiledWeightExpression::Value(value) => {
            let Value::Number(value) = runtime_value(value, inputs, locals)? else {
                return Err(runtime_error(
                    DiagnosticCode::TYPE_MISMATCH,
                    "dynamic weight encountered a non-number value",
                ));
            };
            Ok(value)
        }
        CompiledWeightExpression::Add(left, right) => {
            evaluate_weight_expression(left, inputs, locals)?
                .checked_add(evaluate_weight_expression(right, inputs, locals)?)
                .map_err(|_| weight_runtime_overflow("dynamic weight addition overflowed"))
        }
        CompiledWeightExpression::Subtract(left, right) => {
            evaluate_weight_expression(left, inputs, locals)?
                .checked_sub(evaluate_weight_expression(right, inputs, locals)?)
                .map_err(|_| weight_runtime_overflow("dynamic weight subtraction overflowed"))
        }
        CompiledWeightExpression::Multiply(left, right) => {
            evaluate_weight_expression(left, inputs, locals)?
                .checked_mul(evaluate_weight_expression(right, inputs, locals)?)
                .map_err(|_| weight_runtime_overflow("dynamic weight multiplication overflowed"))
        }
    }
}

fn evaluate_guard(guard: &CompiledGuard, inputs: &[Value], locals: &[Value]) -> MecoResult<bool> {
    match guard {
        CompiledGuard::Value(value) => {
            let Value::Boolean(value) = evaluate_guard_value(value, inputs, locals)? else {
                return Err(runtime_error(
                    DiagnosticCode::TYPE_MISMATCH,
                    "guard encountered a non-boolean value",
                ));
            };
            Ok(value)
        }
        CompiledGuard::Is(left, right) => Ok(evaluate_guard_value(left, inputs, locals)?
            == evaluate_guard_value(right, inputs, locals)?),
        CompiledGuard::IsNot(left, right) => Ok(evaluate_guard_value(left, inputs, locals)?
            != evaluate_guard_value(right, inputs, locals)?),
        CompiledGuard::Less(left, right) => {
            compare_guard_numbers(left, right, inputs, locals).map(core::cmp::Ordering::is_lt)
        }
        CompiledGuard::LessOrEqual(left, right) => {
            compare_guard_numbers(left, right, inputs, locals).map(core::cmp::Ordering::is_le)
        }
        CompiledGuard::Greater(left, right) => {
            compare_guard_numbers(left, right, inputs, locals).map(core::cmp::Ordering::is_gt)
        }
        CompiledGuard::GreaterOrEqual(left, right) => {
            compare_guard_numbers(left, right, inputs, locals).map(core::cmp::Ordering::is_ge)
        }
        CompiledGuard::Not(value) => Ok(!evaluate_guard(value, inputs, locals)?),
        CompiledGuard::And(left, right) => {
            Ok(evaluate_guard(left, inputs, locals)? && evaluate_guard(right, inputs, locals)?)
        }
        CompiledGuard::Or(left, right) => {
            Ok(evaluate_guard(left, inputs, locals)? || evaluate_guard(right, inputs, locals)?)
        }
    }
}

fn evaluate_guard_value(
    value: &CompiledGuardValue,
    inputs: &[Value],
    locals: &[Value],
) -> MecoResult<Value> {
    match value {
        CompiledGuardValue::Value(value) => runtime_value(value, inputs, locals),
        CompiledGuardValue::Constant(value) => Ok(value.clone()),
    }
}

fn compare_guard_numbers(
    left: &CompiledGuardValue,
    right: &CompiledGuardValue,
    inputs: &[Value],
    locals: &[Value],
) -> MecoResult<core::cmp::Ordering> {
    let Value::Number(left) = evaluate_guard_value(left, inputs, locals)? else {
        return Err(runtime_error(
            DiagnosticCode::TYPE_MISMATCH,
            "ordered guard is not numeric",
        ));
    };
    let Value::Number(right) = evaluate_guard_value(right, inputs, locals)? else {
        return Err(runtime_error(
            DiagnosticCode::TYPE_MISMATCH,
            "ordered guard is not numeric",
        ));
    };
    let left_scaled = i128::from(left.numerator()) * i128::from(right.denominator());
    let right_scaled = i128::from(right.numerator()) * i128::from(left.denominator());
    Ok(left_scaled.cmp(&right_scaled))
}

fn runtime_value(value: &CompiledValue, inputs: &[Value], locals: &[Value]) -> MecoResult<Value> {
    match value {
        CompiledValue::Input(index) => inputs.get(*index).cloned(),
        CompiledValue::Local(index) => locals.get(*index).cloned(),
        CompiledValue::Constant(value) => return Ok(value.clone()),
    }
    .ok_or_else(|| {
        runtime_error(
            DiagnosticCode::VALUE_NAME,
            "runtime value slot is unavailable",
        )
    })
}

fn evaluate_arguments(
    arguments: &[CompiledValue],
    inputs: &[Value],
    locals: &[Value],
) -> MecoResult<Vec<Value>> {
    arguments
        .iter()
        .map(|value| runtime_value(value, inputs, locals))
        .collect()
}

fn evaluate_argument_origins(
    arguments: &[CompiledValue],
    locals: &[ValueOrigin],
) -> Vec<ValueOrigin> {
    arguments
        .iter()
        .map(|value| compiled_value_origin(value, locals))
        .collect()
}

fn compiled_value_origin(value: &CompiledValue, locals: &[ValueOrigin]) -> ValueOrigin {
    match value {
        CompiledValue::Input(_) => ValueOrigin::Host,
        CompiledValue::Local(slot) => locals.get(*slot).copied().unwrap_or(ValueOrigin::Constant),
        CompiledValue::Constant(_) => ValueOrigin::Constant,
    }
}

fn bind_runtime_value(
    frame: &mut RuntimeFrame,
    slot: usize,
    value: Value,
    origin: ValueOrigin,
) -> MecoResult<()> {
    if frame.values.len() != slot || frame.origins.len() != slot {
        return Err(runtime_error(
            DiagnosticCode::BINDING_NAME,
            "binding slot order is inconsistent with the compiled frame",
        ));
    }
    frame.values.push(value);
    frame.origins.push(origin);
    Ok(())
}

fn validate_runtime_type(value: &Value, expected: &ValueType) -> Result<(), String> {
    let valid = match (value, expected) {
        (Value::Text(_), ValueType::Text)
        | (Value::Number(_), ValueType::Number)
        | (Value::Boolean(_), ValueType::Boolean) => true,
        (Value::Enum(value), ValueType::Enum { variants, .. }) => {
            variants.iter().any(|variant| variant == value)
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(format!(
            "expects `{}` but received `{}`",
            expected.display_name(),
            value.kind_name()
        ))
    }
}

fn output_cursor(output: &str) -> (usize, u64) {
    (output.len(), scalar_len(output))
}

fn scalar_len(output: &str) -> u64 {
    u64::try_from(output.chars().count()).unwrap_or(u64::MAX)
}

#[allow(clippy::too_many_arguments)]
fn push_provenance(
    nodes: &mut Vec<ProvenanceNode>,
    enabled: bool,
    parent: Option<u32>,
    kind: ProvenanceKind,
    rule: &CompiledRule,
    production: usize,
    source_span: Span,
    depth: u32,
    name: Option<String>,
) -> Option<u32> {
    if !enabled {
        return None;
    }
    let id = u32::try_from(nodes.len()).unwrap_or(u32::MAX);
    nodes.push(ProvenanceNode::new(
        id,
        parent,
        kind,
        rule.name.clone(),
        rule.productions[production].id.clone(),
        source_span,
        None,
        depth,
        name,
    ));
    Some(id)
}

#[allow(clippy::too_many_arguments)]
fn record_emission(
    nodes: &mut Vec<ProvenanceNode>,
    enabled: bool,
    parent: Option<u32>,
    kind: ProvenanceKind,
    rule: &CompiledRule,
    production: usize,
    source_span: Span,
    sink: usize,
    start: (usize, u64),
    output: &str,
    depth: u32,
    name: Option<String>,
) {
    let node = push_provenance(
        nodes,
        enabled,
        parent,
        kind,
        rule,
        production,
        source_span,
        depth,
        name,
    );
    finish_provenance(nodes, node, sink, start.0, start.1, output);
}

fn finish_provenance(
    nodes: &mut [ProvenanceNode],
    node: Option<u32>,
    sink: usize,
    start: usize,
    start_scalar: u64,
    output: &str,
) {
    let Some(node) = node.and_then(|value| usize::try_from(value).ok()) else {
        return;
    };
    let range = (sink == 0).then(|| {
        OutputRange::new(
            u64::try_from(start).unwrap_or(u64::MAX),
            u64::try_from(output.len()).unwrap_or(u64::MAX),
            start_scalar,
            scalar_len(output),
        )
    });
    if let Some(node) = nodes.get_mut(node) {
        node.set_output(range);
    }
}

fn finalize_formatted_provenance(nodes: &mut [ProvenanceNode], text: &str) {
    let full = OutputRange::new(
        0,
        u64::try_from(text.len()).unwrap_or(u64::MAX),
        0,
        scalar_len(text),
    );
    for node in nodes {
        let formatted_ancestor = node.kind() == ProvenanceKind::Production
            && node.output().is_some_and(OutputRange::is_empty);
        if node.kind() == ProvenanceKind::Message || formatted_ancestor {
            node.set_output(Some(full));
        }
    }
}

fn append_output(
    output: &mut String,
    text: &str,
    output_scalars: &mut u32,
    limits: GenerationLimits,
) -> MecoResult<()> {
    let added = u32::try_from(text.chars().count()).map_err(|_| {
        runtime_error(
            DiagnosticCode::LIMIT_OUTPUT,
            "one literal exceeds the output scalar counter",
        )
    })?;
    *output_scalars = output_scalars.checked_add(added).ok_or_else(|| {
        runtime_error(
            DiagnosticCode::LIMIT_OUTPUT,
            "generated output exceeds the scalar counter",
        )
    })?;
    if *output_scalars > limits.max_output_scalars {
        return Err(runtime_error(
            DiagnosticCode::LIMIT_OUTPUT,
            format!(
                "generated output exceeds {} Unicode scalars",
                limits.max_output_scalars
            ),
        ));
    }
    let next_bytes = output.len().checked_add(text.len()).ok_or_else(|| {
        runtime_error(
            DiagnosticCode::LIMIT_OUTPUT,
            "generated output exceeds the byte counter",
        )
    })?;
    if u32::try_from(next_bytes).map_or(true, |bytes| bytes > limits.max_output_bytes) {
        return Err(runtime_error(
            DiagnosticCode::LIMIT_OUTPUT,
            format!(
                "generated output exceeds {} UTF-8 bytes",
                limits.max_output_bytes
            ),
        ));
    }
    output.push_str(text);
    Ok(())
}

fn validate_locale_request(locale: LocaleRequest<'_>) -> MecoResult<()> {
    if !valid_locale(locale.requested) {
        return Err(runtime_error(
            DiagnosticCode::LOCALE,
            format!("invalid requested locale `{}`", locale.requested),
        ));
    }
    for (index, fallback) in locale.fallbacks.iter().enumerate() {
        if !valid_locale(fallback) {
            return Err(runtime_error(
                DiagnosticCode::LOCALE,
                format!("invalid fallback locale `{fallback}`"),
            ));
        }
        if *fallback == locale.requested || locale.fallbacks[..index].contains(fallback) {
            return Err(runtime_error(
                DiagnosticCode::LOCALE,
                format!("locale chain contains duplicate `{fallback}`"),
            ));
        }
    }
    Ok(())
}

fn valid_locale(locale: &str) -> bool {
    !locale.is_empty()
        && !locale.starts_with('-')
        && !locale.ends_with('-')
        && !locale.contains("--")
        && locale
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn validate_formatter_result(
    request: &FormatterRequest,
    result: &crate::FormatterResult,
    limits: GenerationLimits,
) -> MecoResult<()> {
    if result.actual_locale != request.requested_locale()
        && !request
            .fallback_locales()
            .iter()
            .any(|fallback| fallback == &result.actual_locale)
    {
        return Err(runtime_error(
            DiagnosticCode::LOCALE,
            format!(
                "formatter returned locale `{}` outside the requested fallback chain",
                result.actual_locale
            ),
        ));
    }
    if result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity() == Severity::Error)
    {
        return Err(MecoError::with_related(
            Diagnostic::new(
                DiagnosticCode::FORMATTER,
                Severity::Error,
                None,
                "formatter reported a fatal diagnostic",
            ),
            result.diagnostics.clone(),
        ));
    }
    if result.work_units > 10_000 {
        return Err(runtime_error(
            DiagnosticCode::FORMATTER_LIMIT,
            format!(
                "formatter work units exceed 10000 (reported {})",
                result.work_units
            ),
        ));
    }
    if result.replayable && result.environment_hash.is_empty() {
        return Err(runtime_error(
            DiagnosticCode::FORMATTER,
            "a replayable formatter result requires a non-empty environment hash",
        ));
    }
    let scalars = u32::try_from(result.text.chars().count()).unwrap_or(u32::MAX);
    let bytes = u32::try_from(result.text.len()).unwrap_or(u32::MAX);
    if scalars > limits.max_output_scalars || bytes > limits.max_output_bytes {
        return Err(runtime_error(
            DiagnosticCode::LIMIT_OUTPUT,
            format!("formatted output exceeds scalar/byte limits ({scalars}/{bytes})"),
        ));
    }
    Ok(())
}

fn analyze_graph(rules: &mut [CompiledRule], entries: &[(String, usize)]) -> MecoResult<()> {
    let edges = graph_edges(rules);
    let reachable = reachable_rules(&edges, entries);
    let productive = productive_rules(rules, &edges);
    let nullable = nullable_rules(rules, &edges);
    let components = strongly_connected_components(&edges);
    let mut component_sizes = vec![0_usize; rules.len()];
    for component in &components {
        component_sizes[*component] += 1;
    }
    let mut diagnostics = Vec::new();
    for index in 0..rules.len() {
        let recursive = component_sizes[components[index]] > 1 || edges[index].contains(&index);
        rules[index].analysis = RuleAnalysis {
            reachable: reachable[index],
            productive: productive[index],
            nullable: nullable[index],
            recursive,
        };
        if reachable[index] && !productive[index] {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::UNPRODUCTIVE_RULE,
                Severity::Error,
                Some(rules[index].span),
                format!(
                    "reachable rule `{}` has no terminal derivation",
                    rules[index].name
                ),
            ));
        }
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        let primary = diagnostics.remove(0);
        Err(MecoError::with_related(primary, diagnostics))
    }
}

fn graph_edges(rules: &[CompiledRule]) -> Vec<Vec<usize>> {
    rules
        .iter()
        .map(|rule| {
            rule.productions
                .iter()
                .flat_map(|production| {
                    production
                        .bindings
                        .iter()
                        .map(|binding| binding.rule)
                        .chain(production.parts.iter().filter_map(part_rule))
                })
                .collect()
        })
        .collect()
}

fn reachable_rules(edges: &[Vec<usize>], entries: &[(String, usize)]) -> Vec<bool> {
    let mut reachable = vec![false; edges.len()];
    let mut queue = entries
        .iter()
        .map(|(_, rule)| *rule)
        .collect::<VecDeque<_>>();
    while let Some(rule) = queue.pop_front() {
        if reachable[rule] {
            continue;
        }
        reachable[rule] = true;
        queue.extend(edges[rule].iter().copied());
    }
    reachable
}

fn productive_rules(rules: &[CompiledRule], _edges: &[Vec<usize>]) -> Vec<bool> {
    solve_monotone_rule_property(rules, |_| true, true)
}

fn nullable_rules(rules: &[CompiledRule], _edges: &[Vec<usize>]) -> Vec<bool> {
    solve_monotone_rule_property(
        rules,
        |production| {
            production.parts.iter().all(|part| match part {
                CompiledPart::Literal { text, .. } => text.is_empty(),
                CompiledPart::RuleCall { .. }
                | CompiledPart::Capture { .. }
                | CompiledPart::Value { .. } => true,
                CompiledPart::MessageCall { .. } => false,
            })
        },
        false,
    )
}

fn solve_monotone_rule_property(
    rules: &[CompiledRule],
    candidate: impl Fn(&CompiledProduction) -> bool,
    include_bindings: bool,
) -> Vec<bool> {
    let mut property = vec![false; rules.len()];
    let mut remaining = rules
        .iter()
        .map(|rule| {
            rule.productions
                .iter()
                .map(|production| {
                    if candidate(production) {
                        let body = production
                            .parts
                            .iter()
                            .filter(|part| part_rule(part).is_some())
                            .count();
                        body + if include_bindings {
                            production.bindings.len()
                        } else {
                            0
                        }
                    } else {
                        usize::MAX
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut reverse = vec![Vec::new(); rules.len()];
    for (rule, compiled_rule) in rules.iter().enumerate() {
        for (production, compiled_production) in compiled_rule.productions.iter().enumerate() {
            if remaining[rule][production] == usize::MAX {
                continue;
            }
            if include_bindings {
                for binding in &compiled_production.bindings {
                    reverse[binding.rule].push((rule, production));
                }
            }
            for part in &compiled_production.parts {
                if let Some(child) = part_rule(part) {
                    reverse[child].push((rule, production));
                }
            }
        }
    }
    let mut queue = VecDeque::new();
    for rule in 0..rules.len() {
        if remaining[rule].contains(&0) {
            property[rule] = true;
            queue.push_back(rule);
        }
    }
    while let Some(child) = queue.pop_front() {
        for (parent, production) in &reverse[child] {
            if property[*parent] || remaining[*parent][*production] == usize::MAX {
                continue;
            }
            remaining[*parent][*production] -= 1;
            if remaining[*parent][*production] == 0 {
                property[*parent] = true;
                queue.push_back(*parent);
            }
        }
    }
    property
}

fn part_rule(part: &CompiledPart) -> Option<usize> {
    match part {
        CompiledPart::RuleCall { rule, .. } | CompiledPart::Capture { rule, .. } => Some(*rule),
        CompiledPart::Literal { .. }
        | CompiledPart::Value { .. }
        | CompiledPart::MessageCall { .. } => None,
    }
}

fn strongly_connected_components(edges: &[Vec<usize>]) -> Vec<usize> {
    let mut visited = vec![false; edges.len()];
    let mut order = Vec::with_capacity(edges.len());
    for start in 0..edges.len() {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let mut stack = vec![(start, 0_usize)];
        while let Some((node, next)) = stack.last_mut() {
            if *next < edges[*node].len() {
                let child = edges[*node][*next];
                *next += 1;
                if !visited[child] {
                    visited[child] = true;
                    stack.push((child, 0));
                }
            } else {
                let (finished, _) = stack.pop().expect("DFS frame exists");
                order.push(finished);
            }
        }
    }
    let mut reverse = vec![Vec::new(); edges.len()];
    for (source, children) in edges.iter().enumerate() {
        for child in children {
            reverse[*child].push(source);
        }
    }
    let mut components = vec![usize::MAX; edges.len()];
    let mut component = 0_usize;
    for start in order.into_iter().rev() {
        if components[start] != usize::MAX {
            continue;
        }
        components[start] = component;
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            for parent in &reverse[node] {
                if components[*parent] == usize::MAX {
                    components[*parent] = component;
                    stack.push(*parent);
                }
            }
        }
        component += 1;
    }
    components
}

fn recursion_warnings(rules: &[CompiledRule]) -> Vec<Diagnostic> {
    let edges = graph_edges(rules);
    let components = strongly_connected_components(&edges);
    let mut warnings = Vec::new();
    for (rule_index, rule) in rules.iter().enumerate() {
        if !rule.analysis.reachable || !rule.analysis.recursive {
            continue;
        }
        if rule
            .productions
            .iter()
            .any(|production| production.guard.is_some())
        {
            continue;
        }
        let Some(rationals) = rule
            .productions
            .iter()
            .map(|production| match production.weight {
                CompiledWeight::Static(value) => Some(value),
                CompiledWeight::Dynamic(_) => None,
            })
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Ok(weights) = normalize_rationals(&rationals, Some(rule.span)) else {
            continue;
        };
        let total = weights
            .iter()
            .map(|weight| u128::from(*weight))
            .sum::<u128>();
        let offspring = rule
            .productions
            .iter()
            .zip(weights)
            .map(|(production, weight)| {
                let body_count = production
                    .parts
                    .iter()
                    .filter_map(part_rule)
                    .filter(|child| components[*child] == components[rule_index])
                    .count() as u128;
                let binding_count = production
                    .bindings
                    .iter()
                    .filter(|binding| components[binding.rule] == components[rule_index])
                    .count() as u128;
                u128::from(weight) * (body_count + binding_count)
            })
            .sum::<u128>();
        if offspring >= total {
            warnings.push(Diagnostic::new(
                DiagnosticCode::RECURSION_RISK,
                Severity::Warning,
                Some(rule.span),
                format!(
                    "recursive rule `{}` has mean in-component offspring at least one",
                    rule.name
                ),
            ));
        }
    }
    warnings
}

fn source_error(code: DiagnosticCode, span: Span, message: impl Into<String>) -> MecoError {
    MecoError::new(Diagnostic::new(code, Severity::Error, Some(span), message))
}

fn runtime_error(code: DiagnosticCode, message: impl Into<String>) -> MecoError {
    MecoError::new(Diagnostic::new(code, Severity::Error, None, message))
}

#[cfg(test)]
mod tests {
    use alloc::{format, string::ToString, vec, vec::Vec};
    use core::fmt::Write as _;

    use super::{
        GenerationLimits, GenerationRequest, compile_package, compile_package_with_manifest,
        validate_formatter_result,
    };
    use crate::{
        DataBinding, Diagnostic, DiagnosticCode, Formatter, FormatterRequest, FormatterResult,
        LocaleRequest, MecoResult, MessageArgument, MessageDefinition, MessageManifest,
        OutputRange, PackageInput, PackageSource, ProvenanceKind, Rational, SchemaType, Severity,
        SourceFile, SourceId, Value,
    };

    fn package(source: &str) -> PackageInput {
        PackageInput {
            root_id: "root".to_string(),
            modules: vec![PackageSource {
                canonical_id: "root".to_string(),
                source: SourceFile::new(SourceId::new(0), "root.meco", source),
                resolved_imports: vec![],
            }],
        }
    }

    fn data(name: &str, value: Value) -> DataBinding {
        DataBinding::new(name.to_string(), value)
    }

    fn arrival_manifest() -> MessageManifest {
        MessageManifest {
            messages: vec![MessageDefinition {
                id: "arrival".to_string(),
                arguments: vec![
                    MessageArgument {
                        name: "hero".to_string(),
                        type_: SchemaType::Text,
                    },
                    MessageArgument {
                        name: "count".to_string(),
                        type_: SchemaType::Number,
                    },
                ],
            }],
        }
    }

    struct EnglishFormatter;

    impl Formatter for EnglishFormatter {
        fn format(&mut self, request: &FormatterRequest) -> MecoResult<FormatterResult> {
            assert_eq!(request.message_id(), "arrival");
            assert_eq!(request.requested_locale(), "en");
            assert_eq!(request.arguments()[0].0, "hero");
            assert_eq!(request.arguments()[1].0, "count");
            let Value::Text(hero) = &request.arguments()[0].1 else {
                panic!("hero is text")
            };
            let Value::Number(count) = request.arguments()[1].1 else {
                panic!("count is numeric")
            };
            Ok(FormatterResult {
                text: format!("{hero} arrived with {count} items."),
                actual_locale: "en".to_string(),
                environment_hash: "test-formatter/en-v1".to_string(),
                diagnostics: vec![],
                work_units: 1,
                replayable: true,
            })
        }
    }

    #[test]
    fn compiles_and_generates_exact_static_weights() {
        let package = package(concat!(
            "---\nmeco: 2\nmodule: root\nentry: line\nexports: [line]\n---\n\n",
            "# line\n- [0.5] A\n- [1.5] B\n",
        ));
        let grammar = compile_package(&package).expect("weighted package compiles");
        let first = grammar
            .generate_weighted(&GenerationRequest::with_seed(0))
            .expect("generation succeeds");
        let replay = grammar
            .generate_weighted(&GenerationRequest::with_seed(0))
            .expect("replay succeeds");

        assert_eq!(first, replay);
        assert!(matches!(first.text(), "A" | "B"));
        assert_eq!(first.expansions(), 1);
        assert_eq!(first.sampler_words(), 1);
    }

    #[test]
    fn cached_static_fanout_preserves_traced_selection_and_seed_mapping() {
        let mut source = concat!(
            "---\nmeco: 2\nmodule: root\nentry: line\nexports: [line]\n---\n\n",
            "# line\n",
        )
        .to_string();
        for index in 0..1_024 {
            use core::fmt::Write as _;
            writeln!(source, "- alternative-{index}").expect("write source");
        }
        let grammar = compile_package(&package(&source)).expect("fanout package compiles");
        assert!(grammar.rules[0].static_selection.is_some());

        for seed in 0..64 {
            let fast = grammar
                .generate_weighted(&GenerationRequest::with_seed(seed))
                .expect("cached generation succeeds");
            let mut traced_request = GenerationRequest::with_seed(seed);
            traced_request.trace_selections = true;
            let traced = grammar
                .generate_weighted(&traced_request)
                .expect("traced generation succeeds");
            assert_eq!(fast.text(), traced.text());
            assert_eq!(fast.sampler_words(), traced.sampler_words());
            assert_eq!(traced.selections().len(), 1);
            assert_eq!(
                traced.selections()[0].selected_production_id(),
                grammar
                    .production_id(
                        "root.line",
                        traced.selections()[0].selected_production() as usize
                    )
                    .expect("selected production has an ID")
            );
        }
    }

    #[test]
    fn complete_messages_use_typed_manifest_and_synchronous_formatter() {
        let package = package(concat!(
            "---\nmeco: 2\nmodule: root\nentry: arrival\n",
            "inputs:\n  itemCount: number\nexports: [arrival]\n---\n\n",
            "# arrival\n- {name as hero}\n  &arrival <- $hero, count: $itemCount\n",
            "# name\n- Ada\n",
        ));
        let grammar = compile_package_with_manifest(&package, &arrival_manifest())
            .expect("message package compiles");
        let request_data = vec![data(
            "itemCount",
            Value::Number(Rational::new(2, 1).expect("count")),
        )];
        let request = GenerationRequest {
            data: &request_data,
            trace_bindings: true,
            trace_provenance: true,
            ..GenerationRequest::with_seed(0)
        };
        assert_eq!(
            grammar
                .generate_weighted(&request)
                .expect_err("plain generation requires formatter")
                .diagnostics()[0]
                .code(),
            DiagnosticCode::FORMATTER_REQUIRED
        );
        let mut formatter = EnglishFormatter;
        let generated = grammar
            .generate_weighted_with_formatter(
                &request,
                LocaleRequest {
                    requested: "en",
                    fallbacks: &[],
                },
                &mut formatter,
            )
            .expect("message is formatted");

        assert_eq!(generated.text(), "Ada arrived with 2 items.");
        assert_eq!(generated.bindings()[0].name(), "hero");
        let message = generated.message().expect("message trace");
        assert_eq!(message.message_id(), "arrival");
        assert_eq!(message.actual_locale(), "en");
        assert!(message.replayable());
        let message_node = generated
            .provenance()
            .iter()
            .find(|node| node.kind() == ProvenanceKind::Message)
            .expect("coarse message provenance");
        assert_eq!(message_node.name(), Some("arrival"));
        assert_eq!(
            message_node.output(),
            Some(OutputRange::new(
                0,
                generated.text().len() as u64,
                0,
                generated.text().chars().count() as u64
            ))
        );
        assert_eq!(grammar.manifest().messages, arrival_manifest());
        assert_eq!(grammar.manifest().inputs[0].name, "itemCount");
    }

    #[test]
    fn message_schema_and_transitive_effect_fail_with_stable_codes() {
        let missing = package(concat!(
            "---\nmeco: 2\nmodule: root\nentry: start\nexports: [start]\n---\n\n",
            "# start\n- &unknown\n",
        ));
        assert_eq!(
            compile_package_with_manifest(&missing, &arrival_manifest())
                .expect_err("missing ID fails")
                .diagnostics()[0]
                .code(),
            DiagnosticCode::MESSAGE_MISSING
        );

        for source in [
            concat!(
                "---\nmeco: 2\nmodule: root\nentry: start\ninputs:\n  itemCount: number\n",
                "exports: [start]\n---\n\n# start\n- before @localized\n",
                "# localized\n- &arrival <- hero: \"Ada\", count: $itemCount\n",
            ),
            concat!(
                "---\nmeco: 2\nmodule: root\nentry: start\ninputs:\n  itemCount: number\n",
                "exports: [start]\n---\n\n# start\n- @{localized as line}$line\n",
                "# localized\n- &arrival <- hero: \"Ada\", count: $itemCount\n",
            ),
            concat!(
                "---\nmeco: 2\nmodule: root\nentry: start\ninputs:\n  itemCount: number\n",
                "exports: [start]\n---\n\n# start\n- {localized as line}\n  $line\n",
                "# localized\n- &arrival <- hero: \"Ada\", count: $itemCount\n",
            ),
            concat!(
                "---\nmeco: 2\nmodule: root\nentry: start\ninputs:\n  itemCount: number\n",
                "exports: [start]\n---\n\n# start\n",
                "- &arrival <- hero: \"Ada\", count: $itemCount\n- plain\n",
            ),
        ] {
            assert_eq!(
                compile_package_with_manifest(&package(source), &arrival_manifest())
                    .expect_err("invalid message composition fails")
                    .diagnostics()[0]
                    .code(),
                DiagnosticCode::MESSAGE_EFFECT
            );
        }
    }

    #[test]
    fn formatter_results_enforce_locale_work_provenance_diagnostics_and_output_limits() {
        let request = FormatterRequest::new(
            "arrival".to_string(),
            vec![],
            "fr".to_string(),
            vec!["en".to_string()],
        );
        let valid = FormatterResult {
            text: "hello".to_string(),
            actual_locale: "en".to_string(),
            environment_hash: "formatter/v1".to_string(),
            diagnostics: vec![],
            work_units: 1,
            replayable: true,
        };
        validate_formatter_result(&request, &valid, GenerationLimits::default())
            .expect("valid fallback formatter result");

        let mut invalid = valid.clone();
        invalid.actual_locale = "de".to_string();
        assert_eq!(
            validate_formatter_result(&request, &invalid, GenerationLimits::default())
                .expect_err("outside locale fails")
                .diagnostics()[0]
                .code(),
            DiagnosticCode::LOCALE
        );
        invalid = valid.clone();
        invalid.work_units = 10_001;
        assert_eq!(
            validate_formatter_result(&request, &invalid, GenerationLimits::default())
                .expect_err("work limit fails")
                .diagnostics()[0]
                .code(),
            DiagnosticCode::FORMATTER_LIMIT
        );
        invalid = valid.clone();
        invalid.environment_hash.clear();
        assert_eq!(
            validate_formatter_result(&request, &invalid, GenerationLimits::default())
                .expect_err("replay provenance fails")
                .diagnostics()[0]
                .code(),
            DiagnosticCode::FORMATTER
        );
        invalid = valid.clone();
        invalid.diagnostics.push(Diagnostic::new(
            DiagnosticCode::FORMATTER,
            Severity::Error,
            None,
            "catalog failed",
        ));
        assert_eq!(
            validate_formatter_result(&request, &invalid, GenerationLimits::default())
                .expect_err("fatal formatter diagnostic fails")
                .diagnostics()[0]
                .code(),
            DiagnosticCode::FORMATTER
        );
        let limits = GenerationLimits {
            max_output_scalars: 4,
            ..GenerationLimits::default()
        };
        assert_eq!(
            validate_formatter_result(&request, &valid, limits)
                .expect_err("formatted output limit fails")
                .diagnostics()[0]
                .code(),
            DiagnosticCode::LIMIT_OUTPUT
        );
    }

    #[test]
    fn iterative_generation_reports_depth_without_stack_recursion() {
        let package = package(concat!(
            "---\nmeco: 2\nmodule: root\nentry: loop\nexports: [loop]\n---\n\n",
            "# loop\n- [1] done\n- [999999999999999999] @loop\n",
        ));
        let grammar = compile_package(&package).expect("productive recursion compiles");
        let request = GenerationRequest {
            entry: None,
            seed: 0,
            limits: GenerationLimits {
                max_depth: 8,
                ..GenerationLimits::default()
            },
            data: &[],
            trace_bindings: false,
            trace_selections: false,
            trace_provenance: false,
        };
        let error = grammar
            .generate_weighted(&request)
            .expect_err("recursive stream reaches deterministic depth limit");

        assert_eq!(error.diagnostics()[0].code(), DiagnosticCode::LIMIT_DEPTH);
    }

    #[test]
    fn rejects_reachable_unproductive_cycles() {
        let package = package(concat!(
            "---\nmeco: 2\nmodule: root\nentry: a\nexports: [a]\n---\n\n",
            "# a\n- @b\n# b\n- @a\n",
        ));
        let error = compile_package(&package).expect_err("cycle has no terminal derivation");

        assert_eq!(
            error.diagnostics()[0].code(),
            DiagnosticCode::UNPRODUCTIVE_RULE
        );
        assert_eq!(error.diagnostics().len(), 2);
    }

    #[test]
    fn validates_named_call_arity_before_the_later_parameter_runtime() {
        let package = package(concat!(
            "---\nmeco: 2\nmodule: root\nentry: start\nexports: [start]\n---\n\n",
            "# target <- name: text\n- hello\n",
            "# start\n- @target <- wrong: \"value\"\n",
        ));
        let error = compile_package(&package).expect_err("wrong named argument must fail");

        assert_eq!(error.diagnostics()[0].code(), DiagnosticCode::RULE_ARITY);
    }

    #[test]
    fn executes_typed_calls_guards_inputs_and_dynamic_weights() {
        let package = package(concat!(
            "---\nmeco: 2\nmodule: root\nentry: start\n",
            "types:\n  Mood: [calm, tense]\n",
            "inputs:\n  playerName: text\n  mood: Mood\n  urgency: number\n  enabled: boolean\n",
            "exports: [start]\n---\n\n",
            "# start\n- @greeting <- name: $playerName, tone: $mood, level: $urgency\n",
            "# greeting <- name: text, tone: Mood, level: number\n",
            "- [weight = level] {tone is tense and level >= 2} Alert, $name!\n",
            "- [1] {tone is calm} Hello, $name.\n",
        ));
        let grammar = compile_package(&package).expect("typed package compiles");
        let request_data = vec![
            data("playerName", Value::Text("Ada".to_string())),
            data("mood", Value::Enum("tense".to_string())),
            data(
                "urgency",
                Value::Number(Rational::new(2, 1).expect("number")),
            ),
            data("enabled", Value::Boolean(true)),
        ];
        let result = grammar
            .generate_weighted(&GenerationRequest {
                data: &request_data,
                trace_selections: true,
                ..GenerationRequest::with_seed(0)
            })
            .expect("typed generation succeeds");

        assert_eq!(result.text(), "Alert, Ada!");
        let greeting = result
            .selections()
            .iter()
            .find(|selection| selection.rule() == "root.greeting")
            .expect("parameterized selection is traced");
        assert_eq!(
            greeting.eligible()[0].base_weight(),
            Rational::new(2, 1).expect("valid trace weight")
        );
    }

    #[test]
    fn zero_dynamic_weight_and_false_guards_remove_productions() {
        let package = package(concat!(
            "---\nmeco: 2\nmodule: root\nentry: start\n",
            "inputs:\n  urgency: number\n  enabled: boolean\n",
            "exports: [start]\n---\n\n",
            "# start\n",
            "- [weight = urgency] dynamic\n",
            "- [1] {enabled} enabled\n",
            "- [1] {not enabled} fallback\n",
        ));
        let grammar = compile_package(&package).expect("dynamic package compiles");
        let request_data = vec![
            data("urgency", Value::Number(Rational::ZERO)),
            data("enabled", Value::Boolean(false)),
        ];
        let result = grammar
            .generate_weighted(&GenerationRequest {
                data: &request_data,
                ..GenerationRequest::with_seed(0)
            })
            .expect("one production remains");

        assert_eq!(result.text(), "fallback");
    }

    #[test]
    fn invalid_dynamic_weight_results_have_stable_runtime_codes() {
        let package = package(concat!(
            "---\nmeco: 2\nmodule: root\nentry: start\n",
            "inputs:\n  weight: number\nexports: [start]\n---\n\n",
            "# start\n- [weight = weight - 1] value\n",
        ));
        let grammar = compile_package(&package).expect("dynamic package compiles");
        for (value, expected) in [
            (Rational::ZERO, DiagnosticCode::WEIGHT_VALUE),
            (Rational::ONE, DiagnosticCode::NO_ELIGIBLE_PRODUCTION),
        ] {
            let request_data = vec![data("weight", Value::Number(value))];
            let error = grammar
                .generate_weighted(&GenerationRequest {
                    data: &request_data,
                    ..GenerationRequest::with_seed(0)
                })
                .expect_err("invalid dynamic result fails");
            assert_eq!(error.diagnostics()[0].code(), expected);
        }
    }

    #[test]
    fn bindings_and_captures_are_ordered_local_and_optionally_traced() {
        let package = package(concat!(
            "---\nmeco: 2\nmodule: root\nentry: start\nexports: [start]\n---\n\n",
            "# start\n",
            "- {name as hero}\n",
            "  {name as companion}\n",
            "  $hero/@{name as witness}/$witness/$companion\n",
            "# name\n- Ada\n- Marcus\n- Priya\n",
        ));
        let grammar = compile_package(&package).expect("binding package compiles");
        let result = grammar
            .generate_weighted(&GenerationRequest {
                trace_bindings: true,
                ..GenerationRequest::with_seed(3)
            })
            .expect("binding generation succeeds");
        let pieces = result.text().split('/').collect::<Vec<_>>();

        assert_eq!(pieces.len(), 4);
        assert_eq!(pieces[1], pieces[2]);
        assert_eq!(result.bindings().len(), 3);
        assert_eq!(result.bindings()[0].name(), "hero");
        assert_eq!(result.bindings()[1].name(), "companion");
        assert_eq!(result.bindings()[2].name(), "witness");
        assert!(!result.bindings()[0].emitted());
        assert!(result.bindings()[2].emitted());
    }

    #[test]
    fn nested_calls_use_fresh_explicit_parameter_frames() {
        let package = package(concat!(
            "---\nmeco: 2\nmodule: root\nentry: start\nexports: [start]\n---\n\n",
            "# start\n- @outer <- outerName: \"outer\", innerName: \"inner\"\n",
            "# outer <- outerName: text, innerName: text\n",
            "- @inner <- name: $innerName, outerName: $outerName\n",
            "# inner <- name: text, outerName: text\n- $name/$outerName\n",
        ));
        let grammar = compile_package(&package).expect("parameter frames compile");
        let result = grammar
            .generate_weighted(&GenerationRequest::with_seed(0))
            .expect("nested calls generate");

        assert_eq!(result.text(), "inner/outer");
    }

    #[test]
    fn request_data_and_call_types_are_checked() {
        let package = package(concat!(
            "---\nmeco: 2\nmodule: root\nentry: start\n",
            "types:\n  Mood: [calm, tense]\ninputs:\n  mood: Mood\n",
            "exports: [start]\n---\n\n",
            "# start\n- @line <- tone: $mood\n",
            "# line <- tone: Mood\n- {tone is tense} tense\n- {tone is calm} calm\n",
        ));
        let grammar = compile_package(&package).expect("enum package compiles");
        let wrong = vec![data("mood", Value::Enum("unknown".to_string()))];
        let error = grammar
            .generate_weighted(&GenerationRequest {
                data: &wrong,
                ..GenerationRequest::with_seed(0)
            })
            .expect_err("unknown enum member fails request validation");

        assert_eq!(error.diagnostics()[0].code(), DiagnosticCode::TYPE_MISMATCH);
    }

    #[test]
    fn rejects_scaled_static_weight_totals_outside_the_contract() {
        let mut source = concat!(
            "---\nmeco: 2\nmodule: root\nentry: line\nexports: [line]\n---\n\n",
            "# line\n",
        )
        .to_string();
        for index in 0..10_u64 {
            writeln!(
                source,
                "- [{}] value-{index}",
                999_999_999_999_999_990_u64 + index
            )
            .expect("string formatting cannot fail");
        }
        let error = compile_package(&package(&source)).expect_err("scaled sum must fail");

        assert_eq!(
            error.diagnostics()[0].code(),
            DiagnosticCode::WEIGHT_OVERFLOW
        );
    }

    #[test]
    fn retains_reachability_nullability_recursion_and_risk_facts() {
        let package = package(concat!(
            "---\nmeco: 2\nmodule: root\nentry: start\nexports: [start]\n---\n\n",
            "# start\n- @nullable@risk\n",
            "# nullable\n- \"\"\n",
            "# risk\n- done\n- @risk@risk\n",
            "# unreachable-cycle\n- @unreachable-cycle\n",
        ));
        let grammar = compile_package(&package).expect("unreachable bad cycle is allowed");
        let nullable = grammar
            .rule_analysis("root.nullable")
            .expect("nullable analysis");
        let risk = grammar.rule_analysis("root.risk").expect("risk analysis");
        let unreachable = grammar
            .rule_analysis("root.unreachable-cycle")
            .expect("unreachable analysis");

        assert!(nullable.nullable && nullable.productive && nullable.reachable);
        assert!(risk.recursive && risk.productive && risk.reachable);
        assert!(unreachable.recursive && !unreachable.productive && !unreachable.reachable);
        assert_eq!(grammar.warnings().len(), 1);
        assert_eq!(grammar.warnings()[0].code(), DiagnosticCode::RECURSION_RISK);
    }

    #[test]
    fn enforces_expansion_output_and_sampler_limits_independently() {
        let package = package(concat!(
            "---\nmeco: 2\nmodule: root\nentry: start\nexports: [start]\n---\n\n",
            "# start\n- @child\n# child\n- 🦀\n",
        ));
        let grammar = compile_package(&package).expect("limit package compiles");
        let mut request = GenerationRequest::with_seed(0);
        request.limits.max_expansions = 1;
        assert_eq!(
            grammar
                .generate_weighted(&request)
                .expect_err("expansion limit")
                .diagnostics()[0]
                .code(),
            DiagnosticCode::LIMIT_EXPANSIONS
        );

        request.limits = GenerationLimits::default();
        request.limits.max_output_bytes = 3;
        assert_eq!(
            grammar
                .generate_weighted(&request)
                .expect_err("UTF-8 byte limit")
                .diagnostics()[0]
                .code(),
            DiagnosticCode::LIMIT_OUTPUT
        );

        request.limits = GenerationLimits::default();
        request.limits.max_sampler_words = 0;
        assert_eq!(
            grammar
                .generate_weighted(&request)
                .expect_err("sampler limit")
                .diagnostics()[0]
                .code(),
            DiagnosticCode::SAMPLER_BUDGET
        );
    }

    #[test]
    fn production_ids_survive_reordering_and_authored_ids_survive_edits() {
        let header = "---\nmeco: 2\nmodule: root\nentry: line\nexports: [line]\n---\n# line\n";
        let first = compile_package(&package(&format!(
            "{header}- Alpha.\n- Beta.\n- [weight = 1, id = fixed] Before.\n"
        )))
        .expect("identity fixture compiles");
        let reordered = compile_package(&package(&format!(
            "{header}- Beta.\n- Alpha.\n- [weight = 9, id = fixed] After.\n"
        )))
        .expect("reordered identity fixture compiles");

        assert_eq!(
            first.production_id("root.line", 0),
            reordered.production_id("root.line", 1)
        );
        assert_eq!(
            first.production_id("root.line", 1),
            reordered.production_id("root.line", 0)
        );
        assert_eq!(first.production_id("root.line", 2), Some("fixed"));
        assert_eq!(reordered.production_id("root.line", 2), Some("fixed"));
        assert_eq!(
            reordered.production_id_is_authored("root.line", 2),
            Some(true)
        );
    }

    #[test]
    fn identical_unlabelled_productions_are_rejected_as_identity_collisions() {
        let error = compile_package(&package(concat!(
            "---\nmeco: 2\nmodule: root\nentry: line\nexports: [line]\n---\n",
            "# line\n- same\n- same\n",
        )))
        .expect_err("identical alternatives cannot receive stable IDs");

        assert_eq!(error.diagnostics()[0].code(), DiagnosticCode::PRODUCTION_ID);
        assert_eq!(error.diagnostics().len(), 2);
    }

    #[test]
    fn provenance_covers_visible_emitters_and_keeps_bindings_non_emitting() {
        let grammar = compile_package(&package(concat!(
            "---\nmeco: 2\nmodule: root\nentry: line\nexports: [line]\n",
            "inputs:\n  playerName: text\n---\n",
            "# line\n- [weight = 1, id = root-shell] {name as hero}\n",
            "  Begin @deep $playerName and $hero.\n",
            "# name\n- [weight = 1, id = bound-name] Ada\n",
            "# deep\n- [weight = 1, id = deep-word] middle\n",
        )))
        .expect("provenance fixture compiles");
        let values = vec![DataBinding::new(
            "playerName".to_string(),
            Value::Text("Rin".to_string()),
        )];
        let result = grammar
            .generate_weighted(&GenerationRequest {
                data: &values,
                trace_selections: true,
                trace_provenance: true,
                ..GenerationRequest::with_seed(0)
            })
            .expect("traced generation succeeds");
        let untraced = grammar
            .generate_weighted(&GenerationRequest {
                data: &values,
                ..GenerationRequest::with_seed(0)
            })
            .expect("untraced generation succeeds");

        assert_eq!(result.text(), "Begin middle Rin and Ada.");
        assert_eq!(untraced.text(), result.text());
        assert_eq!(untraced.expansions(), result.expansions());
        assert_eq!(untraced.sampler_words(), result.sampler_words());
        assert!(untraced.provenance().is_empty());
        assert!(
            result
                .selections()
                .iter()
                .any(|selection| selection.selected_production_id() == "root-shell")
        );
        let binding = result
            .provenance()
            .iter()
            .find(|node| node.kind() == ProvenanceKind::Binding)
            .expect("binding node retained");
        assert_eq!(binding.name(), Some("hero"));
        assert_eq!(binding.output(), None);
        assert!(result.provenance().iter().any(|node| {
            node.kind() == ProvenanceKind::HostValue
                && node.output().is_some_and(|range| {
                    let start = usize::try_from(range.start_byte()).expect("test range fits usize");
                    let end = usize::try_from(range.end_byte()).expect("test range fits usize");
                    &result.text()[start..end] == "Rin"
                })
        }));
        assert!(result.provenance().iter().any(|node| {
            node.kind() == ProvenanceKind::BoundValue
                && node.parent() == Some(binding.id())
                && node.output().is_some_and(|range| {
                    let start = usize::try_from(range.start_byte()).expect("test range fits usize");
                    let end = usize::try_from(range.end_byte()).expect("test range fits usize");
                    &result.text()[start..end] == "Ada"
                })
        }));

        let mut covered = vec![false; result.text().chars().count()];
        for node in result.provenance().iter().filter(|node| {
            matches!(
                node.kind(),
                ProvenanceKind::AuthoredText
                    | ProvenanceKind::HostValue
                    | ProvenanceKind::BoundValue
                    | ProvenanceKind::Message
            )
        }) {
            if let Some(range) = node.output() {
                for scalar in range.start_scalar()..range.end_scalar() {
                    covered[usize::try_from(scalar).expect("small fixture scalar")] = true;
                }
            }
        }
        assert!(covered.into_iter().all(core::convert::identity));
    }
}
