use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::str::FromStr;

use crate::{
    ArgumentSyntax, BindingSyntax, BlockChomp, BlockSyntax, BodyPartSyntax, BodySyntax, CallSyntax,
    ClauseSyntax, Diagnostic, DiagnosticCode, GuardExpression, GuardValue, MecoError, MecoResult,
    ModuleSyntax, ParameterSyntax, ProductionSyntax, Rational, RuleSyntax, Severity, SourceFile,
    Span, Spanned, ValueSyntax, WeightExpression, WeightSyntax, parse_front_matter,
};

#[derive(Clone, Copy, Debug)]
struct Line<'a> {
    text: &'a str,
    start: usize,
    end: usize,
}

/// Parses one complete format-2 source module into a span-preserving AST.
///
/// # Errors
///
/// Returns structured diagnostics for invalid headers, comments, headings,
/// productions, expressions, references, calls, strings, or blocks.
///
/// # Panics
///
/// Panics only if a [`SourceFile`] violates its own UTF-8 boundary and ordered
/// span invariants, which cannot be constructed through its safe public API.
#[allow(clippy::too_many_lines)]
pub fn parse_module(source: &SourceFile) -> MecoResult<ModuleSyntax> {
    let front_matter = parse_front_matter(source)?;
    let body_start = usize::try_from(front_matter.span().end().byte())
        .expect("source offsets fit the host address space");
    let lines = collect_lines(source.text(), body_start);
    let mut rules = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        if lines[index].text.is_empty() {
            index += 1;
            continue;
        }
        if is_comment_start(lines[index].text) {
            index = skip_comment(source, &lines, index)?;
            continue;
        }
        if !lines[index].text.starts_with("# ") {
            return Err(failure(
                DiagnosticCode::RULE_SYNTAX,
                line_span(source, lines[index]),
                "module body content must belong to a `# rule` heading",
            ));
        }

        let heading = lines[index];
        let (name, parameters) = parse_heading(source, heading)?;
        if rules
            .iter()
            .any(|rule: &RuleSyntax| rule.name.value() == name.value())
        {
            return Err(failure(
                DiagnosticCode::DUPLICATE_RULE,
                name.span(),
                format!("duplicate rule `{}`", name.value()),
            ));
        }
        index += 1;
        let mut productions = Vec::new();

        while index < lines.len() {
            let line = lines[index];
            if line.text.is_empty() {
                index += 1;
                continue;
            }
            if is_comment_start(line.text) {
                index = skip_comment(source, &lines, index)?;
                continue;
            }
            if line.text.starts_with("# ") {
                break;
            }
            if !line.text.starts_with("- ") {
                return Err(failure(
                    DiagnosticCode::PRODUCTION_SYNTAX,
                    line_span(source, line),
                    "a rule body contains only `- production` items",
                ));
            }

            let start = index;
            index += 1;
            while index < lines.len() {
                let continuation = lines[index];
                if continuation.text.starts_with("  ") {
                    index += 1;
                } else {
                    break;
                }
            }
            productions.push(parse_production(source, &lines[start..index])?);
        }

        if productions.is_empty() {
            return Err(failure(
                DiagnosticCode::RULE_SYNTAX,
                line_span(source, heading),
                format!("rule `{}` requires at least one production", name.value()),
            ));
        }
        let end = productions
            .last()
            .expect("non-empty productions")
            .span
            .end();
        rules.push(RuleSyntax {
            name,
            parameters,
            productions,
            span: Span::new(
                source.id(),
                source.position(heading.start).expect("line boundary"),
                end,
            )
            .expect("ordered rule span"),
        });
    }

    if rules.is_empty() {
        return Err(failure(
            DiagnosticCode::RULE_SYNTAX,
            empty_span(source, body_start),
            "a module requires at least one rule",
        ));
    }
    Ok(ModuleSyntax {
        span: span(source, 0, source.len()),
        front_matter,
        rules,
    })
}

fn parse_heading(
    source: &SourceFile,
    line: Line<'_>,
) -> MecoResult<(Spanned<String>, Vec<ParameterSyntax>)> {
    if line.text.ends_with(char::is_whitespace) {
        return Err(failure(
            DiagnosticCode::RULE_SYNTAX,
            line_span(source, line),
            "rule headings cannot end in whitespace",
        ));
    }
    let content = &line.text[2..];
    let (name_source, parameters_source) = content
        .split_once(" <- ")
        .map_or((content, None), |(name, parameters)| {
            (name, Some(parameters))
        });
    let name_span = span(source, line.start + 2, line.start + 2 + name_source.len());
    let name = parsed_identifier(name_source, name_span, false)?;
    let mut parameters = Vec::new();

    if let Some(parameters_source) = parameters_source {
        if parameters_source.is_empty() {
            return Err(failure(
                DiagnosticCode::RULE_SYNTAX,
                line_span(source, line),
                "`<-` in a heading requires typed parameters",
            ));
        }
        let parameters_start = line.end - parameters_source.len();
        let mut relative = 0;
        for raw in parameters_source.split(',') {
            let leading = raw.len() - raw.trim_start_matches(' ').len();
            let item = raw.trim_matches(' ');
            let item_start = parameters_start + relative + leading;
            let Some((parameter_name, type_name)) = item.split_once(": ") else {
                return Err(failure(
                    DiagnosticCode::RULE_SYNTAX,
                    span(source, item_start, item_start + item.len()),
                    "parameters use `name: type` syntax",
                ));
            };
            let parameter_name_span = span(source, item_start, item_start + parameter_name.len());
            let type_start = item_start + parameter_name.len() + 2;
            let type_span = span(source, type_start, type_start + type_name.len());
            let parameter_name = parsed_identifier(parameter_name, parameter_name_span, false)?;
            if parameters
                .iter()
                .any(|parameter: &ParameterSyntax| parameter.name.value() == parameter_name.value())
            {
                return Err(failure(
                    DiagnosticCode::RULE_SYNTAX,
                    parameter_name.span(),
                    format!("duplicate parameter `{}`", parameter_name.value()),
                ));
            }
            parameters.push(ParameterSyntax {
                name: parameter_name,
                type_name: parsed_identifier(type_name, type_span, false)?,
                span: span(source, item_start, item_start + item.len()),
            });
            relative += raw.len() + 1;
        }
    }

    Ok((name, parameters))
}

