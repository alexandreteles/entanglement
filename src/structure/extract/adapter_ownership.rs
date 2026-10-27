use tree_sitter::Node;

pub(super) fn member_owner(node: Node<'_>, source: &[u8], owners: &[String]) -> Vec<String> {
    let nearest =
        std::iter::successors(node.parent(), |ancestor| ancestor.parent()).find(|ancestor| {
            super::adapters::declaration(ancestor.kind()).is_some()
                || ancestor.kind() == "impl_item"
        });
    if nearest.is_some_and(|ancestor| {
        super::adapters::declaration(ancestor.kind()).is_some_and(|(kind, _)| kind == "member")
    }) {
        return Vec::new();
    }
    node.child_by_field_name("receiver")
        .and_then(type_leaf)
        .map_or_else(
            || owners.to_vec(),
            |receiver_type| vec![super::capture::text(receiver_type, source)],
        )
}

pub(super) fn receiver(node: Node<'_>, source: &[u8]) -> Option<String> {
    if node.kind() == "field_declaration" && node.child_by_field_name("name").is_none() {
        let mut cursor = node.walk();
        let pointer = node
            .children(&mut cursor)
            .any(|child| child.kind() == "*" || child.kind() == "pointer_type");
        return Some(
            if pointer {
                "embed_pointer"
            } else {
                "embed_value"
            }
            .to_owned(),
        );
    }
    node.child_by_field_name("receiver").map(|receiver| {
        if super::capture::text(receiver, source).contains('*') {
            "pointer".to_owned()
        } else {
            "value".to_owned()
        }
    })
}

pub(super) fn impl_trait(node: Node<'_>, source: &[u8]) -> Option<String> {
    std::iter::successors(node.parent(), |ancestor| ancestor.parent())
        .find(|ancestor| ancestor.kind() == "impl_item")
        .and_then(|implementation| implementation.child_by_field_name("trait"))
        .map(|trait_node| super::capture::text(trait_node, source))
}

pub(super) fn type_leaf(node: Node<'_>) -> Option<Node<'_>> {
    if node.kind() == "type_identifier" {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find_map(type_leaf)
}
