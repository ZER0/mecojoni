//! Canonical frozen compiled artifacts.

use alloc::{boxed::Box, format, string::String, vec::Vec};

use crate::compiler::{
    CompiledBinding, CompiledGrammar, CompiledGuard, CompiledGuardValue, CompiledInput,
    CompiledPart, CompiledProduction, CompiledRule, CompiledValue, CompiledWeight,
    CompiledWeightExpression, StaticSelection, ValueType,
};
use crate::{
    Diagnostic, DiagnosticCode, MecoError, MecoResult, MessageArgument, MessageDefinition,
    MessageManifest, Rational, RuleAnalysis, SchemaType, Severity, SourceId, SourcePosition, Span,
    Value,
};

/// Frozen compiled-artifact compatibility identifier.
pub const BYTECODE_VERSION: &str = "bytecode/1";
/// Versioned invariants shared by source compilation and artifact decoding.
pub const LOWERED_IR_CONTRACT: &str = "lowered-ir/1";

const MAGIC: &[u8; 4] = b"MECB";
const MAJOR: u16 = 1;
const MINOR: u16 = 0;
const HEADER_BYTES: u32 = 72;
const DIRECTORY_BYTES: u64 = 32;
const PAYLOAD_OFFSET: u64 = HEADER_BYTES as u64 + DIRECTORY_BYTES;
const CONTENT_HASH_OFFSET: usize = 48;
const RUNTIME_FINGERPRINT: &[u8; 16] = b"meco-bc1-0000001";
const SECTION_LOWERED_GRAMMAR: u16 = 1;

/// Declared tooling capability for a compiled artifact.
///
/// Bytecode/1 retains identical lowered runtime spans under every profile and
/// never embeds source text or names. The profile controls capability checks,
/// not payload compression.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ArtifactDebugProfile {
    #[default]
    Full,
    Mapped,
    Stripped,
}

impl ArtifactDebugProfile {
    const fn flag(self) -> u32 {
        match self {
            Self::Full => 0,
            Self::Mapped => 1,
            Self::Stripped => 2,
        }
    }

    fn from_flag(flag: u32) -> MecoResult<Self> {
        match flag {
            0 => Ok(Self::Full),
            1 => Ok(Self::Mapped),
            2 => Ok(Self::Stripped),
            _ => Err(error(
                DiagnosticCode::BYTECODE_CORRUPT,
                "artifact has an unknown debug profile",
            )),
        }
    }
}

/// Encoder policy for one artifact.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ArtifactOptions {
    pub debug_profile: ArtifactDebugProfile,
}

/// Caller-controlled decoder limits, bounded again by hard implementation caps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArtifactLimits {
    pub maximum_bytes: u64,
    pub maximum_decoded_bytes: u64,
    pub maximum_strings: u32,
    pub maximum_rules: u32,
    pub maximum_productions: u32,
    pub maximum_instructions: u32,
    pub maximum_stack_depth: u32,
    pub maximum_diagnostics: u32,
}

impl ArtifactLimits {
    pub const HARD_MAXIMUM_BYTES: u64 = 64 * 1024 * 1024;
    pub const HARD_MAXIMUM_DECODED_BYTES: u64 = 128 * 1024 * 1024;

    #[must_use]
    pub const fn standard() -> Self {
        Self {
            maximum_bytes: Self::HARD_MAXIMUM_BYTES,
            maximum_decoded_bytes: Self::HARD_MAXIMUM_DECODED_BYTES,
            maximum_strings: 1_000_000,
            maximum_rules: 100_000,
            maximum_productions: 1_000_000,
            maximum_instructions: 4_000_000,
            maximum_stack_depth: 256,
            maximum_diagnostics: 100_000,
        }
    }

    fn validate(self, supplied: usize) -> MecoResult<Self> {
        if self.maximum_bytes > Self::HARD_MAXIMUM_BYTES
            || self.maximum_decoded_bytes > Self::HARD_MAXIMUM_DECODED_BYTES
            || u64::try_from(supplied).unwrap_or(u64::MAX) > self.maximum_bytes
        {
            return Err(error(
                DiagnosticCode::BYTECODE_LIMIT,
                "artifact exceeds its configured byte budget",
            ));
        }
        Ok(self)
    }
}

impl Default for ArtifactLimits {
    fn default() -> Self {
        Self::standard()
    }
}

/// Bounded metadata returned without exposing mutable lowered IR.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactMetadata {
    pub version: &'static str,
    pub debug_profile: ArtifactDebugProfile,
    pub semantic_package_hash: u64,
    pub bytecode_content_hash: u64,
    pub total_bytes: u64,
    pub rule_count: u32,
    pub production_count: u32,
    pub entries: Vec<String>,
    pub default_entry: Option<String>,
}

impl ArtifactMetadata {
    /// Requires development-grade artifact diagnostics.
    ///
    /// # Errors
    ///
    /// Returns `E_BYTECODE_CAPABILITY` for mapped or stripped artifacts.
    pub fn require_full_debug(&self) -> MecoResult<()> {
        if self.debug_profile != ArtifactDebugProfile::Full {
            return Err(error(
                DiagnosticCode::BYTECODE_CAPABILITY,
                "artifact does not retain the requested full debug capability",
            ));
        }
        Ok(())
    }
}

