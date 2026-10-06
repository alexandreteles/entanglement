use std::ops::Range;

use tree_sitter::Node;

use crate::model::{Definition, DefinitionKind, Export, LocalBinding};

use super::SemanticFacts;
use super::syntax::{
    ancestors, direct_function, field, is_top_level, module_path, nodes, range, target_names, text,
};

pub(super) fn collect(
    root: Node<'_>,
    source: &[u8],
    facts: &mut SemanticFacts,
) -> Vec<Range<usize>> {
    let mut occupied = Vec::new();
    for node in nodes(root) {
        match node.kind() {
            "function_definition" | "class_definition" => {
                if let Some(name) = field(node, "name") {
                    add_named_definition(node, name, root, source, facts);
                    occupied.push(range(name));
                }
            }
            "assignment" | "augmented_assignment" => {
                add_module_assignment(node, root, source, facts, &mut occupied);
            }
            _ => {}
        }
    }
    occupied
}

fn add_named_definition(
    node: Node<'_>,
    name: Node<'_>,
    root: Node<'_>,
    source: &[u8],
    facts: &mut SemanticFacts,
) {
    let module_level = is_top_level(node);
    let name_text = text(name, source);
    let kind = definition_kind(node);
    let scope = symbol_scope(node, root, module_level);
    let start = node.start_byte();
    facts.definitions.push(Definition {
        name: name_text.clone(),
        kind: kind.clone(),
        module: module_path(),
        start_byte: start,
        end_byte: node.end_byte(),
        scope_start: scope.start,
        scope_end: scope.end,
        is_public: module_level,
        inline_module: false,
        external_module: false,
        context_id: 0,
    });
    if module_level {
        facts.exports.push(Export {
            source: None,
            imported_name: None,
            local_name: Some(name_text.clone()),
            exported_name: name_text,
            namespace: false,
            start_byte: start,
            end_byte: node.end_byte(),
            scope_start: 0,
            scope_end: root.end_byte(),
            context_id: 0,
        });
    }
}

fn definition_kind(node: Node<'_>) -> DefinitionKind {
    if node.kind() == "class_definition" {
        return DefinitionKind::Other;
    }
    let enclosing = ancestors(node)
        .skip(1)
        .find(|item| matches!(item.kind(), "function_definition" | "class_definition"));
    if enclosing.is_some_and(|item| item.kind() == "class_definition") {
        DefinitionKind::Method
    } else {
        DefinitionKind::Function
    }
}

fn symbol_scope(node: Node<'_>, root: Node<'_>, module_level: bool) -> Range<usize> {
    if module_level {
        return root.start_byte()..root.end_byte();
    }
    let owner = ancestors(node)
        .skip(1)
        .find(|item| matches!(item.kind(), "function_definition" | "class_definition"));
    owner
        .and_then(|item| field(item, "body"))
        .map(range)
        .unwrap_or_else(|| range(root))
}

fn add_module_assignment(
    node: Node<'_>,
    root: Node<'_>,
    source: &[u8],
    facts: &mut SemanticFacts,
    occupied: &mut Vec<Range<usize>>,
) {
    if !is_top_level(node) {
        return;
    }
    let Some(left) = field(node, "left") else {
        return;
    };
    let names = target_names(left, source);
    let direct_lambda = direct_function(node);
    for (name, name_range) in names {
        occupied.push(name_range.clone());
        if let Some(lambda) = direct_lambda {
            add_lambda_definition(node, lambda, name, root, facts);
        } else {
            facts.locals.push(LocalBinding {
                name,
                start_byte: name_range.start,
                end_byte: name_range.end,
                scope_start: root.start_byte(),
                scope_end: root.end_byte(),
                context_id: 0,
            });
        }
    }
}

fn add_lambda_definition(
    assignment: Node<'_>,
    lambda: Node<'_>,
    name: String,
    root: Node<'_>,
    facts: &mut SemanticFacts,
) {
    let start = lambda.start_byte();
    facts.definitions.push(Definition {
        name: name.clone(),
        kind: DefinitionKind::Function,
        module: module_path(),
        start_byte: start,
        end_byte: lambda.end_byte(),
        scope_start: root.start_byte(),
        scope_end: root.end_byte(),
        is_public: true,
        inline_module: false,
        external_module: false,
        context_id: 0,
    });
    facts.exports.push(Export {
        source: None,
        imported_name: None,
        local_name: Some(name.clone()),
        exported_name: name,
        namespace: false,
        start_byte: assignment.start_byte(),
        end_byte: assignment.end_byte(),
        scope_start: 0,
        scope_end: root.end_byte(),
        context_id: 0,
    });
}