fn parse_production(source: &SourceFile, lines: &[Line<'_>]) -> MecoResult<ProductionSyntax> {
    let first = lines[0];
    let last = lines.last().copied().expect("production has a first line");
    let production_span = span(source, first.start, last.end);
    let mut content = &first.text[2..];
    let mut content_start = first.start + 2;
    if content.is_empty() {
        return Err(failure(
            DiagnosticCode::PRODUCTION_SYNTAX,
            production_span,
            "a production requires clauses or a visible body",
        ));
    }

    let (weight, authored_id, remainder, remainder_start) =
        parse_weight_prefix(source, content, content_start)?;
    content = remainder;
    content_start = remainder_start;

    let mut logical = Vec::new();
    if !content.is_empty() {
        logical.push((content, content_start, first.end));
    }
    for line in &lines[1..] {
        if line.text.is_empty() {
            logical.push(("", line.start, line.end));
            continue;
        }
        logical.push((&line.text[2..], line.start + 2, line.end));
    }

    let mut clauses = Vec::new();
    let mut body_lines = Vec::new();
    let mut binding_seen = false;
    for (line_text, start, end) in logical {
        if body_lines.is_empty() {
            let (parsed_clauses, remainder, remainder_start) =
                parse_clause_prefixes(source, line_text, start, binding_seen)?;
            for clause in parsed_clauses {
                if matches!(clause, ClauseSyntax::Binding(_)) {
                    binding_seen = true;
                }
                clauses.push(clause);
            }
            if !remainder.is_empty() {
                body_lines.push((remainder, remainder_start, end));
            }
        } else {
            body_lines.push((line_text, start, end));
        }
    }

    if body_lines.is_empty() {
        return Err(failure(
            DiagnosticCode::BODY_SYNTAX,
            production_span,
            "a production requires one visible body after its clauses",
        ));
    }
    let body = parse_body(source, &body_lines)?;
    Ok(ProductionSyntax {
        weight,
        authored_id,
        clauses,
        body,
        span: production_span,
    })
}

fn parse_weight_prefix<'a>(
    source: &SourceFile,
    content: &'a str,
    start: usize,
) -> MecoResult<(WeightSyntax, Option<Spanned<String>>, &'a str, usize)> {
    if !content.starts_with('[') {
        return Ok((WeightSyntax::Default, None, content, start));
    }
    let Some(close) = content.find(']') else {
        return Err(failure(
            DiagnosticCode::WEIGHT_SYNTAX,
            span(source, start, start + content.len()),
            "weight metadata is missing `]`",
        ));
    };
    let metadata = &content[1..close];
    let metadata_span = span(source, start + 1, start + close);
    let (weight, authored_id) = if metadata.contains('=') {
        parse_long_weight(source, metadata, start + 1, metadata_span)?
    } else {
        let value = Rational::from_str(metadata).map_err(|error| {
            failure(
                DiagnosticCode::WEIGHT_SYNTAX,
                metadata_span,
                format!("invalid static weight: {error}"),
            )
        })?;
        if !value.is_positive() {
            return Err(failure(
                DiagnosticCode::WEIGHT_SYNTAX,
                metadata_span,
                "a static weight must be greater than zero",
            ));
        }
        (
            WeightSyntax::Static(Spanned::new(value, metadata_span)),
            None,
        )
    };

    let after = &content[close + 1..];
    if after.is_empty() {
        return Ok((weight, authored_id, "", start + close + 1));
    }
    let Some(remainder) = after.strip_prefix(' ') else {
        return Err(failure(
            DiagnosticCode::WEIGHT_SYNTAX,
            span(source, start + close + 1, start + content.len()),
            "weight metadata is followed by exactly one space",
        ));
    };
    Ok((weight, authored_id, remainder, start + close + 2))
}

fn parse_long_weight(
    source: &SourceFile,
    metadata: &str,
    start: usize,
    metadata_span: Span,
) -> MecoResult<(WeightSyntax, Option<Spanned<String>>)> {
    let mut weight = None;
    let mut authored_id = None;
    let mut relative = 0;
    for raw in metadata.split(',') {
        let item = raw.trim_matches(' ');
        let leading = raw.len() - raw.trim_start_matches(' ').len();
        let item_start = start + relative + leading;
        let Some((key, value)) = item.split_once(" = ") else {
            return Err(failure(
                DiagnosticCode::WEIGHT_SYNTAX,
                span(source, item_start, item_start + item.len()),
                "long weight fields use `name = value` syntax",
            ));
        };
        let value_start = item_start + key.len() + 3;
        let value_span = span(source, value_start, value_start + value.len());
        match key {
            "weight" if weight.is_none() => {
                weight = Some(WeightSyntax::Dynamic(Spanned::new(
                    parse_weight_expression(value).map_err(|message| {
                        failure(DiagnosticCode::WEIGHT_SYNTAX, value_span, message)
                    })?,
                    value_span,
                )));
            }
            "id" if authored_id.is_none() => {
                authored_id = Some(parsed_identifier(value, value_span, false)?);
            }
            "weight" | "id" => {
                return Err(failure(
                    DiagnosticCode::WEIGHT_SYNTAX,
                    value_span,
                    format!("duplicate long weight field `{key}`"),
                ));
            }
            _ => {
                return Err(failure(
                    DiagnosticCode::WEIGHT_SYNTAX,
                    span(source, item_start, item_start + key.len()),
                    format!("unknown long weight field `{key}`"),
                ));
            }
        }
        relative += raw.len() + 1;
    }
    let weight = weight.ok_or_else(|| {
        failure(
            DiagnosticCode::WEIGHT_SYNTAX,
            metadata_span,
            "long weight metadata requires `weight = expression`",
        )
    })?;
    Ok((weight, authored_id))
}

