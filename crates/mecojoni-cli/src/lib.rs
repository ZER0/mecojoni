#![forbid(unsafe_code)]

mod format;
mod loader;
mod v1;

pub use format::format_source;
pub use v1::{
    MigrationDiagnostic, MigrationReport, V1Document, V1Error, V1Part, V1Production, V1Rule,
    migrate_v1, parse_v1,
};

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
    time::Instant,
};

use loader::{format_meco_error, load_package};
use mecojoni_core::{
    CompiledGrammar, DataBinding, Diagnostic, GenerationLimits, GenerationRequest, MecoError,
    MessageArgument, MessageDefinition, MessageManifest, PackageManifest, Rational, SchemaType,
    Severity, Value, audit_rendered_repetition, audit_structural_repetition, compile_package,
    compile_package_with_manifest,
};

pub const CLI_VERSION: &str = "cli/1";

type CliResult<T> = Result<T, CliError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExitClass {
    Domain = 1,
    Usage = 2,
    Internal = 3,
}

#[derive(Debug)]
struct CliError {
    class: ExitClass,
    message: String,
}

impl CliError {
    #[allow(clippy::needless_pass_by_value)]
    fn domain(error: MecoError) -> Self {
        Self {
            class: ExitClass::Domain,
            message: format_meco_error(&error, None),
        }
    }

    fn domain_message(message: impl Into<String>) -> Self {
        Self {
            class: ExitClass::Domain,
            message: message.into(),
        }
    }

    fn usage(message: impl Into<String>) -> Self {
        Self {
            class: ExitClass::Usage,
            message: message.into(),
        }
    }

