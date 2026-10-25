use super::{SyntaxDeclaration, SyntaxRelation, adapters};
use crate::model::SemanticStore;
use crate::structure::types::TypeExpression;
use tree_sitter::{Node, Tree};

/// Add source structure to the existing semantic context before resolution.
pub(crate) fn capture(tree: &Tree, source: &[u8], language: &str, facts: &mut SemanticStore) {
    facts.semantic.structure_language = Some(language.to_owned());
    facts.semantic.structure_fingerprint = Some(super::fingerprint::syntax(
        tree.root_node(),
        None,
        source,
        &[],
    ));
    facts.semantic.structure_errors = tree.root_node().has_error();
    visit(tree.root_node(), source, &[], facts);
}

fn visit(node: Node<'_>, source: &[u8], owners: &[String], facts: &mut SemanticStore) {
    let mut child_owners = owners.to_vec();
    if let Some(record) = implementation(node, source) {
        child_owners = record.owner.clone();
        facts.semantic.structure.push(record);
    }
    if let Some(record) = declaration(node, source, owners, facts) {
        if record.is_type || record.kind == "member" {
            if owners.is_empty() && !record.owner.is_empty() {
                child_owners = record.owner.clone();
            }
            child_owners.push(record.name.clone());
        }
        store_declaration(record, node, source, facts);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit(child, source, &child_owners, facts);
    }
}

fn store_declaration(
    record: SyntaxDeclaration,
    node: Node<'_>,
    source: &[u8],
    facts: &mut SemanticStore,
) {
    let additional = adapters::additional_names(node, source)
        .into_iter()
        .map(|name| {
            let public = adapters::public(
                facts.semantic.structure_language.as_deref().unwrap_or(""),
                node,
                source,
                &name,
                None,
            );
            SyntaxDeclaration {
                name,
                public,
                ..record.clone()
            }
        })
        .collect::<Vec<_>>();
    facts.semantic.structure.push(record);
    facts.semantic.structure.extend(additional);
}

fn implementation(node: Node<'_>, source: &[u8]) -> Option<SyntaxDeclaration> {
    if node.kind() != "impl_item" {
        return None;
    }
    let target = node.child_by_field_name("type")?;
    let target_name = text(target, source)
        .split('<')
        .next()
        .unwrap_or("")
        .trim()
        .to_owned();
    let mut relations: Vec<SyntaxRelation> = node
        .child_by_field_name("trait")
        .map(|trait_node| SyntaxRelation {
            start: trait_node.start_byte(),
            end: trait_node.end_byte(),
            kind: super::super::model::RelationshipKind::Implement,
            ordered: false,
        })
        .into_iter()
        .collect();
    relations.push(SyntaxRelation {
        start: target.start_byte(),
        end: target.end_byte(),
        kind: super::super::model::RelationshipKind::Type,
        ordered: false,
    });
    Some(SyntaxDeclaration {
        start: node.start_byte(),
        end: node.end_byte(),
        body_start: body_node(node).map(|body| body.start_byte()),
        name: "@impl".to_owned(),
        fingerprint: None,
        owner: vec![target_name],
        kind: "impl".to_owned(),
        is_type: false,
        public: true,
        required: false,
        optional: false,
        receiver: None,
        member_kind: None,
        signature: None,
        impl_trait: node
            .child_by_field_name("trait")
            .map(|trait_node| text(trait_node, source)),
        types: Vec::new(),
        type_ranges: Vec::new(),
        relations,
        alias: None,
    })
}