/// Encodes a canonical owned `bytecode/1` artifact.
///
/// # Errors
///
/// Returns `E_BYTECODE_LIMIT` if the canonical output exceeds the hard format
/// ceiling, or `E_BYTECODE_CORRUPT` if the supplied internal grammar violates
/// the shared lowered invariant contract.
pub fn encode_artifact(grammar: &CompiledGrammar, options: ArtifactOptions) -> MecoResult<Vec<u8>> {
    grammar.validate_lowered_invariants()?;
    let mut payload = Writer::new();
    encode_grammar(&mut payload, grammar)?;
    let payload = payload.finish();
    let total = PAYLOAD_OFFSET
        .checked_add(u64::try_from(payload.len()).map_err(|_| limit_error())?)
        .ok_or_else(limit_error)?;
    if total > ArtifactLimits::HARD_MAXIMUM_BYTES {
        return Err(limit_error());
    }
    let mut writer = Writer::with_capacity(usize::try_from(total).map_err(|_| limit_error())?);
    writer.bytes(MAGIC);
    writer.u16(MAJOR);
    writer.u16(MINOR);
    writer.u32(HEADER_BYTES);
    writer.u32(options.debug_profile.flag());
    writer.u64(total);
    writer.u32(1);
    writer.u32(2);
    writer.u32(crate::API_VERSION);
    writer.u32(0);
    writer.u64(grammar.artifact_hash());
    writer.u64(0);
    writer.bytes(RUNTIME_FINGERPRINT);
    writer.u16(SECTION_LOWERED_GRAMMAR);
    writer.u16(1);
    writer.u32(0);
    writer.u64(PAYLOAD_OFFSET);
    writer.u64(u64::try_from(payload.len()).map_err(|_| limit_error())?);
    writer.u32(1);
    writer.u32(0);
    writer.bytes(&payload);
    let mut bytes = writer.finish();
    let hash = bytecode_hash(&bytes);
    bytes[CONTENT_HASH_OFFSET..CONTENT_HASH_OFFSET + 8].copy_from_slice(&hash.to_le_bytes());
    Ok(bytes)
}

/// Decodes and verifies an owned artifact without compiling source text.
///
/// # Errors
///
/// Returns a stable `E_BYTECODE_*` diagnostic for incompatible, corrupt, or
/// over-budget input. No partial grammar is returned.
pub fn decode_artifact(bytes: &[u8], limits: ArtifactLimits) -> MecoResult<CompiledGrammar> {
    let limits = limits.validate(bytes.len())?;
    let header = validate_container(bytes)?;
    let payload_start = usize::try_from(PAYLOAD_OFFSET).map_err(|_| limit_error())?;
    let mut decoder = Decoder::new(&bytes[payload_start..], limits);
    let grammar = decode_grammar(&mut decoder, header.semantic_hash)?;
    decoder.finish()?;
    grammar.validate_lowered_invariants()?;
    Ok(grammar)
}

/// Decodes bounded metadata and verifies the complete artifact.
///
/// # Errors
///
/// Returns the same stable compatibility, corruption, and limit diagnostics as
/// [`decode_artifact`].
pub fn inspect_artifact(bytes: &[u8], limits: ArtifactLimits) -> MecoResult<ArtifactMetadata> {
    let header = validate_container(bytes)?;
    let grammar = decode_artifact(bytes, limits)?;
    Ok(ArtifactMetadata {
        version: BYTECODE_VERSION,
        debug_profile: header.profile,
        semantic_package_hash: grammar.artifact_hash(),
        bytecode_content_hash: header.content_hash,
        total_bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        rule_count: u32::try_from(grammar.rule_count()).unwrap_or(u32::MAX),
        production_count: u32::try_from(grammar.production_count()).unwrap_or(u32::MAX),
        entries: grammar.entries().map(String::from).collect(),
        default_entry: grammar.default_entry().map(String::from),
    })
}

/// Returns a deterministic human-readable structural listing.
///
/// # Errors
///
/// Returns a stable artifact diagnostic when verification fails.
pub fn disassemble_artifact(bytes: &[u8], limits: ArtifactLimits) -> MecoResult<String> {
    let metadata = inspect_artifact(bytes, limits)?;
    let mut output = format!(
        "{} profile={:?} semantic={:016x} content={:016x} bytes={} rules={} productions={}\n",
        metadata.version,
        metadata.debug_profile,
        metadata.semantic_package_hash,
        metadata.bytecode_content_hash,
        metadata.total_bytes,
        metadata.rule_count,
        metadata.production_count
    );
    for entry in &metadata.entries {
        output.push_str("entry ");
        output.push_str(entry);
        if metadata.default_entry.as_deref() == Some(entry) {
            output.push_str(" default");
        }
        output.push('\n');
    }
    Ok(output)
}

struct Header {
    profile: ArtifactDebugProfile,
    semantic_hash: u64,
    content_hash: u64,
}