fn parse_clause_prefixes<'a>(
    source: &SourceFile,
    mut text: &'a str,
    mut start: usize,
    binding_already_seen: bool,
) -> MecoResult<(Vec<ClauseSyntax>, &'a str, usize)> {
    let mut clauses = Vec::new();
    let mut binding_seen = binding_already_seen;
    loop {
        while text.starts_with("<!--") {
            let Some(close) = text[4..].find("-->") else {
                return Err(failure(
                    DiagnosticCode::COMMENT_SYNTAX,
                    span(source, start, start + text.len()),
                    "inline comment is missing its closing `-->`",
                ));
            };
            let consumed = 4 + close + 3;
            if text[4..4 + close].contains("<!--") {
                return Err(failure(
                    DiagnosticCode::COMMENT_SYNTAX,
                    span(source, start, start + consumed),
                    "comments cannot nest",
                ));
            }
            let after = &text[consumed..];
            if after.is_empty() {
                return Ok((clauses, "", start + consumed));
            }
            let Some(remainder) = after.strip_prefix(' ') else {
                return Err(failure(
                    DiagnosticCode::COMMENT_SYNTAX,
                    span(source, start + consumed, start + text.len()),
                    "an inline comment is followed by a space or line end",
                ));
            };
            start += consumed + 1;
            text = remainder;
        }
        if !text.starts_with('{') {
            break;
        }
        let Some(close) = text.find('}') else {
            return Err(failure(
                DiagnosticCode::BODY_SYNTAX,
                span(source, start, start + text.len()),
                "non-emitting clause is missing `}`",
            ));
        };
        let inner = &text[1..close];
        let clause_span = span(source, start, start + close + 1);
        if let Some((rule, name)) = inner.split_once(" as ") {
            let rule_span = span(source, start + 1, start + 1 + rule.len());
            let name_start = start + 1 + rule.len() + 4;
            let name_span = span(source, name_start, name_start + name.len());
            clauses.push(ClauseSyntax::Binding(BindingSyntax {
                rule: parsed_identifier(rule, rule_span, true)?,
                name: parsed_identifier(name, name_span, false)?,
                span: clause_span,
            }));
            binding_seen = true;
        } else {
            if binding_seen {
                return Err(failure(
                    DiagnosticCode::CLAUSE_ORDER,
                    clause_span,
                    "guards must precede non-emitting bindings",
                ));
            }
            let expression = parse_guard_expression(inner)
                .map_err(|message| failure(DiagnosticCode::GUARD_SYNTAX, clause_span, message))?;
            clauses.push(ClauseSyntax::Guard(Spanned::new(expression, clause_span)));
        }

        let after = &text[close + 1..];
        if after.is_empty() {
            return Ok((clauses, "", start + close + 1));
        }
        let Some(remainder) = after.strip_prefix(' ') else {
            return Err(failure(
                DiagnosticCode::BODY_SYNTAX,
                span(source, start + close + 1, start + text.len()),
                "a clause is followed by exactly one space or a new logical line",
            ));
        };
        start += close + 2;
        text = remainder;
    }
    Ok((clauses, text, start))
}

fn parse_body(source: &SourceFile, lines: &[(&str, usize, usize)]) -> MecoResult<BodySyntax> {
    let first = lines[0];
    if let Some((raw, chomp)) = block_marker(first.0) {
        return parse_block(source, lines, raw, chomp);
    }
    if lines.len() > 1 {
        if first.0.starts_with('@') || first.0.starts_with('&') {
            return parse_multiline_call(source, lines);
        }
        return Err(failure(
            DiagnosticCode::BODY_SYNTAX,
            span(source, first.1, lines.last().expect("body").2),
            "multiline visible text uses a `|` or `|raw` block",
        ));
    }
    parse_inline_body(source, first.0, first.1, first.2)
}

fn parse_multiline_call(
    source: &SourceFile,
    lines: &[(&str, usize, usize)],
) -> MecoResult<BodySyntax> {
    let first = lines[0];
    let sigil = first.0.as_bytes()[0];
    let name_end = scan_qualified_identifier(first.0, 1);
    if name_end == 1 || !matches!(sigil, b'@' | b'&') {
        return Err(failure(
            DiagnosticCode::CALL_SYNTAX,
            span(source, first.1, first.2),
            "multiline call must begin with `@rule <-` or `&message <-`",
        ));
    }
    let rest = &first.0[name_end..];
    let Some(first_arguments) = rest.strip_prefix(" <-") else {
        return Err(failure(
            DiagnosticCode::CALL_SYNTAX,
            span(source, first.1, first.2),
            "multiline call must use the `<-` argument operator",
        ));
    };
    let target = parsed_identifier(
        &first.0[1..name_end],
        span(source, first.1 + 1, first.1 + name_end),
        true,
    )?;
    let mut arguments = Vec::new();
    if !first_arguments.is_empty() {
        let Some(first_arguments) = first_arguments.strip_prefix(' ') else {
            return Err(failure(
                DiagnosticCode::CALL_SYNTAX,
                span(source, first.1 + name_end, first.2),
                "inline arguments follow `<-` with exactly one space",
            ));
        };
        arguments.extend(parse_arguments(
            source,
            first_arguments,
            first.1 + name_end + 4,
        )?);
    }
    for (line, start, _) in &lines[1..] {
        let leading = line.len() - line.trim_start_matches(' ').len();
        let item = line.trim_matches(' ');
        if item.is_empty() {
            return Err(failure(
                DiagnosticCode::ARGUMENT_SYNTAX,
                empty_span(source, *start),
                "multiline call arguments cannot be blank",
            ));
        }
        let parsed = parse_arguments(source, item, *start + leading)?;
        for argument in parsed {
            if arguments
                .iter()
                .any(|existing: &ArgumentSyntax| existing.name.value() == argument.name.value())
            {
                return Err(failure(
                    DiagnosticCode::ARGUMENT_SYNTAX,
                    argument.name.span(),
                    format!("duplicate argument `{}`", argument.name.value()),
                ));
            }
            arguments.push(argument);
        }
    }
    let call = CallSyntax {
        target,
        arguments,
        span: span(source, first.1, lines.last().expect("call line").2),
    };
    Ok(BodySyntax::Parts(alloc::vec![if sigil == b'@' {
        BodyPartSyntax::RuleCall(call)
    } else {
        BodyPartSyntax::MessageCall(call)
    }]))
}

fn block_marker(text: &str) -> Option<(bool, BlockChomp)> {
    match text {
        "|" => Some((false, BlockChomp::Clip)),
        "|-" => Some((false, BlockChomp::Strip)),
        "|+" => Some((false, BlockChomp::Keep)),
        "|raw" => Some((true, BlockChomp::Clip)),
        "|raw-" => Some((true, BlockChomp::Strip)),
        "|raw+" => Some((true, BlockChomp::Keep)),
        _ => None,
    }
}

fn parse_block(
    source: &SourceFile,
    lines: &[(&str, usize, usize)],
    raw: bool,
    chomp: BlockChomp,
) -> MecoResult<BodySyntax> {
    if lines.len() == 1 {
        return Err(failure(
            DiagnosticCode::BLOCK_SYNTAX,
            span(source, lines[0].1, lines[0].2),
            "a block marker requires an indented content line",
        ));
    }
    let content_lines = &lines[1..];
    let mut text = String::new();
    for (index, (line, _, _)) in content_lines.iter().enumerate() {
        text.push_str(line);
        if index + 1 < content_lines.len() {
            text.push('\n');
        }
    }
    match chomp {
        BlockChomp::Clip => {
            while text.ends_with('\n') {
                text.pop();
            }
            text.push('\n');
        }
        BlockChomp::Strip => {
            while text.ends_with('\n') {
                text.pop();
            }
        }
        BlockChomp::Keep => text.push('\n'),
    }
    let text_span = span(
        source,
        content_lines[0].1,
        content_lines.last().expect("content").2,
    );
    Ok(BodySyntax::Block(BlockSyntax {
        text: Spanned::new(text, text_span),
        raw,
        chomp,
        span: span(source, lines[0].1, lines.last().expect("body").2),
    }))
}

