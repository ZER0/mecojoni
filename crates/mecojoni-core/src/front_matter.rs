use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};

use crate::{
    Diagnostic, DiagnosticCode, MecoError, MecoResult, Severity, SourceFile, Span, Spanned,
};

/// Strict, dependency-free representation of a format-2 module header.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontMatter {
    span: Span,
    version: Spanned<u32>,
    module: Spanned<String>,
    entry: Option<Spanned<String>>,
    sampler: Option<Spanned<String>>,
    types: Vec<TypeDeclaration>,
    inputs: Vec<InputDeclaration>,
    imports: Vec<ImportDeclaration>,
    exports: Vec<Spanned<String>>,
}

impl FrontMatter {
    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }

    #[must_use]
    pub const fn version(&self) -> &Spanned<u32> {
        &self.version
    }

    #[must_use]
    pub const fn module(&self) -> &Spanned<String> {
        &self.module
    }

    #[must_use]
    pub const fn entry(&self) -> Option<&Spanned<String>> {
        self.entry.as_ref()
    }

    #[must_use]
    pub const fn sampler(&self) -> Option<&Spanned<String>> {
        self.sampler.as_ref()
    }

    #[must_use]
    pub fn types(&self) -> &[TypeDeclaration] {
        &self.types
    }

    #[must_use]
    pub fn inputs(&self) -> &[InputDeclaration] {
        &self.inputs
    }

    #[must_use]
    pub fn imports(&self) -> &[ImportDeclaration] {
        &self.imports
    }

    #[must_use]
    pub fn exports(&self) -> &[Spanned<String>] {
        &self.exports
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeDeclaration {
    name: Spanned<String>,
    variants: Vec<Spanned<String>>,
    span: Span,
}

impl TypeDeclaration {
    #[must_use]
    pub const fn name(&self) -> &Spanned<String> {
        &self.name
    }

    #[must_use]
    pub fn variants(&self) -> &[Spanned<String>] {
        &self.variants
    }

    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputDeclaration {
    name: Spanned<String>,
    type_name: Spanned<String>,
    span: Span,
}

impl InputDeclaration {
    #[must_use]
    pub const fn name(&self) -> &Spanned<String> {
        &self.name
    }

    #[must_use]
    pub const fn type_name(&self) -> &Spanned<String> {
        &self.type_name
    }

    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportDeclaration {
    alias: Spanned<String>,
    path: Spanned<String>,
    span: Span,
}

impl ImportDeclaration {
    #[must_use]
    pub const fn alias(&self) -> &Spanned<String> {
        &self.alias
    }

    #[must_use]
    pub const fn path(&self) -> &Spanned<String> {
        &self.path
    }

    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Section {
    Types,
    Inputs,
    Imports,
}

#[derive(Clone, Copy)]
struct SourceLine<'a> {
    text: &'a str,
    start: usize,
    end: usize,
    next: usize,
}

struct HeaderBuilder {
    seen: u16,
    version: Option<Spanned<u32>>,
    module: Option<Spanned<String>>,
    entry: Option<Spanned<String>>,
    sampler: Option<Spanned<String>>,
    types: Vec<TypeDeclaration>,
    inputs: Vec<InputDeclaration>,
    imports: Vec<ImportDeclaration>,
    exports: Vec<Spanned<String>>,
}

impl HeaderBuilder {
    const fn new() -> Self {
        Self {
            seen: 0,
            version: None,
            module: None,
            entry: None,
            sampler: None,
            types: Vec::new(),
            inputs: Vec::new(),
            imports: Vec::new(),
            exports: Vec::new(),
        }
    }
}

/// Parses only the framed format-2 header. Rule parsing starts at the returned
/// header span's end in the next compiler phase.
///
/// # Errors
///
/// Returns [`MecoError`] for a missing or unterminated header, invalid strict
/// mapping syntax, unknown or duplicate fields, invalid identifiers and values,
/// or a format version other than exactly `2`.
pub fn parse_front_matter(source: &SourceFile) -> MecoResult<FrontMatter> {
    let text = source.text();
    let first = next_line(text, 0).ok_or_else(|| {
        failure(
            DiagnosticCode::HEADER_MISSING,
            empty_span(source, 0),
            "a v2 module must begin with an exact `---` header delimiter",
        )
    })?;
    if first.text != "---" {
        return Err(failure(
            DiagnosticCode::HEADER_MISSING,
            line_span(source, first),
            "a v2 module must begin with an exact `---` header delimiter",
        ));
    }

    let mut builder = HeaderBuilder::new();
    let mut section = None;
    let mut offset = first.next;
    let closing;

    loop {
        let Some(line) = next_line(text, offset) else {
            return Err(failure(
                DiagnosticCode::HEADER_UNTERMINATED,
                empty_span(source, text.len()),
                "front matter is missing its closing `---` delimiter",
            ));
        };
        offset = line.next;

        if line.text == "---" {
            closing = line;
            break;
        }
        if line.text.is_empty() {
            continue;
        }
        if line.text.contains('\t') {
            return Err(failure(
                DiagnosticCode::HEADER_INDENT,
                line_span(source, line),
                "tabs are not allowed in front matter",
            ));
        }

        if line.text.starts_with(' ') {
            let active = section.ok_or_else(|| {
                failure(
                    DiagnosticCode::HEADER_INDENT,
                    line_span(source, line),
                    "an indented field must belong to `types`, `inputs`, or `imports`",
                )
            })?;
            if !line.text.starts_with("  ")
                || line.text.starts_with("   ")
                || line.text.get(2..).is_some_and(|rest| rest.starts_with(' '))
            {
                return Err(failure(
                    DiagnosticCode::HEADER_INDENT,
                    line_span(source, line),
                    "nested front-matter fields use exactly two spaces",
                ));
            }
            parse_nested(source, line, active, &mut builder)?;
            continue;
        }

        section = parse_top_level(source, line, &mut builder)?;
    }

    let required_at = empty_span(source, closing.start);
    let version = builder.version.ok_or_else(|| {
        failure(
            DiagnosticCode::HEADER_REQUIRED_FIELD,
            required_at,
            "front matter requires `meco: 2`",
        )
    })?;
    let module = builder.module.ok_or_else(|| {
        failure(
            DiagnosticCode::HEADER_REQUIRED_FIELD,
            required_at,
            "front matter requires a `module` identifier",
        )
    })?;

    Ok(FrontMatter {
        span: span(source, first.start, closing.next),
        version,
        module,
        entry: builder.entry,
        sampler: builder.sampler,
        types: builder.types,
        inputs: builder.inputs,
        imports: builder.imports,
        exports: builder.exports,
    })
}

fn parse_top_level(
    source: &SourceFile,
    line: SourceLine<'_>,
    builder: &mut HeaderBuilder,
) -> MecoResult<Option<Section>> {
    let field = parse_mapping(source, line, 0)?;
    let (bit, section) = match field.key {
        "meco" => (1 << 0, None),
        "module" => (1 << 1, None),
        "entry" => (1 << 2, None),
        "sampler" => (1 << 3, None),
        "types" => (1 << 4, Some(Section::Types)),
        "inputs" => (1 << 5, Some(Section::Inputs)),
        "imports" => (1 << 6, Some(Section::Imports)),
        "exports" => (1 << 7, None),
        _ => {
            return Err(failure(
                DiagnosticCode::HEADER_UNKNOWN_FIELD,
                field.key_span,
                format!("unknown front-matter field `{}`", field.key),
            ));
        }
    };

    if builder.seen & bit != 0 {
        return Err(failure(
            DiagnosticCode::HEADER_DUPLICATE_FIELD,
            field.key_span,
            format!("duplicate front-matter field `{}`", field.key),
        ));
    }
    builder.seen |= bit;

    if let Some(active) = section {
        if field.value.is_some() {
            return Err(failure(
                DiagnosticCode::HEADER_VALUE,
                field.value_span.unwrap_or(field.key_span),
                format!(
                    "`{}` introduces an indented mapping and has no inline value",
                    field.key
                ),
            ));
        }
        return Ok(Some(active));
    }

    let value = field.value.ok_or_else(|| {
        failure(
            DiagnosticCode::HEADER_VALUE,
            field.key_span,
            format!("`{}` requires an inline value", field.key),
        )
    })?;
    let value_span = field.value_span.expect("a parsed value has a span");

    match field.key {
        "meco" => {
            if value != "2" {
                return Err(failure(
                    DiagnosticCode::UNSUPPORTED_VERSION,
                    value_span,
                    "format version must be the exact integer `2`",
                ));
            }
            builder.version = Some(Spanned::new(2, value_span));
        }
        "module" => builder.module = Some(parse_identifier(value, value_span)?),
        "entry" => builder.entry = Some(parse_identifier(value, value_span)?),
        "sampler" => {
            if value != "weighted/1" && value != "diverse/1" {
                return Err(failure(
                    DiagnosticCode::HEADER_VALUE,
                    value_span,
                    "sampler must be `weighted/1` or `diverse/1`",
                ));
            }
            builder.sampler = Some(Spanned::new(value.to_string(), value_span));
        }
        "exports" => builder.exports = parse_identifier_list(source, value, value_span)?,
        _ => unreachable!("all top-level fields are handled above"),
    }

    Ok(None)
}

fn parse_nested(
    source: &SourceFile,
    line: SourceLine<'_>,
    section: Section,
    builder: &mut HeaderBuilder,
) -> MecoResult<()> {
    let field = parse_mapping(source, line, 2)?;
    let value = field.value.ok_or_else(|| {
        failure(
            DiagnosticCode::HEADER_VALUE,
            field.key_span,
            format!("`{}` requires an inline value", field.key),
        )
    })?;
    let value_span = field.value_span.expect("a parsed value has a span");
    let name = parse_identifier(field.key, field.key_span)?;

    match section {
        Section::Types => {
            ensure_unique(
                builder.types.iter().map(|item| item.name.value().as_str()),
                field.key,
                field.key_span,
            )?;
            let variants = parse_identifier_list(source, value, value_span)?;
            if variants.is_empty() {
                return Err(failure(
                    DiagnosticCode::HEADER_VALUE,
                    value_span,
                    "a finite type requires at least one variant",
                ));
            }
            for (index, variant) in variants.iter().enumerate() {
                if variants[..index]
                    .iter()
                    .any(|previous| previous.value() == variant.value())
                {
                    return Err(failure(
                        DiagnosticCode::HEADER_DUPLICATE_FIELD,
                        variant.span(),
                        format!("duplicate type variant `{}`", variant.value()),
                    ));
                }
            }
            builder.types.push(TypeDeclaration {
                name,
                variants,
                span: line_span(source, line),
            });
        }
        Section::Inputs => {
            ensure_unique(
                builder.inputs.iter().map(|item| item.name.value().as_str()),
                field.key,
                field.key_span,
            )?;
            let type_name = parse_identifier(value, value_span)?;
            builder.inputs.push(InputDeclaration {
                name,
                type_name,
                span: line_span(source, line),
            });
        }
        Section::Imports => {
            ensure_unique(
                builder
                    .imports
                    .iter()
                    .map(|item| item.alias.value().as_str()),
                field.key,
                field.key_span,
            )?;
            let path = parse_quoted_string(value, value_span)?;
            builder.imports.push(ImportDeclaration {
                alias: name,
                path: Spanned::new(path, value_span),
                span: line_span(source, line),
            });
        }
    }

    Ok(())
}

struct Mapping<'a> {
    key: &'a str,
    key_span: Span,
    value: Option<&'a str>,
    value_span: Option<Span>,
}

fn parse_mapping<'a>(
    source: &SourceFile,
    line: SourceLine<'a>,
    indentation: usize,
) -> MecoResult<Mapping<'a>> {
    let content = line
        .text
        .get(indentation..)
        .expect("validated indentation is a character boundary");
    let Some(colon) = content.find(':') else {
        return Err(failure(
            DiagnosticCode::HEADER_SYNTAX,
            line_span(source, line),
            "front-matter fields use `name: value` syntax",
        ));
    };
    let key = &content[..colon];
    let key_start = line.start + indentation;
    let key_span = span(source, key_start, key_start + colon);
    if !is_identifier(key) {
        return Err(failure(
            DiagnosticCode::INVALID_IDENTIFIER,
            key_span,
            format!("`{key}` is not a valid ASCII identifier"),
        ));
    }

    let rest = &content[colon + 1..];
    if rest.is_empty() {
        return Ok(Mapping {
            key,
            key_span,
            value: None,
            value_span: None,
        });
    }
    if !rest.starts_with(' ') || rest.starts_with("  ") {
        return Err(failure(
            DiagnosticCode::HEADER_SYNTAX,
            span(source, key_start + colon + 1, line.end),
            "an inline front-matter value follows exactly one space after `:`",
        ));
    }
    let value = &rest[1..];
    if value.is_empty() || value.ends_with(char::is_whitespace) {
        return Err(failure(
            DiagnosticCode::HEADER_VALUE,
            span(source, key_start + colon + 2, line.end),
            "front-matter values cannot be empty or end in whitespace",
        ));
    }
    let value_start = key_start + colon + 2;

    Ok(Mapping {
        key,
        key_span,
        value: Some(value),
        value_span: Some(span(source, value_start, line.end)),
    })
}