fn validate_container(bytes: &[u8]) -> MecoResult<Header> {
    if bytes.len() < usize::try_from(PAYLOAD_OFFSET).unwrap_or(usize::MAX) {
        return Err(error(
            DiagnosticCode::BYTECODE_CORRUPT,
            "artifact is shorter than its header and directory",
        ));
    }
    if bytes.get(..4) != Some(MAGIC.as_slice()) {
        return Err(error(
            DiagnosticCode::BYTECODE_MAGIC,
            "artifact magic is not MECB",
        ));
    }
    let u16_at = |offset| u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
    let u32_at = |offset| {
        u32::from_le_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .expect("bounded header"),
        )
    };
    let u64_at = |offset| {
        u64::from_le_bytes(
            bytes[offset..offset + 8]
                .try_into()
                .expect("bounded header"),
        )
    };
    if u16_at(4) != MAJOR || u16_at(6) != MINOR {
        return Err(error(
            DiagnosticCode::BYTECODE_VERSION,
            "artifact bytecode/1 revision is unsupported",
        ));
    }
    if u32_at(8) != HEADER_BYTES
        || u32_at(24) != 1
        || u32_at(28) != 2
        || u32_at(32) != crate::API_VERSION
        || u32_at(36) != 0
        || bytes.get(56..72) != Some(RUNTIME_FINGERPRINT.as_slice())
    {
        return Err(error(
            DiagnosticCode::BYTECODE_CONTRACT,
            "artifact contract or runtime fingerprint does not match",
        ));
    }
    if u64_at(16) != u64::try_from(bytes.len()).unwrap_or(u64::MAX)
        || u16_at(72) != SECTION_LOWERED_GRAMMAR
        || u16_at(74) != 1
        || u32_at(76) != 0
        || u64_at(80) != PAYLOAD_OFFSET
        || u64_at(88) != u64::try_from(bytes.len()).unwrap_or(u64::MAX) - PAYLOAD_OFFSET
        || u32_at(96) != 1
        || u32_at(100) != 0
    {
        return Err(error(
            DiagnosticCode::BYTECODE_CORRUPT,
            "artifact directory or declared size is inconsistent",
        ));
    }
    let expected_hash = u64_at(CONTENT_HASH_OFFSET);
    if expected_hash != bytecode_hash(bytes) {
        return Err(error(
            DiagnosticCode::BYTECODE_CORRUPT,
            "artifact content hash does not match",
        ));
    }
    Ok(Header {
        profile: ArtifactDebugProfile::from_flag(u32_at(12))?,
        semantic_hash: u64_at(40),
        content_hash: expected_hash,
    })
}

fn encode_grammar(writer: &mut Writer, grammar: &CompiledGrammar) -> MecoResult<()> {
    writer.vec_len(grammar.inputs.len())?;
    for input in &grammar.inputs {
        writer.string(&input.external_name)?;
        encode_type(writer, &input.type_)?;
    }
    writer.vec_len(grammar.rules.len())?;
    for rule in &grammar.rules {
        encode_rule(writer, rule)?;
    }
    writer.vec_len(grammar.entries.len())?;
    for (name, rule) in &grammar.entries {
        writer.string(name)?;
        writer.index(*rule)?;
    }
    writer.u32(
        grammar
            .default_entry
            .map_or(u32::MAX, |rule| u32::try_from(rule).unwrap_or(u32::MAX - 1)),
    );
    writer.vec_len(grammar.warnings.len())?;
    for warning in &grammar.warnings {
        writer.string(warning.code().as_str())?;
        writer.u8(match warning.severity() {
            Severity::Error => 0,
            Severity::Warning => 1,
        });
        encode_optional_span(writer, warning.span());
        writer.string(warning.message())?;
    }
    writer.vec_len(grammar.message_manifest.messages.len())?;
    for message in &grammar.message_manifest.messages {
        writer.string(&message.id)?;
        writer.vec_len(message.arguments.len())?;
        for argument in &message.arguments {
            writer.string(&argument.name)?;
            encode_schema_type(writer, &argument.type_)?;
        }
    }
    Ok(())
}

fn encode_rule(writer: &mut Writer, rule: &CompiledRule) -> MecoResult<()> {
    writer.string(&rule.name)?;
    writer.vec_len(rule.parameters.len())?;
    for (name, type_) in &rule.parameters {
        writer.string(name)?;
        encode_type(writer, type_)?;
    }
    encode_span(writer, rule.span);
    let mut analysis = 0_u8;
    analysis |= u8::from(rule.analysis.reachable);
    analysis |= u8::from(rule.analysis.productive) << 1;
    analysis |= u8::from(rule.analysis.nullable) << 2;
    analysis |= u8::from(rule.analysis.recursive) << 3;
    writer.u8(analysis);
    writer.bool(rule.message_effect);
    match &rule.static_selection {
        Some(selection) => {
            writer.bool(true);
            writer.vec_len(selection.cumulative.len())?;
            for value in &selection.cumulative {
                writer.u64(*value);
            }
            writer.u64(selection.total);
        }
        None => writer.bool(false),
    }
    writer.vec_len(rule.productions.len())?;
    for production in &rule.productions {
        encode_production(writer, production)?;
    }
    Ok(())
}

fn encode_production(writer: &mut Writer, production: &CompiledProduction) -> MecoResult<()> {
    writer.string(&production.id)?;
    writer.bool(production.authored_id);
    encode_span(writer, production.span);
    encode_weight(writer, &production.weight, 0)?;
    match &production.guard {
        Some(guard) => {
            writer.bool(true);
            encode_guard(writer, guard, 0)?;
        }
        None => writer.bool(false),
    }
    writer.vec_len(production.bindings.len())?;
    for binding in &production.bindings {
        writer.index(binding.rule)?;
        encode_values(writer, &binding.arguments)?;
        writer.index(binding.slot)?;
        writer.string(&binding.name)?;
        encode_span(writer, binding.span);
    }
    writer.vec_len(production.parts.len())?;
    for part in &production.parts {
        encode_part(writer, part)?;
    }
    writer.u32(production.diversity_factor_16_16);
    Ok(())
}

