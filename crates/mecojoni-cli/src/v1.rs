use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

/// Frozen `meco/1` terminal/reference representation used only by migration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum V1Part {
    Terminal(String),
    Reference(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct V1Production {
    pub weight: Option<String>,
    pub parts: Vec<V1Part>,
    pub line: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct V1Rule {
    pub name: String,
    pub productions: Vec<V1Production>,
    pub line: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum V1Item {
    Comment { text: String, line: u32 },
    Rule(V1Rule),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct V1Document {
    pub start: String,
    pub rules: Vec<V1Rule>,
    items: Vec<V1Item>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationDiagnostic {
    pub code: &'static str,
    pub line: u32,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationReport {
    pub source: String,
    pub diagnostics: Vec<MigrationDiagnostic>,
    /// Behavioral contracts that cannot be preserved by a source rewrite alone.
    pub differences: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct V1Error {
    pub diagnostics: Vec<MigrationDiagnostic>,
}

impl std::fmt::Display for V1Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "invalid Mecojoni v1 source")
    }
}

impl std::error::Error for V1Error {}

/// Parses the frozen dependency-free v1 syntax, including its trim and `@@` rules.
///
/// # Errors
///
/// Returns all independently detectable v1 source diagnostics.
#[allow(clippy::too_many_lines)]
pub fn parse_v1(source: &str) -> Result<V1Document, V1Error> {
    let normalized = source.replace("\r\n", "\n");
    let mut diagnostics = Vec::new();
    let mut version = None;
    let mut start = None;
    let mut rules = Vec::<V1Rule>::new();
    let mut items = Vec::<V1Item>::new();
    let mut current = None::<usize>;

    for (index, original) in normalized.split('\n').enumerate() {
        let line = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let trimmed_end = original.trim_end();
        let text = trimmed_end.trim_start();
        if text.is_empty() {
            continue;
        }
        if let Some(comment) = text.strip_prefix("//") {
            items.push(V1Item::Comment {
                text: comment.trim_start().to_string(),
                line,
            });
            continue;
        }
        if let Some(value) = strip_marker_whitespace(text, "@meco") {
            if version.replace(value.to_string()).is_some() {
                push_error(
                    &mut diagnostics,
                    line,
                    "V1_DUPLICATE_DIRECTIVE",
                    "@meco appears more than once",
                );
            }
            current = None;
            continue;
        }
        if let Some(value) = strip_marker_whitespace(text, "@start") {
            if !valid_identifier(value) {
                push_error(
                    &mut diagnostics,
                    line,
                    "V1_START",
                    "@start requires a valid rule name",
                );
            } else if start.replace(value.to_string()).is_some() {
                push_error(
                    &mut diagnostics,
                    line,
                    "V1_DUPLICATE_DIRECTIVE",
                    "@start appears more than once",
                );
            }
            current = None;
            continue;
        }
        if let Some(name) = strip_marker_whitespace(text, "#") {
            let name = name.trim_end();
            if !valid_identifier(name) || name == "empty" {
                push_error(
                    &mut diagnostics,
                    line,
                    "V1_RULE",
                    "invalid or reserved v1 rule name",
                );
                current = None;
                continue;
            }
            if rules.iter().any(|rule| rule.name == name) {
                push_error(
                    &mut diagnostics,
                    line,
                    "V1_DUPLICATE_RULE",
                    "duplicate v1 rule",
                );
                current = None;
                continue;
            }
            rules.push(V1Rule {
                name: name.to_string(),
                productions: Vec::new(),
                line,
            });
            current = Some(rules.len() - 1);
            continue;
        }
        if let Some(after_dash) = text.strip_prefix('-') {
            let Some(rule_index) = current else {
                push_error(
                    &mut diagnostics,
                    line,
                    "V1_PRODUCTION",
                    "production appears outside a rule",
                );
                continue;
            };
            let body = after_dash
                .strip_prefix(char::is_whitespace)
                .unwrap_or(after_dash);
            let (weight, body) = split_weight(body);
            if weight.as_ref().is_some_and(|value| {
                value
                    .parse::<f64>()
                    .map_or(true, |number| !number.is_finite() || number <= 0.0)
            }) {
                push_error(
                    &mut diagnostics,
                    line,
                    "V1_WEIGHT",
                    "production weight must be finite and greater than zero",
                );
                continue;
            }
            match parse_parts(body, line) {
                Ok(parts) => rules[rule_index].productions.push(V1Production {
                    weight,
                    parts,
                    line,
                }),
                Err(diagnostic) => diagnostics.push(diagnostic),
            }
            continue;
        }
        push_error(
            &mut diagnostics,
            line,
            "V1_SYNTAX",
            "expected @meco, @start, # rule, or - production",
        );
    }

    if version.as_deref() != Some("1") {
        push_error(
            &mut diagnostics,
            1,
            "V1_VERSION",
            "source must declare exactly @meco 1",
        );
    }
    let start = start.unwrap_or_else(|| {
        push_error(&mut diagnostics, 1, "V1_START", "source is missing @start");
        String::new()
    });
    let names = rules
        .iter()
        .map(|rule| rule.name.as_str())
        .collect::<BTreeSet<_>>();
    if !start.is_empty() && !names.contains(start.as_str()) {
        push_error(&mut diagnostics, 1, "V1_START", "start rule is not defined");
    }
    for rule in &rules {
        if rule.productions.is_empty() {
            push_error(
                &mut diagnostics,
                rule.line,
                "V1_EMPTY_RULE",
                "rule has no productions",
            );
        }
        for production in &rule.productions {
            for part in &production.parts {
                if let V1Part::Reference(name) = part {
                    if names.contains(name.as_str()) {
                        continue;
                    }
                    push_error(
                        &mut diagnostics,
                        production.line,
                        "V1_UNDEFINED_RULE",
                        &format!("reference @{name} is not defined"),
                    );
                }
            }
        }
    }
    let mut productive = BTreeSet::<String>::new();
    loop {
        let before = productive.len();
        for rule in &rules {
            if rule.productions.iter().any(|production| {
                production.parts.iter().all(|part| match part {
                    V1Part::Terminal(_) => true,
                    V1Part::Reference(name) => productive.contains(name),
                })
            }) {
                productive.insert(rule.name.clone());
            }
        }
        if productive.len() == before {
            break;
        }
    }
    if !start.is_empty() && names.contains(start.as_str()) {
        let by_name = rules
            .iter()
            .map(|rule| (rule.name.as_str(), rule))
            .collect::<BTreeMap<_, _>>();
        let mut reachable = BTreeSet::from([start.as_str()]);
        let mut pending = vec![start.as_str()];
        while let Some(name) = pending.pop() {
            if let Some(rule) = by_name.get(name) {
                for reference in rule
                    .productions
                    .iter()
                    .flat_map(|production| production.parts.iter())
                    .filter_map(|part| match part {
                        V1Part::Reference(reference) => Some(reference.as_str()),
                        V1Part::Terminal(_) => None,
                    })
                {
                    if reachable.insert(reference) {
                        pending.push(reference);
                    }
                }
            }
        }
        for name in reachable {
            if !productive.contains(name) {
                let line = by_name.get(name).map_or(1, |rule| rule.line);
                push_error(
                    &mut diagnostics,
                    line,
                    "V1_UNPRODUCTIVE_RULE",
                    &format!("reachable rule # {name} can never finish"),
                );
            }
        }
    }
    if !diagnostics.is_empty() {
        return Err(V1Error { diagnostics });
    }

    let by_line = rules
        .iter()
        .cloned()
        .map(|rule| (rule.line, rule))
        .collect::<BTreeMap<_, _>>();
    for rule in by_line.into_values() {
        items.push(V1Item::Rule(rule));
    }
    items.sort_by_key(|item| match item {
        V1Item::Comment { line, .. } => *line,
        V1Item::Rule(rule) => rule.line,
    });
    Ok(V1Document {
        start,
        rules,
        items,
    })
}

/// Rewrites a valid v1 document into explicit v2 syntax and reports every known
/// semantic or lexical migration hazard.
///
/// # Errors
///
/// Returns frozen-reader diagnostics when the v1 source itself is invalid.
pub fn migrate_v1(source: &str, module_hint: &str) -> Result<MigrationReport, V1Error> {
    let document = parse_v1(source)?;
    let module = module_identifier(module_hint);
    let mut diagnostics = migration_diagnostics(source);
    let mut output = format!(
        "---\nmeco: 2\nmodule: {module}\nentry: {}\nsampler: diverse/1\nexports: [{}]\n---\n",
        document.start, document.start
    );
    for item in &document.items {
        match item {
            V1Item::Comment { text, line } => {
                let safe = text.replace("--", "- -");
                writeln!(output, "\n<!-- {safe} -->").expect("string formatting cannot fail");
                diagnostics.push(MigrationDiagnostic {
                    code: "M_COMMENT_REWRITE",
                    line: *line,
                    message: "rewrote a v1 // comment as a v2 block comment".to_string(),
                });
            }
            V1Item::Rule(rule) => {
                writeln!(output, "\n# {}", rule.name).expect("string formatting cannot fail");
                for production in &rule.productions {
                    output.push_str("- ");
                    if let Some(weight) = &production.weight {
                        write!(output, "[{weight}] ").expect("string formatting cannot fail");
                    }
                    output.push_str(&render_parts(&production.parts));
                    output.push('\n');
                }
            }
        }
    }
    diagnostics.sort_by_key(|diagnostic| (diagnostic.line, diagnostic.code));
    Ok(MigrationReport {
        source: output,
        diagnostics,
        differences: vec![
            "v1 string-seed/mulberry32 streams and v2 u64/splitmix64 streams are not sequence-compatible"
                .to_string(),
            "diverse/1 preserves the purpose of v1 varied selection, not its exact candidate scores or history"
                .to_string(),
            "stable production IDs are derived anew unless authors add explicit v2 id fields".to_string(),
        ],
    })
}

fn split_weight(body: &str) -> (Option<String>, &str) {
    let Some(rest) = body.strip_prefix('[') else {
        return (None, body);
    };
    let Some(close) = rest.find(']') else {
        return (None, body);
    };
    let candidate = &rest[..close];
    let valid = !candidate.is_empty()
        && candidate.chars().enumerate().all(|(index, character)| {
            character.is_ascii_digit() || (character == '.' && index > 0)
        })
        && candidate.matches('.').count() <= 1
        && !candidate.ends_with('.');
    if !valid {
        return (None, body);
    }
    let after = &rest[close + 1..];
    let after = after.strip_prefix(char::is_whitespace).unwrap_or(after);
    (Some(candidate.to_string()), after)
}

fn strip_marker_whitespace<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    let rest = text.strip_prefix(marker)?;
    let first = rest.chars().next()?;
    if !first.is_whitespace() {
        return None;
    }
    Some(rest[first.len_utf8()..].trim_start())
}

fn parse_parts(body: &str, line: u32) -> Result<Vec<V1Part>, MigrationDiagnostic> {
    if body == "ε" {
        return Ok(Vec::new());
    }
    if body.is_empty() {
        return Err(MigrationDiagnostic {
            code: "V1_EMPTY_PRODUCTION",
            line,
            message: "v1 empty production must use @empty or ε".to_string(),
        });
    }
    let mut parts = Vec::new();
    let mut terminal = String::new();
    let mut cursor = 0;
    while cursor < body.len() {
        let rest = &body[cursor..];
        if !rest.starts_with('@') {
            let character = rest.chars().next().expect("cursor is before end");
            terminal.push(character);
            cursor += character.len_utf8();
            continue;
        }
        if rest.starts_with("@@") {
            terminal.push('@');
            cursor += 2;
            continue;
        }
        let name_len = rest[1..]
            .char_indices()
            .take_while(|(index, character)| {
                if *index == 0 {
                    character.is_ascii_alphabetic()
                } else {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
                }
            })
            .map(|(index, character)| index + character.len_utf8())
            .last()
            .unwrap_or(0);
        if name_len == 0 {
            return Err(MigrationDiagnostic {
                code: "V1_REFERENCE",
                line,
                message: "@ must precede a v1 rule name or be doubled as @@".to_string(),
            });
        }
        if !terminal.is_empty() {
            parts.push(V1Part::Terminal(std::mem::take(&mut terminal)));
        }
        let name = &rest[1..=name_len];
        if name != "empty" {
            parts.push(V1Part::Reference(name.to_string()));
        }
        cursor += 1 + name_len;
    }
    if !terminal.is_empty() {
        parts.push(V1Part::Terminal(terminal));
    }
    Ok(parts)
}

fn render_parts(parts: &[V1Part]) -> String {
    if parts.is_empty() {
        return "\"\"".to_string();
    }
    let mut rendered = String::new();
    for part in parts {
        match part {
            V1Part::Terminal(text) => rendered.push_str(&quoted_terminal(text)),
            V1Part::Reference(name) => {
                write!(rendered, "@{{{name}}}").expect("string formatting cannot fail");
            }
        }
    }
    rendered
}

fn quoted_terminal(text: &str) -> String {
    if !text.contains('"') && !text.contains('\r') && !text.contains('\n') {
        return format!("r\"{text}\"");
    }
    let mut value = String::from("\"");
    for character in text.chars() {
        match character {
            '\\' => value.push_str("\\\\"),
            '"' => value.push_str("\\\""),
            '\n' => value.push_str("\\n"),
            '\r' => value.push_str("\\r"),
            '\t' => value.push_str("\\t"),
            other => value.push(other),
        }
    }
    value.push('"');
    value
}

fn migration_diagnostics(source: &str) -> Vec<MigrationDiagnostic> {
    let mut diagnostics = Vec::new();
    for (index, original) in source.replace("\r\n", "\n").split('\n').enumerate() {
        let line = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let trimmed = original.trim_start();
        if trimmed.starts_with('-') && (original != original.trim_end() || original != trimmed) {
            diagnostics.push(MigrationDiagnostic {
                code: "M_AMBIGUOUS_WHITESPACE",
                line,
                message: "v1 discarded indentation or trailing whitespace; migrated output follows v1 runtime text"
                    .to_string(),
            });
        }
        if trimmed.starts_with('-') {
            let body = trimmed.strip_prefix('-').unwrap_or_default().trim_start();
            let (_, after_weight) = split_weight(body);
            if body.starts_with('[') && after_weight == body {
                diagnostics.push(MigrationDiagnostic {
                    code: "M_WEIGHT_LOOKING_PROSE",
                    line,
                    message: "leading bracket text was prose in v1 and is quoted explicitly in v2"
                        .to_string(),
                });
            }
            if body == "@empty" || body == "ε" || body.contains("@empty") {
                diagnostics.push(MigrationDiagnostic {
                    code: "M_EMPTY_REWRITE",
                    line,
                    message: "rewrote v1 empty output to the explicit v2 empty literal".to_string(),
                });
            }
            if body.contains("@@") || body.contains('$') || body.contains('&') {
                diagnostics.push(MigrationDiagnostic {
                    code: "M_SIGIL_REWRITE",
                    line,
                    message: "quoted terminal text whose sigils acquire syntax in v2".to_string(),
                });
            }
        }
    }
    diagnostics
}

fn module_identifier(hint: &str) -> String {
    let mut value = String::new();
    for character in hint.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
            value.push(character.to_ascii_lowercase());
        } else if !value.ends_with('-') {
            value.push('-');
        }
    }
    while value.ends_with('-') {
        value.pop();
    }
    if value
        .chars()
        .next()
        .is_none_or(|first| !first.is_ascii_alphabetic())
    {
        value.insert_str(0, "migrated-");
    }
    if value.is_empty() {
        "migrated".to_string()
    } else {
        value
    }
}

fn valid_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && characters
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

fn push_error(
    diagnostics: &mut Vec<MigrationDiagnostic>,
    line: u32,
    code: &'static str,
    message: &str,
) {
    diagnostics.push(MigrationDiagnostic {
        code,
        line,
        message: message.to_string(),
    });
}

#[cfg(test)]
mod tests {
    use super::{migrate_v1, parse_v1};
    use mecojoni_core::{PackageInput, PackageSource, SourceFile, SourceId, compile_package};

    #[test]
    fn frozen_reader_and_migrator_rewrite_every_changed_sigil_explicitly() {
        let source = "@meco 1\n@start line\n// note\n# line\n- [2] @name@@host $5 & tea\n- @empty\n# name\n- Ada\n";
        let document = parse_v1(source).unwrap();
        assert_eq!(document.start, "line");
        let report = migrate_v1(source, "NPC file").unwrap();
        assert!(report.source.contains("module: npc-file"));
        assert!(report.source.contains("@{name}r\"@host $5 & tea\""));
        assert!(report.source.contains("- \"\""));
        assert!(
            report
                .diagnostics
                .iter()
                .any(|item| item.code == "M_SIGIL_REWRITE")
        );
        let package = PackageInput {
            root_id: "root".to_string(),
            modules: vec![PackageSource {
                canonical_id: "root".to_string(),
                source: SourceFile::new(SourceId::new(0), "migrated.meco", report.source),
                resolved_imports: vec![],
            }],
        };
        compile_package(&package).expect("migrated v1 source compiles as v2");
    }

    #[test]
    fn invalid_v1_source_aggregates_independent_diagnostics() {
        let error = parse_v1("@meco 3\n@start missing\n# empty\n- \n").unwrap_err();
        assert!(error.diagnostics.len() >= 3);
    }
}
