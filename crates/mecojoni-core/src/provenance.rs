use alloc::string::String;

use crate::Span;

/// Meaning of one optional derivation/provenance trace node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProvenanceKind {
    Production,
    AuthoredText,
    HostValue,
    BoundValue,
    EmittingCapture,
    Binding,
    Message,
}

/// Half-open generated-output range in UTF-8 bytes and Unicode scalar values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputRange {
    start_byte: u64,
    end_byte: u64,
    start_scalar: u64,
    end_scalar: u64,
}

impl OutputRange {
    #[must_use]
    pub const fn new(start_byte: u64, end_byte: u64, start_scalar: u64, end_scalar: u64) -> Self {
        Self {
            start_byte,
            end_byte,
            start_scalar,
            end_scalar,
        }
    }

    #[must_use]
    pub const fn start_byte(self) -> u64 {
        self.start_byte
    }

    #[must_use]
    pub const fn end_byte(self) -> u64 {
        self.end_byte
    }

    #[must_use]
    pub const fn start_scalar(self) -> u64 {
        self.start_scalar
    }

    #[must_use]
    pub const fn end_scalar(self) -> u64 {
        self.end_scalar
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start_byte == self.end_byte && self.start_scalar == self.end_scalar
    }

    #[must_use]
    pub const fn overlaps(self, other: Self) -> bool {
        self.start_scalar < other.end_scalar && other.start_scalar < self.end_scalar
    }
}

/// One stable source-to-output derivation node retained only when requested.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProvenanceNode {
    id: u32,
    parent: Option<u32>,
    kind: ProvenanceKind,
    rule: String,
    production_id: String,
    source_span: Span,
    output: Option<OutputRange>,
    depth: u32,
    name: Option<String>,
}

impl ProvenanceNode {
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn new(
        id: u32,
        parent: Option<u32>,
        kind: ProvenanceKind,
        rule: String,
        production_id: String,
        source_span: Span,
        output: Option<OutputRange>,
        depth: u32,
        name: Option<String>,
    ) -> Self {
        Self {
            id,
            parent,
            kind,
            rule,
            production_id,
            source_span,
            output,
            depth,
            name,
        }
    }

    pub(crate) fn set_output(&mut self, output: Option<OutputRange>) {
        self.output = output;
    }

    #[must_use]
    pub const fn id(&self) -> u32 {
        self.id
    }

    #[must_use]
    pub const fn parent(&self) -> Option<u32> {
        self.parent
    }

    #[must_use]
    pub const fn kind(&self) -> ProvenanceKind {
        self.kind
    }

    #[must_use]
    pub fn rule(&self) -> &str {
        &self.rule
    }

    #[must_use]
    pub fn production_id(&self) -> &str {
        &self.production_id
    }

    #[must_use]
    pub const fn source_span(&self) -> Span {
        self.source_span
    }

    #[must_use]
    pub const fn output(&self) -> Option<OutputRange> {
        self.output
    }

    #[must_use]
    pub const fn depth(&self) -> u32 {
        self.depth
    }

    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}
