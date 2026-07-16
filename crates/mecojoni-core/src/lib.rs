#![no_std]
#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

extern crate alloc;

mod ast;
mod audit;
mod compiler;
mod diagnostic;
mod diverse;
mod error;
mod formatter;
mod front_matter;
mod package;
mod parser;
mod prng;
mod profile;
mod provenance;
mod rational;
mod source;
mod span;
mod value;

pub use ast::{
    ArgumentSyntax, BindingSyntax, BlockChomp, BlockSyntax, BodyPartSyntax, BodySyntax, CallSyntax,
    ClauseSyntax, GuardExpression, GuardValue, ModuleSyntax, ParameterSyntax, ProductionSyntax,
    RuleSyntax, ValueSyntax, WeightExpression, WeightSyntax,
};
pub use audit::{
    AttributionRole, CompositionFinding, RenderedRepetitionFinding, RepetitionAttribution,
    StructuralRepetitionFinding, WORD_TOKENIZER_VERSION, audit_composition,
    audit_rendered_repetition, audit_structural_repetition, composition_profile_version,
};
pub use compiler::{
    CompiledGrammar, GeneratedContent, GenerationLimits, GenerationRequest, GenerationResult,
    RuleAnalysis, StructuralGenerationResult, WEIGHTED_SAMPLER_VERSION, compile_package,
    compile_package_with_manifest,
};
pub use diagnostic::{Diagnostic, DiagnosticCode, Severity};
pub use diverse::{
    DIVERSE_SAMPLER_VERSION, DiverseGenerationRequest, DiverseResult, FRAGMENT_TOKENIZER_VERSION,
    NORMALIZER_VERSION, RepetitionSnapshot, RepetitionStore, ReplayReceipt, SNAPSHOT_VERSION,
    SamplerSession, SessionSnapshot, SnapshotPolicy,
};
pub use error::{MecoError, MecoResult};
pub use formatter::{
    Formatter, FormatterRequest, FormatterResult, InputDefinition, LocaleRequest, MessageArgument,
    MessageDefinition, MessageManifest, MessageTrace, PackageManifest, SchemaType,
};
pub use front_matter::{
    FrontMatter, ImportDeclaration, InputDeclaration, TypeDeclaration, parse_front_matter,
};
pub use package::{PackageInput, PackageSource, ResolvedImport, validate_package_input};
pub use parser::parse_module;
pub use prng::{PRNG_VERSION, SplitMix64};
pub use profile::{
    COMPOSITION_PROFILE_VERSION, CompositionProfile, INTERACTIVE_PROFILE_VERSION,
    LOCATION_PROFILE_VERSION, LocationProfile, ResourceProfile, diversity_factor_16_16,
    location_cooldown_multiplier,
};
pub use provenance::{OutputRange, ProvenanceKind, ProvenanceNode};
pub use rational::{RATIONAL_LIMIT, RATIONAL_VERSION, Rational, RationalError};
pub use source::{SourceError, SourceFile};
pub use span::{SourceId, SourcePosition, Span, SpanError, Spanned};
pub use value::{BindingTrace, DataBinding, EligibleWeightTrace, SelectionTrace, Value};

/// Version of the public Rust API under active development.
pub const API_VERSION: u32 = 2;
