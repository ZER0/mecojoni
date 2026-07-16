#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

mod ast;
mod audit;
mod compiler;
mod diagnostic;
mod error;
mod front_matter;
mod package;
mod parser;
mod prng;
mod profile;
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
    CompositionFinding, WORD_TOKENIZER_VERSION, audit_composition, composition_profile_version,
};
pub use compiler::{
    CompiledGrammar, GenerationLimits, GenerationRequest, GenerationResult, RuleAnalysis,
    WEIGHTED_SAMPLER_VERSION, compile_package,
};
pub use diagnostic::{Diagnostic, DiagnosticCode, Severity};
pub use error::{MecoError, MecoResult};
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
pub use rational::{RATIONAL_LIMIT, RATIONAL_VERSION, Rational, RationalError};
pub use source::{SourceError, SourceFile};
pub use span::{SourceId, SourcePosition, Span, SpanError, Spanned};
pub use value::{BindingTrace, DataBinding, EligibleWeightTrace, SelectionTrace, Value};

/// Version of the public Rust API under active development.
pub const API_VERSION: u32 = 2;
