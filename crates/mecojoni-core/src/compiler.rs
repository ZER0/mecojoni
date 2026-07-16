use alloc::{
    boxed::Box,
    collections::VecDeque,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use crate::{
    ArgumentSyntax, BindingTrace, BodyPartSyntax, BodySyntax, ClauseSyntax, DataBinding,
    Diagnostic, DiagnosticCode, EligibleWeightTrace, GuardExpression, GuardValue, MecoError,
    MecoResult, ModuleSyntax, PackageInput, Rational, SelectionTrace, Severity, Span, SplitMix64,
    Value, ValueSyntax, WeightExpression, WeightSyntax, parse_module, validate_package_input,
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
enum CompiledPart {
    Literal(String),
    RuleCall {
        rule: usize,
        arguments: Vec<CompiledValue>,
    },
    Value(CompiledValue),
    Capture {
        rule: usize,
        slot: usize,
        name: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CompiledValue {
    Input(usize),
    Local(usize),
    Constant(Value),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CompiledWeight {
    Static(Rational),
    Dynamic(CompiledWeightExpression),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CompiledWeightExpression {
    Literal(Rational),
    Value(CompiledValue),
    Add(Box<Self>, Box<Self>),
    Subtract(Box<Self>, Box<Self>),
    Multiply(Box<Self>, Box<Self>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CompiledGuardValue {
    Value(CompiledValue),
    Constant(Value),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CompiledGuard {
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
struct CompiledBinding {
    rule: usize,
    arguments: Vec<CompiledValue>,
    slot: usize,
    name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CompiledProduction {
    weight: CompiledWeight,
    guard: Option<CompiledGuard>,
    bindings: Vec<CompiledBinding>,
    parts: Vec<CompiledPart>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ValueType {
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
struct CompiledInput {
    external_name: String,
    type_: ValueType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CompiledRule {
    name: String,
    parameters: Vec<(String, ValueType)>,
    span: Span,
    productions: Vec<CompiledProduction>,
    analysis: RuleAnalysis,
}

/// Immutable, indexed package artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledGrammar {
    rules: Vec<CompiledRule>,
    inputs: Vec<CompiledInput>,
    entries: Vec<(String, usize)>,
    default_entry: Option<usize>,
    warnings: Vec<Diagnostic>,
}

impl CompiledGrammar {
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

    #[must_use]
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    #[must_use]
    pub fn rule_analysis(&self, name: &str) -> Option<RuleAnalysis> {
        self.rules
            .iter()
            .find(|rule| rule.name == name)
            .map(|rule| rule.analysis)
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
        let (entry_name, entry_rule) = self.resolve_entry(request.entry)?;
        let inputs = self.validate_request_data(request.data)?;
        let mut random = SplitMix64::new(request.seed);
        let mut buffers = vec![String::new()];
        let mut frames = Vec::<RuntimeFrame>::new();
        let mut binding_trace = Vec::new();
        let mut selection_trace = Vec::new();
        let mut output_scalars = 0_u32;
        let mut expansions = 0_u32;
        let mut stack = vec![Work::Expand {
            rule: entry_rule,
            arguments: Vec::new(),
            sink: 0,
            depth: 1,
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
                    });
                    match compiled_part {
                        CompiledPart::Literal(text) => append_output(
                            &mut buffers[sink],
                            text,
                            &mut output_scalars,
                            request.limits,
                        )?,
                        CompiledPart::Value(value) => {
                            let value = runtime_value(value, &inputs, &frames[frame].values)?;
                            let Value::Text(text) = value else {
                                return Err(runtime_error(
                                    DiagnosticCode::TYPE_MISMATCH,
                                    "only text values can be emitted directly",
                                ));
                            };
                            append_output(
                                &mut buffers[sink],
                                &text,
                                &mut output_scalars,
                                request.limits,
                            )?;
                        }
                        CompiledPart::RuleCall { rule, arguments } => {
                            let arguments =
                                evaluate_arguments(arguments, &inputs, &frames[frame].values)?;
                            stack.push(Work::Expand {
                                rule: *rule,
                                arguments,
                                sink,
                                depth: depth.checked_add(1).unwrap_or(u32::MAX),
                            });
                        }
                        CompiledPart::Capture { rule, slot, name } => {
                            let start = buffers[sink].len();
                            stack.push(Work::FinishCapture {
                                frame,
                                slot: *slot,
                                name: name.clone(),
                                sink,
                                start,
                            });
                            stack.push(Work::Expand {
                                rule: *rule,
                                arguments: Vec::new(),
                                sink,
                                depth: depth.checked_add(1).unwrap_or(u32::MAX),
                            });
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
                        });
                        continue;
                    };
                    let temporary_sink = buffers.len();
                    buffers.push(String::new());
                    stack.push(Work::PrepareBinding {
                        rule,
                        production,
                        binding: binding + 1,
                        frame,
                        sink,
                        depth,
                    });
                    stack.push(Work::FinishBinding {
                        frame,
                        slot: compiled_binding.slot,
                        name: compiled_binding.name.clone(),
                        sink: temporary_sink,
                    });
                    stack.push(Work::Expand {
                        rule: compiled_binding.rule,
                        arguments: evaluate_arguments(
                            &compiled_binding.arguments,
                            &inputs,
                            &frames[frame].values,
                        )?,
                        sink: temporary_sink,
                        depth: depth.checked_add(1).unwrap_or(u32::MAX),
                    });
                }
                Work::FinishBinding {
                    frame,
                    slot,
                    name,
                    sink,
                } => {
                    let text = core::mem::take(&mut buffers[sink]);
                    let value = Value::Text(text);
                    bind_runtime_value(&mut frames[frame], slot, value.clone())?;
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
                    bind_runtime_value(&mut frames[frame], slot, value.clone())?;
                    if request.trace_bindings {
                        binding_trace.push(BindingTrace::new(name, value, true));
                    }
                }
                Work::Expand {
                    rule,
                    arguments,
                    sink,
                    depth,
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
                    if arguments.len() != compiled_rule.parameters.len() {
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
                    frames.push(RuntimeFrame { values: arguments });
                    let weighted = eligible_weights(compiled_rule, &inputs, &frames[frame].values)?;
                    let total = weighted
                        .iter()
                        .try_fold(0_u64, |sum, weight| sum.checked_add(weight.normalized))
                        .ok_or_else(|| {
                            weight_runtime_overflow("eligible weight total overflowed")
                        })?;
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
                    let production = select_eligible_production(&weighted, choice);
                    if request.trace_selections {
                        selection_trace.push(SelectionTrace::new(
                            compiled_rule.name.clone(),
                            u32::try_from(production).unwrap_or(u32::MAX),
                            weighted
                                .iter()
                                .map(|weight| {
                                    EligibleWeightTrace::new(
                                        u32::try_from(weight.production).unwrap_or(u32::MAX),
                                        weight.base,
                                        weight.normalized,
                                    )
                                })
                                .collect(),
                        ));
                    }
                    stack.push(Work::PrepareBinding {
                        rule,
                        production,
                        binding: 0,
                        frame,
                        sink,
                        depth,
                    });
                }
            }
        }

        Ok(GenerationResult {
            text: buffers.swap_remove(0),
            entry: entry_name.to_string(),
            expansions,
            sampler_words: u32::try_from(random.words()).unwrap_or(u32::MAX),
            bindings: binding_trace,
            selections: selection_trace,
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
        sink: usize,
        depth: u32,
    },
    Continue {
        rule: usize,
        production: usize,
        part: usize,
        frame: usize,
        sink: usize,
        depth: u32,
    },
    PrepareBinding {
        rule: usize,
        production: usize,
        binding: usize,
        frame: usize,
        sink: usize,
        depth: u32,
    },
    FinishBinding {
        frame: usize,
        slot: usize,
        name: String,
        sink: usize,
    },
    FinishCapture {
        frame: usize,
        slot: usize,
        name: String,
        sink: usize,
        start: usize,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RuntimeFrame {
    values: Vec<Value>,
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
            let mut productions = Vec::with_capacity(rule.productions.len());
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
                )?;
                validate_bound_value_usage(production.span, &bindings, &parts)?;
                productions.push(CompiledProduction {
                    weight,
                    guard,
                    bindings,
                    parts,
                });
            }
            validate_static_weight_budget(rule.span, &productions)?;
            rules.push(CompiledRule {
                name: format!(
                    "{}.{}",
                    module.syntax.front_matter.module().value(),
                    rule.name.value()
                ),
                parameters: rule_parameters[global_rule].clone(),
                span: rule.span,
                productions,
                analysis: RuleAnalysis {
                    reachable: false,
                    productive: false,
                    nullable: false,
                    recursive: false,
                },
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
    analyze_graph(&mut rules, &entries)?;
    let warnings = recursion_warnings(&rules);
    Ok(CompiledGrammar {
        rules,
        inputs,
        entries,
        default_entry,
        warnings,
    })
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

fn lower_body(
    package: &PackageInput,
    modules: &[ModuleBuild<'_>],
    offsets: &[usize],
    schemas: &[ModuleSchema],
    rule_parameters: &[Vec<(String, ValueType)>],
    scope: &mut CompileScope,
    body: &BodySyntax,
) -> MecoResult<Vec<CompiledPart>> {
    match body {
        BodySyntax::Empty(_) => Ok(Vec::new()),
        BodySyntax::Block(block) if block.raw => Ok(if block.text.value().is_empty() {
            Vec::new()
        } else {
            vec![CompiledPart::Literal(block.text.value().clone())]
        }),
        BodySyntax::Block(block) => lower_parts(
            package,
            modules,
            offsets,
            schemas,
            rule_parameters,
            scope,
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
            parts,
        ),
    }
}

fn lower_parts(
    package: &PackageInput,
    modules: &[ModuleBuild<'_>],
    offsets: &[usize],
    schemas: &[ModuleSchema],
    rule_parameters: &[Vec<(String, ValueType)>],
    scope: &mut CompileScope,
    parts: &[BodyPartSyntax],
) -> MecoResult<Vec<CompiledPart>> {
    let mut lowered = Vec::new();
    for part in parts {
        match part {
            BodyPartSyntax::Literal(literal) => {
                if !literal.value().is_empty() {
                    lowered.push(CompiledPart::Literal(literal.value().clone()));
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
                });
            }
            BodyPartSyntax::EmittingCapture {
                rule,
                name,
                span: _,
            } => {
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
                lowered.push(CompiledPart::Value(value));
            }
            BodyPartSyntax::MessageCall(call) => {
                return Err(source_error(
                    DiagnosticCode::UNSUPPORTED_FEATURE,
                    call.span,
                    "complete messages are implemented in Milestone 6",
                ));
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
            CompiledPart::Value(value) => collect_local_value(value, &mut used),
            CompiledPart::RuleCall { arguments, .. } => {
                for argument in arguments {
                    collect_local_value(argument, &mut used);
                }
            }
            CompiledPart::Literal(_) | CompiledPart::Capture { .. } => {}
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

fn validate_static_weight_budget(span: Span, productions: &[CompiledProduction]) -> MecoResult<()> {
    let Some(rationals) = productions
        .iter()
        .map(|production| match production.weight {
            CompiledWeight::Static(value) => Some(value),
            CompiledWeight::Dynamic(_) => None,
        })
        .collect::<Option<Vec<_>>>()
    else {
        return Ok(());
    };
    let _ = normalize_rationals(&rationals, Some(span))?;
    Ok(())
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

fn bind_runtime_value(frame: &mut RuntimeFrame, slot: usize, value: Value) -> MecoResult<()> {
    if frame.values.len() != slot {
        return Err(runtime_error(
            DiagnosticCode::BINDING_NAME,
            "binding slot order is inconsistent with the compiled frame",
        ));
    }
    frame.values.push(value);
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
                CompiledPart::Literal(text) => text.is_empty(),
                CompiledPart::RuleCall { .. }
                | CompiledPart::Capture { .. }
                | CompiledPart::Value(_) => true,
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
        CompiledPart::Literal(_) | CompiledPart::Value(_) => None,
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

    use super::{GenerationLimits, GenerationRequest, compile_package};
    use crate::{
        DataBinding, DiagnosticCode, PackageInput, PackageSource, Rational, SourceFile, SourceId,
        Value,
    };

    fn package(source: &str) -> PackageInput {
        PackageInput {
            root_id: "root".to_string(),
            modules: vec![PackageSource {
                canonical_id: "root".to_string(),
                source: SourceFile::new(SourceId::new(0), "root.meco.md", source),
                resolved_imports: vec![],
            }],
        }
    }

    fn data(name: &str, value: Value) -> DataBinding {
        DataBinding::new(name.to_string(), value)
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
            source.push_str(&format!(
                "- [{}] value-{index}\n",
                999_999_999_999_999_990_u64 + index
            ));
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
}
