use tree_sitter::Node;

use crate::model::{Export, Import, LocalBinding};

use super::super::SemanticFacts;
use super::super::syntax::{body, range};
use super::parse::ImportedName;

pub(super) fn add_import(
    statement: Node<'_>,
    item: &ImportedName,
    root: Node<'_>,
    module_level: bool,
    facts: &mut SemanticFacts,
) {
    for scope in scopes(statement, root, module_level) {
        facts.imports.push(Import {
            path: Vec::new(),
            alias: item
                .alias
                .clone()
                .or_else(|| item.namespace.then(|| item.binding.clone())),
            source: Some(item.source.clone()),
            imported_name: item.imported_name.clone(),
            namespace: item.namespace,
            start_byte: statement.start_byte(),
            end_byte: statement.end_byte(),
            module: Default::default(),
            scope_start: scope.start,
            scope_end: scope.end,
            is_public: module_level,
            context_id: 0,
        });
    }
    if !module_level && !item.wildcard {
        add_local(statement, item, facts);
    }
}

pub(super) fn add_export(
    statement: Node<'_>,
    item: &ImportedName,
    root: Node<'_>,
    facts: &mut SemanticFacts,
) {
    facts.exports.push(Export {
        source: Some(item.source.clone()),
        imported_name: item.imported_name.clone(),
        local_name: Some(item.binding.clone()),
        exported_name: item.binding.clone(),
        namespace: item.namespace,
        start_byte: statement.start_byte(),
        end_byte: statement.end_byte(),
        scope_start: root.start_byte(),
        scope_end: root.end_byte(),
        context_id: 0,
    });
}

fn scopes(statement: Node<'_>, root: Node<'_>, module_level: bool) -> Vec<std::ops::Range<usize>> {
    if module_level {
        return vec![range(root)];
    }
    let owner = statement.parent().and_then(nearest_owner);
    match owner {
        Some(owner) if owner.kind() == "class_definition" => {
            super::super::bindings::class_segments(owner, statement.end_byte())
        }
        Some(owner) => std::iter::once(
            statement.end_byte().max(body(owner).start_byte())..body(owner).end_byte(),
        )
        .collect(),
        None => std::iter::once(statement.end_byte()..root.end_byte()).collect(),
    }
}

fn add_local(statement: Node<'_>, item: &ImportedName, facts: &mut SemanticFacts) {
    let Some(owner) = statement.parent().and_then(find_function) else {
        return;
    };
    let scope = body(owner);
    facts.locals.push(LocalBinding {
        name: item.binding.clone(),
        start_byte: statement.start_byte(),
        end_byte: statement.end_byte(),
        scope_start: scope.start_byte(),
        scope_end: scope.end_byte(),
        context_id: 0,
    });
}

fn find_function(node: Node<'_>) -> Option<Node<'_>> {
    let mut current = Some(node);
    while let Some(item) = current {
        if matches!(item.kind(), "function_definition" | "lambda") {
            return Some(item);
        }
        if item.kind() == "class_definition" {
            return None;
        }
        current = item.parent();
    }
    None
}

fn nearest_owner(node: Node<'_>) -> Option<Node<'_>> {
    let mut current = Some(node);
    while let Some(item) = current {
        if matches!(
            item.kind(),
            "function_definition" | "class_definition" | "lambda"
        ) {
            return Some(item);
        }
        current = item.parent();
    }
    None
}
