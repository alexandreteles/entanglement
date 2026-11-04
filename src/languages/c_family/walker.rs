//! Collect C and C++ definitions, local bindings, includes, and references.

mod bindings;
mod blocks;
mod declarations;
mod imports;
mod keywords;
mod references;
mod types;

use std::ops::Range;

use tree_sitter::Node;

use crate::languages::SemanticStore;
use crate::languages::syntax::{named_children, text};
use crate::model::ReferenceKind;
use references::qualified_kind;

pub(super) fn facts(root: Node<'_>, source: &[u8], cpp: bool) -> SemanticStore {
    let mut walker = Walker {
        source,
        cpp,
        root: root.byte_range(),
        store: SemanticStore::default(),
    };
    walker.visit(root, &Context::default());
    walker.store
}

struct Walker<'source> {
    source: &'source [u8],
    cpp: bool,
    root: Range<usize>,
    store: SemanticStore,
}

/// Where a node sits: its enclosing namespaces and classes, and the code that runs it.
#[derive(Clone, Default)]
struct Context<'tree> {
    module: Vec<String>,
    /// The innermost function or lambda whose body contains the node.
    function: Option<Node<'tree>>,
    /// Declarations directly in a C++ class body are members.
    class: bool,
    /// Names in an anonymous namespace have internal linkage.
    internal: bool,
}

impl<'tree> Context<'tree> {
    fn in_function(&self, function: Node<'tree>) -> Self {
        Self {
            function: Some(function),
            class: false,
            ..self.clone()
        }
    }

    fn is_block_scope(&self) -> bool {
        self.function.is_some() && !self.class
    }
}

impl<'tree> Walker<'_> {
    fn visit(&mut self, node: Node<'tree>, context: &Context<'tree>) {
        if node.is_missing() {
            return;
        }
        match node.kind() {
            "comment"
            | "string_literal"
            | "raw_string_literal"
            | "char_literal"
            | "number_literal"
            | "concatenated_string"
            | "field_identifier"
            | "namespace_identifier"
            | "statement_identifier"
            | "goto_statement"
            | "destructor_name"
            | "operator_name"
            | "field_designator"
            | "preproc_call"
            | "attribute_specifier"
            | "attribute_declaration"
            | "ms_declspec_modifier" => {}
            // Error recovery can present a keyword, such as C++ code in a C header, as a name.
            "identifier" if keywords::is_keyword(&text(node, self.source)) => {}
            "identifier" => self.reference(node, ReferenceKind::Value, context),
            "type_identifier" | "template_type" => {
                self.reference(node, ReferenceKind::Type, context)
            }
            "qualified_identifier" => self.reference(node, qualified_kind(node), context),
            "template_function" => self.reference(node, ReferenceKind::Value, context),
            "call_expression" => self.call(node, context),
            "field_expression" => self.visit_field(node, "argument", context),
            "labeled_statement" => self.visit_except(node, "label", context),
            "preproc_ifdef" | "preproc_elifdef" => self.visit_except(node, "name", context),
            "preproc_if" | "preproc_elif" => self.visit_except(node, "condition", context),
            "preproc_include" => self.include(node),
            "preproc_def" | "preproc_function_def" => self.macro_definition(node),
            "function_definition" => self.function_definition(node, context),
            "declaration" | "field_declaration" => self.declaration(node, context),
            "type_definition" => self.type_definition(node, context),
            "alias_declaration" => self.alias_declaration(node, context),
            "struct_specifier" | "union_specifier" | "class_specifier" | "enum_specifier" => {
                self.type_specifier(node, context)
            }
            "namespace_definition" => self.namespace_definition(node, context),
            "template_declaration" => self.template_declaration(node, context),
            "using_declaration" => self.using_declaration(node, context),
            "lambda_expression" => self.lambda(node, context),
            "for_range_loop" => self.range_loop(node, context),
            "catch_clause" => self.catch_clause(node, context),
            "parameter_list" => self.parameters(node, None, context),
            _ => self.visit_children(node, context),
        }
    }

    fn visit_children(&mut self, node: Node<'tree>, context: &Context<'tree>) {
        for child in named_children(node) {
            self.visit(child, context);
        }
    }

    fn visit_field(&mut self, node: Node<'tree>, field: &str, context: &Context<'tree>) {
        if let Some(child) = node.child_by_field_name(field) {
            self.visit(child, context);
        }
    }

    fn visit_except(&mut self, node: Node<'tree>, field: &str, context: &Context<'tree>) {
        let skipped = node.child_by_field_name(field).map(|child| child.id());
        for child in named_children(node).filter(|child| Some(child.id()) != skipped) {
            self.visit(child, context);
        }
    }
}