#[allow(clippy::too_many_lines)]
fn parse_inline_body(
    source: &SourceFile,
    text: &str,
    start: usize,
    end: usize,
) -> MecoResult<BodySyntax> {
    let body_span = span(source, start, end);
    if text == "\"\"" {
        return Ok(BodySyntax::Empty(body_span));
    }
    if text.is_empty() || text.starts_with(' ') || text.ends_with(' ') {
        return Err(failure(
            DiagnosticCode::BODY_SYNTAX,
            body_span,
            "leading or trailing output whitespace must be quoted or raw",
        ));
    }

    if let Some(call) = parse_complete_call(source, text, start)? {
        return Ok(BodySyntax::Parts(alloc::vec![call]));
    }

    let mut parts = Vec::new();
    let mut cursor = 0;
    let mut literal_start = 0;
    let bytes = text.as_bytes();
    while cursor < text.len() {
        let special = bytes[cursor];
        let raw_quote = special == b'r' && bytes.get(cursor + 1) == Some(&b'"');
        let inline_comment = special == b'<' && text[cursor..].starts_with("<!--");
        if !matches!(special, b'@' | b'$' | b'&' | b'"' | b'\\') && !raw_quote && !inline_comment {
            cursor += text[cursor..]
                .chars()
                .next()
                .expect("cursor is in text")
                .len_utf8();
            continue;
        }
        push_literal(source, &mut parts, text, start, literal_start, cursor);
        match special {
            b'@' => {
                let (part, next) = parse_at_reference(source, text, start, cursor)?;
                parts.push(part);
                cursor = next;
            }
            b'$' => {
                let (name, name_start, next) =
                    parse_sigil_name(text, cursor, b'$').map_err(|message| {
                        failure(
                            DiagnosticCode::BODY_SYNTAX,
                            span(source, start + cursor, start + text.len()),
                            message,
                        )
                    })?;
                let name_span = span(source, start + name_start, start + name_start + name.len());
                parts.push(BodyPartSyntax::ValueReference(parsed_identifier(
                    name, name_span, false,
                )?));
                cursor = next;
            }
            b'&' => {
                return Err(failure(
                    DiagnosticCode::CALL_SYNTAX,
                    span(source, start + cursor, end),
                    "a complete `&message` must own the entire visible body",
                ));
            }
            b'"' => {
                let (value, next) = parse_quoted(text, cursor, false).map_err(|message| {
                    failure(
                        DiagnosticCode::STRING_SYNTAX,
                        span(source, start + cursor, start + text.len()),
                        message,
                    )
                })?;
                if value.is_empty() {
                    return Err(failure(
                        DiagnosticCode::STRING_SYNTAX,
                        span(source, start + cursor, start + next),
                        "`\"\"` is only valid as the complete production body",
                    ));
                }
                parts.push(BodyPartSyntax::Literal(Spanned::new(
                    value,
                    span(source, start + cursor, start + next),
                )));
                cursor = next;
            }
            b'r' if raw_quote => {
                let (value, next) = parse_quoted(text, cursor + 1, true).map_err(|message| {
                    failure(
                        DiagnosticCode::STRING_SYNTAX,
                        span(source, start + cursor, start + text.len()),
                        message,
                    )
                })?;
                parts.push(BodyPartSyntax::Literal(Spanned::new(
                    value,
                    span(source, start + cursor, start + next),
                )));
                cursor = next;
            }
            b'\\' => {
                let (escaped, next) = parse_escape(text, cursor).map_err(|message| {
                    failure(
                        DiagnosticCode::ESCAPE_SYNTAX,
                        span(source, start + cursor, start + text.len()),
                        message,
                    )
                })?;
                parts.push(BodyPartSyntax::Literal(Spanned::new(
                    escaped,
                    span(source, start + cursor, start + next),
                )));
                cursor = next;
            }
            b'<' if inline_comment => {
                let Some(close) = text[cursor + 4..].find("-->") else {
                    return Err(failure(
                        DiagnosticCode::COMMENT_SYNTAX,
                        span(source, start + cursor, start + text.len()),
                        "inline comment is missing its closing `-->`",
                    ));
                };
                let next = cursor + 4 + close + 3;
                if text[cursor + 4..cursor + 4 + close].contains("<!--") {
                    return Err(failure(
                        DiagnosticCode::COMMENT_SYNTAX,
                        span(source, start + cursor, start + next),
                        "comments cannot nest",
                    ));
                }
                cursor = next;
            }
            _ => unreachable!("all special bytes are handled"),
        }
        literal_start = cursor;
    }
    push_literal(source, &mut parts, text, start, literal_start, text.len());
    if parts.is_empty() {
        return Err(failure(
            DiagnosticCode::BODY_SYNTAX,
            body_span,
            "a production body cannot be empty",
        ));
    }
    Ok(BodySyntax::Parts(parts))
}

fn push_literal(
    source: &SourceFile,
    parts: &mut Vec<BodyPartSyntax>,
    text: &str,
    start: usize,
    from: usize,
    to: usize,
) {
    if from < to {
        parts.push(BodyPartSyntax::Literal(Spanned::new(
            text[from..to].to_string(),
            span(source, start + from, start + to),
        )));
    }
}

fn parse_complete_call(
    source: &SourceFile,
    text: &str,
    start: usize,
) -> MecoResult<Option<BodyPartSyntax>> {
    let Some(sigil) = text.as_bytes().first().copied() else {
        return Ok(None);
    };
    if !matches!(sigil, b'@' | b'&') || text.starts_with("@{") {
        return Ok(None);
    }
    let name_end = scan_qualified_identifier(text, 1);
    if name_end == 1 {
        return Ok(None);
    }
    let rest = &text[name_end..];
    if sigil == b'@' && !rest.starts_with(" <-") {
        return Ok(None);
    }
    if sigil == b'&' && !rest.is_empty() && !rest.starts_with(" <-") {
        return Err(failure(
            DiagnosticCode::CALL_SYNTAX,
            span(source, start, start + text.len()),
            "a message call uses `&id` or `&id <- arguments`",
        ));
    }

    let target_span = span(source, start + 1, start + name_end);
    let target = parsed_identifier(&text[1..name_end], target_span, true)?;
    let arguments = if rest.is_empty() {
        Vec::new()
    } else {
        let Some(argument_source) = rest.strip_prefix(" <-") else {
            return Ok(None);
        };
        let arguments_start = start + name_end + 3;
        parse_arguments(
            source,
            argument_source.trim_start_matches([' ', '\n']),
            arguments_start,
        )?
    };
    let call = CallSyntax {
        target,
        arguments,
        span: span(source, start, start + text.len()),
    };
    Ok(Some(if sigil == b'@' {
        BodyPartSyntax::RuleCall(call)
    } else {
        BodyPartSyntax::MessageCall(call)
    }))
}

