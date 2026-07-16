use alloc::{
    collections::VecDeque,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use crate::{
    BodyPartSyntax, BodySyntax, ClauseSyntax, Diagnostic, DiagnosticCode, MecoError, MecoResult,
    ModuleSyntax, PackageInput, Rational, Severity, Span, SplitMix64, WeightSyntax, parse_module,
    validate_package_input,
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
}

impl GenerationRequest<'_> {
    #[must_use]
    pub const fn with_seed(seed: u64) -> Self {
        Self {
            entry: None,
            seed,
            limits: GenerationLimits::INTERACTIVE_WEIGHTED_V1,
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
    Rule(usize),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CompiledProduction {
    weight: u64,
    parts: Vec<CompiledPart>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CompiledRule {
    name: String,
    span: Span,
    productions: Vec<CompiledProduction>,
    analysis: RuleAnalysis,
}

/// Immutable, indexed package artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledGrammar {
    rules: Vec<CompiledRule>,
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
        let mut random = SplitMix64::new(request.seed);
        let mut output = String::new();
        let mut output_scalars = 0_u32;
        let mut expansions = 0_u32;
        let mut stack = vec![Work::Expand {
            rule: entry_rule,
            depth: 1,
        }];

        while let Some(work) = stack.pop() {
            match work {
                Work::Emit {
                    rule,
                    production,
                    part,
                } => {
                    let CompiledPart::Literal(text) =
                        &self.rules[rule].productions[production].parts[part]
                    else {
                        unreachable!("only literal work items are emitted");
                    };
                    let added = u32::try_from(text.chars().count()).map_err(|_| {
                        runtime_error(
                            DiagnosticCode::LIMIT_OUTPUT,
                            "one literal exceeds the output scalar counter",
                        )
                    })?;
                    output_scalars = output_scalars.checked_add(added).ok_or_else(|| {
                        runtime_error(
                            DiagnosticCode::LIMIT_OUTPUT,
                            "generated output exceeds the scalar counter",
                        )
                    })?;
                    if output_scalars > request.limits.max_output_scalars {
                        return Err(runtime_error(
                            DiagnosticCode::LIMIT_OUTPUT,
                            format!(
                                "generated output exceeds {} Unicode scalars",
                                request.limits.max_output_scalars
                            ),
                        ));
                    }
                    let next_bytes = output.len().checked_add(text.len()).ok_or_else(|| {
                        runtime_error(
                            DiagnosticCode::LIMIT_OUTPUT,
                            "generated output exceeds the byte counter",
                        )
                    })?;
                    if u32::try_from(next_bytes)
                        .map_or(true, |bytes| bytes > request.limits.max_output_bytes)
                    {
                        return Err(runtime_error(
                            DiagnosticCode::LIMIT_OUTPUT,
                            format!(
                                "generated output exceeds {} UTF-8 bytes",
                                request.limits.max_output_bytes
                            ),
                        ));
                    }
                    output.push_str(text);
                }
                Work::Continue {
                    rule,
                    production,
                    part,
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
                        depth,
                    });
                    stack.push(match compiled_part {
                        CompiledPart::Literal(_) => Work::Emit {
                            rule,
                            production,
                            part,
                        },
                        CompiledPart::Rule(child) => Work::Expand {
                            rule: *child,
                            depth: depth.checked_add(1).unwrap_or(u32::MAX),
                        },
                    });
                }
                Work::Expand { rule, depth } => {
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
                    let total = compiled_rule
                        .productions
                        .iter()
                        .try_fold(0_u64, |sum, production| sum.checked_add(production.weight))
                        .ok_or_else(|| {
                            runtime_error(
                                DiagnosticCode::WEIGHT_OVERFLOW,
                                "compiled weight total is outside the supported budget",
                            )
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
                    let production = select_production(&compiled_rule.productions, choice);
                    stack.push(Work::Continue {
                        rule,
                        production,
                        part: 0,
                        depth,
                    });
                }
            }
        }

        Ok(GenerationResult {
            text: output,
            entry: entry_name.to_string(),
            expansions,
            sampler_words: u32::try_from(random.words()).unwrap_or(u32::MAX),
        })
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Work {
    Expand {
        rule: usize,
        depth: u32,
    },
    Emit {
        rule: usize,
        production: usize,
        part: usize,
    },
    Continue {
        rule: usize,
        production: usize,
        part: usize,
        depth: u32,
    },
}

struct ModuleBuild<'a> {
    canonical_id: &'a str,
    syntax: ModuleSyntax,
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

    let mut rules = Vec::with_capacity(rule_count);
    for (module_index, module) in modules.iter().enumerate() {
        for rule in &module.syntax.rules {
            let weights = normalize_static_weights(rule)?;
            let mut productions = Vec::with_capacity(rule.productions.len());
            for (production, weight) in rule.productions.iter().zip(weights) {
                if !production.clauses.is_empty() {
                    let span = match &production.clauses[0] {
                        ClauseSyntax::Guard(guard) => guard.span(),
                        ClauseSyntax::Binding(binding) => binding.span,
                    };
                    return Err(source_error(
                        DiagnosticCode::UNSUPPORTED_FEATURE,
                        span,
                        "guards and bindings are implemented in Milestone 5",
                    ));
                }
                let parts =
                    lower_body(package, &modules, &offsets, module_index, &production.body)?;
                productions.push(CompiledProduction { weight, parts });
            }
            rules.push(CompiledRule {
                name: format!(
                    "{}.{}",
                    module.syntax.front_matter.module().value(),
                    rule.name.value()
                ),
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

    if let Some(rule) = modules
        .iter()
        .flat_map(|module| module.syntax.rules.iter())
        .find(|rule| !rule.parameters.is_empty())
    {
        return Err(source_error(
            DiagnosticCode::UNSUPPORTED_FEATURE,
            rule.span,
            "typed parameter execution is implemented in Milestone 5",
        ));
    }

    let compiled_entries = compile_entries(&modules, &offsets, root_module)?;
    let entries = compiled_entries.entries;
    let default_entry = compiled_entries.default_entry;
    analyze_graph(&mut rules, &entries)?;
    let warnings = recursion_warnings(&rules);
    Ok(CompiledGrammar {
        rules,
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

fn lower_body(
    package: &PackageInput,
    modules: &[ModuleBuild<'_>],
    offsets: &[usize],
    module: usize,
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
            module,
            block
                .parts
                .as_ref()
                .expect("cooked blocks have parsed parts"),
        ),
        BodySyntax::Parts(parts) => lower_parts(package, modules, offsets, module, parts),
    }
}

fn lower_parts(
    package: &PackageInput,
    modules: &[ModuleBuild<'_>],
    offsets: &[usize],
    module: usize,
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
                lowered.push(CompiledPart::Rule(resolve_rule(
                    package,
                    modules,
                    offsets,
                    module,
                    reference.value(),
                    reference.span(),
                )?));
            }
            BodyPartSyntax::RuleCall(call) => {
                let target = resolve_rule(
                    package,
                    modules,
                    offsets,
                    module,
                    call.target.value(),
                    call.target.span(),
                )?;
                validate_call_arity(modules, offsets, target, call)?;
                if !call.arguments.is_empty() {
                    return Err(source_error(
                        DiagnosticCode::UNSUPPORTED_FEATURE,
                        call.span,
                        "typed call arguments are implemented in Milestone 5",
                    ));
                }
                lowered.push(CompiledPart::Rule(target));
            }
            BodyPartSyntax::EmittingCapture { span, .. } => {
                return Err(source_error(
                    DiagnosticCode::UNSUPPORTED_FEATURE,
                    *span,
                    "captures and runtime values are implemented in Milestone 5",
                ));
            }
            BodyPartSyntax::ValueReference(reference) => {
                return Err(source_error(
                    DiagnosticCode::UNSUPPORTED_FEATURE,
                    reference.span(),
                    "captures and runtime values are implemented in Milestone 5",
                ));
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

fn validate_call_arity(
    modules: &[ModuleBuild<'_>],
    offsets: &[usize],
    target: usize,
    call: &crate::CallSyntax,
) -> MecoResult<()> {
    let (module, local) = locate_rule(offsets, target);
    let parameters = &modules[module].syntax.rules[local].parameters;
    if parameters.len() != call.arguments.len()
        || parameters.iter().any(|parameter| {
            !call
                .arguments
                .iter()
                .any(|argument| argument.name.value() == parameter.name.value())
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
    Ok(())
}

fn locate_rule(offsets: &[usize], target: usize) -> (usize, usize) {
    let module = offsets
        .iter()
        .enumerate()
        .rev()
        .find(|(_, offset)| **offset <= target)
        .map(|(index, _)| index)
        .expect("global rule has a module");
    (module, target - offsets[module])
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

fn normalize_static_weights(rule: &crate::RuleSyntax) -> MecoResult<Vec<u64>> {
    let mut rationals = Vec::with_capacity(rule.productions.len());
    for production in &rule.productions {
        rationals.push(match &production.weight {
            WeightSyntax::Default => Rational::ONE,
            WeightSyntax::Static(weight) => *weight.value(),
            WeightSyntax::Dynamic(weight) => {
                return Err(source_error(
                    DiagnosticCode::UNSUPPORTED_FEATURE,
                    weight.span(),
                    "dynamic weights are implemented in Milestone 5",
                ));
            }
        });
    }
    let mut common_denominator = 1_u128;
    for rational in &rationals {
        let denominator = u128::from(rational.denominator());
        let divisor = gcd(common_denominator, denominator);
        common_denominator = common_denominator
            .checked_div(divisor)
            .and_then(|value| value.checked_mul(denominator))
            .ok_or_else(|| weight_overflow(rule.span))?;
    }
    let mut scaled = Vec::with_capacity(rationals.len());
    for rational in rationals {
        let numerator = u128::try_from(rational.numerator())
            .expect("parser accepted only positive static weights");
        scaled.push(
            numerator
                .checked_mul(common_denominator / u128::from(rational.denominator()))
                .ok_or_else(|| weight_overflow(rule.span))?,
        );
    }
    let divisor = scaled.iter().copied().reduce(gcd).unwrap_or(1);
    let mut total = 0_u128;
    let mut normalized = Vec::with_capacity(scaled.len());
    for value in scaled {
        let value = value / divisor;
        total = total
            .checked_add(value)
            .ok_or_else(|| weight_overflow(rule.span))?;
        normalized.push(value);
    }
    if total > i64::MAX as u128 {
        return Err(weight_overflow(rule.span));
    }
    normalized
        .into_iter()
        .map(|value| u64::try_from(value).map_err(|_| weight_overflow(rule.span)))
        .collect()
}

fn weight_overflow(span: Span) -> MecoError {
    source_error(
        DiagnosticCode::WEIGHT_OVERFLOW,
        span,
        "a rule's scaled static weight total exceeds 2^63 - 1",
    )
}

const fn gcd(mut left: u128, mut right: u128) -> u128 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

fn select_production(productions: &[CompiledProduction], choice: u64) -> usize {
    let mut cursor = choice;
    for (index, production) in productions.iter().enumerate() {
        if cursor < production.weight {
            return index;
        }
        cursor -= production.weight;
    }
    unreachable!("choice is strictly below the checked weight total")
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
                .flat_map(|production| production.parts.iter())
                .filter_map(|part| match part {
                    CompiledPart::Rule(rule) => Some(*rule),
                    CompiledPart::Literal(_) => None,
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
    solve_monotone_rule_property(rules, |_| true)
}

fn nullable_rules(rules: &[CompiledRule], _edges: &[Vec<usize>]) -> Vec<bool> {
    solve_monotone_rule_property(rules, |production| {
        production.parts.iter().all(|part| match part {
            CompiledPart::Literal(text) => text.is_empty(),
            CompiledPart::Rule(_) => true,
        })
    })
}

fn solve_monotone_rule_property(
    rules: &[CompiledRule],
    candidate: impl Fn(&CompiledProduction) -> bool,
) -> Vec<bool> {
    let mut property = vec![false; rules.len()];
    let mut remaining = rules
        .iter()
        .map(|rule| {
            rule.productions
                .iter()
                .map(|production| {
                    if candidate(production) {
                        production
                            .parts
                            .iter()
                            .filter(|part| matches!(part, CompiledPart::Rule(_)))
                            .count()
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
            for part in &compiled_production.parts {
                if let CompiledPart::Rule(child) = part {
                    reverse[*child].push((rule, production));
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
        let total = rule
            .productions
            .iter()
            .map(|production| u128::from(production.weight))
            .sum::<u128>();
        let offspring = rule
            .productions
            .iter()
            .map(|production| {
                let count = production
                    .parts
                    .iter()
                    .filter(|part| {
                        matches!(part, CompiledPart::Rule(child)
                            if components[*child] == components[rule_index])
                    })
                    .count() as u128;
                u128::from(production.weight) * count
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
    use alloc::{format, string::ToString, vec};

    use super::{GenerationLimits, GenerationRequest, compile_package};
    use crate::{DiagnosticCode, PackageInput, PackageSource, SourceFile, SourceId};

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
