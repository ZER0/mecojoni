use alloc::{boxed::Box, string::String, vec::Vec};

use crate::{FrontMatter, Rational, Span, Spanned};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleSyntax {
    pub front_matter: FrontMatter,
    pub rules: Vec<RuleSyntax>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleSyntax {
    pub name: Spanned<String>,
    pub parameters: Vec<ParameterSyntax>,
    pub productions: Vec<ProductionSyntax>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParameterSyntax {
    pub name: Spanned<String>,
    pub type_name: Spanned<String>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductionSyntax {
    pub weight: WeightSyntax,
    pub authored_id: Option<Spanned<String>>,
    pub clauses: Vec<ClauseSyntax>,
    pub body: BodySyntax,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WeightSyntax {
    Default,
    Static(Spanned<Rational>),
    Dynamic(Spanned<WeightExpression>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WeightExpression {
    Literal(Rational),
    Name(String),
    Add(Box<Self>, Box<Self>),
    Subtract(Box<Self>, Box<Self>),
    Multiply(Box<Self>, Box<Self>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClauseSyntax {
    Guard(Spanned<GuardExpression>),
    Binding(BindingSyntax),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindingSyntax {
    pub rule: Spanned<String>,
    pub arguments: Vec<ArgumentSyntax>,
    pub name: Spanned<String>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GuardExpression {
    Value(GuardValue),
    Is(GuardValue, GuardValue),
    IsNot(GuardValue, GuardValue),
    Less(GuardValue, GuardValue),
    LessOrEqual(GuardValue, GuardValue),
    Greater(GuardValue, GuardValue),
    GreaterOrEqual(GuardValue, GuardValue),
    Not(Box<Self>),
    And(Box<Self>, Box<Self>),
    Or(Box<Self>, Box<Self>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GuardValue {
    Name(String),
    Number(Rational),
    Boolean(bool),
    Text(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BodySyntax {
    Parts(Vec<BodyPartSyntax>),
    Block(BlockSyntax),
    Empty(Span),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BodyPartSyntax {
    Literal(Spanned<String>),
    RuleReference(Spanned<String>),
    EmittingCapture {
        rule: Spanned<String>,
        name: Spanned<String>,
        span: Span,
    },
    ValueReference(Spanned<String>),
    RuleCall(CallSyntax),
    MessageCall(CallSyntax),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallSyntax {
    pub target: Spanned<String>,
    pub arguments: Vec<ArgumentSyntax>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArgumentSyntax {
    pub name: Spanned<String>,
    pub value: ValueSyntax,
    pub punned: bool,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValueSyntax {
    Reference(Spanned<String>),
    Number(Spanned<Rational>),
    Text(Spanned<String>),
    Boolean(Spanned<bool>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockChomp {
    Clip,
    Strip,
    Keep,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockSyntax {
    pub text: Spanned<String>,
    /// Parsed interpolation parts for a cooked block; raw blocks use `None`.
    pub parts: Option<Vec<BodyPartSyntax>>,
    pub raw: bool,
    pub chomp: BlockChomp,
    pub span: Span,
}
