use super::*;
use crate::languages::c_family::{declarator, semantic};
use tree_sitter::{Node, Tree};

pub(super) fn capture(tree: &Tree, source: &[u8], store: &mut SemanticStore) {
    let mut pending = vec![tree.root_node()];
    while let Some(node) = pending.pop() {
        match node.kind() {
            "class_specifier" | "struct_specifier" | "union_specifier" | "enum_specifier" => {
                aggregate(node, source, store)
            }
            "alias_declaration" | "type_definition" => alias(node, source, store),
            "function_definition" | "declaration" | "field_declaration" => {
                members(node, source, store)
            }
            _ => {}
        }
        let mut cursor = node.walk();
        pending.extend(node.named_children(&mut cursor));
    }
}

fn aggregate(node: Node<'_>, source: &[u8], store: &mut SemanticStore) {
    let Some(body) = node.child_by_field_name("body") else {
        return;
    };
    let Some(name) = node.child_by_field_name("name").or_else(|| {
        node.parent()
            .filter(|parent| parent.kind() == "type_definition")
            .and_then(|parent| parent.child_by_field_name("declarator"))
    }) else {
        return;
    };
    if name.has_error() {
        return;
    }
    let mut record = record(
        node,
        name,
        node.kind().trim_end_matches("_specifier"),
        source,
    );
    record.is_type = true;
    record.body_start = Some(body.start_byte());
    let mut cursor = node.walk();
    for base in node
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "base_class_clause")
    {
        let mut children = base.walk();
        for item in base
            .named_children(&mut children)
            .filter(|child| !matches!(child.kind(), "access_specifier" | "comment"))
        {
            relation(&mut record, item, RelationshipKind::Inherit, source);
        }
    }
    store.semantic.structure.push(record);
}

fn alias(node: Node<'_>, source: &[u8], store: &mut SemanticStore) {
    let Some(name) = node.child_by_field_name("name").or_else(|| {
        node.child_by_field_name("declarator")
            .and_then(declarator::identifier)
    }) else {
        return;
    };
    let Some(ty) = node.child_by_field_name("type") else {
        return;
    };
    if ty.child_by_field_name("body").is_some()
        && ty
            .child_by_field_name("name")
            .is_none_or(|tag| capture::text(tag, source) == capture::text(name, source))
    {
        return;
    }
    let mut record = record(node, name, "alias", source);
    record.is_type = true;
    record.alias = Some(TypeExpression::from_node(ty, source));
    relation(&mut record, ty, RelationshipKind::Type, source);
    store.semantic.structure.push(record);
}

fn members(node: Node<'_>, source: &[u8], store: &mut SemanticStore) {
    let mut cursor = node.walk();
    for declaration in node.children_by_field_name("declarator", &mut cursor) {
        if declaration.has_error() {
            continue;
        }
        if let Some(mut callable) = declarator::callable(declaration, source) {
            let Some(leaf) = callable.path.pop() else {
                continue;
            };
            let mut record = record(node, declaration, "member", source);
            record.name = format!(
                "{}({}){}",
                leaf,
                callable.parameters.join(", "),
                callable.qualifiers
            );
            record.owner = callable.path;
            record.member_kind = Some("method".into());
            record.public &= !node.children(&mut node.walk()).any(|child| {
                child.kind() == "storage_class_specifier"
                    && capture::text(child, source) == "static"
                    && record.owner.is_empty()
            });
            record.body_start = node
                .child_by_field_name("body")
                .map(|body| body.start_byte());
            record.required = node
                .child_by_field_name("default_value")
                .is_some_and(|value| capture::text(value, source) == "0");
            if let Some(scope) = qualified_owner(declaration) {
                let mut implementation = record.clone();
                implementation.kind = "impl".into();
                relation(&mut implementation, scope, RelationshipKind::Type, source);
                store.semantic.structure.push(implementation);
                continue;
            }
            if let Some(ty) = node.child_by_field_name("type") {
                relation(&mut record, ty, RelationshipKind::Type, source);
            }
            if let Some(params) = parameters(declaration) {
                let mut cursor = params.walk();
                for ty in params
                    .named_children(&mut cursor)
                    .filter_map(|param| param.child_by_field_name("type"))
                {
                    relation(&mut record, ty, RelationshipKind::Type, source);
                }
            }
            store.semantic.structure.push(record);
        } else if node.kind() == "field_declaration"
            && let Some(name) = declarator::identifier(declaration)
        {
            let mut record = record(node, name, "property", source);
            record.member_kind = Some("field".into());
            if let Some(ty) = node.child_by_field_name("type") {
                relation(&mut record, ty, RelationshipKind::Field, source);
            }
            store.semantic.structure.push(record);
        }
    }
}

fn qualified_owner(mut node: Node<'_>) -> Option<Node<'_>> {
    while node.kind() != "qualified_identifier" {
        node = node.child_by_field_name("declarator")?;
    }
    node.child_by_field_name("scope")
}

fn parameters(mut node: Node<'_>) -> Option<Node<'_>> {
    while node.kind() != "function_declarator" {
        node = node.child_by_field_name("declarator")?;
    }
    node.child_by_field_name("parameters")
}

fn relation(record: &mut SyntaxDeclaration, node: Node<'_>, kind: RelationshipKind, source: &[u8]) {
    record.relations.push(SyntaxRelation {
        start: node.start_byte(),
        end: node.end_byte(),
        kind,
        ordered: false,
    });
    record.types.push(TypeExpression::from_node(node, source));
    record
        .type_ranges
        .push((node.start_byte(), node.end_byte()));
}

fn record(node: Node<'_>, name: Node<'_>, kind: &str, source: &[u8]) -> SyntaxDeclaration {
    let owner = declarator::owners(node, source);
    SyntaxDeclaration {
        start: node.start_byte(),
        end: node.end_byte(),
        body_start: None,
        name: capture::text(name, source),
        fingerprint: Some(fingerprint::syntax(node, Some(name), source, &owner)),
        owner,
        kind: kind.into(),
        is_type: false,
        public: semantic::public_member(node, source),
        required: false,
        optional: false,
        receiver: None,
        impl_trait: None,
        signature: None,
        member_kind: None,
        types: vec![],
        type_ranges: vec![],
        relations: vec![],
        alias: None,
    }
}
