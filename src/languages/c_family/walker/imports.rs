//! Visit includes, macro definitions, and using declarations.

use tree_sitter::Node;

use super::super::declarators::qualified_name;
use super::{Context, Walker};
use crate::languages::syntax::{named_children, text};
use crate::model::{DefinitionKind, Import, ModulePath};

impl<'tree> Walker<'_> {
    /// `using namespace std;` imports a namespace; `using std::vector;` imports one name.
    pub(super) fn using_declaration(&mut self, node: Node<'tree>, context: &Context<'tree>) {
        let Some(target) = named_children(node).last() else {
            return;
        };
        let Some(name) = qualified_name(target, self.source) else {
            return;
        };
        let mut cursor = node.walk();
        let namespace = node
            .children(&mut cursor)
            .any(|child| child.kind() == "namespace");
        let scope = match context.function {
            Some(function) => function.byte_range(),
            None => self.root.clone(),
        };
        let mut path = name.scope;
        path.push(name.name.clone());
        self.store.imports.push(Import {
            path,
            alias: None,
            source: None,
            imported_name: (!namespace).then_some(name.name),
            namespace,
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
            module: ModulePath(context.module.clone()),
            scope_start: scope.start,
            scope_end: scope.end,
            is_public: false,
            context_id: 0,
        });
    }

    /// Quoted includes keep their path; system includes keep their angle brackets.
    pub(super) fn include(&mut self, node: Node<'tree>) {
        let Some(path) = node.child_by_field_name("path") else {
            return;
        };
        let source = match path.kind() {
            "string_literal" => text(path, self.source).trim_matches('"').to_owned(),
            "system_lib_string" => text(path, self.source),
            _ => return,
        };
        self.store.imports.push(Import {
            path: Vec::new(),
            alias: None,
            source: Some(source),
            imported_name: None,
            namespace: true,
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
            module: ModulePath::default(),
            scope_start: self.root.start,
            scope_end: self.root.end,
            is_public: false,
            context_id: 0,
        });
    }

    /// Macros ignore namespaces and stay defined after the enclosing block.
    pub(super) fn macro_definition(&mut self, node: Node<'tree>) {
        if let Some(name) = node.child_by_field_name("name") {
            self.define(name, DefinitionKind::Macro, node, true, &Context::default());
        }
    }
}