fn parse_arguments(
    source: &SourceFile,
    text: &str,
    start: usize,
) -> MecoResult<Vec<ArgumentSyntax>> {
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let mut arguments = Vec::new();
    let mut item_start = 0;
    for item_end in text
        .match_indices([',', '\n'])
        .map(|(index, _)| index)
        .chain(core::iter::once(text.len()))
    {
        let raw = &text[item_start..item_end];
        let leading = raw.len() - raw.trim_start_matches(' ').len();
        let item = raw.trim_matches(' ');
        if item.is_empty() {
            return Err(failure(
                DiagnosticCode::ARGUMENT_SYNTAX,
                span(source, start + item_start, start + item_end),
                "call arguments cannot be empty",
            ));
        }
        let absolute = start + item_start + leading;
        let item_span = span(source, absolute, absolute + item.len());
        let (name, value, punned) = if let Some(reference) = item.strip_prefix('$') {
            let reference_span = span(source, absolute + 1, absolute + item.len());
            let name = parsed_identifier(reference, reference_span, false)?;
            (name.clone(), ValueSyntax::Reference(name), true)
        } else {
            let Some((name_source, value_source)) = item.split_once(": ") else {
                return Err(failure(
                    DiagnosticCode::ARGUMENT_SYNTAX,
                    item_span,
                    "arguments use `name: value` or `$name` punning",
                ));
            };
            let name_span = span(source, absolute, absolute + name_source.len());
            let value_start = absolute + name_source.len() + 2;
            let value_span = span(source, value_start, absolute + item.len());
            (
                parsed_identifier(name_source, name_span, false)?,
                parse_value(value_source, value_span)?,
                false,
            )
        };
        if arguments
            .iter()
            .any(|argument: &ArgumentSyntax| argument.name.value() == name.value())
        {
            return Err(failure(
                DiagnosticCode::ARGUMENT_SYNTAX,
                name.span(),
                format!("duplicate argument `{}`", name.value()),
            ));
        }
        arguments.push(ArgumentSyntax {
            name,
            value,
            punned,
            span: item_span,
        });
        item_start = item_end + 1;
    }
    Ok(arguments)
}

fn parse_value(text: &str, value_span: Span) -> MecoResult<ValueSyntax> {
    if let Some(reference) = text.strip_prefix('$') {
        return Ok(ValueSyntax::Reference(parsed_identifier(
            reference, value_span, false,
        )?));
    }
    if text == "true" || text == "false" {
        return Ok(ValueSyntax::Boolean(Spanned::new(
            text == "true",
            value_span,
        )));
    }
    if text.starts_with('"') {
        let (value, next) = parse_quoted(text, 0, false)
            .map_err(|message| failure(DiagnosticCode::STRING_SYNTAX, value_span, message))?;
        if next != text.len() {
            return Err(failure(
                DiagnosticCode::ARGUMENT_SYNTAX,
                value_span,
                "quoted argument values cannot have a suffix",
            ));
        }
        return Ok(ValueSyntax::Text(Spanned::new(value, value_span)));
    }
    let number = Rational::from_str(text).map_err(|_| {
        failure(
            DiagnosticCode::ARGUMENT_SYNTAX,
            value_span,
            "argument value must be `$name`, a number, boolean, or quoted text",
        )
    })?;
    Ok(ValueSyntax::Number(Spanned::new(number, value_span)))
}

fn parse_at_reference(
    source: &SourceFile,
    text: &str,
    start: usize,
    cursor: usize,
) -> MecoResult<(BodyPartSyntax, usize)> {
    if text[cursor..].starts_with("@{") {
        let Some(relative_close) = text[cursor + 2..].find('}') else {
            return Err(failure(
                DiagnosticCode::BODY_SYNTAX,
                span(source, start + cursor, start + text.len()),
                "delimited rule reference is missing `}`",
            ));
        };
        let close = cursor + 2 + relative_close;
        let inner = &text[cursor + 2..close];
        let whole_span = span(source, start + cursor, start + close + 1);
        if let Some((rule, name)) = inner.split_once(" as ") {
            let rule_span = span(source, start + cursor + 2, start + cursor + 2 + rule.len());
            let name_start = start + cursor + 2 + rule.len() + 4;
            let name_span = span(source, name_start, name_start + name.len());
            return Ok((
                BodyPartSyntax::EmittingCapture {
                    rule: parsed_identifier(rule, rule_span, true)?,
                    name: parsed_identifier(name, name_span, false)?,
                    span: whole_span,
                },
                close + 1,
            ));
        }
        let name_span = span(source, start + cursor + 2, start + close);
        return Ok((
            BodyPartSyntax::RuleReference(parsed_identifier(inner, name_span, true)?),
            close + 1,
        ));
    }

    let end = scan_qualified_identifier(text, cursor + 1);
    if end == cursor + 1 {
        return Err(failure(
            DiagnosticCode::BODY_SYNTAX,
            span(source, start + cursor, start + cursor + 1),
            "`@` must begin a valid rule reference or be escaped as `\\@`",
        ));
    }
    let name_span = span(source, start + cursor + 1, start + end);
    Ok((
        BodyPartSyntax::RuleReference(parsed_identifier(&text[cursor + 1..end], name_span, true)?),
        end,
    ))
}

fn parse_sigil_name(text: &str, cursor: usize, sigil: u8) -> Result<(&str, usize, usize), String> {
    debug_assert_eq!(text.as_bytes()[cursor], sigil);
    if text.as_bytes().get(cursor + 1) == Some(&b'{') {
        let Some(close) = text[cursor + 2..].find('}') else {
            return Err("braced value reference is missing `}`".to_string());
        };
        let end = cursor + 2 + close;
        return Ok((&text[cursor + 2..end], cursor + 2, end + 1));
    }
    let end = scan_identifier(text, cursor + 1);
    if end == cursor + 1 {
        return Err("sigil must be followed by an identifier".to_string());
    }
    Ok((&text[cursor + 1..end], cursor + 1, end))
}

