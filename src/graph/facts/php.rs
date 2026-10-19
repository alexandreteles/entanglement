//! PHP syntax evidence that supplements the shared program facts.

use crate::model::SourceRange;

#[derive(Clone, Debug, Default)]
pub(crate) struct PhpFacts {
    pub namespaces: Vec<PhpNamespace>,
    pub imports: Vec<PhpImport>,
    pub declarations: Vec<PhpDeclaration>,
    pub references: Vec<PhpReference>,
    pub traits: Vec<PhpTraitUse>,
    pub includes: Vec<PhpInclude>,
}

#[derive(Clone, Debug)]
pub(crate) struct PhpNamespace {
    pub range: SourceRange,
    pub name: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PhpSymbolKind {
    Class,
    Function,
    Constant,
    Method,
    Property,
}

#[derive(Clone, Debug)]
pub(crate) struct PhpImport {
    pub range: SourceRange,
    pub namespace_start: usize,
    pub kind: PhpSymbolKind,
    pub path: Vec<String>,
    pub alias: String,
}

#[derive(Clone, Debug)]
pub(crate) struct PhpDeclaration {
    pub setter_visibility: Option<super::SemanticVisibility>,
    pub start_byte: usize,
    pub namespace: Vec<String>,
    pub kind: PhpSymbolKind,
    pub is_static: bool,
    pub conditional: bool,
    pub is_trait: bool,
    pub is_interface: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PhpReferenceKind {
    Function,
    Class,
    Constant,
    Method,
    StaticMethod,
    Property,
    StaticProperty,
    ClassConstant,
    Variable,
}

#[derive(Clone, Debug)]
pub(crate) struct PhpReference {
    pub start_byte: usize,
    /// The original name, including namespace separators or a variable prefix.
    pub name: String,
    pub kind: PhpReferenceKind,
    /// Original receiver text. It can be a type name or a variable expression.
    pub receiver: Option<String>,
    /// Source start of the receiver's own property or callable reference.
    pub receiver_reference: Option<usize>,
    pub namespace_start: usize,
    /// Fully qualified lexical class name, without a leading separator.
    pub class: Option<String>,
    /// The syntax invokes the target, rather than creating a callable value.
    pub callable: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct PhpTraitUse {
    pub start_byte: usize,
    pub owner: String,
    pub traits: Vec<String>,
    pub adaptations: Vec<PhpTraitAdaptation>,
}

#[derive(Clone, Debug)]
pub(crate) struct PhpTraitAdaptation {
    pub is_final: bool,
    pub method: String,
    pub trait_name: Option<String>,
    pub alias: Option<String>,
    pub visibility: Option<super::SemanticVisibility>,
    pub instead_of: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct PhpInclude {
    pub conditional: bool,
    pub start_byte: usize,
    /// A literal path, optionally prefixed with the source file directory.
    pub path: Option<String>,
}
