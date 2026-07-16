#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

mod diagnostic;
mod error;
mod front_matter;
mod source;
mod span;

pub use diagnostic::{Diagnostic, DiagnosticCode, Severity};
pub use error::{MecoError, MecoResult};
pub use front_matter::{
    FrontMatter, ImportDeclaration, InputDeclaration, TypeDeclaration, parse_front_matter,
};
pub use source::{SourceError, SourceFile};
pub use span::{SourceId, SourcePosition, Span, SpanError, Spanned};

/// Version of the public Rust API under active development.
pub const API_VERSION: u32 = 1;