fn parse_quoted(text: &str, quote: usize, raw: bool) -> Result<(String, usize), String> {
    debug_assert_eq!(text.as_bytes()[quote], b'"');
    let mut value = String::new();
    let mut cursor = quote + 1;
    while cursor < text.len() {
        let character = text[cursor..].chars().next().expect("quoted cursor");
        if character == '"' {
            return Ok((value, cursor + 1));
        }
        if character == '\\' && !raw {
            let (escaped, next) = parse_escape(text, cursor)?;
            value.push_str(&escaped);
            cursor = next;
        } else {
            value.push(character);
            cursor += character.len_utf8();
        }
    }
    Err("quoted string is missing its closing quote".to_string())
}

fn parse_escape(text: &str, cursor: usize) -> Result<(String, usize), String> {
    let Some(escaped) = text[cursor + 1..].chars().next() else {
        return Err("escape is missing its escaped character".to_string());
    };
    let value = match escaped {
        '\\' => "\\".to_string(),
        '"' => "\"".to_string(),
        'n' => "\n".to_string(),
        'r' => "\r".to_string(),
        't' => "\t".to_string(),
        '@' => "@".to_string(),
        '$' => "$".to_string(),
        '&' => "&".to_string(),
        '/' if text[cursor + 1..].starts_with("//") => {
            return Ok(("//".to_string(), cursor + 3));
        }
        _ => return Err(format!("unknown escape `\\{escaped}`")),
    };
    Ok((value, cursor + 1 + escaped.len_utf8()))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WeightToken<'a> {
    Number(&'a str),
    Name(&'a str),
    Plus,
    Minus,
    Star,
    Left,
    Right,
}

fn parse_weight_expression(source: &str) -> Result<WeightExpression, String> {
    let tokens = lex_weight_expression(source)?;
    let mut parser = WeightParser {
        tokens: &tokens,
        cursor: 0,
    };
    let expression = parser.parse_additive()?;
    if parser.cursor != tokens.len() {
        return Err("unexpected token in weight expression".to_string());
    }
    Ok(expression)
}

fn lex_weight_expression(source: &str) -> Result<Vec<WeightToken<'_>>, String> {
    let mut tokens = Vec::new();
    let mut cursor = 0;
    while cursor < source.len() {
        let byte = source.as_bytes()[cursor];
        match byte {
            b' ' => cursor += 1,
            b'+' => {
                tokens.push(WeightToken::Plus);
                cursor += 1;
            }
            b'-' => {
                tokens.push(WeightToken::Minus);
                cursor += 1;
            }
            b'*' => {
                tokens.push(WeightToken::Star);
                cursor += 1;
            }
            b'(' => {
                tokens.push(WeightToken::Left);
                cursor += 1;
            }
            b')' => {
                tokens.push(WeightToken::Right);
                cursor += 1;
            }
            b'0'..=b'9' => {
                let start = cursor;
                cursor += 1;
                while cursor < source.len()
                    && matches!(source.as_bytes()[cursor], b'0'..=b'9' | b'.' | b'e' | b'E')
                {
                    cursor += 1;
                    if matches!(source.as_bytes()[cursor - 1], b'e' | b'E')
                        && matches!(source.as_bytes().get(cursor), Some(b'+' | b'-'))
                    {
                        cursor += 1;
                    }
                }
                tokens.push(WeightToken::Number(&source[start..cursor]));
            }
            _ if is_identifier_start(byte) => {
                let start = cursor;
                cursor = scan_identifier(source, cursor);
                tokens.push(WeightToken::Name(&source[start..cursor]));
            }
            _ => {
                return Err(
                    "weight expressions use numbers, names, `+`, `-`, `*`, and parentheses"
                        .to_string(),
                );
            }
        }
    }
    if tokens.is_empty() {
        return Err("weight expression cannot be empty".to_string());
    }
    Ok(tokens)
}

struct WeightParser<'a, 'source> {
    tokens: &'a [WeightToken<'source>],
    cursor: usize,
}

impl WeightParser<'_, '_> {
    fn parse_additive(&mut self) -> Result<WeightExpression, String> {
        let mut left = self.parse_multiplicative()?;
        loop {
            match self.tokens.get(self.cursor) {
                Some(WeightToken::Plus) => {
                    self.cursor += 1;
                    left = WeightExpression::Add(
                        Box::new(left),
                        Box::new(self.parse_multiplicative()?),
                    );
                }
                Some(WeightToken::Minus) => {
                    self.cursor += 1;
                    left = WeightExpression::Subtract(
                        Box::new(left),
                        Box::new(self.parse_multiplicative()?),
                    );
                }
                _ => return Ok(left),
            }
        }
    }

    fn parse_multiplicative(&mut self) -> Result<WeightExpression, String> {
        let mut left = self.parse_primary()?;
        while self.tokens.get(self.cursor) == Some(&WeightToken::Star) {
            self.cursor += 1;
            left = WeightExpression::Multiply(Box::new(left), Box::new(self.parse_primary()?));
        }
        Ok(left)
    }