fn encode_part(writer: &mut Writer, part: &CompiledPart) -> MecoResult<()> {
    match part {
        CompiledPart::Literal { text, span } => {
            writer.u8(0);
            writer.string(text)?;
            encode_span(writer, *span);
        }
        CompiledPart::RuleCall {
            rule,
            arguments,
            span,
        } => {
            writer.u8(1);
            writer.index(*rule)?;
            encode_values(writer, arguments)?;
            encode_span(writer, *span);
        }
        CompiledPart::Value { value, span } => {
            writer.u8(2);
            encode_value_operand(writer, value)?;
            encode_span(writer, *span);
        }
        CompiledPart::Capture {
            rule,
            slot,
            name,
            span,
        } => {
            writer.u8(3);
            writer.index(*rule)?;
            writer.index(*slot)?;
            writer.string(name)?;
            encode_span(writer, *span);
        }
        CompiledPart::MessageCall {
            id,
            arguments,
            span,
        } => {
            writer.u8(4);
            writer.string(id)?;
            writer.vec_len(arguments.len())?;
            for (name, value) in arguments {
                writer.string(name)?;
                encode_value_operand(writer, value)?;
            }
            encode_span(writer, *span);
        }
    }
    Ok(())
}

fn encode_values(writer: &mut Writer, values: &[CompiledValue]) -> MecoResult<()> {
    writer.vec_len(values.len())?;
    for value in values {
        encode_value_operand(writer, value)?;
    }
    Ok(())
}

fn encode_value_operand(writer: &mut Writer, value: &CompiledValue) -> MecoResult<()> {
    match value {
        CompiledValue::Input(index) => {
            writer.u8(0);
            writer.index(*index)?;
        }
        CompiledValue::Local(index) => {
            writer.u8(1);
            writer.index(*index)?;
        }
        CompiledValue::Constant(value) => {
            writer.u8(2);
            encode_constant(writer, value)?;
        }
    }
    Ok(())
}

fn encode_constant(writer: &mut Writer, value: &Value) -> MecoResult<()> {
    match value {
        Value::Text(value) => {
            writer.u8(0);
            writer.string(value)?;
        }
        Value::Number(value) => {
            writer.u8(1);
            encode_rational(writer, *value);
        }
        Value::Boolean(value) => {
            writer.u8(2);
            writer.bool(*value);
        }
        Value::Enum(value) => {
            writer.u8(3);
            writer.string(value)?;
        }
    }
    Ok(())
}

fn encode_weight(writer: &mut Writer, weight: &CompiledWeight, depth: u32) -> MecoResult<()> {
    if depth > 256 {
        return Err(limit_error());
    }
    match weight {
        CompiledWeight::Static(value) => {
            writer.u8(0);
            encode_rational(writer, *value);
        }
        CompiledWeight::Dynamic(expression) => {
            writer.u8(1);
            encode_weight_expression(writer, expression, depth + 1)?;
        }
    }
    Ok(())
}

fn encode_weight_expression(
    writer: &mut Writer,
    expression: &CompiledWeightExpression,
    depth: u32,
) -> MecoResult<()> {
    if depth > 256 {
        return Err(limit_error());
    }
    match expression {
        CompiledWeightExpression::Literal(value) => {
            writer.u8(0);
            encode_rational(writer, *value);
        }
        CompiledWeightExpression::Value(value) => {
            writer.u8(1);
            encode_value_operand(writer, value)?;
        }
        CompiledWeightExpression::Add(left, right)
        | CompiledWeightExpression::Subtract(left, right)
        | CompiledWeightExpression::Multiply(left, right) => {
            writer.u8(match expression {
                CompiledWeightExpression::Add(_, _) => 2,
                CompiledWeightExpression::Subtract(_, _) => 3,
                CompiledWeightExpression::Multiply(_, _) => 4,
                CompiledWeightExpression::Literal(_) | CompiledWeightExpression::Value(_) => {
                    unreachable!()
                }
            });
            encode_weight_expression(writer, left, depth + 1)?;
            encode_weight_expression(writer, right, depth + 1)?;
        }
    }
    Ok(())
}

fn encode_guard(writer: &mut Writer, guard: &CompiledGuard, depth: u32) -> MecoResult<()> {
    if depth > 256 {
        return Err(limit_error());
    }
    match guard {
        CompiledGuard::Value(value) => {
            writer.u8(0);
            encode_guard_value(writer, value)?;
        }
        CompiledGuard::Is(left, right)
        | CompiledGuard::IsNot(left, right)
        | CompiledGuard::Less(left, right)
        | CompiledGuard::LessOrEqual(left, right)
        | CompiledGuard::Greater(left, right)
        | CompiledGuard::GreaterOrEqual(left, right) => {
            writer.u8(match guard {
                CompiledGuard::Is(_, _) => 1,
                CompiledGuard::IsNot(_, _) => 2,
                CompiledGuard::Less(_, _) => 3,
                CompiledGuard::LessOrEqual(_, _) => 4,
                CompiledGuard::Greater(_, _) => 5,
                CompiledGuard::GreaterOrEqual(_, _) => 6,
                _ => unreachable!(),
            });
            encode_guard_value(writer, left)?;
            encode_guard_value(writer, right)?;
        }
        CompiledGuard::Not(value) => {
            writer.u8(7);
            encode_guard(writer, value, depth + 1)?;
        }
        CompiledGuard::And(left, right) | CompiledGuard::Or(left, right) => {
            writer.u8(if matches!(guard, CompiledGuard::And(_, _)) {
                8
            } else {
                9
            });
            encode_guard(writer, left, depth + 1)?;
            encode_guard(writer, right, depth + 1)?;
        }
    }
    Ok(())
}

fn encode_guard_value(writer: &mut Writer, value: &CompiledGuardValue) -> MecoResult<()> {
    match value {
        CompiledGuardValue::Value(value) => {
            writer.u8(0);
            encode_value_operand(writer, value)?;
        }
        CompiledGuardValue::Constant(value) => {
            writer.u8(1);
            encode_constant(writer, value)?;
        }
    }
    Ok(())
}

