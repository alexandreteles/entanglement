use tree_sitter::Node;

pub(super) fn public(
    language: &str,
    node: Node<'_>,
    source: &[u8],
    name: &str,
    existing: Option<&crate::model::Definition>,
) -> bool {
    if language.contains("python") {
        return !name.starts_with('_');
    }
    if language.contains("go") {
        return name.chars().next().is_some_and(char::is_uppercase);
    }
    let prefix = node
        .child_by_field_name("body")
        .map_or(node.end_byte(), |body| body.start_byte());
    let header = String::from_utf8_lossy(&source[node.start_byte()..prefix]);
    if header
        .split_whitespace()
        .take(4)
        .any(|word| word == "private")
        || name.starts_with('#')
    {
        return false;
    }
    if language.contains("rust") {
        let owner =
            std::iter::successors(node.parent(), |ancestor| ancestor.parent()).find(|ancestor| {
                matches!(
                    ancestor.kind(),
                    "trait_item" | "function_item" | "impl_item" | "source_file"
                )
            });
        return owner.is_some_and(|owner| owner.kind() == "trait_item")
            || existing.is_some_and(|definition| definition.is_public)
            || header.starts_with("pub");
    }
    if language.contains("php") || language.contains("kotlin") {
        return existing
            .and_then(|definition| definition.declaration.as_ref())
            .is_none_or(|declaration| {
                matches!(
                    declaration.visibility,
                    crate::graph::SemanticVisibility::Public
                        | crate::graph::SemanticVisibility::Protected
                        | crate::graph::SemanticVisibility::Internal
                )
            });
    }
    true
}

pub(super) fn required(
    node: Node<'_>,
    source: &[u8],
    kind: &str,
    owners: &[String],
    has_body: bool,
    existing: Option<&crate::model::Definition>,
) -> bool {
    let declaration = existing.and_then(|definition| definition.declaration.as_deref());
    if declaration.is_some_and(|declaration| declaration.modifiers.is_abstract) {
        return true;
    }
    let written = super::capture::text(node, source);
    if written
        .split_whitespace()
        .take(4)
        .any(|word| word == "abstract")
    {
        return true;
    }
    if optional(node) {
        return false;
    }
    match kind {
        "property" if node.kind() == "property_signature" => true,
        "member" if node.child_by_field_name("receiver").is_some() => false,
        "member" => !has_body,
        "associated" => {
            node.child_by_field_name("type").is_none()
                && node.child_by_field_name("value").is_none()
        }
        "property" if node.kind() == "const_item" => {
            !owners.is_empty() && node.child_by_field_name("value").is_none()
        }
        "property" => {
            declaration.is_some_and(|declaration| declaration.is_interface_member)
                && !has_body
                && !written.contains('=')
                && !written.contains("get(")
        }
        _ => false,
    }
}

pub(super) fn optional(node: Node<'_>) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|child| child.kind() == "?")
}
