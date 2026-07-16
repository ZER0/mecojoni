use alloc::{string::String, vec::Vec};

use crate::{Diagnostic, MecoResult, Value};

/// Public scalar schema used by formatter manifests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SchemaType {
    Text,
    Number,
    Boolean,
    /// Qualified enum type name, for example `npc.Mood`.
    Enum(String),
}

/// One named argument required by a stable external message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageArgument {
    pub name: String,
    pub type_: SchemaType,
}

/// One message and its exact argument schema.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageDefinition {
    pub id: String,
    pub arguments: Vec<MessageArgument>,
}

/// Preloaded formatter schema supplied at compilation time.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MessageManifest {
    pub messages: Vec<MessageDefinition>,
}

/// One immutable host input exposed by a compiled package.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputDefinition {
    pub name: String,
    pub type_: SchemaType,
}

/// Compiler-produced cross-boundary schema for host inputs and messages.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PackageManifest {
    pub inputs: Vec<InputDefinition>,
    pub messages: MessageManifest,
}

/// Explicit requested locale and ordered fallback chain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocaleRequest<'a> {
    pub requested: &'a str,
    pub fallbacks: &'a [&'a str],
}

/// Owned request passed to a synchronous host formatter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FormatterRequest {
    message_id: String,
    arguments: Vec<(String, Value)>,
    requested_locale: String,
    fallback_locales: Vec<String>,
}

impl FormatterRequest {
    pub(crate) const fn new(
        message_id: String,
        arguments: Vec<(String, Value)>,
        requested_locale: String,
        fallback_locales: Vec<String>,
    ) -> Self {
        Self {
            message_id,
            arguments,
            requested_locale,
            fallback_locales,
        }
    }

    #[must_use]
    pub fn message_id(&self) -> &str {
        &self.message_id
    }

    #[must_use]
    pub fn arguments(&self) -> &[(String, Value)] {
        &self.arguments
    }

    #[must_use]
    pub fn requested_locale(&self) -> &str {
        &self.requested_locale
    }

    #[must_use]
    pub fn fallback_locales(&self) -> &[String] {
        &self.fallback_locales
    }
}

/// Successful deterministic formatter response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FormatterResult {
    pub text: String,
    pub actual_locale: String,
    pub environment_hash: String,
    pub diagnostics: Vec<Diagnostic>,
    pub work_units: u32,
    pub replayable: bool,
}

/// Side-effect-free synchronous formatter over already-loaded resources.
pub trait Formatter {
    /// Resolves one complete message without I/O or ambient state.
    ///
    /// # Errors
    ///
    /// Returns stable formatter diagnostics and no partial text.
    fn format(&mut self, request: &FormatterRequest) -> MecoResult<FormatterResult>;
}

/// Coarse complete-message provenance retained after formatting.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageTrace {
    message_id: String,
    requested_locale: String,
    actual_locale: String,
    environment_hash: String,
    work_units: u32,
    replayable: bool,
}

impl MessageTrace {
    pub(crate) const fn new(
        message_id: String,
        requested_locale: String,
        actual_locale: String,
        environment_hash: String,
        work_units: u32,
        replayable: bool,
    ) -> Self {
        Self {
            message_id,
            requested_locale,
            actual_locale,
            environment_hash,
            work_units,
            replayable,
        }
    }

    #[must_use]
    pub fn message_id(&self) -> &str {
        &self.message_id
    }

    #[must_use]
    pub fn requested_locale(&self) -> &str {
        &self.requested_locale
    }

    #[must_use]
    pub fn actual_locale(&self) -> &str {
        &self.actual_locale
    }

    #[must_use]
    pub fn environment_hash(&self) -> &str {
        &self.environment_hash
    }

    #[must_use]
    pub const fn work_units(&self) -> u32 {
        self.work_units
    }

    #[must_use]
    pub const fn replayable(&self) -> bool {
        self.replayable
    }
}