fn encode_type(writer: &mut Writer, type_: &ValueType) -> MecoResult<()> {
    match type_ {
        ValueType::Text => writer.u8(0),
        ValueType::Number => writer.u8(1),
        ValueType::Boolean => writer.u8(2),
        ValueType::Enum { name, variants } => {
            writer.u8(3);
            writer.string(name)?;
            writer.vec_len(variants.len())?;
            for variant in variants {
                writer.string(variant)?;
            }
        }
    }
    Ok(())
}

fn encode_schema_type(writer: &mut Writer, type_: &SchemaType) -> MecoResult<()> {
    match type_ {
        SchemaType::Text => writer.u8(0),
        SchemaType::Number => writer.u8(1),
        SchemaType::Boolean => writer.u8(2),
        SchemaType::Enum(name) => {
            writer.u8(3);
            writer.string(name)?;
        }
    }
    Ok(())
}

fn encode_rational(writer: &mut Writer, value: Rational) {
    writer.i64(value.numerator());
    writer.u64(value.denominator());
}

fn encode_optional_span(writer: &mut Writer, span: Option<Span>) {
    writer.bool(span.is_some());
    if let Some(span) = span {
        encode_span(writer, span);
    }
}

fn encode_span(writer: &mut Writer, span: Span) {
    writer.u32(span.source().get());
    writer.u64(span.start().byte());
    writer.u64(span.start().scalar());
    writer.u64(span.end().byte());
    writer.u64(span.end().scalar());
}

fn decode_grammar(decoder: &mut Decoder<'_>, semantic_hash: u64) -> MecoResult<CompiledGrammar> {
    let input_count = decoder.count(decoder.limits.maximum_strings)?;
    let mut inputs = Vec::with_capacity(input_count);
    for _ in 0..input_count {
        inputs.push(CompiledInput {
            external_name: decoder.string()?,
            type_: decode_type(decoder)?,
        });
    }
    let rule_count = decoder.count(decoder.limits.maximum_rules)?;
    let mut rules = Vec::with_capacity(rule_count);
    let mut productions = 0_u32;
    for _ in 0..rule_count {
        let rule = decode_rule(decoder)?;
        productions = productions
            .checked_add(u32::try_from(rule.productions.len()).map_err(|_| limit_error())?)
            .ok_or_else(limit_error)?;
        if productions > decoder.limits.maximum_productions {
            return Err(limit_error());
        }
        rules.push(rule);
    }
    let entry_count = decoder.count(decoder.limits.maximum_rules)?;
    let mut entries = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        entries.push((decoder.string()?, decoder.index()?));
    }
    let default = decoder.u32()?;
    let default_entry = (default != u32::MAX).then_some(default as usize);
    let warning_count = decoder.count(decoder.limits.maximum_diagnostics)?;
    let mut warnings = Vec::with_capacity(warning_count);
    for _ in 0..warning_count {
        let name = decoder.string()?;
        let code = DiagnosticCode::artifact_warning(&name).ok_or_else(|| {
            error(
                DiagnosticCode::BYTECODE_CORRUPT,
                "artifact contains an unknown compiler warning code",
            )
        })?;
        if decoder.u8()? != 1 {
            return Err(corrupt("artifact warning severity is invalid"));
        }
        let span = if decoder.bool()? {
            Some(decode_span(decoder)?)
        } else {
            None
        };
        warnings.push(Diagnostic::new(
            code,
            Severity::Warning,
            span,
            decoder.string()?,
        ));
    }
    let message_count = decoder.count(decoder.limits.maximum_strings)?;
    let mut messages = Vec::with_capacity(message_count);
    for _ in 0..message_count {
        let id = decoder.string()?;
        let argument_count = decoder.count(decoder.limits.maximum_strings)?;
        let mut arguments = Vec::with_capacity(argument_count);
        for _ in 0..argument_count {
            arguments.push(MessageArgument {
                name: decoder.string()?,
                type_: decode_schema_type(decoder)?,
            });
        }
        messages.push(MessageDefinition { id, arguments });
    }
    Ok(CompiledGrammar {
        artifact_hash: semantic_hash,
        rules,
        inputs,
        entries,
        default_entry,
        warnings,
        message_manifest: MessageManifest { messages },
    })
}

fn decode_rule(decoder: &mut Decoder<'_>) -> MecoResult<CompiledRule> {
    let name = decoder.string()?;
    let parameter_count = decoder.count(decoder.limits.maximum_strings)?;
    let mut parameters = Vec::with_capacity(parameter_count);
    for _ in 0..parameter_count {
        parameters.push((decoder.string()?, decode_type(decoder)?));
    }
    let span = decode_span(decoder)?;
    let analysis = decoder.u8()?;
    if analysis & !0x0f != 0 {
        return Err(corrupt("artifact rule analysis flags are invalid"));
    }
    let message_effect = decoder.bool()?;
    let static_selection = if decoder.bool()? {
        let count = decoder.count(decoder.limits.maximum_productions)?;
        let mut cumulative = Vec::with_capacity(count);
        for _ in 0..count {
            cumulative.push(decoder.u64()?);
        }
        Some(StaticSelection {
            cumulative,
            total: decoder.u64()?,
        })
    } else {
        None
    };
    let production_count = decoder.count(decoder.limits.maximum_productions)?;
    let mut productions = Vec::with_capacity(production_count);
    for _ in 0..production_count {
        productions.push(decode_production(decoder)?);
    }
    Ok(CompiledRule {
        name,
        parameters,
        span,
        productions,
        static_selection,
        analysis: RuleAnalysis {
            reachable: analysis & 1 != 0,
            productive: analysis & 2 != 0,
            nullable: analysis & 4 != 0,
            recursive: analysis & 8 != 0,
        },
        message_effect,
    })
}