    fn io(path: &Path, error: &std::io::Error) -> Self {
        Self::usage(format!("{}: {error}", path.display()))
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            class: ExitClass::Internal,
            message: message.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum OutputMode {
    #[default]
    Text,
    Jsonl,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Command {
    Check,
    Generate,
    Trace,
    Lint,
    Audit,
    Manifest,
    Migrate,
    Format,
    Bench,
}

#[derive(Debug, Default)]
struct Options {
    path: Option<PathBuf>,
    output: OutputMode,
    entry: Option<String>,
    seed: u64,
    count: u32,
    samples: u32,
    trace: bool,
    deny_warnings: bool,
    write: Option<PathBuf>,
    messages: Option<PathBuf>,
    data: Vec<(String, String)>,
}

/// Runs the dependency-free authoring CLI against supplied streams.
///
/// This is public so integration hosts can exercise exactly the same stream and
/// status contract without subprocess-specific global state.
pub fn run<I, O, E>(arguments: I, stdout: &mut O, stderr: &mut E) -> i32
where
    I: IntoIterator<Item = OsString>,
    O: Write,
    E: Write,
{
    match run_inner(arguments, stdout, stderr) {
        Ok(status) => status,
        Err(error) => {
            let _ = writeln!(stderr, "{}", error.message);
            error.class as i32
        }
    }
}

fn run_inner<I, O, E>(arguments: I, stdout: &mut O, stderr: &mut E) -> CliResult<i32>
where
    I: IntoIterator<Item = OsString>,
    O: Write,
    E: Write,
{
    let arguments = arguments
        .into_iter()
        .map(|argument| {
            argument
                .into_string()
                .map_err(|_| CliError::usage("arguments must be valid UTF-8"))
        })
        .collect::<CliResult<Vec<_>>>()?;
    if cfg!(debug_assertions) && std::env::var_os("MECO_TEST_INTERNAL_ERROR").is_some() {
        return Err(CliError::internal("simulated internal failure"));
    }
    if arguments.is_empty() || arguments == ["--help"] || arguments == ["-h"] {
        stdout
            .write_all(usage().as_bytes())
            .map_err(|error| CliError::usage(format!("stdout: {error}")))?;
        return Ok(0);
    }
    if arguments
        .iter()
        .skip(1)
        .any(|argument| matches!(argument.as_str(), "--help" | "-h"))
    {
        stdout
            .write_all(usage().as_bytes())
            .map_err(|error| CliError::usage(format!("stdout: {error}")))?;
        return Ok(0);
    }
    let command = parse_command(&arguments[0])?;
    let options = parse_options(command, &arguments[1..])?;
    execute(command, &options, stdout, stderr)
}

fn execute<O: Write, E: Write>(
    command: Command,
    options: &Options,
    stdout: &mut O,
    stderr: &mut E,
) -> CliResult<i32> {
    match command {
        Command::Migrate => migrate_command(options, stdout, stderr),
        Command::Format => format_command(options, stdout),
        _ => package_command(command, options, stdout, stderr),
    }
}

fn package_command<O: Write, E: Write>(
    command: Command,
    options: &Options,
    stdout: &mut O,
    stderr: &mut E,
) -> CliResult<i32> {
    let path = required_path(options)?;
    let package = load_package(path)?;
    let message_manifest = options
        .messages
        .as_deref()
        .map(read_message_manifest)
        .transpose()?;
    let compiled = if let Some(manifest) = &message_manifest {
        compile_package_with_manifest(&package.input, manifest)
    } else {
        compile_package(&package.input)
    };
    let grammar = compiled.map_err(|error| CliError {
        class: ExitClass::Domain,
        message: format_meco_error(&error, Some(&package)),
    })?;
    if options.output == OutputMode::Text {
        emit_compiler_warnings(&grammar, stderr)?;
    }
    match command {
        Command::Check => report_check(&grammar, options, stdout),
        Command::Generate | Command::Trace => generate(&grammar, options, stdout, stderr),
        Command::Lint => lint(&grammar, options, stdout, stderr),
        Command::Audit => audit(&grammar, options, stdout),
        Command::Manifest => manifest(&grammar, options, stdout),
        Command::Bench => bench(&grammar, options, stdout),
        Command::Migrate | Command::Format => {
            Err(CliError::internal("invalid package command dispatch"))
        }
    }
}

fn report_check<O: Write>(
    grammar: &CompiledGrammar,
    options: &Options,
    stdout: &mut O,
) -> CliResult<i32> {
    if options.output == OutputMode::Jsonl {
        writeln!(
            stdout,
            "{{\"cli\":\"{CLI_VERSION}\",\"kind\":\"check\",\"rules\":{},\"entries\":{},\"diagnostics\":{}}}",
            grammar.rule_count(),
            grammar.entries().count(),
            diagnostics_json(grammar.warnings())
        )
    } else {
        writeln!(
            stdout,
            "check: ok ({} rules, {} entries)",
            grammar.rule_count(),
            grammar.entries().count()
        )
    }
    .map_err(output_error)?;
    Ok(warning_status(grammar.warnings(), options.deny_warnings))
}

fn generate<O: Write, E: Write>(
    grammar: &CompiledGrammar,
    options: &Options,
    stdout: &mut O,
    stderr: &mut E,
) -> CliResult<i32> {
    let data = parse_data(&grammar.manifest(), &options.data)?;
    let mut results = Vec::new();
    for index in 0..options.count.max(1) {
        let request = GenerationRequest {
            entry: options.entry.as_deref(),
            seed: options.seed.wrapping_add(u64::from(index)),
            limits: GenerationLimits::default(),
            data: &data,
            trace_bindings: options.trace,
            trace_selections: options.trace,
            trace_provenance: options.trace,
        };
        results.push(
            grammar
                .generate_weighted(&request)
                .map_err(CliError::domain)?,
        );
    }
    for (index, result) in results.iter().enumerate() {
        if options.output == OutputMode::Jsonl {
            write!(
                stdout,
                "{{\"cli\":\"{CLI_VERSION}\",\"kind\":\"generation\",\"index\":{},\"text\":{},\"entry\":{},\"expansions\":{},\"samplerWords\":{},\"diagnostics\":{}",
                index + 1,
                json_string(result.text()),
                json_string(result.entry()),
                result.expansions(),
                result.sampler_words(),
                diagnostics_json(grammar.warnings())
            )
            .map_err(output_error)?;
            if options.trace {
                write!(stdout, ",\"selections\":[").map_err(output_error)?;
                for (selection_index, selection) in result.selections().iter().enumerate() {
                    if selection_index > 0 {
                        write!(stdout, ",").map_err(output_error)?;
                    }
                    write!(
                        stdout,
                        "{{\"rule\":{},\"productionId\":{}}}",
                        json_string(selection.rule()),
                        json_string(selection.selected_production_id())
                    )
                    .map_err(output_error)?;
                }
                write!(stdout, "]").map_err(output_error)?;
            }
            writeln!(stdout, "}}").map_err(output_error)?;
        } else {
            writeln!(stdout, "{}", result.text()).map_err(output_error)?;
            if options.trace {
                writeln!(
                    stderr,
                    "trace {}: {} expansions, {} sampler words",
                    index + 1,
                    result.expansions(),
                    result.sampler_words()
                )
                .map_err(output_error)?;
                for selection in result.selections() {
                    writeln!(
                        stderr,
                        "  {} -> {}",
                        selection.rule(),
                        selection.selected_production_id()
                    )
                    .map_err(output_error)?;
                }
            }
        }
    }
    Ok(0)
}

fn lint<O: Write, E: Write>(
    grammar: &CompiledGrammar,
    options: &Options,
    stdout: &mut O,
    stderr: &mut E,
) -> CliResult<i32> {
    let findings = grammar.audit_composition();
    if options.output == OutputMode::Jsonl {
        writeln!(
            stdout,
            "{{\"cli\":\"{CLI_VERSION}\",\"kind\":\"lint\",\"warnings\":{},\"compositionFindings\":{},\"diagnostics\":{}}}",
            grammar.warnings().len(),
            findings.len(),
            diagnostics_json(grammar.warnings())
        )
        .map_err(output_error)?;
    } else {
        writeln!(
            stdout,
            "lint: {} compiler warnings, {} composition findings",
            grammar.warnings().len(),
            findings.len()
        )
        .map_err(output_error)?;
    }
    if options.output == OutputMode::Text {
        for finding in &findings {
            writeln!(
                stderr,
                "W_COMPOSITION_SHELL: {} production {} ({})",
                finding.rule, finding.production_index, finding.production_id
            )
            .map_err(output_error)?;
        }
    }
    let warning_count = grammar.warnings().len().saturating_add(findings.len());
    Ok(i32::from(options.deny_warnings && warning_count > 0))
}

fn audit<O: Write>(grammar: &CompiledGrammar, options: &Options, stdout: &mut O) -> CliResult<i32> {
    let data = parse_data(&grammar.manifest(), &options.data)?;
    let mut results = Vec::new();
    for index in 0..options.samples.max(2) {
        results.push(
            grammar
                .generate_weighted(&GenerationRequest {
                    entry: options.entry.as_deref(),
                    seed: options.seed.wrapping_add(u64::from(index)),
                    limits: GenerationLimits::default(),
                    data: &data,
                    trace_bindings: false,
                    trace_selections: true,
                    trace_provenance: true,
                })
                .map_err(CliError::domain)?,
        );
    }
    let structural = audit_structural_repetition(&results);
    let rendered = audit_rendered_repetition(&results, 3);
    if options.output == OutputMode::Jsonl {
        writeln!(
            stdout,
            "{{\"cli\":\"{CLI_VERSION}\",\"kind\":\"audit\",\"samples\":{},\"structuralFindings\":{},\"renderedFindings\":{},\"diagnostics\":{}}}",
            results.len(),
            structural.len(),
            rendered.len(),
            diagnostics_json(grammar.warnings())
        )
    } else {
        writeln!(
            stdout,
            "audit: {} samples, {} structural findings, {} rendered findings",
            results.len(),
            structural.len(),
            rendered.len()
        )
    }
    .map_err(output_error)?;
    Ok(0)
}

fn manifest<O: Write>(
    grammar: &CompiledGrammar,
    options: &Options,
    stdout: &mut O,
) -> CliResult<i32> {
    let manifest = grammar.manifest();
    if options.output == OutputMode::Jsonl {
        write!(
            stdout,
            "{{\"cli\":\"{CLI_VERSION}\",\"kind\":\"manifest\",\"inputs\":["
        )
        .map_err(output_error)?;
        for (index, input) in manifest.inputs.iter().enumerate() {
            if index > 0 {
                write!(stdout, ",").map_err(output_error)?;
            }
            write!(
                stdout,
                "{{\"name\":{},\"type\":{}}}",
                json_string(&input.name),
                json_string(schema_name(&input.type_))
            )
            .map_err(output_error)?;
        }
        write!(stdout, "],\"messages\":[").map_err(output_error)?;
        for (index, message) in manifest.messages.messages.iter().enumerate() {
            if index > 0 {
                write!(stdout, ",").map_err(output_error)?;
            }
            write!(
                stdout,
                "{{\"id\":{},\"arguments\":[",
                json_string(&message.id)
            )
            .map_err(output_error)?;
            for (argument_index, argument) in message.arguments.iter().enumerate() {
                if argument_index > 0 {
                    write!(stdout, ",").map_err(output_error)?;
                }
                write!(
                    stdout,
                    "{{\"name\":{},\"type\":{}}}",
                    json_string(&argument.name),
                    json_string(schema_name(&argument.type_))
                )
                .map_err(output_error)?;
            }
            write!(stdout, "]}}").map_err(output_error)?;
        }
        writeln!(
            stdout,
            "],\"diagnostics\":{}}}",
            diagnostics_json(grammar.warnings())
        )
        .map_err(output_error)?;
    } else {
        writeln!(stdout, "manifest: {} inputs", manifest.inputs.len()).map_err(output_error)?;
        for input in &manifest.inputs {
            writeln!(stdout, "  {}: {}", input.name, schema_name(&input.type_))
                .map_err(output_error)?;
        }
        writeln!(stdout, "messages: {}", manifest.messages.messages.len()).map_err(output_error)?;
        for message in &manifest.messages.messages {
            writeln!(stdout, "  &{}", message.id).map_err(output_error)?;
            for argument in &message.arguments {
                writeln!(
                    stdout,
                    "    {}: {}",
                    argument.name,
                    schema_name(&argument.type_)
                )
                .map_err(output_error)?;
            }
        }
    }
    Ok(0)
}

fn bench<O: Write>(grammar: &CompiledGrammar, options: &Options, stdout: &mut O) -> CliResult<i32> {
    let data = parse_data(&grammar.manifest(), &options.data)?;
    let count = options.count.max(1);
    let start = Instant::now();
    let mut expansions = 0_u64;
    let mut sampler_words = 0_u64;
    for index in 0..count {
        let result = grammar
            .generate_weighted(&GenerationRequest {
                entry: options.entry.as_deref(),
                seed: options.seed.wrapping_add(u64::from(index)),
                limits: GenerationLimits::default(),
                data: &data,
                trace_bindings: false,
                trace_selections: false,
                trace_provenance: false,
            })
            .map_err(CliError::domain)?;
        expansions = expansions.saturating_add(u64::from(result.expansions()));
        sampler_words = sampler_words.saturating_add(u64::from(result.sampler_words()));
    }
    let elapsed = start.elapsed().as_nanos();
    if options.output == OutputMode::Jsonl {
        writeln!(
            stdout,
            "{{\"cli\":\"{CLI_VERSION}\",\"kind\":\"bench\",\"generations\":{count},\"expansions\":{expansions},\"samplerWords\":{sampler_words},\"elapsedNs\":{elapsed},\"diagnostics\":{}}}",
            diagnostics_json(grammar.warnings())
        )
    } else {
        writeln!(
            stdout,
            "bench: {count} generations, {expansions} expansions, {sampler_words} sampler words, {elapsed} ns"
        )
    }
    .map_err(output_error)?;
    Ok(0)
}

fn migrate_command<O: Write, E: Write>(
    options: &Options,
    stdout: &mut O,
    stderr: &mut E,
) -> CliResult<i32> {
    let path = required_path(options)?;
    let source = fs::read_to_string(path).map_err(|error| CliError::io(path, &error))?;
    let hint = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("migrated");
    let report = migrate_v1(&source, hint).map_err(|error| {
        CliError::domain_message(
            error
                .diagnostics
                .iter()
                .map(|item| format!("{}:{}: {}", item.code, item.line, item.message))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    })?;
    if options.output == OutputMode::Text {
        for diagnostic in &report.diagnostics {
            writeln!(
                stderr,
                "{}:{}: {}",
                diagnostic.code, diagnostic.line, diagnostic.message
            )
            .map_err(output_error)?;
        }
        for difference in &report.differences {
            writeln!(stderr, "M_BEHAVIOR_CHANGE: {difference}").map_err(output_error)?;
        }
    }
    if let Some(write_path) = &options.write {
        fs::write(write_path, &report.source).map_err(|error| CliError::io(write_path, &error))?;
        if options.output == OutputMode::Jsonl {
            writeln!(
                stdout,
                "{{\"cli\":\"{CLI_VERSION}\",\"kind\":\"migrate\",\"path\":{},\"diagnostics\":{},\"differences\":{}}}",
                json_string(&write_path.display().to_string()),
                migration_diagnostics_json(&report.diagnostics),
                strings_json(&report.differences)
            )
            .map_err(output_error)?;
        } else {
            writeln!(stdout, "migrated {}", write_path.display()).map_err(output_error)?;
        }
    } else if options.output == OutputMode::Jsonl {
        writeln!(
            stdout,
            "{{\"cli\":\"{CLI_VERSION}\",\"kind\":\"migrate\",\"source\":{},\"diagnostics\":{},\"differences\":{}}}",
            json_string(&report.source),
            migration_diagnostics_json(&report.diagnostics),
            strings_json(&report.differences)
        )
        .map_err(output_error)?;
    } else {
        stdout
            .write_all(report.source.as_bytes())
            .map_err(output_error)?;
    }
    Ok(i32::from(
        options.deny_warnings && !report.diagnostics.is_empty(),
    ))
}

fn format_command<O: Write>(options: &Options, stdout: &mut O) -> CliResult<i32> {
    let path = required_path(options)?;
    let source = fs::read_to_string(path).map_err(|error| CliError::io(path, &error))?;
    let formatted =
        format_source(&source, &path.display().to_string()).map_err(CliError::domain)?;
    if let Some(write_path) = &options.write {
        fs::write(write_path, &formatted).map_err(|error| CliError::io(write_path, &error))?;
        if options.output == OutputMode::Jsonl {
            writeln!(
                stdout,
                "{{\"cli\":\"{CLI_VERSION}\",\"kind\":\"format\",\"path\":{},\"changed\":false}}",
                json_string(&write_path.display().to_string())
            )
        } else {
            writeln!(stdout, "format: unchanged ({})", write_path.display())
        }
        .map_err(output_error)?;
    } else if options.output == OutputMode::Jsonl {
        writeln!(
            stdout,
            "{{\"cli\":\"{CLI_VERSION}\",\"kind\":\"format\",\"source\":{},\"changed\":false}}",
            json_string(&formatted)
        )
        .map_err(output_error)?;
    } else {
        stdout
            .write_all(formatted.as_bytes())
            .map_err(output_error)?;
    }
    Ok(0)
}

fn parse_options(command: Command, arguments: &[String]) -> CliResult<Options> {
    let mut options = Options {
        count: 1,
        samples: 100,
        ..Options::default()
    };
    let mut seen = BTreeSet::new();
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        if argument == "--trace" {
            duplicate(&mut seen, "trace")?;
            options.trace = true;
            index += 1;
            continue;
        }
        if argument == "--deny-warnings" {
            duplicate(&mut seen, "deny-warnings")?;
            options.deny_warnings = true;
            index += 1;
            continue;
        }
        if argument == "--help" || argument == "-h" {
            return Err(CliError::usage(format!(
                "command help is available as `meco --help`; unexpected {argument}"
            )));
        }
        if argument.starts_with('-') {
            let (flag, inline) = argument
                .split_once('=')
                .map_or((argument.as_str(), None), |(flag, value)| {
                    (flag, Some(value))
                });
            if flag == "--data" {
                let value = flag_value(flag, inline, arguments, &mut index)?;
                let (name, value) = value
                    .split_once('=')
                    .ok_or_else(|| CliError::usage("--data requires name=value"))?;
                options.data.push((name.to_string(), value.to_string()));
                continue;
            }
            duplicate(&mut seen, flag)?;
            let value = flag_value(flag, inline, arguments, &mut index)?;
            match flag {
                "--output" => {
                    options.output = match value {
                        "text" => OutputMode::Text,
                        "jsonl" => OutputMode::Jsonl,
                        _ => return Err(CliError::usage("--output must be text or jsonl")),
                    };
                }
                "--entry" => options.entry = Some(value.to_string()),
                "--seed" => options.seed = parse_u64(flag, value)?,
                "--count" => options.count = parse_positive_u32(flag, value)?,
                "--samples" => options.samples = parse_positive_u32(flag, value)?,
                "--write" => options.write = Some(PathBuf::from(value)),
                "--messages" => options.messages = Some(PathBuf::from(value)),
                _ => return Err(CliError::usage(format!("unknown option {flag}"))),
            }
            continue;
        }
        if options.path.replace(PathBuf::from(argument)).is_some() {
            return Err(CliError::usage(format!("unexpected argument {argument}")));
        }
        index += 1;
    }
    if command == Command::Trace {
        options.trace = true;
    }
    if options.path.is_none() {
        return Err(CliError::usage("command requires a source path"));
    }
    Ok(options)
}

fn flag_value<'a>(
    flag: &str,
    inline: Option<&'a str>,
    arguments: &'a [String],
    index: &mut usize,
) -> CliResult<&'a str> {
    if let Some(value) = inline {
        if value.is_empty() {
            return Err(CliError::usage(format!("{flag} requires a value")));
        }
        *index += 1;
        return Ok(value);
    }
    let value = arguments
        .get(*index + 1)
        .ok_or_else(|| CliError::usage(format!("{flag} requires a value")))?;
    if value.starts_with('-') {
        return Err(CliError::usage(format!("{flag} requires a value")));
    }
    *index += 2;
    Ok(value)
}

fn parse_command(value: &str) -> CliResult<Command> {
    match value {
        "check" => Ok(Command::Check),
        "generate" => Ok(Command::Generate),
        "trace" => Ok(Command::Trace),
        "lint" => Ok(Command::Lint),
        "audit" => Ok(Command::Audit),
        "manifest" => Ok(Command::Manifest),
        "migrate" => Ok(Command::Migrate),
        "fmt" | "format" => Ok(Command::Format),
        "bench" => Ok(Command::Bench),
        other => Err(CliError::usage(format!("unknown command {other}"))),
    }
}

fn required_path(options: &Options) -> CliResult<&Path> {
    options
        .path
        .as_deref()
        .ok_or_else(|| CliError::usage("command requires a source path"))
}

fn duplicate(seen: &mut BTreeSet<String>, flag: &str) -> CliResult<()> {
    if !seen.insert(flag.to_string()) {
        return Err(CliError::usage(format!("duplicate option {flag}")));
    }
    Ok(())
}

fn parse_positive_u32(flag: &str, value: &str) -> CliResult<u32> {
    let number = value
        .parse::<u32>()
        .map_err(|_| CliError::usage(format!("{flag} requires a positive integer")))?;
    if number == 0 {
        return Err(CliError::usage(format!(
            "{flag} requires a positive integer"
        )));
    }
    Ok(number)
}

fn parse_u64(flag: &str, value: &str) -> CliResult<u64> {
    value
        .parse::<u64>()
        .map_err(|_| CliError::usage(format!("{flag} requires an unsigned 64-bit integer")))
}

fn parse_data(manifest: &PackageManifest, raw: &[(String, String)]) -> CliResult<Vec<DataBinding>> {
    let values = raw.iter().cloned().collect::<BTreeMap<_, _>>();
    if values.len() != raw.len() {
        return Err(CliError::usage("duplicate --data name"));
    }
    for name in values.keys() {
        if !manifest.inputs.iter().any(|input| &input.name == name) {
            return Err(CliError::domain_message(format!("unknown input {name}")));
        }
    }
    manifest
        .inputs
        .iter()
        .map(|input| {
            let raw = values.get(&input.name).ok_or_else(|| {
                CliError::domain_message(format!("missing required input {}", input.name))
            })?;
            let value = match &input.type_ {
                SchemaType::Text => Value::Text(raw.clone()),
                SchemaType::Number => Value::Number(Rational::from_str(raw).map_err(|error| {
                    CliError::domain_message(format!("{}: {error}", input.name))
                })?),
                SchemaType::Boolean => match raw.as_str() {
                    "true" => Value::Boolean(true),
                    "false" => Value::Boolean(false),
                    _ => {
                        return Err(CliError::domain_message(format!(
                            "{} must be true or false",
                            input.name
                        )));
                    }
                },
                SchemaType::Enum(_) => Value::Enum(raw.clone()),
            };
            Ok(DataBinding::new(input.name.clone(), value))
        })
        .collect()
}

fn schema_name(type_: &SchemaType) -> &str {
    match type_ {
        SchemaType::Text => "text",
        SchemaType::Number => "number",
        SchemaType::Boolean => "boolean",
        SchemaType::Enum(name) => name,
    }
}

fn read_message_manifest(path: &Path) -> CliResult<MessageManifest> {
    let source = fs::read_to_string(path).map_err(|error| CliError::io(path, &error))?;
    let mut messages = Vec::new();
    for (index, line) in source.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line_number = index + 1;
        let (id, raw_arguments) = line.split_once('|').ok_or_else(|| {
            CliError::usage(format!(
                "{}:{line_number}: message schema requires id|name:type,...",
                path.display()
            ))
        })?;
        if id.is_empty() {
            return Err(CliError::usage(format!(
                "{}:{line_number}: message ID cannot be empty",
                path.display()
            )));
        }
        let mut arguments = Vec::new();
        if !raw_arguments.is_empty() {
            for raw_argument in raw_arguments.split(',') {
                let (name, type_name) = raw_argument.split_once(':').ok_or_else(|| {
                    CliError::usage(format!(
                        "{}:{line_number}: message argument requires name:type",
                        path.display()
                    ))
                })?;
                let type_ = match type_name {
                    "text" => SchemaType::Text,
                    "number" => SchemaType::Number,
                    "boolean" => SchemaType::Boolean,
                    enum_name if !enum_name.is_empty() => SchemaType::Enum(enum_name.to_string()),
                    _ => {
                        return Err(CliError::usage(format!(
                            "{}:{line_number}: message argument type cannot be empty",
                            path.display()
                        )));
                    }
                };
                arguments.push(MessageArgument {
                    name: name.to_string(),
                    type_,
                });
            }
        }
        messages.push(MessageDefinition {
            id: id.to_string(),
            arguments,
        });
    }
    Ok(MessageManifest { messages })
}

