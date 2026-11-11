//! Record references to named values, types, and callees.

use tree_sitter::Node;

use super::super::declarators::{QualifiedName, qualified_name};
use super::{Context, Walker};
use crate::languages::syntax::named_children;
use crate::model::{ModulePath, Reference, ReferenceKind};

impl<'tree> Walker<'_> {
    /// Calls through an object need its type, so only named callees are references.
    pub(super) fn call(&mut self, node: Node<'tree>, context: &Context<'tree>) {
        match node.child_by_field_name("function") {
            Some(callee)
                if matches!(
                    callee.kind(),
                    "identifier" | "qualified_identifier" | "template_function"
                ) =>
            {
                self.reference(callee, ReferenceKind::Call, context)
            }
            Some(callee) => self.visit(callee, context),
            None => {}
        }
        self.visit_field(node, "arguments", context);
    }

    pub(super) fn field_initializer(&mut self, node: Node<'tree>, context: &Context<'tree>) {
        for child in named_children(node) {
            if child.kind() == "field_identifier" {
                self.reference(child, ReferenceKind::MemberInitializer, context);
            } else {
                self.visit(child, context);
            }
        }
    }

    pub(super) fn reference(
        &mut self,
        node: Node<'tree>,
        kind: ReferenceKind,
        context: &Context<'tree>,
    ) {
        self.visit_template_arguments(node, context);
        let Some(QualifiedName {
            mut scope,
            name,
            global,
        }) = self.name_of(node)
        else {
            return;
        };
        scope.push(name);
        let owner = context.function;
        let range = owner.map_or(self.root.clone(), |function| function.byte_range());
        self.store.references.push(Reference {
            path: scope,
            module: ModulePath(if global {
                Vec::new()
            } else {
                context.module.clone()
            }),
            kind,
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
            scope_start: range.start,
            scope_end: range.end,
            call_owner: owner
                .filter(|_| kind == ReferenceKind::Call)
                .map(|function| function.start_byte()),
            execution_owner: owner.map(|function| function.start_byte()),
            access: None,
            context_id: 0,
        });
    }

    /// C keeps struct, union, and enum tags apart from ordinary names, so a
    /// tag is named with its keyword, as in `struct node`.
    pub(super) fn name_of(&self, node: Node<'_>) -> Option<QualifiedName> {
        let mut name = qualified_name(node, self.source)?;
        let keyword = node.parent().and_then(|parent| match parent.kind() {
            "struct_specifier" => Some("struct"),
            "union_specifier" => Some("union"),
            "enum_specifier" => Some("enum"),
            _ => None,
        });
        if let Some(keyword) = keyword.filter(|_| !self.cpp) {
            name.name = format!("{keyword} {}", name.name);
        }
        Some(name)
    }

    /// Template arguments and computed scopes inside a name contain their own references.
    fn visit_template_arguments(&mut self, node: Node<'tree>, context: &Context<'tree>) {
        for child in named_children(node) {
            match child.kind() {
                "template_argument_list" | "decltype" => self.visit(child, context),
                "qualified_identifier"
                | "template_type"
                | "template_function"
                | "template_method" => self.visit_template_arguments(child, context),
                _ => {}
            }
        }
    }
}

/// A qualified name ending in a type names that type, as in `std::vector<int>`.
pub(super) fn qualified_kind(node: Node<'_>) -> ReferenceKind {
    let mut current = node;
    while let Some(name) = current.child_by_field_name("name") {
        current = name;
    }
    match current.kind() {
        "type_identifier" | "template_type" => ReferenceKind::Type,
        _ => ReferenceKind::Qualified,
    }
}