fn decode_production(decoder: &mut Decoder<'_>) -> MecoResult<CompiledProduction> {
    let id = decoder.string()?;
    let authored_id = decoder.bool()?;
    let span = decode_span(decoder)?;
    let weight = decode_weight(decoder, 0)?;
    let guard = if decoder.bool()? {
        Some(decode_guard(decoder, 0)?)
    } else {
        None
    };
    let binding_count = decoder.count(decoder.limits.maximum_instructions)?;
    let mut bindings = Vec::with_capacity(binding_count);
    for _ in 0..binding_count {
        decoder.instruction()?;
        bindings.push(CompiledBinding {
            rule: decoder.index()?,
            arguments: decode_values(decoder)?,
            slot: decoder.index()?,
            name: decoder.string()?,
            span: decode_span(decoder)?,
        });
    }
    let part_count = decoder.count(decoder.limits.maximum_instructions)?;
    let mut parts = Vec::with_capacity(part_count);
    for _ in 0..part_count {
        decoder.instruction()?;
        parts.push(decode_part(decoder)?);
    }
    Ok(CompiledProduction {
        id,
        authored_id,
        span,
        weight,
        guard,
        bindings,
        parts,
        diversity_factor_16_16: decoder.u32()?,
    })
}

fn decode_part(decoder: &mut Decoder<'_>) -> MecoResult<CompiledPart> {
    match decoder.u8()? {
        0 => Ok(CompiledPart::Literal {
            text: decoder.string()?,
            span: decode_span(decoder)?,
        }),
        1 => Ok(CompiledPart::RuleCall {
            rule: decoder.index()?,
            arguments: decode_values(decoder)?,
            span: decode_span(decoder)?,
        }),
        2 => Ok(CompiledPart::Value {
            value: decode_value_operand(decoder)?,
            span: decode_span(decoder)?,
        }),
        3 => Ok(CompiledPart::Capture {
            rule: decoder.index()?,
            slot: decoder.index()?,
            name: decoder.string()?,
            span: decode_span(decoder)?,
        }),
        4 => {
            let id = decoder.string()?;
            let count = decoder.count(decoder.limits.maximum_instructions)?;
            let mut arguments = Vec::with_capacity(count);
            for _ in 0..count {
                arguments.push((decoder.string()?, decode_value_operand(decoder)?));
            }
            Ok(CompiledPart::MessageCall {
                id,
                arguments,
                span: decode_span(decoder)?,
            })
        }
        _ => Err(corrupt("artifact contains an unknown body instruction")),
    }
}

fn decode_values(decoder: &mut Decoder<'_>) -> MecoResult<Vec<CompiledValue>> {
    let count = decoder.count(decoder.limits.maximum_instructions)?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(decode_value_operand(decoder)?);
    }
    Ok(values)
}

fn decode_value_operand(decoder: &mut Decoder<'_>) -> MecoResult<CompiledValue> {
    decoder.instruction()?;
    match decoder.u8()? {
        0 => Ok(CompiledValue::Input(decoder.index()?)),
        1 => Ok(CompiledValue::Local(decoder.index()?)),
        2 => Ok(CompiledValue::Constant(decode_constant(decoder)?)),
        _ => Err(corrupt("artifact contains an unknown value operand")),
    }
}

fn decode_constant(decoder: &mut Decoder<'_>) -> MecoResult<Value> {
    match decoder.u8()? {
        0 => Ok(Value::Text(decoder.string()?)),
        1 => Ok(Value::Number(decode_rational(decoder)?)),
        2 => Ok(Value::Boolean(decoder.bool()?)),
        3 => Ok(Value::Enum(decoder.string()?)),
        _ => Err(corrupt("artifact contains an unknown constant")),
    }
}

fn decode_weight(decoder: &mut Decoder<'_>, depth: u32) -> MecoResult<CompiledWeight> {
    decoder.depth(depth)?;
    decoder.instruction()?;
    match decoder.u8()? {
        0 => Ok(CompiledWeight::Static(decode_rational(decoder)?)),
        1 => Ok(CompiledWeight::Dynamic(decode_weight_expression(
            decoder,
            depth + 1,
        )?)),
        _ => Err(corrupt("artifact contains an unknown weight instruction")),
    }
}

fn decode_weight_expression(
    decoder: &mut Decoder<'_>,
    depth: u32,
) -> MecoResult<CompiledWeightExpression> {
    decoder.depth(depth)?;
    decoder.instruction()?;
    match decoder.u8()? {
        0 => Ok(CompiledWeightExpression::Literal(decode_rational(decoder)?)),
        1 => Ok(CompiledWeightExpression::Value(decode_value_operand(
            decoder,
        )?)),
        opcode @ 2..=4 => {
            let left = decode_weight_expression(decoder, depth + 1)?;
            let right = decode_weight_expression(decoder, depth + 1)?;
            Ok(match opcode {
                2 => CompiledWeightExpression::Add(Box::new(left), Box::new(right)),
                3 => CompiledWeightExpression::Subtract(Box::new(left), Box::new(right)),
                4 => CompiledWeightExpression::Multiply(Box::new(left), Box::new(right)),
                _ => unreachable!(),
            })
        }
        _ => Err(corrupt("artifact contains an unknown weight expression")),
    }
}

