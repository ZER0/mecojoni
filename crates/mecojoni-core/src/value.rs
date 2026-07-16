use alloc::string::String;

use crate::Rational;

/// One deeply owned scalar supplied by a host or carried through a rule frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Value {
    Text(String),
    Number(Rational),
    Boolean(bool),
    /// A finite enum member. Its enum type is determined by the compiled schema.
    Enum(String),
}

impl Value {
    #[must_use]
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::Text(_) => "text",
            Self::Number(_) => "number",
            Self::Boolean(_) => "boolean",
            Self::Enum(_) => "enum",
        }
    }
}

/// One immutable host-data item for a generation request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataBinding {
    pub name: String,
    pub value: Value,
}

impl DataBinding {
    #[must_use]
    pub const fn new(name: String, value: Value) -> Self {
        Self { name, value }
    }
}

/// Optional ordered capture/binding provenance returned by generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindingTrace {
    name: String,
    value: Value,
    emitted: bool,
}

/// One exact eligible production weight retained for replay inspection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EligibleWeightTrace {
    production: u32,
    production_id: String,
    base_weight: Rational,
    normalized_weight: u64,
}

impl EligibleWeightTrace {
    pub(crate) const fn new(
        production: u32,
        production_id: String,
        base_weight: Rational,
        normalized_weight: u64,
    ) -> Self {
        Self {
            production,
            production_id,
            base_weight,
            normalized_weight,
        }
    }

    #[must_use]
    pub const fn production(&self) -> u32 {
        self.production
    }

    #[must_use]
    pub fn production_id(&self) -> &str {
        &self.production_id
    }

    #[must_use]
    pub const fn base_weight(&self) -> Rational {
        self.base_weight
    }

    #[must_use]
    pub const fn normalized_weight(&self) -> u64 {
        self.normalized_weight
    }
}

/// One rule selection and the exact eligible weight set used to choose it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectionTrace {
    rule: String,
    selected_production: u32,
    selected_production_id: String,
    eligible: alloc::vec::Vec<EligibleWeightTrace>,
}

impl SelectionTrace {
    pub(crate) const fn new(
        rule: String,
        selected_production: u32,
        selected_production_id: String,
        eligible: alloc::vec::Vec<EligibleWeightTrace>,
    ) -> Self {
        Self {
            rule,
            selected_production,
            selected_production_id,
            eligible,
        }
    }

    #[must_use]
    pub fn rule(&self) -> &str {
        &self.rule
    }

    #[must_use]
    pub const fn selected_production(&self) -> u32 {
        self.selected_production
    }

    #[must_use]
    pub fn selected_production_id(&self) -> &str {
        &self.selected_production_id
    }

    #[must_use]
    pub fn eligible(&self) -> &[EligibleWeightTrace] {
        &self.eligible
    }
}

impl BindingTrace {
    pub(crate) const fn new(name: String, value: Value, emitted: bool) -> Self {
        Self {
            name,
            value,
            emitted,
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn value(&self) -> &Value {
        &self.value
    }

    #[must_use]
    pub const fn emitted(&self) -> bool {
        self.emitted
    }
}