    fn parse_primary(&mut self) -> Result<WeightExpression, String> {
        let Some(token) = self.tokens.get(self.cursor).copied() else {
            return Err("weight expression ends before a value".to_string());
        };
        self.cursor += 1;
        match token {
            WeightToken::Number(number) => Rational::from_str(number)
                .map(WeightExpression::Literal)
                .map_err(|error| format!("invalid weight number: {error}")),
            WeightToken::Name(name) => Ok(WeightExpression::Name(name.to_string())),
            WeightToken::Left => {
                let expression = self.parse_additive()?;
                if self.tokens.get(self.cursor) != Some(&WeightToken::Right) {
                    return Err("weight expression is missing `)`".to_string());
                }
                self.cursor += 1;
                Ok(expression)
            }
            _ => Err("expected a number, name, or parenthesized weight expression".to_string()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum GuardToken {
    Name(String),
    Number(Rational),
    Text(String),
    True,
    False,
    Is,
    Not,
    And,
    Or,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Left,
    Right,
}

fn parse_guard_expression(source: &str) -> Result<GuardExpression, String> {
    let tokens = lex_guard(source)?;
    let mut parser = GuardParser {
        tokens: &tokens,
        cursor: 0,
    };
    let expression = parser.parse_or()?;
    if parser.cursor != tokens.len() {
        return Err("unexpected token in guard expression".to_string());
    }
    Ok(expression)
}

fn lex_guard(source: &str) -> Result<Vec<GuardToken>, String> {
    let mut tokens = Vec::new();
    let mut cursor = 0;
    while cursor < source.len() {
        match source.as_bytes()[cursor] {
            b' ' => cursor += 1,
            b'(' => {
                tokens.push(GuardToken::Left);
                cursor += 1;
            }
            b')' => {
                tokens.push(GuardToken::Right);
                cursor += 1;
            }
            b'<' => {
                if source.as_bytes().get(cursor + 1) == Some(&b'=') {
                    tokens.push(GuardToken::LessEqual);
                    cursor += 2;
                } else {
                    tokens.push(GuardToken::Less);
                    cursor += 1;
                }
            }
            b'>' => {
                if source.as_bytes().get(cursor + 1) == Some(&b'=') {
                    tokens.push(GuardToken::GreaterEqual);
                    cursor += 2;
                } else {
                    tokens.push(GuardToken::Greater);
                    cursor += 1;
                }
            }
            b'"' => {
                let (text, next) = parse_quoted(source, cursor, false)?;
                tokens.push(GuardToken::Text(text));
                cursor = next;
            }
            b'0'..=b'9' => {
                let start = cursor;
                cursor += 1;
                while cursor < source.len()
                    && matches!(
                        source.as_bytes()[cursor],
                        b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-'
                    )
                {
                    cursor += 1;
                }
                let number = Rational::from_str(&source[start..cursor])
                    .map_err(|error| format!("invalid guard number: {error}"))?;
                tokens.push(GuardToken::Number(number));
            }
            byte if is_identifier_start(byte) => {
                let start = cursor;
                cursor = scan_identifier(source, cursor);
                tokens.push(match &source[start..cursor] {
                    "true" => GuardToken::True,
                    "false" => GuardToken::False,
                    "is" => GuardToken::Is,
                    "not" => GuardToken::Not,
                    "and" => GuardToken::And,
                    "or" => GuardToken::Or,
                    name => GuardToken::Name(name.to_string()),
                });
            }
            _ => return Err("guard contains an unsupported token".to_string()),
        }
    }
    if tokens.is_empty() {
        return Err("guard expression cannot be empty".to_string());
    }
    Ok(tokens)
}

struct GuardParser<'a> {
    tokens: &'a [GuardToken],
    cursor: usize,
}

impl GuardParser<'_> {
    fn parse_or(&mut self) -> Result<GuardExpression, String> {
        let mut left = self.parse_and()?;
        while self.tokens.get(self.cursor) == Some(&GuardToken::Or) {
            self.cursor += 1;
            left = GuardExpression::Or(Box::new(left), Box::new(self.parse_and()?));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<GuardExpression, String> {
        let mut left = self.parse_not()?;
        while self.tokens.get(self.cursor) == Some(&GuardToken::And) {
            self.cursor += 1;
            left = GuardExpression::And(Box::new(left), Box::new(self.parse_not()?));
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<GuardExpression, String> {
        if self.tokens.get(self.cursor) == Some(&GuardToken::Not) {
            self.cursor += 1;
            return Ok(GuardExpression::Not(Box::new(self.parse_not()?)));
        }
        if self.tokens.get(self.cursor) == Some(&GuardToken::Left) {
            self.cursor += 1;
            let expression = self.parse_or()?;
            if self.tokens.get(self.cursor) != Some(&GuardToken::Right) {
                return Err("guard expression is missing `)`".to_string());
            }
            self.cursor += 1;
            return Ok(expression);
        }
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<GuardExpression, String> {
        let left = self.parse_value()?;
        let Some(operator) = self.tokens.get(self.cursor) else {
            return Ok(GuardExpression::Value(left));
        };
        let is_not = *operator == GuardToken::Is
            && self.tokens.get(self.cursor + 1) == Some(&GuardToken::Not);
        let expression = match operator {
            GuardToken::Is if is_not => {
                self.cursor += 2;
                GuardExpression::IsNot(left, self.parse_value()?)
            }
            GuardToken::Is => {
                self.cursor += 1;
                GuardExpression::Is(left, self.parse_value()?)
            }
            GuardToken::Less => {
                self.cursor += 1;
                GuardExpression::Less(left, self.parse_value()?)
            }
            GuardToken::LessEqual => {
                self.cursor += 1;
                GuardExpression::LessOrEqual(left, self.parse_value()?)
            }
            GuardToken::Greater => {
                self.cursor += 1;
                GuardExpression::Greater(left, self.parse_value()?)
            }
            GuardToken::GreaterEqual => {
                self.cursor += 1;
                GuardExpression::GreaterOrEqual(left, self.parse_value()?)
            }
            _ => return Ok(GuardExpression::Value(left)),
        };
        Ok(expression)
    }

    fn parse_value(&mut self) -> Result<GuardValue, String> {
        let Some(token) = self.tokens.get(self.cursor) else {
            return Err("guard expression ends before a value".to_string());
        };
        self.cursor += 1;
        match token {
            GuardToken::Name(name) => Ok(GuardValue::Name(name.clone())),
            GuardToken::Number(number) => Ok(GuardValue::Number(*number)),
            GuardToken::Text(text) => Ok(GuardValue::Text(text.clone())),
            GuardToken::True => Ok(GuardValue::Boolean(true)),
            GuardToken::False => Ok(GuardValue::Boolean(false)),
            _ => Err("expected a name, number, boolean, or quoted guard value".to_string()),
        }
    }
}

fn parsed_identifier(
    value: &str,
    value_span: Span,
    qualified: bool,
) -> MecoResult<Spanned<String>> {
    let valid = if qualified {
        !value.is_empty() && value.split('.').all(is_identifier)
    } else {
        is_identifier(value)
    };
    if !valid {
        return Err(failure(
            DiagnosticCode::INVALID_IDENTIFIER,
            value_span,
            format!("`{value}` is not a valid ASCII identifier"),
        ));
    }
    Ok(Spanned::new(value.to_string(), value_span))
}

fn is_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    is_identifier_start(first)
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

const fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn scan_identifier(text: &str, mut cursor: usize) -> usize {
    if !text
        .as_bytes()
        .get(cursor)
        .is_some_and(|byte| is_identifier_start(*byte))
    {
        return cursor;
    }
    cursor += 1;
    while text
        .as_bytes()
        .get(cursor)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'-'))
    {
        cursor += 1;
    }
    cursor
}

fn scan_qualified_identifier(text: &str, mut cursor: usize) -> usize {
    let first_end = scan_identifier(text, cursor);
    if first_end == cursor {
        return cursor;
    }
    cursor = first_end;
    while text.as_bytes().get(cursor) == Some(&b'.') {
        let next = scan_identifier(text, cursor + 1);
        if next == cursor + 1 {
            break;
        }
        cursor = next;
    }
    cursor
}

fn collect_lines(text: &str, mut start: usize) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    while start < text.len() {
        let tail = &text[start..];
        let newline = tail.find('\n');
        let raw_end = newline.map_or(text.len(), |relative| start + relative);
        let end = if raw_end > start && text.as_bytes()[raw_end - 1] == b'\r' {
            raw_end - 1
        } else {
            raw_end
        };
        let next = newline.map_or(text.len(), |_| raw_end + 1);
        lines.push(Line {
            text: &text[start..end],
            start,
            end,
        });
        start = next;
    }
    lines
}

fn is_comment_start(line: &str) -> bool {
    line.trim_start_matches(' ').starts_with("<!--")
}

fn skip_comment(source: &SourceFile, lines: &[Line<'_>], start: usize) -> MecoResult<usize> {
    let opening = lines[start];
    let leading = opening.text.len() - opening.text.trim_start_matches(' ').len();
    if leading != 0 {
        return Err(failure(
            DiagnosticCode::COMMENT_SYNTAX,
            line_span(source, opening),
            "comments between rules and productions are not indented",
        ));
    }
    let mut index = start;
    loop {
        let line = lines[index];
        if let Some(close) = line.text.find("-->") {
            if line.text[close + 3..]
                .chars()
                .any(|character| character != ' ')
            {
                return Err(failure(
                    DiagnosticCode::COMMENT_SYNTAX,
                    line_span(source, line),
                    "only whitespace may follow a whole-line comment",
                ));
            }
            return Ok(index + 1);
        }
        index += 1;
        if index == lines.len() {
            return Err(failure(
                DiagnosticCode::COMMENT_SYNTAX,
                line_span(source, opening),
                "comment is missing its closing `-->`",
            ));
        }
        if lines[index].text.contains("<!--") {
            return Err(failure(
                DiagnosticCode::COMMENT_SYNTAX,
                line_span(source, lines[index]),
                "comments cannot nest",
            ));
        }
    }
}

fn line_span(source: &SourceFile, line: Line<'_>) -> Span {
    span(source, line.start, line.end)
}

fn empty_span(source: &SourceFile, byte: usize) -> Span {
    Span::empty(source.id(), source.position(byte).expect("source boundary"))
}

fn span(source: &SourceFile, start: usize, end: usize) -> Span {
    Span::new(
        source.id(),
        source.position(start).expect("parser start boundary"),
        source.position(end).expect("parser end boundary"),
    )
    .expect("ordered parser span")
}

fn failure(code: DiagnosticCode, span: Span, message: impl Into<String>) -> MecoError {
    MecoError::new(Diagnostic::new(code, Severity::Error, Some(span), message))
}

#[cfg(test)]
mod tests {
    use super::parse_module;
    use crate::{
        BodyPartSyntax, BodySyntax, ClauseSyntax, DiagnosticCode, SourceFile, SourceId,
        WeightExpression, WeightSyntax,
    };

    fn source(body: &str) -> SourceFile {
        SourceFile::new(
            SourceId::new(0),
            "test.meco.md",
            alloc::format!("---\nmeco: 2\nmodule: test\n---\n\n{body}"),
        )
    }

    #[test]
    fn parses_rules_parameters_weights_and_references() {
        let source = source(concat!(
            "# greeting <- name: text\n",
            "- [3] Hello, $name and @person!\n",
            "- [weight = urgency * 2, id = urgent] r\"Wait @here\"\n",
        ));
        let module = parse_module(&source).expect("module parses");
        let rule = &module.rules[0];

        assert_eq!(rule.name.value(), "greeting");
        assert_eq!(rule.parameters[0].name.value(), "name");
        assert!(matches!(
            rule.productions[0].weight,
            WeightSyntax::Static(_)
        ));
        assert!(matches!(
            rule.productions[1].weight,
            WeightSyntax::Dynamic(ref expression)
                if matches!(expression.value(), WeightExpression::Multiply(_, _))
        ));
        assert_eq!(
            rule.productions[1]
                .authored_id
                .as_ref()
                .expect("id")
                .value(),
            "urgent"
        );
        assert!(matches!(
            rule.productions[0].body,
            BodySyntax::Parts(ref parts)
                if parts.iter().any(|part| matches!(part, BodyPartSyntax::RuleReference(_)))
        ));
    }

    #[test]
    fn parses_guards_bindings_captures_and_message_calls() {
        let source = source(concat!(
            "# arrival\n",
            "- {mood is tense}\n",
            "  {common.name as hero}\n",
            "  &arrival <- hero: $hero\n",
            "# intro\n",
            "- @{common.name as hero} arrived. $hero waved.\n",
        ));
        let module = parse_module(&source).expect("module parses");
        let production = &module.rules[0].productions[0];

        assert!(matches!(production.clauses[0], ClauseSyntax::Guard(_)));
        assert!(matches!(production.clauses[1], ClauseSyntax::Binding(_)));
        assert!(matches!(
            production.body,
            BodySyntax::Parts(ref parts) if matches!(parts[0], BodyPartSyntax::MessageCall(_))
        ));
        assert!(matches!(
            module.rules[1].productions[0].body,
            BodySyntax::Parts(ref parts)
                if parts.iter().any(|part| matches!(part, BodyPartSyntax::EmittingCapture { .. }))
        ));
    }

    #[test]
    fn rejects_guards_after_bindings() {
        let source = source("# bad\n- {name as hero}\n  {mood is tense}\n  Hello.\n");
        let error = parse_module(&source).expect_err("clause order must fail");

        assert_eq!(error.diagnostics()[0].code(), DiagnosticCode::CLAUSE_ORDER);
    }

    #[test]
    fn comments_are_syntax_outside_literals_only() {
        let source = source(concat!(
            "# line\n",
            "- <!-- before --> Hello<!-- middle -->, @person!\n",
            "- r\"<!-- raw -->\"\n",
            "- \"<!-- quoted -->\"\n",
            "# block\n",
            "- |raw-\n",
            "  <!-- raw block -->\n",
        ));
        let module = parse_module(&source).expect("comments and literals parse");
        let first = &module.rules[0].productions[0].body;

        assert!(matches!(
            first,
            BodySyntax::Parts(parts)
                if parts.iter().all(|part| match part {
                    BodyPartSyntax::Literal(text) => !text.value().contains("<!--"),
                    _ => true,
                })
        ));
        assert!(matches!(
            module.rules[0].productions[1].body,
            BodySyntax::Parts(ref parts)
                if matches!(&parts[0], BodyPartSyntax::Literal(text) if text.value() == "<!-- raw -->")
        ));
        assert!(matches!(
            module.rules[0].productions[2].body,
            BodySyntax::Parts(ref parts)
                if matches!(&parts[0], BodyPartSyntax::Literal(text) if text.value() == "<!-- quoted -->")
        ));
        assert!(matches!(
            module.rules[1].productions[0].body,
            BodySyntax::Block(ref block) if block.text.value() == "<!-- raw block -->"
        ));
    }
}
