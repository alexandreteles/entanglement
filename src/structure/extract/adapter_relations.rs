use tree_sitter::Node;

pub(super) fn relation(kind: &str) -> Option<super::super::model::RelationshipKind> {
    use super::super::model::RelationshipKind;
    match kind {
        "super_interfaces" | "implements_clause" => Some(RelationshipKind::Implement),
        "superclass"
        | "base_class_clause"
        | "extends_type_clause"
        | "extends_clause"
        | "delegation_specifiers"
        | "trait_bounds"
        | "class_heritage" => Some(RelationshipKind::Inherit),
        "use_declaration" | "trait_use_clause" => Some(RelationshipKind::Compose),
        _ => None,
    }
}

fn type_node(kind: &str) -> bool {
    matches!(
        kind,
        "type_annotation"
            | "return_type"
            | "type_parameters"
            | "type_parameter_list"
            | "type_parameter_constraints"
            | "where_clause"
            | "where_predicate"
            | "generic_type"
            | "type_arguments"
            | "parameter_type"
            | "bounded_type"
            | "union_type"
            | "intersection_type"
            | "function_type"
            | "tuple_type"
    )
}

pub(super) fn is_type_position(node: Node<'_>) -> bool {
    type_node(node.kind())
        || node.parent().is_some_and(|parent| {
            ["type", "return_type", "result"]
                .into_iter()
                .filter_map(|field| parent.child_by_field_name(field))
                .any(|field| field.id() == node.id())
        })
}

pub(super) fn declared_relations(node: Node<'_>) -> Vec<super::SyntaxRelation> {
    use super::super::model::RelationshipKind;
    let mut relations = Vec::new();
    match node.kind() {
        "class_definition" => {
            if let Some(bases) = node.child_by_field_name("superclasses") {
                relations.push(super::SyntaxRelation {
                    start: bases.start_byte(),
                    end: bases.end_byte(),
                    kind: RelationshipKind::Inherit,
                    ordered: true,
                });
            }
        }
        "type_spec" => {
            if let Some(underlying) = node.child_by_field_name("type") {
                embedded(underlying, &mut relations);
            }
        }
        "type_alias_declaration" => {
            if let Some(value) = node.child_by_field_name("value") {
                alias_composition(value, &mut relations);
            }
        }
        "class_declaration" | "trait_declaration" => {
            if let Some(body) = super::capture::body_node(node) {
                let mut cursor = body.walk();
                relations.extend(
                    body.named_children(&mut cursor)
                        .filter(|child| {
                            matches!(child.kind(), "use_declaration" | "trait_use_clause")
                        })
                        .map(|child| super::SyntaxRelation {
                            start: child.start_byte(),
                            end: child.end_byte(),
                            kind: RelationshipKind::Compose,
                            ordered: false,
                        }),
                );
            }
        }
        _ => {}
    }
    relations
}

fn alias_composition(node: Node<'_>, relations: &mut Vec<super::SyntaxRelation>) {
    use super::super::model::RelationshipKind;
    match node.kind() {
        "type_identifier" | "nested_type_identifier" => relations.push(super::SyntaxRelation {
            start: node.start_byte(),
            end: node.end_byte(),
            kind: RelationshipKind::Compose,
            ordered: false,
        }),
        "generic_type" => {
            if let Some(name) = node.child_by_field_name("name") {
                alias_composition(name, relations);
            }
        }
        "intersection_type" | "parenthesized_type" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                alias_composition(child, relations);
            }
        }
        _ => {}
    }
}

fn embedded(node: Node<'_>, relations: &mut Vec<super::SyntaxRelation>) {
    use super::super::model::RelationshipKind;
    if node.kind() == "type_elem"
        || (node.kind() == "field_declaration" && node.child_by_field_name("name").is_none())
    {
        relations.push(super::SyntaxRelation {
            start: node.start_byte(),
            end: node.end_byte(),
            kind: RelationshipKind::Compose,
            ordered: false,
        });
        relations.extend(argument_relations(node));
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        embedded(child, relations);
    }
}

pub(super) fn argument_relations(node: Node<'_>) -> Vec<super::SyntaxRelation> {
    if node.kind() == "type_arguments" {
        return vec![super::SyntaxRelation {
            start: node.start_byte(),
            end: node.end_byte(),
            kind: super::super::model::RelationshipKind::Type,
            ordered: false,
        }];
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .flat_map(argument_relations)
        .collect()
}