fn emit_compiler_warnings<E: Write>(grammar: &CompiledGrammar, stderr: &mut E) -> CliResult<()> {
    for warning in grammar.warnings() {
        writeln!(stderr, "{}: {}", warning.code().as_str(), warning.message())
            .map_err(output_error)?;
    }
    Ok(())
}

fn warning_status(warnings: &[Diagnostic], deny: bool) -> i32 {
    i32::from(
        deny && warnings
            .iter()
            .any(|item| item.severity() == Severity::Warning),
    )
}

fn diagnostics_json(diagnostics: &[Diagnostic]) -> String {
    let items = diagnostics
        .iter()
        .map(|diagnostic| {
            let severity = match diagnostic.severity() {
                Severity::Error => "error",
                Severity::Warning => "warning",
            };
            format!(
                "{{\"code\":{},\"severity\":{},\"message\":{}}}",
                json_string(diagnostic.code().as_str()),
                json_string(severity),
                json_string(diagnostic.message())
            )
        })
        .collect::<Vec<_>>();
    format!("[{}]", items.join(","))
}

fn migration_diagnostics_json(diagnostics: &[MigrationDiagnostic]) -> String {
    let items = diagnostics
        .iter()
        .map(|diagnostic| {
            format!(
                "{{\"code\":{},\"line\":{},\"message\":{}}}",
                json_string(diagnostic.code),
                diagnostic.line,
                json_string(&diagnostic.message)
            )
        })
        .collect::<Vec<_>>();
    format!("[{}]", items.join(","))
}