fn parse_identifier(value: &str, value_span: Span) -> MecoResult<Spanned<String>> {
    if !is_identifier(value) {
        return Err(failure(
            DiagnosticCode::INVALID_IDENTIFIER,
            value_span,
            format!("`{value}` is not a valid ASCII identifier"),
        ));
    }
    Ok(Spanned::new(value.to_string(), value_span))
}

fn parse_identifier_list(
    source: &SourceFile,
    value: &str,
    value_span: Span,
) -> MecoResult<Vec<Spanned<String>>> {
    if !value.starts_with('[') || !value.ends_with(']') {
        return Err(failure(
            DiagnosticCode::HEADER_VALUE,
            value_span,
            "expected an inline identifier list such as `[one, two]`",
        ));
    }
    let body = &value[1..value.len() - 1];
    if body.is_empty() {
        return Ok(Vec::new());
    }
    if body.starts_with(' ') || body.ends_with(' ') {
        return Err(failure(
            DiagnosticCode::HEADER_VALUE,
            value_span,
            "identifier lists cannot contain spaces next to `[` or `]`",
        ));
    }

    let body_start = usize::try_from(value_span.start().byte())
        .expect("source positions fit the host address space")
        + 1;
    let mut values = Vec::new();
    let mut item_start = 0;
    for item_end in body
        .match_indices(',')
        .map(|(index, _)| index)
        .chain(core::iter::once(body.len()))
    {
        let raw = &body[item_start..item_end];
        let leading = raw.len() - raw.trim_start_matches(' ').len();
        let trimmed = raw.trim_start_matches(' ');
        if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
            return Err(failure(
                DiagnosticCode::HEADER_VALUE,
                value_span,
                "identifier lists contain comma-separated ASCII identifiers",
            ));
        }
        let start = body_start + item_start + leading;
        let item_span = span(source, start, start + trimmed.len());
        let parsed = parse_identifier(trimmed, item_span)?;
        if values
            .iter()
            .any(|previous: &Spanned<String>| previous.value() == parsed.value())
        {
            return Err(failure(
                DiagnosticCode::HEADER_DUPLICATE_FIELD,
                item_span,
                format!("duplicate list item `{trimmed}`"),
            ));
        }
        values.push(parsed);
        item_start = item_end + 1;
    }
    Ok(values)
}