fn declaration(
    node: Node<'_>,
    source: &[u8],
    owners: &[String],
    facts: &SemanticStore,
) -> Option<SyntaxDeclaration> {
    if matches!(node.kind(), "property_declaration" | "const_declaration")
        && facts.semantic.php.is_some()
    {
        return None;
    }
    let (kind, is_type) = adapters::classification(node, owners)?;
    let name_node = adapters::name(node)?;
    let name = text(name_node, source);
    let name = if facts.semantic.php.is_some() {
        name.trim_start_matches('$').to_owned()
    } else {
        name
    };
    let owner = if is_type {
        owners.to_vec()
    } else {
        adapters::member_owner(node, source, owners)
    };
    if node.kind() == "assignment" && owner.is_empty() {
        return None;
    }
    let body = body_node(node);
    let existing = facts.definitions.iter().find(|definition| {
        definition.name == name
            && node.start_byte() <= definition.start_byte
            && definition.start_byte < node.end_byte()
    });
    let public = adapters::public(
        facts.semantic.structure_language.as_deref().unwrap_or(""),
        node,
        source,
        &name,
        existing,
    );
    let required =
        !is_type && adapters::required(node, source, kind, owners, body.is_some(), existing);
    let (types, type_ranges, relations) = contract(node, source);
    let alias = (kind == "alias")
        .then(|| {
            node.child_by_field_name("type")
                .or_else(|| node.child_by_field_name("value"))
        })
        .flatten()
        .map(|node| TypeExpression::from_node(node, source));
    let range = if matches!(node.kind(), "property_element" | "const_element") {
        node.parent().unwrap_or(node)
    } else {
        node
    };
    Some(SyntaxDeclaration {
        start: range.start_byte(),
        end: range.end_byte(),
        body_start: body.map(|body| body.start_byte()),
        fingerprint: Some(super::fingerprint::syntax(
            node,
            Some(name_node),
            source,
            owners,
        )),
        name,
        owner,
        kind: kind.to_owned(),
        is_type,
        public,
        required,
        optional: adapters::optional(node),
        receiver: adapters::receiver(node, source),
        member_kind: adapters::member_kind(node, source),
        signature: super::signatures::capture(node, source),
        impl_trait: adapters::impl_trait(node, source),
        types,
        type_ranges,
        relations,
        alias,
    })
}

fn contract(
    node: Node<'_>,
    source: &[u8],
) -> (
    Vec<TypeExpression>,
    Vec<(usize, usize)>,
    Vec<SyntaxRelation>,
) {
    let mut types = adapters::constraints(node, source);
    let mut type_ranges = Vec::new();
    let mut relations = adapters::declared_relations(node);
    signature(
        node,
        node,
        source,
        &mut types,
        &mut type_ranges,
        &mut relations,
    );
    let property_type = matches!(node.kind(), "property_element" | "const_element")
        .then(|| {
            node.parent()
                .and_then(|parent| parent.child_by_field_name("type"))
        })
        .flatten();
    for field_type in property_type
        .into_iter()
        .chain(adapters::contract_fields(node))
    {
        types.push(TypeExpression::from_node(field_type, source));
        type_ranges.push((field_type.start_byte(), field_type.end_byte()));
    }
    (types, type_ranges, relations)
}

fn signature(
    node: Node<'_>,
    root: Node<'_>,
    source: &[u8],
    types: &mut Vec<TypeExpression>,
    type_ranges: &mut Vec<(usize, usize)>,
    relations: &mut Vec<SyntaxRelation>,
) {
    if node.id() != root.id()
        && (adapters::declaration(node.kind()).is_some()
            || body_node(root).is_some_and(|body| body.id() == node.id()))
    {
        return;
    }
    if let Some(kind) = adapters::relation(node.kind()) {
        relations.push(SyntaxRelation {
            start: node.start_byte(),
            end: node.end_byte(),
            kind,
            ordered: false,
        });
    }
    if matches!(node.kind(), "struct_type" | "interface_type") {
        return;
    }
    if adapters::is_type_position(node) {
        relations.extend(adapters::argument_relations(node));
        let expression = if node.kind() == "type_annotation" && node.named_child_count() == 1 {
            node.named_child(0).unwrap()
        } else {
            node
        };
        types.push(TypeExpression::from_node(expression, source));
        type_ranges.push((node.start_byte(), node.end_byte()));
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        signature(child, root, source, types, type_ranges, relations);
    }
}

pub(super) fn text(node: Node<'_>, source: &[u8]) -> String {
    String::from_utf8_lossy(&source[node.start_byte()..node.end_byte()]).into_owned()
}

pub(super) fn body_node(node: Node<'_>) -> Option<Node<'_>> {
    node.child_by_field_name("body").or_else(|| {
        let mut cursor = node.walk();
        node.named_children(&mut cursor).find(|child| {
            matches!(
                child.kind(),
                "function_body" | "class_body" | "enum_class_body" | "declaration_list"
            )
        })
    })
}
