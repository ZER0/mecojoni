//! Stable public policy types and the private lowered-grammar serialization boundary.
//!
//! `CompiledGrammar` is immutable after source compilation or artifact decoding.
//! Both construction paths must pass the same invariant verifier before a value
//! reaches generation. The binary container is implemented separately from the
//! runtime so generation cannot observe how the grammar was constructed.

/// Experimental compiled-artifact compatibility identifier.
pub const BYTECODE_VERSION: &str = "bytecode/0";

/// Source/debug information retained in a compiled artifact.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ArtifactDebugProfile {
    /// Retain complete diagnostics and source-coordinate information.
    #[default]
    Full,
    /// Retain stable source coordinates but omit authoring-only detail.
    Mapped,
    /// Retain only stable runtime identities and minimum provenance coordinates.
    Stripped,
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
            maximum_stack_depth: 4_096,
            maximum_diagnostics: 100_000,
        }
    }
}

impl Default for ArtifactLimits {
    fn default() -> Self {
        Self::standard()
    }
}

/// Versioned invariants shared by source compilation and artifact decoding.
pub const LOWERED_IR_CONTRACT: &str = "lowered-ir/1";