fn parse_quoted_string(value: &str, value_span: Span) -> MecoResult<String> {
    if value.len() < 2 || !value.starts_with('"') || !value.ends_with('"') {
        return Err(failure(
            DiagnosticCode::HEADER_VALUE,
            value_span,
            "import paths must be double-quoted strings",
        ));
    }

    let body = &value[1..value.len() - 1];
    let mut result = String::new();
    let mut characters = body.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            if character == '"' || character.is_control() {
                return Err(failure(
                    DiagnosticCode::HEADER_VALUE,
                    value_span,
                    "quoted import paths require escapes for quotes and control characters",
                ));
            }
            result.push(character);
            continue;
        }
        let escaped = characters.next().ok_or_else(|| {
            failure(
                DiagnosticCode::HEADER_VALUE,
                value_span,
                "a quoted import path cannot end with a backslash",
            )
        })?;
        match escaped {
            '\\' | '"' => result.push(escaped),
            'n' => result.push('\n'),
            'r' => result.push('\r'),
            't' => result.push('\t'),
            _ => {
                return Err(failure(
                    DiagnosticCode::HEADER_VALUE,
                    value_span,
                    format!("unknown quoted-string escape `\\{escaped}`"),
                ));
            }
        }
    }
    if result.is_empty() {
        return Err(failure(
            DiagnosticCode::HEADER_VALUE,
            value_span,
            "an import path cannot be empty",
        ));
    }
    Ok(result)
}