fn decode_guard(decoder: &mut Decoder<'_>, depth: u32) -> MecoResult<CompiledGuard> {
    decoder.depth(depth)?;
    decoder.instruction()?;
    match decoder.u8()? {
        0 => Ok(CompiledGuard::Value(decode_guard_value(decoder)?)),
        opcode @ 1..=6 => {
            let left = decode_guard_value(decoder)?;
            let right = decode_guard_value(decoder)?;
            Ok(match opcode {
                1 => CompiledGuard::Is(left, right),
                2 => CompiledGuard::IsNot(left, right),
                3 => CompiledGuard::Less(left, right),
                4 => CompiledGuard::LessOrEqual(left, right),
                5 => CompiledGuard::Greater(left, right),
                6 => CompiledGuard::GreaterOrEqual(left, right),
                _ => unreachable!(),
            })
        }
        7 => Ok(CompiledGuard::Not(Box::new(decode_guard(
            decoder,
            depth + 1,
        )?))),
        opcode @ 8..=9 => {
            let left = decode_guard(decoder, depth + 1)?;
            let right = decode_guard(decoder, depth + 1)?;
            Ok(if opcode == 8 {
                CompiledGuard::And(Box::new(left), Box::new(right))
            } else {
                CompiledGuard::Or(Box::new(left), Box::new(right))
            })
        }
        _ => Err(corrupt("artifact contains an unknown guard instruction")),
    }
}

fn decode_guard_value(decoder: &mut Decoder<'_>) -> MecoResult<CompiledGuardValue> {
    match decoder.u8()? {
        0 => Ok(CompiledGuardValue::Value(decode_value_operand(decoder)?)),
        1 => Ok(CompiledGuardValue::Constant(decode_constant(decoder)?)),
        _ => Err(corrupt("artifact contains an unknown guard value")),
    }
}

fn decode_type(decoder: &mut Decoder<'_>) -> MecoResult<ValueType> {
    match decoder.u8()? {
        0 => Ok(ValueType::Text),
        1 => Ok(ValueType::Number),
        2 => Ok(ValueType::Boolean),
        3 => {
            let name = decoder.string()?;
            let count = decoder.count(decoder.limits.maximum_strings)?;
            let mut variants = Vec::with_capacity(count);
            for _ in 0..count {
                variants.push(decoder.string()?);
            }
            Ok(ValueType::Enum { name, variants })
        }
        _ => Err(corrupt("artifact contains an unknown value type")),
    }
}

fn decode_schema_type(decoder: &mut Decoder<'_>) -> MecoResult<SchemaType> {
    match decoder.u8()? {
        0 => Ok(SchemaType::Text),
        1 => Ok(SchemaType::Number),
        2 => Ok(SchemaType::Boolean),
        3 => Ok(SchemaType::Enum(decoder.string()?)),
        _ => Err(corrupt("artifact contains an unknown schema type")),
    }
}

fn decode_rational(decoder: &mut Decoder<'_>) -> MecoResult<Rational> {
    Rational::new(decoder.i64()?, decoder.u64()?)
        .map_err(|_| corrupt("artifact contains a noncanonical rational"))
}

fn decode_span(decoder: &mut Decoder<'_>) -> MecoResult<Span> {
    Span::new(
        SourceId::new(decoder.u32()?),
        SourcePosition::new(decoder.u64()?, decoder.u64()?),
        SourcePosition::new(decoder.u64()?, decoder.u64()?),
    )
    .map_err(|_| corrupt("artifact contains a reversed source span"))
}

struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    const fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn with_capacity(capacity: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(capacity),
        }
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }

    fn bytes(&mut self, value: &[u8]) {
        self.bytes.extend_from_slice(value);
    }

    fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    fn u16(&mut self, value: u16) {
        self.bytes(value.to_le_bytes().as_slice());
    }

    fn u32(&mut self, value: u32) {
        self.bytes(value.to_le_bytes().as_slice());
    }

    fn u64(&mut self, value: u64) {
        self.bytes(value.to_le_bytes().as_slice());
    }

    fn i64(&mut self, value: i64) {
        self.bytes(value.to_le_bytes().as_slice());
    }

    fn vec_len(&mut self, value: usize) -> MecoResult<()> {
        self.u32(u32::try_from(value).map_err(|_| limit_error())?);
        Ok(())
    }

    fn index(&mut self, value: usize) -> MecoResult<()> {
        self.u32(u32::try_from(value).map_err(|_| limit_error())?);
        Ok(())
    }

    fn string(&mut self, value: &str) -> MecoResult<()> {
        self.vec_len(value.len())?;
        self.bytes(value.as_bytes());
        Ok(())
    }
}

struct Decoder<'a> {
    bytes: &'a [u8],
    cursor: usize,
    limits: ArtifactLimits,
    decoded_bytes: u64,
    strings: u32,
    instructions: u32,
}

impl<'a> Decoder<'a> {
    fn new(bytes: &'a [u8], limits: ArtifactLimits) -> Self {
        Self {
            bytes,
            cursor: 0,
            limits,
            decoded_bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            strings: 0,
            instructions: 0,
        }
    }

    fn finish(&self) -> MecoResult<()> {
        if self.cursor != self.bytes.len() {
            return Err(corrupt("artifact payload contains trailing bytes"));
        }
        Ok(())
    }

