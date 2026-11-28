//! C# declarations and references keep their physical source positions.
mod accessors;
pub(super) mod declarations;
mod expressions;
mod references;
mod walk;

use crate::model::{DefinitionKind, ReferenceKind};
use crate::structure::model::RelationshipKind;
use crate::structure::types::TypeExpression;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

pub(super) use walk::capture;

#[derive(Clone, Debug, Default)]
pub(crate) struct Data {
    pub declarations: BTreeMap<usize, Decl>,
    pub uses: BTreeMap<(usize, usize), Usage>,
    pub imports: Vec<Using>,
    pub bindings: Vec<Binding>,
    pub errors: Vec<Range<usize>>,
    pub directives: Vec<super::conditional::Line>,
    pub script: Vec<super::script::Directive>,
}

#[derive(Clone, Debug)]
pub(crate) struct Using {
    pub position: usize,
    pub scope: Range<usize>,
    pub namespace: String,
    pub alias: String,
    pub target: String,
    pub global: bool,
    pub is_static: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Binding {
    pub name: String,
    pub scope: Range<usize>,
    pub position: usize,
    pub value: Expression,
}

#[derive(Clone, Debug, Default)]
pub(crate) enum Expression {
    #[default]
    Unknown,
    Type(String),
    Name(String),
    Call(usize),
    Member(Box<Expression>, String),
}

#[derive(Clone, Debug)]
pub(crate) struct Parameter {
    pub name: String,
    pub ty: String,
    pub mode: String,
    pub optional: bool,
    pub variadic: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Decl {
    pub fingerprint: Option<String>,
    pub execution: Option<crate::graph::DeclarationExecutionFacts>,
    pub range: Range<usize>,
    pub name: String,
    pub owner: String,
    pub namespace: String,
    pub category: String,
    pub kind: DefinitionKind,
    pub is_type: bool,
    pub scope: Range<usize>,
    pub visibility: String,
    pub modifiers: BTreeSet<String>,
    pub parameters: Vec<Parameter>,
    pub generics: Vec<String>,
    pub ty: String,
    pub types: Vec<TypeExpression>,
    pub bases: Vec<String>,
    pub extension: Option<String>,
    pub explicit: Option<String>,
    pub body: Option<Range<usize>>,
}

impl Decl {
    pub fn qualified(&self) -> String {
        let name = if self.is_type && !self.generics.is_empty() {
            format!("{}`{}", self.name, self.generics.len())
        } else {
            self.name.clone()
        };
        qualify(&self.owner, &name)
    }
    pub fn signature(&self) -> String {
        let types = self
            .parameters
            .iter()
            .map(|p| format!("{}{}", if p.mode.is_empty() { "" } else { "&" }, p.ty))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{}{}`{}({types})",
            self.explicit
                .as_ref()
                .map_or(String::new(), |n| format!("{n}.")),
            self.name,
            self.generics.len()
        )
    }
    pub fn public(&self) -> bool {
        matches!(
            self.visibility.as_str(),
            "public" | "protected" | "protected internal"
        )
    }
}

pub(crate) fn qualify(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.into()
    } else {
        format!("{prefix}.{name}")
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Argument {
    pub name: Option<String>,
    pub mode: String,
    pub value: Expression,
}

#[derive(Clone, Debug)]
pub(crate) struct Usage {
    pub attribute: bool,
    pub write: bool,
    pub range: Range<usize>,
    pub name: String,
    pub namespace: String,
    pub owner: String,
    pub receiver: Expression,
    pub arguments: Vec<Argument>,
    pub arity: Option<usize>,
    pub kind: ReferenceKind,
    pub relation: RelationshipKind,
    pub constructor: bool,
    pub contract: bool,
}