fn ensure_unique<'a>(
    existing: impl Iterator<Item = &'a str>,
    candidate: &str,
    candidate_span: Span,
) -> MecoResult<()> {
    if existing.into_iter().any(|name| name == candidate) {
        return Err(failure(
            DiagnosticCode::HEADER_DUPLICATE_FIELD,
            candidate_span,
            format!("duplicate declaration `{candidate}`"),
        ));
    }
    Ok(())
}

fn is_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn next_line(text: &str, start: usize) -> Option<SourceLine<'_>> {
    if start >= text.len() {
        return None;
    }
    let tail = &text[start..];
    let newline = tail.find('\n');
    let raw_end = newline.map_or(text.len(), |relative| start + relative);
    let end = if raw_end > start && text.as_bytes()[raw_end - 1] == b'\r' {
        raw_end - 1
    } else {
        raw_end
    };
    let next = newline.map_or(text.len(), |_| raw_end + 1);
    Some(SourceLine {
        text: &text[start..end],
        start,
        end,
        next,
    })
}

fn line_span(source: &SourceFile, line: SourceLine<'_>) -> Span {
    span(source, line.start, line.end)
}

fn empty_span(source: &SourceFile, byte: usize) -> Span {
    let at = source
        .position(byte)
        .expect("line scanner only creates UTF-8 character boundaries");
    Span::empty(source.id(), at)
}