    fn take(&mut self, count: usize) -> MecoResult<&'a [u8]> {
        let end = self
            .cursor
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| corrupt("artifact payload is truncated"))?;
        let value = &self.bytes[self.cursor..end];
        self.cursor = end;
        Ok(value)
    }

    fn u8(&mut self) -> MecoResult<u8> {
        Ok(self.take(1)?[0])
    }

    fn bool(&mut self) -> MecoResult<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(corrupt("artifact boolean is not zero or one")),
        }
    }

    fn u32(&mut self) -> MecoResult<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("exact decoder slice"),
        ))
    }

    fn u64(&mut self) -> MecoResult<u64> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("exact decoder slice"),
        ))
    }

    fn i64(&mut self) -> MecoResult<i64> {
        Ok(i64::from_le_bytes(
            self.take(8)?.try_into().expect("exact decoder slice"),
        ))
    }

    fn index(&mut self) -> MecoResult<usize> {
        Ok(self.u32()? as usize)
    }

    fn count(&mut self, maximum: u32) -> MecoResult<usize> {
        let value = self.u32()?;
        if value > maximum {
            return Err(limit_error());
        }
        Ok(value as usize)
    }

    fn string(&mut self) -> MecoResult<String> {
        self.strings = self.strings.checked_add(1).ok_or_else(limit_error)?;
        if self.strings > self.limits.maximum_strings {
            return Err(limit_error());
        }
        let length = self.count(1_048_576)?;
        self.decoded_bytes = self
            .decoded_bytes
            .checked_add(u64::try_from(length).map_err(|_| limit_error())?)
            .ok_or_else(limit_error)?;
        if self.decoded_bytes > self.limits.maximum_decoded_bytes {
            return Err(limit_error());
        }
        let bytes = self.take(length)?;
        let value = core::str::from_utf8(bytes)
            .map_err(|_| corrupt("artifact string is not valid UTF-8"))?;
        Ok(String::from(value))
    }

    fn instruction(&mut self) -> MecoResult<()> {
        self.instructions = self.instructions.checked_add(1).ok_or_else(limit_error)?;
        if self.instructions > self.limits.maximum_instructions {
            return Err(limit_error());
        }
        Ok(())
    }

    fn depth(&self, depth: u32) -> MecoResult<()> {
        if depth > self.limits.maximum_stack_depth {
            return Err(limit_error());
        }
        Ok(())
    }
}

fn bytecode_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for (index, value) in bytes.iter().copied().enumerate() {
        let value = if (CONTENT_HASH_OFFSET..CONTENT_HASH_OFFSET + 8).contains(&index) {
            0
        } else {
            value
        };
        hash ^= u64::from(value);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn corrupt(message: &'static str) -> MecoError {
    error(DiagnosticCode::BYTECODE_CORRUPT, message)
}

fn limit_error() -> MecoError {
    error(
        DiagnosticCode::BYTECODE_LIMIT,
        "artifact exceeds a configured structural or decoded-memory limit",
    )
}

fn error(code: DiagnosticCode, message: impl Into<String>) -> MecoError {
    MecoError::new(Diagnostic::new(code, Severity::Error, None, message))
}

#[cfg(test)]
mod tests {
    use alloc::{string::ToString, vec};

    use super::{
        ArtifactLimits, ArtifactOptions, decode_artifact, disassemble_artifact, encode_artifact,
        inspect_artifact,
    };
    use crate::{
        DiagnosticCode, GenerationRequest, PackageInput, PackageSource, SourceFile, SourceId,
        compile_package,
    };

    fn grammar() -> crate::CompiledGrammar {
        compile_package(&PackageInput {
            root_id: "tiny".to_string(),
            modules: vec![PackageSource {
                canonical_id: "tiny".to_string(),
                source: SourceFile::new(
                    SourceId::new(0),
                    "tiny.meco",
                    "---\nmeco: 2\nmodule: tiny\nentry: line\nexports: [line]\n---\n\n# line\n- hello\n",
                ),
                resolved_imports: vec![],
            }],
        })
        .expect("tiny grammar compiles")
    }

    #[test]
    fn canonical_round_trip_preserves_weighted_generation() {
        let source = grammar();
        let first = encode_artifact(&source, ArtifactOptions::default()).expect("encode");
        let decoded = decode_artifact(&first, ArtifactLimits::default()).expect("decode");
        let second = encode_artifact(&decoded, ArtifactOptions::default()).expect("re-encode");
        assert_eq!(first, second);
        assert_eq!(source.artifact_hash(), decoded.artifact_hash());
        assert_eq!(
            source
                .generate_weighted(&GenerationRequest::with_seed(7))
                .expect("source generate"),
            decoded
                .generate_weighted(&GenerationRequest::with_seed(7))
                .expect("artifact generate")
        );
        let metadata = inspect_artifact(&first, ArtifactLimits::default()).expect("inspect");
        assert_eq!(metadata.rule_count, 1);
        assert!(
            disassemble_artifact(&first, ArtifactLimits::default())
                .expect("disassemble")
                .contains("entry tiny.line default")
        );
    }

    #[test]
    fn header_hash_and_limits_fail_with_stable_codes() {
        let bytes = encode_artifact(&grammar(), ArtifactOptions::default()).expect("encode");
        let mut bad_magic = bytes.clone();
        bad_magic[0] = 0;
        assert_eq!(
            decode_artifact(&bad_magic, ArtifactLimits::default())
                .unwrap_err()
                .diagnostics()[0]
                .code(),
            DiagnosticCode::BYTECODE_MAGIC
        );
        let mut corrupt = bytes.clone();
        *corrupt.last_mut().expect("nonempty artifact") ^= 1;
        assert_eq!(
            decode_artifact(&corrupt, ArtifactLimits::default())
                .unwrap_err()
                .diagnostics()[0]
                .code(),
            DiagnosticCode::BYTECODE_CORRUPT
        );
        let limits = ArtifactLimits {
            maximum_bytes: 1,
            ..ArtifactLimits::default()
        };
        assert_eq!(
            decode_artifact(&bytes, limits).unwrap_err().diagnostics()[0].code(),
            DiagnosticCode::BYTECODE_LIMIT
        );
    }
}
