//! Shared semantic evidence emitted by language adapters.
//!
//! The types here deliberately store facts rather than resolver decisions.
//! Adapters can record syntax they can prove, while the common resolver and
//! call graph decide whether those facts are sufficient for an exact edge.

use crate::model::SourceRange;

#[derive(Clone, Debug, Default)]
pub(crate) struct SemanticFacts {
    /// The semantic layer that owns these facts, including injected programs.
    pub context_id: usize,
    /// Resolution policy for this layer; grammar details stay in adapters.
    pub resolution_family: Option<crate::languages::ResolutionFamily>,
    pub file_module_rules: Option<&'static crate::languages::FileModuleRules>,
    /// Metadata keyed by the start byte of the corresponding `Definition`.
    pub declarations: Vec<DeclarationFacts>,
    /// Statically declared or narrowly inferred local binding types.
    pub bindings: Vec<BindingType>,
    /// Receiver types that are in scope at a source range.
    pub receivers: Vec<ReceiverScope>,
    /// Type evidence for individual call expressions.
    pub calls: Vec<CallFacts>,
    /// The source package and build context known to the language adapter.
    pub context: Option<SemanticContext>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SemanticContext {
    /// Language-level package name, independent of the source directory.
    pub package: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct DeclarationFacts {
    /// Start byte of the generic definition represented by these facts.
    pub start_byte: usize,
    /// Enclosing type/object names from the outermost owner inward.
    pub owner: Vec<String>,
    /// The access level proven by syntax or the language's default visibility.
    pub visibility: SemanticVisibility,
    /// Class, callable, property, or type-alias metadata.
    pub kind: DeclarationKind,
    /// Explicit declared type for properties and type-like declarations.
    pub declared_type: Option<String>,
    /// Direct statically named supertypes for classes and objects.
    pub supertypes: Vec<String>,
    /// Right-hand type name for a type alias or a simple immutable alias.
    pub alias_target: Option<String>,
    /// Callable parameter and receiver evidence.
    pub signature: Option<CallableSignature>,
    /// Language modifiers that affect cross-source-set identity and lookup.
    pub modifiers: DeclarationModifiers,
    /// This callable or property is a member of an interface, where dispatch
    /// is implicitly virtual unless source syntax proves an explicit `super`.
    pub is_interface_member: bool,
    /// Marker annotations declared on a type, including `@DslMarker` types.
    pub dsl_markers: Vec<String>,
    /// Exact executable ranges attached to a property or accessor declaration.
    pub execution: Option<DeclarationExecutionFacts>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DeclarationExecutionFacts {
    /// A directly initialized callable value, such as `val task = { ... }`.
    pub callable_value: Option<SourceRange>,
    /// Getter body range, when syntax provides a concrete executable body.
    pub getter: Option<SourceRange>,
    /// Setter body range, when syntax provides a concrete executable body.
    pub setter: Option<SourceRange>,
    /// Constructor bodies owned by a class or constructor declaration.
    pub constructors: Vec<SourceRange>,
    /// Property initializer and `init` block scopes run during construction.
    pub initializers: Vec<SourceRange>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SemanticVisibility {
    Public,
    Private,
    Protected,
    Internal,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum DeclarationKind {
    Function,
    Constructor,
    Property,
    Class,
    Object,
    CompanionObject,
    TypeAlias,
    Accessor,
    #[default]
    Other,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DeclarationModifiers {
    pub is_expect: bool,
    pub is_actual: bool,
    pub is_abstract: bool,
    pub is_open: bool,
    pub is_final: bool,
    pub is_override: bool,
    pub is_operator: bool,
    pub is_infix: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CallableSignature {
    pub type_parameters: Vec<CallableTypeParameter>,
    pub parameters: Vec<CallableParameter>,
    pub return_type: Option<String>,
    /// Receiver type for an extension function or extension property.
    pub receiver_type: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CallableTypeParameter {
    pub name: String,
    pub bounds: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CallableParameter {
    pub name: Option<String>,
    pub type_name: Option<String>,
    pub has_default: bool,
    pub is_vararg: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct BindingType {
    /// Bound source name and the byte range of the name token.
    pub name: String,
    pub name_start_byte: usize,
    pub name_end_byte: usize,
    /// Lexical scope in which the binding shadows package declarations.
    pub scope_start: usize,
    pub scope_end: usize,
    /// Explicit type or a type inferred from a direct, unambiguous initializer.
    pub type_name: Option<String>,
    /// A simple value alias, retained only when the initializer is a bare path.
    pub alias_target: Option<String>,
    /// The initializer is an explicit callable reference such as `::handler`.
    pub callable_reference: bool,
    /// `val`/parameter bindings are immutable; `var` bindings are not.
    pub immutable: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ReceiverScope {
    pub start_byte: usize,
    pub end_byte: usize,
    /// The receiver type contributed by this function or lambda scope.
    pub receiver_type: String,
    /// Source label that can explicitly select this receiver.
    pub label: Option<String>,
    /// The receiver is implicit for a lambda with receiver or extension body.
    pub implicit: bool,
    /// `@DslMarker` annotation names carried by the receiver's declared type.
    pub dsl_markers: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CallFacts {
    pub start_byte: usize,
    pub end_byte: usize,
    /// Exact callee token range for this syntactic invocation, when known.
    pub target: Option<SourceRange>,
    /// Syntax evidence requiring an operator or infix declaration.
    pub convention: Option<CallConvention>,
    /// Proven receiver type for a qualified or implicit-receiver call.
    pub receiver_type: Option<String>,
    pub arguments: Vec<CallArgumentFact>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CallConvention {
    Operator,
    Infix,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CallDispatch {
    /// The syntax or static target does not prove the runtime implementation.
    #[default]
    Unknown,
    /// The call has a statically selected, non-overridable target.
    Direct,
    /// The declaration is known, but runtime override dispatch can change it.
    Virtual,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CallArgumentFact {
    pub start_byte: usize,
    pub end_byte: usize,
    pub name: Option<String>,
    pub type_name: Option<String>,
    pub is_lambda: bool,
    /// The lambda appears after the call's parenthesized argument list.
    pub is_trailing_lambda: bool,
    /// A source lambda header is present, including an empty `->` header.
    pub lambda_has_explicit_parameters: bool,
    /// Explicitly annotated lambda parameter types, if present.
    pub lambda_parameter_types: Vec<Option<String>>,
    /// Lambda parameter names in source order, for signature-based inference.
    pub lambda_parameter_names: Vec<String>,
    /// An explicitly labeled lambda, independent of the callee's name.
    pub lambda_label: Option<String>,
    /// Explicit receiver annotation for a receiver lambda, if present.
    pub lambda_receiver_type: Option<String>,
}