fn strings_json(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| json_string(value))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn json_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character <= '\u{1f}' => {
                use std::fmt::Write as _;
                let _ = write!(escaped, "\\u{:04x}", u32::from(character));
            }
            character => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

#[allow(clippy::needless_pass_by_value)]
fn output_error(error: std::io::Error) -> CliError {
    CliError::usage(format!("output stream: {error}"))
}

fn usage() -> &'static str {
    "Usage: meco <command> <source> [options]\n\nCommands:\n  check      Parse, compile, and validate a v2 package\n  generate   Generate deterministic weighted text\n  trace      Generate text with derivation traces\n  lint       Report compiler and composition warnings\n  audit      Sample and report structural/rendered repetition\n  manifest   Export the compiled input/message schema\n  migrate    Rewrite a frozen v1 source as explicit v2\n  fmt        Validate and conservatively format v2 source\n  bench      Measure deterministic local generation work\n\nOptions:\n  --output <text|jsonl>  Output contract (default: text)\n  --entry <rule>         Explicit exported qualified rule\n  --seed <u64>           Deterministic splitmix64 seed\n  --count <n>            Generation/bench count\n  --samples <n>          Audit sample count\n  --data <name=value>    Typed host input (repeatable)\n  --trace                 Include traces\n  --deny-warnings         Return status 1 when warnings occur\n  --write <path>          Write migrate/fmt output\n  --messages <path>       Message schema (id|name:type,...)\n  -h, --help              Show this help\n"
}

#[cfg(test)]
mod tests {
    use super::{json_string, run};
    use std::ffi::OsString;

    #[test]
    fn json_encoder_handles_every_control_and_sigil_without_dependencies() {
        assert_eq!(json_string("a\n\"\\\u{1}"), "\"a\\n\\\"\\\\\\u0001\"");
    }

    #[test]
    fn argument_layer_rejects_missing_duplicate_and_unknown_flags_as_usage() {
        for args in [
            vec!["check", "x", "--entry", "--trace"],
            vec!["check", "x", "--seed=1", "--seed", "2"],
            vec!["check", "x", "--wat", "2"],
        ] {
            let mut out = Vec::new();
            let mut err = Vec::new();
            let status = run(args.into_iter().map(OsString::from), &mut out, &mut err);
            assert_eq!(status, 2);
            assert!(out.is_empty());
            assert!(!err.is_empty());
        }
    }
}
