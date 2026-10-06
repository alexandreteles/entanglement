use std::ops::Range;

use tree_sitter::{Node, QueryCapture};

use super::captures::{Captures, Role};
use super::scopes;
use super::syntax::{contains, import, is_exported, reference_path, text, type_kind};
use crate::model::{
    Definition, DefinitionKind, Import, LocalBinding, ModulePath, Reference, ReferenceKind,
};

#[derive(Default)]
pub(super) struct SemanticFacts {
    pub definitions: Vec<Definition>,
    pub imports: Vec<Import>,
    pub references: Vec<Reference>,
    pub locals: Vec<LocalBinding>,
}

pub(super) struct CaptureFacts {
    root: Range<usize>,
    semantic: SemanticFacts,
    occupied: Vec<Range<usize>>,
    references: Vec<Reference>,
}

impl CaptureFacts {
    pub(super) fn new(root: Range<usize>) -> Self {
        Self {
            root,
            semantic: SemanticFacts::default(),
            occupied: Vec::new(),
            references: Vec::new(),
        }
    }

    pub(super) fn record(
        &mut self,
        query: &Captures,
        captures: &[QueryCapture<'_>],
        source: &[u8],
    ) {
        let name = query.name(captures);
        for capture in captures {
            let node = capture.node;
            match query.role(capture.index) {
                Some(Role::Definition(kind)) => {
                    self.add_definition(node, name.unwrap_or(node), kind, source)
                }
                Some(Role::Import) => self.semantic.imports.push(import(node, source)),
                Some(Role::Parameter) => {
                    self.add_local(node, scopes::parameter_scope(node), source)
                }
                Some(Role::Declaration) => {
                    self.add_local(node, scopes::declaration_scope(node), source)
                }
                Some(Role::Ignored) => self.occupied.push(node.byte_range()),
                Some(Role::Reference(kind)) => {
                    self.references.push(self.reference(node, kind, source))
                }
                None => {}
            }
        }
    }

    /// Keep the most specific reference for each source range. Calls claim
    /// their target, qualified names claim their operand, and declared names
    /// are bindings rather than references.
    pub(super) fn finish(mut self) -> SemanticFacts {
        self.references.sort_by_key(|item| {
            (
                item.kind != ReferenceKind::Call,
                item.path.len() == 1,
                item.start_byte,
            )
        });
        let mut claimed = self.occupied;
        for reference in self.references {
            let span = reference.start_byte..reference.end_byte;
            if !claimed.iter().any(|range| contains(range, &span)) {
                claimed.push(span);
                self.semantic.references.push(reference);
            }
        }
        self.semantic
    }

    fn add_definition(
        &mut self,
        node: Node<'_>,
        name: Node<'_>,
        kind: DefinitionKind,
        source: &[u8],
    ) {
        if kind != DefinitionKind::Method && scopes::enclosing_function(node).is_some() {
            return self.add_local(name, scopes::declaration_scope(name), source);
        }
        self.occupied.push(name.byte_range());
        let name = text(name, source);
        self.semantic.definitions.push(Definition {
            is_public: is_exported(&name),
            kind: match kind {
                DefinitionKind::Other => type_kind(node),
                kind => kind,
            },
            name,
            module: ModulePath::default(),
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
            scope_start: self.root.start,
            scope_end: self.root.end,
            inline_module: false,
            external_module: false,
            context_id: 0,
        });
    }

    /// A declared name is never a reference, even when it binds nothing at
    /// runtime, such as a parameter of a function type.
    fn add_local(&mut self, name: Node<'_>, scope: Option<Range<usize>>, source: &[u8]) {
        self.occupied.push(name.byte_range());
        let Some(scope) = scope else {
            return;
        };
        self.semantic.locals.push(LocalBinding {
            name: text(name, source),
            start_byte: name.start_byte(),
            end_byte: name.end_byte(),
            scope_start: scope.start,
            scope_end: scope.end,
            context_id: 0,
        });
    }

    fn reference(&self, node: Node<'_>, kind: ReferenceKind, source: &[u8]) -> Reference {
        let function = scopes::enclosing_function(node);
        let scope = function.map_or(self.root.clone(), |function| function.byte_range());
        Reference {
            path: reference_path(node, source),
            module: ModulePath::default(),
            kind,
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
            scope_start: scope.start,
            scope_end: scope.end,
            call_owner: function
                .filter(|_| kind == ReferenceKind::Call)
                .map(|function| function.start_byte()),
            context_id: 0,
        }
    }
}