fn span(source: &SourceFile, start: usize, end: usize) -> Span {
    let start = source
        .position(start)
        .expect("parser start offset is a UTF-8 character boundary");
    let end = source
        .position(end)
        .expect("parser end offset is a UTF-8 character boundary");
    Span::new(source.id(), start, end).expect("parser emits ordered source spans")
}

fn failure(code: DiagnosticCode, span: Span, message: impl Into<String>) -> MecoError {
    MecoError::new(Diagnostic::new(code, Severity::Error, Some(span), message))
}

#[cfg(test)]
mod tests {
    use super::parse_front_matter;
    use crate::{DiagnosticCode, SourceFile, SourceId};

    fn parse(text: &str) -> super::FrontMatter {
        let source = SourceFile::new(SourceId::new(0), "test.meco.md", text);
        parse_front_matter(&source).expect("header should parse")
    }

    fn error_code(text: &str) -> DiagnosticCode {
        let source = SourceFile::new(SourceId::new(0), "test.meco.md", text);
        parse_front_matter(&source)
            .expect_err("header should fail")
            .diagnostics()[0]
            .code()
    }

    #[test]
    fn parses_the_canonical_header_shape() {
        let header = parse(concat!(
            "---\n",
            "meco: 2\n",
            "module: npc\n",
            "sampler: diverse/1\n",
            "types:\n",
            "  Mood: [calm, tense]\n",
            "inputs:\n",
            "  playerName: text\n",
            "imports:\n",
            "  common: \"./common.meco\"\n",
            "exports: [pickup, warning]\n",
            "---\n",
            "# pickup\n",
        ));

        assert_eq!(*header.version().value(), 2);
        assert_eq!(header.module().value(), "npc");
        assert_eq!(header.sampler().expect("sampler").value(), "diverse/1");
        assert_eq!(header.types()[0].variants().len(), 2);
        assert_eq!(header.inputs()[0].name().value(), "playerName");
        assert_eq!(header.imports()[0].path().value(), "./common.meco");
        assert_eq!(header.exports().len(), 2);
    }

    #[test]
    fn accepts_crlf_without_losing_original_byte_coordinates() {
        let header = parse("---\r\nmeco: 2\r\nmodule: npc\r\n---\r\n# greeting\r\n");

        assert_eq!(header.module().span().byte_len(), 3);
        assert_eq!(header.span().end().byte(), 32);
    }

    #[test]
    fn rejects_unknown_duplicate_and_missing_fields() {
        assert_eq!(
            error_code("---\nmeco: 2\nmodule: npc\nyaml: nope\n---\n"),
            DiagnosticCode::HEADER_UNKNOWN_FIELD
        );
        assert_eq!(
            error_code("---\nmeco: 2\nmodule: npc\nmodule: again\n---\n"),
            DiagnosticCode::HEADER_DUPLICATE_FIELD
        );
        assert_eq!(
            error_code("---\nmeco: 2\n---\n"),
            DiagnosticCode::HEADER_REQUIRED_FIELD
        );
    }

    #[test]
    fn rejects_yaml_features_and_non_ascii_identifiers() {
        assert_eq!(
            error_code("---\nmeco: 2\nmodule: &npc npc\n---\n"),
            DiagnosticCode::INVALID_IDENTIFIER
        );
        assert_eq!(
            error_code("---\nmeco: 2\nmodule: naïve\n---\n"),
            DiagnosticCode::INVALID_IDENTIFIER
        );
    }

    #[test]
    fn rejects_wrong_version_and_indentation() {
        assert_eq!(
            error_code("---\nmeco: 2.0\nmodule: npc\n---\n"),
            DiagnosticCode::UNSUPPORTED_VERSION
        );
        assert_eq!(
            error_code("---\nmeco: 2\nmodule: npc\ntypes:\n Mood: [calm]\n---\n"),
            DiagnosticCode::HEADER_INDENT
        );
    }

    #[test]
    fn rejects_noncanonical_lists_and_quoted_paths() {
        assert_eq!(
            error_code("---\nmeco: 2\nmodule: npc\nexports: [ greeting]\n---\n"),
            DiagnosticCode::HEADER_VALUE
        );
        assert_eq!(
            error_code(concat!(
                "---\n",
                "meco: 2\n",
                "module: npc\n",
                "imports:\n",
                "  common: \"a\"b\"\n",
                "---\n",
            )),
            DiagnosticCode::HEADER_VALUE
        );
    }
}
