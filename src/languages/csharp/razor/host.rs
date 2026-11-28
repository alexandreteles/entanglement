use super::super::facts::{Binding, Data, Expression, Usage};
use crate::languages::syntax::{named_children, text};
use crate::model::{ModulePath, Reference, ReferenceKind, SemanticStore};
use crate::structure::model::RelationshipKind;
use std::ops::Range;
use tree_sitter::Node;

pub(super) fn capture(
    root: Node<'_>,
    source: &[u8],
    namespace: &str,
    owner: &str,
    store: &mut SemanticStore,
    data: &mut Data,
    existing: &std::collections::BTreeSet<(usize, usize)>,
) {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        pending.extend(named_children(node));
        let mut add = |range: Range<usize>, name: String, relation: RelationshipKind| {
            if existing.contains(&(range.start, range.end)) {
                return;
            }
            let kind = ReferenceKind::Type;
            store.references.push(Reference {
                path: vec![name.clone()],
                module: ModulePath::default(),
                kind,
                start_byte: range.start,
                end_byte: range.end,
                scope_start: root.start_byte(),
                scope_end: root.end_byte(),
                context_id: 0,
                call_owner: None,
                execution_owner: None,
                access: None,
            });
            data.uses.insert(
                (range.start, range.end),
                Usage {
                    attribute: false,
                    write: false,
                    range,
                    name,
                    namespace: namespace.into(),
                    owner: owner.into(),
                    receiver: Expression::Unknown,
                    arguments: Vec::new(),
                    arity: None,
                    kind,
                    relation,
                    constructor: false,
                    contract: relation == RelationshipKind::Inherit,
                },
            );
        };
        match node.kind() {
            "element" if source.get(node.start_byte()) == Some(&b'<') => {
                let start = node.start_byte() + 1;
                let name = std::str::from_utf8(&source[start..node.end_byte()])
                    .unwrap_or("")
                    .split(|c: char| c.is_whitespace() || matches!(c, '/' | '>'))
                    .next()
                    .unwrap_or("");
                if name.starts_with(char::is_uppercase) {
                    add(
                        start..start + name.len(),
                        name.into(),
                        RelationshipKind::Compose,
                    );
                }
            }
            "razor_model_directive"
            | "razor_inject_directive"
            | "razor_implements_directive"
            | "razor_inherits_directive" => {
                let (ty, binding) = if node.kind() == "razor_inject_directive" {
                    let Some(variable) = node.named_child(0) else {
                        continue;
                    };
                    let Some(ty) = variable.child_by_field_name("type") else {
                        continue;
                    };
                    let name = named_children(variable)
                        .find(|n| n.kind() == "variable_declarator")
                        .and_then(|n| n.child_by_field_name("name"))
                        .map(|n| text(n, source));
                    (ty, name)
                } else {
                    let Some(ty) = node.child_by_field_name("name") else {
                        continue;
                    };
                    (
                        ty,
                        (node.kind() == "razor_model_directive").then(|| "Model".to_owned()),
                    )
                };
                let relation = if binding.is_some() {
                    RelationshipKind::Type
                } else {
                    RelationshipKind::Inherit
                };
                let ty_name = text(ty, source);
                add(ty.byte_range(), ty_name.clone(), relation);
                if let Some(name) = binding {
                    data.bindings.push(Binding {
                        name,
                        scope: root.byte_range(),
                        position: root.start_byte(),
                        value: Expression::Type(ty_name),
                    });
                }
            }
            _ => {}
        }
    }
    store.references.sort_by_key(|r| (r.start_byte, r.end_byte));
}
